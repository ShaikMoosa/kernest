//! Scripts and plug-ins: discovery, the out-of-process runner, and reporters.
//!
//! Scripts are `.py` files in the per-user Scripts folder (folders become
//! submenus, `# MenuTitle:` sets the name). Plug-ins live in the Plugins folder,
//! each with a `plugin.json` manifest. A run sends the font snapshot to Python,
//! streams its output to the console, and applies the changed font as one undo
//! step. Reporters only return drawing instructions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kernest_script::{DrawItem, Kind, OutputLine, Request, RunnerConfig, Selection};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::commands::{doc_mut, lock, AppState, CmdResult};
use crate::settings::{kernest_dir, SettingsState};

/// The cancel token of the script currently running, if any.
#[derive(Default)]
pub struct ScriptState(pub Mutex<Option<Arc<AtomicBool>>>);

pub fn scripts_dir() -> PathBuf {
    kernest_dir().join("Scripts")
}

pub fn plugins_dir() -> PathBuf {
    kernest_dir().join("Plugins")
}

fn manifest_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/kernest-script")
}

/// Where bundled samples and the Python package live: the crate in development,
/// the app's resources in a shipped build.
fn resource_root(app: &AppHandle) -> CmdResult<PathBuf> {
    if cfg!(debug_assertions) {
        Ok(manifest_root())
    } else {
        app.path()
            .resource_dir()
            .map_err(|e| format!("Could not locate app resources: {e}"))
    }
}

fn resolve_python(app: &AppHandle, settings: &crate::settings::Settings) -> CmdResult<PathBuf> {
    if let Some(p) = &settings.python_path {
        return Ok(PathBuf::from(p));
    }
    if cfg!(debug_assertions) {
        kernest_script::find_python().ok_or_else(|| {
            "Python was not found. Install Python or set its path in Settings › Python.".into()
        })
    } else {
        Ok(resource_root(app)?.join("python/runtime/python.exe"))
    }
}

fn runner_config(app: &AppHandle, settings: &crate::settings::Settings) -> CmdResult<RunnerConfig> {
    let package_dir = resource_root(app)?.join("python");
    let python = resolve_python(app, settings)?;
    let mut cfg = RunnerConfig::new(python, package_dir);
    cfg.timeout = std::time::Duration::from_secs(60);
    Ok(cfg)
}

/// The Python Kernest would run: its path and version (for Settings › Python).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PythonInfo {
    pub path: String,
    pub version: Option<String>,
    pub bundled: bool,
}

#[tauri::command]
pub fn python_info(app: AppHandle, settings: State<SettingsState>) -> CmdResult<PythonInfo> {
    let s = settings_of(&settings)?;
    let path = resolve_python(&app, &s)?;
    let bundled = s.python_path.is_none() && !cfg!(debug_assertions);
    let version = std::process::Command::new(&path)
        .args(["-c", "import sys; print(sys.version.split()[0])"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|v| !v.is_empty());
    Ok(PythonInfo {
        path: path.to_string_lossy().to_string(),
        version,
        bundled,
    })
}

// ---------- Discovery ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptEntry {
    pub name: String,
    pub path: String,
    /// Submenu path relative to the Scripts folder ("" at the top level).
    pub folder: String,
}

/// Reads `# MenuTitle:` from the head of a script, else uses the file stem.
fn menu_title(path: &Path) -> String {
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines().take(30) {
            if let Some(rest) = line.trim().strip_prefix("# MenuTitle:") {
                let title = rest.trim();
                if !title.is_empty() {
                    return title.to_string();
                }
            }
        }
    }
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Script".into())
}

fn walk_scripts(dir: &Path, base: &Path, out: &mut Vec<ScriptEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_scripts(&path, base, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("py") {
            let folder = path
                .parent()
                .and_then(|p| p.strip_prefix(base).ok())
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            out.push(ScriptEntry {
                name: menu_title(&path),
                path: path.to_string_lossy().to_string(),
                folder,
            });
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginParam {
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(rename = "type", default = "default_param_type")]
    pub kind: String,
    #[serde(default)]
    pub default: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

fn default_param_type() -> String {
    "number".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub entry: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub min_kernest: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: Vec<PluginParam>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    #[serde(flatten)]
    pub manifest: PluginManifest,
    pub enabled: bool,
    /// Folder name under Plugins, used as the stable id.
    pub id: String,
}

pub fn parse_manifest(json: &str) -> CmdResult<PluginManifest> {
    let m: PluginManifest =
        serde_json::from_str(json).map_err(|e| format!("Bad plugin.json: {e}"))?;
    if m.name.trim().is_empty() {
        return Err("plugin.json needs a name.".into());
    }
    if !matches!(m.kind.as_str(), "filter" | "reporter" | "general") {
        return Err(format!("Unknown plug-in type \"{}\".", m.kind));
    }
    if m.entry.trim().is_empty() {
        return Err("plugin.json needs an entry file.".into());
    }
    Ok(m)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// Copies the bundled sample scripts and plug-ins into the user folders the
/// first time, so the menus are not empty on a fresh install.
fn seed_samples(app: &AppHandle) {
    let root = resource_root(app).ok();
    let Some(root) = root else { return };
    let samples = root.join("samples");
    let scripts_from = samples.join("scripts");
    let plugins_from = samples.join("plugins");
    let scripts_to = scripts_dir();
    let plugins_to = plugins_dir();
    if scripts_from.is_dir() && !scripts_to.exists() {
        let _ = copy_dir(&scripts_from, &scripts_to);
    }
    if plugins_from.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&plugins_from) {
            for entry in entries.flatten() {
                let dest = plugins_to.join(entry.file_name());
                if entry.path().is_dir() && !dest.exists() {
                    let _ = copy_dir(&entry.path(), &dest);
                }
            }
        }
    }
}

// ---------- Running ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResult {
    pub ok: bool,
    pub output: Vec<OutputLine>,
    pub error: Option<String>,
    pub applied: bool,
    pub revision: u64,
    pub draw: Vec<DrawItem>,
}

fn selection(
    font: &kernest_core::Font,
    current: Option<String>,
    master_id: Option<String>,
    glyphs: Vec<String>,
) -> Selection {
    let master_id = master_id
        .filter(|m| !m.is_empty())
        .or_else(|| font.masters.first().map(|m| m.id.clone()))
        .unwrap_or_default();
    Selection {
        glyphs,
        current_glyph: current,
        layers: vec![master_id.clone()],
        master_id,
    }
}

/// Runs a request, streaming output to the console, and applies the changed font
/// (unless it is a reporter). Holds the cancel token so `cancel_script` can stop it.
fn execute(
    app: &AppHandle,
    state: &State<AppState>,
    script_state: &State<ScriptState>,
    config: &RunnerConfig,
    request: Request,
    apply: bool,
) -> CmdResult<ScriptResult> {
    let token = Arc::new(AtomicBool::new(false));
    {
        let mut slot = script_state
            .0
            .lock()
            .map_err(|_| "Internal error: script lock poisoned")?;
        if slot.is_some() {
            return Err("A script is already running. Cancel it first.".into());
        }
        *slot = Some(token.clone());
    }

    let outcome = kernest_script::run(config, &request, token.clone(), |line| {
        let _ = app.emit("script-output", &line);
    });

    {
        let mut slot = script_state
            .0
            .lock()
            .map_err(|_| "Internal error: script lock poisoned")?;
        *slot = None;
    }

    let outcome = outcome?;
    let ok = outcome.ok();
    let mut result = ScriptResult {
        ok,
        output: outcome.output,
        error: outcome.error.clone(),
        applied: false,
        revision: 0,
        draw: outcome.draw,
    };

    if ok && apply {
        if let Some(next) = outcome.font {
            let mut guard = lock(state)?;
            let doc = doc_mut(&mut guard)?;
            let changed = doc
                .history
                .apply_font(&mut doc.font, next)
                .map_err(|e| e.to_string())?;
            if changed {
                doc.revision += 1;
                doc.preview = None;
                result.applied = true;
            }
            result.revision = doc.revision;
        }
    }
    Ok(result)
}

fn settings_of(state: &State<SettingsState>) -> CmdResult<crate::settings::Settings> {
    Ok(state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")?
        .clone())
}

#[tauri::command]
pub fn list_scripts(app: AppHandle) -> CmdResult<Vec<ScriptEntry>> {
    seed_samples(&app);
    let dir = scripts_dir();
    let mut out = Vec::new();
    walk_scripts(&dir, &dir, &mut out);
    out.sort_by(|a, b| a.folder.cmp(&b.folder).then(a.name.cmp(&b.name)));
    Ok(out)
}

#[tauri::command]
pub fn list_plugins(app: AppHandle, settings: State<SettingsState>) -> CmdResult<Vec<PluginInfo>> {
    seed_samples(&app);
    let s = settings_of(&settings)?;
    let dir = plugins_dir();
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let manifest_path = path.join("plugin.json");
            if !manifest_path.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            match parse_manifest(&text) {
                Ok(manifest) => {
                    let id = entry.file_name().to_string_lossy().to_string();
                    let enabled = s.plugin_enabled(&manifest.name);
                    out.push(PluginInfo {
                        manifest,
                        enabled,
                        id,
                    });
                }
                Err(_) => continue,
            }
        }
    }
    out.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
    Ok(out)
}

/// Runs off the main thread so the IPC loop stays free to process `cancel_script`
/// while a script blocks; a sync command would queue the cancel behind the run.
#[tauri::command]
pub async fn run_script(
    app: AppHandle,
    code: Option<String>,
    path: Option<String>,
    current: Option<String>,
    master_id: Option<String>,
    glyphs: Vec<String>,
) -> CmdResult<ScriptResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = app.state::<SettingsState>();
        let script_state = app.state::<ScriptState>();
        let config = runner_config(&app, &settings_of(&settings)?)?;
        let (font, sel) = {
            let mut guard = lock(&state)?;
            let doc = doc_mut(&mut guard)?;
            (
                doc.font.clone(),
                selection(&doc.font, current, master_id, glyphs),
            )
        };
        let kind = if code.is_some() {
            Kind::Macro
        } else {
            Kind::Script
        };
        let mut request = Request::new(kind, font, sel);
        request.code = code;
        request.path = path;
        execute(&app, &state, &script_state, &config, request, true)
    })
    .await
    .map_err(|e| format!("Internal error: script task failed: {e}"))?
}

#[tauri::command]
pub fn cancel_script(script_state: State<ScriptState>) -> CmdResult<()> {
    if let Some(token) = script_state
        .0
        .lock()
        .map_err(|_| "Internal error: script lock poisoned")?
        .as_ref()
    {
        token.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn find_plugin(
    app: &AppHandle,
    settings: &State<SettingsState>,
    name: &str,
) -> CmdResult<(PathBuf, PluginManifest)> {
    seed_samples(app);
    let s = settings_of(settings)?;
    let dir = plugins_dir();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let manifest_path = path.join("plugin.json");
            if !manifest_path.is_file() {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = parse_manifest(&text) else {
                continue;
            };
            if manifest.name == name {
                if !s.plugin_enabled(&manifest.name) {
                    return Err(format!("The plug-in \"{}\" is disabled.", name));
                }
                let id = entry.file_name().to_string_lossy().to_string();
                let entry_path = dir.join(id).join(&manifest.entry);
                return Ok((entry_path, manifest));
            }
        }
    }
    Err(format!("No plug-in named \"{}\".", name))
}

#[tauri::command]
pub async fn run_filter_plugin(
    app: AppHandle,
    name: String,
    params: serde_json::Value,
    current: Option<String>,
    master_id: Option<String>,
    glyphs: Vec<String>,
) -> CmdResult<ScriptResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = app.state::<SettingsState>();
        let script_state = app.state::<ScriptState>();
        let (entry, _manifest) = find_plugin(&app, &settings, &name)?;
        let config = runner_config(&app, &settings_of(&settings)?)?;
        let (font, sel) = {
            let mut guard = lock(&state)?;
            let doc = doc_mut(&mut guard)?;
            (
                doc.font.clone(),
                selection(&doc.font, current, master_id, glyphs),
            )
        };
        let mut request = Request::new(Kind::Filter, font, sel);
        request.path = Some(entry.to_string_lossy().to_string());
        request.params = params;
        execute(&app, &state, &script_state, &config, request, true)
    })
    .await
    .map_err(|e| format!("Internal error: script task failed: {e}"))?
}

#[tauri::command]
pub async fn reporter_draw(
    app: AppHandle,
    name: String,
    glyph: String,
    master_id: String,
) -> CmdResult<Vec<DrawItem>> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let settings = app.state::<SettingsState>();
        let script_state = app.state::<ScriptState>();
        let (entry, _manifest) = find_plugin(&app, &settings, &name)?;
        let config = runner_config(&app, &settings_of(&settings)?)?;
        let font = {
            let mut guard = lock(&state)?;
            let doc = doc_mut(&mut guard)?;
            doc.font.clone()
        };
        let sel = Selection {
            glyphs: vec![glyph.clone()],
            current_glyph: Some(glyph),
            master_id: master_id.clone(),
            layers: vec![master_id],
        };
        let mut request = Request::new(Kind::Reporter, font, sel);
        request.path = Some(entry.to_string_lossy().to_string());
        let result = execute(&app, &state, &script_state, &config, request, false)?;
        if !result.ok {
            return Err(result
                .error
                .unwrap_or_else(|| "The reporter failed.".into()));
        }
        Ok(result.draw)
    })
    .await
    .map_err(|e| format!("Internal error: script task failed: {e}"))?
}

#[tauri::command]
pub fn open_scripts_folder(app: AppHandle) -> CmdResult<()> {
    seed_samples(&app);
    let dir = scripts_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create Scripts folder: {e}"))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("Could not open the Scripts folder: {e}"))
}

#[tauri::command]
pub fn open_plugins_folder(app: AppHandle) -> CmdResult<()> {
    seed_samples(&app);
    let dir = plugins_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create Plugins folder: {e}"))?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| format!("Could not open the Plugins folder: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_parses_and_validates() {
        let m = parse_manifest(
            r#"{"name":"Round Widths","type":"filter","entry":"filter.py","version":"1.0","minKernest":"2.0","parameters":[{"name":"step","label":"Step","type":"number","default":10,"min":1}]}"#,
        )
        .unwrap();
        assert_eq!(m.name, "Round Widths");
        assert_eq!(m.kind, "filter");
        assert_eq!(m.parameters.len(), 1);
        assert_eq!(m.parameters[0].name, "step");
        assert_eq!(m.min_kernest, "2.0");
    }

    #[test]
    fn bad_manifests_are_refused() {
        assert!(parse_manifest("{}").is_err());
        assert!(parse_manifest(r#"{"name":"X","type":"nope","entry":"x.py"}"#).is_err());
        assert!(parse_manifest(r#"{"name":"X","type":"filter"}"#).is_err());
        assert!(parse_manifest("not json").is_err());
    }

    /// Handbook 5.5.6 (Plug-ins & Scripts): scripts in subfolders become submenus,
    /// other files are ignored, and a script sees the selection and the master.
    #[test]
    fn scripts_menu_follows_folders_and_scripts_get_the_selection() {
        let dir = std::env::temp_dir().join(format!("kernest-menu-{}", std::process::id()));
        let deep = dir.join("Spacing").join("Metrics");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(dir.join("top.py"), "print(1)\n").unwrap();
        std::fs::write(deep.join("deep.py"), "# MenuTitle: Check Widths\n").unwrap();
        std::fs::write(deep.join("notes.txt"), "not a script").unwrap();
        let mut out = Vec::new();
        walk_scripts(&dir, &dir, &mut out);
        out.sort_by(|a, b| a.folder.cmp(&b.folder));
        let got: Vec<(&str, &str)> = out
            .iter()
            .map(|e| (e.folder.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(got, [("", "top"), ("Spacing/Metrics", "Check Widths")]);
        let _ = std::fs::remove_dir_all(&dir);

        let font = kernest_core::sample::sample_font();
        let names = vec!["H".to_string(), "O".to_string()];
        let s = selection(&font, Some("H".into()), None, names.clone());
        assert_eq!(s.glyphs, names);
        assert_eq!(s.current_glyph.as_deref(), Some("H"));
        assert_eq!(s.master_id, font.masters[0].id);
        assert_eq!(s.layers, [font.masters[0].id.clone()]);
        let s = selection(&font, None, Some("m09".into()), Vec::new());
        assert_eq!(s.master_id, "m09");
        assert!(s.glyphs.is_empty());
    }

    #[test]
    fn menu_title_from_comment_or_stem() {
        let dir = std::env::temp_dir().join(format!("kernest-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let named = dir.join("a.py");
        std::fs::write(&named, "# MenuTitle: My Script\nprint(1)\n").unwrap();
        assert_eq!(menu_title(&named), "My Script");
        let plain = dir.join("plain_name.py");
        std::fs::write(&plain, "print(1)\n").unwrap();
        assert_eq!(menu_title(&plain), "plain_name");
        let mut out = Vec::new();
        walk_scripts(&dir, &dir, &mut out);
        assert_eq!(out.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
