//! Aligning and distributing the selection (Glyphs Handbook 3.2.8 and 4.4.5).
//!
//! The selection is split into units that move as a whole:
//! - a path whose nodes are all selected (as one rigid shape);
//! - otherwise each selected on-curve node, with its handles;
//! - each selected handle whose node isn't selected;
//! - each selected component and anchor.
//!
//! A unit's box is its outline's exact bounds (paths, components) or its point.
//! Aligning moves each unit so one side or centre of its box meets the selection's
//! box, or a given coordinate (a metric line, the glyph width). Distributing puts
//! equal gaps between three or more units. `align_points` lines up selected points
//! (Path › Align Selection) on whichever axis they are closer to sharing.

use crate::geometry;
use crate::model::{Font, Layer, NodeKind, Shape};
use crate::transform::Selection;

/// What to do with the units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
    DistributeH,
    DistributeV,
}

impl Op {
    pub fn parse(s: &str) -> Result<Op, String> {
        Ok(match s {
            "left" => Op::Left,
            "h-center" => Op::HCenter,
            "right" => Op::Right,
            "top" => Op::Top,
            "v-center" => Op::VCenter,
            "bottom" => Op::Bottom,
            "distribute-h" => Op::DistributeH,
            "distribute-v" => Op::DistributeV,
            other => return Err(format!("Unknown align operation \"{other}\"")),
        })
    }
    fn horizontal(self) -> bool {
        matches!(self, Op::Left | Op::HCenter | Op::Right | Op::DistributeH)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Unit {
    /// A whole path (index among paths).
    Path(usize),
    /// A node (path, node) and the handles that move with it.
    Node(usize, usize, Vec<usize>),
    Component(usize),
    Anchor(String),
}

#[derive(Debug, Clone, Copy)]
struct Box2 {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

fn paths(layer: &Layer) -> Vec<&crate::model::Path> {
    layer
        .shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn units(layer: &Layer, sel: &Selection) -> Vec<Unit> {
    let mut out = Vec::new();
    for (pi, p) in paths(layer).into_iter().enumerate() {
        if p.locked || p.nodes.is_empty() {
            continue;
        }
        let picked: Vec<usize> = (0..p.nodes.len())
            .filter(|&ni| sel.nodes.contains(&(pi, ni)))
            .collect();
        if picked.is_empty() {
            continue;
        }
        if picked.len() == p.nodes.len() {
            out.push(Unit::Path(pi));
            continue;
        }
        let n = p.nodes.len();
        let neighbour = |ni: usize, step: isize| -> Option<usize> {
            let j = ni as isize + step;
            if p.closed {
                Some(j.rem_euclid(n as isize) as usize)
            } else {
                (0..n as isize).contains(&j).then_some(j as usize)
            }
        };
        for &ni in &picked {
            if p.nodes[ni].kind == NodeKind::OffCurve {
                // A handle moves on its own unless its node is picked too.
                let owner = [-1, 1]
                    .iter()
                    .filter_map(|&s| neighbour(ni, s))
                    .find(|&j| p.nodes[j].kind != NodeKind::OffCurve && picked.contains(&j));
                if owner.is_none() {
                    out.push(Unit::Node(pi, ni, Vec::new()));
                }
            } else {
                let handles = [-1, 1]
                    .iter()
                    .filter_map(|&s| neighbour(ni, s))
                    .filter(|&j| p.nodes[j].kind == NodeKind::OffCurve)
                    .collect();
                out.push(Unit::Node(pi, ni, handles));
            }
        }
    }
    let ncomp = layer
        .shapes
        .iter()
        .filter(|s| matches!(s, Shape::Component { .. }))
        .count();
    for &ci in &sel.components {
        if ci < ncomp && !out.contains(&Unit::Component(ci)) {
            out.push(Unit::Component(ci));
        }
    }
    for a in &layer.anchors {
        if sel.anchors.contains(&a.name) {
            out.push(Unit::Anchor(a.name.clone()));
        }
    }
    out
}

/// A layer holding only one shape, for its bounds.
fn only(layer: &Layer, shape: &Shape) -> Layer {
    let mut l = layer.clone();
    l.shapes = vec![shape.clone()];
    l.anchors.clear();
    l
}

fn unit_box(font: &Font, layer: &Layer, u: &Unit) -> Option<Box2> {
    let point = |x: f64, y: f64| Box2 {
        min_x: x,
        min_y: y,
        max_x: x,
        max_y: y,
    };
    match u {
        Unit::Path(pi) => {
            let shape = layer
                .shapes
                .iter()
                .filter(|s| matches!(s, Shape::Path(_)))
                .nth(*pi)?;
            let r = geometry::bounds(font, &only(layer, shape))?;
            Some(Box2 {
                min_x: r.min_x,
                min_y: r.min_y,
                max_x: r.max_x,
                max_y: r.max_y,
            })
        }
        Unit::Node(pi, ni, _) => {
            let n = paths(layer).get(*pi)?.nodes.get(*ni).copied()?;
            Some(point(n.x, n.y))
        }
        Unit::Component(ci) => {
            let shape = layer
                .shapes
                .iter()
                .filter(|s| matches!(s, Shape::Component { .. }))
                .nth(*ci)?;
            let r = geometry::bounds(font, &only(layer, shape))?;
            Some(Box2 {
                min_x: r.min_x,
                min_y: r.min_y,
                max_x: r.max_x,
                max_y: r.max_y,
            })
        }
        Unit::Anchor(name) => {
            let a = layer.anchors.iter().find(|a| &a.name == name)?;
            Some(point(a.x, a.y))
        }
    }
}

fn shift(layer: &mut Layer, u: &Unit, dx: f64, dy: f64) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    match u {
        Unit::Path(pi) => {
            if let Some(Shape::Path(p)) = layer
                .shapes
                .iter_mut()
                .filter(|s| matches!(s, Shape::Path(_)))
                .nth(*pi)
            {
                for n in &mut p.nodes {
                    n.x += dx;
                    n.y += dy;
                }
            }
        }
        Unit::Node(pi, ni, handles) => {
            if let Some(Shape::Path(p)) = layer
                .shapes
                .iter_mut()
                .filter(|s| matches!(s, Shape::Path(_)))
                .nth(*pi)
            {
                for &j in std::iter::once(ni).chain(handles.iter()) {
                    p.nodes[j].x += dx;
                    p.nodes[j].y += dy;
                }
            }
        }
        Unit::Component(ci) => {
            if let Some(Shape::Component {
                transform, align, ..
            }) = layer
                .shapes
                .iter_mut()
                .filter(|s| matches!(s, Shape::Component { .. }))
                .nth(*ci)
            {
                transform[4] += dx;
                transform[5] += dy;
                // Moved by hand: it no longer follows its anchors.
                *align = false;
            }
        }
        Unit::Anchor(name) => {
            if let Some(a) = layer.anchors.iter_mut().find(|a| &a.name == name) {
                a.x += dx;
                a.y += dy;
            }
        }
    }
}

/// Aligns or distributes the selection. `to` replaces the selection's box on the
/// operation's axis (a metric line's y, or 0, half the width or the width for x).
/// Returns how many units moved.
pub fn align(font: &Font, layer: &mut Layer, sel: &Selection, op: Op, to: Option<f64>) -> usize {
    let units = units(layer, sel);
    let boxes: Vec<(Unit, Box2)> = units
        .into_iter()
        .filter_map(|u| unit_box(font, layer, &u).map(|b| (u, b)))
        .collect();
    let needed = if matches!(op, Op::DistributeH | Op::DistributeV) {
        3
    } else if to.is_some() {
        1
    } else {
        2
    };
    if boxes.len() < needed {
        return 0;
    }
    let all = boxes.iter().fold(boxes[0].1, |a, (_, b)| Box2 {
        min_x: a.min_x.min(b.min_x),
        min_y: a.min_y.min(b.min_y),
        max_x: a.max_x.max(b.max_x),
        max_y: a.max_y.max(b.max_y),
    });
    let mut moves: Vec<(Unit, f64, f64)> = Vec::new();
    match op {
        Op::DistributeH | Op::DistributeV => {
            let h = op.horizontal();
            let mut order: Vec<&(Unit, Box2)> = boxes.iter().collect();
            order.sort_by(|a, b| {
                let (a0, a1) = lo_hi(&a.1, h);
                let (b0, b1) = lo_hi(&b.1, h);
                (a0 + a1).total_cmp(&(b0 + b1))
            });
            let (start, end) = lo_hi(&all, h);
            let sizes: f64 = order
                .iter()
                .map(|(_, b)| lo_hi(b, h).1 - lo_hi(b, h).0)
                .sum();
            let gap = (end - start - sizes) / (order.len() - 1) as f64;
            let mut at = start;
            for (u, b) in order {
                let (b0, b1) = lo_hi(b, h);
                let d = at - b0;
                moves.push((u.clone(), if h { d } else { 0.0 }, if h { 0.0 } else { d }));
                at += b1 - b0 + gap;
            }
        }
        _ => {
            let h = op.horizontal();
            let (lo, hi) = lo_hi(&all, h);
            let target = to.unwrap_or(match op {
                Op::Left | Op::Bottom => lo,
                Op::Right | Op::Top => hi,
                _ => (lo + hi) / 2.0,
            });
            for (u, b) in &boxes {
                let (b0, b1) = lo_hi(b, h);
                let edge = match op {
                    Op::Left | Op::Bottom => b0,
                    Op::Right | Op::Top => b1,
                    _ => (b0 + b1) / 2.0,
                };
                let d = target - edge;
                moves.push((u.clone(), if h { d } else { 0.0 }, if h { 0.0 } else { d }));
            }
        }
    }
    let mut moved = 0;
    for (u, dx, dy) in moves {
        if dx.abs() > 1e-9 || dy.abs() > 1e-9 {
            shift(layer, &u, dx, dy);
            moved += 1;
        }
    }
    moved
}

fn lo_hi(b: &Box2, horizontal: bool) -> (f64, f64) {
    if horizontal {
        (b.min_x, b.max_x)
    } else {
        (b.min_y, b.max_y)
    }
}

/// Path › Align Selection: lines the selected points (nodes, handles, anchors) up on
/// a vertical line when they are closer together horizontally, else on a horizontal
/// line. `origin` is the transform origin (0–8, row by row from the top left): its
/// column picks left, centre or right for a vertical line, its row top, middle or
/// bottom for a horizontal one. Whole paths count as their nodes here.
pub fn align_points(layer: &mut Layer, sel: &Selection, origin: usize) -> usize {
    let mut pts: Vec<Unit> = Vec::new();
    for (pi, p) in paths(layer).into_iter().enumerate() {
        if p.locked {
            continue;
        }
        for ni in 0..p.nodes.len() {
            if sel.nodes.contains(&(pi, ni)) {
                pts.push(Unit::Node(pi, ni, Vec::new()));
            }
        }
    }
    for a in &layer.anchors {
        if sel.anchors.contains(&a.name) {
            pts.push(Unit::Anchor(a.name.clone()));
        }
    }
    let at = |l: &Layer, u: &Unit| -> (f64, f64) {
        match u {
            Unit::Node(pi, ni, _) => {
                let n = paths(l)[*pi].nodes[*ni];
                (n.x, n.y)
            }
            Unit::Anchor(name) => {
                let a = l.anchors.iter().find(|a| &a.name == name).expect("anchor");
                (a.x, a.y)
            }
            _ => unreachable!(),
        }
    };
    if pts.len() < 2 {
        return 0;
    }
    let xy: Vec<(f64, f64)> = pts.iter().map(|u| at(layer, u)).collect();
    let (min_x, max_x) = xy
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (min_y, max_y) = xy
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let vertical_line = max_x - min_x <= max_y - min_y;
    let target = if vertical_line {
        [min_x, (min_x + max_x) / 2.0, max_x][origin.min(8) % 3]
    } else {
        [max_y, (min_y + max_y) / 2.0, min_y][origin.min(8) / 3]
    };
    let mut moved = 0;
    for (u, (x, y)) in pts.iter().zip(xy) {
        let (dx, dy) = if vertical_line {
            (target - x, 0.0)
        } else {
            (0.0, target - y)
        };
        if dx.abs() > 1e-9 || dy.abs() > 1e-9 {
            shift(layer, u, dx, dy);
            moved += 1;
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Anchor, Node, Path};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Shape {
        Shape::Path(Path {
            closed: true,
            nodes: vec![
                Node::new(x, y, NodeKind::Line),
                Node::new(x + w, y, NodeKind::Line),
                Node::new(x + w, y + h, NodeKind::Line),
                Node::new(x, y + h, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
            mask: false,
        })
    }

    fn whole(paths: &[usize]) -> Selection {
        Selection {
            nodes: paths
                .iter()
                .flat_map(|&p| (0..4).map(move |n| (p, n)))
                .collect(),
            ..Default::default()
        }
    }

    fn bounds_of(font: &Font, layer: &Layer, pi: usize) -> Box2 {
        unit_box(font, layer, &Unit::Path(pi)).unwrap()
    }

    fn setup() -> (Font, Layer) {
        let font = crate::sample::sample_font();
        let mut layer = Layer::new("m01", 600.0);
        layer.shapes = vec![
            rect(50.0, 0.0, 100.0, 300.0),
            rect(220.0, 80.0, 60.0, 200.0),
            rect(400.0, 40.0, 80.0, 80.0),
        ];
        (font, layer)
    }

    #[test]
    fn whole_paths_align_to_the_selection_box() {
        let (font, mut layer) = setup();
        assert_eq!(align(&font, &mut layer, &whole(&[0, 1]), Op::Left, None), 1);
        assert_eq!(
            bounds_of(&font, &layer, 0).min_x,
            bounds_of(&font, &layer, 1).min_x
        );
        assert_eq!(bounds_of(&font, &layer, 1).min_x, 50.0);
        // Paths keep their shape.
        assert_eq!(bounds_of(&font, &layer, 1).max_x, 110.0);
        assert_eq!(align(&font, &mut layer, &whole(&[0, 2]), Op::Top, None), 1);
        assert_eq!(bounds_of(&font, &layer, 2).max_y, 300.0);
        align(&font, &mut layer, &whole(&[0, 1]), Op::VCenter, None);
        assert_eq!(bounds_of(&font, &layer, 1).min_y, 50.0);
        // The third path wasn't selected and didn't move horizontally.
        assert_eq!(bounds_of(&font, &layer, 2).min_x, 400.0);
    }

    #[test]
    fn distribute_gives_equal_gaps() {
        let (font, mut layer) = setup();
        assert_eq!(
            align(&font, &mut layer, &whole(&[0, 1, 2]), Op::DistributeH, None),
            1
        );
        let b: Vec<Box2> = (0..3).map(|i| bounds_of(&font, &layer, i)).collect();
        let g1 = b[1].min_x - b[0].max_x;
        let g2 = b[2].min_x - b[1].max_x;
        assert!((g1 - g2).abs() < 1e-9, "{g1} {g2}");
        // Three nodes on a line: equal spacing.
        let mut l = Layer::new("m01", 600.0);
        l.shapes = vec![rect(0.0, 0.0, 100.0, 100.0)];
        let Shape::Path(p) = &mut l.shapes[0] else {
            unreachable!()
        };
        p.nodes[1].x = 30.0;
        let sel = Selection {
            nodes: vec![(0, 0), (0, 1), (0, 2)],
            ..Default::default()
        };
        align(&font, &mut l, &sel, Op::DistributeH, None);
        let Shape::Path(p) = &l.shapes[0] else {
            unreachable!()
        };
        assert_eq!(
            (p.nodes[0].x, p.nodes[1].x, p.nodes[2].x),
            (0.0, 50.0, 100.0)
        );
        // Fewer than three units: nothing to distribute.
        assert_eq!(
            align(&font, &mut layer, &whole(&[0, 1]), Op::DistributeV, None),
            0
        );
    }

    #[test]
    fn nodes_align_to_a_metric_line() {
        let (font, mut layer) = setup();
        // Two nodes of the first rectangle (the top ones) and an anchor, to x-height 500.
        layer.anchors.push(Anchor {
            name: "top".into(),
            x: 100.0,
            y: 650.0,
        });
        let sel = Selection {
            nodes: vec![(0, 2), (0, 3)],
            anchors: vec!["top".into()],
            ..Default::default()
        };
        assert_eq!(align(&font, &mut layer, &sel, Op::Top, Some(500.0)), 3);
        let Shape::Path(p) = &layer.shapes[0] else {
            unreachable!()
        };
        assert_eq!((p.nodes[2].y, p.nodes[3].y), (500.0, 500.0));
        assert_eq!(p.nodes[0].y, 0.0, "unselected nodes stay");
        assert_eq!(layer.anchors[0].y, 500.0);
        // A single path to the glyph's width (right edge at 600).
        align(&font, &mut layer, &whole(&[1]), Op::Right, Some(600.0));
        assert_eq!(bounds_of(&font, &layer, 1).max_x, 600.0);
    }

    #[test]
    fn handles_follow_their_node() {
        let font = crate::sample::sample_font();
        let mut layer = Layer::new("m01", 600.0);
        layer.shapes = vec![Shape::Path(Path {
            closed: false,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(0.0, 100.0, NodeKind::OffCurve),
                Node::new(100.0, 200.0, NodeKind::OffCurve),
                Node::new(200.0, 200.0, NodeKind::Curve),
            ],
            locked: false,
            stroke: None,
            mask: false,
        })];
        let sel = Selection {
            nodes: vec![(0, 0), (0, 3)],
            ..Default::default()
        };
        align(&font, &mut layer, &sel, Op::Bottom, None);
        let Shape::Path(p) = &layer.shapes[0] else {
            unreachable!()
        };
        assert_eq!(p.nodes[3].y, 0.0);
        assert_eq!(p.nodes[2].y, 0.0, "the handle moved with its node");
        assert_eq!(p.nodes[1].y, 100.0, "the other node's handle stays");
    }

    #[test]
    fn align_selection_picks_the_closer_axis_and_the_origin() {
        let mut layer = Layer::new("m01", 600.0);
        layer.shapes = vec![rect(0.0, 0.0, 100.0, 300.0)];
        let Shape::Path(p) = &mut layer.shapes[0] else {
            unreachable!()
        };
        p.nodes[1].x = 90.0; // (90, 0) and (100, 300): closer horizontally.
        let sel = Selection {
            nodes: vec![(0, 1), (0, 2)],
            ..Default::default()
        };
        // Origin column right: both at x = 100.
        assert_eq!(align_points(&mut layer, &sel, 5), 1);
        let Shape::Path(p) = &layer.shapes[0] else {
            unreachable!()
        };
        assert_eq!((p.nodes[1].x, p.nodes[2].x), (100.0, 100.0));
        assert_eq!((p.nodes[1].y, p.nodes[2].y), (0.0, 300.0));
        // (0,0) and (100,0)... after moving: bottom pair is closer vertically.
        let sel = Selection {
            nodes: vec![(0, 0), (0, 3)],
            ..Default::default()
        };
        // Nodes (0,0) and (0,300) are on one vertical already; origin centre.
        assert_eq!(align_points(&mut layer, &sel, 4), 0);
    }
}
