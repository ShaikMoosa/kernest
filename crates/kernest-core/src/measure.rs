//! The measure tool: where a dragged line crosses the outline, and the distances
//! between consecutive crossings.

use kurbo::{CubicBez, Line, PathSeg, Point, QuadBez};
use serde::{Deserialize, Serialize};

use crate::geometry::{for_each_path, segments};
use crate::model::{Font, Layer, Node, NodeKind, IDENTITY};
use crate::transform::map_point;

/// One crossing of the measure line with the outline; `t` is its position along the
/// line from `p0` (0) to `p1` (1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub x: f64,
    pub y: f64,
    pub t: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Measurement {
    /// Crossings in order along the line.
    pub hits: Vec<Hit>,
    /// Distance between each pair of consecutive crossings, in font units.
    pub distances: Vec<f64>,
    /// The line's direction in degrees.
    pub angle: f64,
    /// The line's length in font units.
    pub length: f64,
}

fn pt(t: &[f64; 6], n: &Node) -> Point {
    let (x, y) = map_point(t, n.x, n.y);
    Point::new(x, y)
}

/// The kurbo segments of one outline segment, in the component's transform `t`.
fn segs_of(a: &Node, offs: &[Node], b: &Node, t: &[f64; 6]) -> Vec<PathSeg> {
    let (p0, p1) = (pt(t, a), pt(t, b));
    match offs.len() {
        0 => vec![PathSeg::Line(Line::new(p0, p1))],
        1 => vec![PathSeg::Quad(QuadBez::new(p0, pt(t, &offs[0]), p1))],
        2 if b.kind != NodeKind::QCurve => vec![PathSeg::Cubic(CubicBez::new(
            p0,
            pt(t, &offs[0]),
            pt(t, &offs[1]),
            p1,
        ))],
        _ => {
            // A quadratic chain: implied on-curve points sit midway between controls.
            let mut out = Vec::new();
            let mut prev = p0;
            for i in 0..offs.len() {
                let c = pt(t, &offs[i]);
                let next = if i + 1 < offs.len() {
                    let n = pt(t, &offs[i + 1]);
                    Point::new((c.x + n.x) / 2.0, (c.y + n.y) / 2.0)
                } else {
                    p1
                };
                out.push(PathSeg::Quad(QuadBez::new(prev, c, next)));
                prev = next;
            }
            out
        }
    }
}

/// Crossings of the segment `p0`–`p1` with the layer's outline (components included),
/// ordered along the line, with the gaps between them.
pub fn measure(font: &Font, layer: &Layer, p0: (f64, f64), p1: (f64, f64)) -> Measurement {
    let dx = p1.0 - p0.0;
    let dy = p1.1 - p0.1;
    let length = dx.hypot(dy);
    let angle = dy.atan2(dx).to_degrees();
    if length == 0.0 {
        return Measurement {
            hits: Vec::new(),
            distances: Vec::new(),
            angle,
            length,
        };
    }
    let line = Line::new(Point::new(p0.0, p0.1), Point::new(p1.0, p1.1));
    let mut ts: Vec<f64> = Vec::new();
    for_each_path(font, layer, &IDENTITY, 0, &mut |path, t| {
        for (a, offs, b) in segments(path) {
            for seg in segs_of(&a, &offs, &b, t) {
                for ix in seg.intersect_line(line) {
                    ts.push(ix.line_t.clamp(0.0, 1.0));
                }
            }
        }
    });
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    // Adjacent segments share endpoints; count a crossing once.
    ts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let hits: Vec<Hit> = ts
        .iter()
        .map(|&t| Hit {
            x: p0.0 + dx * t,
            y: p0.1 + dy * t,
            t,
        })
        .collect();
    let distances = hits
        .windows(2)
        .map(|w| (w[1].t - w[0].t) * length)
        .collect();
    Measurement {
        hits,
        distances,
        angle,
        length,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Path, Shape};
    use crate::sample::sample_font;

    fn rect_layer() -> Layer {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![Shape::Path(Path {
            locked: false,
            stroke: None,
            closed: true,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(100.0, 0.0, NodeKind::Line),
                Node::new(100.0, 200.0, NodeKind::Line),
                Node::new(0.0, 200.0, NodeKind::Line),
            ],
        })];
        l
    }

    #[test]
    fn horizontal_line_across_rectangle() {
        let f = sample_font();
        let m = measure(&f, &rect_layer(), (-50.0, 100.0), (150.0, 100.0));
        assert_eq!(m.hits.len(), 2);
        assert!((m.hits[0].x - 0.0).abs() < 1e-9 && (m.hits[1].x - 100.0).abs() < 1e-9);
        assert_eq!(m.distances.len(), 1);
        assert!((m.distances[0] - 100.0).abs() < 1e-9);
        assert!((m.angle - 0.0).abs() < 1e-9);
        assert!((m.length - 200.0).abs() < 1e-9);
    }

    #[test]
    fn vertical_line_across_rectangle() {
        let f = sample_font();
        let m = measure(&f, &rect_layer(), (50.0, -50.0), (50.0, 250.0));
        assert_eq!(m.hits.len(), 2);
        assert!((m.hits[0].y - 0.0).abs() < 1e-9 && (m.hits[1].y - 200.0).abs() < 1e-9);
        assert!((m.distances[0] - 200.0).abs() < 1e-9);
        assert!((m.angle - 90.0).abs() < 1e-9);
    }

    #[test]
    fn line_that_misses_has_no_hits() {
        let f = sample_font();
        let m = measure(&f, &rect_layer(), (-50.0, -50.0), (-10.0, -50.0));
        assert!(m.hits.is_empty() && m.distances.is_empty());
    }

    #[test]
    fn diagonal_line_gaps_are_euclidean() {
        let f = sample_font();
        let m = measure(&f, &rect_layer(), (-50.0, -50.0), (250.0, 250.0));
        assert_eq!(m.hits.len(), 2);
        assert!(
            (m.distances[0] - 141.421356).abs() < 1e-4,
            "{:?}",
            m.distances
        );
        assert!((m.angle - 45.0).abs() < 1e-9);
    }

    #[test]
    fn zero_length_line_is_empty() {
        let f = sample_font();
        let m = measure(&f, &rect_layer(), (10.0, 10.0), (10.0, 10.0));
        assert!(m.hits.is_empty());
        assert_eq!(m.length, 0.0);
    }

    #[test]
    fn components_are_measured_too() {
        let f = sample_font();
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![Shape::component("I", IDENTITY)];
        let m = measure(&f, &l, (0.0, 350.0), (300.0, 350.0));
        assert_eq!(m.hits.len(), 2);
        assert!((m.hits[0].x - 80.0).abs() < 1e-9 && (m.hits[1].x - 170.0).abs() < 1e-9);
        assert!((m.distances[0] - 90.0).abs() < 1e-9);
    }

    #[test]
    fn curves_give_four_hits_across_an_o() {
        let f = sample_font();
        let o = f.glyph("o").unwrap().layer("m01").unwrap();
        let m = measure(&f, o, (-50.0, 250.0), (630.0, 250.0));
        assert_eq!(m.hits.len(), 4, "{:?}", m.hits);
        let xs: Vec<f64> = m.hits.iter().map(|h| h.x).collect();
        for (got, want) in xs.iter().zip([50.0, 135.0, 445.0, 530.0]) {
            assert!((got - want).abs() < 0.5, "{xs:?}");
        }
        assert!((m.distances[1] - 310.0).abs() < 1.0, "{:?}", m.distances);
    }
}
