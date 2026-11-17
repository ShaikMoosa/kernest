//! App settings that live outside the font: which plug-ins are enabled, custom
//! keyboard shortcuts, and an optional Python path. Stored as JSON in the
//! per-user Kernest folder so they survive restarts but never enter a `.kernest`
//! file or a compiled font.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::CmdResult;

/// `%APPDATA%\Kernest` on Windows; a dot-folder in the home directory elsewhere.
pub fn kernest_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("Kernest");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kernest");
    }
    PathBuf::from(".kernest")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Plug-in name → enabled. A plug-in absent from the map is enabled.
    pub plugin_enabled: BTreeMap<String, bool>,
    /// Action id → shortcut text (e.g. "Ctrl+Shift+T"). Empty means the default.
    pub shortcuts: BTreeMap<String, String>,
    /// Overrides the bundled Python when set.
    pub python_path: Option<String>,
}

impl Settings {
    pub fn plugin_enabled(&self, name: &str) -> bool {
        self.plugin_enabled.get(name).copied().unwrap_or(true)
    }
}

pub struct SettingsState(pub Mutex<Settings>);

impl Default for SettingsState {
    fn default() -> Self {
        Self(Mutex::new(load_from(&settings_path())))
    }
}

pub fn settings_path() -> PathBuf {
    kernest_dir().join("settings.json")
}

pub fn load_from(path: &std::path::Path) -> Settings {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save_to(path: &std::path::Path, settings: &Settings) -> CmdResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create settings folder: {e}"))?;
    }
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("Could not encode settings: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("Could not save settings: {e}"))
}

fn persist(state: &SettingsState, settings: Settings) -> CmdResult<Settings> {
    save_to(&settings_path(), &settings)?;
    *state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")? = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub fn get_settings(state: State<SettingsState>) -> CmdResult<Settings> {
    Ok(state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")?
        .clone())
}

#[tauri::command]
pub fn set_plugin_enabled(
    state: State<SettingsState>,
    name: String,
    enabled: bool,
) -> CmdResult<Settings> {
    let mut s = state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")?
        .clone();
    s.plugin_enabled.insert(name, enabled);
    persist(&state, s)
}

#[tauri::command]
pub fn set_shortcut(
    state: State<SettingsState>,
    action: String,
    shortcut: String,
) -> CmdResult<Settings> {
    let mut s = state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")?
        .clone();
    let trimmed = shortcut.trim().to_string();
    if trimmed.is_empty() {
        s.shortcuts.remove(&action);
    } else {
        s.shortcuts.insert(action, trimmed);
    }
    persist(&state, s)
}

#[tauri::command]
pub fn reset_shortcuts(state: State<SettingsState>) -> CmdResult<Settings> {
    let mut s = state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")?
        .clone();
    s.shortcuts.clear();
    persist(&state, s)
}

#[tauri::command]
pub fn set_python_path(state: State<SettingsState>, path: Option<String>) -> CmdResult<Settings> {
    let mut s = state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")?
        .clone();
    s.python_path = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    persist(&state, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("kernest-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        s.plugin_enabled.insert("Round Widths".into(), false);
        s.shortcuts.insert("tidy-up".into(), "Ctrl+Alt+T".into());
        s.python_path = Some("C:/Python/python.exe".into());
        save_to(&path, &s).unwrap();
        let back = load_from(&path);
        assert_eq!(back, s);
        assert!(!back.plugin_enabled("Round Widths"));
        assert!(back.plugin_enabled("Unknown"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_default() {
        let s = load_from(std::path::Path::new("Z:/definitely/not/here.json"));
        assert_eq!(s, Settings::default());
        assert!(s.plugin_enabled("anything"));
    }
}
