//! Commands for the Features editor and the text view's shaped feature preview.
//!
//! The preview shapes a quick TrueType compile of the current master with rustybuzz,
//! so what the text view shows is what the exported font will do. The compile is
//! cached per revision and location; the UI debounces calls while typing.

use std::collections::HashMap;

use kernest_core::bidi::{self, TextDirection};
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

/// What export-specific feature code parameters (SMO-23) would do to this font's
/// feature code: a warning for each one that can't be read or names something the
/// code doesn't have. Empty when they all apply.
#[tauri::command]
pub fn check_feature_parameters(
    state: State<AppState>,
    parameters: Vec<kernest_core::CustomParameter>,
) -> CmdResult<Vec<String>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    Ok(feature_parameter_warnings(&doc.font, &parameters))
}

fn feature_parameter_warnings(
    font: &Font,
    parameters: &[kernest_core::CustomParameter],
) -> Vec<String> {
    kernest_core::featureedits::apply(&kernest_core::features::combined(font), parameters).1
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
    /// Vertical advance (negative: down), for vertical text.
    y_advance: f64,
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

/// Features HarfBuzz applies where they belong without being asked (positional
/// forms, marks, kerning, required ligatures, vertical forms).
const SHAPER_DEFAULT: &[&str] = &[
    "abvf", "abvm", "abvs", "akhn", "blwf", "blwm", "blws", "calt", "ccmp", "cfar", "cjct", "clig",
    "curs", "dist", "fin2", "fin3", "fina", "half", "haln", "init", "isol", "kern", "liga", "ljmo",
    "locl", "mark", "med2", "medi", "mkmk", "nukt", "pref", "pres", "pstf", "psts", "rclt", "rkrf",
    "rlig", "rphf", "rvrn", "stch", "tjmo", "vatu", "vert", "vjmo", "vkrn",
];

/// The compiled preview font for this revision and location, compiling it if the
/// cache is stale. Returns the bytes and the glyph order they were built with.
pub(crate) fn preview_font(
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
    direction: Option<TextDirection>,
) -> CmdResult<ShapedView> {
    let master = doc
        .font
        .master(master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    let metrics = master.metrics;
    let (bytes, order) = preview_font(doc, master_id, location)?;
    let face = rustybuzz::Face::from_slice(&bytes, 0)
        .ok_or_else(|| "could not read the preview font".to_string())?;
    // Features the shaper applies by itself are only passed to turn them off: forcing
    // `fina=1` over the whole text would put every Arabic letter in its final form.
    let feats: Vec<rustybuzz::Feature> = features
        .iter()
        .filter(|t| {
            t.split_once('=')
                .is_none_or(|(tag, v)| v.trim() != "1" || !SHAPER_DEFAULT.contains(&tag.trim()))
        })
        .filter_map(|t| t.parse().ok())
        .collect();

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
            let mut glyphs = Vec::new();
            for (start, end, dir) in runs(line, direction) {
                let mut buffer = rustybuzz::UnicodeBuffer::new();
                buffer.push_str(&line[start..end]);
                buffer.guess_segment_properties();
                buffer.set_direction(dir);
                let out = rustybuzz::shape(&face, &feats, buffer);
                for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
                    let name = order
                        .get(info.glyph_id as usize)
                        .cloned()
                        .unwrap_or_else(|| ".notdef".to_string());
                    let (svg, color) = draw(&name);
                    glyphs.push(ShapedGlyph {
                        name,
                        svg,
                        color,
                        advance: f64::from(pos.x_advance),
                        y_advance: f64::from(pos.y_advance),
                        x_offset: f64::from(pos.x_offset),
                        y_offset: f64::from(pos.y_offset),
                        cluster: info.cluster + start as u32,
                    });
                }
            }
            glyphs
        })
        .collect();

    Ok(ShapedView {
        lines,
        metrics,
        palette: css_palette(&doc.font),
        status: status(doc),
    })
}

/// A line split into runs to shape, in visual order: byte range and direction. Vertical
/// text is one top-to-bottom run; horizontal text is split where the resolved bidi
/// direction changes.
fn runs(line: &str, direction: Option<TextDirection>) -> Vec<(usize, usize, rustybuzz::Direction)> {
    if line.is_empty() {
        return Vec::new();
    }
    if direction == Some(TextDirection::Vertical) {
        return vec![(0, line.len(), rustybuzz::Direction::TopToBottom)];
    }
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let strong: Vec<Option<bool>> = chars
        .iter()
        .map(|(_, c)| bidi::strong_codepoint(*c as u32))
        .collect();
    let para = TextDirection::paragraph_rtl(direction, &strong);
    let resolved = bidi::resolve(&strong, para);
    let mut out: Vec<(usize, usize, bool)> = Vec::new();
    for (i, &rtl) in resolved.iter().enumerate() {
        let end = chars.get(i + 1).map_or(line.len(), |c| c.0);
        match out.last_mut() {
            Some(last) if last.2 == rtl => last.1 = end,
            _ => out.push((chars[i].0, end, rtl)),
        }
    }
    if para {
        out.reverse();
    }
    out.into_iter()
        .map(|(a, b, rtl)| {
            let dir = if rtl {
                rustybuzz::Direction::RightToLeft
            } else {
                rustybuzz::Direction::LeftToRight
            };
            (a, b, dir)
        })
        .collect()
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
    direction: Option<String>,
) -> CmdResult<ShapedView> {
    let mut guard = lock(&state)?;
    shape(
        doc_mut(&mut guard)?,
        &text,
        &master_id,
        location.as_ref(),
        &features,
        direction.as_deref().and_then(TextDirection::parse),
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
    fn arabic_letters_join_with_generated_positional_features() {
        // beh and alef with positional forms drawn like H (SMO-20).
        let mut f = sample_font();
        let mid = f.masters[0].id.clone();
        let layer = f.glyph("H").and_then(|g| g.layer(&mid)).cloned().unwrap();
        for (n, cp) in [
            ("beh-ar", Some(0x0628)),
            ("alef-ar", Some(0x0627)),
            ("beh-ar.init", None),
            ("beh-ar.medi", None),
            ("beh-ar.fina", None),
            ("alef-ar.fina", None),
        ] {
            let mut g = Glyph::new(n, cp.map(kernest_core::Codepoint).into_iter().collect());
            g.layers.push(layer.clone());
            f.glyphs.push(g);
        }
        f.feature_settings.automatic = available(&f);
        let mut doc = Document::new(f, None);
        let names = |doc: &mut Document, text: &str| -> Vec<String> {
            shape(doc, text, "m01", None, &[], None).unwrap().lines[0]
                .iter()
                .map(|g| g.name.clone())
                .collect()
        };
        // Right to left, so the glyphs come back in visual order.
        assert_eq!(
            names(&mut doc, "\u{628}\u{628}"),
            ["beh-ar.fina", "beh-ar.init"]
        );
        assert_eq!(
            names(&mut doc, "\u{628}\u{628}\u{628}"),
            ["beh-ar.fina", "beh-ar.medi", "beh-ar.init"]
        );
        // Alef doesn't join to the left, so beh after it starts a new word form.
        assert_eq!(
            names(&mut doc, "\u{628}\u{627}"),
            ["alef-ar.fina", "beh-ar.init"]
        );

        // SMO-33: in a right-to-left paragraph a Latin word keeps its order, at the left.
        let rtl = shape(
            &mut doc,
            "HO \u{628}\u{628}",
            "m01",
            None,
            &[],
            Some(TextDirection::Rtl),
        )
        .unwrap();
        let visual: Vec<&str> = rtl.lines[0].iter().map(|g| g.name.as_str()).collect();
        assert_eq!(visual.first(), Some(&"beh-ar.fina"));
        assert_eq!(visual[1], "beh-ar.init");
        assert_eq!(&visual[visual.len() - 2..], ["H", "O"]);
        // The clusters still point into the whole line.
        assert_eq!(rtl.lines[0].last().unwrap().cluster, 1);
        // Left to right, the Arabic word still joins and reads right to left.
        let ltr = shape(
            &mut doc,
            "HO \u{628}\u{628}",
            "m01",
            None,
            &[],
            Some(TextDirection::Ltr),
        )
        .unwrap();
        let visual: Vec<&str> = ltr.lines[0].iter().map(|g| g.name.as_str()).collect();
        assert_eq!(&visual[..2], ["H", "O"]);
        assert_eq!(&visual[visual.len() - 2..], ["beh-ar.fina", "beh-ar.init"]);
    }

    #[test]
    fn explicit_positional_features_do_not_force_every_form() {
        let mut f = sample_font();
        let mid = f.masters[0].id.clone();
        let layer = f.glyph("H").and_then(|g| g.layer(&mid)).cloned().unwrap();
        for (n, cp) in [
            ("beh-ar", Some(0x0628)),
            ("beh-ar.init", None),
            ("beh-ar.medi", None),
            ("beh-ar.fina", None),
        ] {
            let mut g = Glyph::new(n, cp.map(kernest_core::Codepoint).into_iter().collect());
            g.layers.push(layer.clone());
            f.glyphs.push(g);
        }
        f.feature_settings.automatic = available(&f);
        let mut doc = Document::new(f, None);
        let on: Vec<String> = ["init", "medi", "fina"]
            .iter()
            .map(|t| format!("{t}=1"))
            .collect();
        let names = |doc: &mut Document, feats: &[String]| -> Vec<String> {
            shape(
                doc,
                "\u{628}\u{628}\u{628}",
                "m01",
                None,
                feats,
                Some(TextDirection::Rtl),
            )
            .unwrap()
            .lines[0]
                .iter()
                .map(|g| g.name.clone())
                .collect()
        };
        assert_eq!(
            names(&mut doc, &on),
            ["beh-ar.fina", "beh-ar.medi", "beh-ar.init"]
        );
        // Unticking still turns a feature off.
        assert_eq!(
            names(&mut doc, &["init=0".into()]),
            ["beh-ar.fina", "beh-ar.medi", "beh-ar"]
        );
    }

    #[test]
    fn preview_names_follow_the_compiled_glyph_ids() {
        // A glyph that doesn't export shifts the compiled ids; production names differ
        // from the source names. Shaped glyphs still come back by source name.
        let mut f = sample_font();
        let first = f
            .glyphs
            .iter()
            .position(|g| !matches!(g.name.as_str(), ".notdef" | "H" | "o"))
            .unwrap();
        f.glyphs[first].export = false;
        f.glyphs
            .iter_mut()
            .find(|g| g.name == "o")
            .unwrap()
            .production_name = Some("uni006F".into());
        let mut doc = Document::new(f, None);
        let out = shape(&mut doc, "Ho", "m01", None, &[], None).unwrap();
        let names: Vec<&str> = out.lines[0].iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["H", "o"]);
    }

    #[test]
    fn vertical_text_advances_down() {
        let mut doc = doc_with_alternates();
        let v = shape(
            &mut doc,
            "HO",
            "m01",
            None,
            &[],
            Some(TextDirection::Vertical),
        )
        .unwrap();
        assert_eq!(v.lines[0].len(), 2);
        for g in &v.lines[0] {
            assert_eq!(g.advance, 0.0);
            assert!(g.y_advance < 0.0, "{} advances {}", g.name, g.y_advance);
        }
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
            None,
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
            None,
        )
        .unwrap();
        let names: Vec<&str> = off.lines[0].iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["f", "i", "space", "a"], "{names:?}");
    }

    #[test]
    fn preview_cache_rebuilds_after_an_edit() {
        let mut doc = doc_with_alternates();
        shape(&mut doc, "a", "m01", None, &["ss01=1".into()], None).unwrap();
        let first = doc.preview.as_ref().unwrap().0;
        // Same revision: the cached compile is reused.
        shape(&mut doc, "a", "m01", None, &["ss01=1".into()], None).unwrap();
        assert_eq!(doc.preview.as_ref().unwrap().0, first);
        doc.revision += 1;
        shape(&mut doc, "a", "m01", None, &["ss01=1".into()], None).unwrap();
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

    #[test]
    fn generated_fraction_ligature_zero_and_figure_features_shape() {
        // SMO-21: alternates drawn like H, added to the sample font.
        let mut f = sample_font();
        let mid = f.masters[0].id.clone();
        let layer = f.glyph("H").and_then(|g| g.layer(&mid)).cloned().unwrap();
        let mut add = |n: &str, cp: Option<u32>| {
            if f.glyph(n).is_none() {
                let mut g = Glyph::new(n, cp.map(kernest_core::Codepoint).into_iter().collect());
                g.layers.push(layer.clone());
                f.glyphs.push(g);
            }
        };
        add("fraction", Some(0x2044));
        for (n, cp) in [
            ("zero", 0x30),
            ("one", 0x31),
            ("two", 0x32),
            ("slash", 0x2F),
        ] {
            add(n, Some(cp));
        }
        for n in [
            "one.numr",
            "one.dnom",
            "two.numr",
            "two.dnom",
            "f_f_i.dlig",
            "zero.zero",
            "one.tf",
            "one.osf",
            "one.tosf",
        ] {
            add(n, None);
        }
        f.feature_settings.automatic = available(&f);
        let mut doc = Document::new(f, None);
        let names = |doc: &mut Document, text: &str, feats: &[&str]| -> Vec<String> {
            let feats: Vec<String> = feats.iter().map(|t| format!("{t}=1")).collect();
            shape(doc, text, "m01", None, &feats, None).unwrap().lines[0]
                .iter()
                .map(|g| g.name.clone())
                .collect()
        };
        assert_eq!(
            names(&mut doc, "1/2", &["frac"]),
            ["one.numr", "fraction", "two.dnom"]
        );
        assert_eq!(
            names(&mut doc, "12/21", &["frac"]),
            ["one.numr", "two.numr", "fraction", "two.dnom", "one.dnom"]
        );
        assert_eq!(names(&mut doc, "1/2", &[]), ["one", "slash", "two"]);
        assert_eq!(names(&mut doc, "ffi", &["dlig"]), ["f_f_i.dlig"]);
        assert_eq!(names(&mut doc, "0", &["zero"]), ["zero.zero"]);
        assert_eq!(names(&mut doc, "1", &["onum", "tnum"]), ["one.tosf"]);
        assert_eq!(names(&mut doc, "1", &["onum"]), ["one.osf"]);
    }

    #[test]
    fn feature_parameters_are_checked_against_the_fonts_code() {
        // SMO-23: liga is generated, so removing it is fine; dlig isn't there.
        let doc = doc_with_alternates();
        let p = |name: &str, value: &str| kernest_core::CustomParameter {
            name: name.into(),
            value: value.into(),
            disabled: false,
        };
        assert!(feature_parameter_warnings(&doc.font, &[p("Remove Features", "liga")]).is_empty());
        let w = feature_parameter_warnings(
            &doc.font,
            &[p("Remove Features", "dlig"), p("Replace Feature", "oops")],
        );
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("dlig"), "{w:?}");
    }
}
