//! Master compatibility and interpolating glyph layers at any design-space location.

use crate::designspace::{normalize, Location, VariationModel};
use crate::model::{Font, Layer, Master, NodeKind, Shape};

/// The master at the default location of every axis, or the first master.
pub fn default_master(font: &Font) -> &Master {
    font.masters
        .iter()
        .find(|m| normalize(font, &m.location).is_empty())
        .unwrap_or(&font.masters[0])
}

fn describe(shape: &Shape) -> String {
    match shape {
        Shape::Path(p) => format!(
            "{} path with {} points",
            if p.closed { "closed" } else { "open" },
            p.nodes.len()
        ),
        Shape::Component { base, .. } => format!("component \"{base}\""),
    }
}

/// Reasons a glyph's masters can't be interpolated (empty when compatible).
/// Each layer is compared with the default master's layer.
pub fn check_glyph(font: &Font, glyph: &str) -> Vec<String> {
    let Some(g) = font.glyph(glyph) else {
        return vec![format!("no glyph named \"{glyph}\"")];
    };
    let def = default_master(font);
    let Some(reference) = g.layer(&def.id) else {
        return vec![format!("no layer for {}", def.name)];
    };
    let mut issues = Vec::new();
    for m in font.masters.iter().filter(|m| m.id != def.id) {
        let Some(layer) = g.layer(&m.id) else {
            issues.push(format!("{}: layer is missing", m.name));
            continue;
        };
        if layer.shapes.len() != reference.shapes.len() {
            issues.push(format!(
                "{}: {} shapes, {} has {}",
                m.name,
                layer.shapes.len(),
                def.name,
                reference.shapes.len()
            ));
            continue;
        }
        for (i, (a, b)) in reference.shapes.iter().zip(&layer.shapes).enumerate() {
            let ok = match (a, b) {
                (Shape::Path(pa), Shape::Path(pb)) => {
                    pa.closed == pb.closed
                        && pa.nodes.len() == pb.nodes.len()
                        && pa.nodes.iter().zip(&pb.nodes).all(|(x, y)| {
                            (x.kind == NodeKind::OffCurve) == (y.kind == NodeKind::OffCurve)
                        })
                }
                (Shape::Component { base: ba, .. }, Shape::Component { base: bb, .. }) => ba == bb,
                _ => false,
            };
            if !ok {
                issues.push(format!(
                    "{}: shape {} is a {}, {} has a {}",
                    m.name,
                    i + 1,
                    describe(b),
                    def.name,
                    describe(a)
                ));
            }
        }
        if anchor_names(layer) != anchor_names(reference) {
            issues.push(format!("{}: anchors differ from {}", m.name, def.name));
        }
    }
    issues
}

fn anchor_names(l: &Layer) -> Vec<&str> {
    let mut n: Vec<&str> = l.anchors.iter().map(|a| a.name.as_str()).collect();
    n.sort_unstable();
    n
}

/// The numbers that interpolate: width, every point, component transforms, anchors (by name).
fn values(layer: &Layer) -> Vec<f64> {
    let mut v = vec![layer.width];
    for s in &layer.shapes {
        match s {
            Shape::Path(p) => p.nodes.iter().for_each(|n| v.extend([n.x, n.y])),
            Shape::Component { transform, .. } => v.extend(transform.iter()),
        }
    }
    let mut anchors: Vec<_> = layer.anchors.iter().collect();
    anchors.sort_by(|a, b| a.name.cmp(&b.name));
    anchors.iter().for_each(|a| v.extend([a.x, a.y]));
    v
}

/// Writes interpolated numbers back into a copy of the template layer.
fn rebuild(template: &Layer, vals: &[f64]) -> Layer {
    let mut out = template.clone();
    let mut it = vals.iter().copied();
    let mut next = || it.next().unwrap_or(0.0);
    out.width = next();
    for s in &mut out.shapes {
        match s {
            Shape::Path(p) => p.nodes.iter_mut().for_each(|n| {
                n.x = next();
                n.y = next();
            }),
            Shape::Component { transform, .. } => transform.iter_mut().for_each(|t| *t = next()),
        }
    }
    let mut order: Vec<usize> = (0..out.anchors.len()).collect();
    order.sort_by(|&a, &b| out.anchors[a].name.cmp(&out.anchors[b].name));
    for i in order {
        out.anchors[i].x = next();
        out.anchors[i].y = next();
    }
    out
}

/// Interpolates layers for one design-space location.
pub struct Interpolator {
    model: Option<VariationModel>,
    location: Location,
    default_id: String,
    master_ids: Vec<String>,
}

impl Interpolator {
    /// Prepares interpolation at `location` (design-space units, keyed by axis tag).
    pub fn new(font: &Font, location: &Location) -> Result<Self, String> {
        let default_id = default_master(font).id.clone();
        let master_ids: Vec<String> = font.masters.iter().map(|m| m.id.clone()).collect();
        let model = if font.masters.len() > 1 {
            let locs: Vec<Location> = font
                .masters
                .iter()
                .map(|m| normalize(font, &m.location))
                .collect();
            let axis_order: Vec<String> = font.axes.iter().map(|a| a.tag.clone()).collect();
            Some(VariationModel::new(&locs, &axis_order)?)
        } else {
            None
        };
        Ok(Self {
            model,
            location: normalize(font, location),
            default_id,
            master_ids,
        })
    }

    /// Interpolates arbitrary per-master numbers (in master order), e.g. kerning values.
    pub fn values(&self, per_master: &[Vec<f64>]) -> Vec<f64> {
        match &self.model {
            Some(m) => m.interpolate(per_master, &self.location),
            None => per_master.first().cloned().unwrap_or_default(),
        }
    }

    /// The glyph's layer at this location. Fails if its masters are incompatible.
    /// The result's `master_id` is the default master's.
    pub fn layer(&self, font: &Font, glyph: &str) -> Result<Layer, String> {
        let g = font
            .glyph(glyph)
            .ok_or_else(|| format!("no glyph named \"{glyph}\""))?;
        let template = g
            .layer(&self.default_id)
            .ok_or_else(|| format!("\"{glyph}\" has no default layer"))?;
        let Some(model) = &self.model else {
            return Ok(template.clone());
        };
        let issues = check_glyph(font, glyph);
        if !issues.is_empty() {
            return Err(format!(
                "\"{glyph}\" can't be interpolated: {}",
                issues.join("; ")
            ));
        }
        let per_master: Vec<Vec<f64>> = self
            .master_ids
            .iter()
            .map(|id| {
                g.layer(id)
                    .map(values)
                    .ok_or_else(|| format!("\"{glyph}\" is missing a layer"))
            })
            .collect::<Result<_, _>>()?;
        let vals = model.interpolate(&per_master, &self.location);
        Ok(rebuild(template, &vals))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{bounds, svg_path};
    use crate::model::{Axis, Master, Metrics, Node, Path};
    use crate::sample::sample_font;
    use std::collections::BTreeMap;

    /// The sample font with a Bold master whose H stems are 50 units thicker.
    fn two_masters() -> Font {
        let mut f = sample_font();
        f.axes.push(Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
        });
        f.masters[0].location = BTreeMap::from([("wght".into(), 400.0)]);
        f.masters.push(Master {
            id: "m02".into(),
            name: "Bold".into(),
            location: BTreeMap::from([("wght".into(), 700.0)]),
            metrics: Metrics::default(),
        });
        for g in &mut f.glyphs {
            let mut l = g.layers[0].clone();
            l.master_id = "m02".into();
            if g.name == "H" {
                // Make the first stem 50 units wider and the glyph 50 units wider.
                if let Shape::Path(p) = &mut l.shapes[0] {
                    p.nodes[1].x += 50.0;
                    p.nodes[2].x += 50.0;
                }
                l.width += 50.0;
            }
            g.layers.push(l);
        }
        f
    }

    #[test]
    fn compatible_and_interpolated() {
        let f = two_masters();
        f.validate().unwrap();
        assert!(check_glyph(&f, "H").is_empty());
        let mid = Interpolator::new(&f, &BTreeMap::from([("wght".into(), 550.0)])).unwrap();
        let h = mid.layer(&f, "H").unwrap();
        assert_eq!(h.width, 705.0);
        let b = bounds(&f, &h).unwrap();
        assert_eq!(b.min_x, 80.0);
        // Default location returns the Regular outline.
        let reg = Interpolator::new(&f, &BTreeMap::new()).unwrap();
        assert_eq!(
            svg_path(&f, &reg.layer(&f, "H").unwrap()),
            svg_path(&f, &f.glyph("H").unwrap().layers[0])
        );
    }

    #[test]
    fn incompatible_glyphs_are_reported() {
        let mut f = two_masters();
        let bold = f.glyph_mut("H").unwrap().layer_mut("m02").unwrap();
        if let Shape::Path(p) = &mut bold.shapes[0] {
            p.nodes.push(Node::new(0.0, 0.0, NodeKind::Line));
        }
        let issues = check_glyph(&f, "H");
        assert_eq!(issues.len(), 1);
        assert!(
            issues[0].contains("Bold: shape 1 is a closed path with 5 points"),
            "{issues:?}"
        );
        let mid = Interpolator::new(&f, &BTreeMap::from([("wght".into(), 550.0)])).unwrap();
        assert!(mid.layer(&f, "H").is_err());
        // Extra shape.
        f.glyph_mut("I")
            .unwrap()
            .layer_mut("m02")
            .unwrap()
            .shapes
            .push(Shape::Path(Path {
                closed: true,
                nodes: vec![],
            }));
        assert!(check_glyph(&f, "I")[0].contains("2 shapes"));
    }

    #[test]
    fn single_master_needs_no_axes() {
        let f = sample_font();
        let i = Interpolator::new(&f, &BTreeMap::new()).unwrap();
        assert_eq!(i.layer(&f, "H").unwrap(), f.glyph("H").unwrap().layers[0]);
        assert!(check_glyph(&f, "H").is_empty());
    }
}
