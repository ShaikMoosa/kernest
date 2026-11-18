//! The Import command: reads a Glyphs, UFO or designspace source, or a compiled
//! .otf/.ttf/.woff2 font, on a background thread and opens it as a new, unsaved Kernest font.

use std::path::{Path, PathBuf};

use kernest_core::{geometry, Font, Shape};
use kernest_import::{
    apply_metrics, from_font, import, parse_afm, svg_shapes, write_glyphs, KeepSide, Placement,
    EXTENSIONS,
};
use serde::Serialize;
use tauri::State;

use crate::commands::{
    doc_mut, ensure_glyph_unlocked, lock, replace_dirty, status, AppState, CmdResult, DocStatus,
    Document, FontView,
};

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

/// Writes the open font as a Glyphs 3 `.glyphs` source. This is a copy out: the
/// working document stays a `.kernest` font, since Glyphs cannot hold everything
/// Kernest keeps (colour layers, guides, notes).
#[tauri::command]
pub async fn save_glyphs(state: State<'_, AppState>, path: String) -> CmdResult<String> {
    let mut out = PathBuf::from(path.trim());
    if out.as_os_str().is_empty() {
        return Err("Choose where to save the .glyphs file.".into());
    }
    if !out.is_absolute() {
        return Err(format!(
            "\"{}\" isn't a full file path. Use Browse…, or type a path such as C:\\Fonts\\My.glyphs.",
            out.display()
        ));
    }
    if out
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        != Some("glyphs".into())
    {
        out.set_extension("glyphs");
    }
    let font = {
        let mut guard = lock(&state)?;
        doc_mut(&mut guard)?.font.clone()
    };
    let written = out.clone();
    tauri::async_runtime::spawn_blocking(move || write_glyphs(&font, &out))
        .await
        .map_err(|e| format!("Saving stopped unexpectedly: {e}"))??;
    Ok(written.display().to_string())
}

// ---------- Import outlines from SVG (S23) ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SvgTextResult {
    /// What was left out while reading the SVG (strokes, text, images).
    warnings: Vec<String>,
    status: DocStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SvgFilesResult {
    /// Glyphs whose outlines were replaced, matched by file name.
    changed: Vec<String>,
    warnings: Vec<String>,
    status: DocStatus,
}

fn parse_placement(s: &str) -> CmdResult<Placement> {
    Placement::parse(s).ok_or_else(|| format!("Unknown SVG placement \"{s}\"."))
}

/// The cap height of the master a layer key belongs to, for fitting the SVG's height.
fn cap_height(font: &Font, glyph: &str, key: &str) -> f64 {
    font.glyph(glyph)
        .and_then(|g| g.layer(key))
        .and_then(|l| font.master(&l.master_id))
        .map(|m| m.metrics.cap_height)
        .unwrap_or(700.0)
}

/// Replaces a layer's outlines with imported shapes and gives the glyph symmetric
/// sidebearings: the advance width becomes the right edge plus the left edge.
fn place_shapes(next: &mut Font, glyph: &str, key: &str, shapes: Vec<Shape>) {
    next.glyph_mut(glyph)
        .expect("glyph exists")
        .layer_mut(key)
        .expect("layer exists")
        .shapes = shapes;
    let bounds = geometry::bounds(next, next.glyph(glyph).unwrap().layer(key).unwrap());
    if let Some(b) = bounds {
        next.glyph_mut(glyph)
            .expect("glyph exists")
            .layer_mut(key)
            .expect("layer exists")
            .width = b.max_x + b.min_x;
    }
}

/// Pastes SVG text into one glyph's layer as a single undoable step.
fn import_svg_text_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    svg: &str,
    placement: Placement,
) -> CmdResult<SvgTextResult> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    if doc.font.glyph(glyph).and_then(|g| g.layer(key)).is_none() {
        return Err(format!("\"{glyph}\" has no layer \"{key}\""));
    }
    let cap = cap_height(&doc.font, glyph, key);
    let (shapes, warnings) = svg_shapes(svg, placement, cap)?;
    let mut next = doc.font.clone();
    place_shapes(&mut next, glyph, key, shapes);
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(SvgTextResult {
        warnings,
        status: status(doc),
    })
}

#[tauri::command]
pub fn import_svg_text(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    svg: String,
    placement: String,
) -> CmdResult<SvgTextResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    import_svg_text_in(doc, &glyph, &master_id, &svg, parse_placement(&placement)?)
}

/// Imports one SVG file per glyph, matching each file's name to a glyph, as a single
/// undoable step. Unmatched, unreadable or locked files are reported and left out.
fn import_svg_files_in(
    doc: &mut Document,
    paths: &[String],
    master_id: &str,
    placement: Placement,
) -> CmdResult<SvgFilesResult> {
    let mut next = doc.font.clone();
    let mut changed: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut skipped_locked = false;
    for path in paths {
        let p = Path::new(path);
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            warnings.push(format!("\"{path}\" has no usable file name."));
            continue;
        };
        let Some(g) = next.glyph(stem) else {
            warnings.push(format!("No glyph named \"{stem}\"; the file was left out."));
            continue;
        };
        if g.locked {
            skipped_locked = true;
            continue;
        }
        if g.layer(master_id).is_none() {
            warnings.push(format!(
                "\"{stem}\" has no layer for this master; it was left out."
            ));
            continue;
        }
        let svg = match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!("\"{path}\" couldn't be read: {e}"));
                continue;
            }
        };
        let cap = cap_height(&next, stem, master_id);
        match svg_shapes(&svg, placement, cap) {
            Ok((shapes, mut w)) => {
                place_shapes(&mut next, stem, master_id, shapes);
                changed.push(stem.to_string());
                for msg in w.drain(..) {
                    warnings.push(format!("{stem}: {msg}"));
                }
            }
            Err(e) => warnings.push(format!("{stem}: {e}")),
        }
    }
    if skipped_locked {
        warnings.push("Locked glyphs were skipped.".into());
    }
    if !changed.is_empty()
        && doc
            .history
            .apply_font(&mut doc.font, next)
            .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(SvgFilesResult {
        changed,
        warnings,
        status: status(doc),
    })
}

#[tauri::command]
pub fn import_svg_files(
    state: State<AppState>,
    paths: Vec<String>,
    master_id: String,
    placement: String,
) -> CmdResult<SvgFilesResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    import_svg_files_in(doc, &paths, &master_id, parse_placement(&placement)?)
}

// ---------- Import metrics (S24) ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsMaster {
    id: String,
    name: String,
}

/// What a metrics source offers, so the dialog can show a master choice for fonts.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSourceInfo {
    /// "afm" or "font".
    kind: String,
    masters: Vec<MetricsMaster>,
    glyphs: usize,
    kerning_pairs: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsResult {
    changed: Vec<String>,
    warnings: Vec<String>,
    status: DocStatus,
}

/// The `.kernest` package folder at or above `path` (the folder itself, or a file inside it).
fn kernest_dir(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("kernest"))
        })
        .map(Path::to_path_buf)
}

/// Loads a metrics source font: a `.kernest` package, or any source/compiled font the
/// importer reads.
fn load_source_font(path: &Path) -> CmdResult<Font> {
    if let Some(dir) = kernest_dir(path) {
        return kernest_core::package::open(&dir).map_err(|e| e.to_string());
    }
    let src = resolve(path);
    Ok(import(&src)?.font)
}

fn read_afm(path: &Path) -> CmdResult<kernest_import::SourceMetrics> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("\"{}\" couldn't be read: {e}", path.display()))?;
    parse_afm(&text)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

/// Reads a metrics source without touching the open font, for the import dialog.
#[tauri::command]
pub async fn read_metrics_source(path: String) -> CmdResult<MetricsSourceInfo> {
    let p = PathBuf::from(path.trim());
    if extension(&p) == "afm" {
        let m = read_afm(&p)?;
        return Ok(MetricsSourceInfo {
            kind: "afm".into(),
            masters: Vec::new(),
            glyphs: m.widths.len(),
            kerning_pairs: m.kerning.len(),
        });
    }
    let font = tauri::async_runtime::spawn_blocking(move || load_source_font(&p))
        .await
        .map_err(|e| format!("Import stopped unexpectedly: {e}"))??;
    Ok(MetricsSourceInfo {
        kind: "font".into(),
        masters: font
            .masters
            .iter()
            .map(|m| MetricsMaster {
                id: m.id.clone(),
                name: m.name.clone(),
            })
            .collect(),
        glyphs: font.glyphs.len(),
        kerning_pairs: font.kerning.values().map(Vec::len).sum(),
    })
}

/// Applies source metrics to the open font's master as a single undoable step.
fn import_metrics_in(
    doc: &mut Document,
    src: &kernest_import::SourceMetrics,
    master_id: &str,
    keep: KeepSide,
    widths: bool,
    kerning: bool,
) -> CmdResult<MetricsResult> {
    let (next, changed, warnings) =
        apply_metrics(&doc.font, master_id, src, keep, widths, kerning)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(MetricsResult {
        changed,
        warnings,
        status: status(doc),
    })
}

#[tauri::command]
pub async fn import_metrics(
    state: State<'_, AppState>,
    path: String,
    master_id: String,
    source_master_id: String,
    keep: String,
    widths: bool,
    kerning: bool,
) -> CmdResult<MetricsResult> {
    let keep =
        KeepSide::parse(&keep).ok_or_else(|| format!("Unknown sidebearing option \"{keep}\"."))?;
    let p = PathBuf::from(path.trim());
    let src = if extension(&p) == "afm" {
        read_afm(&p)?
    } else {
        let font = tauri::async_runtime::spawn_blocking(move || load_source_font(&p))
            .await
            .map_err(|e| format!("Import stopped unexpectedly: {e}"))??;
        let sm = if source_master_id.is_empty() {
            font.masters[0].id.clone()
        } else {
            source_master_id
        };
        from_font(&font, &sm)?
    };
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    import_metrics_in(doc, &src, &master_id, keep, widths, kerning)
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

#[cfg(test)]
mod svg_tests {
    use super::*;
    use kernest_core::sample::sample_font;

    const RECT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="200"><rect x="0" y="0" width="100" height="200" fill="black"/></svg>"#;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.5
    }

    #[test]
    fn paste_svg_replaces_outlines_in_one_step() {
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        let cap = doc.font.master("m01").unwrap().metrics.cap_height;
        let r = import_svg_text_in(&mut doc, "A", "m01", RECT, Placement::FitCapHeight).unwrap();
        assert!(r.status.can_undo && r.status.dirty);
        let layer = doc.font.glyph("A").unwrap().layer("m01").unwrap();
        assert!(!layer.shapes.is_empty());
        let b = geometry::bounds(&doc.font, layer).unwrap();
        assert!(near(b.max_y - b.min_y, cap));
        assert!(near(b.min_x, 0.0) && near(b.min_y, 0.0));
        assert!(near(layer.width, b.max_x + b.min_x));
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn paste_svg_refuses_a_locked_glyph() {
        let mut doc = Document::new(sample_font(), None);
        doc.font.glyph_mut("A").unwrap().locked = true;
        assert!(import_svg_text_in(&mut doc, "A", "m01", RECT, Placement::FitCapHeight).is_err());
    }

    #[test]
    fn svg_files_match_by_name_and_warn_about_unmatched() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("A.svg");
        let nope = dir.path().join("nope.svg");
        std::fs::write(&a, RECT).unwrap();
        std::fs::write(&nope, RECT).unwrap();
        let mut doc = Document::new(sample_font(), None);
        let paths = vec![a.display().to_string(), nope.display().to_string()];
        let r = import_svg_files_in(&mut doc, &paths, "m01", Placement::FitCapHeight).unwrap();
        assert_eq!(r.changed, vec!["A".to_string()]);
        assert!(r
            .warnings
            .iter()
            .any(|w| w.contains("No glyph named \"nope\"")));
        assert!(r.status.can_undo);
    }

    #[test]
    fn svg_files_batch_is_one_undo_step() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("A.svg");
        let h = dir.path().join("H.svg");
        std::fs::write(&a, RECT).unwrap();
        std::fs::write(&h, RECT).unwrap();
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        let paths = vec![a.display().to_string(), h.display().to_string()];
        let r = import_svg_files_in(&mut doc, &paths, "m01", Placement::KeepSizeAtOrigin).unwrap();
        assert_eq!(r.changed.len(), 2);
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }
}

#[cfg(test)]
mod metrics_tests {
    use super::*;
    use kernest_core::sample::sample_font;

    #[test]
    fn kernest_dir_finds_the_package_at_or_above_a_file() {
        let dir = Path::new(r"C:\fonts\My.kernest");
        assert_eq!(kernest_dir(dir), Some(dir.to_path_buf()));
        let inside = Path::new(r"C:\fonts\My.kernest\font.json");
        assert_eq!(kernest_dir(inside), Some(dir.to_path_buf()));
        assert_eq!(kernest_dir(Path::new(r"C:\fonts\My.glyphs")), None);
    }

    #[test]
    fn import_metrics_from_afm_is_one_undo_step() {
        let afm = "StartCharMetrics 1\nC 72 ; WX 720 ; N H ; B 80 0 640 700 ;\nEndCharMetrics\n";
        let src = parse_afm(afm).unwrap();
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        let r = import_metrics_in(&mut doc, &src, "m01", KeepSide::Left, true, true).unwrap();
        assert_eq!(r.changed, vec!["H".to_string()]);
        assert!(r.status.can_undo && r.status.dirty);
        assert_eq!(
            doc.font.glyph("H").unwrap().layer("m01").unwrap().width,
            720.0
        );
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }
}
