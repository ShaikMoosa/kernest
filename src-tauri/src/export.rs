//! The Export command: compiles the open font with fontc on a background thread.

use std::path::PathBuf;

use kernest_export::{export, ExportOptions, ExportReport};
use tauri::State;

use crate::commands::{doc_mut, lock, AppState, CmdResult};

/// Builds a variable font and/or static fonts into `out_dir`, as TTF, OTF and/or WOFF2.
/// The font is copied first so the editor stays responsive while compiling.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn export_font(
    state: State<'_, AppState>,
    variable: bool,
    statics: bool,
    out_dir: String,
    ttf: Option<bool>,
    otf: Option<bool>,
    woff2: Option<bool>,
    remove_overlaps: Option<bool>,
    apply_metrics_keys: Option<bool>,
) -> CmdResult<ExportReport> {
    if !variable && !statics {
        return Err("Choose at least one kind of font to export.".into());
    }
    let out_dir = PathBuf::from(out_dir.trim());
    if out_dir.as_os_str().is_empty() {
        return Err("Choose a folder to export to.".into());
    }
    let font = {
        let mut guard = lock(&state)?;
        doc_mut(&mut guard)?.font.clone()
    };
    tauri::async_runtime::spawn_blocking(move || {
        export(
            &font,
            &ExportOptions {
                variable,
                statics,
                ttf: ttf.unwrap_or(true),
                otf: otf.unwrap_or(false),
                woff2: woff2.unwrap_or(false),
                remove_overlaps: remove_overlaps.unwrap_or(false),
                apply_metrics_keys: apply_metrics_keys.unwrap_or(true),
                out_dir,
            },
        )
    })
    .await
    .map_err(|e| format!("Export stopped unexpectedly: {e}"))?
}
