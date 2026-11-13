//! Outline clean-up: correcting path direction and adding points at extremes.
//!
//! Kernest follows the PostScript/UFO convention: outer contours run
//! counter-clockwise (with y up) and counters run clockwise. The compiler reverses
//! them when it builds TrueType outlines.

use crate::model::{Layer, Node, NodeKind, Path, Shape};

/// A polygon approximating a closed path (curves sampled), for area and containment.
fn polygon(p: &Path) -> Vec<(f64, f64)> {
    let n = p.nodes.len();
    let mut out = Vec::new();
    for i in 0..n {
        let node = p.nodes[i];
        if node.kind == NodeKind::Curve && n >= 4 {
            let a = p.nodes[(i + n - 3) % n];
            let b = p.nodes[(i + n - 2) % n];
            let c = p.nodes[(i + n - 1) % n];
            if a.on_curve() && !b.on_curve() && !c.on_curve() {
                for k in 1..8 {
                    let t = f64::from(k) / 8.0;
                    out.push(cubic(
                        (a.x, a.y),
                        (b.x, b.y),
                        (c.x, c.y),
                        (node.x, node.y),
                        t,
                    ));
                }
            }
        }
        if node.on_curve() || node.kind == NodeKind::OffCurve && is_quadratic(p) {
            out.push((node.x, node.y));
        }
    }
    out
}

fn is_quadratic(p: &Path) -> bool {
    p.nodes.iter().any(|n| n.kind == NodeKind::QCurve)
}

/// Signed area (positive for counter-clockwise with y up).
fn area(poly: &[(f64, f64)]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % n];
            x0 * y1 - x1 * y0
        })
        .sum::<f64>()
        / 2.0
}

fn contains(poly: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[(i + n - 1) % n];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
    }
    inside
}

/// Reverses a path's direction, keeping its start node first when it is on-curve.
pub fn reverse(p: &Path) -> Path {
    let n = p.nodes.len();
    let ons: Vec<usize> = (0..n).filter(|&i| p.nodes[i].on_curve()).collect();
    if ons.is_empty() {
        let mut nodes = p.nodes.clone();
        nodes.reverse();
        return Path {
            closed: p.closed,
            nodes,
        };
    }
    let mut nodes = p.nodes.clone();
    if p.closed {
        // Each on-curve node takes the segment type of the next on-curve node: the
        // segment that used to end there now ends here.
        for (k, &i) in ons.iter().enumerate() {
            nodes[i].kind = p.nodes[ons[(k + 1) % ons.len()]].kind;
        }
        nodes.reverse();
        if p.nodes[0].on_curve() {
            let start = n - 1; // the old first node, now last
            nodes.rotate_left(start);
        }
    } else {
        // Open: the old last node becomes the start (a line "move"); every other
        // on-curve node takes the type of the next one.
        for (k, &i) in ons.iter().enumerate() {
            nodes[i].kind = match ons.get(k + 1) {
                Some(&j) => p.nodes[j].kind,
                None => NodeKind::Line,
            };
        }
        nodes.reverse();
    }
    Path {
        closed: p.closed,
        nodes,
    }
}

/// What Correct Path Direction does to a layer's paths (indices among its paths):
/// which to reverse, and the new order of the paths.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectionPlan {
    pub reverse: Vec<usize>,
    pub order: Vec<usize>,
}

fn bottom_left(poly: &[(f64, f64)]) -> (f64, f64) {
    poly.iter()
        .fold((f64::INFINITY, f64::INFINITY), |(x, y), p| {
            (x.min(p.0), y.min(p.1))
        })
}

fn by_position(a: (f64, f64), b: (f64, f64)) -> std::cmp::Ordering {
    a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
}

/// Works out Correct Path Direction for a layer:
/// - outer contours counter-clockwise, counters clockwise (judged by how they nest);
/// - outer contours ordered left to right (then bottom to top), each followed by the
///   contours inside it; open paths keep their order after the closed ones.
pub fn direction_plan(layer: &Layer) -> DirectionPlan {
    let paths = layer_paths(layer);
    let polys: Vec<Option<Vec<(f64, f64)>>> = paths
        .iter()
        .map(|p| (p.closed && p.nodes.len() >= 3).then(|| polygon(p)))
        .collect();
    let mut reverse = Vec::new();
    // The innermost closed path around each closed path.
    let mut parent: Vec<Option<usize>> = vec![None; paths.len()];
    for (i, poly) in polys.iter().enumerate() {
        let Some(poly) = poly else { continue };
        let probe = poly[0];
        let around: Vec<usize> = polys
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && o.as_ref().is_some_and(|o| contains(o, probe)))
            .map(|(j, _)| j)
            .collect();
        let depth = around.len();
        parent[i] = around.into_iter().min_by(|&a, &b| {
            let size = |k: usize| area(polys[k].as_deref().unwrap_or(&[])).abs();
            size(a).total_cmp(&size(b))
        });
        let a = area(poly);
        if a.abs() > 1e-9 && (a > 0.0) != depth.is_multiple_of(2) {
            reverse.push(i);
        }
    }
    let key = |i: usize| bottom_left(polys[i].as_deref().unwrap_or(&[]));
    let mut order = Vec::with_capacity(paths.len());
    let mut stack: Vec<usize> = {
        let mut roots: Vec<usize> = (0..paths.len())
            .filter(|&i| polys[i].is_some() && parent[i].is_none())
            .collect();
        roots.sort_by(|&a, &b| by_position(key(b), key(a)));
        roots
    };
    // Depth-first: a contour, then the contours directly inside it, and so on.
    while let Some(i) = stack.pop() {
        order.push(i);
        let mut kids: Vec<usize> = (0..paths.len()).filter(|&j| parent[j] == Some(i)).collect();
        kids.sort_by(|&a, &b| by_position(key(b), key(a)));
        stack.extend(kids);
    }
    order.extend((0..paths.len()).filter(|&i| polys[i].is_none()));
    DirectionPlan { reverse, order }
}

/// Applies a [`DirectionPlan`] (usually worked out on another master of the same glyph,
/// so all masters change the same way). Does nothing if the number of paths differs.
pub fn apply_direction(layer: &mut Layer, plan: &DirectionPlan) {
    let slots: Vec<usize> = (0..layer.shapes.len())
        .filter(|&i| matches!(layer.shapes[i], Shape::Path(_)))
        .collect();
    if slots.len() != plan.order.len() {
        return;
    }
    let mut paths: Vec<Path> = layer_paths(layer).into_iter().cloned().collect();
    for &i in &plan.reverse {
        paths[i] = reverse(&paths[i]);
    }
    for (slot, &from) in slots.iter().zip(&plan.order) {
        layer.shapes[*slot] = Shape::Path(paths[from].clone());
    }
}

/// Correct Path Direction on one layer. Returns whether anything changed.
pub fn correct_direction(layer: &mut Layer) -> bool {
    let before = layer.clone();
    let plan = direction_plan(layer);
    apply_direction(layer, &plan);
    *layer != before
}

fn layer_paths(layer: &Layer) -> Vec<&Path> {
    layer
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .collect()
}

type Pt = (f64, f64);

fn lerp(p: Pt, q: Pt, t: f64) -> Pt {
    (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t)
}

fn cubic(a: Pt, b: Pt, c: Pt, d: Pt, t: f64) -> Pt {
    let mt = 1.0 - t;
    let f = |p: f64, q: f64, r: f64, s: f64| {
        mt * mt * mt * p + 3.0 * mt * mt * t * q + 3.0 * mt * t * t * r + t * t * t * s
    };
    (f(a.0, b.0, c.0, d.0), f(a.1, b.1, c.1, d.1))
}

/// Parameters in (0, 1) where one coordinate of a cubic has a zero derivative.
fn cubic_ts(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    // B'(t)/3 = a t² + b t + c
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    if a.abs() < 1e-12 {
        return if b.abs() > 1e-12 {
            vec![-c / b]
        } else {
            Vec::new()
        };
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Vec::new();
    }
    let r = disc.sqrt();
    vec![(-b + r) / (2.0 * a), (-b - r) / (2.0 * a)]
}

/// The parameter where one coordinate of a quadratic has a zero derivative.
fn quad_ts(p0: f64, p1: f64, p2: f64) -> Vec<f64> {
    let d = p0 - 2.0 * p1 + p2;
    if d.abs() < 1e-12 {
        Vec::new()
    } else {
        vec![(p0 - p1) / d]
    }
}

fn keep_inside(mut ts: Vec<f64>) -> Vec<f64> {
    ts.retain(|t| *t > 0.01 && *t < 0.99);
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|x, y| (*x - *y).abs() < 0.01);
    ts
}

/// One curve piece: a cubic (two handles) or a quadratic (one handle).
#[derive(Debug, Clone, Copy)]
enum Piece {
    Cubic(Pt, Pt, Pt, Pt),
    Quad(Pt, Pt, Pt),
}

impl Piece {
    fn extremes(&self) -> Vec<f64> {
        keep_inside(match *self {
            Piece::Cubic(a, b, c, d) => {
                let mut t = cubic_ts(a.0, b.0, c.0, d.0);
                t.extend(cubic_ts(a.1, b.1, c.1, d.1));
                t
            }
            Piece::Quad(a, b, c) => {
                let mut t = quad_ts(a.0, b.0, c.0);
                t.extend(quad_ts(a.1, b.1, c.1));
                t
            }
        })
    }

    /// This piece's nodes after splitting it at `ts`, up to (not including) its end.
    fn split_nodes(&self, ts: &[f64]) -> Vec<Node> {
        let mut out = Vec::new();
        let mut piece = *self;
        let mut done = 0.0;
        for &t in ts {
            let local = (t - done) / (1.0 - done);
            piece = match piece {
                Piece::Cubic(a, b, c, d) => {
                    let (ab, bc, cd) = (lerp(a, b, local), lerp(b, c, local), lerp(c, d, local));
                    let (abc, bcd) = (lerp(ab, bc, local), lerp(bc, cd, local));
                    let m = lerp(abc, bcd, local);
                    out.push(Node::new(ab.0, ab.1, NodeKind::OffCurve));
                    out.push(Node::new(abc.0, abc.1, NodeKind::OffCurve));
                    out.push(Node {
                        x: m.0,
                        y: m.1,
                        kind: NodeKind::Curve,
                        smooth: true,
                    });
                    Piece::Cubic(m, bcd, cd, d)
                }
                Piece::Quad(a, b, c) => {
                    let (ab, bc) = (lerp(a, b, local), lerp(b, c, local));
                    let m = lerp(ab, bc, local);
                    out.push(Node::new(ab.0, ab.1, NodeKind::OffCurve));
                    out.push(Node {
                        x: m.0,
                        y: m.1,
                        kind: NodeKind::QCurve,
                        smooth: true,
                    });
                    Piece::Quad(m, bc, c)
                }
            };
            done = t;
        }
        match piece {
            Piece::Cubic(_, b, c, _) => {
                out.push(Node::new(b.0, b.1, NodeKind::OffCurve));
                out.push(Node::new(c.0, c.1, NodeKind::OffCurve));
            }
            Piece::Quad(_, b, _) => out.push(Node::new(b.0, b.1, NodeKind::OffCurve)),
        }
        out
    }
}

/// A segment: the index of the on-curve node that ends it, its handles, and its curve
/// pieces. A quadratic segment with several handles has one piece per handle, joined
/// at the implied on-curve midpoints.
struct Segment {
    end: usize,
    offs: Vec<Node>,
    pieces: Vec<Piece>,
}

fn segments(p: &Path) -> Option<(Vec<usize>, Vec<Segment>)> {
    let n = p.nodes.len();
    let ons: Vec<usize> = (0..n).filter(|&i| p.nodes[i].on_curve()).collect();
    if ons.is_empty() {
        return None;
    }
    let pt = |n: &Node| (n.x, n.y);
    let mut segs = Vec::new();
    for (k, &end) in ons.iter().enumerate() {
        if k == 0 && !p.closed {
            continue;
        }
        let prev = if k == 0 {
            ons[ons.len() - 1]
        } else {
            ons[k - 1]
        };
        let offs: Vec<Node> = (1..)
            .map(|d| (prev + d) % n)
            .take_while(|&i| i != end)
            .map(|i| p.nodes[i])
            .collect();
        let (s, e) = (p.nodes[prev], p.nodes[end]);
        let pieces = match e.kind {
            NodeKind::Curve if offs.len() == 2 => {
                vec![Piece::Cubic(pt(&s), pt(&offs[0]), pt(&offs[1]), pt(&e))]
            }
            NodeKind::QCurve if !offs.is_empty() => {
                let mut v = Vec::new();
                let mut start = pt(&s);
                for (i, o) in offs.iter().enumerate() {
                    let stop = match offs.get(i + 1) {
                        Some(next) => lerp(pt(o), pt(next), 0.5),
                        None => pt(&e),
                    };
                    v.push(Piece::Quad(start, pt(o), stop));
                    start = stop;
                }
                v
            }
            _ => Vec::new(),
        };
        segs.push(Segment { end, offs, pieces });
    }
    Some((ons, segs))
}

/// Where Add Extremes splits curves: per path, per segment, per piece, the curve
/// parameters. Paths not in `only` (indices among the layer's paths) get none.
pub type ExtremesPlan = Vec<Vec<Vec<Vec<f64>>>>;

pub fn extremes_plan(layer: &Layer, only: Option<&[usize]>) -> ExtremesPlan {
    layer_paths(layer)
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if only.is_some_and(|o| !o.contains(&i)) {
                return Vec::new();
            }
            segments(p)
                .map(|(_, segs)| {
                    segs.iter()
                        .map(|s| s.pieces.iter().map(Piece::extremes).collect())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// Adds points at extremes following `plan` (usually worked out on another master),
/// so every master gets the same new points. Each piece is split at this layer's own
/// extremes when it has as many as the plan, otherwise at the plan's parameters.
/// Returns how many points were added.
pub fn apply_extremes(layer: &mut Layer, plan: &ExtremesPlan) -> usize {
    let mut added = 0;
    let mut path_index = 0;
    for s in &mut layer.shapes {
        let Shape::Path(p) = s else { continue };
        let empty = Vec::new();
        let path_plan = plan.get(path_index).unwrap_or(&empty);
        path_index += 1;
        if path_plan.iter().all(|seg| seg.iter().all(Vec::is_empty)) {
            continue;
        }
        let Some((ons, segs)) = segments(p) else {
            continue;
        };
        if segs.len() != path_plan.len() {
            continue;
        }
        let mut out: Vec<Node> = Vec::with_capacity(p.nodes.len() + 8);
        let mut start_pos = None;
        if !p.closed {
            out.push(p.nodes[ons[0]]);
            start_pos = Some(0);
        }
        for (seg, seg_plan) in segs.iter().zip(path_plan) {
            let e = p.nodes[seg.end];
            let split =
                seg_plan.iter().any(|ts| !ts.is_empty()) && seg.pieces.len() == seg_plan.len();
            if !split {
                out.extend(seg.offs.iter().copied());
            } else {
                for (k, (piece, planned)) in seg.pieces.iter().zip(seg_plan).enumerate() {
                    let own = piece.extremes();
                    let ts = if own.len() == planned.len() {
                        own
                    } else {
                        planned.clone()
                    };
                    added += ts.len();
                    out.extend(piece.split_nodes(&ts));
                    // Splitting makes the implied on-curve points between quadratic
                    // handles real ones.
                    if k + 1 < seg.pieces.len() {
                        if let Piece::Quad(_, _, m) = piece {
                            out.push(Node {
                                x: m.0,
                                y: m.1,
                                kind: NodeKind::QCurve,
                                smooth: true,
                            });
                        }
                    }
                }
            }
            if seg.end == 0 {
                start_pos = Some(out.len());
            }
            out.push(e);
        }
        if p.closed && p.nodes[0].on_curve() {
            if let Some(i) = start_pos {
                out.rotate_left(i);
            }
        }
        p.nodes = out;
    }
    added
}

/// Add Extremes on one layer: the paths in `only` (indices among its paths), or all.
/// Returns how many points were added.
pub fn add_extremes(layer: &mut Layer, only: Option<&[usize]>) -> usize {
    let plan = extremes_plan(layer, only);
    apply_extremes(layer, &plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: f64, y: f64, kind: NodeKind) -> Node {
        Node::new(x, y, kind)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64, ccw: bool) -> Shape {
        let mut pts = vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        if !ccw {
            pts.reverse();
        }
        Shape::Path(Path {
            closed: true,
            nodes: pts
                .into_iter()
                .map(|(x, y)| node(x, y, NodeKind::Line))
                .collect(),
        })
    }

    fn path(s: &Shape) -> &Path {
        let Shape::Path(p) = s else {
            panic!("not a path")
        };
        p
    }

    fn signed(s: &Shape) -> f64 {
        area(&polygon(path(s)))
    }

    #[test]
    fn corrects_outer_and_counter() {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![
            rect(0.0, 0.0, 500.0, 700.0, false),
            rect(100.0, 100.0, 400.0, 600.0, true),
        ];
        assert!(correct_direction(&mut l));
        assert!(signed(&l.shapes[0]) > 0.0, "outer is counter-clockwise");
        assert!(signed(&l.shapes[1]) < 0.0, "counter is clockwise");
        assert!(!correct_direction(&mut l), "already correct");
        // The clockwise rectangle started at (0, 700); it still does.
        let p = path(&l.shapes[0]);
        assert_eq!(
            (p.nodes[0].x, p.nodes[0].y),
            (0.0, 700.0),
            "start node kept"
        );
    }

    #[test]
    fn reorders_contours_left_to_right_with_counters_after_their_outline() {
        // Listed: right counter, right outline, open stroke, left outline.
        let mut l = Layer::new("m01", 1000.0);
        let open = Shape::Path(Path {
            closed: false,
            nodes: vec![
                node(0.0, 900.0, NodeKind::Line),
                node(50.0, 900.0, NodeKind::Line),
            ],
        });
        let component = Shape::Component {
            base: "acute".into(),
            transform: crate::model::IDENTITY,
        };
        l.shapes = vec![
            rect(600.0, 100.0, 800.0, 600.0, false),
            component.clone(),
            rect(500.0, 0.0, 900.0, 700.0, true),
            open.clone(),
            rect(0.0, 0.0, 400.0, 700.0, true),
        ];
        let plan = direction_plan(&l);
        assert_eq!(
            plan.order,
            [3, 1, 0, 2],
            "left outline, right outline, its counter, open path"
        );
        assert!(plan.reverse.is_empty());
        apply_direction(&mut l, &plan);
        assert_eq!(path(&l.shapes[0]).nodes[0].x, 0.0);
        assert_eq!(l.shapes[1], component, "components keep their place");
        assert_eq!(path(&l.shapes[2]).nodes[0].x, 500.0);
        assert_eq!(path(&l.shapes[3]).nodes[0].x, 600.0);
        assert_eq!(l.shapes[4], open);
    }

    #[test]
    fn reversing_keeps_curve_segments() {
        // A closed path: line, then a curve back to the start.
        let p = Path {
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Curve),
                node(100.0, 0.0, NodeKind::Line),
                node(100.0, 50.0, NodeKind::OffCurve),
                node(0.0, 50.0, NodeKind::OffCurve),
            ],
        };
        let r = reverse(&p);
        let kinds: Vec<NodeKind> = r.nodes.iter().map(|n| n.kind).collect();
        assert_eq!((r.nodes[0].x, r.nodes[0].y), (0.0, 0.0));
        // 0,0 → (offs 0,50 / 100,50) → 100,0 curve → back to 0,0 as a line.
        assert_eq!(
            kinds,
            [
                NodeKind::Line,
                NodeKind::OffCurve,
                NodeKind::OffCurve,
                NodeKind::Curve
            ]
        );
        assert_eq!((r.nodes[1].x, r.nodes[1].y), (0.0, 50.0));
        assert_eq!(reverse(&r), p, "reversing twice gives the original");
    }

    fn bulge(h: f64) -> Layer {
        // A cubic from (0,0) to (200,0) bulging up to y=0.75h at t=0.5.
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(0.0, h, NodeKind::OffCurve),
                node(200.0, h, NodeKind::OffCurve),
                node(200.0, 0.0, NodeKind::Curve),
            ],
        })];
        l
    }

    #[test]
    fn adds_extremes_to_a_cubic() {
        let mut l = bulge(200.0);
        assert_eq!(add_extremes(&mut l, None), 1);
        let p = path(&l.shapes[0]);
        assert_eq!(p.nodes.len(), 7);
        let top = p.nodes[3];
        assert_eq!(
            (top.x, top.y, top.kind, top.smooth),
            (100.0, 150.0, NodeKind::Curve, true)
        );
        // The new handles are level at the extreme.
        assert_eq!(p.nodes[2].y, 150.0);
        assert_eq!(p.nodes[4].y, 150.0);
        assert_eq!((p.nodes[0].x, p.nodes[0].y), (0.0, 0.0), "start node kept");
        assert_eq!(add_extremes(&mut l, None), 0, "nothing left to add");
    }

    #[test]
    fn adds_extremes_to_quadratics() {
        // A TrueType-style bump: two handles with an implied on-curve point between them.
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(0.0, 200.0, NodeKind::OffCurve),
                node(200.0, 200.0, NodeKind::OffCurve),
                node(200.0, 0.0, NodeKind::QCurve),
            ],
        })];
        // Pieces: (0,0)-(0,200)-(100,200) and (100,200)-(200,200)-(200,0). Their
        // extremes are at the shared point, so nothing is added there.
        assert_eq!(add_extremes(&mut l, None), 0);

        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(100.0, 200.0, NodeKind::OffCurve),
                node(200.0, 0.0, NodeKind::QCurve),
            ],
        })];
        assert_eq!(add_extremes(&mut l, None), 1);
        let p = path(&l.shapes[0]);
        let kinds: Vec<NodeKind> = p.nodes.iter().map(|n| n.kind).collect();
        assert_eq!(
            kinds,
            [
                NodeKind::Line,
                NodeKind::OffCurve,
                NodeKind::QCurve,
                NodeKind::OffCurve,
                NodeKind::QCurve
            ]
        );
        assert_eq!((p.nodes[2].x, p.nodes[2].y), (100.0, 100.0));
        assert!(p.nodes[2].smooth);
        assert_eq!(p.nodes[1].y, 100.0, "level handle");
    }

    #[test]
    fn the_same_plan_keeps_masters_compatible() {
        // Regular bulges to 150, Bold to 225: same structure after Add Extremes.
        let (mut regular, mut bold) = (bulge(200.0), bulge(300.0));
        let plan = extremes_plan(&regular, None);
        assert_eq!(apply_extremes(&mut regular, &plan), 1);
        assert_eq!(apply_extremes(&mut bold, &plan), 1);
        let (r, b) = (path(&regular.shapes[0]), path(&bold.shapes[0]));
        assert_eq!(r.nodes.len(), b.nodes.len());
        assert_eq!(
            (b.nodes[3].x, b.nodes[3].y),
            (100.0, 225.0),
            "Bold's own extreme"
        );
        assert!(r.nodes.iter().zip(&b.nodes).all(|(x, y)| x.kind == y.kind));

        // A master with no extreme on that curve still gets a point (at the plan's t).
        let mut flat = bulge(0.0);
        assert_eq!(apply_extremes(&mut flat, &plan), 1);
        assert_eq!(path(&flat.shapes[0]).nodes.len(), r.nodes.len());
    }
}
