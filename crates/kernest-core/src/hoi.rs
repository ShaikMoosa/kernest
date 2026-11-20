//! Higher-order interpolation (S40): nodes that travel along a curve, not a
//! straight line, from one master to the next master up an axis.
//!
//! A curve is kept on the lower master's layer, for one node and one axis. On export
//! [`bake`] turns the curves into intermediate (brace) layers between the two
//! masters, at a quarter, half and three quarters of the way, with every curved
//! node placed on its curve and every other point interpolated as usual. Static
//! instances and variable fonts both follow the curves that way, and the masters
//! themselves stay as drawn.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::interpolate::{rebuild, values};
use crate::model::{Font, Glyph, Layer, Shape};

type R<T> = Result<T, String>;

/// Where along the way the baked intermediate layers go.
pub const STEPS: [f64; 3] = [0.25, 0.5, 0.75];

/// Which way a rotation turns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Turn {
    /// The smaller angle.
    #[default]
    Auto,
    Clockwise,
    Counterclockwise,
}

/// The shape of a node's path between two masters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum HoiKind {
    /// A curve through this point, halfway along in time.
    Through { x: f64, y: f64 },
    /// A cubic curve with these two handles.
    Handles { a: [f64; 2], b: [f64; 2] },
    /// A turn around a centre.
    Rotation {
        center: [f64; 2],
        #[serde(default)]
        turn: Turn,
    },
}

/// One node's interpolation curve along one axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoiCurve {
    /// The axis tag.
    pub axis: String,
    /// The path (counting paths only) and the node in it.
    pub path: usize,
    pub node: usize,
    pub kind: HoiKind,
}

impl HoiKind {
    /// The point at time `t` (0 at `from`, 1 at `to`).
    pub fn at(&self, from: [f64; 2], to: [f64; 2], t: f64) -> [f64; 2] {
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        match self {
            HoiKind::Through { x, y } => {
                // The quadratic through (x, y) at t = 0.5.
                let c = [
                    2.0 * x - (from[0] + to[0]) / 2.0,
                    2.0 * y - (from[1] + to[1]) / 2.0,
                ];
                let m = 1.0 - t;
                [
                    m * m * from[0] + 2.0 * m * t * c[0] + t * t * to[0],
                    m * m * from[1] + 2.0 * m * t * c[1] + t * t * to[1],
                ]
            }
            HoiKind::Handles { a, b } => {
                let m = 1.0 - t;
                let f = |p0: f64, p1: f64, p2: f64, p3: f64| {
                    m * m * m * p0 + 3.0 * m * m * t * p1 + 3.0 * m * t * t * p2 + t * t * t * p3
                };
                [f(from[0], a[0], b[0], to[0]), f(from[1], a[1], b[1], to[1])]
            }
            HoiKind::Rotation { center, turn } => {
                let (dx0, dy0) = (from[0] - center[0], from[1] - center[1]);
                let (dx1, dy1) = (to[0] - center[0], to[1] - center[1]);
                let (r0, r1) = (dx0.hypot(dy0), dx1.hypot(dy1));
                if r0 < 1e-9 || r1 < 1e-9 {
                    return [lerp(from[0], to[0]), lerp(from[1], to[1])];
                }
                let (a0, a1) = (dy0.atan2(dx0), dy1.atan2(dx1));
                let tau = std::f64::consts::TAU;
                let mut d = (a1 - a0).rem_euclid(tau); // counter-clockwise, 0..2π
                match turn {
                    Turn::Counterclockwise => {}
                    Turn::Clockwise => d -= tau,
                    Turn::Auto => {
                        if d > std::f64::consts::PI {
                            d -= tau;
                        }
                    }
                }
                let a = a0 + d * t;
                let r = lerp(r0, r1);
                [center[0] + r * a.cos(), center[1] + r * a.sin()]
            }
        }
    }
}

/// The next master up from `master` along `axis`: the nearest one with a larger
/// value there and the same values on every other axis.
pub fn next_up<'a>(font: &'a Font, master: &str, axis: &str) -> Option<&'a crate::model::Master> {
    let m = font.master(master)?;
    let at = |mm: &crate::model::Master, tag: &str| {
        mm.location.get(tag).copied().unwrap_or_else(|| {
            font.axes
                .iter()
                .find(|a| a.tag == tag)
                .map(|a| crate::designspace::design_bounds(a).1)
                .unwrap_or(0.0)
        })
    };
    let here = at(m, axis);
    font.masters
        .iter()
        .filter(|o| o.id != m.id && at(o, axis) > here)
        .filter(|o| {
            font.axes
                .iter()
                .filter(|a| a.tag != axis)
                .all(|a| (at(o, &a.tag) - at(m, &a.tag)).abs() < 1e-9)
        })
        .min_by(|x, y| at(x, axis).total_cmp(&at(y, axis)))
}

/// The next master down from `master` along `axis` (the mirror of [`next_up`]).
pub fn next_down<'a>(font: &'a Font, master: &str, axis: &str) -> Option<&'a crate::model::Master> {
    font.masters
        .iter()
        .find(|o| next_up(font, &o.id, axis).is_some_and(|u| u.id == master))
}

fn node_at(l: &Layer, path: usize, node: usize) -> Option<[f64; 2]> {
    l.shapes
        .iter()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .nth(path)
        .and_then(|p| p.nodes.get(node))
        .map(|n| [n.x, n.y])
}

fn set_node(l: &mut Layer, path: usize, node: usize, p: [f64; 2]) {
    if let Some(n) = l
        .shapes
        .iter_mut()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .nth(path)
        .and_then(|pp| pp.nodes.get_mut(node))
    {
        n.x = p[0];
        n.y = p[1];
    }
}

/// The font with every interpolation curve turned into intermediate layers (see the
/// module notes). Glyphs and master pairs that can't be interpolated, and places
/// that already have an intermediate layer, are left as they are.
pub fn bake(font: &Font) -> Font {
    if !font
        .glyphs
        .iter()
        .any(|g| g.layers.iter().any(|l| !l.hoi.is_empty()))
    {
        return font.clone();
    }
    let mut f = font.clone();
    for gi in 0..f.glyphs.len() {
        let extra = baked_layers(font, &font.glyphs[gi]);
        let g = &mut f.glyphs[gi];
        for mut l in extra {
            l.id = Some(g.new_layer_id(&l.master_id));
            g.layers.push(l);
        }
        for l in &mut g.layers {
            l.hoi.clear();
        }
    }
    f
}

fn baked_layers(font: &Font, g: &Glyph) -> Vec<Layer> {
    let mut out: Vec<Layer> = Vec::new();
    for lower in g
        .layers
        .iter()
        .filter(|l| l.is_master() && !l.hoi.is_empty())
    {
        let mut by_axis: BTreeMap<&str, Vec<&HoiCurve>> = BTreeMap::new();
        for c in &lower.hoi {
            by_axis.entry(c.axis.as_str()).or_default().push(c);
        }
        for (axis, curves) in by_axis {
            let Some(up) = next_up(font, &lower.master_id, axis) else {
                continue;
            };
            let Some(upper) = g.layer(&up.id).filter(|l| l.is_master()) else {
                continue;
            };
            if !crate::interpolate::compare("", upper, "", lower).is_empty() {
                continue;
            }
            let (v0, v1) = (values(lower), values(upper));
            if v0.len() != v1.len() {
                continue;
            }
            let Some(m0) = font.master(&lower.master_id) else {
                continue;
            };
            for &t in &STEPS {
                let vals: Vec<f64> = v0.iter().zip(&v1).map(|(a, b)| a + (b - a) * t).collect();
                let mut l = rebuild(lower, &vals);
                l.hoi.clear();
                for c in &curves {
                    let (Some(p0), Some(p1)) = (
                        node_at(lower, c.path, c.node),
                        node_at(upper, c.path, c.node),
                    ) else {
                        continue;
                    };
                    set_node(&mut l, c.path, c.node, c.kind.at(p0, p1, t));
                }
                let mut loc = m0.location.clone();
                let a0 = m0.location.get(axis).copied().unwrap_or(0.0);
                let a1 = up.location.get(axis).copied().unwrap_or(a0);
                loc.insert(axis.to_string(), a0 + (a1 - a0) * t);
                // Every axis gets a value, as brace layers need.
                for a in &font.axes {
                    loc.entry(a.tag.clone())
                        .or_insert_with(|| crate::designspace::design_bounds(a).1);
                }
                let taken = g.brace_layers().any(|b| b.location.as_ref() == Some(&loc))
                    || out.iter().any(|b| b.location.as_ref() == Some(&loc));
                if taken {
                    continue;
                }
                l.location = Some(loc);
                l.bracket.clear();
                l.smart = None;
                l.backup = None;
                l.id = None;
                out.push(l);
            }
        }
    }
    out
}

/// Sets (or, with `None`, removes) a node's interpolation curve along `axis` on a
/// master layer. The node must exist and there must be a master further up the axis.
pub fn set_curve(
    font: &Font,
    glyph: &str,
    key: &str,
    axis: &str,
    path: usize,
    node: usize,
    kind: Option<HoiKind>,
) -> R<Font> {
    if !font.axes.iter().any(|a| a.tag == axis) {
        return Err(format!("There is no axis \"{axis}\"."));
    }
    let l = font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or("That layer doesn't exist.")?;
    if !l.is_master() {
        return Err("Interpolation curves go on master layers.".into());
    }
    if next_up(font, &l.master_id, axis).is_none() {
        return Err("There is no master further up this axis to interpolate to.".into());
    }
    node_at(l, path, node).ok_or("No node at that position.")?;
    if let Some(HoiKind::Through { x, y }) | Some(HoiKind::Rotation { center: [x, y], .. }) = &kind
    {
        if !x.is_finite() || !y.is_finite() {
            return Err("That point isn't valid.".into());
        }
    }
    let mut f = font.clone();
    let l = f
        .glyph_mut(glyph)
        .and_then(|g| g.layer_mut(key))
        .expect("checked");
    l.hoi
        .retain(|c| !(c.axis == axis && c.path == path && c.node == node));
    if let Some(kind) = kind {
        l.hoi.push(HoiCurve {
            axis: axis.to_string(),
            path,
            node,
            kind,
        });
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Axis, Master, Metrics, Node, NodeKind, Path};
    use crate::sample::sample_font;

    fn two_masters() -> Font {
        let mut f = sample_font();
        f.axes.push(Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 100.0,
            max: 900.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        });
        f.masters[0].location.insert("wght".into(), 100.0);
        let mut bold = Master {
            id: "m02".into(),
            name: "Bold".into(),
            location: BTreeMap::from([("wght".into(), 900.0)]),
            metrics: Metrics::default(),
            guides: Vec::new(),
            alignment_zones: Vec::new(),
            stems: Vec::new(),
            vertical: Default::default(),
        };
        bold.metrics = f.masters[0].metrics;
        f.masters.push(bold);
        for g in &mut f.glyphs {
            let mut l = g.layers[0].clone();
            l.master_id = "m02".into();
            g.layers.push(l);
        }
        // One node of H moves 200 to the right in the bold master.
        let g = f.glyph_mut("H").unwrap();
        g.layers[0].shapes = vec![Shape::Path(Path {
            closed: true,
            nodes: vec![
                Node::new(0.0, 0.0, NodeKind::Line),
                Node::new(100.0, 0.0, NodeKind::Line),
                Node::new(100.0, 100.0, NodeKind::Line),
            ],
            locked: false,
            stroke: None,
            mask: false,
        })];
        let mut bold_layer = g.layers[0].clone();
        bold_layer.master_id = "m02".into();
        if let Shape::Path(p) = &mut bold_layer.shapes[0] {
            p.nodes[2].x = 300.0;
        }
        g.layers[1] = bold_layer;
        f
    }

    #[test]
    fn curves_have_the_right_shape() {
        let k = HoiKind::Through { x: 50.0, y: 100.0 };
        assert_eq!(k.at([0.0, 0.0], [100.0, 0.0], 0.5), [50.0, 100.0]);
        assert_eq!(k.at([0.0, 0.0], [100.0, 0.0], 0.0), [0.0, 0.0]);
        let r = HoiKind::Rotation {
            center: [0.0, 0.0],
            turn: Turn::Auto,
        };
        let p = r.at([100.0, 0.0], [0.0, 100.0], 0.5);
        assert!(
            (p[0] - 70.7107).abs() < 1e-3 && (p[1] - 70.7107).abs() < 1e-3,
            "{p:?}"
        );
        let cw = HoiKind::Rotation {
            center: [0.0, 0.0],
            turn: Turn::Clockwise,
        };
        let p = cw.at([100.0, 0.0], [0.0, 100.0], 0.5);
        assert!(p[1] < 0.0, "the long way round: {p:?}");
    }

    #[test]
    fn baking_adds_intermediate_layers_on_the_curve() {
        let f = two_masters();
        assert_eq!(
            next_up(&f, "m01", "wght").map(|m| m.id.as_str()),
            Some("m02")
        );
        assert_eq!(
            next_down(&f, "m02", "wght").map(|m| m.id.as_str()),
            Some("m01")
        );
        // Node 2 travels from (100,100) to (300,100) through (200,200).
        let f = set_curve(
            &f,
            "H",
            "m01",
            "wght",
            0,
            2,
            Some(HoiKind::Through { x: 200.0, y: 200.0 }),
        )
        .unwrap();
        let baked = bake(&f);
        let g = baked.glyph("H").unwrap();
        let braces: Vec<&Layer> = g.brace_layers().collect();
        assert_eq!(braces.len(), 3);
        let mid = braces
            .iter()
            .find(|l| l.location.as_ref().unwrap()["wght"] == 500.0)
            .unwrap();
        assert_eq!(node_at(mid, 0, 2), Some([200.0, 200.0]));
        // Other points interpolate as usual.
        assert_eq!(node_at(mid, 0, 1), Some([100.0, 0.0]));
        assert!(crate::interpolate::check_glyph(&baked, "H").is_empty());
        assert!(baked.validate().is_ok());
        // The curve itself is gone from the baked font; the source keeps it.
        assert!(g.layers.iter().all(|l| l.hoi.is_empty()));
        assert!(
            set_curve(
                &f,
                "H",
                "m02",
                "wght",
                0,
                2,
                Some(HoiKind::Through { x: 0.0, y: 0.0 })
            )
            .is_err(),
            "no master above Bold"
        );
    }
}
