//! Boolean operations on paths: Union, Subtract and Intersect (Glyphs Handbook 4.4.6).
//!
//! - Union merges the overlaps of the selected paths (every path when none is
//!   selected).
//! - Subtract removes the selected paths from the unselected ones.
//! - Intersect keeps only where the selected paths overlap the unselected ones.
//!
//! For Subtract and Intersect, both sides are merged first, and with no selection the
//! frontmost path (the last one) is the selected side. Open, locked and stroked paths
//! take no part and are left as they are; components too. Results follow Kernest's
//! direction convention.

use kurbo::{BezPath, Shape as _};
use linesweeper::{binary_op, BinaryOp, FillRule};

use crate::model::{Layer, Path, Shape};
use crate::overlap::{from_bez, to_bez};
use crate::pathops::correct_direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Union,
    Subtract,
    Intersect,
}

/// Whether a path can take part: closed, editable and filled.
fn usable(p: &Path) -> bool {
    p.closed && !p.locked && p.stroke.is_none() && p.nodes.len() >= 2
}

/// Indices (among paths) of the picked paths that are open, so can't take part.
pub fn open_picks(layer: &Layer, picked: Option<&[usize]>) -> usize {
    let Some(picked) = picked else { return 0 };
    layer
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .enumerate()
        .filter(|(i, p)| picked.contains(i) && !p.closed)
        .count()
}

fn merged(paths: &[&Path]) -> BezPath {
    let mut all = BezPath::new();
    for p in paths {
        if let Some(b) = to_bez(p) {
            all.extend(b.elements().iter().copied());
        }
    }
    all
}

/// Runs `op` on the layer's paths. `picked` are path indices (paths counted without
/// components); `None` means nothing is selected. Returns whether the layer changed,
/// or an error when there is nothing the operation can work on.
pub fn apply(layer: &mut Layer, op: Op, picked: Option<&[usize]>) -> Result<bool, String> {
    // (shape index, path) of every path that can take part.
    let mut usable_paths: Vec<(usize, usize, &Path)> = Vec::new(); // (shape, path index, path)
    let mut pi = 0;
    for (si, s) in layer.shapes.iter().enumerate() {
        if let Shape::Path(p) = s {
            if usable(p) {
                usable_paths.push((si, pi, p));
            }
            pi += 1;
        }
    }
    let selected: Vec<usize> = match picked {
        Some(list) => usable_paths
            .iter()
            .enumerate()
            .filter(|(_, (_, pi, _))| list.contains(pi))
            .map(|(k, _)| k)
            .collect(),
        None => match op {
            Op::Union => (0..usable_paths.len()).collect(),
            // The frontmost path is the one taken away or intersected with.
            _ => usable_paths.len().checked_sub(1).into_iter().collect(),
        },
    };
    if selected.is_empty() {
        return Err("Select closed paths to combine.".into());
    }
    let boxes: Vec<Option<kurbo::Rect>> = usable_paths
        .iter()
        .map(|(_, _, p)| to_bez(p).map(|b| b.bounding_box().inflate(0.5, 0.5)))
        .collect();
    let touches = |i: usize, j: usize| match (boxes[i], boxes[j]) {
        (Some(a), Some(b)) => a.intersect(b).area() > 0.0,
        _ => false,
    };
    let (a_side, b_side): (Vec<usize>, Vec<usize>) = match op {
        // With no selection, only paths that touch another one: the rest stay as drawn.
        Op::Union if picked.is_none() => (
            (0..usable_paths.len())
                .filter(|&i| (0..usable_paths.len()).any(|j| j != i && touches(i, j)))
                .collect(),
            Vec::new(),
        ),
        Op::Union => (selected.clone(), Vec::new()),
        // Subtract and Intersect only change the unselected paths the selected ones
        // reach; paths elsewhere in the glyph stay as drawn.
        _ => (
            (0..usable_paths.len())
                .filter(|k| !selected.contains(k) && selected.iter().any(|&j| touches(*k, j)))
                .collect(),
            selected.clone(),
        ),
    };
    if a_side.is_empty() && op == Op::Union {
        return Ok(false);
    }
    if op != Op::Union && a_side.is_empty() {
        return if usable_paths.len() > selected.len() {
            Ok(false) // nothing under the selected paths
        } else {
            Err("Subtract and Intersect need another closed path to work on.".into())
        };
    }
    let a: Vec<&Path> = a_side.iter().map(|&k| usable_paths[k].2).collect();
    let b: Vec<&Path> = b_side.iter().map(|&k| usable_paths[k].2).collect();
    let (set_a, set_b) = (merged(&a), merged(&b));
    let bop = match op {
        Op::Union => BinaryOp::Union,
        Op::Subtract => BinaryOp::Difference,
        Op::Intersect => BinaryOp::Intersection,
    };
    let result = binary_op(&set_a, &set_b, FillRule::NonZero, bop)
        .map_err(|e| format!("The paths couldn't be combined: {e:?}"))?;
    let contours: Vec<BezPath> = result.contours().map(|c| c.path.clone()).collect();
    if op == Op::Union {
        // Same number of contours and the same filled area: nothing overlapped.
        let before: f64 = a
            .iter()
            .filter_map(|p| to_bez(p))
            .map(|b| b.area())
            .sum::<f64>()
            .abs();
        let after: f64 = contours.iter().map(|b| b.area()).sum::<f64>().abs();
        if contours.len() == a.len() && (before - after).abs() <= (before * 1e-4).max(1.0) {
            return Ok(false);
        }
    }
    let out: Vec<Path> = contours.iter().filter_map(|b| from_bez(b, true)).collect();

    // Every path that took part is replaced by the result, placed where the first of
    // them was.
    let involved: Vec<usize> = a_side
        .iter()
        .chain(b_side.iter())
        .map(|&k| usable_paths[k].0)
        .collect();
    let first = *involved.iter().min().expect("at least one path");
    let mut shapes = Vec::with_capacity(layer.shapes.len() + out.len());
    for (si, s) in layer.shapes.iter().enumerate() {
        if si == first {
            shapes.extend(out.iter().cloned().map(Shape::Path));
        }
        if !involved.contains(&si) {
            shapes.push(s.clone());
        }
    }
    if shapes == layer.shapes {
        return Ok(false);
    }
    layer.shapes = shapes;
    correct_direction(layer);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, NodeKind};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
        Shape::Path(Path {
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
        })
    }

    /// A circle of four cubic arcs.
    fn circle(cx: f64, cy: f64, r: f64) -> Shape {
        let k = r * 0.5523;
        let mut nodes = Vec::new();
        for (x, y, h1, h2) in [
            (cx + r, cy, (cx + r, cy - k), (cx + k, cy - r)),
            (cx, cy - r, (cx - k, cy - r), (cx - r, cy - k)),
            (cx - r, cy, (cx - r, cy + k), (cx - k, cy + r)),
            (cx, cy + r, (cx + k, cy + r), (cx + r, cy + k)),
        ] {
            let mut n = Node::new(x, y, NodeKind::Curve);
            n.smooth = true;
            nodes.push(n);
            nodes.push(Node::new(h1.0, h1.1, NodeKind::OffCurve));
            nodes.push(Node::new(h2.0, h2.1, NodeKind::OffCurve));
        }
        Shape::Path(Path {
            closed: true,
            nodes,
            locked: false,
            stroke: None,
            mask: false,
        })
    }

    fn paths(layer: &Layer) -> Vec<&Path> {
        layer
            .shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Path(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    fn area(layer: &Layer) -> f64 {
        paths(layer)
            .iter()
            .filter_map(|p| to_bez(p))
            .map(|b| b.area())
            .sum::<f64>()
            .abs()
    }

    fn layer(shapes: Vec<Shape>) -> Layer {
        let mut l = Layer::new("m01", 600.0);
        l.shapes = shapes;
        l
    }

    #[test]
    fn union_of_two_squares_is_one_contour() {
        let mut l = layer(vec![
            rect(0.0, 0.0, 100.0, 100.0),
            rect(50.0, 50.0, 150.0, 150.0),
        ]);
        assert_eq!(apply(&mut l, Op::Union, None), Ok(true));
        let ps = paths(&l);
        assert_eq!(ps.len(), 1);
        assert!(ps[0].nodes.len() <= 8, "{}", ps[0].nodes.len());
        assert!((area(&l) - 17500.0).abs() < 1.0, "{}", area(&l));
    }

    #[test]
    fn subtracting_a_circle_leaves_a_counter() {
        let mut l = layer(vec![
            rect(0.0, 0.0, 400.0, 400.0),
            circle(200.0, 200.0, 100.0),
        ]);
        // Nothing selected: the frontmost path (the circle) is taken away.
        assert_eq!(apply(&mut l, Op::Subtract, None), Ok(true));
        let ps = paths(&l);
        assert_eq!(ps.len(), 2, "square plus a counter");
        // The counter runs the other way from the outline.
        let signed: Vec<f64> = ps.iter().map(|p| to_bez(p).unwrap().area()).collect();
        assert!(signed[0].signum() != signed[1].signum(), "{signed:?}");
        let hole = std::f64::consts::PI * 100.0 * 100.0;
        assert!((area(&l) - (160000.0 - hole)).abs() < 300.0, "{}", area(&l));
        // The counter keeps curves.
        assert!(ps
            .iter()
            .any(|p| p.nodes.iter().any(|n| n.kind == NodeKind::OffCurve)));
    }

    #[test]
    fn intersect_keeps_the_overlap_and_selection_picks_the_sides() {
        let mut l = layer(vec![
            rect(0.0, 0.0, 100.0, 100.0),
            rect(50.0, 50.0, 150.0, 150.0),
            rect(400.0, 0.0, 500.0, 100.0),
        ]);
        // The second square, against the first; the third is out of reach and stays.
        assert_eq!(apply(&mut l, Op::Intersect, Some(&[1])), Ok(true));
        let ps = paths(&l);
        assert_eq!(ps.len(), 2);
        assert!((area(&l) - 2500.0 - 10000.0).abs() < 1.0);
        // Union of just the first two leaves the third alone.
        let mut l = layer(vec![
            rect(0.0, 0.0, 100.0, 100.0),
            rect(50.0, 50.0, 150.0, 150.0),
            rect(400.0, 0.0, 500.0, 100.0),
        ]);
        assert_eq!(apply(&mut l, Op::Union, Some(&[0, 1])), Ok(true));
        assert_eq!(paths(&l).len(), 2);
    }

    #[test]
    fn paths_out_of_reach_stay_as_drawn() {
        let far = rect(800.0, 0.0, 900.0, 100.0);
        let mut l = layer(vec![
            far.clone(),
            rect(0.0, 0.0, 400.0, 400.0),
            circle(200.0, 200.0, 100.0),
        ]);
        assert_eq!(apply(&mut l, Op::Subtract, None), Ok(true));
        assert!(l.shapes.contains(&far), "the far square wasn't rewritten");
        assert_eq!(paths(&l).len(), 3);
        let mut l = layer(vec![
            far.clone(),
            rect(0.0, 0.0, 100.0, 100.0),
            rect(50.0, 50.0, 150.0, 150.0),
        ]);
        assert_eq!(apply(&mut l, Op::Union, None), Ok(true));
        assert!(l.shapes.contains(&far));
        assert_eq!(paths(&l).len(), 2);
        let mut l = layer(vec![
            far.clone(),
            rect(0.0, 0.0, 100.0, 100.0),
            rect(50.0, 50.0, 150.0, 150.0),
        ]);
        assert_eq!(apply(&mut l, Op::Intersect, None), Ok(true));
        assert!(l.shapes.contains(&far));
        assert_eq!(paths(&l).len(), 2);
        assert!((area(&l) - 10000.0 - 2500.0).abs() < 1.0);
    }

    #[test]
    fn open_paths_are_left_out() {
        let mut open = rect(0.0, 0.0, 100.0, 100.0);
        if let Shape::Path(p) = &mut open {
            p.closed = false;
        }
        let mut l = layer(vec![open.clone(), rect(500.0, 0.0, 600.0, 100.0)]);
        assert_eq!(open_picks(&l, Some(&[0])), 1);
        assert!(apply(&mut l, Op::Union, Some(&[0])).is_err());
        assert_eq!(l.shapes[0], open, "unchanged");
        // Subtract with only one closed path has nothing to work on.
        assert!(apply(&mut l, Op::Subtract, None).is_err());
        // Union with nothing overlapping changes nothing.
        let mut l = layer(vec![
            rect(0.0, 0.0, 100.0, 100.0),
            rect(500.0, 0.0, 600.0, 100.0),
        ]);
        assert_eq!(apply(&mut l, Op::Union, None), Ok(false));
    }
}
