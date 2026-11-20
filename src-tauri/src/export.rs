//! The Export command: compiles the open font with fontc on a background thread.

use std::path::PathBuf;

use kernest_export::{export, ExportOptions, ExportReport, UfoExportOptions, UfoExportReport};
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
    autohint: Option<bool>,
    production_names: Option<bool>,
    instances: Option<Vec<String>>,
) -> CmdResult<ExportReport> {
    if !variable && !statics {
        return Err("Choose at least one kind of font to export.".into());
    }
    let out_dir = PathBuf::from(out_dir.trim());
    if out_dir.as_os_str().is_empty() {
        return Err("Choose a folder to export to.".into());
    }
    // A relative path would land wherever the app happens to run from.
    if !out_dir.is_absolute() {
        return Err(format!(
            "\"{}\" isn't a full folder path. Use Browse…, or type a path such as C:\\Fonts.",
            out_dir.display()
        ));
    }
    let (font, assets_dir) = {
        let mut guard = lock(&state)?;
        let doc = doc_mut(&mut guard)?;
        let dir = doc
            .path
            .as_deref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());
        (doc.font.clone(), dir)
    };
    let font = kernest_core::presets::select_instances(&font, &instances.unwrap_or_default())?;
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
                autohint: autohint.unwrap_or(false),
                production_names: production_names.unwrap_or(false),
                out_dir,
                assets_dir,
            },
        )
    })
    .await
    .map_err(|e| format!("Export stopped unexpectedly: {e}"))?
}

#[tauri::command]
pub async fn export_preset(
    state: State<'_, AppState>,
    name: String,
    out_dir: Option<String>,
) -> CmdResult<ExportReport> {
    let out = out_dir
        .filter(|p| !p.trim().is_empty())
        .map(|p| PathBuf::from(p.trim()));
    if out.as_ref().is_some_and(|p| !p.is_absolute()) {
        return Err("Choose a full folder path for the export.".into());
    }
    let (font, source_dir) = {
        let mut guard = lock(&state)?;
        let doc = doc_mut(&mut guard)?;
        let source_dir = doc
            .path
            .as_deref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| out.clone())
            .ok_or("Choose a folder to export to.")?;
        (doc.font.clone(), source_dir)
    };
    tauri::async_runtime::spawn_blocking(move || {
        kernest_export::export_preset(&font, &name, &source_dir, out.as_deref())
    })
    .await
    .map_err(|e| format!("Export stopped unexpectedly: {e}"))?
}

/// Writes editable UFO sources and a designspace into `out_dir`, honouring the
/// user's master selection, smart-component and colour-layer choices.
#[tauri::command]
pub async fn export_ufo(
    state: State<'_, AppState>,
    out_dir: String,
    masters: Option<Vec<String>>,
    decompose_smart: Option<bool>,
    color_layers: Option<bool>,
) -> CmdResult<UfoExportReport> {
    let out_dir = PathBuf::from(out_dir.trim());
    if out_dir.as_os_str().is_empty() {
        return Err("Choose a folder to export to.".into());
    }
    if !out_dir.is_absolute() {
        return Err(format!(
            "\"{}\" isn't a full folder path. Use Browse…, or type a path such as C:\\Fonts.",
            out_dir.display()
        ));
    }
    let font = {
        let mut guard = lock(&state)?;
        doc_mut(&mut guard)?.font.clone()
    };
    let opts = UfoExportOptions {
        masters: masters.unwrap_or_default(),
        decompose_smart: decompose_smart.unwrap_or(false),
        color_layers: color_layers.unwrap_or(false),
    };
    tauri::async_runtime::spawn_blocking(move || kernest_export::export_ufo(&font, &out_dir, &opts))
        .await
        .map_err(|e| format!("Export stopped unexpectedly: {e}"))?
}
