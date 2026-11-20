//! Edit-view commands for guides, affine transforms, the background layer and the
//! measure tool. Each edit is one undoable step through the document history.

use kernest_core::model::{Anchor, Layer, Shape};
use kernest_core::transform::{self, Selection};
use kernest_core::{background, measure, structure, Guide};
use tauri::State;

use crate::commands::{
    doc_mut, ensure_glyph_unlocked, layer_view_of, lock, to_component, to_path, AppState,
    CmdResult, ComponentInput, Document, LayerView, PathDto,
};

/// The background as a layer, so the transform and measure engines can work on it.
fn background_layer(doc: &Document, glyph: &str, key: &str) -> CmdResult<Layer> {
    let layer = doc
        .font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
    let mut l = Layer::new(key, layer.width);
    if let Some(b) = &layer.background {
        l.shapes = b.shapes.clone();
        l.anchors = b.anchors.clone();
    }
    Ok(l)
}

fn set_guides_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    guides: Vec<Guide>,
) -> CmdResult<LayerView> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    structure::check_guides(&guides)?;
    let current = doc
        .font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
    let mut next = current.clone();
    next.guides = guides;
    if doc
        .history
        .apply(&mut doc.font, glyph, key, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// Replaces a layer's local guides.
#[tauri::command]
pub fn set_guides(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    guides: Vec<Guide>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    set_guides_in(doc, &glyph, &master_id, guides)
}

fn set_guide_scope_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    index: usize,
    to_global: bool,
) -> CmdResult<LayerView> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let next = structure::move_guide(&doc.font, glyph, key, index, to_global)
        .map_err(|e| e.to_string())?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// Moves a guide between the layer's local list and the master's global list
/// as one undoable step.
#[tauri::command]
pub fn set_guide_scope(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    index: usize,
    to_global: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    set_guide_scope_in(doc, &glyph, &master_id, index, to_global)
}

#[allow(clippy::too_many_arguments)]
fn transform_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    selection: &Selection,
    affine: &[f64; 6],
    origin: (f64, f64),
    all_masters: bool,
    on_background: bool,
) -> CmdResult<LayerView> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    if affine
        .iter()
        .chain([origin.0, origin.1].iter())
        .any(|v| !v.is_finite())
    {
        return Err("The transform has an invalid number.".into());
    }
    let next = if on_background {
        let mut pseudo = background_layer(doc, glyph, key)?;
        transform::apply(&mut pseudo, selection, affine, origin);
        background::updated(&doc.font, glyph, key, pseudo.shapes, pseudo.anchors)
            .map_err(|e| e.to_string())?
    } else {
        transform::transformed(
            &doc.font,
            glyph,
            key,
            selection,
            affine,
            origin,
            all_masters,
        )
        .map_err(|e| e.to_string())?
    };
    let changed = if !on_background && all_masters && doc.font.masters.len() > 1 {
        doc.history
            .apply_font(&mut doc.font, next)
            .map_err(|e| e.to_string())?
    } else {
        let layer = next
            .glyph(glyph)
            .and_then(|g| g.layer(key))
            .ok_or_else(|| format!("No layer for \"{glyph}\""))?
            .clone();
        doc.history
            .apply(&mut doc.font, glyph, key, layer)
            .map_err(|e| e.to_string())?
    };
    if changed {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// Scales, rotates, skews or mirrors the selection (or the whole glyph) around
/// `origin`, on one master or on every master the same way. With `on_background`,
/// the layer's background is transformed instead of its outline.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn transform(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    selection: Selection,
    affine: [f64; 6],
    origin: [f64; 2],
    all_masters: bool,
    on_background: bool,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    transform_in(
        doc,
        &glyph,
        &master_id,
        &selection,
        &affine,
        (origin[0], origin[1]),
        all_masters,
        on_background,
    )
}

fn background_edit_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    edit: background::Edit,
    selection: &Selection,
) -> CmdResult<LayerView> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let next =
        background::edited(&doc.font, glyph, key, edit, selection).map_err(|e| e.to_string())?;
    let layer = next
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?
        .clone();
    if doc
        .history
        .apply(&mut doc.font, glyph, key, layer)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// Moves the selection to / adds it to / swaps it with / clears the background.
#[tauri::command]
pub fn background_edit(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    edit: background::Edit,
    selection: Selection,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    background_edit_in(doc, &glyph, &master_id, edit, &selection)
}

fn measure_in(
    doc: &Document,
    glyph: &str,
    key: &str,
    p0: (f64, f64),
    p1: (f64, f64),
    on_background: bool,
) -> CmdResult<measure::Measurement> {
    let layer = if on_background {
        background_layer(doc, glyph, key)?
    } else {
        doc.font
            .glyph(glyph)
            .and_then(|g| g.layer(key))
            .ok_or_else(|| format!("No layer for \"{glyph}\""))?
            .clone()
    };
    Ok(measure::measure(&doc.font, &layer, p0, p1))
}

/// Where the segment `p0`–`p1` crosses the outline, and the gaps between crossings.
/// With `on_background`, the background's outlines are measured instead.
#[tauri::command]
pub fn measure(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    p0: [f64; 2],
    p1: [f64; 2],
    on_background: bool,
) -> CmdResult<measure::Measurement> {
    let guard = lock(&state)?;
    let doc = guard
        .as_ref()
        .ok_or_else(|| "No font is open.".to_string())?;
    measure_in(
        doc,
        &glyph,
        &master_id,
        (p0[0], p0[1]),
        (p1[0], p1[1]),
        on_background,
    )
}

fn update_background_in(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    paths: &[PathDto],
    components: Vec<ComponentInput>,
    anchors: Vec<Anchor>,
) -> CmdResult<LayerView> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let mut shapes = paths
        .iter()
        .map(|p| to_path(p).map(Shape::Path))
        .collect::<CmdResult<Vec<_>>>()?;
    for c in components {
        shapes.push(to_component(&doc.font, glyph, c)?);
    }
    let next =
        background::updated(&doc.font, glyph, key, shapes, anchors).map_err(|e| e.to_string())?;
    let layer = next
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("No layer for \"{glyph}\""))?
        .clone();
    if doc
        .history
        .apply(&mut doc.font, glyph, key, layer)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// Replaces the background's outlines and anchors (drawing while editing the
/// background). An empty drawing removes the background.
#[tauri::command]
pub fn update_background(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    paths: Vec<PathDto>,
    components: Vec<ComponentInput>,
    anchors: Vec<Anchor>,
) -> CmdResult<LayerView> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    update_background_in(doc, &glyph, &master_id, &paths, components, anchors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::model::{Node, NodeKind, Path, Shape};
    use kernest_core::sample::sample_font;
    use kernest_core::{Axis, Grid};
    use std::collections::BTreeMap;

    fn guide(x: f64, y: f64) -> Guide {
        Guide {
            x,
            y,
            angle: 0.0,
            name: None,
            locked: false,
        }
    }

    #[test]
    fn guides_round_trip_with_undo() {
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        let v = set_guides_in(&mut doc, "H", "m01", vec![guide(100.0, 0.0)]).unwrap();
        assert_eq!(v.guides.len(), 1);
        assert!(v.status.dirty && v.status.can_undo);
        assert!(set_guides_in(&mut doc, "H", "m01", vec![guide(f64::NAN, 0.0)]).is_err());
        assert!(set_guides_in(&mut doc, "nope", "m01", vec![]).is_err());

        // Global guides and the grid reach the layer view too.
        let next = structure::set_master_guides(&doc.font, "m01", vec![guide(0.0, 500.0)]).unwrap();
        doc.history.apply_font(&mut doc.font, next).unwrap();
        let next = structure::set_grid(
            &doc.font,
            Some(Grid {
                spacing: 100,
                subdivision: 2,
            }),
        )
        .unwrap();
        doc.history.apply_font(&mut doc.font, next).unwrap();
        let v = layer_view_of(&doc, "H", "m01").unwrap();
        assert_eq!(v.master_guides.len(), 1);
        assert_eq!(v.grid.unwrap().step(), Some(50.0));

        for _ in 0..3 {
            doc.history.undo(&mut doc.font);
        }
        assert_eq!(doc.font, original);
    }

    #[test]
    fn guide_scope_moves_both_ways_with_undo() {
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        set_guides_in(&mut doc, "H", "m01", vec![guide(100.0, 0.0)]).unwrap();
        let v = set_guide_scope_in(&mut doc, "H", "m01", 0, true).unwrap();
        assert!(v.guides.is_empty());
        assert_eq!(v.master_guides.len(), 1);
        assert_eq!(v.master_guides[0].x, 100.0);
        assert!(set_guide_scope_in(&mut doc, "H", "m01", 5, true).is_err());
        assert!(set_guide_scope_in(&mut doc, "nope", "m01", 0, false).is_err());
        let v = set_guide_scope_in(&mut doc, "H", "m01", 0, false).unwrap();
        assert_eq!(v.guides.len(), 1);
        assert!(v.master_guides.is_empty());
        for _ in 0..3 {
            doc.history.undo(&mut doc.font);
        }
        assert_eq!(doc.font, original);
    }

    #[test]
    fn transform_one_layer_undoes_exactly() {
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        let before = doc.font.glyph("H").unwrap().layer("m01").unwrap().clone();
        let v = transform_in(
            &mut doc,
            "H",
            "m01",
            &Selection::default(),
            &transform::scale(2.0, 1.0),
            (0.0, 0.0),
            false,
            false,
        )
        .unwrap();
        let after = doc.font.glyph("H").unwrap().layer("m01").unwrap();
        assert_eq!(after.shapes.len(), before.shapes.len());
        assert_eq!(v.width, before.width);
        assert!(v.status.can_undo);
        doc.history.undo(&mut doc.font);
        assert_eq!(doc.font, original);
    }

    #[test]
    fn transform_all_masters_is_one_font_step() {
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
        let next = structure::set_axes(&doc.font, vec![wght]).unwrap();
        doc.history.apply_font(&mut doc.font, next).unwrap();
        let (next, _) = structure::add_master(
            &doc.font,
            "m01",
            "Bold",
            &BTreeMap::from([("wght".into(), 700.0)]),
        )
        .unwrap();
        doc.history.apply_font(&mut doc.font, next).unwrap();
        let original = doc.font.clone();
        transform_in(
            &mut doc,
            "H",
            "m01",
            &Selection::default(),
            &transform::mirror(true, false),
            (350.0, 0.0),
            true,
            false,
        )
        .unwrap();
        for m in ["m01", "m02"] {
            assert_ne!(
                doc.font.glyph("H").unwrap().layer(m).unwrap(),
                original.glyph("H").unwrap().layer(m).unwrap()
            );
        }
        assert_eq!(
            doc.history.undo(&mut doc.font),
            Some(kernest_core::edit::Affected::Font)
        );
        assert_eq!(doc.font, original);
    }

    #[test]
    fn background_moves_swap_and_clear() {
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        let drawn = doc
            .font
            .glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .shapes
            .len();
        let sel = Selection::default();
        let v =
            background_edit_in(&mut doc, "H", "m01", background::Edit::ToBackground, &sel).unwrap();
        assert!(v.paths.is_empty() && v.components.is_empty());
        assert_eq!(v.background.as_ref().unwrap().paths.len(), drawn);
        let v = background_edit_in(&mut doc, "H", "m01", background::Edit::Swap, &sel).unwrap();
        assert_eq!(v.paths.len(), drawn);
        assert!(v.background.is_none());
        background_edit_in(&mut doc, "H", "m01", background::Edit::ToBackground, &sel).unwrap();
        let v = background_edit_in(&mut doc, "H", "m01", background::Edit::Clear, &sel).unwrap();
        assert!(v.paths.is_empty() && v.background.is_none());
        for _ in 0..4 {
            doc.history.undo(&mut doc.font);
        }
        assert_eq!(doc.font, original);
    }

    #[test]
    fn measure_reports_crossings() {
        let mut doc = Document::new(sample_font(), None);
        doc.font
            .glyph_mut("H")
            .unwrap()
            .layer_mut("m01")
            .unwrap()
            .shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(100.0, 0.0, NodeKind::Line),
                Node::new(100.0, 200.0, NodeKind::Line),
                Node::new(0.0, 200.0, NodeKind::Line),
            ],
        })];
        let m = measure_in(&doc, "H", "m01", (-50.0, 100.0), (150.0, 100.0), false).unwrap();
        assert_eq!(m.hits.len(), 2);
        assert_eq!(m.distances.len(), 1);
        assert!((m.distances[0] - 100.0).abs() < 1e-9);
        assert!(measure_in(&doc, "nope", "m01", (0.0, 0.0), (1.0, 0.0), false).is_err());
    }

    #[test]
    fn background_draw_transform_and_measure() {
        let mut doc = Document::new(sample_font(), None);
        let original = doc.font.clone();
        let rect: PathDto = serde_json::from_value(serde_json::json!({
            "closed": true,
            "nodes": [
                { "x": 0.0, "y": 0.0, "kind": "line" },
                { "x": 100.0, "y": 0.0, "kind": "line" },
                { "x": 100.0, "y": 200.0, "kind": "line" },
                { "x": 0.0, "y": 200.0, "kind": "line" }
            ]
        }))
        .unwrap();
        let v = update_background_in(&mut doc, "H", "m01", &[rect], vec![], vec![]).unwrap();
        assert_eq!(v.background.as_ref().unwrap().paths.len(), 1);
        assert_eq!(
            v.paths.len(),
            original
                .glyph("H")
                .unwrap()
                .layer("m01")
                .unwrap()
                .shapes
                .len()
        );

        // The transform and measure act on the background, not the outline.
        transform_in(
            &mut doc,
            "H",
            "m01",
            &Selection::default(),
            &transform::scale(2.0, 1.0),
            (0.0, 0.0),
            false,
            true,
        )
        .unwrap();
        assert_eq!(
            doc.font.glyph("H").unwrap().layer("m01").unwrap().shapes,
            original.glyph("H").unwrap().layer("m01").unwrap().shapes
        );
        let m = measure_in(&doc, "H", "m01", (-50.0, 100.0), (250.0, 100.0), true).unwrap();
        assert_eq!(m.hits.len(), 2);
        assert!((m.distances[0] - 200.0).abs() < 1e-9);

        // An empty drawing removes the background; undo brings every step back.
        let v = update_background_in(&mut doc, "H", "m01", &[], vec![], vec![]).unwrap();
        assert!(v.background.is_none());
        for _ in 0..3 {
            doc.history.undo(&mut doc.font);
        }
        assert_eq!(doc.font, original);
    }
}
