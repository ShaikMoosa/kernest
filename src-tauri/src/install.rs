//! Trying fonts in other apps (S65): Test Install, installing for the current user,
//! quick export destinations and the web preview.
//!
//! Test Install builds the static fonts into a temporary folder and loads them for
//! this Windows session only (they disappear when Kernest quits or Windows restarts),
//! so apps that cache fonts by file never see a stale copy. Install for Me copies the
//! fonts into the per-user Fonts folder, where they stay; no admin rights needed.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kernest_export::{export, ExportOptions, ExportReport};
use serde::Serialize;
use tauri::State;

use crate::commands::{doc_mut, lock, AppState, CmdResult};

/// Fonts loaded by Test Install in this session.
static TEST_FONTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Graphics::Gdi::{AddFontResourceW, RemoveFontResourceW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_FONTCHANGE,
    };

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn add(path: &Path) -> bool {
        let w = wide(path.as_os_str());
        unsafe { AddFontResourceW(w.as_ptr()) > 0 }
    }

    pub fn remove(path: &Path) {
        let w = wide(path.as_os_str());
        // A font added more than once needs as many removals.
        unsafe { while RemoveFontResourceW(w.as_ptr()) != 0 {} }
    }

    /// Tells running apps that the font list changed.
    pub fn broadcast() {
        unsafe {
            let mut result = 0usize;
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_FONTCHANGE,
                0,
                0,
                SMTO_ABORTIFHUNG,
                1000,
                &mut result,
            );
        }
    }

    /// Registers a per-user font so it loads at every sign-in.
    pub fn register_user_font(name: &str, path: &Path) -> Result<(), String> {
        use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
        let key: Vec<u16> = "Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let value: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let data = wide(path.as_os_str());
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("Windows didn't record the font (error {status})."))
        }
    }
}

#[cfg(not(windows))]
mod win {
    use std::path::Path;
    pub fn add(_: &Path) -> bool {
        false
    }
    pub fn remove(_: &Path) {}
    pub fn broadcast() {}
    pub fn register_user_font(_: &str, _: &Path) -> Result<(), String> {
        Err("Installing fonts works on Windows only.".into())
    }
}

/// Unloads every Test Install font and deletes its files (on quit, too).
pub fn remove_test_fonts() {
    let mut list = TEST_FONTS.lock().unwrap_or_else(|e| e.into_inner());
    if list.is_empty() {
        return;
    }
    for p in list.iter() {
        win::remove(p);
    }
    win::broadcast();
    for p in list.drain(..) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    /// The fonts now available in other apps.
    installed: Vec<String>,
    report: ExportReport,
}

fn current_font(state: &AppState) -> CmdResult<(kernest_core::Font, Option<PathBuf>)> {
    let mut guard = lock(state)?;
    let doc = doc_mut(&mut guard)?;
    let dir = doc
        .path
        .as_deref()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf);
    Ok((doc.font.clone(), dir))
}

fn options(out_dir: PathBuf, assets_dir: Option<PathBuf>) -> ExportOptions {
    let mut o = ExportOptions::ttf(false, true, out_dir);
    o.remove_overlaps = true;
    o.assets_dir = assets_dir;
    o
}

/// Builds the static TTFs and loads them for this session only, replacing any fonts
/// an earlier Test Install loaded.
#[tauri::command]
pub async fn test_install(
    state: State<'_, AppState>,
    instances: Option<Vec<String>>,
) -> CmdResult<InstallReport> {
    let (font, assets) = current_font(&state)?;
    let font = kernest_core::presets::select_instances(&font, &instances.unwrap_or_default())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = std::env::temp_dir()
        .join("Kernest Test Install")
        .join(stamp.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        let report = export(&font, &options(dir, assets))?;
        remove_test_fonts();
        let mut installed = Vec::new();
        let mut list = TEST_FONTS.lock().unwrap_or_else(|e| e.into_inner());
        for f in &report.files {
            let p = PathBuf::from(&f.path);
            if win::add(&p) {
                list.push(p);
                installed.push(f.style.clone());
            }
        }
        drop(list);
        win::broadcast();
        if installed.is_empty() && !report.files.is_empty() {
            return Err("Windows didn't load the fonts.".to_string());
        }
        Ok(InstallReport { installed, report })
    })
    .await
    .map_err(|e| format!("Test Install stopped unexpectedly: {e}"))?
}

/// Unloads the Test Install fonts now.
#[tauri::command]
pub fn test_uninstall() -> usize {
    let n = TEST_FONTS.lock().map(|l| l.len()).unwrap_or(0);
    remove_test_fonts();
    n
}

/// The per-user Fonts folder (`%LOCALAPPDATA%\Microsoft\Windows\Fonts`).
pub fn user_fonts_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| {
        PathBuf::from(d)
            .join("Microsoft")
            .join("Windows")
            .join("Fonts")
    })
}

/// Installs the static fonts for the current user: into the per-user Fonts folder,
/// registered so they load at every sign-in, and loaded now.
#[tauri::command]
pub async fn install_for_user(
    state: State<'_, AppState>,
    instances: Option<Vec<String>>,
) -> CmdResult<InstallReport> {
    let (font, assets) = current_font(&state)?;
    let font = kernest_core::presets::select_instances(&font, &instances.unwrap_or_default())?;
    let fonts_dir = user_fonts_dir().ok_or("Windows didn't say where your fonts folder is.")?;
    let family = font.info.family_name.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Build elsewhere first: an installed font file is locked while loaded.
        let tmp = tempfile_dir()?;
        let report = export(&font, &options(tmp.clone(), assets))?;
        std::fs::create_dir_all(&fonts_dir)
            .map_err(|e| format!("Couldn't create {}: {e}", fonts_dir.display()))?;
        let mut installed = Vec::new();
        for f in &report.files {
            let src = PathBuf::from(&f.path);
            let Some(name) = src.file_name() else { continue };
            let dest = fonts_dir.join(name);
            win::remove(&dest);
            std::fs::copy(&src, &dest).map_err(|e| {
                format!(
                    "Couldn't copy {} into your fonts folder ({e}). Close apps that use it and try again.",
                    name.to_string_lossy()
                )
            })?;
            win::register_user_font(&format!("{family} {} (TrueType)", f.style), &dest)?;
            win::add(&dest);
            installed.push(f.style.clone());
        }
        let _ = std::fs::remove_dir_all(&tmp);
        win::broadcast();
        Ok(InstallReport { installed, report })
    })
    .await
    .map_err(|e| format!("Install stopped unexpectedly: {e}"))?
}

fn tempfile_dir() -> Result<PathBuf, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("kernest-install-{stamp}"));
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Couldn't create a temporary folder: {e}"))?;
    Ok(dir)
}

/// A quick export destination.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Destination {
    /// "user-fonts", "adobe" or "web-preview".
    id: &'static str,
    path: String,
    /// Whether the folder is there now.
    exists: bool,
}

/// The web preview folder, where its page and fonts live.
pub fn web_preview_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Kernest").join("Web Preview"))
}

/// Quick destinations for the Export dialog.
#[tauri::command]
pub fn export_destinations() -> Vec<Destination> {
    let mut out = Vec::new();
    if let Some(p) = user_fonts_dir() {
        out.push(Destination {
            id: "user-fonts",
            exists: p.is_dir(),
            path: p.display().to_string(),
        });
    }
    let common = std::env::var_os("CommonProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Program Files\\Common Files"));
    let adobe = common.join("Adobe").join("Fonts");
    out.push(Destination {
        id: "adobe",
        exists: adobe.is_dir(),
        path: adobe.display().to_string(),
    });
    if let Some(p) = web_preview_dir() {
        out.push(Destination {
            id: "web-preview",
            exists: p.is_dir(),
            path: p.join("fonts").display().to_string(),
        });
    }
    out
}

// --- Web preview ------------------------------------------------------------------------

/// The port the web preview listens on (localhost only).
pub const PREVIEW_PORT: u16 = 4123;

static SERVER: Mutex<bool> = Mutex::new(false);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PreviewState {
    version: u64,
    family: String,
    fonts: Vec<PreviewFont>,
    sample: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PreviewFont {
    file: String,
    style: String,
    variable: bool,
    /// For variable fonts: named instances as CSS font-variation-settings.
    instances: Vec<(String, String)>,
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

/// Exports WOFF2 fonts into the web preview folder, starts the local server and
/// returns the page address. Pages already open reload the fonts by themselves.
#[tauri::command]
pub async fn web_preview(state: State<'_, AppState>) -> CmdResult<String> {
    let (font, assets) = current_font(&state)?;
    let dir = web_preview_dir().ok_or("Windows didn't say where app data goes.")?;
    let fonts_dir = dir.join("fonts");
    tauri::async_runtime::spawn_blocking(move || {
        let _ = std::fs::remove_dir_all(&fonts_dir);
        let variable = font.masters.len() > 1 && !font.axes.is_empty();
        let mut o = ExportOptions::ttf(variable, !variable, fonts_dir.clone());
        o.ttf = false;
        o.woff2 = true;
        o.remove_overlaps = true;
        o.assets_dir = assets;
        let report = export(&font, &o)?;
        if report.files.is_empty() {
            return Err(report
                .problems
                .first()
                .cloned()
                .unwrap_or_else(|| "Nothing was exported.".into()));
        }
        let instances: Vec<(String, String)> =
            font.instances
                .iter()
                .filter(|i| i.export)
                .map(|i| {
                    let css: Vec<String> =
                        font.axes
                            .iter()
                            .map(|a| {
                                let d = i.location.get(&a.tag).copied().unwrap_or_else(|| {
                                    kernest_core::designspace::design_bounds(a).1
                                });
                                format!(
                                    "\"{}\" {}",
                                    a.tag,
                                    kernest_core::designspace::design_to_user(a, d)
                                )
                            })
                            .collect();
                    (i.name.clone(), css.join(", "))
                })
                .collect();
        let version = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let st = PreviewState {
            version,
            family: font.info.family_name.clone(),
            fonts: report
                .files
                .iter()
                .map(|f| PreviewFont {
                    file: Path::new(&f.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    style: f.style.clone(),
                    variable: f.kind == "variable",
                    instances: if f.kind == "variable" {
                        instances.clone()
                    } else {
                        Vec::new()
                    },
                })
                .collect(),
            sample: font
                .info
                .sample_text
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Sphinx of black quartz, judge my vow.".into()),
        };
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(
            state_path(&dir),
            serde_json::to_vec(&st).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("Couldn't write the preview: {e}"))?;
        std::fs::write(dir.join("index.html"), PAGE)
            .map_err(|e| format!("Couldn't write the preview: {e}"))?;
        start_server(dir)?;
        Ok(format!("http://localhost:{PREVIEW_PORT}/"))
    })
    .await
    .map_err(|e| format!("The web preview stopped unexpectedly: {e}"))?
}

fn start_server(dir: PathBuf) -> Result<(), String> {
    let mut running = SERVER.lock().unwrap_or_else(|e| e.into_inner());
    if *running {
        return Ok(());
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", PREVIEW_PORT)).map_err(|e| {
        format!("Port {PREVIEW_PORT} is in use, so the web preview can't start ({e}).")
    })?;
    *running = true;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let dir = dir.clone();
            std::thread::spawn(move || serve(stream, &dir));
        }
    });
    Ok(())
}

fn serve(mut stream: std::net::TcpStream, dir: &Path) {
    use std::io::{Read, Write};
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    let (status, kind, body) = match route(dir, &path) {
        Some((kind, body)) => ("200 OK", kind, body),
        None => ("404 Not Found", "text/plain", b"Not found".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}

/// The file for a request path: the page, its state, or a font in `fonts/`.
fn route(dir: &Path, path: &str) -> Option<(&'static str, Vec<u8>)> {
    match path {
        "/" | "/index.html" => Some(("text/html; charset=utf-8", PAGE.as_bytes().to_vec())),
        "/state.json" => Some(("application/json", std::fs::read(state_path(dir)).ok()?)),
        _ => {
            let name = path.strip_prefix("/fonts/")?;
            // Plain file names only: nothing outside the fonts folder.
            if name.is_empty()
                || name.contains(['/', '\\', ':'])
                || name.contains("..")
                || !name.ends_with(".woff2")
            {
                return None;
            }
            Some((
                "font/woff2",
                std::fs::read(dir.join("fonts").join(name)).ok()?,
            ))
        }
    }
}

const PAGE: &str = include_str!("web_preview.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_stay_inside_the_preview_folder() {
        let dir = std::env::temp_dir().join("kernest-route-test");
        std::fs::create_dir_all(dir.join("fonts")).unwrap();
        std::fs::write(dir.join("fonts").join("A.woff2"), b"wOF2").unwrap();
        assert!(route(&dir, "/fonts/A.woff2").is_some());
        assert!(route(&dir, "/fonts/../state.json").is_none());
        assert!(route(&dir, "/fonts/..\\x.woff2").is_none());
        assert!(route(&dir, "/secret.txt").is_none());
        assert_eq!(route(&dir, "/").unwrap().0, "text/html; charset=utf-8");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
