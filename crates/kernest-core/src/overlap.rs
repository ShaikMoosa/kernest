//! Removing overlaps: merging closed contours that overlap (or cross themselves) into
//! their outline, the way the glyph renders with the non-zero fill rule.
//!
//! Contours that don't overlap anything are left exactly as they are; only groups of
//! contours whose union differs from the contours themselves are replaced. The result
//! follows Kernest's direction convention (outer contours counter-clockwise).

use kurbo::{BezPath, PathEl, Point, Shape as _};
use linesweeper::{binary_op, BinaryOp, FillRule};

use crate::model::{Layer, Node, NodeKind, Path, Shape};
use crate::pathops::correct_direction;

pub(crate) fn to_bez(p: &Path) -> Option<BezPath> {
    let n = p.nodes.len();
    let start = (0..n).find(|&i| p.nodes[i].on_curve())?;
    let pt = |i: usize| Point::new(p.nodes[i % n].x, p.nodes[i % n].y);
    let mut b = BezPath::new();
    b.move_to(pt(start));
    let mut offs: Vec<Point> = Vec::new();
    let steps = if p.closed { n } else { n - 1 - start };
    for k in 1..=steps {
        let i = (start + k) % n;
        let node = p.nodes[i];
        match node.kind {
            NodeKind::OffCurve => offs.push(pt(i)),
            NodeKind::Curve if offs.len() == 2 => {
                b.curve_to(offs[0], offs[1], pt(i));
                offs.clear();
            }
            NodeKind::QCurve | NodeKind::Curve if !offs.is_empty() => {
                for (j, q) in offs.iter().enumerate() {
                    let end = match offs.get(j + 1) {
                        Some(next) => q.midpoint(*next),
                        None => pt(i),
                    };
                    b.quad_to(*q, end);
                }
                offs.clear();
            }
            _ => {
                b.line_to(pt(i));
                offs.clear();
            }
        }
    }
    if p.closed {
        b.close_path();
    }
    Some(b)
}

fn round(p: Point) -> (f64, f64) {
    (p.x.round(), p.y.round())
}

/// A Kernest path from a kurbo subpath (coordinates rounded to units).
pub(crate) fn from_bez(b: &BezPath, closed: bool) -> Option<Path> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut start: Option<(f64, f64)> = None;
    for el in b.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let (x, y) = round(p);
                start = Some((x, y));
                nodes.push(Node::new(x, y, NodeKind::Line));
            }
            PathEl::LineTo(p) => {
                let (x, y) = round(p);
                if nodes.last().is_some_and(|l| (l.x, l.y) == (x, y)) {
                    continue; // collapsed by rounding
                }
                nodes.push(Node::new(x, y, NodeKind::Line));
            }
            PathEl::QuadTo(q, p) => {
                // Exact as a cubic.
                let prev = nodes.last().map(|n| Point::new(n.x, n.y))?;
                let c1 = prev.lerp(q, 2.0 / 3.0);
                let c2 = p.lerp(q, 2.0 / 3.0);
                for c in [c1, c2] {
                    let (x, y) = round(c);
                    nodes.push(Node::new(x, y, NodeKind::OffCurve));
                }
                let (x, y) = round(p);
                nodes.push(Node::new(x, y, NodeKind::Curve));
            }
            PathEl::CurveTo(a, c, p) => {
                for q in [a, c] {
                    let (x, y) = round(q);
                    nodes.push(Node::new(x, y, NodeKind::OffCurve));
                }
                let (x, y) = round(p);
                nodes.push(Node::new(x, y, NodeKind::Curve));
            }
            PathEl::ClosePath => {}
        }
    }
    let start = start?;
    // The last segment usually ends back on the start point: fold it into the start
    // node (a closed path's first node carries the type of the closing segment).
    if closed && nodes.len() > 1 {
        let last = *nodes.last().expect("not empty");
        if last.on_curve() && (last.x, last.y) == start {
            nodes.pop();
            nodes[0].kind = last.kind;
        }
    }
    if closed {
        drop_collinear(&mut nodes);
    }
    if nodes.iter().filter(|n| n.on_curve()).count() < 2 {
        return None;
    }
    mark_smooth(&mut nodes, closed);
    Some(Path {
        locked: false,
        stroke: None,
        mask: false,
        closed,
        nodes,
    })
}

/// Removes points in the middle of straight edges: a corner between two line segments
/// that continue in the same direction (left where shapes were merged).
fn drop_collinear(nodes: &mut Vec<Node>) {
    let mut i = 0;
    while nodes.len() > 3 && i < nodes.len() {
        let n = nodes.len();
        let (a, p, b) = (nodes[(i + n - 1) % n], nodes[i], nodes[(i + 1) % n]);
        let straight = p.kind == NodeKind::Line && b.kind == NodeKind::Line && a.on_curve();
        let (ux, uy, vx, vy) = (p.x - a.x, p.y - a.y, b.x - p.x, b.y - p.y);
        if straight && (ux * vy - uy * vx).abs() < 1e-9 && ux * vx + uy * vy >= 0.0 {
            nodes.remove(i);
            i = i.saturating_sub(1);
        } else {
            i += 1;
        }
    }
}

/// On-curve nodes whose incoming and outgoing directions line up are smooth.
fn mark_smooth(nodes: &mut [Node], closed: bool) {
    let n = nodes.len();
    for i in 0..n {
        if !nodes[i].on_curve() {
            continue;
        }
        if !closed && (i == 0 || i == n - 1) {
            continue;
        }
        let (p, a, b) = (nodes[i], nodes[(i + n - 1) % n], nodes[(i + 1) % n]);
        let (ux, uy) = (p.x - a.x, p.y - a.y);
        let (vx, vy) = (b.x - p.x, b.y - p.y);
        let (lu, lv) = ((ux * ux + uy * uy).sqrt(), (vx * vx + vy * vy).sqrt());
        if lu == 0.0 || lv == 0.0 || (a.on_curve() && b.on_curve()) {
            continue;
        }
        let cross = (ux * vy - uy * vx) / (lu * lv);
        let dot = (ux * vx + uy * vy) / (lu * lv);
        nodes[i].smooth = cross.abs() < 0.02 && dot > 0.0;
    }
}

fn area(b: &BezPath) -> f64 {
    b.area()
}

/// Removes overlaps between the closed paths of a layer. Open paths and components are
/// kept. Returns whether anything changed.
pub fn remove_overlaps(layer: &mut Layer) -> bool {
    let closed: Vec<(usize, BezPath)> = layer
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Shape::Path(p) if p.closed && p.nodes.len() >= 2 => to_bez(p).map(|b| (i, b)),
            _ => None,
        })
        .collect();
    if closed.is_empty() {
        return false;
    }
    // Group contours whose bounding boxes overlap or touch (directly or through others).
    let boxes: Vec<kurbo::Rect> = closed.iter().map(|(_, b)| b.bounding_box()).collect();
    let mut group: Vec<usize> = (0..closed.len()).collect();
    fn find(g: &mut [usize], i: usize) -> usize {
        if g[i] != i {
            let r = find(g, g[i]);
            g[i] = r;
        }
        g[i]
    }
    for i in 0..closed.len() {
        for j in i + 1..closed.len() {
            // Touching counts: contours that share an edge merge too.
            if boxes[i].inflate(0.5, 0.5).intersect(boxes[j]).area() > 0.0 {
                let (a, b) = (find(&mut group, i), find(&mut group, j));
                group[a] = b;
            }
        }
    }
    let roots: Vec<usize> = (0..closed.len()).map(|i| find(&mut group, i)).collect();
    let mut clusters: Vec<(usize, Vec<usize>)> = Vec::new();
    for (i, &r) in roots.iter().enumerate() {
        match clusters.iter_mut().find(|(root, _)| *root == r) {
            Some((_, c)) => c.push(i),
            None => clusters.push((r, vec![i])),
        }
    }
    let clusters: Vec<Vec<usize>> = clusters.into_iter().map(|(_, c)| c).collect();

    let mut replace: Vec<(Vec<usize>, Vec<Path>)> = Vec::new();
    for c in clusters {
        let mut all = BezPath::new();
        for &k in &c {
            all.extend(closed[k].1.elements().iter().copied());
        }
        let Ok(result) = binary_op(&all, &BezPath::new(), FillRule::NonZero, BinaryOp::Union)
        else {
            continue;
        };
        let out: Vec<BezPath> = result.contours().map(|c| c.path.clone()).collect();
        // Same number of contours and the same filled area: nothing overlapped.
        let before: f64 = c.iter().map(|&k| area(&closed[k].1)).sum::<f64>().abs();
        let after: f64 = out.iter().map(area).sum::<f64>().abs();
        let tolerance = (before * 1e-4).max(1.0);
        if out.len() == c.len() && (before - after).abs() <= tolerance {
            continue;
        }
        let paths: Vec<Path> = out.iter().filter_map(|b| from_bez(b, true)).collect();
        replace.push((c.iter().map(|&k| closed[k].0).collect(), paths));
    }
    if replace.is_empty() {
        return false;
    }
    // Rebuild: replaced contours go where the first of their group was.
    let mut shapes = Vec::with_capacity(layer.shapes.len());
    for (i, s) in layer.shapes.iter().enumerate() {
        match replace.iter().find(|(idx, _)| idx.contains(&i)) {
            Some((idx, paths)) if idx[0] == i => {
                shapes.extend(paths.iter().cloned().map(Shape::Path));
            }
            Some(_) => {}
            None => shapes.push(s.clone()),
        }
    }
    layer.shapes = shapes;
    correct_direction(layer);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
        Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .into_iter()
                .map(|(x, y)| Node::new(x, y, NodeKind::Line))
                .collect(),
        })
    }

    fn paths(l: &Layer) -> Vec<&Path> {
        l.shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Path(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn merges_a_cross() {
        // A plus sign made of two bars → one 12-point outline.
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![
            rect(0.0, 200.0, 600.0, 400.0),
            rect(200.0, 0.0, 400.0, 600.0),
        ];
        assert!(remove_overlaps(&mut l));
        let p = paths(&l);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].nodes.len(), 12);
        assert!(p[0].nodes.iter().all(|n| n.kind == NodeKind::Line));
        let b = to_bez(p[0]).unwrap();
        assert!((b.area() - (600.0 * 200.0 * 2.0 - 200.0 * 200.0)).abs() < 1e-6);
        assert!(b.area() > 0.0, "outer contour counter-clockwise");
    }

    #[test]
    fn merged_edges_have_no_extra_points() {
        // Like a sans-serif H: the crossbar runs the full width, over both stems.
        let mut l = Layer::new("m01", 680.0);
        l.shapes = vec![
            rect(80.0, 0.0, 170.0, 700.0),
            rect(510.0, 0.0, 600.0, 700.0),
            rect(80.0, 310.0, 600.0, 390.0),
        ];
        assert!(remove_overlaps(&mut l));
        let p = paths(&l);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].nodes.len(), 12, "only the corners of the H");
    }

    #[test]
    fn leaves_separate_and_nested_contours_alone() {
        let mut l = Layer::new("m01", 600.0);
        let mut counter = rect(100.0, 100.0, 300.0, 500.0);
        if let Shape::Path(p) = &mut counter {
            p.nodes.reverse(); // clockwise: a hole
        }
        l.shapes = vec![
            rect(0.0, 0.0, 400.0, 600.0),
            counter,
            rect(500.0, 0.0, 600.0, 100.0),
        ];
        let before = l.clone();
        assert!(!remove_overlaps(&mut l));
        assert_eq!(l, before);
    }

    #[test]
    fn merges_curves_and_keeps_components() {
        // Two overlapping circles (cubic) plus a component.
        let circle = |cx: f64| {
            let k = 0.5523 * 100.0;
            let n = |x: f64, y: f64, kind| Node::new(cx + x, 300.0 + y, kind);
            Shape::Path(Path {
                locked: false,
                stroke: None,
                mask: false,
                closed: true,
                nodes: vec![
                    n(100.0, 0.0, NodeKind::Curve),
                    n(100.0, k, NodeKind::OffCurve),
                    n(k, 100.0, NodeKind::OffCurve),
                    n(0.0, 100.0, NodeKind::Curve),
                    n(-k, 100.0, NodeKind::OffCurve),
                    n(-100.0, k, NodeKind::OffCurve),
                    n(-100.0, 0.0, NodeKind::Curve),
                    n(-100.0, -k, NodeKind::OffCurve),
                    n(-k, -100.0, NodeKind::OffCurve),
                    n(0.0, -100.0, NodeKind::Curve),
                    n(k, -100.0, NodeKind::OffCurve),
                    n(100.0, -k, NodeKind::OffCurve),
                ],
            })
        };
        let comp = Shape::component("acute", crate::model::IDENTITY);
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![circle(200.0), comp.clone(), circle(330.0)];
        assert!(remove_overlaps(&mut l));
        assert_eq!(paths(&l).len(), 1);
        assert!(l.shapes.contains(&comp));
        let p = paths(&l)[0];
        assert!(p.nodes.iter().any(|n| n.kind == NodeKind::Curve));
        let b = to_bez(p).unwrap().bounding_box();
        assert!((b.x0 - 100.0).abs() <= 1.0 && (b.x1 - 430.0).abs() <= 1.0);
    }
}
