//! Corner, cap and segment components (S58): small glyphs placed onto an outline.
//!
//! - A **corner component** (`_corner.name`) replaces the corner at one node with its
//!   own open path: a serif, an ink trap, a rounded or cut corner.
//! - A **cap component** (`_cap.name`) replaces the segment from one node to the next
//!   (the end of a stem) with its path, stretched to fit between them.
//! - A **segment component** (`_segment.name`) replaces the segment from one node to
//!   the next with its path, stretched along it and bent to follow a curve.
//!
//! They stay live: the layer keeps the outline as drawn plus a list of parts, and
//! [`apply`] builds the outline with them wherever it is drawn or exported.
//!
//! Corners are fitted the way Glyphs fits them. That code follows the corner
//! component support in glyphs-reader (part of fontc, Apache-2.0,
//! <https://github.com/googlefonts/fontc>), itself a port of glyphsLib's
//! `cornerComponents.py`: the same `origin`, `left` and `right` anchors, the same
//! turning, shearing and sliding, so corner glyphs made in Glyphs place the same.

use kurbo::{
    Affine, CubicBez, Line, ParamCurve, ParamCurveArclen, ParamCurveDeriv, PathSeg, Point, Vec2,
};
use serde::{Deserialize, Serialize};

use crate::model::{Font, Layer, Node, NodeKind, Path, Shape};

type R<T> = Result<T, String>;

/// What kind of part a layer places on its outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PartKind {
    Corner,
    Cap,
    Segment,
}

/// How a corner component lines up with the strokes either side of its node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CornerAlign {
    /// Along the stroke leaving the node (Glyphs: left).
    #[default]
    Out,
    /// Along the stroke arriving at the node (Glyphs: right).
    In,
    /// Halfway between the two.
    Middle,
    /// Not turned at all.
    Unaligned,
}

/// A corner, cap or segment component placed on one of a layer's paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathPart {
    pub kind: PartKind,
    /// The glyph drawn there, e.g. `_corner.serif`.
    pub glyph: String,
    /// Which path (counting paths only) and which node of it.
    pub path: usize,
    pub node: usize,
    /// Horizontal and vertical scale; a negative horizontal scale flips a corner.
    #[serde(default = "unit_scale")]
    pub scale: [f64; 2],
    #[serde(default, skip_serializing_if = "is_default_align")]
    pub align: CornerAlign,
    /// Cap only: scale the cap's height with its width, so it keeps its shape.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fit: bool,
}

fn unit_scale() -> [f64; 2] {
    [1.0, 1.0]
}

fn is_default_align(a: &CornerAlign) -> bool {
    *a == CornerAlign::Out
}

impl PathPart {
    pub fn new(kind: PartKind, glyph: impl Into<String>, path: usize, node: usize) -> Self {
        Self {
            kind,
            glyph: glyph.into(),
            path,
            node,
            scale: unit_scale(),
            align: CornerAlign::Out,
            fit: false,
        }
    }
}

/// The glyph-name prefix for each kind.
pub fn prefix(kind: PartKind) -> &'static str {
    match kind {
        PartKind::Corner => "_corner",
        PartKind::Cap => "_cap",
        PartKind::Segment => "_segment",
    }
}

// ---------------------------------------------------------------------------
// Working path: points with kinds, for splicing.

#[derive(Debug, Clone, Copy)]
struct WNode {
    pt: Point,
    kind: NodeKind,
    smooth: bool,
}

#[derive(Debug, Clone)]
struct WPath {
    closed: bool,
    nodes: Vec<WNode>,
}

impl WPath {
    fn from(p: &Path) -> Self {
        Self {
            closed: p.closed,
            nodes: p
                .nodes
                .iter()
                .map(|n| WNode {
                    pt: Point::new(n.x, n.y),
                    kind: n.kind,
                    smooth: n.smooth,
                })
                .collect(),
        }
    }

    fn write_to(&self, p: &mut Path) {
        p.closed = self.closed;
        p.nodes = self
            .nodes
            .iter()
            .map(|n| Node {
                x: (n.pt.x * 100.0).round() / 100.0,
                y: (n.pt.y * 100.0).round() / 100.0,
                kind: n.kind,
                smooth: n.smooth,
            })
            .collect();
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn next_idx(&self, i: usize) -> usize {
        (i + 1) % self.len()
    }

    fn prev_idx(&self, i: usize) -> usize {
        (i + self.len() - 1) % self.len()
    }

    fn on(&self, i: usize) -> bool {
        self.nodes[i].kind != NodeKind::OffCurve
    }

    /// The segment arriving at on-curve node `i` (from the previous on-curve node).
    fn prev_segment(&self, i: usize) -> Option<PathSeg> {
        if !self.closed && i == 0 {
            return None;
        }
        let mut offs = Vec::new();
        let mut j = self.prev_idx(i);
        let mut guard = 0;
        while !self.on(j) {
            offs.push(self.nodes[j].pt);
            j = self.prev_idx(j);
            guard += 1;
            if guard > self.len() {
                return None;
            }
        }
        offs.reverse();
        seg(self.nodes[j].pt, &offs, self.nodes[i].pt)
    }

    /// The segment leaving on-curve node `i` (to the next on-curve node).
    fn next_segment(&self, i: usize) -> Option<PathSeg> {
        if !self.closed && i + 1 >= self.len() {
            return None;
        }
        let mut offs = Vec::new();
        let mut j = self.next_idx(i);
        let mut guard = 0;
        while !self.on(j) {
            offs.push(self.nodes[j].pt);
            j = self.next_idx(j);
            guard += 1;
            if guard > self.len() {
                return None;
            }
        }
        seg(self.nodes[i].pt, &offs, self.nodes[j].pt)
    }

    /// The index of the next on-curve node after `i`.
    fn next_on(&self, i: usize) -> Option<usize> {
        let mut j = self.next_idx(i);
        for _ in 0..self.len() {
            if self.on(j) {
                return Some(j);
            }
            j = self.next_idx(j);
        }
        None
    }

    fn replace_instroke(&mut self, idx: usize, c: CubicBez) {
        // The two off-curves before `idx` and the on-curve before them.
        let h2 = self.prev_idx(idx);
        let h1 = self.prev_idx(h2);
        let start = self.prev_idx(h1);
        self.nodes[start].pt = c.p0;
        self.nodes[h1].pt = c.p1;
        self.nodes[h2].pt = c.p2;
        self.nodes[idx].pt = c.p3;
    }

    fn replace_outstroke(&mut self, idx: usize, c: CubicBez) {
        let h1 = self.next_idx(idx);
        let h2 = self.next_idx(h1);
        let end = self.next_idx(h2);
        self.nodes[idx].pt = c.p0;
        self.nodes[h1].pt = c.p1;
        self.nodes[h2].pt = c.p2;
        self.nodes[end].pt = c.p3;
    }

    fn reverse_direction(&mut self) {
        let mut p = Path {
            closed: self.closed,
            nodes: Vec::new(),
            locked: false,
            stroke: None,
            mask: false,
        };
        self.write_to_exact(&mut p);
        let r = crate::pathops::reverse(&p);
        *self = WPath::from(&r);
    }

    fn write_to_exact(&self, p: &mut Path) {
        p.closed = self.closed;
        p.nodes = self
            .nodes
            .iter()
            .map(|n| Node {
                x: n.pt.x,
                y: n.pt.y,
                kind: n.kind,
                smooth: n.smooth,
            })
            .collect();
    }
}

fn seg(a: Point, offs: &[Point], b: Point) -> Option<PathSeg> {
    match offs.len() {
        0 => Some(PathSeg::Line(Line::new(a, b))),
        2 => Some(PathSeg::Cubic(CubicBez::new(a, offs[0], offs[1], b))),
        1 => Some(PathSeg::Quad(kurbo::QuadBez::new(a, offs[0], b))),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Applying parts.

/// The layer's outline with its corner, cap and segment components applied (the
/// parts list emptied). Parts that can't be placed (a missing glyph, a node that no
/// longer exists) are left out.
pub fn apply(font: &Font, layer: &Layer) -> Layer {
    let mut out = layer.clone();
    out.parts.clear();
    if layer.parts.is_empty() {
        return out;
    }
    // Path index → shape index.
    let shape_of: Vec<usize> = out
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, Shape::Path(_)))
        .map(|(i, _)| i)
        .collect();
    // Corners first, in node order per path, then caps and segments from the
    // last node backwards so earlier node numbers stay valid.
    let mut corners: Vec<&PathPart> = layer
        .parts
        .iter()
        .filter(|p| p.kind == PartKind::Corner)
        .collect();
    corners.sort_by_key(|p| (p.path, p.node));
    let mut others: Vec<&PathPart> = layer
        .parts
        .iter()
        .filter(|p| p.kind != PartKind::Corner)
        .collect();
    others.sort_by_key(|p| (p.path, std::cmp::Reverse(p.node)));
    let mut extra: Vec<Path> = Vec::new();
    // Inserted nodes per path, so later corners on the same path find their node.
    let mut shift: std::collections::HashMap<usize, usize> = Default::default();
    // Caps and segments go first (from the end) when a path has no corners after
    // them; corners record how many nodes they add.
    for part in others {
        let Some(&si) = shape_of.get(part.path) else {
            continue;
        };
        let Some(source) = part_layer(font, &part.glyph, &layer.master_id) else {
            continue;
        };
        if let Shape::Path(p) = &mut out.shapes[si] {
            let mut w = WPath::from(p);
            let ok = match part.kind {
                PartKind::Cap => place_cap(&mut w, part, &source),
                PartKind::Segment => place_segment(&mut w, part, &source),
                PartKind::Corner => false,
            };
            if ok {
                w.write_to(p);
            }
        }
    }
    for part in corners {
        let Some(&si) = shape_of.get(part.path) else {
            continue;
        };
        let Some(source) = part_layer(font, &part.glyph, &layer.master_id) else {
            continue;
        };
        let Ok(component) = CornerComponent::new(&source) else {
            continue;
        };
        if let Shape::Path(p) = &mut out.shapes[si] {
            let mut w = WPath::from(p);
            let delta = shift.get(&part.path).copied().unwrap_or(0);
            if let Ok((added, others)) = insert_corner(&mut w, component, part, delta) {
                *shift.entry(part.path).or_default() += added;
                w.write_to(p);
                extra.extend(others);
            }
        }
    }
    out.shapes.extend(extra.into_iter().map(Shape::Path));
    out
}

/// The part glyph's layer for this master (or the default master), with its own
/// components resolved to outlines.
fn part_layer(font: &Font, glyph: &str, master_id: &str) -> Option<Layer> {
    let g = font.glyph(glyph)?;
    let l = g
        .layer(master_id)
        .or_else(|| g.layer(&crate::interpolate::default_master(font).id))?;
    let mut flat = l.clone();
    flat.shapes.clear();
    // The part glyph's own parts aren't applied, so a part can't place itself.
    let mut plain = l.clone();
    plain.parts.clear();
    crate::geometry::for_each_path(font, &plain, &crate::model::IDENTITY, 0, &mut |q, t| {
        let mut q = q.clone();
        q.stroke = None;
        for n in &mut q.nodes {
            let (x, y) = (n.x, n.y);
            n.x = t[0] * x + t[2] * y + t[4];
            n.y = t[1] * x + t[3] * y + t[5];
        }
        flat.shapes.push(Shape::Path(q));
    });
    (!flat.shapes.is_empty()).then_some(flat)
}

fn first_path(l: &Layer) -> Option<WPath> {
    l.shapes.iter().find_map(|s| match s {
        Shape::Path(p) if p.nodes.len() >= 2 => Some(WPath::from(p)),
        _ => None,
    })
}

/// The ends a cap or segment glyph spans: its `start` and `end` anchors, else the
/// first and last nodes of its first path.
fn span_ends(l: &Layer, path: &WPath) -> (Point, Point) {
    let anchor = |n: &str| {
        l.anchors
            .iter()
            .find(|a| a.name == n)
            .map(|a| Point::new(a.x, a.y))
    };
    match (anchor("start"), anchor("end")) {
        (Some(a), Some(b)) if (b - a).hypot() > 1e-9 => (a, b),
        _ => (path.nodes[0].pt, path.nodes[path.len() - 1].pt),
    }
}

/// Replaces the segment from `node` to the next on-curve node with `piece`, whose
/// first and last nodes land exactly on the segment's ends.
fn splice_segment(w: &mut WPath, node: usize, piece: &[WNode]) -> bool {
    if node >= w.len() || !w.on(node) || piece.len() < 2 {
        return false;
    }
    let Some(end) = w.next_on(node) else {
        return false;
    };
    if !w.closed && end <= node {
        return false;
    }
    let mut middle: Vec<WNode> = piece[1..piece.len() - 1].to_vec();
    // The end node takes the piece's last segment type.
    let end_kind = piece[piece.len() - 1].kind;
    // Remove the off-curves between node and end, insert the middle.
    let mut idxs = Vec::new();
    let mut j = w.next_idx(node);
    while j != end {
        idxs.push(j);
        j = w.next_idx(j);
    }
    // Rebuild the node list starting just after `node`.
    let n = w.len();
    let mut rebuilt: Vec<WNode> = Vec::with_capacity(n + middle.len());
    let mut i = 0;
    while i < n {
        if idxs.contains(&i) {
            if i == idxs[0] {
                rebuilt.append(&mut middle);
            }
        } else {
            let mut nd = w.nodes[i];
            if i == end {
                nd.kind = end_kind;
                nd.smooth = false;
            }
            if i == node {
                nd.smooth = false;
            }
            rebuilt.push(nd);
            if idxs.is_empty() && i == node {
                rebuilt.append(&mut middle);
            }
        }
        i += 1;
    }
    w.nodes = rebuilt;
    true
}

/// The outward normal of a segment from `a` to `b`: to the right of travel, which
/// is outside an outer contour (outer contours run counter-clockwise).
fn outward(a: Point, b: Point) -> (Vec2, Vec2) {
    let d = (b - a).normalize();
    (d, Vec2::new(d.y, -d.x))
}

fn place_cap(w: &mut WPath, part: &PathPart, source: &Layer) -> bool {
    let Some(cap) = first_path(source) else {
        return false;
    };
    if part.node >= w.len() || !w.on(part.node) {
        return false;
    }
    let Some(end) = w.next_on(part.node) else {
        return false;
    };
    let (a, b) = (w.nodes[part.node].pt, w.nodes[end].pt);
    let (s0, s1) = span_ends(source, &cap);
    let span = (s1 - s0).hypot();
    let len = (b - a).hypot();
    if span < 1e-9 || len < 1e-9 {
        return false;
    }
    let (d, n) = outward(a, b);
    let (cd, cn) = outward(s0, s1);
    // In the cap's own frame: along its span, and up (away from the outline).
    let sx = len / span * part.scale[0].abs().max(1e-9);
    let sy = if part.fit { sx } else { 1.0 } * part.scale[1];
    let map = |p: Point| {
        let v = p - s0;
        let along = v.dot(cd);
        let up = -v.dot(cn);
        a + d * (along * sx) + n * (up * sy)
    };
    let mut piece: Vec<WNode> = cap
        .nodes
        .iter()
        .map(|nd| WNode {
            pt: map(nd.pt),
            kind: nd.kind,
            smooth: nd.smooth,
        })
        .collect();
    let last = piece.len() - 1;
    piece[0].pt = a;
    piece[last].pt = b;
    splice_segment(w, part.node, &piece)
}

fn place_segment(w: &mut WPath, part: &PathPart, source: &Layer) -> bool {
    let Some(shape) = first_path(source) else {
        return false;
    };
    if part.node >= w.len() || !w.on(part.node) {
        return false;
    }
    let Some(target) = w.next_segment(part.node) else {
        return false;
    };
    let (s0, s1) = span_ends(source, &shape);
    let span = (s1 - s0).hypot();
    if span < 1e-9 {
        return false;
    }
    let (cd, cn) = outward(s0, s1);
    let len = target.arclen(0.01);
    let at = |along: f64, up: f64| -> Point {
        // Distance along the target segment, then outwards from it.
        let t = match target {
            PathSeg::Line(_) => along / len.max(1e-9),
            other => other.inv_arclen(along.clamp(0.0, len), 0.01),
        };
        let p = target.eval(t.clamp(0.0, 1.0));
        let tan = match target {
            PathSeg::Line(l) => l.p1 - l.p0,
            PathSeg::Quad(q) => q.deriv().eval(t).to_vec2(),
            PathSeg::Cubic(c) => c.deriv().eval(t).to_vec2(),
        };
        let tan = if tan.hypot() < 1e-9 {
            Vec2::new(1.0, 0.0)
        } else {
            tan.normalize()
        };
        let n = Vec2::new(tan.y, -tan.x);
        // Beyond either end the segment continues straight.
        let extra = if along < 0.0 {
            along
        } else if along > len {
            along - len
        } else {
            0.0
        };
        p + tan * extra + n * (up * part.scale[1])
    };
    let sx = len / span * part.scale[0].abs().max(1e-9);
    let map = |p: Point| {
        let v = p - s0;
        at(v.dot(cd) * sx, -v.dot(cn))
    };
    let straight = matches!(target, PathSeg::Line(_));
    let mut piece: Vec<WNode> = Vec::new();
    // On a curve, split the segment glyph's curves so they bend with it.
    let steps = if straight { 1 } else { 4 };
    let nodes = &shape.nodes;
    piece.push(WNode {
        pt: map(nodes[0].pt),
        kind: NodeKind::Line,
        smooth: false,
    });
    let mut i = 1;
    let mut prev = nodes[0].pt;
    while i < nodes.len() {
        let mut offs = Vec::new();
        while i < nodes.len() && nodes[i].kind == NodeKind::OffCurve {
            offs.push(nodes[i].pt);
            i += 1;
        }
        if i >= nodes.len() {
            break;
        }
        let end = nodes[i];
        let c = match offs.len() {
            0 => CubicBez::new(
                prev,
                prev.lerp(end.pt, 1.0 / 3.0),
                prev.lerp(end.pt, 2.0 / 3.0),
                end.pt,
            ),
            1 => kurbo::QuadBez::new(prev, offs[0], end.pt).raise(),
            _ => CubicBez::new(prev, offs[0], offs[offs.len() - 1], end.pt),
        };
        let is_line = offs.is_empty() && straight;
        for k in 0..steps {
            let sub = c.subsegment(k as f64 / steps as f64..(k + 1) as f64 / steps as f64);
            if is_line {
                piece.push(WNode {
                    pt: map(sub.p3),
                    kind: NodeKind::Line,
                    smooth: false,
                });
            } else {
                piece.push(WNode {
                    pt: map(sub.p1),
                    kind: NodeKind::OffCurve,
                    smooth: false,
                });
                piece.push(WNode {
                    pt: map(sub.p2),
                    kind: NodeKind::OffCurve,
                    smooth: false,
                });
                piece.push(WNode {
                    pt: map(sub.p3),
                    kind: NodeKind::Curve,
                    smooth: k + 1 < steps || end.smooth,
                });
            }
        }
        prev = end.pt;
        i += 1;
    }
    let a = w.nodes[part.node].pt;
    let end_idx = match w.next_on(part.node) {
        Some(e) => e,
        None => return false,
    };
    let b = w.nodes[end_idx].pt;
    let last = piece.len() - 1;
    piece[0].pt = a;
    piece[last].pt = b;
    splice_segment(w, part.node, &piece)
}

// ---------------------------------------------------------------------------
// Corner components, fitted the way Glyphs fits them (after glyphs-reader).

#[derive(Clone, Copy)]
enum End {
    First,
    Last,
}

struct CornerComponent {
    corner: WPath,
    others: Vec<WPath>,
    left: Option<Point>,
    right: Option<Point>,
    axes: [Vec2; 2],
    mirror: Option<Affine>,
}

struct Fitting {
    align: CornerAlign,
    ends: [Vec2; 2],
    sign: f64,
}

impl CornerComponent {
    fn new(l: &Layer) -> R<Self> {
        let anchor = |n: &str| {
            l.anchors
                .iter()
                .find(|a| a.name == n)
                .map(|a| Point::new(a.x, a.y))
        };
        let origin = anchor("origin").unwrap_or_default().to_vec2();
        let left = anchor("left").map(|p| p - origin);
        let right = anchor("right").map(|p| p - origin);
        let mut paths = l.shapes.iter().filter_map(|s| match s {
            Shape::Path(p) => {
                let mut w = WPath::from(p);
                w.nodes.iter_mut().for_each(|n| n.pt -= origin);
                Some(w)
            }
            _ => None,
        });
        let corner = paths.next().ok_or("The corner glyph has no paths.")?;
        if corner.len() < 2 {
            return Err("The corner glyph's path is too short.".into());
        }
        Ok(Self {
            corner,
            others: paths.collect(),
            left,
            right,
            axes: [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            mirror: None,
        })
    }

    fn paths_mut(&mut self) -> impl Iterator<Item = &mut WPath> {
        std::iter::once(&mut self.corner).chain(self.others.iter_mut())
    }

    fn apply_transform(&mut self, t: Affine) {
        for n in self.paths_mut().flat_map(|p| p.nodes.iter_mut()) {
            n.pt = t * n.pt;
        }
        for a in [&mut self.left, &mut self.right].into_iter().flatten() {
            *a = t * *a;
        }
    }

    fn ends(&self) -> (Point, Point) {
        let n = &self.corner.nodes;
        (n[0].pt, n[n.len() - 1].pt)
    }

    fn pointing_ends(&self) -> (Vec2, Vec2) {
        let (first, last) = self.ends();
        (
            self.left.unwrap_or(first).to_vec2(),
            self.right.unwrap_or(last).to_vec2(),
        )
    }

    fn reverse_paths(&mut self) {
        self.paths_mut().for_each(WPath::reverse_direction);
    }

    fn mirror(&mut self) {
        let angle = self.ends().0.to_vec2().atan2();
        let m =
            Affine::rotate(angle) * Affine::scale_non_uniform(1.0, -1.0) * Affine::rotate(-angle);
        self.apply_transform(m);
        self.axes = self.axes.map(|a| (m * a.to_point()).to_vec2());
        self.mirror = Some(m);
    }

    fn end_indices(&self, end: End) -> (usize, usize) {
        let n = self.corner.len();
        match end {
            End::First => (0, 1),
            End::Last => (n - 1, n - 2),
        }
    }

    fn rotation(&self, f: &Fitting, dirs: [Vec2; 2]) -> f64 {
        let [x_axis, y_axis] = self.axes;
        let [in_turn, out_turn] = [0, 1].map(|i| {
            let end = if f.ends[i] == Vec2::ZERO {
                y_axis
            } else {
                f.ends[i]
            };
            turn_towards(end, dirs[i])
        });
        match f.align {
            CornerAlign::Out => out_turn,
            CornerAlign::In => in_turn,
            CornerAlign::Middle if f.ends.contains(&Vec2::ZERO) => {
                let [a, b] = dirs.map(Vec2::atan2);
                a + ieee_remainder(b - a, std::f64::consts::TAU) / 2.0 - x_axis.atan2()
            }
            CornerAlign::Middle => {
                in_turn + ieee_remainder(out_turn - in_turn, std::f64::consts::TAU) / 2.0
            }
            CornerAlign::Unaligned => 0.0,
        }
    }

    fn end_fit(&self, end: End, f: &Fitting, dirs: [Vec2; 2], aiming: bool) -> Option<Affine> {
        let (ei, ni) = self.end_indices(end);
        let (i, anchor) = match end {
            End::First => (0, self.left),
            End::Last => (1, self.right),
        };
        let nodes = &self.corner.nodes;
        let segment = nodes[ni].pt - nodes[ei].pt;
        if segment == Vec2::ZERO {
            return None;
        }
        let pivot = anchor.unwrap_or_default();
        let towards = f.ends[i];
        let (turn, along) = if towards == Vec2::ZERO {
            (turn_towards(self.axes[1], dirs[i]), true)
        } else {
            let turned = Affine::rotate(self.rotation(f, dirs));
            let mut line = (turned * towards.normalize().to_point()).to_vec2();
            let mut segment = (turned * segment.normalize().to_point()).to_vec2();
            if aiming && pivot != Point::ZERO {
                let offset = self.anchor_turn_offset(turned, pivot);
                line += offset;
                segment += offset;
            }
            (turn_towards(line, dirs[i]), line.cross(segment).abs() < 0.5)
        };
        let turn = f.sign * turn;
        let fit = if along {
            Affine::rotate(turn)
        } else if turn.cos().abs() <= 1e-4 {
            return None;
        } else {
            let [x_axis, y_axis] = self.axes;
            let axis = if y_axis.dot(towards).abs() > x_axis.dot(towards).abs() {
                y_axis
            } else {
                x_axis
            };
            shear_across(axis, turn)
        };
        let pivot = pivot.to_vec2();
        Some(Affine::translate(pivot) * fit * Affine::translate(-pivot))
    }

    fn apply_end_fit(&mut self, end: End, fit: Affine) {
        let (ei, ni) = self.end_indices(end);
        let moving: &[usize] = if self.corner.nodes[ni].kind == NodeKind::OffCurve {
            &[ei, ni]
        } else {
            &[ei]
        };
        for &i in moving {
            self.corner.nodes[i].pt = fit * self.corner.nodes[i].pt;
        }
    }

    fn anchor_turn_offset(&self, turned: Affine, anchor: Point) -> Vec2 {
        match self.mirror {
            None => anchor - turned * anchor,
            Some(m) => {
                let a = m * anchor;
                a.to_vec2() + (turned * m * Point::new(-a.x, a.y)).to_vec2()
            }
        }
    }

    fn place(
        &mut self,
        align: CornerAlign,
        node: Point,
        rotation: f64,
        in_dir: Vec2,
        out_dir: Vec2,
    ) {
        let mut t = Affine::translate(node.to_vec2()) * Affine::rotate(rotation);
        let slide = match align {
            CornerAlign::Out => self.left.map(|l| (l, out_dir, in_dir)),
            CornerAlign::In => self.right.map(|r| (r, in_dir, out_dir)),
            _ => None,
        };
        if let Some((anchor, along, onto)) = slide {
            if along != Vec2::ZERO {
                if let Some(d) = distance_to_line(t * anchor, along, Line::new(node, node + onto)) {
                    t = Affine::translate(along * d) * t;
                }
            }
        }
        self.apply_transform(t);
    }
}

/// Inserts a corner component at `part.node` (+ `delta` nodes added by earlier
/// corners on this path). Returns how many nodes were added and any extra paths.
fn insert_corner(
    w: &mut WPath,
    mut c: CornerComponent,
    part: &PathPart,
    delta: usize,
) -> R<(usize, Vec<Path>)> {
    if w.len() < 2 {
        return Err("path too short".into());
    }
    let idx = (part.node + delta) % w.len();
    if !w.on(idx) {
        return Err("off-curve node".into());
    }
    let instroke = line_or_cubic(w.prev_segment(idx))?;
    let outstroke = line_or_cubic(w.next_segment(idx))?;
    let node = w.nodes[idx].pt;

    c.apply_transform(Affine::scale_non_uniform(part.scale[0], part.scale[1]));
    let flipped = part.scale[0] * part.scale[1] < 0.0;
    if flipped {
        c.reverse_paths();
        std::mem::swap(&mut c.left, &mut c.right);
    }
    let (first, last) = c.ends();
    let in_dir = aim_along(instroke.reverse(), first.to_vec2().hypot());
    let out_dir = aim_along(outstroke, last.to_vec2().hypot());
    let host_turn = match in_dir.cross(out_dir) {
        0.0 if in_dir.dot(out_dir) < 0.0 => 1.0,
        t => t,
    };
    let (in_end, out_end) = c.pointing_ends();
    let zero_end = in_end == Vec2::ZERO || out_end == Vec2::ZERO;
    let corner_turn = if zero_end {
        -1.0
    } else {
        in_end.cross(out_end)
    };
    let mut other_way = host_turn * corner_turn < 0.0;
    if other_way && part.align != CornerAlign::Unaligned {
        c.mirror();
        other_way = false;
    }
    let (in_end, out_end) = c.pointing_ends();
    let fitting = Fitting {
        align: part.align,
        ends: [in_end, out_end],
        sign: if other_way { -1.0 } else { 1.0 },
    };
    let strokes = [instroke.reverse(), outstroke];
    let mut dirs = [in_dir, out_dir];
    let mut reach = [0.0; 2];
    let (first, last) = c.ends();
    for (i, (end, pt)) in [(End::First, first), (End::Last, last)]
        .into_iter()
        .enumerate()
    {
        let fit = c
            .end_fit(end, &fitting, dirs, true)
            .unwrap_or(Affine::IDENTITY);
        reach[i] = (fit * pt).to_vec2().hypot();
        dirs[i] = aim_along(strokes[i], reach[i]);
    }
    let rotation = c.rotation(&fitting, dirs);
    for end in [End::First, End::Last] {
        if let Some(fit) = c.end_fit(end, &fitting, dirs, false) {
            c.apply_end_fit(end, fit);
        }
    }
    let (first, last) = c.ends();
    let (in_cut, out_cut) = (first.to_vec2().hypot(), last.to_vec2().hypot());
    let [in_dir, out_dir] = dirs;
    let past_end = |s: PathSeg, d: f64| matches!(s, PathSeg::Cubic(cb) if d >= glyphs_length(cb));
    let straight_in = past_end(instroke, reach[0].max(in_cut));
    let uncut_out = past_end(outstroke, reach[1]);
    let slide = |end: Vec2, dir: Vec2| if end == Vec2::ZERO { Vec2::ZERO } else { dir };
    c.place(
        part.align,
        node,
        rotation,
        slide(in_end, in_dir),
        slide(out_end, out_dir),
    );

    let first = c.ends().0;
    match instroke {
        PathSeg::Cubic(cb) if straight_in => {
            let start = cb.p0;
            w.replace_instroke(
                idx,
                CubicBez::new(
                    start,
                    start.lerp(first, 1.0 / 3.0),
                    start.lerp(first, 2.0 / 3.0),
                    first,
                ),
            );
        }
        PathSeg::Cubic(cb) => {
            let t = t_at_distance(instroke.reverse(), in_cut);
            w.replace_instroke(idx, cb.subsegment(0.0..1.0 - t));
        }
        _ => w.nodes[idx].pt = first,
    }
    w.nodes[idx].smooth = false;
    let insert_at = w.next_idx(idx);
    let mut new_nodes: Vec<WNode> = c.corner.nodes.iter().skip(1).copied().collect();
    if let Some(n0) = new_nodes.first_mut() {
        if n0.kind != NodeKind::OffCurve {
            n0.smooth = false;
        }
    }
    let added = new_nodes.len();
    // A closed path inserting after its last node puts the nodes at the end.
    let insert_at = if insert_at == 0 { w.len() } else { insert_at };
    w.nodes.splice(insert_at..insert_at, new_nodes);
    let out_idx = insert_at + added - 1;
    let last = c.ends().1;
    // The outstroke now starts at the corner's last node, which takes the type of
    // the corner's last segment; the old outstroke keeps its own type after it.
    match outstroke {
        PathSeg::Cubic(_) if uncut_out => {}
        PathSeg::Cubic(cb) => {
            let t = t_at_distance(outstroke, out_cut);
            let mut rest = cb.subsegment(t..1.0);
            rest.p0 = last;
            w.replace_outstroke(out_idx, rest);
        }
        _ => w.nodes[out_idx].pt = last,
    }
    let others = c
        .others
        .into_iter()
        .map(|o| {
            let mut p = Path {
                closed: o.closed,
                nodes: Vec::new(),
                locked: false,
                stroke: None,
                mask: false,
            };
            o.write_to(&mut p);
            p
        })
        .collect();
    Ok((added, others))
}

fn line_or_cubic(seg: Option<PathSeg>) -> R<PathSeg> {
    match seg {
        Some(PathSeg::Quad(q)) => Ok(PathSeg::Cubic(q.raise())),
        Some(s) => Ok(s),
        None => Err("no segment".into()),
    }
}

fn aim_along(stroke: PathSeg, distance: f64) -> Vec2 {
    let cubic = stroke.to_cubic();
    let target = match stroke {
        PathSeg::Line(_) => cubic.p0,
        curve => curve.eval(t_at_distance(curve, distance)),
    };
    [target, cubic.p1, cubic.p2, cubic.p3]
        .into_iter()
        .map(|p| p - cubic.p0)
        .find(|d| *d != Vec2::ZERO)
        .map(Vec2::normalize)
        .unwrap_or(Vec2::new(0.0, 1.0))
}

fn turn_towards(v: Vec2, dir: Vec2) -> f64 {
    if dir == Vec2::ZERO {
        return 0.0;
    }
    ieee_remainder(dir.atan2() - v.atan2(), std::f64::consts::TAU)
}

fn shear_across(axis: Vec2, angle: f64) -> Affine {
    let Vec2 { x, y } = axis;
    let k = angle.tan();
    Affine::new([
        1.0 - k * x * y,
        k * x * x,
        -k * y * y,
        1.0 + k * x * y,
        0.0,
        0.0,
    ])
}

fn alignment_transform(start: Point, end: Point) -> Affine {
    Affine::rotate(-(end - start).atan2()) * Affine::translate(-start.to_vec2())
}

fn glyphs_length(c: CubicBez) -> f64 {
    (1..=10)
        .map(|i| {
            c.eval((i - 1) as f64 / 10.0)
                .distance(c.eval(i as f64 / 10.0))
        })
        .sum()
}

fn glyphs_t_for_distance(c: CubicBez, distance: f64) -> f64 {
    let total = glyphs_length(c);
    if distance >= total {
        return 1.0;
    }
    if distance <= 0.0 {
        return 0.0;
    }
    let mut t = distance / total;
    for _ in 0..4 {
        t *= (1.0 + distance / glyphs_length(c.subsegment(0.0..t))) / 2.0;
    }
    t
}

fn t_at_distance(seg: PathSeg, distance: f64) -> f64 {
    match seg {
        PathSeg::Line(l) => {
            let len = (l.p1 - l.p0).hypot();
            if len == 0.0 {
                0.0
            } else {
                distance / len
            }
        }
        curve => glyphs_t_for_distance(curve.to_cubic(), distance),
    }
}

fn distance_to_line(point: Point, direction: Vec2, line: Line) -> Option<f64> {
    let Line { p0, p1 } = alignment_transform(point, point + direction) * line;
    if (p0.y - p1.y).abs() <= 1e-9 * p0.y.abs().max(p1.y.abs()) {
        return None;
    }
    let t = p0.y / (p0.y - p1.y);
    Some(p0.x + (p1.x - p0.x) * t)
}

fn ieee_remainder(x: f64, y: f64) -> f64 {
    x - (x / y).round_ties_even() * y
}

// ---------------------------------------------------------------------------
// Editing.

/// Adds a part at `path`/`node` of a layer, and the same part in the glyph's other
/// masters and intermediate layers that have that node (so they stay compatible).
pub fn add_part(font: &Font, glyph: &str, key: &str, part: PathPart) -> R<Font> {
    if font.glyph(&part.glyph).is_none() {
        return Err(format!("There is no glyph named \"{}\".", part.glyph));
    }
    if part.glyph == glyph {
        return Err("A glyph can't be placed on its own outline.".into());
    }
    check_target(font, glyph, key, &part)?;
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    for l in g
        .layers
        .iter_mut()
        .filter(|l| l.color.is_none() && l.backup.is_none())
    {
        if l.key() != key && !has_node(l, part.path, part.node) {
            continue;
        }
        l.parts
            .retain(|p| !(p.path == part.path && p.node == part.node && p.kind == part.kind));
        l.parts.push(part.clone());
    }
    Ok(f)
}

/// Changes or (with `None`) removes the part at `index` of a layer, in every
/// partner layer too.
pub fn set_part(
    font: &Font,
    glyph: &str,
    key: &str,
    index: usize,
    part: Option<PathPart>,
) -> R<Font> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let old = g
        .layer(key)
        .and_then(|l| l.parts.get(index))
        .cloned()
        .ok_or("There is no such part.")?;
    if let Some(p) = &part {
        if p.scale.iter().any(|s| !s.is_finite() || *s == 0.0) {
            return Err("A scale can't be 0.".into());
        }
    }
    for l in g
        .layers
        .iter_mut()
        .filter(|l| l.color.is_none() && l.backup.is_none())
    {
        let Some(i) = l
            .parts
            .iter()
            .position(|p| p.kind == old.kind && p.path == old.path && p.node == old.node)
        else {
            continue;
        };
        match &part {
            Some(p) => {
                l.parts[i] = PathPart {
                    path: old.path,
                    node: old.node,
                    kind: old.kind,
                    ..p.clone()
                }
            }
            None => {
                l.parts.remove(i);
            }
        }
    }
    Ok(f)
}

/// Decompose: the glyph's parts become part of its outlines, in every layer.
pub fn decompose_parts(font: &Font, glyph: &str) -> R<Font> {
    let mut f = font.clone();
    let source = font
        .glyph(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    if !source.layers.iter().any(|l| !l.parts.is_empty()) {
        return Err("This glyph has no corner, cap or segment components.".into());
    }
    let g = f.glyph_mut(glyph).expect("checked");
    for l in &mut g.layers {
        if !l.parts.is_empty() {
            *l = apply(font, l);
        }
    }
    Ok(f)
}

fn has_node(l: &Layer, path: usize, node: usize) -> bool {
    l.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .nth(path)
        .is_some_and(|p| {
            p.nodes
                .get(node)
                .is_some_and(|n| n.kind != NodeKind::OffCurve)
        })
}

fn check_target(font: &Font, glyph: &str, key: &str, part: &PathPart) -> R<()> {
    let l = font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or("That layer doesn't exist.")?;
    let p = l
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .nth(part.path)
        .ok_or("No path at that position.")?;
    let n = p.nodes.get(part.node).ok_or("No node at that position.")?;
    if n.kind == NodeKind::OffCurve {
        return Err("Choose an on-curve node.".into());
    }
    if part.kind != PartKind::Corner && !p.closed && part.node + 1 >= p.nodes.len() {
        return Err("The last node of an open path starts no segment.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Glyph};
    use crate::sample::sample_font;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
        // Counter-clockwise, as outer contours run.
        Path {
            closed: true,
            nodes: vec![
                Node::new(x0, y0, NodeKind::Line),
                Node::new(x1, y0, NodeKind::Line),
                Node::new(x1, y1, NodeKind::Line),
                Node::new(x0, y1, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
            mask: false,
        }
    }

    fn add_glyph(f: &mut Font, name: &str, path: Path, anchors: Vec<Anchor>) {
        let mut g = Glyph::new(name, vec![]);
        g.export = false;
        for m in f.masters.clone() {
            let mut l = Layer::new(m.id.clone(), 0.0);
            l.shapes.push(Shape::Path(path.clone()));
            l.anchors = anchors.clone();
            g.layers.push(l);
        }
        f.glyphs.push(g);
    }

    fn host(f: &mut Font) {
        let g = f.glyph_mut("H").unwrap();
        g.layers[0].shapes = vec![Shape::Path(rect(0.0, 0.0, 100.0, 300.0))];
    }

    fn nodes(l: &Layer) -> Vec<(f64, f64)> {
        match &l.shapes[0] {
            Shape::Path(p) => p.nodes.iter().map(|n| (n.x, n.y)).collect(),
            _ => vec![],
        }
    }

    #[test]
    fn a_cap_replaces_the_top_edge() {
        let mut f = sample_font();
        host(&mut f);
        // A pointed cap: from (0,0) up to (50,40) and down to (100,0).
        add_glyph(
            &mut f,
            "_cap.point",
            Path {
                closed: false,
                nodes: vec![
                    Node::new(0.0, 0.0, NodeKind::Line),
                    Node::new(50.0, 40.0, NodeKind::Line),
                    Node::new(100.0, 0.0, NodeKind::Line),
                ],
                locked: false,
                stroke: None,
                mask: false,
            },
            vec![],
        );
        // The top edge runs from node 2 (100,300) to node 3 (0,300).
        let f = add_part(
            &f,
            "H",
            "m01",
            PathPart::new(PartKind::Cap, "_cap.point", 0, 2),
        )
        .unwrap();
        let drawn = apply(&f, f.glyph("H").unwrap().layer("m01").unwrap());
        let pts = nodes(&drawn);
        assert_eq!(pts.len(), 5);
        assert!(
            pts.contains(&(50.0, 340.0)),
            "the point sticks out above the stem: {pts:?}"
        );
        assert!(drawn.parts.is_empty());
    }

    #[test]
    fn a_segment_follows_its_edge() {
        let mut f = sample_font();
        host(&mut f);
        add_glyph(
            &mut f,
            "_segment.notch",
            Path {
                closed: false,
                nodes: vec![
                    Node::new(0.0, 0.0, NodeKind::Line),
                    Node::new(40.0, 0.0, NodeKind::Line),
                    Node::new(50.0, -10.0, NodeKind::Line),
                    Node::new(60.0, 0.0, NodeKind::Line),
                    Node::new(100.0, 0.0, NodeKind::Line),
                ],
                locked: false,
                stroke: None,
                mask: false,
            },
            vec![],
        );
        // The right edge: node 1 (100,0) to node 2 (100,300). Down in the glyph is
        // into the outline, so the notch cuts in (to the left).
        let f = add_part(
            &f,
            "H",
            "m01",
            PathPart::new(PartKind::Segment, "_segment.notch", 0, 1),
        )
        .unwrap();
        let pts = nodes(&apply(&f, f.glyph("H").unwrap().layer("m01").unwrap()));
        assert_eq!(pts.len(), 7);
        assert!(pts.contains(&(90.0, 150.0)), "{pts:?}");
    }

    #[test]
    fn a_corner_component_is_inserted_at_its_node() {
        let mut f = sample_font();
        host(&mut f);
        // A small cut corner: from 20 up the y axis to 20 along the x axis.
        add_glyph(
            &mut f,
            "_corner.cut",
            Path {
                closed: false,
                nodes: vec![
                    Node::new(0.0, 20.0, NodeKind::Line),
                    Node::new(-20.0, 0.0, NodeKind::Line),
                ],
                locked: false,
                stroke: None,
                mask: false,
            },
            vec![],
        );
        let f = add_part(
            &f,
            "H",
            "m01",
            PathPart::new(PartKind::Corner, "_corner.cut", 0, 1),
        )
        .unwrap();
        let pts = nodes(&apply(&f, f.glyph("H").unwrap().layer("m01").unwrap()));
        assert_eq!(pts.len(), 5, "{pts:?}");
        assert!(
            !pts.contains(&(100.0, 0.0)),
            "the corner is cut away: {pts:?}"
        );
        // Both new nodes lie on the rectangle's edges, 20 units from the corner.
        assert!(
            pts.contains(&(80.0, 0.0)) && pts.contains(&(100.0, 20.0)),
            "{pts:?}"
        );
    }

    #[test]
    fn parts_are_checked_and_follow_the_masters() {
        let mut f = sample_font();
        host(&mut f);
        assert!(add_part(
            &f,
            "H",
            "m01",
            PathPart::new(PartKind::Cap, "_cap.none", 0, 2)
        )
        .is_err());
        add_glyph(&mut f, "_cap.flat", rect(0.0, 0.0, 10.0, 10.0), vec![]);
        assert!(add_part(
            &f,
            "H",
            "m01",
            PathPart::new(PartKind::Cap, "_cap.flat", 0, 9)
        )
        .is_err());
        let f = add_part(
            &f,
            "H",
            "m01",
            PathPart::new(PartKind::Cap, "_cap.flat", 0, 2),
        )
        .unwrap();
        let mut p = f.glyph("H").unwrap().layer("m01").unwrap().parts[0].clone();
        p.fit = true;
        let f2 = set_part(&f, "H", "m01", 0, Some(p)).unwrap();
        assert!(f2.glyph("H").unwrap().layer("m01").unwrap().parts[0].fit);
        let f3 = set_part(&f2, "H", "m01", 0, None).unwrap();
        assert!(f3
            .glyph("H")
            .unwrap()
            .layer("m01")
            .unwrap()
            .parts
            .is_empty());
        let f4 = decompose_parts(&f2, "H").unwrap();
        assert!(f4
            .glyph("H")
            .unwrap()
            .layers
            .iter()
            .all(|l| l.parts.is_empty()));
        assert!(f4.validate().is_ok());
    }
}
