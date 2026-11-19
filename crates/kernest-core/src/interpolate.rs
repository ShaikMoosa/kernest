//! Master compatibility and interpolating glyph layers at any design-space location.

use crate::designspace::{normalize, Location, VariationModel};
use crate::model::{
    in_bracket, AxisRange, Font, KernPair, Layer, Master, Metrics, NodeKind, Shape,
};

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

/// Adds a master interpolated at `location` (design units): its metrics, a layer for
/// every glyph and its kerning. Cutting a variable font to a range that excludes the
/// old default needs a master at the new default; this builds one.
pub fn add_master_at(font: &Font, location: &Location, name: &str) -> Result<Font, String> {
    let it = Interpolator::new(font, location)?;
    let mut f = font.clone();
    let id = (1..)
        .map(|n| format!("m{n:02}"))
        .find(|id| f.master(id).is_none())
        .expect("ids are unbounded");
    let per = |get: fn(&Metrics) -> f64| {
        let v: Vec<Vec<f64>> = font.masters.iter().map(|m| vec![get(&m.metrics)]).collect();
        it.values(&v)[0]
    };
    let metrics = Metrics {
        ascender: per(|m| m.ascender),
        cap_height: per(|m| m.cap_height),
        x_height: per(|m| m.x_height),
        descender: per(|m| m.descender),
        italic_angle: per(|m| m.italic_angle),
    };
    for g in &mut f.glyphs {
        let mut layer = it.layer(font, &g.name)?;
        layer.master_id = id.clone();
        g.layers.push(layer);
    }
    // Kerning: every pair any master carries, interpolated across the masters (a
    // master without the pair contributes 0).
    let mut pairs: Vec<(String, String)> = Vec::new();
    for k in font.kerning.values() {
        for p in k {
            let key = (p.left.clone(), p.right.clone());
            if !pairs.contains(&key) {
                pairs.push(key);
            }
        }
    }
    let mut kern = Vec::new();
    for (left, right) in pairs {
        let v: Vec<Vec<f64>> = font
            .masters
            .iter()
            .map(|m| {
                let val = font
                    .kerning
                    .get(&m.id)
                    .and_then(|k| k.iter().find(|p| p.left == left && p.right == right))
                    .map_or(0.0, |p| p.value);
                vec![val]
            })
            .collect();
        let value = it.values(&v)[0];
        if value != 0.0 {
            kern.push(KernPair { left, right, value });
        }
    }
    f.kerning.insert(id.clone(), kern);
    let dflt = default_master(font);
    f.masters.push(Master {
        id,
        name: name.into(),
        location: location.clone(),
        metrics,
        guides: Vec::new(),
        alignment_zones: dflt.alignment_zones.clone(),
        stems: dflt.stems.clone(),
    });
    Ok(f)
}

/// Expands every instance into its own interpolated master in a NEW font, so the named
/// styles become editable masters. The source is left untouched; the result keeps the axes
/// and info but carries one master per instance (in instance order) and no instance or
/// variable-font list, since those styles are now masters.
pub fn generate_instances(font: &Font) -> Result<Font, String> {
    if font.instances.is_empty() {
        return Err("This font has no instances to generate.".into());
    }
    let mut out = font.clone();
    out.masters.clear();
    out.kerning.clear();
    out.instances.clear();
    out.variable_fonts.clear();
    for g in &mut out.glyphs {
        g.layers.clear();
    }
    for inst in &font.instances {
        // Interpolate against the SOURCE masters, then move the new master into `out`.
        let grown = add_master_at(font, &inst.location, &inst.name)?;
        let src = grown
            .masters
            .last()
            .expect("add_master_at appends a master");
        let id = format!("m{:02}", out.masters.len() + 1);
        for (source_glyph, out_glyph) in font.glyphs.iter().zip(out.glyphs.iter_mut()) {
            let layer = grown
                .glyphs
                .iter()
                .find(|g| g.name == source_glyph.name)
                .and_then(|g| g.layers.iter().find(|l| l.master_id == src.id))
                .expect("add_master_at layers every glyph");
            let mut layer = layer.clone();
            layer.master_id = id.clone();
            out_glyph.layers.push(layer);
        }
        out.kerning.insert(
            id.clone(),
            grown.kerning.get(&src.id).cloned().unwrap_or_default(),
        );
        let mut master = src.clone();
        master.id = id;
        out.masters.push(master);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{bounds, svg_path};
    use crate::model::{Axis, Instance, Master, Metrics, Node, Path};
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
            map: Vec::new(),
            labels: Vec::new(),
        });
        f.masters[0].location = BTreeMap::from([("wght".into(), 400.0)]);
        f.masters.push(Master {
            id: "m02".into(),
            name: "Bold".into(),
            location: BTreeMap::from([("wght".into(), 700.0)]),
            metrics: Metrics::default(),
            guides: Vec::new(),
            alignment_zones: Vec::new(),
            stems: Vec::new(),
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
                locked: false,
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

    fn inst(name: &str, wght: f64) -> Instance {
        Instance {
            name: name.into(),
            location: BTreeMap::from([("wght".into(), wght)]),
            export: true,
            weight_class: None,
            width_class: None,
            style_link: None,
        }
    }

    #[test]
    fn generate_instances_makes_one_master_per_instance() {
        let mut f = two_masters();
        f.instances = vec![
            inst("Regular", 400.0),
            inst("SemiBold", 550.0),
            inst("Bold", 700.0),
        ];
        let out = generate_instances(&f).unwrap();
        assert_eq!(
            out.masters
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["Regular", "SemiBold", "Bold"]
        );
        assert_eq!(
            out.masters
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["m01", "m02", "m03"]
        );
        assert_eq!(out.masters[1].location["wght"], 550.0);
        assert!(out.instances.is_empty() && out.variable_fonts.is_empty());
        // The source is untouched.
        assert_eq!(f.masters.len(), 2);
        assert_eq!(f.instances.len(), 3);
        for g in &out.glyphs {
            assert_eq!(g.layers.len(), 3, "{}", g.name);
        }
        let h = out.glyph("H").unwrap();
        let width = |id: &str| h.layers.iter().find(|l| l.master_id == id).unwrap().width;
        assert_eq!(width("m01"), 680.0);
        assert_eq!(width("m02"), 705.0);
        assert_eq!(width("m03"), 730.0);
        // Kerning interpolates too: m02 carries no pairs, so SemiBold is halfway to 0.
        let semi = &out.kerning["m02"];
        assert_eq!(
            semi.iter()
                .find(|p| p.left == "T" && p.right == "o")
                .unwrap()
                .value,
            -30.0
        );
        // At the Bold extreme the pair interpolates to 0 and is dropped.
        assert!(out.kerning["m03"].is_empty());
    }

    #[test]
    fn generate_instances_default_instance_matches_source() {
        let mut f = two_masters();
        f.instances = vec![inst("Regular", 400.0)];
        let out = generate_instances(&f).unwrap();
        assert_eq!(out.masters.len(), 1);
        for g in &f.glyphs {
            let og = out.glyph(&g.name).unwrap();
            assert_eq!(
                svg_path(&out, &og.layers[0]),
                svg_path(&f, &g.layers[0]),
                "{}",
                g.name
            );
        }
    }

    #[test]
    fn generate_instances_needs_instances() {
        let f = two_masters();
        assert!(generate_instances(&f).is_err());
    }
}
