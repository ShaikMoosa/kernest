//! Commands for the text (spacing) view and the kerning window.

use std::collections::{HashMap, HashSet};

use kernest_core::bidi::{self, TextDirection};
use kernest_core::designspace::Location;
use kernest_core::geometry::{sidebearings, svg_path};
use kernest_core::interpolate::{default_master, Interpolator};
use kernest_core::kerning::{edit_keys, pair_value, validate_side, with_pair, EditMode, PairKind};
use kernest_core::text::{parse, Token};
use kernest_core::vertical;
use kernest_core::{Font, KernPair, Layer, Metrics, MetricsKeys, Shape};
use serde::Serialize;
use tauri::State;

use crate::commands::{doc_mut, lock, status, AppState, CmdResult, DocStatus, Document};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)] // only built to send to the UI
pub enum TextItem {
    #[serde(rename_all = "camelCase")]
    Glyph {
        name: String,
        unicode: Option<String>,
        /// Drawn from the default master because its masters can't be interpolated.
        incompatible: bool,
        width: f64,
        lsb: Option<f64>,
        rsb: Option<f64>,
        left_group: Option<String>,
        right_group: Option<String>,
        /// Resolved right to left (bidi), for laying the line out.
        rtl: bool,
        /// Vertical advance and origin height (font units), for vertical text.
        v_advance: f64,
        v_origin: f64,
        tsb: Option<f64>,
        bsb: Option<f64>,
        /// Kerning with the next glyph on the line (0 when none, and between glyphs
        /// of different directions).
        kern: f64,
        kern_kind: Option<PairKind>,
        /// The pair sides the kerning came from, e.g. "@T" and "o".
        kern_left: Option<String>,
        kern_right: Option<String>,
        /// Metrics keys on the current master's layer: the keyboard spacing
        /// shortcuts leave linked sidebearings alone.
        #[serde(skip_serializing_if = "MetricsKeys::is_empty")]
        metrics_keys: MetricsKeys,
        svg: String,
        /// Colour layers (from the master), bottom to top.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        color: Vec<crate::commands::ColorLayerView>,
    },
    Missing {
        text: String,
        rtl: bool,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLayout {
    lines: Vec<Vec<TextItem>>,
    metrics: Metrics,
    /// The default colour palette, for drawing colour glyphs.
    palette: Vec<String>,
    status: DocStatus,
}

/// A glyph as drawn in the text view: outline, sidebearings, advance width, vertical
/// origin and advance, and top/bottom sidebearings.
#[derive(Clone)]
struct DrawnGlyph {
    svg: String,
    sb: Option<(f64, f64)>,
    width: f64,
    v_origin: f64,
    v_advance: f64,
    vsb: Option<(f64, f64)>,
}

/// A one-master copy of the glyphs needed to draw `names` (plus their components)
/// at an interpolated location. Glyphs that can't be interpolated keep their
/// default-master outline and are listed in the returned set.
fn interpolated_font(
    font: &Font,
    interp: &Interpolator,
    names: &[&str],
) -> (Font, HashSet<String>) {
    let def = default_master(font).clone();
    let mut needed: Vec<String> = Vec::new();
    let mut stack: Vec<String> = names.iter().map(|s| s.to_string()).collect();
    while let Some(n) = stack.pop() {
        if needed.contains(&n) {
            continue;
        }
        if let Some(l) = font.glyph(&n).and_then(|g| g.layer(&def.id)) {
            for s in &l.shapes {
                if let Shape::Component { base, .. } = s {
                    stack.push(base.clone());
                }
            }
            needed.push(n);
        }
    }
    let mut bad = HashSet::new();
    let glyphs = needed
        .iter()
        .filter_map(|n| {
            let g = font.glyph(n)?;
            let layer = interp.layer(font, n).unwrap_or_else(|_| {
                bad.insert(n.clone());
                g.layer(&def.id).cloned().expect("checked above")
            });
            let mut g = g.clone();
            g.layers = vec![layer];
            Some(g)
        })
        .collect();
    let scratch = Font {
        info: font.info.clone(),
        axes: font.axes.clone(),
        masters: vec![def],
        instances: Vec::new(),
        glyphs,
        kerning: Default::default(),
        features: String::new(),
        notes: String::new(),
        feature_settings: Default::default(),
        palettes: Vec::new(),
        master_palettes: Default::default(),
        filters: Vec::new(),
        variable_fonts: Vec::new(),
        export_presets: Vec::new(),
    };
    (scratch, bad)
}

fn layout(
    doc: &Document,
    text: &str,
    master_id: &str,
    location: Option<&Location>,
    direction: Option<TextDirection>,
) -> CmdResult<TextLayout> {
    let font = &doc.font;
    let master = font
        .master(master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    let tokens = parse(font, text);

    // Interpolated preview: draw from a one-master scratch font at the location.
    let interp = location.map(|l| Interpolator::new(font, l)).transpose()?;
    let all_names: Vec<&str> = tokens
        .iter()
        .flatten()
        .filter_map(|t| match t {
            Token::Glyph { name } => Some(name.as_str()),
            Token::Missing { .. } => None,
        })
        .collect();
    let (scratch, incompatible) = match &interp {
        Some(i) => interpolated_font(font, i, &all_names),
        None => (Font::new("", []), HashSet::new()),
    };
    let draw_font = if interp.is_some() { &scratch } else { font };
    let draw_master = if interp.is_some() {
        scratch.masters[0].id.clone()
    } else {
        master_id.to_string()
    };
    let metrics = match &interp {
        Some(i) => {
            let per: Vec<Vec<f64>> = font
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
        None => master.metrics,
    };
    let kern_at = |left: &str, right: &str| -> Option<(f64, PairKind, String, String)> {
        match &interp {
            None => pair_value(font, master_id, left, right),
            Some(i) => {
                let per: Vec<Vec<f64>> = font
                    .masters
                    .iter()
                    .map(|m| vec![pair_value(font, &m.id, left, right).map_or(0.0, |p| p.0)])
                    .collect();
                let v = i.values(&per)[0].round();
                let src = font
                    .masters
                    .iter()
                    .find_map(|m| pair_value(font, &m.id, left, right));
                src.map(|(_, kind, l, r)| (v, kind, l, r))
            }
        }
    };

    let Some(draw_m) = draw_font.master(&draw_master) else {
        return Err(format!("No master with id \"{draw_master}\""));
    };
    let vertical = direction == Some(TextDirection::Vertical);
    let mut svgs: HashMap<String, DrawnGlyph> = HashMap::new();
    let lines = tokens
        .into_iter()
        .map(|tokens| {
            let strong: Vec<Option<bool>> = tokens
                .iter()
                .map(|t| match t {
                    Token::Glyph { name } => font.glyph(name).and_then(bidi::strong_glyph),
                    Token::Missing { text } => text
                        .chars()
                        .next()
                        .and_then(|c| bidi::strong_codepoint(c as u32)),
                })
                .collect();
            let rtl = if vertical {
                vec![false; strong.len()]
            } else {
                bidi::resolve(&strong, TextDirection::paragraph_rtl(direction, &strong))
            };
            let names: Vec<Option<&str>> = tokens
                .iter()
                .map(|t| match t {
                    Token::Glyph { name } => Some(name.as_str()),
                    Token::Missing { .. } => None,
                })
                .collect();
            tokens
                .iter()
                .enumerate()
                .map(|(i, t)| match t {
                    Token::Missing { text } => TextItem::Missing {
                        text: text.clone(),
                        rtl: rtl[i],
                    },
                    Token::Glyph { name } => {
                        let g = font.glyph(name).expect("parsed glyphs exist");
                        let d = svgs
                            .entry(name.clone())
                            .or_insert_with(|| {
                                let empty = Layer::new(&draw_master, 0.0);
                                let l = draw_font.glyph(name).and_then(|g| g.layer(&draw_master));
                                let (v_origin, v_advance) =
                                    vertical::origin_and_advance(draw_m, l.unwrap_or(&empty));
                                DrawnGlyph {
                                    svg: l.map(|l| svg_path(draw_font, l)).unwrap_or_default(),
                                    sb: l.and_then(|l| sidebearings(draw_font, l)),
                                    width: l.map_or(0.0, |l| l.width),
                                    v_origin,
                                    v_advance,
                                    vsb: l
                                        .and_then(|l| vertical::sidebearings(draw_font, draw_m, l)),
                                }
                            })
                            .clone();
                        // Kerning applies between glyphs of the same direction.
                        let next = names
                            .get(i + 1)
                            .copied()
                            .flatten()
                            .filter(|_| rtl.get(i + 1) == Some(&rtl[i]));
                        let kern = next.and_then(|n| kern_at(name, n));
                        TextItem::Glyph {
                            name: name.clone(),
                            unicode: g.unicodes.first().map(|c| c.to_string()),
                            incompatible: incompatible.contains(name),
                            width: d.width,
                            lsb: d.sb.map(|s| s.0),
                            rsb: d.sb.map(|s| s.1),
                            rtl: rtl[i],
                            v_advance: d.v_advance,
                            v_origin: d.v_origin,
                            tsb: d.vsb.map(|s| s.0),
                            bsb: d.vsb.map(|s| s.1),
                            left_group: g.left_group.clone(),
                            right_group: g.right_group.clone(),
                            kern: kern.as_ref().map_or(0.0, |k| k.0),
                            kern_kind: kern.as_ref().map(|k| k.1),
                            kern_left: kern.as_ref().map(|k| k.2.clone()),
                            kern_right: kern.as_ref().map(|k| k.3.clone()),
                            metrics_keys: g
                                .layer(master_id)
                                .map(|l| l.metrics_keys.clone())
                                .unwrap_or_default(),
                            svg: d.svg,
                            color: crate::commands::color_views(font, g, master_id),
                        }
                    }
                })
                .collect()
        })
        .collect();
    Ok(TextLayout {
        lines,
        metrics,
        palette: crate::commands::css_palette(font),
        status: status(doc),
    })
}

#[tauri::command]
pub fn text_layout(
    state: State<AppState>,
    text: String,
    master_id: String,
    location: Option<Location>,
    direction: Option<String>,
) -> CmdResult<TextLayout> {
    let mut guard = lock(&state)?;
    layout(
        doc_mut(&mut guard)?,
        &text,
        &master_id,
        location.as_ref(),
        direction.as_deref().and_then(TextDirection::parse),
    )
}

fn mode_of(mode: &str) -> CmdResult<EditMode> {
    Ok(match mode {
        "group" => EditMode::Group,
        "leftException" => EditMode::LeftException,
        "rightException" => EditMode::RightException,
        "exception" => EditMode::Exception,
        other => return Err(format!("Unknown kerning mode \"{other}\"")),
    })
}

fn set_raw(
    doc: &mut Document,
    master_id: &str,
    left: &str,
    right: &str,
    value: f64,
) -> CmdResult<()> {
    if !value.is_finite() {
        return Err("Enter a number.".into());
    }
    let pairs = doc.font.kerning.get(master_id).cloned().unwrap_or_default();
    let next = with_pair(&pairs, left, right, value);
    if doc
        .history
        .apply_kerning(&mut doc.font, master_id, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KernEditResult {
    left: String,
    right: String,
    value: f64,
    status: DocStatus,
}

/// Sets the kerning between two glyphs in the text view. `mode` chooses whether
/// groups or the glyphs themselves are kerned: "group" | "leftException" |
/// "rightException" | "exception". A value of 0 removes the pair.
#[tauri::command]
pub fn set_glyph_kerning(
    state: State<AppState>,
    master_id: String,
    left: String,
    right: String,
    mode: String,
    value: f64,
) -> CmdResult<KernEditResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    for g in [&left, &right] {
        if doc.font.glyph(g).is_none() {
            return Err(format!("There is no glyph named \"{g}\"."));
        }
    }
    let (l, r) = edit_keys(&doc.font, &left, &right, mode_of(&mode)?);
    set_raw(doc, &master_id, &l, &r, value)?;
    Ok(KernEditResult {
        left: l,
        right: r,
        value: value.round(),
        status: status(doc),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KerningList {
    pairs: Vec<KernPair>,
    groups: Vec<String>,
    /// Glyphs in each group: as the left side of a pair (their right group) and as the
    /// right side (their left group).
    left_members: std::collections::BTreeMap<String, Vec<String>>,
    right_members: std::collections::BTreeMap<String, Vec<String>>,
    status: DocStatus,
}

#[tauri::command]
pub fn kerning_list(state: State<AppState>, master_id: String) -> CmdResult<KerningList> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    if doc.font.master(&master_id).is_none() {
        return Err(format!("No master with id \"{master_id}\""));
    }
    let mut groups: Vec<String> = doc
        .font
        .glyphs
        .iter()
        .flat_map(|g| [g.left_group.clone(), g.right_group.clone()])
        .flatten()
        .collect();
    groups.sort();
    groups.dedup();
    let mut left_members: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut right_members: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for g in &doc.font.glyphs {
        if let Some(r) = &g.right_group {
            left_members
                .entry(r.clone())
                .or_default()
                .push(g.name.clone());
        }
        if let Some(l) = &g.left_group {
            right_members
                .entry(l.clone())
                .or_default()
                .push(g.name.clone());
        }
    }
    Ok(KerningList {
        left_members,
        right_members,
        pairs: doc
            .font
            .kerning
            .get(&master_id)
            .cloned()
            .unwrap_or_default(),
        groups,
        status: status(doc),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KernTidyResult {
    tidy: kernest_core::kerning::KernTidy,
    status: DocStatus,
}

/// Kerning › Compress or Clean Up (S56), in one master or all of them, as one undo step.
#[tauri::command]
pub fn tidy_kerning(
    state: State<AppState>,
    master_id: String,
    all_masters: bool,
    compress: bool,
) -> CmdResult<KernTidyResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    if doc.font.master(&master_id).is_none() {
        return Err(format!("No master with id \"{master_id}\""));
    }
    let ids: Vec<String> = if all_masters {
        doc.font.masters.iter().map(|m| m.id.clone()).collect()
    } else {
        vec![master_id]
    };
    let mut next = doc.font.clone();
    let mut tidy = kernest_core::kerning::KernTidy::default();
    for id in ids {
        let pairs = next.kerning.get(&id).cloned().unwrap_or_default();
        let (out, t) = if compress {
            kernest_core::kerning::compress_pairs(&next, &pairs)
        } else {
            kernest_core::kerning::clean_up_pairs(&next, &pairs)
        };
        tidy += t;
        if out.is_empty() {
            next.kerning.remove(&id);
        } else {
            next.kerning.insert(id, out);
        }
    }
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(KernTidyResult {
        tidy,
        status: status(doc),
    })
}

/// Kerning › Paste: pairs copied from another master (or font). `overwrite` replaces
/// values already there; otherwise only missing pairs are added.
#[tauri::command]
pub fn paste_kerning(
    state: State<AppState>,
    master_id: String,
    pairs: Vec<KernPair>,
    overwrite: bool,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    for p in &pairs {
        validate_side(&doc.font, &p.left)?;
        validate_side(&doc.font, &p.right)?;
        if !p.value.is_finite() {
            return Err("A pasted kerning value isn't a number.".into());
        }
    }
    let existing = doc
        .font
        .kerning
        .get(&master_id)
        .cloned()
        .unwrap_or_default();
    let next = kernest_core::kerning::paste_pairs(&existing, &pairs, overwrite);
    if doc
        .history
        .apply_kerning(&mut doc.font, &master_id, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Sets a pair by its sides as written in the kerning window (glyph names or @groups).
#[tauri::command]
pub fn set_kerning_pair(
    state: State<AppState>,
    master_id: String,
    left: String,
    right: String,
    value: f64,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let (left, right) = (left.trim().to_string(), right.trim().to_string());
    validate_side(&doc.font, &left)?;
    validate_side(&doc.font, &right)?;
    set_raw(doc, &master_id, &left, &right, value)?;
    Ok(status(doc))
}

/// Sets a glyph's left and right kerning groups (empty clears).
#[tauri::command]
pub fn set_glyph_groups(
    state: State<AppState>,
    glyph: String,
    left_group: Option<String>,
    right_group: Option<String>,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    if doc
        .history
        .apply_groups(&mut doc.font, &glyph, (left_group, right_group))
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;

    #[test]
    fn right_to_left_layout_resolves_runs_and_vertical_metrics() {
        let mut f = sample_font();
        let mut beh = kernest_core::Glyph::new("beh-ar", vec![kernest_core::Codepoint(0x628)]);
        beh.layers.push(f.glyph("o").unwrap().layers[0].clone());
        f.glyphs.push(beh);
        let doc = Document::new(f, None);
        let items = |dir| {
            layout(&doc, "To \u{628}\u{628}", "m01", None, dir)
                .unwrap()
                .lines
                .remove(0)
        };
        let rtl_of = |items: &[TextItem]| -> Vec<bool> {
            items
                .iter()
                .map(|it| match it {
                    TextItem::Glyph { rtl, .. } | TextItem::Missing { rtl, .. } => *rtl,
                })
                .collect()
        };
        let line = items(Some(TextDirection::Rtl));
        // The space between Latin and Arabic takes the paragraph direction.
        assert_eq!(rtl_of(&line), [false, false, true, true, true]);
        // T–o keeps its kerning; o–space is between directions, so it has none.
        let kern = |it: &TextItem| match it {
            TextItem::Glyph { kern, .. } => *kern,
            TextItem::Missing { .. } => 0.0,
        };
        assert_eq!(kern(&line[0]), -60.0);
        assert_eq!(kern(&line[1]), 0.0);
        // Left to right, the space is left to right; with no choice the Latin first
        // letter makes the paragraph left to right too.
        assert_eq!(
            rtl_of(&items(Some(TextDirection::Ltr))),
            [false, false, false, true, true]
        );
        assert_eq!(rtl_of(&items(None)), [false, false, false, true, true]);
        // Vertical: nothing is right to left; the defaults are ascender − descender.
        let v = items(Some(TextDirection::Vertical));
        assert_eq!(rtl_of(&v), [false; 5]);
        match &v[0] {
            TextItem::Glyph {
                v_advance,
                v_origin,
                tsb,
                ..
            } => {
                let m = doc.font.masters[0].metrics;
                assert_eq!(*v_advance, m.ascender - m.descender);
                assert_eq!(*v_origin, m.ascender);
                assert!(tsb.is_some());
            }
            TextItem::Missing { .. } => panic!("T is a glyph"),
        }
    }

    #[test]
    fn layout_includes_kerning() {
        let doc = Document::new(sample_font(), None);
        // The sample font kerns T–o by -60 (glyph pair).
        let l = layout(&doc, "To\nH/nope", "m01", None, None).unwrap();
        assert_eq!(l.lines.len(), 2);
        match &l.lines[0][0] {
            TextItem::Glyph {
                name,
                kern,
                kern_kind,
                ..
            } => {
                assert_eq!(name, "T");
                assert_eq!(*kern, -60.0);
                assert_eq!(*kern_kind, Some(PairKind::Exception));
            }
            _ => panic!("expected a glyph"),
        }
        assert!(matches!(&l.lines[1][1], TextItem::Missing { text, .. } if text == "/nope"));
    }

    #[test]
    fn kerning_edit_respects_groups() {
        let mut doc = Document::new(sample_font(), None);
        doc.history
            .apply_groups(&mut doc.font, "T", (Some("T".into()), Some("T".into())))
            .unwrap();
        doc.history
            .apply_groups(&mut doc.font, "o", (Some("o".into()), Some("o".into())))
            .unwrap();
        let (l, r) = edit_keys(&doc.font, "T", "o", mode_of("group").unwrap());
        set_raw(&mut doc, "m01", &l, &r, -70.0).unwrap();
        assert!(doc.font.kerning["m01"]
            .iter()
            .any(|p| p.left == "@T" && p.right == "@o" && p.value == -70.0));
        // The existing T–o exception still wins in lookup.
        assert_eq!(pair_value(&doc.font, "m01", "T", "o").unwrap().0, -60.0);
        assert!(mode_of("sideways").is_err());
        assert!(set_raw(&mut doc, "m01", "T", "o", f64::NAN).is_err());
    }
}

#[cfg(test)]
mod interpolation_tests {
    use super::*;
    use kernest_core::sample::sample_font;
    use kernest_core::{structure, Axis};
    use std::collections::BTreeMap;

    #[test]
    fn layout_at_a_location_interpolates_widths_and_kerning() {
        let mut doc = Document::new(sample_font(), None);
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        };
        let f = structure::set_axes(&doc.font, vec![wght]).unwrap();
        let (mut f, bold) =
            structure::add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)]))
                .unwrap();
        f.glyph_mut("T").unwrap().layer_mut(&bold).unwrap().width += 100.0;
        for p in f.kerning.get_mut(&bold).unwrap() {
            if p.left == "T" && p.right == "o" {
                p.value = -100.0;
            }
        }
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let mid = BTreeMap::from([("wght".to_string(), 550.0)]);
        let l = layout(&doc, "To", "m01", Some(&mid), None).unwrap();
        match &l.lines[0][0] {
            TextItem::Glyph {
                width,
                kern,
                incompatible,
                ..
            } => {
                assert_eq!(*width, 650.0);
                assert_eq!(*kern, -80.0);
                assert!(!incompatible);
            }
            _ => panic!("expected a glyph"),
        }
    }
}
