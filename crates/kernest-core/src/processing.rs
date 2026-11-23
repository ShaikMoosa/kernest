//! Glyph processing for one export (SMO-28): Glyphs' Rename Glyphs, Remove Glyphs,
//! Keep Glyphs, Decompose Glyphs, Decompose Components in Variable Font, Filter,
//! PreFilter and Remove Overlap off.
//!
//! [`prepare`] works on the font before an instance is interpolated (names,
//! glyph set, components); [`run_filters`] works on the instance's own outlines.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::model::{Font, GlyphProcessing, Layer, Shape};

type R<T> = std::result::Result<T, String>;

/// Whether `name` matches `pattern`, where `*` stands for any run of characters and
/// `?` for one.
pub fn glob(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some(('?', rest)) => !n.is_empty() && go(rest, &n[1..]),
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

/// The font's glyph names matching any of `patterns`, in glyph order, and the
/// patterns that match none.
pub fn expand(font: &Font, patterns: &[String]) -> (Vec<String>, Vec<String>) {
    let pats: Vec<&str> = patterns
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let names: Vec<String> = font
        .glyphs
        .iter()
        .filter(|g| pats.iter().any(|p| glob(p, &g.name)))
        .map(|g| g.name.clone())
        .collect();
    let unmatched = pats
        .iter()
        .filter(|p| !font.glyphs.iter().any(|g| glob(p, &g.name)))
        .map(|p| p.to_string())
        .collect();
    (names, unmatched)
}

/// Renames for one export. `a=b` with both glyphs present swaps them: their
/// drawings, kerning groups, kerning and hand-written feature rules trade places,
/// while names and Unicode values stay, so `a`'s code point shows `b`'s drawing.
/// When `b` doesn't exist, `a` is simply renamed to `b`.
fn rename(font: &mut Font, pairs: &[[String; 2]]) -> R<()> {
    for [a, b] in pairs {
        if a == b {
            continue;
        }
        let ia = font
            .glyphs
            .iter()
            .position(|g| &g.name == a)
            .ok_or_else(|| format!("Rename Glyphs: there is no glyph named \"{a}\"."))?;
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        match font.glyphs.iter().position(|g| &g.name == b) {
            Some(ib) => {
                let la = std::mem::take(&mut font.glyphs[ia].layers);
                let lb = std::mem::replace(&mut font.glyphs[ib].layers, la);
                font.glyphs[ia].layers = lb;
                // Kerning groups follow the drawings.
                let left = font.glyphs[ia].left_group.take();
                let right = font.glyphs[ia].right_group.take();
                let gb = &mut font.glyphs[ib];
                let left = std::mem::replace(&mut gb.left_group, left);
                let right = std::mem::replace(&mut gb.right_group, right);
                font.glyphs[ia].left_group = left;
                font.glyphs[ia].right_group = right;
                map.insert(a.clone(), b.clone());
                map.insert(b.clone(), a.clone());
            }
            None => {
                font.glyphs[ia].name = b.clone();
                for g in &mut font.glyphs {
                    for l in &mut g.layers {
                        for s in &mut l.shapes {
                            if let Shape::Component { base, .. } = s {
                                if base == a {
                                    *base = b.clone();
                                }
                            }
                        }
                    }
                }
                map.insert(a.clone(), b.clone());
            }
        }
        for pairs in font.kerning.values_mut() {
            for p in pairs.iter_mut() {
                if let Some(to) = map.get(&p.left) {
                    p.left = to.clone();
                }
                if let Some(to) = map.get(&p.right) {
                    p.right = to.clone();
                }
            }
        }
        font.features = crate::featureprune::rename(&font.features, &map);
    }
    Ok(())
}

/// The font one export is built from: glyphs renamed or swapped, then the glyphs
/// `remove` names and those `keep` leaves out removed (their components decomposed
/// where used, kerning and hand-written feature rules dropped), then the components
/// of `decompose_glyphs` (or every component) turned into outlines. Warnings name
/// patterns that match no glyph.
pub fn prepare(
    font: &Font,
    rename_pairs: &[[String; 2]],
    remove: &[String],
    p: &GlyphProcessing,
) -> R<(Font, Vec<String>)> {
    let mut f = font.clone();
    let mut warnings = Vec::new();
    rename(&mut f, rename_pairs)?;

    let (removed, unmatched) = expand(&f, remove);
    for u in unmatched {
        warnings.push(format!("Remove Glyphs: no glyph matches {u}."));
    }
    let mut gone: BTreeSet<String> = removed.into_iter().collect();
    if !p.keep_glyphs.is_empty() {
        let (kept, unmatched) = expand(&f, &p.keep_glyphs);
        for u in unmatched {
            warnings.push(format!("Keep Glyphs: no glyph matches {u}."));
        }
        let kept: HashSet<&str> = kept.iter().map(String::as_str).collect();
        gone.extend(
            f.glyphs
                .iter()
                .filter(|g| g.name != ".notdef" && !kept.contains(g.name.as_str()))
                .map(|g| g.name.clone()),
        );
    }
    if !gone.is_empty() {
        let names: Vec<String> = gone.iter().cloned().collect();
        // Locked glyphs only matter in the editor; an export copy may change them.
        for g in &mut f.glyphs {
            g.locked = false;
        }
        f = crate::glyphset::remove_glyphs(&f, &names)?;
        let gone: HashSet<String> = gone.into_iter().collect();
        f.features = crate::featureprune::prune(&f.features, &gone);
    }

    let decompose: HashSet<String> = if p.decompose_components {
        f.glyphs.iter().map(|g| g.name.clone()).collect()
    } else {
        let (names, unmatched) = expand(&f, &p.decompose_glyphs);
        for u in unmatched {
            warnings.push(format!("Decompose Glyphs: no glyph matches {u}."));
        }
        names.into_iter().collect()
    };
    if !decompose.is_empty() {
        let source = f.clone();
        for g in f.glyphs.iter_mut().filter(|g| decompose.contains(&g.name)) {
            for l in &mut g.layers {
                if l.shapes
                    .iter()
                    .any(|s| matches!(s, Shape::Component { .. }))
                {
                    *l = crate::smart::decompose_layer(&source, l);
                }
            }
        }
    }
    Ok((f, warnings))
}

/// One filter of a Filter or PreFilter parameter, as Glyphs writes them:
/// `RoundCorner;20`, `OffsetCurve;10;10`, `Transformations;Slant:8`,
/// `RemoveOverlap`, each optionally ending in `;include:A,B` or `;exclude:A,B`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportFilter {
    pub kind: FilterKind,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterKind {
    RoundCorner { radius: f64 },
    OffsetCurve { h: f64, v: f64, stroke: bool },
    Transform { affine: [f64; 6] },
    RemoveOverlap,
}

/// The filters Kernest runs, for messages and the parameter help.
pub const FILTERS: &str = "RoundCorner, OffsetCurve, Transformations, RemoveOverlap";

fn number(filter: &str, text: &str) -> R<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{filter}: \"{}\" isn't a number.", text.trim()))
}

/// Parses one filter string.
pub fn parse_filter(text: &str) -> R<ExportFilter> {
    let mut parts: Vec<&str> = text.split(';').map(str::trim).collect();
    let name = parts.remove(0);
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    let mut args = Vec::new();
    for p in parts {
        if let Some(list) = p.strip_prefix("include:") {
            include.extend(
                list.split([',', ' '])
                    .filter(|s| !s.is_empty())
                    .map(String::from),
            );
        } else if let Some(list) = p.strip_prefix("exclude:") {
            exclude.extend(
                list.split([',', ' '])
                    .filter(|s| !s.is_empty())
                    .map(String::from),
            );
        } else if !p.is_empty() {
            args.push(p);
        }
    }
    let kind = match name.to_ascii_lowercase().as_str() {
        "roundcorner" | "roundcorners" => {
            let radius = number(name, args.first().copied().unwrap_or(""))?;
            FilterKind::RoundCorner { radius }
        }
        "offsetcurve" => {
            let h = number(name, args.first().copied().unwrap_or("0"))?;
            let v = number(name, args.get(1).copied().unwrap_or("0"))?;
            let stroke = args.get(2).is_some_and(|s| s.trim() == "1");
            FilterKind::OffsetCurve { h, v, stroke }
        }
        "transformations" | "transform" => {
            let (mut sx, mut sy, mut dx, mut dy, mut slant) = (1.0, 1.0, 0.0, 0.0, 0.0);
            for a in &args {
                let Some((k, v)) = a.split_once(':') else {
                    return Err(format!(
                        "{name}: \"{a}\" should be a setting such as Slant:10."
                    ));
                };
                let v = number(name, v)?;
                match k.trim() {
                    "OffsetX" => dx = v,
                    "OffsetY" => dy = v,
                    "Scale" => {
                        sx = v / 100.0;
                        sy = v / 100.0;
                    }
                    "ScaleX" => sx = v / 100.0,
                    "ScaleY" => sy = v / 100.0,
                    "Slant" => slant = v,
                    other => {
                        return Err(format!(
                            "{name}: {other} isn't supported; use OffsetX, OffsetY, Scale, ScaleX, ScaleY or Slant."
                        ))
                    }
                }
            }
            let t = slant.to_radians().tan();
            FilterKind::Transform {
                affine: [sx, 0.0, t * sy, sy, dx, dy],
            }
        }
        "removeoverlap" => FilterKind::RemoveOverlap,
        _ => {
            return Err(format!(
                "There is no filter named {name}; Kernest has {FILTERS}."
            ))
        }
    };
    Ok(ExportFilter {
        kind,
        include,
        exclude,
    })
}

impl ExportFilter {
    fn applies_to(&self, glyph: &str) -> bool {
        (self.include.is_empty() || self.include.iter().any(|p| glob(p, glyph)))
            && !self.exclude.iter().any(|p| glob(p, glyph))
    }

    fn run(&self, layer: &mut Layer) {
        match &self.kind {
            FilterKind::RoundCorner { radius } => {
                crate::filters::round_corners(layer, *radius, None);
            }
            FilterKind::OffsetCurve { h, v, stroke } => {
                crate::filters::offset_curve(layer, *h, *v, *stroke, None);
            }
            FilterKind::Transform { affine } => {
                let sel = crate::transform::Selection {
                    nodes: layer
                        .shapes
                        .iter()
                        .filter_map(|s| match s {
                            Shape::Path(p) => Some(p.nodes.len()),
                            _ => None,
                        })
                        .enumerate()
                        .flat_map(|(pi, n)| (0..n).map(move |ni| (pi, ni)))
                        .collect(),
                    components: (0..layer
                        .shapes
                        .iter()
                        .filter(|s| matches!(s, Shape::Component { .. }))
                        .count())
                        .collect(),
                    anchors: layer.anchors.iter().map(|a| a.name.clone()).collect(),
                };
                crate::transform::apply(layer, &sel, affine, (0.0, 0.0));
            }
            FilterKind::RemoveOverlap => {
                crate::overlap::remove_overlaps(layer);
            }
        }
    }
}

/// Runs filter strings on an instance's layers, in order. A filter that can't be
/// read is skipped with a warning naming it (`param` is "Filter" or "PreFilter").
pub fn run_filters(
    layers: &mut [(String, Layer)],
    filters: &[String],
    param: &str,
    warnings: &mut Vec<String>,
) {
    for text in filters.iter().filter(|t| !t.trim().is_empty()) {
        match parse_filter(text) {
            Ok(f) => {
                for (name, layer) in layers.iter_mut() {
                    if f.applies_to(name) {
                        f.run(layer);
                    }
                }
            }
            Err(e) => warnings.push(format!("{param}: {e} It was skipped.")),
        }
    }
}

/// What the settings would warn about, without exporting: renames of glyphs that
/// don't exist, patterns that match nothing and filters that can't be read.
pub fn check(
    font: &Font,
    rename_pairs: &[[String; 2]],
    remove: &[String],
    p: &GlyphProcessing,
) -> Vec<String> {
    let mut out = match prepare(font, rename_pairs, remove, p) {
        Ok((_, w)) => w,
        Err(e) => vec![e],
    };
    for (param, list) in [("PreFilter", &p.pre_filters), ("Filter", &p.filters)] {
        for text in list.iter().filter(|t| !t.trim().is_empty()) {
            if let Err(e) = parse_filter(text) {
                out.push(format!("{param}: {e} It was skipped."));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Glyph, KernPair};
    use crate::sample::sample_font;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn wildcards_match_names() {
        assert!(glob("*.ss01", "a.ss01") && !glob("*.ss01", "a.ss02"));
        assert!(glob("a*", "a") && glob("a*", "aacute") && !glob("a*", "b"));
        assert!(glob("?", "A") && !glob("?", "AB"));
        let f = sample_font();
        let (m, un) = expand(&f, &names(&["H", "O*", "nope*"]));
        assert!(m.contains(&"H".to_string()) && m.contains(&"O".to_string()));
        assert_eq!(un, ["nope*"]);
    }

    #[test]
    fn removing_with_wildcards_prunes_features_and_kerning() {
        let mut f = sample_font();
        let mid = f.masters[0].id.clone();
        for n in ["H.ss01", "O.ss01"] {
            let mut g = f.glyph(&n[..1]).unwrap().clone();
            g.name = n.into();
            g.unicodes.clear();
            f.glyphs.push(g);
        }
        f.kerning.entry(mid.clone()).or_default().push(KernPair {
            left: "H.ss01".into(),
            right: "O".into(),
            value: -20.0,
        });
        f.features = "feature ss01 {\n  sub [H O] by [H.ss01 O.ss01];\n} ss01;\n".into();
        let (out, w) = prepare(&f, &[], &names(&["*.ss01"]), &GlyphProcessing::default()).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert!(out.glyph("H.ss01").is_none() && out.glyph("O.ss01").is_none());
        assert!(!out.features.contains("feature ss01"), "{}", out.features);
        assert!(!out.kerning[&mid].iter().any(|p| p.left == "H.ss01"));
    }

    #[test]
    fn keep_glyphs_keeps_notdef_and_decomposes_what_it_needs() {
        let mut f = sample_font();
        // A glyph built from a component of H: keeping only it keeps its shape.
        let mid = f.masters[0].id.clone();
        let mut comp = Glyph::new("Hcomp", vec![]);
        let mut l = crate::model::Layer::new(&mid, 680.0);
        l.shapes.push(Shape::Component {
            base: "H".into(),
            transform: crate::model::IDENTITY,
            smart: Default::default(),
            align: Default::default(),
        });
        comp.layers.push(l);
        f.glyphs.push(comp);
        let p = GlyphProcessing {
            keep_glyphs: names(&["Hcomp", "missing*"]),
            ..Default::default()
        };
        let (out, w) = prepare(&f, &[], &[], &p).unwrap();
        assert_eq!(w, ["Keep Glyphs: no glyph matches missing*."]);
        let left: Vec<&str> = out.glyphs.iter().map(|g| g.name.as_str()).collect();
        assert!(left.contains(&"Hcomp"), "{left:?}");
        assert!(!left.contains(&"H"), "{left:?}");
        assert!(left.contains(&".notdef") || f.glyph(".notdef").is_none());
        let shapes = &out.glyph("Hcomp").unwrap().layers[0].shapes;
        assert!(shapes.iter().all(|s| matches!(s, Shape::Path(_))) && !shapes.is_empty());
    }

    #[test]
    fn renames_swap_drawings_groups_kerning_and_features() {
        let mut f = sample_font();
        let mid = f.masters[0].id.clone();
        let mut alt = f.glyph("O").unwrap().clone();
        alt.name = "H.alt".into();
        alt.unicodes.clear();
        alt.left_group = Some("round".into());
        f.glyphs.push(alt);
        f.glyph_mut("H").unwrap().left_group = Some("straight".into());
        f.kerning.entry(mid.clone()).or_default().push(KernPair {
            left: "H.alt".into(),
            right: "O".into(),
            value: -30.0,
        });
        f.features = "feature ss01 {\n  sub H by H.alt;\n} ss01;\n".into();
        let before_alt = f.glyph("H.alt").unwrap().layers.clone();
        let (out, _) = prepare(
            &f,
            &[["H".into(), "H.alt".into()]],
            &[],
            &GlyphProcessing::default(),
        )
        .unwrap();
        let h = out.glyph("H").unwrap();
        assert_eq!(h.layers, before_alt, "H draws the former H.alt");
        assert_eq!(
            h.unicodes,
            f.glyph("H").unwrap().unicodes,
            "and keeps U+0048"
        );
        assert_eq!(h.left_group.as_deref(), Some("round"));
        assert!(out.kerning[&mid]
            .iter()
            .any(|p| p.left == "H" && p.right == "O" && p.value == -30.0));
        assert!(out.features.contains("sub H.alt by H;"), "{}", out.features);
        // A rename to a new name.
        let (out, _) = prepare(
            &f,
            &[["H.alt".into(), "Hx".into()]],
            &[],
            &GlyphProcessing::default(),
        )
        .unwrap();
        assert!(out.glyph("Hx").is_some() && out.glyph("H.alt").is_none());
        assert!(out.features.contains("sub H by Hx;"), "{}", out.features);
        // A rename of a glyph that doesn't exist is an error.
        assert!(prepare(
            &f,
            &[["nope".into(), "H".into()]],
            &[],
            &GlyphProcessing::default()
        )
        .is_err());
    }

    #[test]
    fn decompose_glyphs_and_all_components() {
        let mut f = sample_font();
        let mid = f.masters[0].id.clone();
        for (name, base) in [("Hc", "H"), ("Oc", "O")] {
            let mut g = Glyph::new(name, vec![]);
            let mut l = crate::model::Layer::new(&mid, 600.0);
            l.shapes.push(Shape::Component {
                base: base.into(),
                transform: crate::model::IDENTITY,
                smart: Default::default(),
                align: Default::default(),
            });
            g.layers.push(l);
            f.glyphs.push(g);
        }
        let with_components: Vec<String> = f
            .glyphs
            .iter()
            .filter(|g| {
                g.layers.iter().any(|l| {
                    l.shapes
                        .iter()
                        .any(|s| matches!(s, Shape::Component { .. }))
                })
            })
            .map(|g| g.name.clone())
            .collect();
        assert!(
            !with_components.is_empty(),
            "the sample font has composites"
        );
        let first = &with_components[0];
        let p = GlyphProcessing {
            decompose_glyphs: vec![first.clone()],
            ..Default::default()
        };
        let (out, _) = prepare(&f, &[], &[], &p).unwrap();
        let no_comp = |f: &Font, n: &str| {
            f.glyph(n)
                .unwrap()
                .layers
                .iter()
                .all(|l| l.shapes.iter().all(|s| matches!(s, Shape::Path(_))))
        };
        assert!(no_comp(&out, first));
        if let Some(second) = with_components.get(1) {
            assert!(!no_comp(&out, second), "only the listed glyph");
        }
        let all = GlyphProcessing {
            decompose_components: true,
            ..Default::default()
        };
        let (out, _) = prepare(&f, &[], &[], &all).unwrap();
        assert!(with_components.iter().all(|n| no_comp(&out, n)));
    }

    #[test]
    fn filters_parse_and_run() {
        assert_eq!(
            parse_filter("RoundCorner;20;include:H,O").unwrap(),
            ExportFilter {
                kind: FilterKind::RoundCorner { radius: 20.0 },
                include: names(&["H", "O"]),
                exclude: vec![],
            }
        );
        assert!(matches!(
            parse_filter("OffsetCurve;10;5").unwrap().kind,
            FilterKind::OffsetCurve { h, v, stroke: false } if h == 10.0 && v == 5.0
        ));
        assert!(parse_filter("Roughenizer;5")
            .unwrap_err()
            .contains("no filter named Roughenizer"));
        assert!(parse_filter("RoundCorner;wide")
            .unwrap_err()
            .contains("isn't a number"));
        assert!(parse_filter("Transformations;LSB:10").is_err());

        let f = sample_font();
        let mid = f.masters[0].id.clone();
        let h = f.glyph("H").unwrap().layer(&mid).unwrap().clone();
        let nodes = |l: &Layer| -> usize {
            l.shapes
                .iter()
                .map(|s| match s {
                    Shape::Path(p) => p.nodes.len(),
                    _ => 0,
                })
                .sum()
        };
        let mut layers = vec![
            ("H".to_string(), h.clone()),
            (
                "O".to_string(),
                f.glyph("O").unwrap().layer(&mid).unwrap().clone(),
            ),
        ];
        let mut w = Vec::new();
        run_filters(
            &mut layers,
            &names(&["RoundCorner;20;exclude:O", "Nope"]),
            "Filter",
            &mut w,
        );
        assert!(nodes(&layers[0].1) > nodes(&h), "H's corners are rounded");
        assert_eq!(
            layers[1].1,
            f.glyph("O").unwrap().layer(&mid).unwrap().clone(),
            "O is excluded"
        );
        assert_eq!(w.len(), 1);
        assert!(
            w[0].starts_with("Filter: There is no filter named Nope"),
            "{w:?}"
        );
        // Slant moves the top of a stem right.
        let mut layers = vec![("H".to_string(), h.clone())];
        run_filters(
            &mut layers,
            &names(&["Transformations;Slant:10"]),
            "Filter",
            &mut w,
        );
        let top = |l: &Layer| {
            l.shapes
                .iter()
                .filter_map(|s| match s {
                    Shape::Path(p) => p
                        .nodes
                        .iter()
                        .map(|n| (n.y, n.x))
                        .max_by(|a, b| a.partial_cmp(b).unwrap()),
                    _ => None,
                })
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap()
        };
        assert!(top(&layers[0].1).1 > top(&h).1);
    }
}
