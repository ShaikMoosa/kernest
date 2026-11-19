//! Affine transforms of a layer's outline, components and anchors, plus the snap
//! grid rounding used when a grid is set.
//!
//! Affines use the model's `[xx, xy, yx, yy, dx, dy]` layout: a point maps to
//! `(xx*x + yx*y + dx, xy*x + yy*y + dy)`.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{Font, Grid, Layer, Shape, IDENTITY};

pub type Affine = [f64; 6];

/// What a transform acts on. An empty selection means the whole layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    /// `(path index, node index)` among the layer's paths, in order.
    #[serde(default)]
    pub nodes: Vec<(usize, usize)>,
    /// Component indices among the layer's components, in order.
    #[serde(default)]
    pub components: Vec<usize>,
    #[serde(default)]
    pub anchors: Vec<String>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.components.is_empty() && self.anchors.is_empty()
    }
}

/// Applies `b` first, then `a`.
pub fn multiply(a: &Affine, b: &Affine) -> Affine {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}

pub fn map_point(m: &Affine, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

pub fn scale(sx: f64, sy: f64) -> Affine {
    [sx, 0.0, 0.0, sy, 0.0, 0.0]
}

pub fn rotate(deg: f64) -> Affine {
    let r = deg.to_radians();
    let (s, c) = (r.sin(), r.cos());
    [c, s, -s, c, 0.0, 0.0]
}

/// Slant in degrees: `x` leans verticals, `y` leans horizontals.
pub fn skew(x_deg: f64, y_deg: f64) -> Affine {
    [
        1.0,
        y_deg.to_radians().tan(),
        x_deg.to_radians().tan(),
        1.0,
        0.0,
        0.0,
    ]
}

pub fn mirror(horizontal: bool, vertical: bool) -> Affine {
    [
        if horizontal { -1.0 } else { 1.0 },
        0.0,
        0.0,
        if vertical { -1.0 } else { 1.0 },
        0.0,
        0.0,
    ]
}

pub fn translate(dx: f64, dy: f64) -> Affine {
    [1.0, 0.0, 0.0, 1.0, dx, dy]
}

/// `m` rebased so it acts around `origin` instead of the coordinate origin.
pub fn about_origin(m: &Affine, origin: (f64, f64)) -> Affine {
    multiply(
        &translate(origin.0, origin.1),
        &multiply(m, &translate(-origin.0, -origin.1)),
    )
}

/// The transform panel's affine: scale, then skew, then rotate, then mirror.
pub fn panel_affine(
    scale_x: f64,
    scale_y: f64,
    rotate_deg: f64,
    skew_x: f64,
    skew_y: f64,
    mirror_h: bool,
    mirror_v: bool,
) -> Affine {
    multiply(
        &mirror(mirror_h, mirror_v),
        &multiply(
            &rotate(rotate_deg),
            &multiply(&skew(skew_x, skew_y), &scale(scale_x, scale_y)),
        ),
    )
}

/// Rounds `v` to the nearest grid step; a step of 0 (grid off) leaves it alone.
pub fn snap(v: f64, step: f64) -> f64 {
    if step <= 0.0 {
        v
    } else {
        (v / step).round() * step
    }
}

pub fn snap_point(x: f64, y: f64, grid: &Grid) -> (f64, f64) {
    match grid.step() {
        Some(step) => (snap(x, step), snap(y, step)),
        None => (x, y),
    }
}

/// Transforms the selected nodes, components and anchors of `layer` (or everything
/// when `sel` is empty) by `affine` around `origin`.
pub fn apply(layer: &mut Layer, sel: &Selection, affine: &Affine, origin: (f64, f64)) {
    let m = about_origin(affine, origin);
    let whole = sel.is_empty();
    let mut pi = 0usize;
    let mut ci = 0usize;
    for shape in &mut layer.shapes {
        match shape {
            Shape::Path(p) => {
                for (ni, n) in p.nodes.iter_mut().enumerate() {
                    if whole || sel.nodes.contains(&(pi, ni)) {
                        let (x, y) = map_point(&m, n.x, n.y);
                        n.x = x;
                        n.y = y;
                    }
                }
                pi += 1;
            }
            Shape::Component {
                transform, align, ..
            } => {
                if whole || sel.components.contains(&ci) {
                    *transform = multiply(&m, transform);
                    // Hand-transformed components stay where they were put.
                    *align = false;
                }
                ci += 1;
            }
        }
    }
    for a in &mut layer.anchors {
        if whole || sel.anchors.contains(&a.name) {
            let (x, y) = map_point(&m, a.x, a.y);
            a.x = x;
            a.y = y;
        }
    }
}

/// Returns `font` with the transform applied to one layer, or to every master's
/// layer of the glyph when `all_masters` (the same affine around the same origin).
pub fn transformed(
    font: &Font,
    glyph: &str,
    key: &str,
    sel: &Selection,
    affine: &Affine,
    origin: (f64, f64),
    all_masters: bool,
) -> Result<Font> {
    let mut next = font.clone();
    let masters: Vec<String> = next.masters.iter().map(|m| m.id.clone()).collect();
    let g = next
        .glyph_mut(glyph)
        .ok_or_else(|| Error::Invalid(format!("no glyph named \"{glyph}\"")))?;
    let keys: Vec<String> = if all_masters {
        masters
    } else {
        vec![key.to_string()]
    };
    for k in keys {
        if let Some(layer) = g.layer_mut(&k) {
            apply(layer, sel, affine, origin);
        }
    }
    Ok(next)
}

/// The identity affine, re-exported for callers building transforms.
pub const ID: Affine = IDENTITY;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Node, NodeKind, Path};
    use crate::sample::sample_font;

    fn rect_layer() -> Layer {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            closed: true,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(100.0, 0.0, NodeKind::Line),
                Node::new(100.0, 200.0, NodeKind::Line),
                Node::new(0.0, 200.0, NodeKind::Line),
            ],
        })];
        l.anchors = vec![Anchor {
            name: "top".into(),
            x: 50.0,
            y: 200.0,
        }];
        l
    }

    fn coords(l: &Layer) -> Vec<(f64, f64)> {
        match &l.shapes[0] {
            Shape::Path(p) => p.nodes.iter().map(|n| (n.x, n.y)).collect(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn scale_about_origin_exact() {
        let mut l = rect_layer();
        apply(&mut l, &Selection::default(), &scale(2.0, 0.5), (0.0, 0.0));
        assert_eq!(
            coords(&l),
            vec![(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)]
        );
        assert_eq!((l.anchors[0].x, l.anchors[0].y), (100.0, 100.0));
    }

    #[test]
    fn scale_about_centre_keeps_centre() {
        let mut l = rect_layer();
        apply(
            &mut l,
            &Selection::default(),
            &scale(2.0, 2.0),
            (50.0, 100.0),
        );
        assert_eq!(
            coords(&l),
            vec![
                (-50.0, -100.0),
                (150.0, -100.0),
                (150.0, 300.0),
                (-50.0, 300.0)
            ]
        );
    }

    #[test]
    fn rotate_90_exact() {
        let mut l = rect_layer();
        apply(&mut l, &Selection::default(), &rotate(90.0), (0.0, 0.0));
        let c = coords(&l);
        for (got, want) in c
            .iter()
            .zip([(0.0, 0.0), (0.0, 100.0), (-200.0, 100.0), (-200.0, 0.0)])
        {
            assert!(
                (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9,
                "{c:?}"
            );
        }
    }

    #[test]
    fn mirror_h_and_v() {
        let mut l = rect_layer();
        apply(
            &mut l,
            &Selection::default(),
            &mirror(true, false),
            (50.0, 0.0),
        );
        assert_eq!(
            coords(&l),
            vec![(100.0, 0.0), (0.0, 0.0), (0.0, 200.0), (100.0, 200.0)]
        );
        let mut l2 = rect_layer();
        apply(
            &mut l2,
            &Selection::default(),
            &mirror(false, true),
            (0.0, 100.0),
        );
        assert_eq!(
            coords(&l2),
            vec![(0.0, 200.0), (100.0, 200.0), (100.0, 0.0), (0.0, 0.0)]
        );
    }

    #[test]
    fn skew_x_leans_verticals() {
        let mut l = rect_layer();
        apply(&mut l, &Selection::default(), &skew(45.0, 0.0), (0.0, 0.0));
        let c = coords(&l);
        assert!(
            (c[2].0 - 300.0).abs() < 1e-9 && (c[2].1 - 200.0).abs() < 1e-9,
            "{c:?}"
        );
        assert!((c[0].0 - 0.0).abs() < 1e-9);
    }

    #[test]
    fn selection_only_moves_chosen_nodes() {
        let mut l = rect_layer();
        let sel = Selection {
            nodes: vec![(0, 1)],
            components: vec![],
            anchors: vec![],
        };
        apply(&mut l, &sel, &translate(10.0, 0.0), (0.0, 0.0));
        assert_eq!(
            coords(&l),
            vec![(0.0, 0.0), (110.0, 0.0), (100.0, 200.0), (0.0, 200.0)]
        );
        // Anchor untouched because the selection was not empty and named none.
        assert_eq!((l.anchors[0].x, l.anchors[0].y), (50.0, 200.0));
    }

    #[test]
    fn component_transform_composes_and_unaligns() {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![Shape::component("l", translate(10.0, 20.0))];
        let sel = Selection {
            nodes: vec![],
            components: vec![0],
            anchors: vec![],
        };
        apply(&mut l, &sel, &scale(2.0, 2.0), (0.0, 0.0));
        match &l.shapes[0] {
            Shape::Component {
                transform, align, ..
            } => {
                assert_eq!(*transform, [2.0, 0.0, 0.0, 2.0, 20.0, 40.0]);
                assert!(!align);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn snap_rounds_to_grid_step() {
        let g = Grid {
            spacing: 100,
            subdivision: 2,
        };
        assert_eq!(snap_point(124.0, 176.0, &g), (100.0, 200.0));
        let off = Grid {
            spacing: 0,
            subdivision: 2,
        };
        assert_eq!(snap_point(124.0, 176.0, &off), (124.0, 176.0));
        assert_eq!(snap(124.0, 0.0), 124.0);
    }

    #[test]
    fn all_masters_stay_compatible() {
        let mut f = sample_font();
        // Give H a second master layer with the same node counts.
        let bold = f.masters[0].clone();
        f.masters.push(crate::model::Master {
            id: "m02".into(),
            name: "Bold".into(),
            location: Default::default(),
            metrics: bold.metrics,
            guides: Vec::new(),
            alignment_zones: Vec::new(),
            stems: Vec::new(),
            vertical: Default::default(),
        });
        for g in &mut f.glyphs {
            let mut l = g.layers[0].clone();
            l.master_id = "m02".into();
            g.layers.push(l);
        }
        let before: Vec<usize> = f
            .glyph("H")
            .unwrap()
            .layers
            .iter()
            .map(|l| l.shapes.len())
            .collect();
        let next = transformed(
            &f,
            "H",
            "m01",
            &Selection::default(),
            &scale(1.1, 1.0),
            (0.0, 0.0),
            true,
        )
        .unwrap();
        let h = next.glyph("H").unwrap();
        assert_eq!(h.layers.len(), 2);
        for (l, n) in h.layers.iter().zip(before) {
            assert_eq!(l.shapes.len(), n);
        }
        // Both masters scaled identically.
        let a = coords(h.layer("m01").unwrap());
        let b = coords(h.layer("m02").unwrap());
        assert_eq!(a, b);
        assert!((a[1].0 - 187.0).abs() < 1e-9, "{a:?}");
    }

    #[test]
    fn transform_is_one_undoable_step() {
        let mut f = sample_font();
        let mut hist = crate::edit::History::default();
        let original = coords(f.glyph("H").unwrap().layer("m01").unwrap());
        let next = transformed(
            &f,
            "H",
            "m01",
            &Selection::default(),
            &scale(2.0, 1.0),
            (0.0, 0.0),
            false,
        )
        .unwrap();
        assert!(hist.apply_font(&mut f, next).unwrap());
        assert!(hist.can_undo());
        hist.undo(&mut f);
        assert_eq!(
            coords(f.glyph("H").unwrap().layer("m01").unwrap()),
            original
        );
    }

    #[test]
    fn unknown_glyph_errors() {
        let f = sample_font();
        assert!(transformed(
            &f,
            "nope",
            "m01",
            &Selection::default(),
            &ID,
            (0.0, 0.0),
            false
        )
        .is_err());
    }
}
