//! Commands for the Features editor and the text view's shaped feature preview.
//!
//! The preview shapes a quick TrueType compile of the current master with rustybuzz,
//! so what the text view shows is what the exported font will do. The compile is
//! cached per revision and location; the UI debounces calls while typing.

use std::collections::HashMap;

use kernest_core::designspace::Location;
use kernest_core::fea_import::merge_features;
use kernest_core::features::{available, check, generate, handwritten_tags};
use kernest_core::geometry::svg_path;
use kernest_core::interpolate::Interpolator;
use kernest_core::{Font, Layer, Metrics};
use serde::Serialize;
use tauri::State;

use crate::commands::{
    color_views, css_palette, doc_mut, lock, status, AppState, CmdResult, ColorLayerView,
    DocStatus, Document,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticView {
    line: usize,
    column: usize,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesView {
    text: String,
    generated: String,
    /// The automatic features currently switched on.
    automatic: Vec<String>,
    /// Every feature Kernest could generate from the glyph names.
    available: Vec<String>,
    /// Features the designer wrote by hand (never generated).
    handwritten: Vec<String>,
    diagnostics: Vec<DiagnosticView>,
    status: DocStatus,
}

fn view(doc: &Document) -> FeaturesView {
    let f = &doc.font;
    FeaturesView {
        text: f.features.clone(),
        generated: generate(f),
        automatic: f.feature_settings.automatic.iter().cloned().collect(),
        available: available(f).into_iter().collect(),
        handwritten: handwritten_tags(f).into_iter().collect(),
        diagnostics: check(f)
            .into_iter()
            .map(|d| DiagnosticView {
                line: d.line,
                column: d.column,
                message: d.message,
            })
            .collect(),
        status: status(doc),
    }
}

/// Applies a feature edit as one undoable step and returns the new Features view.
fn edit(
    state: &AppState,
    f: impl FnOnce(&Font) -> Result<Font, String>,
) -> CmdResult<FeaturesView> {
    let mut guard = lock(state)?;
    let doc = doc_mut(&mut guard)?;
    let next = f(&doc.font)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(view(doc))
}

#[tauri::command]
pub fn features(state: State<AppState>) -> CmdResult<FeaturesView> {
    let mut guard = lock(&state)?;
    Ok(view(doc_mut(&mut guard)?))
}

#[tauri::command]
pub fn set_features(state: State<AppState>, text: String) -> CmdResult<FeaturesView> {
    edit(&state, |f| {
        let mut next = f.clone();
        next.features = text;
        Ok(next)
    })
}

#[tauri::command]
pub fn set_automatic_features(
    state: State<AppState>,
    automatic: Vec<String>,
) -> CmdResult<FeaturesView> {
    edit(&state, |f| {
        let mut next = f.clone();
        next.feature_settings.automatic = automatic.into_iter().collect();
        Ok(next)
    })
}

// ---------- Import features from a .fea file ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureImportPreview {
    /// Tags the file adds that the font didn't have.
    added: Vec<String>,
    /// Tags the file replaces because the font already had them.
    replaced: Vec<String>,
    /// Problems the merged code would have, shown before applying.
    diagnostics: Vec<DiagnosticView>,
}

fn read_fea(path: &str) -> CmdResult<String> {
    let path = path.trim();
    std::fs::read_to_string(path).map_err(|e| format!("\"{path}\" couldn't be read: {e}"))
}

fn preview_in(doc: &Document, incoming: &str) -> CmdResult<FeatureImportPreview> {
    let merged = merge_features(&doc.font.features, incoming)?;
    let mut probe = doc.font.clone();
    probe.features = merged.text;
    Ok(FeatureImportPreview {
        added: merged.added,
        replaced: merged.replaced,
        diagnostics: check(&probe)
            .into_iter()
            .map(|d| DiagnosticView {
                line: d.line,
                column: d.column,
                message: d.message,
            })
            .collect(),
    })
}

/// Shows what a `.fea` file would add and replace, without changing the font.
#[tauri::command]
pub fn preview_feature_import(
    state: State<AppState>,
    path: String,
) -> CmdResult<FeatureImportPreview> {
    let incoming = read_fea(&path)?;
    let mut guard = lock(&state)?;
    preview_in(doc_mut(&mut guard)?, &incoming)
}

fn import_in(doc: &mut Document, incoming: &str) -> CmdResult<FeaturesView> {
    let merged = merge_features(&doc.font.features, incoming)?;
    let mut next = doc.font.clone();
    next.features = merged.text;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(view(doc))
}

/// Merges a `.fea` file into the feature text as one undoable step.
#[tauri::command]
pub fn import_features(state: State<AppState>, path: String) -> CmdResult<FeaturesView> {
    let incoming = read_fea(&path)?;
    let mut guard = lock(&state)?;
    import_in(doc_mut(&mut guard)?, &incoming)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapedGlyph {
    name: String,
    svg: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    color: Vec<ColorLayerView>,
    advance: f64,
    x_offset: f64,
    y_offset: f64,
    /// Byte offset in the line's text, for caret placement and selection.
    cluster: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapedView {
    lines: Vec<Vec<ShapedGlyph>>,
    metrics: Metrics,
    palette: Vec<String>,
    status: DocStatus,
}

/// The compiled preview font for this revision and location, compiling it if the
/// cache is stale. Returns the bytes and the glyph order they were built with.
fn preview_font(
    doc: &mut Document,
    master_id: &str,
    location: Option<&Location>,
) -> CmdResult<(Vec<u8>, Vec<String>)> {
    let cached = doc
        .preview
        .as_ref()
        .filter(|(rev, loc, _, _)| *rev == doc.revision && loc.as_ref() == location)
        .map(|(_, _, bytes, order)| (bytes.clone(), order.clone()));
    match cached {
        Some(hit) => Ok(hit),
        None => {
            let (bytes, order) = kernest_export::preview_ttf(&doc.font, master_id, location)?;
            doc.preview = Some((
                doc.revision,
                location.cloned(),
                bytes.clone(),
                order.clone(),
            ));
            Ok((bytes, order))
        }
    }
}

fn shape(
    doc: &mut Document,
    text: &str,
    master_id: &str,
    location: Option<&Location>,
    features: &[String],
) -> CmdResult<ShapedView> {
    let master = doc
        .font
        .master(master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    let metrics = master.metrics;
    let (bytes, order) = preview_font(doc, master_id, location)?;
    let face = rustybuzz::Face::from_slice(&bytes, 0)
        .ok_or_else(|| "could not read the preview font".to_string())?;
    let feats: Vec<rustybuzz::Feature> = features.iter().filter_map(|t| t.parse().ok()).collect();

    let interp = location
        .map(|l| Interpolator::new(&doc.font, l))
        .transpose()?;
    let metrics = match &interp {
        Some(i) => {
            let per: Vec<Vec<f64>> = doc
                .font
                .masters
                .iter()
                .map(|m| {
                    vec![
                        m.metrics.ascender,
                        m.metrics.cap_height,
                        m.metrics.x_height,
                        m.metrics.descender,
                        m.metrics.italic_angle,
                    ]
                })
                .collect();
            let v = i.values(&per);
            Metrics {
                ascender: v[0],
                cap_height: v[1],
                x_height: v[2],
                descender: v[3],
                italic_angle: v[4],
            }
        }
        None => metrics,
    };

    let mut drawn: HashMap<String, (String, Vec<ColorLayerView>)> = HashMap::new();
    let mut draw = |name: &str| -> (String, Vec<ColorLayerView>) {
        if let Some(hit) = drawn.get(name) {
            return hit.clone();
        }
        let layer = match &interp {
            Some(i) => i.layer(&doc.font, name).unwrap_or_else(|_| {
                doc.font
                    .glyph(name)
                    .and_then(|g| g.layer(master_id))
                    .cloned()
                    .unwrap_or_else(|| Layer::new(master_id, 0.0))
            }),
            None => doc
                .font
                .glyph(name)
                .and_then(|g| g.layer(master_id))
                .cloned()
                .unwrap_or_else(|| Layer::new(master_id, 0.0)),
        };
        let color = doc
            .font
            .glyph(name)
            .map(|g| color_views(&doc.font, g, master_id))
            .unwrap_or_default();
        let entry = (svg_path(&doc.font, &layer), color);
        drawn.insert(name.to_string(), entry.clone());
        entry
    };

    let lines = text
        .split('\n')
        .map(|line| {
            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(line);
            buffer.guess_segment_properties();
            let out = rustybuzz::shape(&face, &feats, buffer);
            out.glyph_infos()
                .iter()
                .zip(out.glyph_positions())
                .map(|(info, pos)| {
                    let name = order
                        .get(info.glyph_id as usize)
                        .cloned()
                        .unwrap_or_else(|| ".notdef".to_string());
                    let (svg, color) = draw(&name);
                    ShapedGlyph {
                        name,
                        svg,
                        color,
                        advance: f64::from(pos.x_advance),
                        x_offset: f64::from(pos.x_offset),
                        y_offset: f64::from(pos.y_offset),
                        cluster: info.cluster,
                    }
                })
                .collect()
        })
        .collect();

    Ok(ShapedView {
        lines,
        metrics,
        palette: css_palette(&doc.font),
        status: status(doc),
    })
}

/// Shapes `text` with the given feature settings (e.g. "ss01=1", "liga=0") against a
/// compiled preview of the current master, for the text view's feature preview.
#[tauri::command]
pub fn shape_text(
    state: State<AppState>,
    text: String,
    master_id: String,
    location: Option<Location>,
    features: Vec<String>,
) -> CmdResult<ShapedView> {
    let mut guard = lock(&state)?;
    shape(
        doc_mut(&mut guard)?,
        &text,
        &master_id,
        location.as_ref(),
        &features,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;
    use kernest_core::Glyph;

    fn doc_with_alternates() -> Document {
        let mut f = sample_font();
        let mid = f.masters[0].id.clone();
        for (n, base) in [("a.ss01", "a"), ("f_i", "f")] {
            let mut g = Glyph::new(n, vec![]);
            g.layers.push(
                f.glyph(base)
                    .and_then(|b| b.layer(&mid))
                    .cloned()
                    .unwrap_or_else(|| Layer::new(&mid, 600.0)),
            );
            f.glyphs.push(g);
        }
        f.feature_settings.automatic = available(&f);
        Document::new(f, None)
    }

    #[test]
    fn shaping_applies_liga_and_ss01() {
        let mut doc = doc_with_alternates();
        let on = shape(
            &mut doc,
            "fi a",
            "m01",
            None,
            &["liga=1".into(), "ss01=1".into()],
        )
        .unwrap();
        let names: Vec<&str> = on.lines[0].iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["f_i", "space", "a.ss01"], "{names:?}");
        let off = shape(
            &mut doc,
            "fi a",
            "m01",
            None,
            &["liga=0".into(), "ss01=0".into()],
        )
        .unwrap();
        let names: Vec<&str> = off.lines[0].iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["f", "i", "space", "a"], "{names:?}");
    }

    #[test]
    fn preview_cache_rebuilds_after_an_edit() {
        let mut doc = doc_with_alternates();
        shape(&mut doc, "a", "m01", None, &["ss01=1".into()]).unwrap();
        let first = doc.preview.as_ref().unwrap().0;
        // Same revision: the cached compile is reused.
        shape(&mut doc, "a", "m01", None, &["ss01=1".into()]).unwrap();
        assert_eq!(doc.preview.as_ref().unwrap().0, first);
        doc.revision += 1;
        shape(&mut doc, "a", "m01", None, &["ss01=1".into()]).unwrap();
        assert_eq!(doc.preview.as_ref().unwrap().0, first + 1);
    }

    #[test]
    fn features_view_lists_and_checks() {
        let mut doc = doc_with_alternates();
        let v = view(&doc);
        assert!(v.available.contains(&"ss01".to_string()));
        assert!(v.generated.contains("sub a by a.ss01;"));
        assert!(v.diagnostics.is_empty());
        doc.font.features = "feature ss01 {\n  sub a by missing;\n} ss01;\n".into();
        let v = view(&doc);
        assert!(v.handwritten.contains(&"ss01".to_string()));
        assert!(!v.available.contains(&"ss01".to_string()));
        assert_eq!(v.diagnostics[0].line, 2);
    }

    #[test]
    fn feature_import_previews_then_applies_as_one_step() {
        let mut doc = doc_with_alternates();
        doc.font.features = "feature liga {\n  sub f i by f_i;\n} liga;\n".into();
        let original = doc.font.clone();
        let incoming = "feature ss01 {\n  sub a by a.ss01;\n} ss01;\nfeature liga {\n  sub f i by f_i;\n} liga;\n";
        let p = preview_in(&doc, incoming).unwrap();
        assert_eq!(p.added, vec!["ss01".to_string()]);
        assert_eq!(p.replaced, vec!["liga".to_string()]);
        assert!(p.diagnostics.is_empty());
        let v = import_in(&mut doc, incoming).unwrap();
        assert!(v.status.can_undo && v.status.dirty);
        assert!(v.text.contains("sub a by a.ss01;"));
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn feature_import_preview_reports_problems_before_applying() {
        let doc = doc_with_alternates();
        let p = preview_in(&doc, "feature ss02 {\n  sub a by missing;\n} ss02;\n").unwrap();
        assert_eq!(p.added, vec!["ss02".to_string()]);
        assert!(!p.diagnostics.is_empty());
    }
}
