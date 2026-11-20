//! Strokes: paths drawn with a pen instead of filled, as on screen S28.
//!
//! A stroked path keeps its centre line editable; [`expand`] turns it into filled
//! outlines (on export, or with Expand Outline). The outline's structure depends
//! only on the path and the stroke settings, never on the width: each segment gives
//! one curve (or line) on each side, joins and caps have fixed shapes, and nothing
//! is merged. Masters whose paths are compatible therefore expand to compatible
//! outlines, so strokes work in variable fonts. Overlaps at inner corners are left
//! in place; export removes them from static fonts, and variable fonts allow them.

use std::collections::BTreeMap;
use std::ops::Range;

use kurbo::{
    BezPath, CubicBez, CurveFitSample, ParamCurve, ParamCurveDeriv, ParamCurveFit, PathSeg, Point,
    Shape as _, Vec2,
};
use serde::{Deserialize, Serialize};

use crate::model::{Layer, Path, Shape};
use crate::overlap::{from_bez, to_bez};

/// How a stroked path is drawn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathStroke {
    /// Pen width in font units.
    pub width: f64,
    /// Where the path sits in the stroke: -100 on its left edge, 0 in the middle,
    /// 100 on its right edge (left and right as the path runs).
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub join: StrokeJoin,
    #[serde(default)]
    pub start_cap: StrokeCap,
    #[serde(default)]
    pub end_cap: StrokeCap,
    /// A closed path is also filled inside.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fill: bool,
    /// Pen points: a width for some on-curve nodes (counted from the path's first
    /// on-curve node). The width changes evenly from one node to the next.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub widths: BTreeMap<usize, f64>,
    /// A brush glyph (by convention named `_brush.something`) drawn along the path
    /// instead of a pen: its outline is stretched from its `start` anchor to its
    /// `end` anchor and bent to follow the path (S59).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brush: Option<String>,
}

impl Default for PathStroke {
    fn default() -> Self {
        Self {
            width: 40.0,
            position: 0.0,
            join: StrokeJoin::Round,
            start_cap: StrokeCap::Butt,
            end_cap: StrokeCap::Butt,
            fill: false,
            widths: BTreeMap::new(),
            brush: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrokeJoin {
    Miter,
    #[default]
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrokeCap {
    /// Cut square at the end point.
    #[default]
    Butt,
    Round,
    /// Cut square half the width beyond the end point.
    Square,
}

impl PathStroke {
    /// Checks the values a user can type.
    pub fn check(&self) -> Result<(), String> {
        let ok = |w: f64| w.is_finite() && w > 0.0 && w <= 10_000.0;
        if !ok(self.width) || self.widths.values().any(|w| !ok(*w)) {
            return Err("A stroke width must be a number above 0.".into());
        }
        if !self.position.is_finite() || self.position.abs() > 100.0 {
            return Err("The stroke position must be between -100 and 100.".into());
        }
        if self.brush.as_deref().is_some_and(|b| b.trim().is_empty()) {
            return Err("Choose a brush glyph.".into());
        }
        Ok(())
    }

    fn width_at(&self, on_curve: usize) -> f64 {
        self.widths.get(&on_curve).copied().unwrap_or(self.width)
    }
}

/// A miter longer than this many stroke widths is cut short (at the limit).
const MITER_LIMIT: f64 = 2.0;

/// One piece of an offset side.
#[derive(Debug, Clone, Copy)]
enum Piece {
    Line(Point, Point),
    Cubic(CubicBez),
}

impl Piece {
    fn start(&self) -> Point {
        match self {
            Piece::Line(a, _) => *a,
            Piece::Cubic(c) => c.p0,
        }
    }
    fn end(&self) -> Point {
        match self {
            Piece::Line(_, b) => *b,
            Piece::Cubic(c) => c.p3,
        }
    }
    fn reversed(&self) -> Piece {
        match self {
            Piece::Line(a, b) => Piece::Line(*b, *a),
            Piece::Cubic(c) => Piece::Cubic(CubicBez::new(c.p3, c.p2, c.p1, c.p0)),
        }
    }
    /// Appends the piece; its start is taken to be the current point.
    fn push_to(&self, b: &mut BezPath) {
        match self {
            Piece::Line(_, e) => b.line_to(*e),
            Piece::Cubic(c) => b.curve_to(c.p1, c.p2, c.p3),
        }
    }
}

/// A segment of the centre line with its pen widths at both ends and whether its end
/// node is a smooth one (no join needed there).
struct Seg {
    c: CubicBez,
    line: bool,
    w0: f64,
    w1: f64,
    smooth_end: bool,
}

/// The filled outlines a stroked path draws, with brushes looked up in `font` for
/// the master `master_id`. `None` when the path has no stroke or nothing to draw.
pub fn expand_in(font: &crate::model::Font, master_id: &str, p: &Path) -> Option<Vec<Path>> {
    if let Some(brush) = p.stroke.as_ref().and_then(|s| s.brush.as_deref()) {
        if let Some(out) = brush_along(font, master_id, brush, p) {
            return Some(out);
        }
    }
    expand(p)
}

/// The filled outlines a pen-stroked path draws (brushes need [`expand_in`]).
/// `None` when the path has no stroke or nothing to draw.
pub fn expand(p: &Path) -> Option<Vec<Path>> {
    let s = p.stroke.as_ref()?;
    let segs = centre_segments(p, s)?;
    let left = (1.0 + s.position / 100.0) / 2.0;
    let right = 1.0 - left;
    // Right side forward and left side backward give a counter-clockwise outline.
    let rside = side(&segs, -right, s.join, p.closed);
    let lside = side(&segs, left, s.join, p.closed);
    let mut contours: Vec<BezPath> = Vec::new();
    if p.closed {
        contours.push(close(&rside));
        let rev: Vec<Piece> = lside.iter().rev().map(Piece::reversed).collect();
        contours.push(close(&rev));
        // The larger loop is the outside: counter-clockwise; the other a counter.
        contours.sort_by(|a, b| b.area().abs().total_cmp(&a.area().abs()));
        for (i, c) in contours.iter_mut().enumerate() {
            let want_ccw = i == 0;
            if (c.area() > 0.0) != want_ccw {
                *c = c.reverse_subpaths();
            }
        }
        if s.fill {
            contours.truncate(1);
        }
    } else {
        let first = &segs[0];
        let last = &segs[segs.len() - 1];
        let mut all: Vec<Piece> = rside.clone();
        all.extend(cap(
            s.end_cap,
            rside.last()?.end(),
            lside.last()?.end(),
            tangent(&last.c, 1.0),
        ));
        all.extend(lside.iter().rev().map(Piece::reversed));
        all.extend(cap(
            s.start_cap,
            lside[0].start(),
            rside[0].start(),
            -tangent(&first.c, 0.0),
        ));
        let mut c = close(&all);
        if c.area() < 0.0 {
            c = c.reverse_subpaths();
        }
        contours.push(c);
    }
    let paths: Vec<Path> = contours
        .iter()
        .filter_map(|b| from_bez(b, true))
        .filter(|p| p.nodes.len() > 2)
        .collect();
    (!paths.is_empty()).then_some(paths)
}

/// The layer with every stroked path replaced by its outlines.
pub fn expand_layer(font: &crate::model::Font, layer: &Layer) -> Layer {
    if !has_strokes(layer) {
        return layer.clone();
    }
    let mut out = layer.clone();
    out.shapes = layer
        .shapes
        .iter()
        .flat_map(|s| match s {
            Shape::Path(p) if p.stroke.is_some() => expand_in(font, &layer.master_id, p)
                .unwrap_or_default()
                .into_iter()
                .map(Shape::Path)
                .collect(),
            other => vec![other.clone()],
        })
        .collect();
    out
}

/// Whether any path of the layer is stroked.
pub fn has_strokes(layer: &Layer) -> bool {
    layer
        .shapes
        .iter()
        .any(|s| matches!(s, Shape::Path(p) if p.stroke.is_some()))
}

/// The font with every stroke expanded, as export builds it.
/// Also applies corner, cap and segment components, which change the outline the
/// strokes follow.
pub fn expand_font(font: &crate::model::Font) -> crate::model::Font {
    let mut f = font.clone();
    for g in &mut f.glyphs {
        for l in &mut g.layers {
            if !l.parts.is_empty() {
                *l = crate::parts::apply(font, l);
            }
            if has_strokes(l) {
                *l = expand_layer(font, l);
            }
        }
    }
    f
}

/// The centre line as segments with their widths. Kernest segments (on-curve to
/// on-curve) that hold a quadratic chain become several segments; the width changes
/// along the whole Kernest segment.
fn centre_segments(p: &Path, s: &PathStroke) -> Option<Vec<Seg>> {
    let kseg = crate::geometry::segments(p);
    let bez = to_bez(p)?;
    let pieces: Vec<PathSeg> = bez.segments().collect();
    let on_count = if p.closed { kseg.len() } else { kseg.len() + 1 };
    let mut out = Vec::new();
    let mut k = 0;
    for (span, (_, offs, end)) in kseg.iter().enumerate() {
        let count =
            if offs.len() == 2 && end.kind == crate::model::NodeKind::Curve || offs.is_empty() {
                1
            } else {
                offs.len()
            };
        let w0 = s.width_at(span);
        let w1 = s.width_at((span + 1) % on_count.max(1));
        for i in 0..count {
            let seg = *pieces.get(k)?;
            k += 1;
            let (t0, t1) = (i as f64 / count as f64, (i + 1) as f64 / count as f64);
            let (c, line) = match seg {
                PathSeg::Line(l) => (
                    CubicBez::new(
                        l.p0,
                        l.p0.lerp(l.p1, 1.0 / 3.0),
                        l.p0.lerp(l.p1, 2.0 / 3.0),
                        l.p1,
                    ),
                    true,
                ),
                PathSeg::Quad(q) => (q.raise(), false),
                PathSeg::Cubic(c) => (c, false),
            };
            if (c.p3 - c.p0).hypot() < 1e-9 && line {
                continue;
            }
            out.push(Seg {
                c,
                line,
                w0: w0 + (w1 - w0) * t0,
                w1: w0 + (w1 - w0) * t1,
                // Points inside a quadratic chain are smooth by construction.
                smooth_end: i + 1 < count || end.smooth,
            });
        }
    }
    (!out.is_empty()).then_some(out)
}

/// One side of the stroke, offset by `frac` of the width (positive to the left),
/// with joins at corners.
fn side(segs: &[Seg], frac: f64, join: StrokeJoin, closed: bool) -> Vec<Piece> {
    let offs: Vec<Piece> = segs.iter().map(|g| offset_piece(g, frac)).collect();
    let mut out: Vec<Piece> = Vec::new();
    let n = offs.len();
    for i in 0..n {
        let piece = match out.last() {
            // Start where the previous piece (or join) ended.
            Some(prev) => snap_start(offs[i], prev.end()),
            None => offs[i],
        };
        out.push(piece);
        let next = if i + 1 < n {
            Some(i + 1)
        } else if closed {
            Some(0)
        } else {
            None
        };
        let Some(j) = next else { break };
        let g = &segs[i];
        if g.smooth_end {
            continue;
        }
        let a = piece.end();
        let b = offs[j].start();
        let centre = g.c.p3;
        let t_in = tangent(&g.c, 1.0);
        let t_out = tangent(&segs[j].c, 0.0);
        // Turning left makes the left side the inner one.
        let turn = t_in.cross(t_out);
        let inner = (turn > 0.0) == (frac > 0.0);
        if inner || frac.abs() < 1e-12 {
            // Straight edges meeting at an inner corner are cut where they cross,
            // so the outline doesn't fold back on itself there.
            if let (Piece::Line(p0, _), Piece::Line(_, q1)) = (piece, offs[j]) {
                if let Some(tip) = segment_cross(p0, a, b, q1) {
                    *out.last_mut().expect("just pushed") = Piece::Line(p0, tip);
                    if j == 0 {
                        out[0] = snap_start(out[0], tip);
                    }
                    continue;
                }
            }
            out.push(Piece::Line(a, b));
            continue;
        }
        match join {
            StrokeJoin::Bevel => out.push(Piece::Line(a, b)),
            StrokeJoin::Round => out.push(Piece::Cubic(arc(centre, a, b))),
            StrokeJoin::Miter => {
                let half = g.w1 * frac.abs();
                let tip = intersect(a, t_in, b, -t_out)
                    .filter(|tip| (*tip - centre).hypot() <= MITER_LIMIT * half.max(1e-9) * 2.0)
                    .unwrap_or_else(|| {
                        // Too sharp: cut the miter at the limit, keeping two lines.
                        let mid = (t_in - t_out).normalize();
                        centre + mid * (MITER_LIMIT * half)
                    });
                out.push(Piece::Line(a, tip));
                out.push(Piece::Line(tip, b));
            }
        }
    }
    // A closed side ends where it began.
    if closed {
        if let (Some(first), Some(last)) = (out.first().copied(), out.last().copied()) {
            if (last.end() - first.start()).hypot() > 1e-9 {
                out.push(Piece::Line(last.end(), first.start()));
            }
        }
    }
    out
}

/// A piece with its start moved to `p` (the joins and smooth nodes meet exactly).
fn snap_start(piece: Piece, p: Point) -> Piece {
    match piece {
        Piece::Line(_, b) => Piece::Line(p, b),
        Piece::Cubic(c) => Piece::Cubic(CubicBez::new(p, c.p1 + (p - c.p0), c.p2, c.p3)),
    }
}

/// A segment offset by `frac` of the pen width: a line stays a line (also when the
/// width changes), a curve becomes the one cubic that fits the offset best.
fn offset_piece(g: &Seg, frac: f64) -> Piece {
    let (n0, n1) = (normal(&g.c, 0.0), normal(&g.c, 1.0));
    let (d0, d1) = (g.w0 * frac, g.w1 * frac);
    let a = g.c.p0 + n0 * d0;
    let b = g.c.p3 + n1 * d1;
    if g.line {
        return Piece::Line(a, b);
    }
    if frac.abs() < 1e-12 {
        return Piece::Cubic(g.c);
    }
    let src = VarOffset { c: g.c, d0, d1 };
    match kurbo::fit_to_cubic(&src, 0.0..1.0, 1e12) {
        Some((c, _)) => Piece::Cubic(CubicBez::new(a, c.p1, c.p2, b)),
        // Fallback: move the handles with their nodes.
        None => Piece::Cubic(CubicBez::new(a, g.c.p1 + n0 * d0, g.c.p2 + n1 * d1, b)),
    }
}

/// The unit direction of travel at `t`, using the chord when a handle sits on its node.
fn tangent(c: &CubicBez, t: f64) -> Vec2 {
    let mut d = c.deriv().eval(t).to_vec2();
    if d.hypot() < 1e-9 {
        d = if t < 0.5 { c.p2 - c.p0 } else { c.p3 - c.p1 };
    }
    if d.hypot() < 1e-9 {
        d = c.p3 - c.p0;
    }
    d / d.hypot().max(1e-12)
}

/// The unit normal to the left of travel at `t`.
fn normal(c: &CubicBez, t: f64) -> Vec2 {
    let d = tangent(c, t);
    Vec2::new(-d.y, d.x)
}

/// A curve offset to the left of `c` by a distance that changes evenly from `d0`
/// to `d1` (negative is to the right), for kurbo's curve fitting.
struct VarOffset {
    c: CubicBez,
    d0: f64,
    d1: f64,
}

impl ParamCurveFit for VarOffset {
    fn sample_pt_tangent(&self, t: f64, _sign: f64) -> CurveFitSample {
        let (p, tangent) = self.sample_pt_deriv(t);
        CurveFitSample { p, tangent }
    }

    fn sample_pt_deriv(&self, t: f64) -> (Point, Vec2) {
        let d1 = self.c.deriv();
        let mut tv = d1.eval(t).to_vec2();
        let ta = d1.deriv().eval(t).to_vec2();
        if tv.hypot() < 1e-9 {
            // A handle on its node: the curve still leaves along its second derivative.
            tv = ta * if t < 0.5 { 1e-6 } else { -1e-6 };
        }
        let len = tv.hypot().max(1e-12);
        let perp = |v: Vec2| Vec2::new(-v.y, v.x);
        let n = perp(tv) / len;
        let dn = perp(ta) / len - perp(tv) * (tv.dot(ta) / (len * len * len));
        let d = self.d0 + (self.d1 - self.d0) * t;
        let dd = self.d1 - self.d0;
        (self.c.eval(t) + n * d, tv + dn * d + n * dd)
    }

    fn break_cusp(&self, _range: Range<f64>) -> Option<f64> {
        None
    }
}

/// A circular-ish arc from `a` to `b` around `c`, as one cubic.
fn arc(c: Point, a: Point, b: Point) -> CubicBez {
    let (va, vb) = (a - c, b - c);
    let angle = va.cross(vb).atan2(va.dot(vb));
    let k = 4.0 / 3.0 * (angle / 4.0).tan();
    let perp = |v: Vec2| Vec2::new(-v.y, v.x);
    CubicBez::new(a, a + perp(va) * k, b - perp(vb) * k, b)
}

/// Where segment `a0`–`a1` crosses segment `b0`–`b1`, strictly inside both.
fn segment_cross(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<Point> {
    let (da, db) = (a1 - a0, b1 - b0);
    let cross = da.cross(db);
    if cross.abs() < 1e-9 {
        return None;
    }
    let t = (b0 - a0).cross(db) / cross;
    let u = (b0 - a0).cross(da) / cross;
    (t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9).then(|| a0 + da * t)
}

/// Where the line through `a` along `da` meets the line through `b` along `db`.
fn intersect(a: Point, da: Vec2, b: Point, db: Vec2) -> Option<Point> {
    let cross = da.cross(db);
    if cross.abs() < 1e-9 {
        return None;
    }
    let t = (b - a).cross(db) / cross;
    Some(a + da * t)
}

/// The cap from `from` to `to` across an end of the stroke, bulging along `out`.
fn cap(kind: StrokeCap, from: Point, to: Point, out: Vec2) -> Vec<Piece> {
    match kind {
        StrokeCap::Butt => vec![Piece::Line(from, to)],
        StrokeCap::Square => {
            let ext = out * ((to - from).hypot() / 2.0);
            vec![
                Piece::Line(from, from + ext),
                Piece::Line(from + ext, to + ext),
                Piece::Line(to + ext, to),
            ]
        }
        StrokeCap::Round => {
            let mid = from.midpoint(to);
            let tip = mid + out * ((to - from).hypot() / 2.0);
            vec![
                Piece::Cubic(arc(mid, from, tip)),
                Piece::Cubic(arc(mid, tip, to)),
            ]
        }
    }
}

/// A closed kurbo path through the pieces.
fn close(pieces: &[Piece]) -> BezPath {
    let mut b = BezPath::new();
    let Some(first) = pieces.first() else {
        return b;
    };
    b.move_to(first.start());
    for p in pieces {
        p.push_to(&mut b);
    }
    b.close_path();
    b
}

type R<T> = Result<T, String>;

/// Brush curves are split this many times per segment before bending, so a long
/// straight edge follows a curved path. Fixed, so masters stay compatible.
const BRUSH_STEPS: usize = 8;

/// The brush glyph's outline bent along `p`: brush x from the `start` anchor to the
/// `end` anchor maps to distance along the path, brush y (from the anchors' line)
/// to distance to its left. Without anchors, the brush's left and right edges at
/// y = 0 stand in. `None` if the brush glyph has no outline.
fn brush_along(
    font: &crate::model::Font,
    master_id: &str,
    brush: &str,
    p: &Path,
) -> Option<Vec<Path>> {
    let g = font.glyph(brush)?;
    let layer = g
        .layer(master_id)
        .or_else(|| g.layer(&crate::interpolate::default_master(font).id))?;
    // Only the brush's own paths (expanded, components resolved) are bent.
    let mut shapes: Vec<Path> = Vec::new();
    crate::geometry::for_each_path(font, layer, &crate::IDENTITY, 0, &mut |q, t| {
        let mut q = q.clone();
        q.stroke = None;
        for n in &mut q.nodes {
            let (x, y) = (n.x, n.y);
            n.x = t[0] * x + t[2] * y + t[4];
            n.y = t[1] * x + t[3] * y + t[5];
        }
        shapes.push(q);
    });
    if shapes.is_empty() {
        return None;
    }
    let anchor = |name: &str| {
        layer
            .anchors
            .iter()
            .find(|a| a.name == name)
            .map(|a| (a.x, a.y))
    };
    let (start, end) = match (anchor("start"), anchor("end")) {
        (Some(a), Some(b)) if (b.0 - a.0).abs() > 1e-9 => (a, b),
        _ => {
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for q in &shapes {
                for n in &q.nodes {
                    lo = lo.min(n.x);
                    hi = hi.max(n.x);
                }
            }
            ((lo, 0.0), (hi, 0.0))
        }
    };
    let mut centre = to_bez(p)?;
    if p.closed {
        centre.close_path();
    }
    let track = Track::new(&centre)?;
    let span = end.0 - start.0;
    let warp = |x: f64, y: f64| -> Point {
        let u = (x - start.0) / span;
        let base_y = start.1 + (end.1 - start.1) * u;
        track.at(u * track.length, y - base_y)
    };
    let mut out = Vec::new();
    for q in &shapes {
        let Some(bez) = to_bez(q) else { continue };
        let mut b = BezPath::new();
        let mut first = true;
        for seg in bez.segments() {
            let c = match seg {
                PathSeg::Line(l) => CubicBez::new(
                    l.p0,
                    l.p0.lerp(l.p1, 1.0 / 3.0),
                    l.p0.lerp(l.p1, 2.0 / 3.0),
                    l.p1,
                ),
                PathSeg::Quad(qd) => qd.raise(),
                PathSeg::Cubic(c) => c,
            };
            for k in 0..BRUSH_STEPS {
                let part = c
                    .subsegment(k as f64 / BRUSH_STEPS as f64..(k + 1) as f64 / BRUSH_STEPS as f64);
                let [a, h1, h2, z] =
                    [part.p0, part.p1, part.p2, part.p3].map(|pt| warp(pt.x, pt.y));
                if first {
                    b.move_to(a);
                    first = false;
                }
                b.curve_to(h1, h2, z);
            }
        }
        b.close_path();
        if let Some(path) = from_bez(&b, true) {
            out.push(path);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A path measured by length, for placing points along it.
struct Track {
    /// Points along the path with the distance to each.
    points: Vec<(f64, Point)>,
    length: f64,
}

impl Track {
    fn new(b: &BezPath) -> Option<Self> {
        let mut points: Vec<(f64, Point)> = Vec::new();
        let mut dist = 0.0;
        let mut last: Option<Point> = None;
        kurbo::flatten(b.elements().iter().copied(), 0.05, |el| {
            let p = match el {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => p,
                _ => return,
            };
            if let Some(q) = last {
                let d = (p - q).hypot();
                if d < 1e-9 {
                    return;
                }
                dist += d;
            }
            points.push((dist, p));
            last = Some(p);
        });
        (points.len() >= 2 && dist > 1e-9).then_some(Self {
            points,
            length: dist,
        })
    }

    /// The point `along` units along the path, moved `left` units to its left.
    /// Beyond either end the path continues straight.
    fn at(&self, along: f64, left: f64) -> Point {
        let i = self
            .points
            .partition_point(|(d, _)| *d < along)
            .clamp(1, self.points.len() - 1);
        let (d0, p0) = self.points[i - 1];
        let (d1, p1) = self.points[i];
        let dir = (p1 - p0) / (d1 - d0).max(1e-12);
        let base = p0 + dir * (along - d0);
        base + Vec2::new(-dir.y, dir.x) * left
    }
}

/// The `index`-th path of a layer (counting paths only).
fn nth_path(layer: &mut Layer, index: usize) -> Option<&mut Path> {
    layer
        .shapes
        .iter_mut()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .nth(index)
}

/// The layers that interpolate together with `key`: masters, intermediate and
/// alternate layers (colour layers and backups are drawn separately).
fn partner_keys(g: &crate::model::Glyph) -> Vec<String> {
    g.layers
        .iter()
        .filter(|l| l.color.is_none() && l.backup.is_none())
        .map(|l| l.key().to_string())
        .collect()
}

/// Strokes (or, with `None`, fills again) the `path`-th path of a layer. The same
/// path in the glyph's other masters and intermediate layers gets the same joins,
/// caps, fill and pen points, keeping its own widths if it is already stroked, so
/// the masters stay compatible.
pub fn set_stroke(
    font: &crate::model::Font,
    glyph: &str,
    key: &str,
    path: usize,
    stroke: Option<PathStroke>,
) -> R<crate::model::Font> {
    if let Some(s) = &stroke {
        s.check()?;
    }
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let target_len = {
        let l = g.layer_mut(key).ok_or("That layer doesn't exist.")?;
        let p = nth_path(l, path).ok_or("No path at that position.")?;
        p.stroke = stroke.clone();
        p.nodes.len()
    };
    for k in partner_keys(g) {
        if k == key {
            continue;
        }
        let Some(l) = g.layer_mut(&k) else { continue };
        let Some(p) = nth_path(l, path).filter(|p| p.nodes.len() == target_len) else {
            continue;
        };
        p.stroke = match (&stroke, &p.stroke) {
            (None, _) => None,
            (Some(new), Some(own)) => Some(PathStroke {
                width: own.width,
                widths: new
                    .widths
                    .keys()
                    .map(|n| (*n, own.widths.get(n).copied().unwrap_or(own.width)))
                    .collect(),
                ..new.clone()
            }),
            (Some(new), None) => Some(new.clone()),
        };
    }
    Ok(f)
}

/// Sets (or, with `None`, removes) the pen width at on-curve node `node` of a
/// stroked path. Adding or removing a pen point does so in the partner layers too
/// (there at their own stroke width), so they stay compatible.
pub fn set_pen_width(
    font: &crate::model::Font,
    glyph: &str,
    key: &str,
    path: usize,
    node: usize,
    width: Option<f64>,
) -> R<crate::model::Font> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    {
        let l = g.layer_mut(key).ok_or("That layer doesn't exist.")?;
        let p = nth_path(l, path).ok_or("No path at that position.")?;
        let on = p.nodes.iter().filter(|n| n.on_curve()).count();
        if node >= on {
            return Err("No node at that position.".into());
        }
        let s = p.stroke.as_mut().ok_or("That path isn't stroked.")?;
        match width {
            Some(w) => {
                s.widths.insert(node, w);
            }
            None => {
                s.widths.remove(&node);
            }
        }
        s.check()?;
    }
    for k in partner_keys(g) {
        if k == key {
            continue;
        }
        let Some(l) = g.layer_mut(&k) else { continue };
        let Some(s) = nth_path(l, path).and_then(|p| p.stroke.as_mut()) else {
            continue;
        };
        match width {
            Some(_) => {
                let w = s.width;
                s.widths.entry(node).or_insert(w);
            }
            None => {
                s.widths.remove(&node);
            }
        }
    }
    Ok(f)
}

/// Expand Outline: every stroked path of the glyph becomes the outlines it draws, in
/// all its layers at once (so masters stay compatible).
pub fn expand_glyph(font: &crate::model::Font, glyph: &str) -> R<crate::model::Font> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    if !g.layers.iter().any(has_strokes) {
        return Err("This glyph has no stroked paths.".into());
    }
    for l in &mut g.layers {
        *l = expand_layer(font, l);
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, NodeKind};

    fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
        Path {
            closed: false,
            nodes: vec![
                Node::new(x0, y0, NodeKind::Line),
                Node::new(x1, y1, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
        }
    }

    fn bbox(paths: &[Path]) -> (f64, f64, f64, f64) {
        let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in paths {
            for n in &p.nodes {
                b = (b.0.min(n.x), b.1.min(n.y), b.2.max(n.x), b.3.max(n.y));
            }
        }
        b
    }

    #[test]
    fn a_line_becomes_a_band_with_caps() {
        let mut p = line(0.0, 0.0, 100.0, 0.0);
        p.stroke = Some(PathStroke {
            width: 20.0,
            ..Default::default()
        });
        let out = expand(&p).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].closed);
        assert_eq!(bbox(&out), (0.0, -10.0, 100.0, 10.0));
        p.stroke.as_mut().unwrap().end_cap = StrokeCap::Square;
        assert_eq!(bbox(&expand(&p).unwrap()).2, 110.0);
    }

    #[test]
    fn position_moves_the_stroke_to_one_side() {
        // A path running to the right: its left is up, its right is down.
        let at = |position: f64| {
            let mut p = line(0.0, 0.0, 100.0, 0.0);
            p.stroke = Some(PathStroke {
                width: 20.0,
                position,
                ..Default::default()
            });
            let b = bbox(&expand(&p).unwrap());
            ((b.1 * 10.0).round() / 10.0, (b.3 * 10.0).round() / 10.0)
        };
        // -100: the path is the stroke's left edge, so the stroke lies to its right.
        assert_eq!(at(-100.0), (-20.0, 0.0));
        assert_eq!(at(100.0), (0.0, 20.0));
        assert_eq!(at(0.0), (-10.0, 10.0));
    }

    #[test]
    fn pen_points_change_the_width() {
        let mut p = line(0.0, 0.0, 100.0, 0.0);
        let mut s = PathStroke {
            width: 20.0,
            ..Default::default()
        };
        s.widths.insert(1, 60.0);
        p.stroke = Some(s);
        let out = expand(&p).unwrap();
        let (_, y0, _, y1) = bbox(&out);
        assert!((y1 - y0 - 60.0).abs() < 1.0, "{y0} {y1}");
        // The start keeps the base width.
        let at_start: Vec<f64> = out[0]
            .nodes
            .iter()
            .filter(|n| n.x.abs() < 0.6)
            .map(|n| n.y)
            .collect();
        let spread = at_start.iter().cloned().fold(f64::MIN, f64::max)
            - at_start.iter().cloned().fold(f64::MAX, f64::min);
        assert!((spread - 20.0).abs() < 1.0, "{at_start:?}");
    }

    #[test]
    fn closed_paths_make_a_ring_or_a_filled_shape() {
        let sq = Path {
            closed: true,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(100.0, 0.0, NodeKind::Line),
                Node::new(100.0, 100.0, NodeKind::Line),
                Node::new(0.0, 100.0, NodeKind::Line),
            ],
            locked: false,
            stroke: Some(PathStroke {
                width: 10.0,
                join: StrokeJoin::Miter,
                ..Default::default()
            }),
        };
        let ring = expand(&sq).unwrap();
        assert_eq!(ring.len(), 2, "outer and inner contour");
        let inner = ring
            .iter()
            .find(|p| p.nodes.iter().all(|n| n.x > 1.0 && n.x < 99.0))
            .unwrap();
        assert_eq!(
            inner.nodes.len(),
            4,
            "inner corners are cut cleanly: {:?}",
            inner.nodes
        );
        let mut filled = sq.clone();
        filled.stroke.as_mut().unwrap().fill = true;
        let out = expand(&filled).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(bbox(&out), (-5.0, -5.0, 105.0, 105.0));
    }

    #[test]
    fn checks_values() {
        assert!(PathStroke::default().check().is_ok());
        assert!(PathStroke {
            width: 0.0,
            ..Default::default()
        }
        .check()
        .is_err());
        assert!(PathStroke {
            position: 150.0,
            ..Default::default()
        }
        .check()
        .is_err());
    }

    #[test]
    fn structure_does_not_depend_on_width() {
        let curve = |w: f64| Path {
            closed: false,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(0.0, 100.0, NodeKind::OffCurve),
                Node::new(100.0, 200.0, NodeKind::OffCurve),
                Node::new(200.0, 200.0, NodeKind::Curve),
                Node::new(200.0, 0.0, NodeKind::Line),
            ],
            locked: false,
            stroke: Some(PathStroke {
                width: w,
                join: StrokeJoin::Round,
                end_cap: StrokeCap::Round,
                ..Default::default()
            }),
        };
        let kinds = |w: f64| -> Vec<Vec<NodeKind>> {
            expand(&curve(w))
                .unwrap()
                .iter()
                .map(|p| p.nodes.iter().map(|n| n.kind).collect())
                .collect()
        };
        assert_eq!(kinds(10.0), kinds(80.0));
    }

    #[test]
    fn stroke_settings_follow_the_path_into_every_master() {
        let mut f = crate::sample::sample_font();
        let mut m2 = f.masters[0].clone();
        m2.id = "m02".into();
        m2.name = "Bold".into();
        f.masters.push(m2);
        for g in &mut f.glyphs {
            let mut l = g.layers[0].clone();
            l.master_id = "m02".into();
            g.layers.push(l);
        }
        let s = PathStroke {
            width: 30.0,
            join: StrokeJoin::Bevel,
            ..Default::default()
        };
        let f = set_stroke(&f, "H", "m01", 0, Some(s)).unwrap();
        let stroke =
            |f: &crate::model::Font, k: &str| match &f.glyph("H").unwrap().layer(k).unwrap().shapes
                [0]
            {
                Shape::Path(p) => p.stroke.clone(),
                _ => None,
            };
        assert_eq!(stroke(&f, "m02").unwrap().join, StrokeJoin::Bevel);
        // A heavier master keeps its own width when the settings change.
        let mut f2 = f.clone();
        if let Shape::Path(p) = &mut f2.glyph_mut("H").unwrap().layer_mut("m02").unwrap().shapes[0]
        {
            p.stroke.as_mut().unwrap().width = 90.0;
        }
        let f3 = set_stroke(
            &f2,
            "H",
            "m01",
            0,
            Some(PathStroke {
                width: 30.0,
                end_cap: StrokeCap::Round,
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(stroke(&f3, "m02").unwrap().width, 90.0);
        assert_eq!(stroke(&f3, "m02").unwrap().end_cap, StrokeCap::Round);
        let f4 = set_pen_width(&f3, "H", "m01", 0, 1, Some(50.0)).unwrap();
        assert_eq!(stroke(&f4, "m02").unwrap().widths.get(&1), Some(&90.0));
        assert!(crate::interpolate::check_glyph(&f4, "H").is_empty());
        let f5 = expand_glyph(&f4, "H").unwrap();
        assert!(!f5.glyph("H").unwrap().layers.iter().any(has_strokes));
        assert!(
            crate::interpolate::check_glyph(&f5, "H").is_empty(),
            "expanded masters stay compatible"
        );
        assert!(set_stroke(&f, "H", "m01", 0, None).is_ok());
    }

    #[test]
    fn a_brush_bends_along_the_path() {
        let mut f = crate::sample::sample_font();
        // A 100 x 20 bar brush from (0,0) to (100,0).
        let mut brush = crate::model::Glyph::new("_brush.bar", vec![]);
        let mut l = Layer::new("m01", 100.0);
        l.shapes.push(Shape::Path(Path {
            closed: true,
            nodes: vec![
                Node::new(0.0, -10.0, NodeKind::Line),
                Node::new(100.0, -10.0, NodeKind::Line),
                Node::new(100.0, 10.0, NodeKind::Line),
                Node::new(0.0, 10.0, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
        }));
        l.anchors = vec![
            crate::model::Anchor {
                name: "start".into(),
                x: 0.0,
                y: 0.0,
            },
            crate::model::Anchor {
                name: "end".into(),
                x: 100.0,
                y: 0.0,
            },
        ];
        brush.layers.push(l);
        f.glyphs.push(brush);
        // Along a 400-unit vertical line, the bar becomes 400 long and 20 wide.
        let mut p = line(50.0, 0.0, 50.0, 400.0);
        p.stroke = Some(PathStroke {
            brush: Some("_brush.bar".into()),
            ..Default::default()
        });
        let out = expand_in(&f, "m01", &p).unwrap();
        let b = bbox(&out);
        assert!(
            (b.0 - 40.0).abs() < 0.6 && (b.2 - 60.0).abs() < 0.6,
            "{b:?}"
        );
        assert!(b.1.abs() < 0.6 && (b.3 - 400.0).abs() < 0.6, "{b:?}");
        // A missing brush falls back on the pen.
        p.stroke.as_mut().unwrap().brush = Some("_brush.none".into());
        assert!(expand_in(&f, "m01", &p).is_some());
    }
}
