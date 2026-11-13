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

/// Makes outer contours counter-clockwise and counters clockwise, judging nesting by
/// how many other closed contours surround each one. Open paths are left alone.
/// Returns how many paths were reversed.
pub fn correct_direction(layer: &mut Layer) -> usize {
    let polys: Vec<Option<Vec<(f64, f64)>>> = layer
        .shapes
        .iter()
        .map(|s| match s {
            Shape::Path(p) if p.closed && p.nodes.len() >= 3 => Some(polygon(p)),
            _ => None,
        })
        .collect();
    let mut flips = Vec::new();
    for (i, poly) in polys.iter().enumerate() {
        let Some(poly) = poly else { continue };
        let a = area(poly);
        if a.abs() < 1e-9 {
            continue;
        }
        // A point inside this contour, near its first vertex, decides the nesting.
        let probe = poly[0];
        let depth = polys
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && o.as_ref().is_some_and(|o| contains(o, probe)))
            .count();
        let want_ccw = depth % 2 == 0;
        if (a > 0.0) != want_ccw {
            flips.push(i);
        }
    }
    for &i in &flips {
        if let Shape::Path(p) = &mut layer.shapes[i] {
            *p = reverse(p);
        }
    }
    flips.len()
}

fn cubic(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64), t: f64) -> (f64, f64) {
    let mt = 1.0 - t;
    let f = |p: f64, q: f64, r: f64, s: f64| {
        mt * mt * mt * p + 3.0 * mt * mt * t * q + 3.0 * mt * t * t * r + t * t * t * s
    };
    (f(a.0, b.0, c.0, d.0), f(a.1, b.1, c.1, d.1))
}

/// Parameters in (0, 1) where one coordinate of a cubic has a zero derivative.
fn extreme_ts(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    // B'(t)/3 = a t² + b t + c
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let mut ts = Vec::new();
    if a.abs() < 1e-12 {
        if b.abs() > 1e-12 {
            ts.push(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let r = disc.sqrt();
            ts.push((-b + r) / (2.0 * a));
            ts.push((-b - r) / (2.0 * a));
        }
    }
    ts.retain(|t| *t > 0.01 && *t < 0.99);
    ts
}

type Pt = (f64, f64);

/// Splits a cubic at `t`: returns (left handles, mid point, right handles).
fn split(a: Pt, b: Pt, c: Pt, d: Pt, t: f64) -> ([Pt; 2], Pt, [Pt; 2]) {
    let l = |p: Pt, q: Pt| (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
    let (ab, bc, cd) = (l(a, b), l(b, c), l(c, d));
    let (abc, bcd) = (l(ab, bc), l(bc, cd));
    let m = l(abc, bcd);
    ([ab, abc], m, [bcd, cd])
}

/// Adds on-curve points at the horizontal and vertical extremes of cubic segments.
/// Only the paths whose indices (among the layer's paths) are in `only` are changed,
/// or every path if `only` is `None`. Returns how many points were added.
pub fn add_extremes(layer: &mut Layer, only: Option<&[usize]>) -> usize {
    let mut added = 0;
    let mut path_index = 0;
    for s in &mut layer.shapes {
        let Shape::Path(p) = s else { continue };
        let this = path_index;
        path_index += 1;
        if only.is_some_and(|o| !o.contains(&this)) {
            continue;
        }
        let (np, k) = path_with_extremes(p);
        *p = np;
        added += k;
    }
    added
}

fn path_with_extremes(p: &Path) -> (Path, usize) {
    let n = p.nodes.len();
    let ons: Vec<usize> = (0..n).filter(|&i| p.nodes[i].on_curve()).collect();
    if ons.is_empty() || is_quadratic(p) {
        return (p.clone(), 0);
    }
    let mut out: Vec<Node> = Vec::with_capacity(n);
    let mut added = 0;
    let mut start_pos = None;
    for (k, &end) in ons.iter().enumerate() {
        let prev = if k == 0 {
            if !p.closed {
                // The first node of an open path starts the outline; nothing ends there.
                out.push(p.nodes[end]);
                start_pos = Some(0);
                continue;
            }
            ons[ons.len() - 1]
        } else {
            ons[k - 1]
        };
        let offs: Vec<Node> = (1..)
            .map(|d| (prev + d) % n)
            .take_while(|&i| i != end)
            .map(|i| p.nodes[i])
            .collect();
        let e = p.nodes[end];
        if e.kind == NodeKind::Curve && offs.len() == 2 {
            let s = p.nodes[prev];
            let (a, b, c, d) = (
                (s.x, s.y),
                (offs[0].x, offs[0].y),
                (offs[1].x, offs[1].y),
                (e.x, e.y),
            );
            let mut ts = extreme_ts(a.0, b.0, c.0, d.0);
            ts.extend(extreme_ts(a.1, b.1, c.1, d.1));
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 0.01);
            let (mut a, mut b, mut c) = (a, b, c);
            let mut done = 0.0;
            for t in ts {
                let local = (t - done) / (1.0 - done);
                let (left, mid, right) = split(a, b, c, d, local);
                out.push(Node::new(left[0].0, left[0].1, NodeKind::OffCurve));
                out.push(Node::new(left[1].0, left[1].1, NodeKind::OffCurve));
                out.push(Node {
                    x: mid.0,
                    y: mid.1,
                    kind: NodeKind::Curve,
                    smooth: true,
                });
                added += 1;
                (a, b, c) = (mid, right[0], right[1]);
                done = t;
            }
            out.push(Node::new(b.0, b.1, NodeKind::OffCurve));
            out.push(Node::new(c.0, c.1, NodeKind::OffCurve));
        } else {
            out.extend(offs);
        }
        if end == 0 {
            start_pos = Some(out.len());
        }
        out.push(e);
    }
    // Keep the original first node first (closed paths are cyclic).
    if p.closed && p.nodes[0].on_curve() {
        if let Some(i) = start_pos {
            out.rotate_left(i);
        }
    }
    (
        Path {
            closed: p.closed,
            nodes: out,
        },
        added,
    )
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

    fn signed(s: &Shape) -> f64 {
        let Shape::Path(p) = s else { panic!() };
        area(&polygon(p))
    }

    #[test]
    fn corrects_outer_and_counter() {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![
            rect(0.0, 0.0, 500.0, 700.0, false),
            rect(100.0, 100.0, 400.0, 600.0, true),
        ];
        assert_eq!(correct_direction(&mut l), 2);
        assert!(signed(&l.shapes[0]) > 0.0, "outer is counter-clockwise");
        assert!(signed(&l.shapes[1]) < 0.0, "counter is clockwise");
        assert_eq!(correct_direction(&mut l), 0, "already correct");
        let Shape::Path(p) = &l.shapes[0] else {
            panic!()
        };
        // The clockwise rectangle started at (0, 700); it still does.
        assert_eq!(
            (p.nodes[0].x, p.nodes[0].y),
            (0.0, 700.0),
            "start node kept"
        );
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

    #[test]
    fn adds_extremes_to_a_bulge() {
        // A cubic from (0,0) to (200,0) bulging up to y=150 at t=0.5.
        let mut l = Layer::new("m01", 300.0);
        l.shapes = vec![Shape::Path(Path {
            closed: true,
            nodes: vec![
                node(0.0, 0.0, NodeKind::Line),
                node(0.0, 200.0, NodeKind::OffCurve),
                node(200.0, 200.0, NodeKind::OffCurve),
                node(200.0, 0.0, NodeKind::Curve),
            ],
        })];
        assert_eq!(add_extremes(&mut l, None), 1);
        let Shape::Path(p) = &l.shapes[0] else {
            panic!()
        };
        assert_eq!(p.nodes.len(), 7);
        let top = p.nodes[3];
        assert_eq!(
            (top.x, top.y, top.kind, top.smooth),
            (100.0, 150.0, NodeKind::Curve, true)
        );
        // The new handles are horizontal at the extreme.
        assert_eq!(p.nodes[2].y, 150.0);
        assert_eq!(p.nodes[4].y, 150.0);
        assert_eq!((p.nodes[0].x, p.nodes[0].y), (0.0, 0.0), "start node kept");
        assert_eq!(add_extremes(&mut l, None), 0, "nothing left to add");
    }
}
