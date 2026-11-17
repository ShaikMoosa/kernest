//! Moving outline work aside into a layer's background: a reference drawing that
//! is shown behind the outline but never exports.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Background, Font, Layer, Shape};
use crate::transform::Selection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Edit {
    /// Replace the background with the selection.
    ToBackground,
    /// Append the selection to the background.
    AddToBackground,
    /// Exchange foreground and background.
    Swap,
    /// Drop the background.
    Clear,
}

/// Splits off the selected shapes and anchors. An empty selection takes everything;
/// a path counts as selected when any of its nodes is.
fn take(layer: &mut Layer, sel: &Selection) -> (Vec<Shape>, Vec<crate::model::Anchor>) {
    let whole = sel.is_empty();
    let mut kept = Vec::new();
    let mut taken = Vec::new();
    let (mut pi, mut ci) = (0usize, 0usize);
    for shape in layer.shapes.drain(..) {
        let selected = match &shape {
            Shape::Path(_) => {
                let s = whole || sel.nodes.iter().any(|&(p, _)| p == pi);
                pi += 1;
                s
            }
            Shape::Component { .. } => {
                let s = whole || sel.components.contains(&ci);
                ci += 1;
                s
            }
        };
        if selected {
            taken.push(shape);
        } else {
            kept.push(shape);
        }
    }
    layer.shapes = kept;
    let mut kept_anchors = Vec::new();
    let mut taken_anchors = Vec::new();
    for a in layer.anchors.drain(..) {
        if whole || sel.anchors.contains(&a.name) {
            taken_anchors.push(a);
        } else {
            kept_anchors.push(a);
        }
    }
    layer.anchors = kept_anchors;
    (taken, taken_anchors)
}

pub fn selection_to_background(layer: &mut Layer, sel: &Selection) {
    let (shapes, anchors) = take(layer, sel);
    layer.background = Some(Background { shapes, anchors });
}

pub fn add_selection_to_background(layer: &mut Layer, sel: &Selection) {
    let (shapes, anchors) = take(layer, sel);
    let bg = layer.background.get_or_insert_with(Background::default);
    bg.shapes.extend(shapes);
    bg.anchors.extend(anchors);
}

pub fn swap_with_background(layer: &mut Layer) {
    let bg = layer.background.take().unwrap_or_default();
    let old = Background {
        shapes: std::mem::take(&mut layer.shapes),
        anchors: std::mem::take(&mut layer.anchors),
    };
    layer.shapes = bg.shapes;
    layer.anchors = bg.anchors;
    layer.background = if old.is_empty() { None } else { Some(old) };
}

pub fn clear_background(layer: &mut Layer) {
    layer.background = None;
}

/// Returns `font` with the background edit applied to one layer.
pub fn edited(font: &Font, glyph: &str, key: &str, edit: Edit, sel: &Selection) -> Result<Font> {
    let mut next = font.clone();
    let layer = next
        .glyph_mut(glyph)
        .ok_or_else(|| Error::Invalid(format!("no glyph named \"{glyph}\"")))?
        .layer_mut(key)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "glyph \"{glyph}\" has no layer for master \"{key}\""
            ))
        })?;
    match edit {
        Edit::ToBackground => selection_to_background(layer, sel),
        Edit::AddToBackground => add_selection_to_background(layer, sel),
        Edit::Swap => swap_with_background(layer),
        Edit::Clear => clear_background(layer),
    }
    Ok(next)
}

/// Replaces the background's contents (drawn on while editing the background).
/// An empty drawing removes the background.
pub fn updated(
    font: &Font,
    glyph: &str,
    key: &str,
    shapes: Vec<Shape>,
    anchors: Vec<crate::model::Anchor>,
) -> Result<Font> {
    let mut next = font.clone();
    let layer = next
        .glyph_mut(glyph)
        .ok_or_else(|| Error::Invalid(format!("no glyph named \"{glyph}\"")))?
        .layer_mut(key)
        .ok_or_else(|| {
            Error::Invalid(format!(
                "glyph \"{glyph}\" has no layer for master \"{key}\""
            ))
        })?;
    layer.background = Some(Background { shapes, anchors }).filter(|b| !b.is_empty());
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Node, NodeKind, Path};
    use crate::sample::sample_font;

    fn two_rects() -> Layer {
        let mut l = Layer::new("m01", 600.0);
        let rect = |x0: f64| {
            Shape::Path(Path {
                closed: true,
                nodes: vec![
                    Node::new(x0, 0.0, NodeKind::Line),
                    Node::new(x0 + 90.0, 0.0, NodeKind::Line),
                    Node::new(x0 + 90.0, 700.0, NodeKind::Line),
                    Node::new(x0, 700.0, NodeKind::Line),
                ],
            })
        };
        l.shapes = vec![rect(80.0), rect(510.0)];
        l.anchors = vec![
            Anchor {
                name: "top".into(),
                x: 125.0,
                y: 700.0,
            },
            Anchor {
                name: "bottom".into(),
                x: 125.0,
                y: 0.0,
            },
        ];
        l
    }

    #[test]
    fn selection_to_background_moves_only_selection() {
        let mut l = two_rects();
        let sel = Selection {
            nodes: vec![(1, 0)],
            components: vec![],
            anchors: vec!["top".into()],
        };
        selection_to_background(&mut l, &sel);
        assert_eq!(l.shapes.len(), 1);
        assert_eq!(l.anchors.len(), 1);
        assert_eq!(l.anchors[0].name, "bottom");
        let bg = l.background.as_ref().unwrap();
        assert_eq!(bg.shapes.len(), 1);
        assert_eq!(bg.anchors.len(), 1);
        assert_eq!(bg.anchors[0].name, "top");
    }

    #[test]
    fn empty_selection_moves_everything() {
        let mut l = two_rects();
        selection_to_background(&mut l, &Selection::default());
        assert!(l.shapes.is_empty() && l.anchors.is_empty());
        assert_eq!(l.background.as_ref().unwrap().shapes.len(), 2);
    }

    #[test]
    fn add_appends_to_existing_background() {
        let mut l = two_rects();
        let first = Selection {
            nodes: vec![(0, 0)],
            components: vec![],
            anchors: vec![],
        };
        selection_to_background(&mut l, &first);
        let second = Selection {
            nodes: vec![(0, 0)],
            components: vec![],
            anchors: vec![],
        };
        add_selection_to_background(&mut l, &second);
        assert!(l.shapes.is_empty());
        assert_eq!(l.background.as_ref().unwrap().shapes.len(), 2);
    }

    #[test]
    fn swap_exchanges_both_ways() {
        let mut l = two_rects();
        selection_to_background(
            &mut l,
            &Selection {
                nodes: vec![(1, 0)],
                components: vec![],
                anchors: vec![],
            },
        );
        swap_with_background(&mut l);
        assert_eq!(l.shapes.len(), 1);
        assert_eq!(l.background.as_ref().unwrap().shapes.len(), 1);
        swap_with_background(&mut l);
        assert_eq!(l.shapes.len(), 1);
        assert_eq!(l.background.as_ref().unwrap().shapes.len(), 1);
    }

    #[test]
    fn swap_with_empty_background_moves_foreground_aside() {
        let mut l = two_rects();
        swap_with_background(&mut l);
        assert!(l.shapes.is_empty());
        assert_eq!(l.background.as_ref().unwrap().shapes.len(), 2);
        swap_with_background(&mut l);
        assert_eq!(l.shapes.len(), 2);
        assert!(l.background.is_none());
    }

    #[test]
    fn clear_drops_background() {
        let mut l = two_rects();
        selection_to_background(&mut l, &Selection::default());
        clear_background(&mut l);
        assert!(l.background.is_none());
        assert!(l.shapes.is_empty());
    }

    #[test]
    fn background_edit_is_one_undoable_step() {
        let mut f = sample_font();
        let mut hist = crate::edit::History::default();
        let next = edited(&f, "H", "m01", Edit::ToBackground, &Selection::default()).unwrap();
        assert!(hist.apply_font(&mut f, next).unwrap());
        assert!(f
            .glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .background
            .is_some());
        hist.undo(&mut f);
        assert!(f
            .glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .background
            .is_none());
        assert_eq!(f.glyph("H").unwrap().layer("m01").unwrap().shapes.len(), 3);
    }

    #[test]
    fn updated_replaces_contents_and_empty_removes() {
        let f = sample_font();
        let rect = Shape::Path(Path {
            closed: true,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(10.0, 0.0, NodeKind::Line),
                Node::new(10.0, 10.0, NodeKind::Line),
            ],
        });
        let next = updated(&f, "H", "m01", vec![rect], vec![]).unwrap();
        let bg = next
            .glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .background
            .as_ref()
            .unwrap();
        assert_eq!(bg.shapes.len(), 1);
        assert_eq!(
            next.glyph("H").unwrap().layer("m01").unwrap().shapes.len(),
            3
        );
        let next = updated(&next, "H", "m01", vec![], vec![]).unwrap();
        assert!(next
            .glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .background
            .is_none());
        assert!(updated(&f, "nope", "m01", vec![], vec![]).is_err());
    }
}
