//! Importing metrics (advance widths, sidebearings and kerning) from an AFM file or
//! another font.
//!
//! An imported width rarely fits a glyph whose outline differs from the source's, so
//! one sidebearing is kept fixed while the width changes: the outline slides to hold
//! either the source's left or its right sidebearing. Kerning pairs are merged in.

use std::collections::BTreeMap;

use kernest_core::{geometry, transform, Font, KernPair};

/// Which sidebearing stays fixed when an imported width is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepSide {
    Left,
    Right,
}

impl KeepSide {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

/// Metrics gathered from a source, keyed by glyph name.
#[derive(Debug, Default, Clone)]
pub struct SourceMetrics {
    /// Advance width per glyph.
    pub widths: BTreeMap<String, f64>,
    /// Left sidebearing per glyph, when the source knows its outlines.
    pub left: BTreeMap<String, f64>,
    /// Right sidebearing per glyph, when the source knows its outlines.
    pub right: BTreeMap<String, f64>,
    /// Kerning pairs (glyph names or `@groups`).
    pub kerning: Vec<KernPair>,
}

/// Reads an AFM (Adobe Font Metrics) file: character widths and bounding boxes from
/// `StartCharMetrics`, kerning pairs from `StartKernPairs`.
pub fn parse_afm(text: &str) -> Result<SourceMetrics, String> {
    let mut m = SourceMetrics::default();
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("StartCharMetrics") {
            section = "char";
            continue;
        }
        if line.starts_with("EndCharMetrics") {
            section = "";
            continue;
        }
        if line.starts_with("StartKernPairs") {
            section = "kern";
            continue;
        }
        if line.starts_with("EndKernPairs") {
            section = "";
            continue;
        }
        match section {
            "char" => {
                let mut name = None;
                let mut width = None;
                let mut bbox = None;
                for part in line.split(';') {
                    let mut it = part.split_whitespace();
                    match it.next() {
                        Some("N") => name = it.next().map(str::to_string),
                        Some("WX") => width = it.next().and_then(|v| v.parse::<f64>().ok()),
                        Some("B") => {
                            let nums: Vec<f64> = it.filter_map(|v| v.parse().ok()).collect();
                            if nums.len() == 4 {
                                bbox = Some(nums);
                            }
                        }
                        _ => {}
                    }
                }
                if let (Some(name), Some(w)) = (name, width) {
                    m.widths.insert(name.clone(), w);
                    if let Some(b) = bbox {
                        m.left.insert(name.clone(), b[0]);
                        m.right.insert(name, w - b[2]);
                    }
                }
            }
            "kern" => {
                let mut it = line.split_whitespace();
                if it.next() == Some("KPX") {
                    if let (Some(l), Some(r), Some(v)) = (
                        it.next(),
                        it.next(),
                        it.next().and_then(|v| v.parse::<f64>().ok()),
                    ) {
                        m.kerning.push(KernPair {
                            left: l.to_string(),
                            right: r.to_string(),
                            value: v,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    if m.widths.is_empty() && m.kerning.is_empty() {
        return Err("the AFM file has no character metrics or kerning pairs.".into());
    }
    Ok(m)
}

/// Gathers widths, sidebearings and kerning from one master of another font.
pub fn from_font(font: &Font, master_id: &str) -> Result<SourceMetrics, String> {
    if font.master(master_id).is_none() {
        return Err(format!("the source font has no master \"{master_id}\"."));
    }
    let mut m = SourceMetrics::default();
    for g in &font.glyphs {
        if let Some(l) = g.layer(master_id) {
            m.widths.insert(g.name.clone(), l.width);
            if let Some(b) = geometry::bounds(font, l) {
                m.left.insert(g.name.clone(), b.min_x);
                m.right.insert(g.name.clone(), l.width - b.max_x);
            }
        }
    }
    if let Some(k) = font.kerning.get(master_id) {
        m.kerning = k.clone();
    }
    Ok(m)
}

/// Applies source metrics to a copy of `font`'s master `master_id`, returning the new
/// font, the glyphs whose width changed, and warnings. The caller records it as one
/// undoable step. Locked glyphs are skipped.
pub fn apply_metrics(
    font: &Font,
    master_id: &str,
    src: &SourceMetrics,
    keep: KeepSide,
    widths: bool,
    kerning: bool,
) -> Result<(Font, Vec<String>, Vec<String>), String> {
    if font.master(master_id).is_none() {
        return Err(format!("no master with id \"{master_id}\""));
    }
    let mut next = font.clone();
    let mut changed: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut skipped_locked = false;
    let mut missing = 0usize;

    if widths {
        for (name, &w) in &src.widths {
            let Some(g) = next.glyph(name) else {
                missing += 1;
                continue;
            };
            if g.locked {
                skipped_locked = true;
                continue;
            }
            let Some(layer) = g.layer(master_id) else {
                continue;
            };
            let mut new_layer = layer.clone();
            if let Some(b) = geometry::bounds(&next, layer) {
                // Keep the chosen source sidebearing; fall back to the glyph's own
                // sidebearing when the source has no outline for it.
                let dx = match keep {
                    KeepSide::Left => src.left.get(name).copied().unwrap_or(b.min_x) - b.min_x,
                    KeepSide::Right => {
                        let rsb = src
                            .right
                            .get(name)
                            .copied()
                            .unwrap_or(layer.width - b.max_x);
                        (w - rsb) - b.max_x
                    }
                };
                if dx.abs() > 1e-9 {
                    transform::apply(
                        &mut new_layer,
                        &transform::Selection::default(),
                        &transform::translate(dx, 0.0),
                        (0.0, 0.0),
                    );
                }
            }
            new_layer.width = w;
            *next
                .glyph_mut(name)
                .expect("glyph exists")
                .layer_mut(master_id)
                .expect("layer exists") = new_layer;
            changed.push(name.clone());
        }
    }

    if kerning && !src.kerning.is_empty() {
        let mut k = next.kerning.get(master_id).cloned().unwrap_or_default();
        let mut dropped = 0usize;
        for p in &src.kerning {
            if !side_exists(&next, &p.left) || !side_exists(&next, &p.right) {
                dropped += 1;
                continue;
            }
            if let Some(existing) = k
                .iter_mut()
                .find(|q| q.left == p.left && q.right == p.right)
            {
                existing.value = p.value;
            } else {
                k.push(p.clone());
            }
        }
        next.kerning.insert(master_id.to_string(), k);
        if dropped > 0 {
            warnings.push(format!(
                "{dropped} kerning pair{} referenced glyphs that aren't in this font and {was} left out.",
                crate::plural(dropped),
                was = if dropped == 1 { "was" } else { "were" }
            ));
        }
    }

    if missing > 0 {
        warnings.push(format!(
            "{missing} glyph{} in the source {was}n't in this font and {was} left out.",
            crate::plural(missing),
            was = if missing == 1 { "was" } else { "were" }
        ));
    }
    if skipped_locked {
        warnings.push("Locked glyphs were skipped.".into());
    }
    Ok((next, changed, warnings))
}

/// A kerning side is valid when it names a glyph in the font or is a `@group`.
fn side_exists(font: &Font, side: &str) -> bool {
    side.starts_with('@') || font.glyph(side).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::{Layer, Node, NodeKind, Path, Shape};

    const AFM: &str = "StartFontMetrics 4.1\nFontName Test\nStartCharMetrics 2\nC 65 ; WX 600 ; N A ; B 10 0 590 700 ;\nC 86 ; WX 550 ; N V ; B 5 0 545 700 ;\nEndCharMetrics\nStartKernData\nStartKernPairs 1\nKPX A V -50\nEndKernPairs\nEndKernData\nEndFontMetrics\n";

    #[test]
    fn afm_reads_widths_sidebearings_and_kerning() {
        let m = parse_afm(AFM).unwrap();
        assert_eq!(m.widths["A"], 600.0);
        assert_eq!(m.left["A"], 10.0);
        assert_eq!(m.right["A"], 10.0); // 600 - 590
        assert_eq!(m.widths["V"], 550.0);
        assert_eq!(m.kerning.len(), 1);
        assert_eq!(m.kerning[0].left, "A");
        assert_eq!(m.kerning[0].value, -50.0);
    }

    #[test]
    fn afm_without_metrics_is_an_error() {
        assert!(parse_afm("StartFontMetrics 4.1\nFontName Test\nEndFontMetrics\n").is_err());
    }

    /// A font with one glyph "A": a 100-wide box from x=50..150, width 300 (LSB 50, RSB 150).
    fn one_glyph() -> Font {
        let mut f = Font::new("Test", Vec::<u32>::new());
        let mut g = kernest_core::Glyph::new("A", Vec::new());
        let mut layer = Layer::new("m01", 300.0);
        let rect = Path {
            locked: false,
            stroke: None,
            closed: true,
            nodes: vec![
                Node::new(50.0, 0.0, NodeKind::Line),
                Node::new(150.0, 0.0, NodeKind::Line),
                Node::new(150.0, 100.0, NodeKind::Line),
                Node::new(50.0, 100.0, NodeKind::Line),
            ],
        };
        layer.shapes.push(Shape::Path(rect));
        g.layers.push(layer);
        f.glyphs.push(g);
        f
    }

    #[test]
    fn keep_left_holds_the_source_left_sidebearing() {
        let f = one_glyph();
        let mut src = SourceMetrics::default();
        src.widths.insert("A".into(), 400.0);
        src.left.insert("A".into(), 20.0);
        src.right.insert("A".into(), 280.0);
        let (next, changed, _) =
            apply_metrics(&f, "m01", &src, KeepSide::Left, true, false).unwrap();
        assert_eq!(changed, vec!["A".to_string()]);
        let l = next.glyph("A").unwrap().layer("m01").unwrap();
        assert_eq!(l.width, 400.0);
        let b = geometry::bounds(&next, l).unwrap();
        assert!((b.min_x - 20.0).abs() < 1e-6); // slid left to the source LSB
        assert!((b.max_x - 120.0).abs() < 1e-6);
    }

    #[test]
    fn keep_right_holds_the_source_right_sidebearing() {
        let f = one_glyph();
        let mut src = SourceMetrics::default();
        src.widths.insert("A".into(), 400.0);
        src.left.insert("A".into(), 20.0);
        src.right.insert("A".into(), 30.0);
        let (next, _, _) = apply_metrics(&f, "m01", &src, KeepSide::Right, true, false).unwrap();
        let l = next.glyph("A").unwrap().layer("m01").unwrap();
        assert_eq!(l.width, 400.0);
        let b = geometry::bounds(&next, l).unwrap();
        assert!((b.max_x - 370.0).abs() < 1e-6); // 400 - 30
        assert!((b.min_x - 270.0).abs() < 1e-6);
    }

    #[test]
    fn keep_left_without_source_sidebearing_holds_the_glyphs_own() {
        let f = one_glyph();
        let mut src = SourceMetrics::default();
        src.widths.insert("A".into(), 400.0); // no left/right (e.g. AFM without B)
        let (next, _, _) = apply_metrics(&f, "m01", &src, KeepSide::Left, true, false).unwrap();
        let l = next.glyph("A").unwrap().layer("m01").unwrap();
        let b = geometry::bounds(&next, l).unwrap();
        assert!((b.min_x - 50.0).abs() < 1e-6); // unchanged LSB
        assert_eq!(l.width, 400.0);
    }

    #[test]
    fn kerning_merges_and_overrides() {
        let mut f = one_glyph();
        f.glyphs.push(kernest_core::Glyph::new("V", Vec::new()));
        f.kerning.insert(
            "m01".into(),
            vec![KernPair {
                left: "A".into(),
                right: "V".into(),
                value: -10.0,
            }],
        );
        let src = SourceMetrics {
            kerning: vec![
                KernPair {
                    left: "A".into(),
                    right: "V".into(),
                    value: -50.0,
                },
                KernPair {
                    left: "A".into(),
                    right: "nope".into(),
                    value: -20.0,
                },
            ],
            ..Default::default()
        };
        let (next, _, warnings) =
            apply_metrics(&f, "m01", &src, KeepSide::Left, false, true).unwrap();
        let k = next.kerning.get("m01").unwrap();
        assert_eq!(k.len(), 1); // the "nope" pair was dropped
        assert_eq!(k[0].value, -50.0);
        assert!(warnings.iter().any(|w| w.contains("kerning pair")));
    }

    #[test]
    fn from_font_reads_a_master() {
        let f = one_glyph();
        let m = from_font(&f, "m01").unwrap();
        assert_eq!(m.widths["A"], 300.0);
        assert_eq!(m.left["A"], 50.0);
        assert_eq!(m.right["A"], 150.0);
        assert!(from_font(&f, "nope").is_err());
    }
}
