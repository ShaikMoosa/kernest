//! Importing outlines from SVG: a file per glyph, or SVG text pasted from a vector
//! editor (Figma "Copy as SVG", Illustrator).
//!
//! SVG is parsed with `usvg` and its filled paths become Kernest contours. SVG space
//! is y-down; font space is y-up, so the y axis is flipped. The placement choice sets
//! the scale and offset applied on top of that flip:
//!
//! - [`Placement::FitCapHeight`] scales the outline so its height matches the master's
//!   cap height and sits its bottom-left on the baseline at the origin.
//! - [`Placement::KeepSizeAtOrigin`] keeps the SVG's size (one SVG unit = one font
//!   unit) and moves its bottom-left to the origin.
//! - [`Placement::KeepAsIs`] keeps the SVG's own coordinates, flipped only.
//!
//! Strokes, text and images can't become outlines here; they are left out and listed
//! in the returned warnings.

use kernest_core::{Node, NodeKind, Path as KPath, Shape};
use usvg::tiny_skia_path::{PathSegment, Point};

/// Where imported SVG outlines land in font space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    FitCapHeight,
    KeepSizeAtOrigin,
    KeepAsIs,
}

impl Placement {
    /// Parses the placement name sent by the frontend.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fit-cap-height" => Some(Self::FitCapHeight),
            "keep-size" => Some(Self::KeepSizeAtOrigin),
            "keep-as-is" => Some(Self::KeepAsIs),
            _ => None,
        }
    }
}

/// One segment of an SVG contour: its off-curve controls, end point and kind, in
/// SVG canvas coordinates (y-down).
#[derive(Clone)]
struct Seg {
    offs: Vec<Point>,
    end: Point,
    kind: NodeKind,
}

/// A whole contour: its start point and the segments leaving it, in order.
struct Contour {
    start: Point,
    segs: Vec<Seg>,
}

/// The combined tight bounds of the filled paths, as `[left, top, right, bottom]`.
type Bounds = [f32; 4];

/// Parses SVG text into Kernest outlines placed in font units.
///
/// `cap_height` is the target master's cap height, used by [`Placement::FitCapHeight`].
/// Returns the shapes plus warnings about anything that couldn't be brought over.
pub fn svg_shapes(
    svg: &str,
    placement: Placement,
    cap_height: f64,
) -> Result<(Vec<Shape>, Vec<String>), String> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
        .map_err(|e| format!("the SVG couldn't be read: {e}"))?;

    let mut contours: Vec<Contour> = Vec::new();
    let mut bounds: Option<Bounds> = None;
    let mut stroked = 0usize;
    let text = text_elements(svg);
    let mut images = 0usize;
    walk(
        tree.root(),
        &mut contours,
        &mut bounds,
        &mut stroked,
        &mut images,
    );

    let Some(b) = bounds else {
        return Err("the SVG has no filled outlines to import.".into());
    };
    let mut warnings = Vec::new();
    if stroked > 0 {
        warnings.push(format!(
            "{stroked} stroked path{} left out; convert strokes to outlines in your vector editor first.",
            crate::plural(stroked)
        ));
    }
    if text > 0 {
        warnings
            .push("Text was left out; convert it to outlines in your vector editor first.".into());
    }
    if images > 0 {
        warnings.push("Images were left out; only vector outlines can be imported.".into());
    }

    let (s, dx, dy) = placement_transform(placement, b, cap_height, &mut warnings);
    let mut shapes = Vec::new();
    for c in &contours {
        if let Some(path) = contour_path(c, s, dx, dy) {
            shapes.push(Shape::Path(path));
        }
    }
    if shapes.is_empty() {
        return Err("the SVG has no filled outlines to import.".into());
    }
    Ok((shapes, warnings))
}

/// The scale and offset mapping SVG canvas coordinates to font space:
/// `font_x = s*x + dx`, `font_y = -s*y + dy`.
fn placement_transform(
    placement: Placement,
    b: Bounds,
    cap_height: f64,
    warnings: &mut Vec<String>,
) -> (f64, f64, f64) {
    let (l, t, bo) = (b[0] as f64, b[1] as f64, b[3] as f64);
    match placement {
        Placement::KeepAsIs => (1.0, 0.0, 0.0),
        Placement::KeepSizeAtOrigin => (1.0, -l, bo),
        Placement::FitCapHeight => {
            let h = bo - t;
            let cap = if cap_height > 0.0 { cap_height } else { 700.0 };
            if h <= 1e-6 {
                warnings.push(
                    "The SVG has no height, so its size was kept instead of fitting the cap height."
                        .into(),
                );
                return (1.0, -l, bo);
            }
            let s = cap / h;
            (s, -s * l, s * bo)
        }
    }
}

/// Counts `<text>` elements in the SVG source, skipping comments. usvg is given no
/// fonts, so it drops text while parsing and the tree never shows it; the source is
/// the only place left to notice it was there.
fn text_elements(svg: &str) -> usize {
    let b = svg.as_bytes();
    let mut i = 0;
    let mut n = 0;
    while i < b.len() {
        if b[i..].starts_with(b"<!--") {
            match svg[i + 4..].find("-->") {
                Some(end) => {
                    i += 4 + end + 3;
                    continue;
                }
                None => break,
            }
        }
        if b[i..].starts_with(b"<text")
            && b.get(i + 5)
                .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>' || *c == b'/')
        {
            n += 1;
        }
        i += 1;
    }
    n
}

/// Collects filled paths from a usvg group, recursing into sub-groups.
fn walk(
    group: &usvg::Group,
    contours: &mut Vec<Contour>,
    bounds: &mut Option<Bounds>,
    stroked: &mut usize,
    images: &mut usize,
) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => walk(g, contours, bounds, stroked, images),
            usvg::Node::Path(p) => {
                if p.fill().is_none() {
                    if p.stroke().is_some() {
                        *stroked += 1;
                    }
                    continue;
                }
                if let Some(bb) = p.abs_bounding_box() {
                    let e = [bb.left(), bb.top(), bb.right(), bb.bottom()];
                    *bounds = Some(match *bounds {
                        Some(a) => [
                            a[0].min(e[0]),
                            a[1].min(e[1]),
                            a[2].max(e[2]),
                            a[3].max(e[3]),
                        ],
                        None => e,
                    });
                }
                let t = p.abs_transform();
                let mut current: Option<Contour> = None;
                for seg in p.data().segments() {
                    match seg {
                        PathSegment::MoveTo(pt) => {
                            if let Some(c) = current.take() {
                                contours.push(c);
                            }
                            current = Some(Contour {
                                start: map(t, pt),
                                segs: Vec::new(),
                            });
                        }
                        PathSegment::LineTo(pt) => {
                            push_seg(&mut current, &[], map(t, pt), NodeKind::Line)
                        }
                        PathSegment::QuadTo(c0, pt) => {
                            push_seg(&mut current, &[map(t, c0)], map(t, pt), NodeKind::QCurve)
                        }
                        PathSegment::CubicTo(c0, c1, pt) => push_seg(
                            &mut current,
                            &[map(t, c0), map(t, c1)],
                            map(t, pt),
                            NodeKind::Curve,
                        ),
                        PathSegment::Close => {
                            if let Some(c) = current.take() {
                                contours.push(c);
                            }
                        }
                    }
                }
                if let Some(c) = current.take() {
                    contours.push(c);
                }
            }
            usvg::Node::Text(_) => {}
            usvg::Node::Image(_) => *images += 1,
        }
    }
}

fn map(t: usvg::Transform, p: Point) -> Point {
    let mut p = p;
    t.map_point(&mut p);
    p
}

fn push_seg(current: &mut Option<Contour>, offs: &[Point], end: Point, kind: NodeKind) {
    if let Some(c) = current {
        c.segs.push(Seg {
            offs: offs.to_vec(),
            end,
            kind,
        });
    }
}

/// Builds a closed Kernest contour from an SVG contour, applying the placement
/// transform. The start node's kind describes the segment closing back into it, and
/// smooth flags are set where the handles on either side of a node are collinear.
fn contour_path(c: &Contour, s: f64, dx: f64, dy: f64) -> Option<KPath> {
    let fx = |p: Point| s * p.x as f64 + dx;
    let fy = |p: Point| -s * p.y as f64 + dy;

    // The closing segment is the last one if it returns to the start; otherwise the
    // contour is closed with a straight line back to the start.
    let mut segs = c.segs.clone();
    let (closing_offs, closing_kind) = match segs.last() {
        Some(last) if near(last.end, c.start) => {
            let last = segs.pop().unwrap();
            (last.offs, last.kind)
        }
        _ => (Vec::new(), NodeKind::Line),
    };

    let mut nodes = vec![Node::new(fx(c.start), fy(c.start), closing_kind)];
    for seg in &segs {
        for o in &seg.offs {
            nodes.push(Node::new(fx(*o), fy(*o), NodeKind::OffCurve));
        }
        nodes.push(Node::new(fx(seg.end), fy(seg.end), seg.kind));
    }
    for o in &closing_offs {
        nodes.push(Node::new(fx(*o), fy(*o), NodeKind::OffCurve));
    }

    // A contour needs at least two nodes to enclose anything.
    if nodes.len() < 2 {
        return None;
    }
    set_smooth(&mut nodes);
    Some(KPath {
        locked: false,
        stroke: None,
        mask: false,
        closed: true,
        nodes,
    })
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
}

/// Marks an on-curve node smooth when its incoming and outgoing handles lie on one
/// straight line through it, with the node between them.
fn set_smooth(nodes: &mut [Node]) {
    let n = nodes.len();
    for i in 0..n {
        if !nodes[i].on_curve() {
            continue;
        }
        let prev = nodes[(i + n - 1) % n];
        let next = nodes[(i + 1) % n];
        if prev.on_curve() || next.on_curve() {
            continue;
        }
        let (ax, ay) = (nodes[i].x - prev.x, nodes[i].y - prev.y);
        let (bx, by) = (next.x - nodes[i].x, next.y - nodes[i].y);
        let cross = ax * by - ay * bx;
        let dot = ax * bx + ay * by;
        let scale = (ax * ax + ay * ay).sqrt() * (bx * bx + by * by).sqrt();
        if scale > 1e-9 && cross.abs() < 1e-6 * scale && dot > 0.0 {
            nodes[i].smooth = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::{geometry, Font, Layer};

    fn bounds_of(shapes: Vec<Shape>) -> geometry::Rect {
        let f = Font::new("Test", Vec::<u32>::new());
        let mut l = Layer::new("m01", 1000.0);
        l.shapes = shapes;
        geometry::bounds(&f, &l).unwrap()
    }

    // A 100x200 rectangle in SVG space (y-down), top-left at (10, 20).
    const RECT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 300 300">
        <path d="M10 20 H110 V220 H10 Z" fill="#000"/>
    </svg>"##;

    #[test]
    fn fit_cap_height_scales_to_cap_and_sits_on_baseline() {
        let (shapes, w) = svg_shapes(RECT, Placement::FitCapHeight, 700.0).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let b = bounds_of(shapes);
        // Height 200 scaled to 700 (x3.5); width 100 -> 350; bottom-left at origin.
        assert!(b.min_x.abs() < 0.01, "{b:?}");
        assert!(b.min_y.abs() < 0.01, "{b:?}");
        assert!((b.max_y - 700.0).abs() < 0.01, "{b:?}");
        assert!((b.max_x - 350.0).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn keep_size_at_origin_preserves_size() {
        let (shapes, _) = svg_shapes(RECT, Placement::KeepSizeAtOrigin, 700.0).unwrap();
        let b = bounds_of(shapes);
        assert!(b.min_x.abs() < 0.01 && b.min_y.abs() < 0.01, "{b:?}");
        assert!((b.max_x - 100.0).abs() < 0.01, "{b:?}");
        assert!((b.max_y - 200.0).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn keep_as_is_flips_only() {
        let (shapes, _) = svg_shapes(RECT, Placement::KeepAsIs, 700.0).unwrap();
        let b = bounds_of(shapes);
        // SVG x stays; y is negated: top 20 -> -20, bottom 220 -> -220.
        assert!((b.min_x - 10.0).abs() < 0.01, "{b:?}");
        assert!((b.max_x - 110.0).abs() < 0.01, "{b:?}");
        assert!((b.min_y + 220.0).abs() < 0.01, "{b:?}");
        assert!((b.max_y + 20.0).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn cubic_contour_keeps_curve_nodes() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <path d="M50 0 C77 0 100 22 100 50 C100 77 77 100 50 100 C22 100 0 77 0 50 C0 22 22 0 50 0 Z" fill="#000"/>
        </svg>"##;
        let (shapes, _) = svg_shapes(svg, Placement::KeepSizeAtOrigin, 700.0).unwrap();
        assert_eq!(shapes.len(), 1);
        let Shape::Path(p) = &shapes[0] else {
            panic!("path")
        };
        assert!(p.closed);
        // Four on-curve nodes, each a curve, with two off-curves between them.
        let on: Vec<_> = p.nodes.iter().filter(|n| n.on_curve()).collect();
        assert_eq!(on.len(), 4);
        assert!(on.iter().all(|n| n.kind == NodeKind::Curve));
        assert_eq!(p.nodes.iter().filter(|n| !n.on_curve()).count(), 8);
        // A circle's cardinal points are smooth.
        assert!(on.iter().all(|n| n.smooth));
    }

    #[test]
    fn quadratic_contour_keeps_qcurve_nodes() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <path d="M0 0 Q50 100 100 0 Z" fill="#000"/>
        </svg>"##;
        let (shapes, _) = svg_shapes(svg, Placement::KeepAsIs, 700.0).unwrap();
        let Shape::Path(p) = &shapes[0] else {
            panic!("path")
        };
        assert!(p.nodes.iter().any(|n| n.kind == NodeKind::QCurve));
        assert!(p.nodes.iter().any(|n| n.kind == NodeKind::OffCurve));
    }

    #[test]
    fn stroke_only_paths_are_left_out_with_a_warning() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <path d="M10 10 H90" stroke="#000" fill="none"/>
            <path d="M0 0 H50 V50 H0 Z" fill="#000"/>
        </svg>"##;
        let (shapes, w) = svg_shapes(svg, Placement::KeepAsIs, 700.0).unwrap();
        assert_eq!(shapes.len(), 1);
        assert!(w.iter().any(|x| x.contains("stroked")), "{w:?}");
    }

    #[test]
    fn text_is_left_out_with_a_warning() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <text x="0" y="50">Hi</text>
            <path d="M0 0 H50 V50 H0 Z" fill="#000"/>
        </svg>"##;
        let (shapes, w) = svg_shapes(svg, Placement::KeepAsIs, 700.0).unwrap();
        assert_eq!(shapes.len(), 1);
        assert!(w.iter().any(|x| x.contains("Text was left out")), "{w:?}");
    }

    #[test]
    fn text_in_a_comment_does_not_warn() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <!-- <text x="0" y="50">Hi</text> -->
            <path d="M0 0 H50 V50 H0 Z" fill="#000"/>
        </svg>"##;
        let (_, w) = svg_shapes(svg, Placement::KeepAsIs, 700.0).unwrap();
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn empty_svg_is_an_error() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"></svg>"##;
        assert!(svg_shapes(svg, Placement::KeepAsIs, 700.0).is_err());
    }

    #[test]
    fn fit_cap_height_on_a_flat_svg_keeps_size() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <path d="M0 50 H100 V50 Z" fill="#000"/>
        </svg>"##;
        let (shapes, w) = svg_shapes(svg, Placement::FitCapHeight, 700.0).unwrap();
        assert!(!shapes.is_empty());
        assert!(w.iter().any(|x| x.contains("no height")), "{w:?}");
    }
}
