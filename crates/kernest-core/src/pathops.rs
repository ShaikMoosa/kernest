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

/// Whether `p` lies on one of the polygon's edges.
fn on_edge(poly: &[(f64, f64)], (x, y): (f64, f64)) -> bool {
    let n = poly.len();
    (0..n).any(|i| {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % n];
        let cross = (x1 - x0) * (y - y0) - (y1 - y0) * (x - x0);
        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt().max(1e-12);
        cross.abs() / len < 1e-6
            && x >= x0.min(x1) - 1e-6
            && x <= x0.max(x1) + 1e-6
            && y >= y0.min(y1) - 1e-6
            && y <= y0.max(y1) + 1e-6
    })
}

/// Whether contour `inner` is nested inside `outer`: smaller, every point inside or on
/// `outer`, and at least one point strictly inside. Contours that only touch or
/// partly overlap (like the bars of an H) are not nested.
fn nested(inner: &[(f64, f64)], outer: &[(f64, f64)]) -> bool {
    if area(inner).abs() >= area(outer).abs() {
        return false;
    }
    let mut strictly = false;
    for &p in inner {
        if on_edge(outer, p) {
            continue;
        }
        if !contains(outer, p) {
            return false;
        }
        strictly = true;
    }
    strictly
}

/// Reverses a path's direction, keeping its start node first when it is on-curve.
pub fn reverse(p: &Path) -> Path {
    let n = p.nodes.len();
    let ons: Vec<usize> = (0..n).filter(|&i| p.nodes[i].on_curve()).collect();
    if ons.is_empty() {
        let mut nodes = p.nodes.clone();
        nodes.reverse();
        return Path {
            locked: p.locked,
            stroke: p.stroke.clone(),
            mask: p.mask,
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
    // Pen-point widths follow their nodes to the new on-curve numbering.
    let stroke = p.stroke.clone().map(|mut s| {
        let new_index = |i: usize| {
            if p.closed && p.nodes[0].on_curve() {
                (n - i) % n
            } else {
                n - 1 - i
            }
        };
        s.widths = s
            .widths
            .iter()
            .filter_map(|(&k, &w)| {
                let ni = new_index(*ons.get(k)?);
                Some((nodes[..ni].iter().filter(|x| x.on_curve()).count(), w))
            })
            .collect();
        s
    });
    Path {
        locked: p.locked,
        stroke,
        mask: p.mask,
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
        let around: Vec<usize> = polys
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && o.as_ref().is_some_and(|o| nested(poly, o)))
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
                        g2: false,
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
                        g2: false,
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
                                g2: false,
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

// ---------- Knife ----------

use kurbo::{CubicBez, Line, PathSeg, Point, QuadBez};

fn kpt(p: Pt) -> Point {
    Point::new(p.0, p.1)
}

/// Distance from a point to the infinite knife line.
fn off_line(line: Line, p: Pt) -> f64 {
    let d = line.p1 - line.p0;
    let len = d.hypot().max(1e-12);
    ((p.0 - line.p0.x) * d.y - (p.1 - line.p0.y) * d.x).abs() / len
}

/// The path's nodes with the knife's crossing points inserted, and the indices of
/// those crossings. Closed paths are walked from node 0 (rotated on-curve first).
fn knife_nodes(p: &Path, line: Line) -> Option<(Vec<Node>, Vec<usize>)> {
    let (ons, segs) = segments(p)?;
    // segments() puts the wrap-around segment first for closed paths; walk it last.
    let order: Vec<&Segment> = if p.closed {
        segs.iter().skip(1).chain(segs.iter().take(1)).collect()
    } else {
        segs.iter().collect()
    };
    let mut out: Vec<Node> = vec![p.nodes[ons[0]]];
    let mut cuts: Vec<usize> = Vec::new();
    if off_line(line, (out[0].x, out[0].y)) < 1e-6 {
        cuts.push(0);
    }
    for seg in order {
        let end = p.nodes[seg.end];
        let s = seg_start(p, seg, &ons);
        let start = (s.x, s.y);
        let mut hits: Vec<(usize, f64)> = Vec::new();
        for (k, piece) in seg.pieces.iter().enumerate() {
            let ks = match *piece {
                Piece::Cubic(a, b, c, d) => {
                    PathSeg::Cubic(CubicBez::new(kpt(a), kpt(b), kpt(c), kpt(d)))
                }
                Piece::Quad(a, b, c) => PathSeg::Quad(QuadBez::new(kpt(a), kpt(b), kpt(c))),
            };
            for h in ks.intersect_line(line) {
                if h.segment_t > 1e-4 && h.segment_t < 1.0 - 1e-4 {
                    hits.push((k, h.segment_t));
                }
            }
        }
        let line_hit = if seg.pieces.is_empty() {
            let ks = PathSeg::Line(Line::new(kpt(start), kpt((end.x, end.y))));
            ks.intersect_line(line)
                .into_iter()
                .find(|h| h.segment_t > 1e-4 && h.segment_t < 1.0 - 1e-4)
                .map(|h| {
                    let m = lerp(start, (end.x, end.y), h.segment_t);
                    Node::new(m.0, m.1, NodeKind::Line)
                })
        } else {
            None
        };
        let end_on_line = off_line(line, (end.x, end.y)) < 1e-6;
        if hits.is_empty() && line_hit.is_none() {
            out.extend(seg.offs.iter().copied());
        } else if seg.pieces.is_empty() {
            if let Some(m) = line_hit {
                cuts.push(out.len());
                out.push(m);
            }
        } else {
            for (k, piece) in seg.pieces.iter().enumerate() {
                let ts: Vec<f64> = hits
                    .iter()
                    .filter(|(pk, _)| *pk == k)
                    .map(|(_, t)| *t)
                    .collect();
                let base = out.len();
                let per = match piece {
                    Piece::Cubic(..) => 3,
                    Piece::Quad(..) => 2,
                };
                out.extend(piece.split_nodes(&ts));
                for j in 0..ts.len() {
                    cuts.push(base + (j + 1) * per - 1);
                }
                // Between quadratic pieces the implied on-curve point becomes real.
                if k + 1 < seg.pieces.len() {
                    if let (Piece::Quad(_, h, _), Piece::Quad(_, h2, _)) =
                        (&seg.pieces[k], &seg.pieces[k + 1])
                    {
                        let m = lerp(*h, *h2, 0.5);
                        out.push(Node {
                            x: m.0,
                            y: m.1,
                            kind: NodeKind::QCurve,
                            smooth: true,
                            g2: false,
                        });
                    }
                }
            }
        }
        if end_on_line {
            cuts.push(out.len());
        }
        out.push(end);
    }
    if p.closed {
        // The walk ends back at node 0; drop the duplicate and fold its cut into 0.
        out.pop();
        if cuts.last() == Some(&out.len()) {
            cuts.pop();
            cuts.push(0);
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    Some((out, cuts))
}

/// The on-curve node a segment starts from.
fn seg_start(p: &Path, seg: &Segment, ons: &[usize]) -> Node {
    let k = ons.iter().position(|&i| i == seg.end).unwrap_or(0);
    let prev = if k == 0 {
        ons[ons.len() - 1]
    } else {
        ons[k - 1]
    };
    p.nodes[prev]
}

/// Cuts one path along the knife line. Closed paths become one closed path per arc
/// between consecutive crossings (each closed by a straight chord on the line);
/// open paths become open pieces. `None` when the line doesn't cut the path.
fn knife_path(p: &Path, line: Line) -> Option<Vec<Path>> {
    let mut owned;
    let p = if p.closed && !p.nodes.first()?.on_curve() {
        let first = p.nodes.iter().position(|n| n.on_curve())?;
        owned = p.clone();
        owned.nodes.rotate_left(first);
        &owned
    } else {
        p
    };
    let (nodes, cuts) = knife_nodes(p, line)?;
    if cuts.len() < 2 && p.closed {
        return None;
    }
    if cuts.is_empty() {
        return None;
    }
    let n = nodes.len();
    let mut out = Vec::new();
    if p.closed {
        for w in 0..cuts.len() {
            let a = cuts[w];
            let b = cuts[(w + 1) % cuts.len()];
            let mut arc: Vec<Node> = Vec::new();
            let mut i = a;
            loop {
                arc.push(nodes[i]);
                if i == b {
                    break;
                }
                i = (i + 1) % n;
            }
            // An arc lying entirely on the knife line is the edge the knife ran
            // along, not a real side; cutting along an edge must change nothing.
            if arc.iter().all(|n| off_line(line, (n.x, n.y)) < 1e-6) {
                continue;
            }
            // The chord back to the arc's start is a straight line.
            arc[0].kind = NodeKind::Line;
            arc[0].smooth = false;
            out.push(Path {
                locked: false,
                stroke: None,
                mask: false,
                closed: true,
                nodes: arc,
            });
        }
        if out.len() < 2 {
            return None;
        }
    } else {
        let mut bounds: Vec<usize> = vec![0];
        bounds.extend(cuts.iter().copied());
        bounds.push(n - 1);
        for w in bounds.windows(2) {
            let piece: Vec<Node> = nodes[w[0]..=w[1]].to_vec();
            if piece.len() < 2 {
                continue;
            }
            let mut piece = piece;
            piece[0].kind = NodeKind::Line;
            piece[0].smooth = false;
            out.push(Path {
                locked: false,
                stroke: None,
                mask: false,
                closed: false,
                nodes: piece,
            });
        }
    }
    Some(out)
}

/// Knife: cuts every unlocked path the line through `p0`–`p1` crosses, closing each
/// side. Returns whether anything changed.
pub fn cut(layer: &mut Layer, p0: (f64, f64), p1: (f64, f64)) -> bool {
    if (p1.0 - p0.0).hypot(p1.1 - p0.1) < 1e-9 {
        return false;
    }
    let line = Line::new(kpt(p0), kpt(p1));
    let mut changed = false;
    let mut shapes: Vec<Shape> = Vec::with_capacity(layer.shapes.len());
    for s in layer.shapes.drain(..) {
        match s {
            Shape::Path(p) if !p.locked => match knife_path(&p, line) {
                Some(parts) => {
                    changed = true;
                    shapes.extend(parts.into_iter().map(Shape::Path));
                }
                None => shapes.push(Shape::Path(p)),
            },
            other => shapes.push(other),
        }
    }
    layer.shapes = shapes;
    changed
}

// ---------- Corners and connections ----------

/// Opens a corner into two overlapping nodes: the original keeps the incoming
/// segment, the new node starts the outgoing one. One undoable step at the command
/// layer. Errors on locked paths and off-curve nodes.
pub fn open_corner(layer: &mut Layer, path: usize, node: usize) -> Result<(), String> {
    let p = path_at(layer, path)?;
    if p.locked {
        return Err("Unlock the path first.".into());
    }
    let n = p
        .nodes
        .get(node)
        .ok_or_else(|| "No node at that position.".to_string())?;
    if !n.on_curve() {
        return Err("Pick an on-curve node.".into());
    }
    if !p.closed && (node == 0 || node == p.nodes.len() - 1) {
        return Err("An open path's ends have no corner to open.".into());
    }
    let dup = Node::new(n.x, n.y, NodeKind::Line);
    let p = path_at_mut(layer, path)?;
    p.nodes[node].smooth = false;
    p.nodes.insert(node + 1, dup);
    Ok(())
}

/// Reconnect Nodes: merges neighbouring duplicate on-curve nodes (what Open Corner
/// made) back into one. Returns how many were merged.
pub fn reconnect_nodes(layer: &mut Layer, only: Option<&[usize]>) -> usize {
    let mut merged = 0;
    let mut pi = 0;
    for s in &mut layer.shapes {
        let Shape::Path(p) = s else { continue };
        let i = pi;
        pi += 1;
        if p.locked || only.is_some_and(|o| !o.contains(&i)) {
            continue;
        }
        let mut kept: Vec<Node> = Vec::with_capacity(p.nodes.len());
        for n in p.nodes.drain(..) {
            if let Some(last) = kept.last() {
                if last.on_curve()
                    && n.on_curve()
                    && n.kind == NodeKind::Line
                    && (last.x - n.x).abs() < 1e-6
                    && (last.y - n.y).abs() < 1e-6
                {
                    merged += 1;
                    continue;
                }
            }
            kept.push(n);
        }
        // A closed path can end on the duplicate of its start.
        if p.closed && kept.len() > 1 {
            let (a, b) = (kept[0], kept[kept.len() - 1]);
            if a.on_curve()
                && b.on_curve()
                && b.kind == NodeKind::Line
                && (a.x - b.x).abs() < 1e-6
                && (a.y - b.y).abs() < 1e-6
            {
                kept.pop();
                merged += 1;
            }
        }
        p.nodes = kept;
    }
    merged
}

/// Connect Nodes: joins two open path ends into one path. The ends may belong to the
/// same path, which closes it. Errors unless both picks are ends of open paths.
pub fn connect_nodes(
    layer: &mut Layer,
    a: (usize, usize),
    b: (usize, usize),
) -> Result<(), String> {
    let pa = path_at(layer, a.0)?;
    let pb = path_at(layer, b.0)?;
    if pa.locked || pb.locked {
        return Err("Unlock the path first.".into());
    }
    for (p, n) in [(pa, a.1), (pb, b.1)] {
        if p.closed {
            return Err("Pick the ends of open paths.".into());
        }
        if n != 0 && n != p.nodes.len() - 1 {
            return Err("Pick the ends of open paths.".into());
        }
    }
    if a.0 == b.0 {
        let p = path_at_mut(layer, a.0)?;
        p.closed = true;
        p.nodes[0].kind = NodeKind::Line;
        p.nodes[0].smooth = false;
        return Ok(());
    }
    let mut pa = pa.clone();
    let mut pb = pb.clone();
    if a.1 == 0 {
        pa = reverse(&pa);
    }
    if b.1 == pb.nodes.len() - 1 {
        pb = reverse(&pb);
    }
    let join = pa.nodes.last().copied().unwrap();
    let skip = pb
        .nodes
        .first()
        .is_some_and(|n| (n.x - join.x).abs() < 1e-6 && (n.y - join.y).abs() < 1e-6);
    pa.nodes
        .extend(pb.nodes.into_iter().skip(usize::from(skip)));
    let (keep, drop) = if a.0 < b.0 { (a.0, b.0) } else { (b.0, a.0) };
    let mut pi = 0;
    layer.shapes.retain_mut(|s| {
        let Shape::Path(_) = s else { return true };
        let i = pi;
        pi += 1;
        if i == drop {
            return false;
        }
        if i == keep {
            *s = Shape::Path(pa.clone());
        }
        true
    });
    Ok(())
}

/// Close Open Paths: closes the open paths in `only` (indices among the layer's
/// paths), or all of them. Returns how many were closed.
pub fn close_open_paths(layer: &mut Layer, only: Option<&[usize]>) -> usize {
    let mut closed = 0;
    let mut pi = 0;
    for s in &mut layer.shapes {
        let Shape::Path(p) = s else { continue };
        let i = pi;
        pi += 1;
        if p.closed || p.locked || only.is_some_and(|o| !o.contains(&i)) {
            continue;
        }
        if p.nodes.len() < 3 {
            continue;
        }
        p.closed = true;
        p.nodes[0].kind = NodeKind::Line;
        p.nodes[0].smooth = false;
        closed += 1;
    }
    closed
}

/// Make Node First: rotates a closed path so the chosen on-curve node starts it.
pub fn make_node_first(layer: &mut Layer, path: usize, node: usize) -> Result<(), String> {
    let p = path_at(layer, path)?;
    if p.locked {
        return Err("Unlock the path first.".into());
    }
    if !p.closed {
        return Err("An open path already has a first node.".into());
    }
    let n = p
        .nodes
        .get(node)
        .ok_or_else(|| "No node at that position.".to_string())?;
    if !n.on_curve() {
        return Err("Pick an on-curve node.".into());
    }
    let p = path_at_mut(layer, path)?;
    p.nodes.rotate_left(node);
    Ok(())
}

/// Reverse Selected Contours: reverses the paths in `only`, or all unlocked paths.
/// Returns how many were reversed.
pub fn reverse_contours(layer: &mut Layer, only: Option<&[usize]>) -> usize {
    let mut done = 0;
    let mut pi = 0;
    for s in &mut layer.shapes {
        let Shape::Path(p) = s else { continue };
        let i = pi;
        pi += 1;
        if p.locked || only.is_some_and(|o| !o.contains(&i)) {
            continue;
        }
        *p = reverse(p);
        done += 1;
    }
    done
}

// ---------- Tidy up and rounding ----------

fn same_pos(a: &Node, b: &Node) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
}

/// One tidy pass over a node list: duplicate on-curve nodes, collinear line nodes and
/// handles sitting on their own node. Returns how many changes were made (nodes
/// removed, plus curves flattened to lines).
fn tidy_nodes(nodes: &mut Vec<Node>, closed: bool) -> usize {
    let before = nodes.len();
    // Handles on top of an adjacent on-curve node carry no shape.
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].kind == NodeKind::OffCurve {
            let prev = (i + nodes.len() - 1) % nodes.len();
            let next = (i + 1) % nodes.len();
            if (nodes[prev].on_curve() && same_pos(&nodes[i], &nodes[prev]))
                || (nodes[next].on_curve() && same_pos(&nodes[i], &nodes[next]))
            {
                nodes.remove(i);
                continue;
            }
        }
        i += 1;
    }
    // A curve node with fewer than two handles left is a line node.
    let mut flattened = 0;
    for k in 0..nodes.len() {
        if nodes[k].kind == NodeKind::Curve {
            let prev = (k + nodes.len() - 1) % nodes.len();
            let prev2 = (k + nodes.len() - 2) % nodes.len();
            if nodes[prev].on_curve() || nodes[prev2].kind != NodeKind::OffCurve {
                nodes[k].kind = NodeKind::Line;
                nodes[k].smooth = false;
                flattened += 1;
            }
        }
    }
    // Duplicate on-curve nodes and collinear line nodes, walking neighbouring
    // on-curve nodes with nothing (or only removed handles) between them.
    let mut kept: Vec<Node> = Vec::with_capacity(nodes.len());
    for n in nodes.drain(..) {
        if n.on_curve() {
            if let Some(last) = kept.last() {
                if last.on_curve() && same_pos(last, &n) {
                    continue;
                }
            }
            if n.kind == NodeKind::Line && !n.smooth && kept.len() >= 2 {
                let a = kept[kept.len() - 2];
                let b = kept[kept.len() - 1];
                if a.on_curve()
                    && b.on_curve()
                    && b.kind == NodeKind::Line
                    && !b.smooth
                    && kept.iter().rev().take(2).all(|k| k.on_curve())
                {
                    let cross = (b.x - a.x) * (n.y - a.y) - (b.y - a.y) * (n.x - a.x);
                    let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2))
                        .sqrt()
                        .max(1e-12);
                    let between = (b.x - a.x) * (n.x - a.x) + (b.y - a.y) * (n.y - a.y) >= 0.0
                        && (n.x - b.x) * (n.x - a.x) + (n.y - b.y) * (n.y - a.y) >= 0.0;
                    if cross.abs() / len < 1e-6 && between {
                        kept.pop();
                        kept.push(n);
                        continue;
                    }
                }
            }
        }
        kept.push(n);
    }
    if closed && kept.len() > 1 {
        let (a, b) = (kept[0], kept[kept.len() - 1]);
        if a.on_curve() && b.on_curve() && same_pos(&a, &b) {
            kept.pop();
        }
    }
    *nodes = kept;
    before - nodes.len() + flattened
}

/// Tidy up Paths: removes duplicate nodes, collinear line nodes and handles on top of
/// their node, in the paths in `only` or all unlocked paths. Returns changes made.
pub fn tidy_up(layer: &mut Layer, only: Option<&[usize]>) -> usize {
    let mut removed = 0;
    let mut pi = 0;
    for s in &mut layer.shapes {
        let Shape::Path(p) = s else { continue };
        let i = pi;
        pi += 1;
        if p.locked || only.is_some_and(|o| !o.contains(&i)) {
            continue;
        }
        // Repeat until stable: removing a node can expose another tidy case.
        loop {
            let n = tidy_nodes(&mut p.nodes, p.closed);
            removed += n;
            if n == 0 {
                break;
            }
        }
    }
    removed
}

/// Round Coordinates: rounds node and anchor positions to whole units in the paths
/// in `only` or all unlocked paths (anchors always). Returns whether anything moved.
pub fn round_coordinates(layer: &mut Layer, only: Option<&[usize]>) -> bool {
    let mut changed = false;
    let mut pi = 0;
    for s in &mut layer.shapes {
        let Shape::Path(p) = s else { continue };
        let i = pi;
        pi += 1;
        if p.locked || only.is_some_and(|o| !o.contains(&i)) {
            continue;
        }
        for n in &mut p.nodes {
            let (x, y) = (n.x.round(), n.y.round());
            if x != n.x || y != n.y {
                changed = true;
            }
            n.x = x;
            n.y = y;
        }
    }
    for a in &mut layer.anchors {
        let (x, y) = (a.x.round(), a.y.round());
        if x != a.x || y != a.y {
            changed = true;
        }
        a.x = x;
        a.y = y;
    }
    changed
}

/// The layer's `path`-th path (counting paths only, not components).
fn path_at(layer: &Layer, path: usize) -> Result<&Path, String> {
    layer_paths(layer)
        .get(path)
        .copied()
        .ok_or_else(|| "No path at that position.".to_string())
}

fn path_at_mut(layer: &mut Layer, path: usize) -> Result<&mut Path, String> {
    layer
        .shapes
        .iter_mut()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .nth(path)
        .ok_or_else(|| "No path at that position.".to_string())
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
            locked: false,
            stroke: None,
            mask: false,
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
            locked: false,
            stroke: None,
            mask: false,
            closed: false,
            nodes: vec![
                node(0.0, 900.0, NodeKind::Line),
                node(50.0, 900.0, NodeKind::Line),
            ],
        });
        let component = Shape::component("acute", crate::model::IDENTITY);
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
    fn touching_and_overlapping_contours_are_not_counters() {
        // The sample H: two stems and a crossbar that starts on the left stem's edge
        // and overlaps both. All three are solid; none may be reversed.
        let mut l = Layer::new("m01", 680.0);
        l.shapes = vec![
            rect(80.0, 0.0, 170.0, 700.0, true),
            rect(510.0, 0.0, 600.0, 700.0, true),
            rect(80.0, 310.0, 600.0, 390.0, true),
        ];
        let plan = direction_plan(&l);
        assert!(plan.reverse.is_empty(), "{plan:?}");
        assert!(!correct_direction(&mut l) || l.shapes.iter().all(|s| signed(s) > 0.0));
        assert!(
            l.shapes.iter().all(|s| signed(s) > 0.0),
            "all solid, counter-clockwise"
        );
        // A counter that touches its outline at a corner is still a counter.
        let mut o = Layer::new("m01", 600.0);
        o.shapes = vec![
            rect(0.0, 0.0, 400.0, 600.0, true),
            rect(0.0, 100.0, 300.0, 500.0, true),
        ];
        assert!(correct_direction(&mut o));
        assert!(signed(&o.shapes[0]) > 0.0 && signed(&o.shapes[1]) < 0.0);
    }

    #[test]
    fn reversing_keeps_curve_segments() {
        // A closed path: line, then a curve back to the start.
        let p = Path {
            locked: false,
            stroke: None,
            mask: false,
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
            locked: false,
            stroke: None,
            mask: false,
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
            locked: false,
            stroke: None,
            mask: false,
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
            locked: false,
            stroke: None,
            mask: false,
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

    // ---------- Phase 5: knife, corners, connections, tidy, rounding ----------

    fn rect_layer() -> Layer {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![rect(100.0, 0.0, 500.0, 700.0, true)];
        l
    }

    fn coords(p: &Path) -> Vec<(f64, f64)> {
        p.nodes.iter().map(|n| (n.x, n.y)).collect()
    }

    #[test]
    fn knife_splits_a_rect_into_two_closed_paths() {
        let mut l = rect_layer();
        assert!(cut(&mut l, (300.0, -100.0), (300.0, 800.0)));
        assert_eq!(l.shapes.len(), 2);
        let (a, b) = (path(&l.shapes[0]), path(&l.shapes[1]));
        assert!(a.closed && b.closed);
        assert_eq!(a.nodes.len(), 4);
        assert_eq!(b.nodes.len(), 4);
        let mut left = coords(a);
        let mut right = coords(b);
        left.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)));
        right.sort_by(|p, q| p.0.total_cmp(&q.0).then(p.1.total_cmp(&q.1)));
        if left[0].0 > right[0].0 {
            std::mem::swap(&mut left, &mut right);
        }
        assert_eq!(
            left,
            [(100.0, 0.0), (100.0, 700.0), (300.0, 0.0), (300.0, 700.0)]
        );
        assert_eq!(
            right,
            [(300.0, 0.0), (300.0, 700.0), (500.0, 0.0), (500.0, 700.0)]
        );
        // Every node is a line node: the chords and the rect sides are straight.
        assert!(a
            .nodes
            .iter()
            .chain(&b.nodes)
            .all(|n| n.kind == NodeKind::Line));
        // Cutting again along the same line does nothing (the halves only touch it).
        assert!(!cut(&mut l, (300.0, -100.0), (300.0, 800.0)));
    }

    #[test]
    fn knife_closes_a_curve_cut_with_a_chord() {
        let mut l = bulge(200.0);
        assert!(cut(&mut l, (-50.0, 100.0), (250.0, 100.0)));
        assert_eq!(l.shapes.len(), 2);
        for s in &l.shapes {
            let p = path(s);
            assert!(p.closed);
            // Each side stays on its side of the knife, with the chord on y=100.
            let above = p
                .nodes
                .iter()
                .filter(|n| n.on_curve())
                .all(|n| n.y >= 100.0 - 1e-6);
            let below = p
                .nodes
                .iter()
                .filter(|n| n.on_curve())
                .all(|n| n.y <= 100.0 + 1e-6);
            assert!(above || below);
            assert!(
                p.nodes
                    .iter()
                    .filter(|n| n.on_curve() && (n.y - 100.0).abs() < 1e-6)
                    .count()
                    >= 2
            );
        }
        // The upper piece keeps curve nodes; the lower is the straight chord side.
        let upper = l
            .shapes
            .iter()
            .map(path)
            .find(|p| p.nodes.iter().any(|n| n.y > 100.0))
            .unwrap();
        assert!(upper.nodes.iter().any(|n| n.kind == NodeKind::Curve));
    }

    #[test]
    fn knife_misses_and_locked_paths_are_left_alone() {
        let mut l = rect_layer();
        let before = l.clone();
        assert!(!cut(&mut l, (600.0, -100.0), (600.0, 800.0)));
        assert_eq!(l, before);
        if let Shape::Path(p) = &mut l.shapes[0] {
            p.locked = true;
        }
        assert!(!cut(&mut l, (300.0, -100.0), (300.0, 800.0)));
        assert_eq!(l.shapes.len(), 1);
    }

    #[test]
    fn knife_splits_an_open_path_into_open_pieces() {
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: false,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line),
                node(200.0, 0.0, NodeKind::Line),
            ],
        })];
        assert!(cut(&mut l, (50.0, -10.0), (50.0, 10.0)));
        // One crossing: two open pieces.
        assert_eq!(l.shapes.len(), 2);
        assert!(l.shapes.iter().map(path).all(|p| !p.closed));
        assert_eq!(coords(path(&l.shapes[0])), [(0.0, 0.0), (50.0, 0.0)]);
        assert_eq!(
            coords(path(&l.shapes[1])),
            [(50.0, 0.0), (100.0, 0.0), (200.0, 0.0)]
        );
    }

    #[test]
    fn open_corner_duplicates_and_reconnect_restores() {
        let mut l = rect_layer();
        let before = l.clone();
        open_corner(&mut l, 0, 1).unwrap();
        let p = path(&l.shapes[0]);
        assert_eq!(p.nodes.len(), 5);
        assert_eq!((p.nodes[1].x, p.nodes[1].y), (500.0, 0.0));
        assert_eq!((p.nodes[2].x, p.nodes[2].y), (500.0, 0.0));
        assert_eq!(p.nodes[2].kind, NodeKind::Line);
        assert_eq!(reconnect_nodes(&mut l, None), 1);
        assert_eq!(l, before, "reconnect undoes open corner exactly");
    }

    #[test]
    fn open_corner_refuses_locked_paths_and_handles() {
        let mut l = rect_layer();
        if let Shape::Path(p) = &mut l.shapes[0] {
            p.locked = true;
        }
        assert!(open_corner(&mut l, 0, 1).is_err());
        let mut l = bulge(200.0);
        assert!(open_corner(&mut l, 0, 1).is_err(), "off-curve node");
        assert!(open_corner(&mut l, 5, 0).is_err(), "no such path");
    }

    #[test]
    fn connect_nodes_joins_two_open_ends() {
        let mut l = Layer::new("m01", 300.0);
        let open = |pts: &[(f64, f64)]| {
            Shape::Path(Path {
                locked: false,
                stroke: None,
                mask: false,
                closed: false,
                nodes: pts
                    .iter()
                    .map(|&(x, y)| node(x, y, NodeKind::Line))
                    .collect(),
            })
        };
        l.shapes = vec![
            open(&[(0.0, 0.0), (100.0, 0.0)]),
            open(&[(100.0, 0.0), (100.0, 100.0)]),
        ];
        connect_nodes(&mut l, (0, 1), (1, 0)).unwrap();
        assert_eq!(l.shapes.len(), 1);
        let p = path(&l.shapes[0]);
        assert!(!p.closed);
        assert_eq!(coords(p), [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]);
    }

    #[test]
    fn connect_nodes_on_one_path_closes_it() {
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: false,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line),
                node(100.0, 100.0, NodeKind::Line),
            ],
        })];
        connect_nodes(&mut l, (0, 0), (0, 2)).unwrap();
        assert!(path(&l.shapes[0]).closed);
        assert!(connect_nodes(&mut l, (0, 0), (0, 1)).is_err(), "closed now");
    }

    #[test]
    fn close_open_paths_closes_only_open_unlocked_ones() {
        let mut l = Layer::new("m01", 300.0);
        let tri = |locked: bool, closed: bool| {
            Shape::Path(Path {
                locked,
                stroke: None,
                mask: false,
                closed,
                nodes: vec![
                    node(0.0, 0.0, NodeKind::Line),
                    node(100.0, 0.0, NodeKind::Line),
                    node(100.0, 100.0, NodeKind::Line),
                ],
            })
        };
        l.shapes = vec![tri(false, false), tri(true, false), tri(false, true)];
        assert_eq!(close_open_paths(&mut l, None), 1);
        assert!(path(&l.shapes[0]).closed);
        assert!(!path(&l.shapes[1]).closed, "locked stays open");
    }

    #[test]
    fn make_node_first_rotates_keeping_segments() {
        let mut l = rect_layer();
        make_node_first(&mut l, 0, 2).unwrap();
        let p = path(&l.shapes[0]);
        assert_eq!(
            coords(p),
            [(500.0, 700.0), (100.0, 700.0), (100.0, 0.0), (500.0, 0.0)]
        );
        assert!(p.nodes.iter().all(|n| n.kind == NodeKind::Line));
        assert!(make_node_first(&mut l, 0, 9).is_err());
    }

    #[test]
    fn reverse_contours_respects_the_selection() {
        let mut l = rect_layer();
        l.shapes.push(rect(600.0, 0.0, 700.0, 700.0, true));
        let second = l.shapes[1].clone();
        assert_eq!(reverse_contours(&mut l, Some(&[0])), 1);
        assert_eq!(l.shapes[1], second);
        assert_eq!(
            coords(path(&l.shapes[0])),
            [(100.0, 0.0), (100.0, 700.0), (500.0, 700.0), (500.0, 0.0)]
        );
    }

    #[test]
    fn tidy_up_removes_duplicates_collinear_and_dead_handles() {
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(50.0, 0.0, NodeKind::Line), // collinear with 0,0 and 100,0
                node(100.0, 0.0, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line), // duplicate
                node(100.0, 100.0, NodeKind::Line),
            ],
        })];
        assert_eq!(tidy_up(&mut l, None), 2);
        assert_eq!(
            coords(path(&l.shapes[0])),
            [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]
        );

        // Both handles sit on their own node and go; the curve left with no
        // handles flattens to a line. Three changes in total.
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(0.0, 0.0, NodeKind::OffCurve),
                node(100.0, 0.0, NodeKind::OffCurve),
                node(100.0, 0.0, NodeKind::Curve),
                node(100.0, 100.0, NodeKind::Line),
            ],
        })];
        assert_eq!(tidy_up(&mut l, None), 3);
        let p = path(&l.shapes[0]);
        assert_eq!(coords(p), [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]);
        assert!(p.nodes.iter().all(|n| n.kind == NodeKind::Line));
    }

    #[test]
    fn tidy_up_keeps_curves_with_two_handles() {
        // A well-formed cubic corner (two off-curve handles before the curve node)
        // must survive tidy_up untouched — and count as no change.
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(0.0, 60.0, NodeKind::OffCurve),
                node(40.0, 100.0, NodeKind::OffCurve),
                node(100.0, 100.0, NodeKind::Curve),
                node(100.0, 0.0, NodeKind::Line),
            ],
        })];
        assert_eq!(tidy_up(&mut l, None), 0);
        let p = path(&l.shapes[0]);
        assert_eq!(p.nodes[3].kind, NodeKind::Curve);
        assert_eq!(p.nodes.len(), 5);
    }

    #[test]
    fn round_coordinates_rounds_nodes_and_anchors_but_not_locked_paths() {
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            mask: false,
            closed: true,
            nodes: vec![
                node(0.4, 0.6, NodeKind::Line),
                node(100.0, 0.0, NodeKind::Line),
                node(100.0, 100.0, NodeKind::Line),
            ],
        })];
        l.anchors = vec![crate::model::Anchor {
            name: "top".into(),
            x: 50.5,
            y: 99.4,
        }];
        assert!(round_coordinates(&mut l, None));
        assert_eq!(
            (path(&l.shapes[0]).nodes[0].x, path(&l.shapes[0]).nodes[0].y),
            (0.0, 1.0)
        );
        assert_eq!((l.anchors[0].x, l.anchors[0].y), (51.0, 99.0));
        assert!(!round_coordinates(&mut l, None), "nothing left to round");
        if let Shape::Path(p) = &mut l.shapes[0] {
            p.locked = true;
            p.nodes[0].x = 0.4;
        }
        assert!(!round_coordinates(&mut l, None));
        assert_eq!(path(&l.shapes[0]).nodes[0].x, 0.4);
    }
}
