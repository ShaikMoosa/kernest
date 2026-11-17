//! Anchor and component-alignment commands. Each edit is composed with the
//! alignment it triggers, so one undo step restores both the edit and the
//! components that moved because of it.

use kernest_core::anchors;
use kernest_core::model::{Anchor, Font, Layer, Shape};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{
    doc_mut, layer_view_of, lock, status, AppState, CmdResult, DocStatus, Document, LayerView,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorMove {
    name: String,
    dx: f64,
    dy: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultAnchorsResult {
    /// How many of the named glyphs received anchors.
    updated: usize,
    status: DocStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositesResult {
    built: Vec<String>,
    skipped: Vec<String>,
    status: DocStatus,
}

/// The single layer that differs between two fonts, if exactly one does and
/// nothing else changed. Such edits undo as a layer change, not a font change.
fn lone_layer(before: &Font, after: &Font) -> Option<(String, String, Layer)> {
    let mut probe = after.clone();
    probe.glyphs = before.glyphs.clone();
    if probe != *before || before.glyphs.len() != after.glyphs.len() {
        return None;
    }
    let mut hit: Option<(String, String, Layer)> = None;
    for (b, a) in before.glyphs.iter().zip(&after.glyphs) {
        if b == a {
            continue;
        }
        if b.name != a.name || b.layers.len() != a.layers.len() || hit.is_some() {
            return None;
        }
        for (bl, al) in b.layers.iter().zip(&a.layers) {
            if bl == al {
                continue;
            }
            if hit.is_some() {
                return None;
            }
            hit = Some((a.name.clone(), al.key().to_string(), al.clone()));
        }
    }
    hit
}

/// Records an edit as one undo step and returns the addressed layer's view.
fn apply_edit(doc: &mut Document, next: Font, glyph: &str, key: &str) -> CmdResult<LayerView> {
    let changed = match lone_layer(&doc.font, &next) {
        Some((g, k, layer)) if g == glyph && k == key => doc
            .history
            .apply(&mut doc.font, &g, &k, layer)
            .map_err(|e| e.to_string())?,
        _ => doc
            .history
            .apply_font(&mut doc.font, next)
            .map_err(|e| e.to_string())?,
    };
    if changed {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

fn finite(values: &[f64]) -> CmdResult<()> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err("An anchor has an invalid coordinate.".into())
    }
}

fn add_anchor_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    anchor: &Anchor,
    all_masters: bool,
) -> CmdResult<LayerView> {
    finite(&[anchor.x, anchor.y])?;
    let next = anchors::add_anchor(&doc.font, glyph, key, anchor, all_masters)?;
    let next = anchors::align_dependents(&next, glyph)?;
    apply_edit(doc, next, glyph, key)
}

#[tauri::command]
pub fn add_anchor(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    name: String,
    x: f64,
    y: f64,
    all_masters: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    add_anchor_in(doc, &glyph, &master_id, &Anchor { name, x, y }, all_masters)
}

fn move_anchors_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    moves: &[AnchorMove],
    all_masters: bool,
) -> CmdResult<LayerView> {
    for m in moves {
        finite(&[m.dx, m.dy])?;
    }
    let moves: Vec<(String, f64, f64)> =
        moves.iter().map(|m| (m.name.clone(), m.dx, m.dy)).collect();
    let next = anchors::move_anchors(&doc.font, glyph, key, &moves, all_masters)?;
    let next = anchors::align_dependents(&next, glyph)?;
    apply_edit(doc, next, glyph, key)
}

#[tauri::command]
pub fn move_anchors(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    moves: Vec<AnchorMove>,
    all_masters: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    move_anchors_in(doc, &glyph, &master_id, &moves, all_masters)
}

fn rename_anchor_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    old: &str,
    new: &str,
    all_masters: bool,
) -> CmdResult<LayerView> {
    let next = anchors::rename_anchor(&doc.font, glyph, key, old, new, all_masters)?;
    let next = anchors::align_dependents(&next, glyph)?;
    apply_edit(doc, next, glyph, key)
}

#[tauri::command]
pub fn rename_anchor(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    old_name: String,
    new_name: String,
    all_masters: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    rename_anchor_in(doc, &glyph, &master_id, &old_name, &new_name, all_masters)
}

fn remove_anchors_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    names: &[String],
    all_masters: bool,
) -> CmdResult<LayerView> {
    let next = anchors::remove_anchors(&doc.font, glyph, key, names, all_masters)?;
    let next = anchors::align_dependents(&next, glyph)?;
    apply_edit(doc, next, glyph, key)
}

#[tauri::command]
pub fn remove_anchors(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    names: Vec<String>,
    all_masters: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    remove_anchors_in(doc, &glyph, &master_id, &names, all_masters)
}

fn set_default_anchors_in(
    doc: &mut Document,
    glyphs: &[String],
) -> CmdResult<DefaultAnchorsResult> {
    let mut next = anchors::set_default_anchors(&doc.font, glyphs)?;
    let mut updated = 0;
    for name in glyphs {
        if doc.font.glyph(name) != next.glyph(name) {
            updated += 1;
        }
        next = anchors::align_dependents(&next, name)?;
    }
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(DefaultAnchorsResult {
        updated,
        status: status(doc),
    })
}

#[tauri::command]
pub fn set_default_anchors(
    state: State<AppState>,
    glyphs: Vec<String>,
) -> CmdResult<DefaultAnchorsResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    set_default_anchors_in(doc, &glyphs)
}

fn set_component_align_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    index: usize,
    align: bool,
) -> CmdResult<LayerView> {
    let mut next = doc.font.clone();
    let layer = next
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?
        .layer_mut(key)
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{key}\"."))?;
    match layer.shapes.get_mut(index) {
        Some(Shape::Component { align: a, .. }) => *a = align,
        _ => return Err("There is no such component.".into()),
    }
    let next = anchors::align_glyph(&next, glyph)?;
    let next = anchors::align_dependents(&next, glyph)?;
    apply_edit(doc, next, glyph, key)
}

#[tauri::command]
pub fn set_component_align(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    index: usize,
    align: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    set_component_align_in(doc, &glyph, &master_id, index, align)
}

fn create_composites_in(doc: &mut Document, names: &[String]) -> CmdResult<CompositesResult> {
    let r = anchors::create_composites(&doc.font, names)?;
    if doc
        .history
        .apply_font(&mut doc.font, r.font)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(CompositesResult {
        built: r.built,
        skipped: r.skipped,
        status: status(doc),
    })
}

#[tauri::command]
pub fn create_composites(
    state: State<AppState>,
    names: Vec<String>,
) -> CmdResult<CompositesResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    create_composites_in(doc, &names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::glyphset::{add_glyphs, parse_glyph_list};
    use kernest_core::sample::sample_font;

    fn doc_with_accents() -> Document {
        let mut doc = Document::new(sample_font(), None);
        let r = add_glyphs(&doc.font, &parse_glyph_list("acutecomb Aacute").unwrap()).unwrap();
        doc.font = r.font;
        let next =
            anchors::set_default_anchors(&doc.font, &["A".into(), "acutecomb".into()]).unwrap();
        doc.font = next;
        let r = anchors::create_composites(&doc.font, &["Aacute".into()]).unwrap();
        doc.font = r.font;
        doc
    }

    fn anchor<'a>(view: &'a LayerView, name: &str) -> &'a kernest_core::Anchor {
        view.anchors.iter().find(|a| a.name == name).unwrap()
    }

    #[test]
    fn anchor_edits_round_trip_with_undo() {
        let mut doc = Document::new(sample_font(), None);
        let a = Anchor {
            name: "top".into(),
            x: 340.0,
            y: 700.0,
        };
        let v = add_anchor_in(&mut doc, "H", "m01", &a, false).unwrap();
        assert_eq!(anchor(&v, "top").x, 340.0);
        assert!(v.status.dirty && v.status.can_undo);

        let moves = vec![AnchorMove {
            name: "top".into(),
            dx: 10.0,
            dy: -20.0,
        }];
        let v = move_anchors_in(&mut doc, "H", "m01", &moves, false).unwrap();
        assert_eq!((anchor(&v, "top").x, anchor(&v, "top").y), (350.0, 680.0));

        let v = rename_anchor_in(&mut doc, "H", "m01", "top", "top_1", false).unwrap();
        assert!(v.anchors.iter().all(|a| a.name != "top"));
        assert_eq!(anchor(&v, "top_1").x, 350.0);

        let v = remove_anchors_in(&mut doc, "H", "m01", &["top_1".into()], false).unwrap();
        assert!(v.anchors.is_empty());

        // Each edit is one undo step, back to the untouched layer.
        for _ in 0..4 {
            doc.history.undo(&mut doc.font);
        }
        assert!(doc
            .font
            .glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .anchors
            .is_empty());
        assert!(!doc.history.can_undo());
    }

    #[test]
    fn moving_a_base_anchor_realigns_composites_in_one_step() {
        let mut doc = doc_with_accents();
        let a_before = doc.font.glyph("A").unwrap().layer("m01").unwrap().clone();
        let before = doc
            .font
            .glyph("Aacute")
            .unwrap()
            .layer("m01")
            .unwrap()
            .clone();
        let top_x = a_before.anchors.iter().find(|a| a.name == "top").unwrap().x;
        let moves = vec![AnchorMove {
            name: "top".into(),
            dx: 25.0,
            dy: 0.0,
        }];
        let v = move_anchors_in(&mut doc, "A", "m01", &moves, false).unwrap();
        assert_eq!(anchor(&v, "top").x, top_x + 25.0);
        let after = doc.font.glyph("Aacute").unwrap().layer("m01").unwrap();
        let dx = match (&before.shapes[1], &after.shapes[1]) {
            (Shape::Component { transform: b, .. }, Shape::Component { transform: a, .. }) => {
                a[4] - b[4]
            }
            _ => panic!("expected components"),
        };
        assert_eq!(dx, 25.0);
        // One undo restores both the anchor and the component position.
        doc.history.undo(&mut doc.font);
        assert_eq!(
            doc.font.glyph("Aacute").unwrap().layer("m01").unwrap(),
            &before
        );
        assert_eq!(
            doc.font.glyph("A").unwrap().layer("m01").unwrap(),
            &a_before
        );
    }

    #[test]
    fn component_align_toggle_moves_and_restores() {
        let mut doc = doc_with_accents();
        let aligned = doc
            .font
            .glyph("Aacute")
            .unwrap()
            .layer("m01")
            .unwrap()
            .clone();
        let v = set_component_align_in(&mut doc, "Aacute", "m01", 1, false).unwrap();
        assert!(!v.components[1].align);
        // Manual placement keeps the position; only the flag changes.
        let transform = |l: &Layer| match &l.shapes[1] {
            Shape::Component { transform, .. } => *transform,
            _ => panic!("expected a component"),
        };
        let manual = doc.font.glyph("Aacute").unwrap().layer("m01").unwrap();
        assert_eq!(transform(manual), transform(&aligned));
        let v = set_component_align_in(&mut doc, "Aacute", "m01", 1, true).unwrap();
        assert!(v.components[1].align);
        assert!(set_component_align_in(&mut doc, "Aacute", "m01", 9, true).is_err());
    }

    #[test]
    fn default_anchors_and_composites_commands() {
        let mut doc = Document::new(sample_font(), None);
        let r = add_glyphs(&doc.font, &parse_glyph_list("acutecomb").unwrap()).unwrap();
        doc.font = r.font;
        let r = set_default_anchors_in(&mut doc, &["A".into(), "acutecomb".into()]).unwrap();
        assert_eq!(r.updated, 2);
        assert!(r.status.can_undo);
        let r = set_default_anchors_in(&mut doc, &["A".into()]).unwrap();
        assert_eq!(r.updated, 0);

        let r = create_composites_in(&mut doc, &["Aacute".into(), "H".into()]).unwrap();
        assert_eq!(r.built, vec!["Aacute"]);
        assert_eq!(r.skipped, vec!["H"]);
        let g = doc.font.glyph("Aacute").unwrap();
        assert_eq!(g.unicodes, vec![kernest_core::Codepoint(0xC1)]);
        let l = g.layer("m01").unwrap();
        assert_eq!(l.shapes.len(), 2);
        assert!(l.anchors.iter().any(|a| a.name == "top"));
        // The acute sits on A's top anchor, not at the origin.
        let acute = match &l.shapes[1] {
            Shape::Component { transform, .. } => transform,
            _ => panic!("expected a component"),
        };
        assert_ne!(acute[5], 0.0);
    }
}
