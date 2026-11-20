//! Interpolation quality: masters that are compatible (same structure) but would morph
//! badly between each other.
//!
//! [`crate::interpolate::check_glyph`] decides whether a glyph *can* be interpolated.
//! This module looks at glyphs that can, and warns when the result is likely to look
//! wrong, the way designers find out too late in a variable font:
//!
//! - a path runs the other way round in one master (it turns inside out halfway);
//! - a closed path starts at a different point (its points slide round the outline);
//! - two similar paths are in a different order (they swap places halfway);
//! - a smooth point is straight in both masters but kinks between them.
//!
//! Direction, start point and order can be fixed automatically ([`fix`]); kinks need
//! the designer.

use crate::interpolate::{check_glyph, default_master};
use crate::model::{Font, Layer, Node, Path, Shape};
use crate::pathops;

type R<T> = Result<T, String>;

/// One problem found in a layer, compared with the default master's layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    /// Path `path` (0-based, among paths) runs the other way round.
    Direction { path: usize },
    /// Closed path `path` starts at a different point.
    StartPoint { path: usize },
    /// Path `path` here looks like path `other` in the default master.
    Order { path: usize, other: usize },
    /// A smooth point of `path` bends between the masters, near (x, y) in the
    /// default master.
    Kink { path: usize, x: f64, y: f64 },
}

/// Findings for one layer of a glyph.
#[derive(Debug, Clone)]
pub struct LayerFindings {
    /// The layer's key (master id or brace layer id).
    pub key: String,
    /// The layer's name in messages (master name or brace label).
    pub who: String,
    pub findings: Vec<Finding>,
}

/// Mean squared distance below which two normalised outlines count as the same shape.
const SAME: f64 = 0.02;
/// A better alignment must cost less than this share of the current one to count.
const BETTER: f64 = 0.5;
/// Angles (degrees) for kinks: straight in both masters, bent halfway.
const STRAIGHT_DEG: f64 = 2.0;
const KINK_DEG: f64 = 4.0;

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

/// Signed area of a closed path's polygon (on- and off-curve points): positive when
/// it runs anticlockwise.
fn signed_area(p: &Path) -> f64 {
    let n = p.nodes.len();
    (0..n)
        .map(|i| {
            let (a, b) = (&p.nodes[i], &p.nodes[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

/// A path's points centred and scaled to unit size, so shapes at different weights
/// can be compared.
fn normalised(nodes: &[Node]) -> Vec<(f64, f64)> {
    let n = nodes.len().max(1) as f64;
    let (cx, cy) = (
        nodes.iter().map(|p| p.x).sum::<f64>() / n,
        nodes.iter().map(|p| p.y).sum::<f64>() / n,
    );
    let r = (nodes
        .iter()
        .map(|p| (p.x - cx).powi(2) + (p.y - cy).powi(2))
        .sum::<f64>()
        / n)
        .sqrt()
        .max(1e-9);
    nodes
        .iter()
        .map(|p| ((p.x - cx) / r, (p.y - cy) / r))
        .collect()
}

fn cost(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let n = a.len().max(1) as f64;
    a.iter()
        .zip(b)
        .map(|(p, q)| (p.0 - q.0).powi(2) + (p.1 - q.1).powi(2))
        .sum::<f64>()
        / n
}

fn pattern(nodes: &[Node]) -> Vec<bool> {
    nodes.iter().map(|n| n.on_curve()).collect()
}

/// The best way to line `other` up with `reference` (both closed, same length):
/// whether to reverse it, by how much to rotate it, and the resulting cost against
/// the cost as it is. Only alignments that keep on-/off-curve points matching count.
fn best_alignment(reference: &Path, other: &Path) -> Option<(bool, usize, f64, f64)> {
    let n = other.nodes.len();
    if !reference.closed || !other.closed || n != reference.nodes.len() || n < 3 {
        return None;
    }
    let target = normalised(&reference.nodes);
    let want = pattern(&reference.nodes);
    let as_is = cost(&target, &normalised(&other.nodes));
    let flip = signed_area(reference).signum() != signed_area(other).signum()
        && signed_area(reference).abs() > 1e-6
        && signed_area(other).abs() > 1e-6;
    let base = if flip {
        pathops::reverse(other)
    } else {
        other.clone()
    };
    let mut best: Option<(usize, f64)> = None;
    for r in 0..n {
        let mut nodes = base.nodes.clone();
        nodes.rotate_left(r);
        if pattern(&nodes) != want {
            continue;
        }
        let c = cost(&target, &normalised(&nodes));
        if best.is_none_or(|(_, b)| c < b) {
            best = Some((r, c));
        }
    }
    best.map(|(r, c)| (flip, r, c, as_is))
}

/// Paths here that fit another path of the reference better than their own, among
/// paths with the same structure: a permutation `perm[i]` = the reference path that
/// path i matches. `None` when the order is fine.
fn better_order(
    reference: &[&Path],
    other: &[&Path],
    ref_layer: &Layer,
    layer: &Layer,
) -> Option<Vec<usize>> {
    if reference.len() != other.len() || other.len() < 2 {
        return None;
    }
    let place = |l: &Layer, p: &Path| -> (f64, f64, f64) {
        let (x0, y0, x1, y1) = bounds_of(l);
        let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        let n = p.nodes.len().max(1) as f64;
        let cx = (p.nodes.iter().map(|q| q.x).sum::<f64>() / n - x0) / w;
        let cy = (p.nodes.iter().map(|q| q.y).sum::<f64>() / n - y0) / h;
        let (a, b, c, d) = path_bounds(p);
        let size = ((c - a) / w).hypot((d - b) / h);
        (cx, cy, size)
    };
    let rp: Vec<_> = reference.iter().map(|p| place(ref_layer, p)).collect();
    let op: Vec<_> = other.iter().map(|p| place(layer, p)).collect();
    let same_kind = |i: usize, j: usize| {
        other[i].closed == reference[j].closed
            && pattern(&other[i].nodes) == pattern(&reference[j].nodes)
    };
    let pair = |i: usize, j: usize| {
        let (a, b) = (op[i], rp[j]);
        (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)
    };
    let n = other.len();
    let identity: f64 = (0..n).map(|i| pair(i, i)).sum();
    if identity < 0.01 {
        return None;
    }
    // Best assignment among same-structure paths: exhaustive for small glyphs,
    // greedy otherwise.
    let mut best: Vec<usize> = (0..n).collect();
    let mut best_cost = identity;
    if n <= 7 {
        let mut perm: Vec<usize> = (0..n).collect();
        permute(&mut perm, 0, &mut |p| {
            if p.iter().enumerate().all(|(i, &j)| same_kind(i, j)) {
                let c: f64 = p.iter().enumerate().map(|(i, &j)| pair(i, j)).sum();
                if c < best_cost {
                    best_cost = c;
                    best = p.to_vec();
                }
            }
        });
    } else {
        let mut used = vec![false; n];
        let mut perm = vec![0; n];
        let mut c = 0.0;
        for (i, slot) in perm.iter_mut().enumerate() {
            let j = (0..n)
                .filter(|&j| !used[j] && same_kind(i, j))
                .min_by(|&a, &b| pair(i, a).total_cmp(&pair(i, b)))?;
            used[j] = true;
            *slot = j;
            c += pair(i, j);
        }
        if c < best_cost {
            best_cost = c;
            best = perm;
        }
    }
    (best.iter().enumerate().any(|(i, &j)| i != j) && best_cost < BETTER * identity).then_some(best)
}

fn permute(p: &mut Vec<usize>, k: usize, f: &mut impl FnMut(&[usize])) {
    if k == p.len() {
        f(p);
        return;
    }
    for i in k..p.len() {
        p.swap(k, i);
        permute(p, k + 1, f);
        p.swap(k, i);
    }
}

fn path_bounds(p: &Path) -> (f64, f64, f64, f64) {
    p.nodes.iter().fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(a, b, c, d), n| (a.min(n.x), b.min(n.y), c.max(n.x), d.max(n.y)),
    )
}

fn bounds_of(l: &Layer) -> (f64, f64, f64, f64) {
    paths(l).iter().map(|p| path_bounds(p)).fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(a, b, c, d), (e, f, g, h)| (a.min(e), b.min(f), c.max(g), d.max(h)),
    )
}

fn angle_deg(ax: f64, ay: f64, bx: f64, by: f64) -> Option<f64> {
    if ax.hypot(ay) < 1e-6 || bx.hypot(by) < 1e-6 {
        return None;
    }
    Some(
        (ax * by - ay * bx)
            .atan2(ax * bx + ay * by)
            .abs()
            .to_degrees(),
    )
}

/// Smooth points of `a` that are straight in both paths but bend halfway between them.
fn kinks(a: &Path, b: &Path) -> Vec<(f64, f64)> {
    let n = a.nodes.len();
    let mut out = Vec::new();
    for i in 0..n {
        let node = &a.nodes[i];
        if !node.smooth || !node.on_curve() {
            continue;
        }
        if !a.closed && (i == 0 || i == n - 1) {
            continue;
        }
        let (p, q) = ((i + n - 1) % n, (i + 1) % n);
        let turn = |pts: &[Node]| {
            let (prev, cur, next) = (&pts[p], &pts[i], &pts[q]);
            angle_deg(
                cur.x - prev.x,
                cur.y - prev.y,
                next.x - cur.x,
                next.y - cur.y,
            )
        };
        let mid: Vec<Node> = a
            .nodes
            .iter()
            .zip(&b.nodes)
            .map(|(x, y)| {
                let mut m = *x;
                m.x = (x.x + y.x) / 2.0;
                m.y = (x.y + y.y) / 2.0;
                m
            })
            .collect();
        if let (Some(ta), Some(tb), Some(tm)) = (turn(&a.nodes), turn(&b.nodes), turn(&mid)) {
            if ta < STRAIGHT_DEG && tb < STRAIGHT_DEG && tm > KINK_DEG {
                out.push((node.x.round(), node.y.round()));
            }
        }
    }
    out
}

/// What's wrong with `layer` compared with the default master's `reference`.
fn findings(reference: &Layer, layer: &Layer) -> Vec<Finding> {
    let (rp, lp) = (paths(reference), paths(layer));
    let mut out = Vec::new();
    if let Some(perm) = better_order(&rp, &lp, reference, layer) {
        out.extend(
            perm.iter()
                .enumerate()
                .filter(|(i, j)| i != *j)
                .map(|(i, &j)| Finding::Order { path: i, other: j }),
        );
        // Direction and start point are judged after the order is fixed.
        return out;
    }
    for (i, (a, b)) in rp.iter().zip(&lp).enumerate() {
        if let Some((flip, r, best, as_is)) = best_alignment(a, b) {
            if flip {
                out.push(Finding::Direction { path: i });
            } else if r != 0 && as_is > SAME && best < BETTER * as_is {
                out.push(Finding::StartPoint { path: i });
            }
        }
        if out.iter().all(|f| !matches!(f, Finding::Direction { path } | Finding::StartPoint { path } if *path == i)) {
            for (x, y) in kinks(a, b) {
                out.push(Finding::Kink { path: i, x, y });
            }
        }
    }
    out
}

/// Every master and intermediate layer of `glyph` compared with the default master.
/// Empty for glyphs that can't be interpolated at all (see `check_glyph`) or with
/// one master.
pub fn analyse(font: &Font, glyph: &str) -> Vec<LayerFindings> {
    if font.masters.len() < 2 || !check_glyph(font, glyph).is_empty() {
        return Vec::new();
    }
    let Some(g) = font.glyph(glyph) else {
        return Vec::new();
    };
    let def = default_master(font);
    let Some(reference) = g.layer(&def.id) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let others = font
        .masters
        .iter()
        .filter(|m| m.id != def.id)
        .filter_map(|m| g.layer(&m.id).map(|l| (l, m.name.clone())))
        .chain(g.brace_layers().map(|l| (l, l.label().unwrap_or_default())));
    for (layer, who) in others {
        let f = findings(reference, layer);
        if !f.is_empty() {
            out.push(LayerFindings {
                key: layer.key().to_string(),
                who,
                findings: f,
            });
        }
    }
    out
}

/// The findings as messages for the UI (translated there; see locales/engine.ts).
pub fn warnings(font: &Font, glyph: &str) -> Vec<String> {
    let reference = default_master(font).name.clone();
    analyse(font, glyph)
        .into_iter()
        .flat_map(|l| {
            let r = reference.clone();
            l.findings.into_iter().map(move |f| match f {
                Finding::Direction { path } => format!(
                    "{}: path {} runs the other way round than in {r}",
                    l.who,
                    path + 1
                ),
                Finding::StartPoint { path } => format!(
                    "{}: path {} starts at a different point than in {r}",
                    l.who,
                    path + 1
                ),
                Finding::Order { path, other } => format!(
                    "{}: path {} looks like path {} of {r}; the paths may be in a different order",
                    l.who,
                    path + 1,
                    other + 1
                ),
                Finding::Kink { path, x, y } => format!(
                    "{}: a kink appears near {x}, {y} on path {} when interpolating with {r}",
                    l.who,
                    path + 1
                ),
            })
        })
        .collect()
}

/// Whether the glyph has any interpolation warnings.
pub fn has_warnings(font: &Font, glyph: &str) -> bool {
    !analyse(font, glyph).is_empty()
}

/// Whether [`fix`] can repair something (order, direction or start point; kinks
/// need the designer).
pub fn fixable(font: &Font, glyph: &str) -> bool {
    analyse(font, glyph)
        .iter()
        .flat_map(|l| &l.findings)
        .any(|f| !matches!(f, Finding::Kink { .. }))
}

/// Fixes contour order, direction and start points in every master and intermediate
/// layer so they line up with the default master. Kinks are left for the designer.
/// Returns the new font and how many paths changed.
pub fn fix(font: &Font, glyph: &str) -> R<(Font, usize)> {
    if !check_glyph(font, glyph).is_empty() {
        return Err(
            "This glyph's masters can't be interpolated yet; make them compatible first.".into(),
        );
    }
    let def = default_master(font).id.clone();
    let mut f = font.clone();
    let reference = f
        .glyph(glyph)
        .and_then(|g| g.layer(&def))
        .cloned()
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let keys: Vec<String> = analyse(font, glyph).into_iter().map(|l| l.key).collect();
    let g = f.glyph_mut(glyph).expect("glyph exists");
    if g.locked {
        return Err("Unlock the glyph first.".into());
    }
    let mut changed = 0;
    for key in keys {
        let layer = g.layer_mut(&key).expect("layer exists");
        // 1. Order: put each path where its counterpart is in the default master.
        let (rp, lp) = (paths(&reference), paths(layer));
        if let Some(perm) = better_order(&rp, &lp, &reference, layer) {
            let old: Vec<Path> = lp.into_iter().cloned().collect();
            let mut new = old.clone();
            for (i, &j) in perm.iter().enumerate() {
                new[j] = old[i].clone();
            }
            let mut it = new.into_iter();
            for s in &mut layer.shapes {
                if let Shape::Path(p) = s {
                    *p = it.next().expect("same number of paths");
                }
            }
            changed += perm.iter().enumerate().filter(|(i, j)| i != *j).count();
        }
        // 2. Direction and start point.
        let rp = paths(&reference);
        let mut pi = 0;
        for s in &mut layer.shapes {
            let Shape::Path(p) = s else { continue };
            let Some(r) = rp.get(pi) else { break };
            pi += 1;
            if p.locked {
                continue;
            }
            if let Some((flip, rot, best, as_is)) = best_alignment(r, p) {
                if flip || (rot != 0 && as_is > SAME && best < BETTER * as_is) {
                    let mut q = if flip { pathops::reverse(p) } else { p.clone() };
                    q.nodes.rotate_left(rot);
                    *p = q;
                    changed += 1;
                }
            }
        }
    }
    if !check_glyph(&f, glyph).is_empty() {
        return Err("Fixing would make the masters incompatible; fix this glyph by hand.".into());
    }
    Ok((f, changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Axis, NodeKind};
    use crate::sample::sample_font;
    use crate::structure;
    use std::collections::BTreeMap;

    /// The sample font with a Bold master (a copy of Regular, so O starts compatible).
    fn two_masters() -> (Font, String) {
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        };
        let f = structure::set_axes(&sample_font(), vec![wght]).unwrap();
        let (f, bold) =
            structure::add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)]))
                .unwrap();
        (f, bold)
    }

    fn bold_paths(f: &mut Font, bold: &str) -> Vec<Path> {
        paths(f.glyph("O").unwrap().layer(bold).unwrap())
            .into_iter()
            .cloned()
            .collect()
    }

    fn set_bold_paths(f: &mut Font, bold: &str, ps: Vec<Path>) {
        let l = f.glyph_mut("O").unwrap().layer_mut(bold).unwrap();
        let mut it = ps.into_iter();
        for s in &mut l.shapes {
            if let Shape::Path(p) = s {
                *p = it.next().unwrap();
            }
        }
    }

    #[test]
    fn identical_masters_have_no_warnings() {
        let (f, _) = two_masters();
        assert!(warnings(&f, "O").is_empty());
        assert!(!has_warnings(&f, "H"));
    }

    #[test]
    fn a_rotated_start_point_is_found_and_fixed() {
        let (mut f, bold) = two_masters();
        let original = bold_paths(&mut f, &bold);
        let mut ps = original.clone();
        // Start the outer contour one segment later.
        let on: Vec<usize> = (0..ps[0].nodes.len())
            .filter(|&i| ps[0].nodes[i].on_curve())
            .collect();
        ps[0].nodes.rotate_left(on[1] - on[0]);
        set_bold_paths(&mut f, &bold, ps);
        assert!(check_glyph(&f, "O").is_empty(), "still compatible");
        let w = warnings(&f, "O");
        assert_eq!(
            w,
            ["Bold: path 1 starts at a different point than in Regular"]
        );
        let (mut fixed, n) = fix(&f, "O").unwrap();
        assert_eq!(n, 1);
        assert!(warnings(&fixed, "O").is_empty());
        assert_eq!(
            bold_paths(&mut fixed, &bold),
            original,
            "the start point is back where it was"
        );
    }

    #[test]
    fn a_reversed_path_is_found_and_fixed() {
        let (mut f, bold) = two_masters();
        let mut ps = bold_paths(&mut f, &bold);
        ps[1] = pathops::reverse(&ps[1]);
        set_bold_paths(&mut f, &bold, ps);
        assert!(
            check_glyph(&f, "O").is_empty(),
            "an O contour reverses into the same on/off-curve pattern"
        );
        let w = warnings(&f, "O");
        assert_eq!(w, ["Bold: path 2 runs the other way round than in Regular"]);
        let (fixed, _) = fix(&f, "O").unwrap();
        assert!(warnings(&fixed, "O").is_empty());
    }

    #[test]
    fn swapped_paths_are_found_and_fixed() {
        let (mut f, bold) = two_masters();
        // Two identical squares, far apart; swap them in Bold.
        let sq = |x: f64| Path {
            closed: true,
            locked: false,
            stroke: None,
            nodes: [(x, 0.0), (x + 100.0, 0.0), (x + 100.0, 100.0), (x, 100.0)]
                .into_iter()
                .map(|(x, y)| Node::new(x, y, NodeKind::Line))
                .collect(),
        };
        for id in ["m01".to_string(), bold.clone()] {
            let l = f.glyph_mut("O").unwrap().layer_mut(&id).unwrap();
            l.shapes = vec![Shape::Path(sq(0.0)), Shape::Path(sq(600.0))];
        }
        let l = f.glyph_mut("O").unwrap().layer_mut(&bold).unwrap();
        l.shapes.swap(0, 1);
        let w = warnings(&f, "O");
        assert_eq!(
            w,
            [
                "Bold: path 1 looks like path 2 of Regular; the paths may be in a different order",
                "Bold: path 2 looks like path 1 of Regular; the paths may be in a different order"
            ]
        );
        let (fixed, n) = fix(&f, "O").unwrap();
        assert_eq!(n, 2);
        assert!(warnings(&fixed, "O").is_empty());
        let l = fixed.glyph("O").unwrap().layer(&bold).unwrap();
        assert_eq!(
            paths(l)[0].nodes[0].x,
            0.0,
            "the left square is first again"
        );
    }

    #[test]
    fn a_kink_between_straight_masters_is_found() {
        let (mut f, bold) = two_masters();
        // A smooth point at (200, 0) whose handles lie on one straight line in each
        // master, but at different angles and with the long handle on opposite sides:
        // halfway between, the two handles no longer line up.
        let path = |hin: (f64, f64), hout: (f64, f64)| {
            let mut smooth = Node::new(200.0, 0.0, NodeKind::Curve);
            smooth.smooth = true;
            Path {
                closed: false,
                locked: false,
                stroke: None,
                nodes: vec![
                    Node::new(0.0, 0.0, NodeKind::Line),
                    Node::new(hin.0, hin.1, NodeKind::OffCurve),
                    smooth,
                    Node::new(hout.0, hout.1, NodeKind::OffCurve),
                    Node::new(400.0, 0.0, NodeKind::Curve),
                ],
            }
        };
        // Regular: tangent at 45°, long incoming handle, short outgoing one.
        let r = path((100.0, -100.0), (210.0, 10.0));
        // Bold: level tangent, short incoming handle, long outgoing one.
        let b = path((190.0, 0.0), (400.0, 0.0));
        f.glyph_mut("O").unwrap().layer_mut("m01").unwrap().shapes = vec![Shape::Path(r)];
        f.glyph_mut("O").unwrap().layer_mut(&bold).unwrap().shapes = vec![Shape::Path(b)];
        assert!(check_glyph(&f, "O").is_empty());
        let w = warnings(&f, "O");
        assert_eq!(
            w,
            ["Bold: a kink appears near 200, 0 on path 1 when interpolating with Regular"]
        );
    }

    #[test]
    fn a_heavier_master_is_not_flagged() {
        // Bold drawn heavier with Offset Curve: real design change, same structure.
        let (mut f, bold) = two_masters();
        let names: Vec<String> = f.glyphs.iter().map(|g| g.name.clone()).collect();
        let mut checked = 0;
        for name in &names {
            let l = f.glyph_mut(name).unwrap().layer_mut(&bold).unwrap();
            if l.shapes.is_empty() {
                continue;
            }
            crate::filters::offset_curve(l, 30.0, 20.0, false, None);
            if check_glyph(&f, name).is_empty() {
                checked += 1;
                assert!(
                    warnings(&f, name).is_empty(),
                    "{name}: {:?}",
                    warnings(&f, name)
                );
            }
        }
        assert!(
            checked >= 5,
            "only {checked} drawn glyphs stayed compatible"
        );
    }

    #[test]
    fn smart_filter_finds_glyphs_with_warnings() {
        use crate::smartfilter::{evaluate, Filter, FilterKind, Rule, RuleGroup};
        let (mut f, bold) = two_masters();
        let mut ps = bold_paths(&mut f, &bold);
        let on: Vec<usize> = (0..ps[0].nodes.len())
            .filter(|&i| ps[0].nodes[i].on_curve())
            .collect();
        ps[0].nodes.rotate_left(on[1] - on[0]);
        set_bold_paths(&mut f, &bold, ps);
        let filter = |value| Filter {
            name: "Warnings".into(),
            kind: FilterKind::Smart,
            rules: RuleGroup {
                rules: vec![Rule::InterpolationWarnings { value }],
                ..Default::default()
            },
            glyphs: Vec::new(),
        };
        assert_eq!(evaluate(&f, &filter(true)), ["O"]);
        assert!(!evaluate(&f, &filter(false)).contains(&"O".to_string()));
    }

    #[test]
    fn incompatible_glyphs_get_no_quality_warnings() {
        let (mut f, bold) = two_masters();
        f.glyph_mut("O")
            .unwrap()
            .layer_mut(&bold)
            .unwrap()
            .shapes
            .pop();
        assert!(!check_glyph(&f, "O").is_empty());
        assert!(warnings(&f, "O").is_empty());
        assert!(fix(&f, "O").unwrap_err().contains("compatible first"));
    }
}
