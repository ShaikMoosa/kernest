//! Outline geometry: bounds, sidebearings and SVG path output.

use serde::Serialize;

use crate::model::{Font, Layer, Node, NodeKind, Path, Shape, IDENTITY};

/// Maximum component nesting depth; deeper (or cyclic) references are ignored.
const MAX_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Rect {
    fn point(x: f64, y: f64) -> Self {
        Rect {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }
    fn include(&mut self, x: f64, y: f64) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }
    fn union(a: Option<Rect>, b: Rect) -> Rect {
        match a {
            None => b,
            Some(mut a) => {
                a.include(b.min_x, b.min_y);
                a.include(b.max_x, b.max_y);
                a
            }
        }
    }
}

fn apply(t: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
}

fn compose(outer: &[f64; 6], inner: &[f64; 6]) -> [f64; 6] {
    [
        outer[0] * inner[0] + outer[2] * inner[1],
        outer[1] * inner[0] + outer[3] * inner[1],
        outer[0] * inner[2] + outer[2] * inner[3],
        outer[1] * inner[2] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
        outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

/// Calls `f` for every path in the layer, with components resolved and transformed.
/// A stroked path is given as the outlines it draws.
pub(crate) fn for_each_path(
    font: &Font,
    layer: &Layer,
    t: &[f64; 6],
    depth: usize,
    f: &mut dyn FnMut(&Path, &[f64; 6]),
) {
    if depth > MAX_DEPTH {
        return;
    }
    // Corner, cap and segment components first change the outline itself.
    if !layer.parts.is_empty() {
        let applied = crate::parts::apply(font, layer);
        return for_each_path(font, &applied, t, depth, f);
    }
    // Masks then cut the shapes below them.
    if crate::masking::has_masks(layer) {
        let applied = crate::masking::apply(font, layer);
        return for_each_path(font, &applied, t, depth, f);
    }
    for shape in &layer.shapes {
        match shape {
            Shape::Path(p) if p.stroke.is_some() => {
                for q in crate::stroke::expand_in(font, &layer.master_id, p).unwrap_or_default() {
                    f(&q, t);
                }
            }
            Shape::Path(p) => f(p, t),
            Shape::Component {
                base,
                transform,
                smart,
                ..
            } => {
                let Some(g) = font.glyph(base) else { continue };
                if g.is_smart() {
                    // A smart component: its shape for these axis values.
                    if let Some(inst) = crate::smart::instance_for(font, base, layer, smart) {
                        for_each_path(font, &inst, &compose(t, transform), depth + 1, f);
                    }
                } else if let Some(base_layer) = g.layer(&layer.master_id) {
                    for_each_path(font, base_layer, &compose(t, transform), depth + 1, f);
                }
            }
        }
    }
}

/// Splits a path into segments `(start, off-curve points, end)`, starting at an on-curve point.
pub(crate) fn segments(p: &Path) -> Vec<(Node, Vec<Node>, Node)> {
    let n = p.nodes.len();
    let Some(start) = p.nodes.iter().position(Node::on_curve) else {
        return Vec::new();
    };
    let order: Vec<Node> = (0..n).map(|i| p.nodes[(start + i) % n]).collect();
    let mut out = Vec::new();
    let mut prev = order[0];
    let mut offs = Vec::new();
    let tail = if p.closed { Some(order[0]) } else { None };
    for node in order.iter().skip(1).copied().chain(tail) {
        if node.on_curve() {
            out.push((prev, std::mem::take(&mut offs), node));
            prev = node;
        } else {
            offs.push(node);
        }
    }
    out
}

fn cubic_extrema(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    // Roots of the derivative of a cubic Bézier in one dimension.
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
    ts.into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect()
}

fn cubic_at(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let mt = 1.0 - t;
    mt * mt * mt * p0 + 3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t * p3
}

/// Exact bounds of the layer's outlines (curve extrema included), or `None` if empty.
pub fn bounds(font: &Font, layer: &Layer) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for_each_path(font, layer, &IDENTITY, 0, &mut |path, t| {
        let tp = |n: &Node| apply(t, n.x, n.y);
        for (a, offs, b) in segments(path) {
            let (ax, ay) = tp(&a);
            let (bx, by) = tp(&b);
            let mut r = Rect::point(ax, ay);
            r.include(bx, by);
            if offs.len() == 2 && b.kind != NodeKind::QCurve {
                let (c1x, c1y) = tp(&offs[0]);
                let (c2x, c2y) = tp(&offs[1]);
                for tt in cubic_extrema(ax, c1x, c2x, bx) {
                    r.include(
                        cubic_at(ax, c1x, c2x, bx, tt),
                        cubic_at(ay, c1y, c2y, by, tt),
                    );
                }
                for tt in cubic_extrema(ay, c1y, c2y, by) {
                    r.include(
                        cubic_at(ax, c1x, c2x, bx, tt),
                        cubic_at(ay, c1y, c2y, by, tt),
                    );
                }
            } else {
                // Quadratic or degenerate: control points bound the curve conservatively.
                for o in &offs {
                    let (ox, oy) = tp(o);
                    r.include(ox, oy);
                }
            }
            acc = Some(Rect::union(acc, r));
        }
    });
    acc
}

/// Left and right sidebearings, or `None` for an empty layer.
pub fn sidebearings(font: &Font, layer: &Layer) -> Option<(f64, f64)> {
    bounds(font, layer).map(|b| (b.min_x, layer.width - b.max_x))
}

fn num(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// SVG path data in font units (y up). Viewers flip the y axis.
pub fn svg_path(font: &Font, layer: &Layer) -> String {
    let mut d = String::new();
    for_each_path(font, layer, &IDENTITY, 0, &mut |path, t| {
        let segs = segments(path);
        let Some((first, _, _)) = segs.first() else {
            return;
        };
        let p = |n: &Node| {
            let (x, y) = apply(t, n.x, n.y);
            format!("{} {}", num(x), num(y))
        };
        d.push_str(&format!("M{}", p(first)));
        for (_, offs, end) in &segs {
            match offs.len() {
                0 => d.push_str(&format!("L{}", p(end))),
                1 => d.push_str(&format!("Q{} {}", p(&offs[0]), p(end))),
                2 if end.kind != NodeKind::QCurve => {
                    d.push_str(&format!("C{} {} {}", p(&offs[0]), p(&offs[1]), p(end)))
                }
                // TrueType-style runs of off-curves: implied on-curve midpoints.
                _ => {
                    for w in offs.windows(2) {
                        let (ax, ay) = apply(t, w[0].x, w[0].y);
                        let (bx, by) = apply(t, w[1].x, w[1].y);
                        d.push_str(&format!(
                            "Q{} {} {} {}",
                            num(ax),
                            num(ay),
                            num((ax + bx) / 2.0),
                            num((ay + by) / 2.0)
                        ));
                    }
                    d.push_str(&format!("Q{} {}", p(offs.last().unwrap()), p(end)));
                }
            }
        }
        if path.closed {
            d.push('Z');
        }
    });
    d
}
