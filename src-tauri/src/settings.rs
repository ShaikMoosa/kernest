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
    pub appearance: crate::preferences::Appearance,
    pub onboarding: crate::preferences::Onboarding,
    pub updates: crate::preferences::UpdateSettings,
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
    settings.appearance.validate()?;
    settings.onboarding.validate()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create settings folder: {e}"))?;
    }
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("Could not encode settings: {e}"))?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, json).map_err(|e| format!("Could not save settings: {e}"))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("Could not save settings: {e}"))
}

pub(crate) fn change(
    state: &SettingsState,
    edit: impl FnOnce(&mut Settings) -> CmdResult<()>,
) -> CmdResult<Settings> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "Internal error: settings lock poisoned")?;
    let mut next = guard.clone();
    edit(&mut next)?;
    save_to(&settings_path(), &next)?;
    *guard = next.clone();
    Ok(next)
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
    change(&state, |s| {
        s.plugin_enabled.insert(name, enabled);
        Ok(())
    })
}

#[tauri::command]
pub fn set_shortcut(
    state: State<SettingsState>,
    action: String,
    shortcut: String,
) -> CmdResult<Settings> {
    change(&state, |s| {
        let trimmed = shortcut.trim().to_string();
        if trimmed.is_empty() {
            s.shortcuts.remove(&action);
        } else {
            s.shortcuts.insert(action, trimmed);
        }
        Ok(())
    })
}

#[tauri::command]
pub fn reset_shortcuts(state: State<SettingsState>) -> CmdResult<Settings> {
    change(&state, |s| {
        s.shortcuts.clear();
        Ok(())
    })
}

#[tauri::command]
pub fn set_python_path(state: State<SettingsState>, path: Option<String>) -> CmdResult<Settings> {
    change(&state, |s| {
        s.python_path = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        Ok(())
    })
}

#[tauri::command]
pub fn set_appearance(
    state: State<SettingsState>,
    appearance: crate::preferences::Appearance,
) -> CmdResult<Settings> {
    change(&state, |s| {
        s.appearance = appearance;
        Ok(())
    })
}

#[tauri::command]
pub fn set_onboarding(
    state: State<SettingsState>,
    onboarding: crate::preferences::Onboarding,
) -> CmdResult<Settings> {
    change(&state, |s| {
        s.onboarding = onboarding;
        Ok(())
    })
}

#[tauri::command]
pub fn record_milestone(state: State<SettingsState>, milestone: String) -> CmdResult<Settings> {
    change(&state, |s| {
        s.onboarding.completed.insert(milestone);
        Ok(())
    })
}

#[tauri::command]
pub fn set_update_check(state: State<SettingsState>, enabled: bool) -> CmdResult<Settings> {
    change(&state, |s| {
        s.updates.enabled = enabled;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_and_onboarding_survive_settings_round_trip() {
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "appearance":{"theme":"dark","language":"ar","handleSize":"large","textWidth":20000,"colors":{"light":{"canvas":"#f0f0f0"},"dark":{"cornerNode":"#abcdef"}}},
            "onboarding":{"welcomeDone":true,"experience":"beginner","tourEnabled":true,"tourStep":2,"completed":["created","drawn"]},
            "updates":{"enabled":true,"lastChecked":1234,"latestVersion":"2.5.0"}
        })).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save_to(&path, &settings).unwrap();
        let back = serde_json::to_value(load_from(&path)).unwrap();
        assert_eq!(back["appearance"]["textWidth"], 20000);
        assert_eq!(back["appearance"]["language"], "ar");
        assert_eq!(
            back["appearance"]["colors"]["dark"]["cornerNode"],
            "#abcdef"
        );
        assert_eq!(back["onboarding"]["tourStep"], 2);
        assert_eq!(back["onboarding"]["completed"].as_array().unwrap().len(), 2);
        assert_eq!(back["updates"]["lastChecked"], 1234);
    }

    #[test]
    fn invalid_appearance_is_rejected_without_changing_saved_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        save_to(&path, &Settings::default()).unwrap();
        let original = std::fs::read(&path).unwrap();
        for appearance in [
            serde_json::json!({"textWidth":0}),
            serde_json::json!({"theme":"unknown"}),
            serde_json::json!({"handleSize":"huge"}),
            serde_json::json!({"colors":{"light":{"canvas":"url(remote)"}}}),
        ] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"appearance":appearance})).unwrap();
            assert!(save_to(&path, &settings).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn first_run_and_update_defaults_preserve_offline_behavior() {
        let value = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(value["onboarding"]["welcomeDone"], false);
        assert_eq!(value["updates"]["enabled"], false);
        assert_eq!(value["appearance"]["textWidth"], 16000);
        assert!(value["appearance"]["theme"].is_null());
        assert!(value["appearance"]["language"].is_null());
    }

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
