//! Runs Python scripts and plug-ins out of process.
//!
//! Kernest spawns the bundled `python.exe` with the `kernest` package on its
//! path, writes one JSON request to stdin, and reads newline-delimited JSON
//! messages from stdout. A separate thread forwards the child's stderr too, so
//! interpreter-level errors reach the console. The run loop honours a timeout
//! and a cancel token; either kills the child, so a slow or hostile script can
//! never hang the editor.

pub mod protocol;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

pub use protocol::{DrawItem, Kind, Message, OutputLine, Request, RunOutcome, Selection};

/// Where to find Python and the `kernest` package, and how long a run may take.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub python: PathBuf,
    pub package_dir: PathBuf,
    pub timeout: Duration,
}

impl RunnerConfig {
    pub fn new(python: impl Into<PathBuf>, package_dir: impl Into<PathBuf>) -> Self {
        Self {
            python: python.into(),
            package_dir: package_dir.into(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// Locates a Python interpreter for development and tests: `KERNEST_PYTHON`
/// first, then `python`, `python3` or `py` on PATH. Returns `None` if absent, so
/// callers can skip process tests gracefully.
pub fn find_python() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KERNEST_PYTHON") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    for name in ["python", "python3", "py"] {
        if let Ok(path) = which(name) {
            return Some(path);
        }
    }
    None
}

fn which(name: &str) -> Result<PathBuf, ()> {
    let path = std::env::var_os("PATH").ok_or(())?;
    let exts = if cfg!(windows) {
        vec![".exe", ".cmd", ".bat", ""]
    } else {
        vec![""]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(())
}

fn spawn(config: &RunnerConfig) -> Result<Child, String> {
    let mut cmd = Command::new(&config.python);
    cmd.arg("-u")
        .arg("-m")
        .arg("kernest")
        .env("PYTHONPATH", &config.package_dir)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.spawn()
        .map_err(|e| format!("Could not start Python ({}): {e}", config.python.display()))
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs a request to completion, streaming each output line through `on_output`.
/// Returns the outcome (changed font, draw items, error). The cancel token lets
/// another thread stop the run; a timeout kills the child as well.
pub fn run(
    config: &RunnerConfig,
    request: &Request,
    cancel: Arc<AtomicBool>,
    mut on_output: impl FnMut(OutputLine),
) -> Result<RunOutcome, String> {
    let mut child = spawn(config)?;

    {
        let mut stdin = child.stdin.take().ok_or("Could not open script stdin")?;
        let line =
            serde_json::to_string(request).map_err(|e| format!("Could not encode request: {e}"))?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("Could not send request to Python: {e}"))?;
    }

    let (tx, rx) = mpsc::channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let msg = Message::Output {
                    stream: "stderr".into(),
                    text: line,
                };
                if let Ok(json) = serde_json::to_string(&msg) {
                    if tx.send(json).is_err() {
                        break;
                    }
                }
            }
        });
    }
    drop(tx);

    let deadline = Instant::now() + config.timeout;
    let mut outcome = RunOutcome::default();
    let mut got_result = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            kill(&mut child);
            return Err("Script cancelled.".into());
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                match protocol::parse_message(&line)? {
                    Message::Output { stream, text } => on_output(OutputLine { stream, text }),
                    Message::Result {
                        ok,
                        font,
                        draw,
                        error,
                    } => {
                        outcome.font = font.map(|f| *f);
                        outcome.draw = draw;
                        outcome.error = if ok {
                            None
                        } else {
                            Some(error.unwrap_or_else(|| "The script failed.".into()))
                        };
                        got_result = true;
                        break;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    kill(&mut child);
                    return Err(format!(
                        "Script timed out after {} seconds.",
                        config.timeout.as_secs()
                    ));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.wait();
    if !got_result {
        return Err("The script ended without returning a result.".into());
    }
    Ok(outcome)
}

/// Convenience for callers without a cancel token (tests, one-shot runs).
pub fn run_blocking(config: &RunnerConfig, request: &Request) -> Result<RunOutcome, String> {
    run(config, request, Arc::new(AtomicBool::new(false)), |_| {})
}

/// The directory holding the bundled `kernest` package, relative to a resource
/// root. Kept here so the shell and tests agree on the layout.
pub fn package_dir(resource_root: &Path) -> PathBuf {
    resource_root.join("python")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;

    fn config() -> Option<RunnerConfig> {
        let python = find_python()?;
        let pkg = Path::new(env!("CARGO_MANIFEST_DIR")).join("python");
        Some(RunnerConfig::new(python, pkg))
    }

    #[test]
    fn round_trip_moves_a_glyph() {
        let Some(cfg) = config() else {
            eprintln!("skipping: no Python");
            return;
        };
        let font = sample_font();
        let mut req = Request::new(Kind::Macro, font.clone(), Selection::default());
        req.code = Some("font.glyphs['A'].layers['m01'].width = 999".into());
        let out = run_blocking(&cfg, &req).expect("run");
        assert!(out.ok(), "{:?}", out.error);
        let changed = out.font.expect("font returned");
        assert_eq!(
            changed.glyph("A").unwrap().layer("m01").unwrap().width,
            999.0
        );
        assert_ne!(changed, font);
    }

    #[test]
    fn invalid_result_is_refused() {
        let Some(cfg) = config() else {
            eprintln!("skipping: no Python");
            return;
        };
        let mut req = Request::new(Kind::Macro, sample_font(), Selection::default());
        // Removing every master makes Font::validate fail on apply.
        req.code = Some("font.masters.clear()".into());
        let out = run_blocking(&cfg, &req).expect("run");
        let bad = out.font.expect("font returned");
        assert!(bad.validate().is_err());
    }

    #[test]
    fn print_streams_to_output() {
        let Some(cfg) = config() else {
            eprintln!("skipping: no Python");
            return;
        };
        let mut req = Request::new(Kind::Macro, sample_font(), Selection::default());
        req.code = Some("print('hello from python')".into());
        let mut lines = Vec::new();
        let out = run(&cfg, &req, Arc::new(AtomicBool::new(false)), |l| {
            lines.push(l)
        })
        .expect("run");
        assert!(out.ok());
        assert!(
            lines.iter().any(|l| l.text.contains("hello from python")),
            "{lines:?}"
        );
    }

    #[test]
    fn timeout_kills_a_slow_script() {
        let Some(mut cfg) = config() else {
            eprintln!("skipping: no Python");
            return;
        };
        cfg.timeout = Duration::from_millis(400);
        let mut req = Request::new(Kind::Macro, sample_font(), Selection::default());
        req.code = Some("import time; time.sleep(10)".into());
        let err = run_blocking(&cfg, &req).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
    }

    #[test]
    fn cancel_stops_a_run() {
        let Some(cfg) = config() else {
            eprintln!("skipping: no Python");
            return;
        };
        let mut req = Request::new(Kind::Macro, sample_font(), Selection::default());
        req.code = Some("import time; time.sleep(10)".into());
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            c2.store(true, Ordering::Relaxed);
        });
        let err = run(&cfg, &req, cancel, |_| {}).unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
    }
}
