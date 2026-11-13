//! Opening fonts from outside the app: paths given on the command line (Explorer's
//! "Open in Kernest" and "Open with"), and paths sent by a second launch while Kernest
//! is already running (single instance).

use std::path::Path;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

/// Paths Kernest was started with, until the UI picks them up.
#[derive(Default)]
pub struct LaunchPaths(pub Mutex<Vec<String>>);

/// The font paths in a command line (the program itself and switches are skipped).
pub fn paths_from_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    args.into_iter()
        .skip(1)
        .filter(|a| !a.starts_with('-') && Path::new(a).exists())
        .collect()
}

/// The paths Kernest was started with (once; later calls return nothing).
#[tauri::command]
pub fn take_launch_paths(state: State<LaunchPaths>) -> Vec<String> {
    state
        .0
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default()
}

/// A second launch (for example "Open in Kernest" while Kernest is open): bring the
/// window forward and hand its paths to the running app.
pub fn on_second_instance(app: &AppHandle, argv: Vec<String>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    let paths = paths_from_args(argv);
    if !paths.is_empty() {
        let _ = app.emit("open-paths", paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_existing_paths_only() {
        let here = std::env::current_dir().unwrap().display().to_string();
        let args = vec![
            "kernest.exe".to_string(),
            "--flag".into(),
            here.clone(),
            r"C:\definitely\not\here.kernest".into(),
        ];
        assert_eq!(paths_from_args(args), vec![here]);
    }
}
