//! Commands for the text (spacing) view and the kerning window.

use std::collections::HashMap;

use kernest_core::geometry::{sidebearings, svg_path};
use kernest_core::kerning::{edit_keys, pair_value, validate_side, with_pair, EditMode, PairKind};
use kernest_core::text::{parse, Token};
use kernest_core::{KernPair, Metrics};
use serde::Serialize;
use tauri::State;

use crate::commands::{doc_mut, lock, status, AppState, CmdResult, DocStatus, Document};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TextItem {
    #[serde(rename_all = "camelCase")]
    Glyph {
        name: String,
        unicode: Option<String>,
        width: f64,
        lsb: Option<f64>,
        rsb: Option<f64>,
        left_group: Option<String>,
        right_group: Option<String>,
        /// Kerning with the next glyph on the line (0 when none).
        kern: f64,
        kern_kind: Option<PairKind>,
        /// The pair sides the kerning came from, e.g. "@T" and "o".
        kern_left: Option<String>,
        kern_right: Option<String>,
        svg: String,
    },
    Missing {
        text: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLayout {
    lines: Vec<Vec<TextItem>>,
    metrics: Metrics,
    status: DocStatus,
}

fn layout(doc: &Document, text: &str, master_id: &str) -> CmdResult<TextLayout> {
    let font = &doc.font;
    let master = font
        .master(master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    let mut svgs: HashMap<String, (String, Option<(f64, f64)>)> = HashMap::new();
    let lines = parse(font, text)
        .into_iter()
        .map(|tokens| {
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
                    Token::Missing { text } => TextItem::Missing { text: text.clone() },
                    Token::Glyph { name } => {
                        let g = font.glyph(name).expect("parsed glyphs exist");
                        let layer = g.layer(master_id);
                        let (svg, sb) = svgs
                            .entry(name.clone())
                            .or_insert_with(|| match layer {
                                Some(l) => (svg_path(font, l), sidebearings(font, l)),
                                None => (String::new(), None),
                            })
                            .clone();
                        let next = names.get(i + 1).copied().flatten();
                        let kern = next.and_then(|n| pair_value(font, master_id, name, n));
                        TextItem::Glyph {
                            name: name.clone(),
                            unicode: g.unicodes.first().map(|c| c.to_string()),
                            width: layer.map_or(0.0, |l| l.width),
                            lsb: sb.map(|s| s.0),
                            rsb: sb.map(|s| s.1),
                            left_group: g.left_group.clone(),
                            right_group: g.right_group.clone(),
                            kern: kern.as_ref().map_or(0.0, |k| k.0),
                            kern_kind: kern.as_ref().map(|k| k.1),
                            kern_left: kern.as_ref().map(|k| k.2.clone()),
                            kern_right: kern.as_ref().map(|k| k.3.clone()),
                            svg,
                        }
                    }
                })
                .collect()
        })
        .collect();
    Ok(TextLayout {
        lines,
        metrics: master.metrics,
        status: status(doc),
    })
}

#[tauri::command]
pub fn text_layout(
    state: State<AppState>,
    text: String,
    master_id: String,
) -> CmdResult<TextLayout> {
    let mut guard = lock(&state)?;
    layout(doc_mut(&mut guard)?, &text, &master_id)
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
    Ok(KerningList {
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
    fn layout_includes_kerning() {
        let doc = Document::new(sample_font(), None);
        // The sample font kerns T–o by -60 (glyph pair).
        let l = layout(&doc, "To\nH/nope", "m01").unwrap();
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
        assert!(matches!(&l.lines[1][1], TextItem::Missing { text } if text == "/nope"));
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
