//! Master compatibility and interpolating glyph layers at any design-space location.

use crate::designspace::{normalize, Location, VariationModel};
use crate::model::{in_bracket, AxisRange, Font, Layer, Master, NodeKind, Shape};

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

/// Why `layer` can't be interpolated with `reference` (empty when compatible).
pub(crate) fn compare(name: &str, reference: &Layer, ref_name: &str, layer: &Layer) -> Vec<String> {
    let mut issues = Vec::new();
    if layer.shapes.len() != reference.shapes.len() {
        issues.push(format!(
            "{name}: {} shapes, {ref_name} has {}",
            layer.shapes.len(),
            reference.shapes.len()
        ));
        return issues;
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
            (
                Shape::Component {
                    base: ba,
                    smart: sa,
                    ..
                },
                Shape::Component {
                    base: bb,
                    smart: sb,
                    ..
                },
            ) => ba == bb && sa.keys().eq(sb.keys()),
            _ => false,
        };
        if !ok {
            issues.push(format!(
                "{name}: shape {} is a {}, {ref_name} has a {}",
                i + 1,
                describe(b),
                describe(a)
            ));
        }
    }
    if anchor_names(layer) != anchor_names(reference) {
        issues.push(format!("{name}: anchors differ from {ref_name}"));
    }
    issues
}

/// Reasons a glyph can't be interpolated (empty when compatible): every master layer
/// and brace layer is compared with the default master's layer, and the bracket
/// layers of each alternate (with the master layers they fall back to) with each other.
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
        match g.layer(&m.id) {
            Some(layer) => issues.extend(compare(&m.name, reference, &def.name, layer)),
            None => issues.push(format!("{}: layer is missing", m.name)),
        }
    }
    for l in g.brace_layers() {
        let name = l.label().unwrap_or_default();
        issues.extend(compare(&name, reference, &def.name, l));
    }
    // Smart poles must match their master's layer point for point.
    for l in g.smart_layers() {
        if let (Some(m), Some(master)) = (font.master(&l.master_id), g.layer(&l.master_id)) {
            let name = format!("{} {}", m.name, l.label().unwrap_or_default());
            issues.extend(compare(&name, master, &m.name, l));
        }
    }
    // Colour layers: the same number in every master, each matching its counterpart.
    let def_colors: Vec<&Layer> = g.color_layers(&def.id).collect();
    for m in font.masters.iter().filter(|m| m.id != def.id) {
        let colors: Vec<&Layer> = g.color_layers(&m.id).collect();
        if colors.len() != def_colors.len() {
            issues.push(format!(
                "{}: {} colour layers, {} has {}",
                m.name,
                colors.len(),
                def.name,
                def_colors.len()
            ));
            continue;
        }
        for (i, (a, b)) in def_colors.iter().zip(&colors).enumerate() {
            issues.extend(compare(
                &format!("{} · colour layer {}", m.name, i + 1),
                a,
                &format!("{} · colour layer {}", def.name, i + 1),
                b,
            ));
        }
    }
    for ranges in g.bracket_groups() {
        let alt = |m: &Master| {
            g.bracket_layers()
                .find(|l| l.master_id == m.id && l.bracket == ranges)
                .or_else(|| g.layer(&m.id))
        };
        let Some(alt_ref) = alt(def) else { continue };
        let label = format!(
            "alternate [{}]",
            ranges
                .iter()
                .map(|r| r.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        for m in font.masters.iter().filter(|m| m.id != def.id) {
            if let Some(l) = alt(m) {
                issues.extend(compare(
                    &format!("{} {label}", m.name),
                    alt_ref,
                    &format!("{} {label}", def.name),
                    l,
                ));
            }
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
pub(crate) fn values(layer: &Layer) -> Vec<f64> {
    let mut v = vec![layer.width];
    for s in &layer.shapes {
        match s {
            Shape::Path(p) => p.nodes.iter().for_each(|n| v.extend([n.x, n.y])),
            Shape::Component {
                transform, smart, ..
            } => {
                v.extend(transform.iter());
                v.extend(smart.values());
            }
        }
    }
    let mut anchors: Vec<_> = layer.anchors.iter().collect();
    anchors.sort_by(|a, b| a.name.cmp(&b.name));
    anchors.iter().for_each(|a| v.extend([a.x, a.y]));
    v
}

/// Writes interpolated numbers back into a copy of the template layer.
pub(crate) fn rebuild(template: &Layer, vals: &[f64]) -> Layer {
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
            Shape::Component {
                transform, smart, ..
            } => {
                transform.iter_mut().for_each(|t| *t = next());
                smart.values_mut().for_each(|v| *v = next());
            }
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
    /// Normalized location.
    location: Location,
    /// Location in design units, for bracket ranges.
    design: Location,
    default_id: String,
    master_ids: Vec<String>,
    master_locs: Vec<Location>,
    axis_order: Vec<String>,
}

impl Interpolator {
    /// Prepares interpolation at `location` (design-space units, keyed by axis tag).
    pub fn new(font: &Font, location: &Location) -> Result<Self, String> {
        let default_id = default_master(font).id.clone();
        let master_ids: Vec<String> = font.masters.iter().map(|m| m.id.clone()).collect();
        let master_locs: Vec<Location> = font
            .masters
            .iter()
            .map(|m| normalize(font, &m.location))
            .collect();
        let axis_order: Vec<String> = font.axes.iter().map(|a| a.tag.clone()).collect();
        let model = if font.masters.len() > 1 {
            Some(VariationModel::new(&master_locs, &axis_order)?)
        } else {
            None
        };
        Ok(Self {
            model,
            location: normalize(font, location),
            design: location.clone(),
            default_id,
            master_ids,
            master_locs,
            axis_order,
        })
    }

    /// Interpolates arbitrary per-master numbers (in master order), e.g. kerning values.
    pub fn values(&self, per_master: &[Vec<f64>]) -> Vec<f64> {
        match &self.model {
            Some(m) => m.interpolate(per_master, &self.location),
            None => per_master.first().cloned().unwrap_or_default(),
        }
    }

    /// The glyph's layer at this location, using its bracket (alternate) layers where
    /// their ranges apply and its brace (intermediate) layers as extra sources. Fails if
    /// the layers involved are incompatible. The result is a plain layer whose
    /// `master_id` is the default master's.
    pub fn layer(&self, font: &Font, glyph: &str) -> Result<Layer, String> {
        let g = font
            .glyph(glyph)
            .ok_or_else(|| format!("no glyph named \"{glyph}\""))?;
        let alternate = g
            .bracket_groups()
            .into_iter()
            .find(|r| in_bracket(font, r, &self.design));
        self.layer_design(font, glyph, alternate.as_deref())
    }

    /// The glyph's layer at this location in one particular design: the normal one
    /// (`None`, ignoring bracket layers) or the alternate with these ranges. Variable
    /// fonts need both everywhere, as separate glyphs.
    pub fn layer_design(
        &self,
        font: &Font,
        glyph: &str,
        alternate: Option<&[AxisRange]>,
    ) -> Result<Layer, String> {
        let g = font
            .glyph(glyph)
            .ok_or_else(|| format!("no glyph named \"{glyph}\""))?;
        // (normalized location, layer) for every source of this glyph.
        let mut sources: Vec<(Location, &Layer)> = Vec::new();
        for (id, loc) in self.master_ids.iter().zip(&self.master_locs) {
            let layer = alternate
                .and_then(|r| {
                    g.bracket_layers()
                        .find(|l| &l.master_id == id && l.bracket.as_slice() == r)
                })
                .or_else(|| g.layer(id))
                .ok_or_else(|| format!("\"{glyph}\" is missing a layer"))?;
            sources.push((loc.clone(), layer));
        }
        if alternate.is_none() {
            for l in g.brace_layers() {
                let loc = l.location.as_ref().expect("brace layers have a location");
                sources.push((normalize(font, loc), l));
            }
        }
        let default_index = self
            .master_ids
            .iter()
            .position(|id| *id == self.default_id)
            .unwrap_or(0);
        let template = sources[default_index].1;
        let plain = |l: &Layer| {
            let mut out = l.clone();
            out.master_id = self.default_id.clone();
            out.id = None;
            out.location = None;
            out.bracket.clear();
            out
        };
        if sources.len() == 1 {
            return Ok(plain(template));
        }
        let issues: Vec<String> = sources
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != default_index)
            .flat_map(|(_, (_, l))| {
                let name = l.label().unwrap_or_else(|| "a master".into());
                compare(&name, template, "the default master", l)
            })
            .collect();
        if !issues.is_empty() {
            return Err(format!(
                "\"{glyph}\" can't be interpolated: {}",
                issues.join("; ")
            ));
        }
        let per_source: Vec<Vec<f64>> = sources.iter().map(|(_, l)| values(l)).collect();
        let vals = if sources.len() == self.master_ids.len() {
            match &self.model {
                Some(m) => m.interpolate(&per_source, &self.location),
                None => per_source[0].clone(),
            }
        } else {
            let locs: Vec<Location> = sources.iter().map(|(l, _)| l.clone()).collect();
            VariationModel::new(&locs, &self.axis_order)
                .map_err(|e| format!("\"{glyph}\": {e}"))?
                .interpolate(&per_source, &self.location)
        };
        Ok(plain(&rebuild(template, &vals)))
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
