//! The Import command: reads a Glyphs, UFO or designspace source on a background thread
//! and opens it as a new, unsaved Kernest font.

use std::path::{Path, PathBuf};

use kernest_import::{import, EXTENSIONS};
use serde::Serialize;
use tauri::State;

use crate::commands::{replace_dirty, AppState, CmdResult, FontView};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportView {
    view: FontView,
    /// What was left out or changed while converting.
    warnings: Vec<String>,
    /// The file or package that was imported.
    source: String,
}

/// `.ufo` and `.glyphspackage` sources are folders; accept the folder itself or any file
/// inside it (such as `fontinfo.plist`), as picked in a file dialog.
fn resolve(path: &Path) -> PathBuf {
    let is_source = |p: &Path| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
    };
    if is_source(path) {
        return path.to_path_buf();
    }
    path.ancestors()
        .skip(1)
        .find(|p| is_source(p))
        .map(Path::to_path_buf)
        .unwrap_or_else(|| path.to_path_buf())
}

#[tauri::command]
pub async fn import_font(state: State<'_, AppState>, path: String) -> CmdResult<ImportView> {
    let source = resolve(Path::new(path.trim()));
    let src = source.clone();
    let imported = tauri::async_runtime::spawn_blocking(move || import(&src))
        .await
        .map_err(|e| format!("Import stopped unexpectedly: {e}"))??;
    let view = replace_dirty(&state, imported.font)?;
    Ok(ImportView {
        view,
        warnings: imported.warnings,
        source: source.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_files_inside_packages() {
        let p = Path::new(r"C:\fonts\My Sans.ufo\fontinfo.plist");
        assert_eq!(resolve(p), Path::new(r"C:\fonts\My Sans.ufo"));
        let p = Path::new(r"C:\fonts\My.glyphspackage\glyphs\A.glyph");
        assert_eq!(resolve(p), Path::new(r"C:\fonts\My.glyphspackage"));
        let p = Path::new(r"C:\fonts\My.glyphs");
        assert_eq!(resolve(p), p);
    }
}
