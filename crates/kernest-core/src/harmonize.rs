//! Curvature continuity (G2) at smooth nodes: Harmonize and super-smooth nodes
//! (Glyphs Handbook 3.2.5).
//!
//! At a smooth node between two cubic segments, the node sits on the straight line
//! between its two handles. The curvature where a cubic ends is
//! `k = (2/3) · h / a²`, with `a` the handle length and `h` the distance of the
//! segment's other handle from that line. Sliding the node along the line keeps the
//! tangent and both `h`, and only changes the two handle lengths (`a + b` stays the
//! handles' distance `L`). Equal curvature needs `h₁/a² = h₂/b²`, so
//! `a/b = √(h₁/h₂)` and the node goes to `a = L · r / (1 + r)` with `r = √(h₁/h₂)`.
//!
//! Nodes where that isn't possible are left alone: corners, nodes next to a straight
//! line or a quadratic curve, flat segments (`h = 0`) and inflections, where the two
//! sides bend opposite ways and only zero curvature would match.

use crate::model::{Layer, NodeKind, Path, Shape};

/// Squared distances below this count as zero (font units²).
const EPS: f64 = 1e-9;

type P = (f64, f64);

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}
fn cross(a: P, b: P) -> f64 {
    a.0 * b.1 - a.1 * b.0
}
fn len(a: P) -> f64 {
    a.0.hypot(a.1)
}

/// The four points around on-curve node `i` of a path, if it joins two cubic segments
/// and isn't at the open end of a path: the incoming segment's first handle, its
/// second handle, the outgoing segment's first handle and its second handle.
fn cubic_neighbours(path: &Path, i: usize) -> Option<[P; 4]> {
    let n = path.nodes.len();
    let node = path.nodes.get(i)?;
    if node.kind != NodeKind::Curve || n < 5 {
        return None;
    }
    let at = |k: isize| -> Option<usize> {
        let j = i as isize + k;
        if path.closed {
            Some(j.rem_euclid(n as isize) as usize)
        } else if (0..n as isize).contains(&j) {
            Some(j as usize)
        } else {
            None
        }
    };
    let (a0, a1, b1, b0) = (at(-2)?, at(-1)?, at(1)?, at(2)?);
    let off = |j: usize| path.nodes[j].kind == NodeKind::OffCurve;
    // The outgoing segment is cubic when two handles lead to an on-curve curve node.
    let after = at(3)?;
    if !(off(a0) && off(a1) && off(b1) && off(b0)) || path.nodes[after].kind != NodeKind::Curve {
        return None;
    }
    let p = |j: usize| (path.nodes[j].x, path.nodes[j].y);
    Some([p(a0), p(a1), p(b1), p(b0)])
}

/// Where node `i` has the same curvature on both sides, or `None` if it can't (see
/// the module notes).
pub fn g2_position(path: &Path, i: usize) -> Option<(f64, f64)> {
    let [a0, a1, b1, b0] = cubic_neighbours(path, i)?;
    let node = (path.nodes[i].x, path.nodes[i].y);
    let line = sub(b1, a1);
    let l = len(line);
    if l * l < EPS {
        return None;
    }
    // A smooth node lies on the line between its handles (allow rounding).
    if (cross(line, sub(node, a1)) / l).abs() > 0.5 {
        return None;
    }
    let h_in = cross(line, sub(a0, a1)) / l;
    let h_out = cross(line, sub(b0, a1)) / l;
    // Both far handles must be on the same side of the tangent line.
    if h_in.abs() < 1e-6 || h_out.abs() < 1e-6 || h_in.signum() != h_out.signum() {
        return None;
    }
    let r = (h_in / h_out).sqrt();
    let t = r / (1.0 + r);
    Some((a1.0 + line.0 * t, a1.1 + line.1 * t))
}

/// The curvature at the end of the incoming segment and at the start of the outgoing
/// one at node `i` (signed; for tests and the comb).
pub fn curvatures(path: &Path, i: usize) -> Option<(f64, f64)> {
    let [a0, a1, b1, b0] = cubic_neighbours(path, i)?;
    let node = (path.nodes[i].x, path.nodes[i].y);
    let k = |near: P, far: P| {
        let t = sub(near, node);
        let a = len(t);
        (2.0 / 3.0) * cross(t, sub(far, near)) / (a * a * a)
    };
    // The incoming tangent points back along the path; flip it to compare.
    Some((-k(a1, a0), k(b1, b0)))
}

/// Moves each listed on-curve node (all smooth ones when `only` is `None`) to its G2
/// position. Returns how many nodes moved.
pub fn harmonize_path(path: &mut Path, only: Option<&[usize]>) -> usize {
    if path.locked {
        return 0;
    }
    let picks: Vec<usize> = match only {
        Some(list) => list.to_vec(),
        None => (0..path.nodes.len())
            .filter(|&i| path.nodes[i].smooth && path.nodes[i].on_curve())
            .collect(),
    };
    // A node's G2 position depends only on the handles, so the order doesn't matter.
    let mut moved = 0;
    for i in picks {
        if let Some((x, y)) = g2_position(path, i) {
            let n = &mut path.nodes[i];
            if (n.x - x).abs() > 1e-9 || (n.y - y).abs() > 1e-9 {
                n.x = x;
                n.y = y;
                moved += 1;
            }
        }
    }
    moved
}

/// Keeps every super-smooth node of a path at its G2 position (after an edit).
pub fn apply_super_smooth(path: &mut Path) -> usize {
    let g2: Vec<usize> = (0..path.nodes.len())
        .filter(|&i| path.nodes[i].g2)
        .collect();
    if g2.is_empty() || path.locked {
        return 0;
    }
    harmonize_path(path, Some(&g2))
}

fn paths_mut(layer: &mut Layer) -> impl Iterator<Item = &mut Path> {
    layer.shapes.iter_mut().filter_map(|s| match s {
        Shape::Path(p) => Some(p),
        _ => None,
    })
}

/// Harmonize on a layer: the picked nodes (`[path, node]`, paths counted without
/// components), or every smooth node when none are picked. Returns how many moved.
pub fn harmonize(layer: &mut Layer, nodes: &[[usize; 2]]) -> usize {
    let mut moved = 0;
    for (pi, p) in paths_mut(layer).enumerate() {
        if nodes.is_empty() {
            moved += harmonize_path(p, None);
        } else {
            let mine: Vec<usize> = nodes.iter().filter(|n| n[0] == pi).map(|n| n[1]).collect();
            if !mine.is_empty() {
                moved += harmonize_path(p, Some(&mine));
            }
        }
    }
    moved
}

/// Turns the picked on-curve nodes into super-smooth nodes, or back into plain smooth
/// ones when they all already are. Only nodes between two cubic curves can be
/// super-smooth; the others are skipped. Returns how many changed, or an error when
/// none of the picks can be super-smooth.
pub fn toggle_super_smooth(layer: &mut Layer, nodes: &[[usize; 2]]) -> Result<usize, String> {
    let paths: Vec<&mut Path> = paths_mut(layer).collect();
    let mut picks: Vec<(usize, usize)> = Vec::new();
    for &[pi, ni] in nodes {
        let p = paths.get(pi).ok_or("No path at that position.")?;
        let n = p.nodes.get(ni).ok_or("No node at that position.")?;
        if n.on_curve() && !p.locked && cubic_neighbours(p, ni).is_some() {
            picks.push((pi, ni));
        }
    }
    if picks.is_empty() {
        return Err("Super-smooth nodes need a curve on both sides.".into());
    }
    let mut paths = paths;
    let turn_on = !picks.iter().all(|&(pi, ni)| paths[pi].nodes[ni].g2);
    for &(pi, ni) in &picks {
        let n = &mut paths[pi].nodes[ni];
        n.g2 = turn_on;
        n.smooth = true;
    }
    if turn_on {
        for p in paths.iter_mut() {
            apply_super_smooth(p);
        }
    }
    Ok(picks.len())
}

/// Keeps all super-smooth nodes of a layer at their G2 positions.
pub fn apply_layer(layer: &mut Layer) -> usize {
    paths_mut(layer).map(apply_super_smooth).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    fn n(x: f64, y: f64, kind: NodeKind) -> Node {
        Node::new(x, y, kind)
    }

    /// An "S"-free bowl: a closed path with a smooth node at (300, 0) whose two sides
    /// bend unequally.
    fn bowl() -> Path {
        use NodeKind::*;
        let mut smooth = n(300.0, 0.0, Curve);
        smooth.smooth = true;
        Path {
            closed: true,
            nodes: vec![
                n(0.0, 400.0, Curve),
                n(0.0, 150.0, OffCurve),
                n(120.0, 0.0, OffCurve),
                smooth,
                n(400.0, 0.0, OffCurve),
                n(600.0, 250.0, OffCurve),
                n(600.0, 400.0, Curve),
                n(600.0, 700.0, OffCurve),
                n(0.0, 700.0, OffCurve),
            ],
            locked: false,
            stroke: None,
            mask: false,
        }
    }

    #[test]
    fn harmonize_matches_curvature_within_one_percent() {
        let mut p = bowl();
        let (k1, k2) = curvatures(&p, 3).unwrap();
        assert!(
            (k1 - k2).abs() / k1.abs() > 0.2,
            "unequal to start: {k1} {k2}"
        );
        assert_eq!(harmonize_path(&mut p, Some(&[3])), 1);
        let (k1, k2) = curvatures(&p, 3).unwrap();
        assert!((k1 - k2).abs() / k1.abs() < 0.01, "{k1} {k2}");
        // Still on the handle line, between the handles.
        assert_eq!(p.nodes[3].y, 0.0);
        assert!(p.nodes[3].x > 120.0 && p.nodes[3].x < 400.0);
        // Running it again changes nothing.
        assert_eq!(harmonize_path(&mut p, Some(&[3])), 0);
    }

    #[test]
    fn corners_lines_and_inflections_are_left_alone() {
        // A corner: the node is off the handle line.
        let mut p = bowl();
        p.nodes[3].y = 30.0;
        assert_eq!(g2_position(&p, 3), None);
        // An inflection: the far handles on opposite sides.
        let mut p = bowl();
        p.nodes[5].y = -250.0;
        assert_eq!(g2_position(&p, 3), None);
        // A line on one side.
        let mut p = bowl();
        p.nodes.drain(4..6);
        p.nodes[4].kind = NodeKind::Line;
        assert_eq!(g2_position(&p, 3), None);
        // Harmonize on everything only touches smooth nodes.
        let mut p = bowl();
        p.nodes[3].smooth = false;
        assert_eq!(harmonize_path(&mut p, None), 0);
    }

    #[test]
    fn super_smooth_nodes_keep_g2_when_a_handle_moves() {
        let mut layer = Layer::new("m01", 600.0);
        layer.shapes.push(Shape::Path(bowl()));
        assert_eq!(toggle_super_smooth(&mut layer, &[[0, 3]]), Ok(1));
        let Shape::Path(p) = &mut layer.shapes[0] else {
            unreachable!()
        };
        assert!(p.nodes[3].g2 && p.nodes[3].smooth);
        // Move a far handle, as dragging would, and re-apply as an edit does.
        p.nodes[5].y = 120.0;
        assert_eq!(apply_layer(&mut layer), 1);
        let Shape::Path(p) = &layer.shapes[0] else {
            unreachable!()
        };
        let (k1, k2) = curvatures(p, 3).unwrap();
        assert!((k1 - k2).abs() / k1.abs() < 0.01, "{k1} {k2}");
        // Toggling again makes it a plain smooth node.
        assert_eq!(toggle_super_smooth(&mut layer, &[[0, 3]]), Ok(1));
        let Shape::Path(p) = &layer.shapes[0] else {
            unreachable!()
        };
        assert!(!p.nodes[3].g2 && p.nodes[3].smooth);
        // A handle can't be super-smooth, and neither can a node next to a line.
        assert!(toggle_super_smooth(&mut layer, &[[0, 1]]).is_err());
        let mut lined = Layer::new("m01", 600.0);
        let mut p = bowl();
        p.nodes.drain(4..6);
        p.nodes[4].kind = NodeKind::Line;
        lined.shapes.push(Shape::Path(p));
        assert_eq!(
            toggle_super_smooth(&mut lined, &[[0, 3]]),
            Err("Super-smooth nodes need a curve on both sides.".into())
        );
    }

    #[test]
    fn g2_round_trips_in_the_node_text() {
        let mut node = n(300.0, 0.0, NodeKind::Curve);
        node.smooth = true;
        node.g2 = true;
        assert_eq!(node.to_string(), "300 0 curve smooth g2");
        assert_eq!("300 0 curve smooth g2".parse::<Node>().unwrap(), node);
        // g2 alone implies smooth.
        assert_eq!("300 0 curve g2".parse::<Node>().unwrap(), node);
        assert!(!"300 0 curve smooth".parse::<Node>().unwrap().g2);
    }
}
