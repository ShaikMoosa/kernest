//! Filters: outline operations that run on whole glyphs, optionally across masters.
//!
//! Round Corners adds the same four nodes at every sharp line corner, so masters
//! that start compatible stay compatible. Offset Curve rebuilds outlines with kurbo
//! and usually changes node counts, so the command layer applies it to one master
//! unless every master's result keeps the same structure.

use kurbo::{
    expand_path, Affine, BezPath, Diagonal2, Join, PathEl, Shape as _, Stroke, StrokeOpts,
};
use linesweeper::{binary_op, BinaryOp, FillRule};

use crate::model::{Layer, Node, NodeKind, Path, Shape};
use crate::overlap::{from_bez, to_bez};

/// Kurbo accuracy in font units; well below one unit at typical UPM sizes.
const TOLERANCE: f64 = 0.1;

/// Node counts per path, used to check whether a filter kept masters compatible.
pub fn signature(layer: &Layer) -> Vec<usize> {
    layer
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p.nodes.len()),
            _ => None,
        })
        .collect()
}

fn in_scope(index: usize, only: Option<&[usize]>) -> bool {
    only.is_none_or(|list| list.contains(&index))
}

/// Rounds sharp corners between straight segments into arcs of `radius` (clamped to
/// half of each neighbouring edge). Every rounded corner becomes the same four nodes
/// — tangent point, two handles, tangent point — keeping compatible masters compatible.
/// Returns the number of corners rounded. Locked paths are skipped.
pub fn round_corners(layer: &mut Layer, radius: f64, only: Option<&[usize]>) -> usize {
    if !radius.is_finite() || radius <= 0.0 {
        return 0;
    }
    let mut total = 0;
    let mut pi = 0;
    for shape in &mut layer.shapes {
        let Shape::Path(p) = shape else { continue };
        let index = pi;
        pi += 1;
        if p.locked || !in_scope(index, only) {
            continue;
        }
        total += round_path_corners(p, radius);
    }
    total
}

fn round_path_corners(p: &mut Path, radius: f64) -> usize {
    let n = p.nodes.len();
    if n < 3 {
        return 0;
    }
    // Corners are found on the original list so neighbouring corners don't interfere;
    // each consumes at most half of the edge they share.
    let mut replacements: Vec<Option<Vec<Node>>> = vec![None; n];
    let mut count = 0;
    for (i, slot) in replacements.iter_mut().enumerate() {
        let b = p.nodes[i];
        if !b.on_curve() || b.kind != NodeKind::Line || b.smooth {
            continue;
        }
        if !p.closed && (i == 0 || i == n - 1) {
            continue;
        }
        let a = p.nodes[(i + n - 1) % n];
        let c = p.nodes[(i + 1) % n];
        if !a.on_curve() || !c.on_curve() {
            continue;
        }
        let (mut ux, mut uy) = (b.x - a.x, b.y - a.y);
        let (mut vx, mut vy) = (c.x - b.x, c.y - b.y);
        let lu = ux.hypot(uy);
        let lv = vx.hypot(vy);
        if lu < 1e-9 || lv < 1e-9 {
            continue;
        }
        ux /= lu;
        uy /= lu;
        vx /= lv;
        vy /= lv;
        let theta = (ux * vy - uy * vx).abs().atan2(ux * vx + uy * vy);
        // Nearly straight or nearly reversing corners have no meaningful arc.
        if !(0.0087..=std::f64::consts::PI - 0.0087).contains(&theta) {
            continue;
        }
        let d = radius.min(lu * 0.5).min(lv * 0.5);
        // Handle length for a cubic approximating an arc of sweep theta, expressed
        // as a fraction of the tangent distance d.
        let k = (4.0 / 3.0) * (theta / 4.0).tan() / (theta / 2.0).tan();
        let mut pt = Node::new(b.x - ux * d, b.y - uy * d, NodeKind::Line);
        pt.smooth = true;
        let h1 = Node::new(
            b.x - ux * d * (1.0 - k),
            b.y - uy * d * (1.0 - k),
            NodeKind::OffCurve,
        );
        let h2 = Node::new(
            b.x + vx * d * (1.0 - k),
            b.y + vy * d * (1.0 - k),
            NodeKind::OffCurve,
        );
        let mut qt = Node::new(b.x + vx * d, b.y + vy * d, NodeKind::Curve);
        qt.smooth = true;
        *slot = Some(vec![pt, h1, h2, qt]);
        count += 1;
    }
    if count == 0 {
        return 0;
    }
    let mut nodes = Vec::with_capacity(n + count * 3);
    for (i, old) in p.nodes.iter().enumerate() {
        match &replacements[i] {
            Some(four) => nodes.extend(four.iter().copied()),
            None => nodes.push(*old),
        }
    }
    p.nodes = nodes;
    count
}

/// Offsets closed outlines by `h` horizontally and `v` vertically (positive grows the
/// filled shape). With `make_stroke`, the path becomes a stroke of that width instead —
/// open paths are allowed then. Locked paths are skipped. Returns whether anything changed.
pub fn offset_curve(
    layer: &mut Layer,
    h: f64,
    v: f64,
    make_stroke: bool,
    only: Option<&[usize]>,
) -> bool {
    if !h.is_finite() || !v.is_finite() {
        return false;
    }
    let amount = if make_stroke {
        h.abs().max(v.abs())
    } else {
        h.abs() + v.abs()
    };
    if amount < 1e-9 {
        return false;
    }
    let mut changed = false;
    let mut out: Vec<Shape> = Vec::with_capacity(layer.shapes.len());
    let mut pi = 0;
    for s in layer.shapes.drain(..) {
        if let Shape::Path(p) = &s {
            let index = pi;
            pi += 1;
            if !p.locked && in_scope(index, only) {
                if let Some(mut parts) = offset_path(p, h, v, make_stroke) {
                    // The offset outline is rebuilt from scratch and may start at any
                    // corner or run the other way: keep the original's start and
                    // direction, so masters offset alike still interpolate cleanly.
                    if !make_stroke && parts.len() == 1 {
                        keep_start_and_direction(p, &mut parts[0]);
                    }
                    changed = true;
                    out.extend(parts.into_iter().map(Shape::Path));
                    continue;
                }
            }
        }
        out.push(s);
    }
    layer.shapes = out;
    changed
}

/// Makes `new` (an offset of `old`) run the same way as `old` and start at the
/// on-curve point nearest to `old`'s start.
fn keep_start_and_direction(old: &Path, new: &mut Path) {
    if !old.closed || !new.closed || new.nodes.is_empty() {
        return;
    }
    let area = |p: &Path| {
        let n = p.nodes.len();
        (0..n)
            .map(|i| {
                let (a, b) = (&p.nodes[i], &p.nodes[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f64>()
    };
    if area(old).signum() != area(new).signum() {
        *new = crate::pathops::reverse(new);
    }
    let Some(start) = old.nodes.iter().find(|n| n.on_curve()) else {
        return;
    };
    let nearest = (0..new.nodes.len())
        .filter(|&i| new.nodes[i].on_curve())
        .min_by(|&a, &b| {
            let d =
                |i: usize| (new.nodes[i].x - start.x).powi(2) + (new.nodes[i].y - start.y).powi(2);
            d(a).total_cmp(&d(b))
        });
    if let Some(i) = nearest {
        new.nodes.rotate_left(i);
    }
}

fn offset_path(p: &Path, h: f64, v: f64, make_stroke: bool) -> Option<Vec<Path>> {
    if !make_stroke && !p.closed {
        return None;
    }
    let bez = to_bez(p)?;
    let expanded = if make_stroke {
        let (ah, av) = (h.abs(), v.abs());
        let base = ah.max(av);
        let style = Stroke::new(2.0 * base);
        if ah > 1e-9 && av > 1e-9 && (ah - av).abs() > 1e-9 {
            // Stroke in a space where the width is uniform, then scale back: the
            // result is 2h thick horizontally and 2v thick vertically.
            let squish = Affine::scale_non_uniform(base / ah, base / av);
            let restore = Affine::scale_non_uniform(ah / base, av / base);
            restore
                * kurbo::stroke(
                    (squish * bez).elements().iter().copied(),
                    &style,
                    &StrokeOpts::default(),
                    TOLERANCE,
                )
        } else {
            kurbo::stroke(
                bez.elements().iter().copied(),
                &style,
                &StrokeOpts::default(),
                TOLERANCE,
            )
        }
    } else {
        expand_bez(bez, h, v)
    };
    Some(
        subpaths(&expanded)
            .into_iter()
            .filter_map(|(b, closed)| from_bez(&b, closed))
            .collect(),
    )
}

/// Offsets a closed kurbo path by (h, v): positive grows the filled shape, negative
/// shrinks it. Kurbo's own inward expansion leaves artifacts at corners, so shrinking
/// erodes instead: the path minus a stroke band around its boundary. Mixed signs
/// offset one axis at a time.
fn expand_bez(bez: BezPath, h: f64, v: f64) -> BezPath {
    if (h >= 0.0 && v >= 0.0) || (h <= 0.0 && v <= 0.0) {
        offset_same_sign(bez, h, v)
    } else {
        let step = offset_same_sign(bez, h, 0.0);
        offset_same_sign(step, 0.0, v)
    }
}

fn offset_same_sign(bez: BezPath, h: f64, v: f64) -> BezPath {
    const EPS: f64 = 1e-9;
    if h.abs() < EPS && v.abs() < EPS {
        return bez;
    }
    if h >= 0.0 && v >= 0.0 {
        return expand_path(bez, Diagonal2::new(h, v), Join::Miter, 10.0, TOLERANCE);
    }
    let (ah, av) = (-h, -v);
    let area = bez.area();
    let result = if ah > EPS && av > EPS {
        // The band is the boundary swept by an ellipse of half-axes (ah, av): a
        // uniform stroke in squished space, scaled back.
        let base = ah.max(av);
        let squish = Affine::scale_non_uniform(base / ah, base / av);
        let restore = Affine::scale_non_uniform(ah / base, av / base);
        let band = restore
            * kurbo::stroke(
                (squish * bez.clone()).elements().iter().copied(),
                &Stroke::new(2.0 * base),
                &StrokeOpts::default(),
                TOLERANCE,
            );
        boolean(bez, band, BinaryOp::Difference)
    } else {
        // One axis is zero: eroding by a straight segment keeps the points whose
        // whole segment neighbourhood is inside — the two extreme translates.
        let (dx, dy) = if ah > EPS { (ah, 0.0) } else { (0.0, av) };
        let plus = Affine::translate((dx, dy)) * bez.clone();
        let minus = Affine::translate((-dx, -dy)) * bez.clone();
        let step = boolean(bez, plus, BinaryOp::Intersection);
        boolean(step, minus, BinaryOp::Intersection)
    };
    // Keep the contour direction the caller had (outer vs counter).
    if result.area() * area < 0.0 {
        result.reverse_subpaths()
    } else {
        result
    }
}

fn boolean(a: BezPath, b: BezPath, op: BinaryOp) -> BezPath {
    match binary_op(&a, &b, FillRule::NonZero, op) {
        Ok(r) => {
            let mut out = BezPath::new();
            for c in r.contours() {
                out.extend(c.path.elements().iter().copied());
            }
            out
        }
        // A failed boolean leaves the shape untouched rather than losing work.
        Err(_) => a,
    }
}

/// Splits a kurbo path into its subpaths, remembering which ones are closed.
/// A subpath whose last point lands back on its first is closed even without an
/// explicit ClosePath (kurbo's stroke output does this).
fn subpaths(b: &BezPath) -> Vec<(BezPath, bool)> {
    let mut out: Vec<(BezPath, bool)> = Vec::new();
    let mut cur = BezPath::new();
    let mut closed = false;
    let mut started = false;
    let mut first: Option<kurbo::Point> = None;
    let mut last: Option<kurbo::Point> = None;
    let closes = |first: Option<kurbo::Point>, last: Option<kurbo::Point>| match (first, last) {
        (Some(a), Some(b)) => (a - b).hypot() < 1e-6,
        _ => false,
    };
    for el in b.elements() {
        match el {
            PathEl::MoveTo(p) => {
                if started {
                    out.push((cur, closed || closes(first, last)));
                }
                cur = BezPath::new();
                closed = false;
                started = true;
                first = Some(*p);
                last = Some(*p);
                cur.push(*el);
            }
            PathEl::ClosePath => {
                closed = true;
                last = first;
                cur.push(*el);
            }
            PathEl::LineTo(p) => {
                last = Some(*p);
                cur.push(*el);
            }
            PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => {
                last = Some(*p);
                cur.push(*el);
            }
        }
    }
    if started {
        out.push((cur, closed || closes(first, last)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Layer, NodeKind};

    fn node(x: f64, y: f64, kind: NodeKind) -> Node {
        Node::new(x, y, kind)
    }

    fn rect(size: f64) -> Layer {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(size, 0.0, NodeKind::Line),
                node(size, size, NodeKind::Line),
                node(0.0, size, NodeKind::Line),
            ],
        })];
        l
    }

    fn path(s: &Shape) -> &Path {
        let Shape::Path(p) = s else {
            panic!("not a path")
        };
        p
    }

    fn bbox(l: &Layer) -> (f64, f64, f64, f64) {
        let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for s in &l.shapes {
            if let Shape::Path(p) = s {
                for n in &p.nodes {
                    b.0 = b.0.min(n.x);
                    b.1 = b.1.min(n.y);
                    b.2 = b.2.max(n.x);
                    b.3 = b.3.max(n.y);
                }
            }
        }
        b
    }

    #[test]
    fn round_corners_exact_nodes() {
        let mut l = rect(100.0);
        assert_eq!(round_corners(&mut l, 10.0, None), 4);
        let p = path(&l.shapes[0]);
        assert_eq!(p.nodes.len(), 16);
        // Corner (100,0) is the second corner: nodes 4..8. Tangent points (90,0)
        // and (100,10), handles at 55.23% of the radius.
        let want = [
            (90.0, 0.0, NodeKind::Line),
            (95.5228, 0.0, NodeKind::OffCurve),
            (100.0, 4.4772, NodeKind::OffCurve),
            (100.0, 10.0, NodeKind::Curve),
        ];
        for (n, (x, y, kind)) in p.nodes[4..8].iter().zip(want) {
            assert!((n.x - x).abs() < 1e-3 && (n.y - y).abs() < 1e-3, "{n:?}");
            assert_eq!(n.kind, kind);
        }
        assert!(p.nodes[4].smooth && p.nodes[7].smooth);
        assert!(p.closed);
    }

    #[test]
    fn round_corners_keeps_masters_compatible() {
        let mut light = rect(100.0);
        let mut bold = rect(120.0);
        // The bold square's corners are clamped by radius, but the node counts match.
        assert_eq!(round_corners(&mut light, 200.0, None), 4);
        assert_eq!(round_corners(&mut bold, 200.0, None), 4);
        assert_eq!(signature(&light), signature(&bold));
        assert_eq!(signature(&light), vec![16]);
    }

    #[test]
    fn round_corners_skips_curves_smooth_locked_and_unselected() {
        // A curve corner is not a line-line corner.
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(0.0, 200.0, NodeKind::OffCurve),
                node(200.0, 200.0, NodeKind::OffCurve),
                node(200.0, 0.0, NodeKind::Curve),
            ],
        })];
        assert_eq!(round_corners(&mut l, 10.0, None), 0);

        let mut l = rect(100.0);
        if let Shape::Path(p) = &mut l.shapes[0] {
            p.nodes[1].smooth = true;
        }
        assert_eq!(round_corners(&mut l, 10.0, None), 3);

        let mut l = rect(100.0);
        if let Shape::Path(p) = &mut l.shapes[0] {
            p.locked = true;
        }
        assert_eq!(round_corners(&mut l, 10.0, None), 0);

        let mut l = rect(100.0);
        l.shapes.push(Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: true,
            nodes: vec![
                node(200.0, 0.0, NodeKind::Line),
                node(300.0, 0.0, NodeKind::Line),
                node(300.0, 100.0, NodeKind::Line),
            ],
        }));
        assert_eq!(round_corners(&mut l, 10.0, Some(&[1])), 3);
        assert_eq!(path(&l.shapes[0]).nodes.len(), 4, "first path untouched");
        assert_eq!(round_corners(&mut l, 0.0, None), 0);
    }

    #[test]
    fn round_corners_on_open_path_keeps_ends() {
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: false,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line),
                node(100.0, 100.0, NodeKind::Line),
            ],
        })];
        assert_eq!(round_corners(&mut l, 10.0, None), 1);
        let p = path(&l.shapes[0]);
        assert!(!p.closed);
        assert_eq!(p.nodes.len(), 6);
        assert_eq!((p.nodes[0].x, p.nodes[0].y), (0.0, 0.0));
        assert_eq!(
            (p.nodes.last().unwrap().x, p.nodes.last().unwrap().y),
            (100.0, 100.0)
        );
    }

    #[test]
    fn offset_grows_and_shrinks_a_square() {
        let mut l = rect(100.0);
        assert!(offset_curve(&mut l, 10.0, 10.0, false, None));
        let (x0, y0, x1, y1) = bbox(&l);
        assert!(
            (x0 + 10.0).abs() < 0.5 && (y0 + 10.0).abs() < 0.5,
            "{x0} {y0}"
        );
        assert!(
            (x1 - 110.0).abs() < 0.5 && (y1 - 110.0).abs() < 0.5,
            "{x1} {y1}"
        );
        assert!(l.shapes.iter().map(path).all(|p| p.closed));

        let mut l = rect(100.0);
        assert!(offset_curve(&mut l, -10.0, -10.0, false, None));
        let (x0, y0, x1, y1) = bbox(&l);
        assert!(
            (x0 - 10.0).abs() < 0.5 && (y0 - 10.0).abs() < 0.5,
            "{x0} {y0} {x1} {y1}"
        );
        assert!(
            (x1 - 90.0).abs() < 0.5 && (y1 - 90.0).abs() < 0.5,
            "{x0} {y0} {x1} {y1}"
        );
    }

    #[test]
    fn offset_is_anisotropic() {
        let mut l = rect(100.0);
        assert!(offset_curve(&mut l, 20.0, 5.0, false, None));
        let (x0, y0, x1, y1) = bbox(&l);
        assert!((x0 + 20.0).abs() < 0.5 && (x1 - 120.0).abs() < 0.5);
        assert!((y0 + 5.0).abs() < 0.5 && (y1 - 105.0).abs() < 0.5);
    }

    #[test]
    fn offset_mixed_signs_and_single_axis() {
        let mut l = rect(100.0);
        assert!(offset_curve(&mut l, 20.0, -5.0, false, None));
        let (x0, y0, x1, y1) = bbox(&l);
        assert!(
            (x0 + 20.0).abs() < 0.5 && (x1 - 120.0).abs() < 0.5,
            "{x0} {x1}"
        );
        assert!(
            (y0 - 5.0).abs() < 0.5 && (y1 - 95.0).abs() < 0.5,
            "{y0} {y1}"
        );

        let mut l = rect(100.0);
        assert!(offset_curve(&mut l, -10.0, 0.0, false, None));
        let (x0, y0, x1, y1) = bbox(&l);
        assert!(
            (x0 - 10.0).abs() < 0.5 && (x1 - 90.0).abs() < 0.5,
            "{x0} {x1}"
        );
        assert!(y0.abs() < 0.5 && (y1 - 100.0).abs() < 0.5, "{y0} {y1}");
    }

    #[test]
    fn offset_noop_locked_and_open() {
        let mut l = rect(100.0);
        let before = l.clone();
        assert!(!offset_curve(&mut l, 0.0, 0.0, false, None));
        assert_eq!(l, before);
        if let Shape::Path(p) = &mut l.shapes[0] {
            p.locked = true;
        }
        let before = l.clone();
        assert!(!offset_curve(&mut l, 10.0, 10.0, false, None));
        assert_eq!(l, before);

        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: false,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line),
            ],
        })];
        let before = l.clone();
        assert!(!offset_curve(&mut l, 10.0, 10.0, false, None));
        assert_eq!(l, before, "plain offset needs closed paths");
    }

    #[test]
    fn make_stroke_turns_a_line_into_a_closed_band() {
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: false,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line),
            ],
        })];
        assert!(offset_curve(&mut l, 10.0, 10.0, true, None));
        assert!(l.shapes.iter().map(path).all(|p| p.closed));
        let (x0, y0, x1, y1) = bbox(&l);
        // Round caps extend half the stroke width past each end.
        assert!(
            (x0 + 10.0).abs() < 0.5 && (x1 - 110.0).abs() < 0.5,
            "{x0} {x1}"
        );
        assert!(
            (y0 + 10.0).abs() < 0.5 && (y1 - 10.0).abs() < 0.5,
            "{y0} {y1}"
        );
    }

    #[test]
    fn make_stroke_anisotropic_widths() {
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: false,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line),
            ],
        })];
        assert!(offset_curve(&mut l, 20.0, 5.0, true, None));
        let (_, y0, _, y1) = bbox(&l);
        assert!(
            (y0 + 5.0).abs() < 0.5 && (y1 - 5.0).abs() < 0.5,
            "{y0} {y1}"
        );
    }

    #[test]
    fn offset_is_one_undoable_step() {
        let mut f = crate::sample::sample_font();
        let mut hist = crate::edit::History::default();
        let original = f.glyph("H").unwrap().layer("m01").unwrap().clone();
        let mut next = f.clone();
        let l = next.glyph_mut("H").unwrap().layer_mut("m01").unwrap();
        assert!(offset_curve(l, 10.0, 10.0, false, None));
        assert!(hist.apply(&mut f, "H", "m01", l.clone()).unwrap());
        assert!(hist.can_undo());
        hist.undo(&mut f);
        assert_eq!(f.glyph("H").unwrap().layer("m01").unwrap(), &original);
    }

    #[test]
    fn signature_counts_paths_only() {
        let mut l = rect(100.0);
        l.shapes.push(Shape::component("a", crate::model::IDENTITY));
        l.anchors = vec![Anchor {
            name: "top".into(),
            x: 0.0,
            y: 0.0,
        }];
        assert_eq!(signature(&l), vec![4]);
    }
}
