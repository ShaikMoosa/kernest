//! Glyphs (`.glyphs`, `.glyphspackage`) import, via glyphs-reader.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use glyphs_reader::{Font as GFont, InstanceType, NodeType, Shape as GShape};
use kernest_core::{
    Anchor, Axis, AxisRange, Codepoint, Font, FontInfo, Glyph, Instance, KernPair, Layer, Master,
    Metrics, Node, NodeKind, Path as KPath, Shape,
};
use ordered_float::OrderedFloat;

use crate::{master_id, plural, sorted_pairs, Imported};

pub fn import(path: &Path) -> Result<Imported, String> {
    let g = GFont::load(path)
        .map_err(|e| format!("could not read the Glyphs file \"{}\": {e}", path.display()))?;
    if g.masters.is_empty() {
        return Err("the Glyphs file has no masters.".into());
    }
    let mut warnings = Vec::new();

    // Axes: Glyphs only stores master positions, so the range is what the masters span.
    let default = g.masters.get(g.default_master_idx).unwrap_or(&g.masters[0]);
    let axes: Vec<Axis> = g
        .axes
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let vals: Vec<f64> = g
                .masters
                .iter()
                .filter_map(|m| m.axes_values.get(i).map(|v| v.0))
                .collect();
            let dflt = default.axes_values.get(i).map(|v| v.0).unwrap_or(0.0);
            Axis {
                name: a.name.clone(),
                tag: a.tag.clone(),
                min: vals.iter().copied().fold(dflt, f64::min),
                default: dflt,
                max: vals.iter().copied().fold(dflt, f64::max),
                hidden: a.hidden.unwrap_or(false),
            }
        })
        .collect();
    let location = |values: &[_]| -> BTreeMap<String, f64> {
        axes.iter()
            .zip(values)
            .map(|(a, v): (&Axis, &OrderedFloat<f64>)| (a.tag.clone(), v.0))
            .collect()
    };

    let ids: HashMap<&str, String> = g
        .masters
        .iter()
        .enumerate()
        .map(|(i, m)| (m.id.as_str(), master_id(i)))
        .collect();
    let d = Metrics::default();
    let masters: Vec<Master> = g
        .masters
        .iter()
        .enumerate()
        .map(|(i, m)| Master {
            id: master_id(i),
            name: m.name.clone(),
            location: location(&m.axes_values),
            metrics: Metrics {
                ascender: m.ascender().unwrap_or(d.ascender),
                cap_height: m.cap_height().unwrap_or(d.cap_height),
                x_height: m.x_height().unwrap_or(d.x_height),
                descender: m.descender().unwrap_or(d.descender),
                // Glyphs and Kernest both store the slant, positive leaning right.
                italic_angle: m.italic_angle().unwrap_or(0.0),
            },
        })
        .collect();

    let info = FontInfo {
        family_name: g
            .get_default_name("familyNames")
            .unwrap_or("Untitled")
            .to_string(),
        units_per_em: g.units_per_em.max(16),
        version_major: g.version_major.clamp(0, 65535) as u16,
        version_minor: g.version_minor.min(65535) as u16,
        designer: g.get_default_name("designers").map(String::from),
        copyright: g.get_default_name("copyrights").map(String::from),
        vendor_id: g
            .vendor_id()
            .map(String::from)
            .or_else(|| g.custom_parameters.vendor_id.clone()),
    };

    // Glyphs in the file's order, then any the order doesn't mention.
    let mut seen = HashSet::new();
    let order: Vec<&str> = g
        .glyph_order
        .iter()
        .map(|n| n.as_str())
        .chain(g.glyphs.keys().map(|n| n.as_str()))
        .filter(|n| g.glyphs.contains_key(*n) && seen.insert(*n))
        .collect();
    let tags: Vec<String> = g.axes.iter().map(|a| a.tag.clone()).collect();
    let default_id = master_id(g.default_master_idx.min(g.masters.len() - 1));
    let master_locs: Vec<BTreeMap<String, f64>> =
        masters.iter().map(|m| m.location.clone()).collect();
    let (mut extra_layers, mut skipped_braces) = (0, 0);
    let glyphs: Vec<Glyph> = order
        .iter()
        .map(|name| {
            let src = &g.glyphs[*name];
            let mut out = Glyph::new(
                name.to_string(),
                src.unicode.iter().map(|&u| Codepoint(u)).collect(),
            );
            out.export = src.export;
            out.left_group = src.left_kern.as_ref().map(|s| s.to_string());
            out.right_group = src.right_kern.as_ref().map(|s| s.to_string());
            let owner = |l: &glyphs_reader::Layer| {
                l.associated_master_id
                    .as_deref()
                    .and_then(|m| ids.get(m))
                    .cloned()
                    .unwrap_or_else(|| default_id.clone())
            };
            for l in &src.layers {
                match ids.get(l.layer_id.as_str()) {
                    Some(id) if out.layer(id).is_none() => out.layers.push(layer(id, l)),
                    Some(_) => extra_layers += 1,
                    None if !l.attributes.coordinates.is_empty() && !tags.is_empty() => {
                        // Brace layer: its own location on the axes.
                        let loc: BTreeMap<String, f64> = tags
                            .iter()
                            .zip(&l.attributes.coordinates)
                            .map(|(t, v)| (t.clone(), v.0))
                            .collect();
                        if master_locs.contains(&loc)
                            || out
                                .brace_layers()
                                .any(|b| b.location.as_ref() == Some(&loc))
                        {
                            skipped_braces += 1;
                            continue;
                        }
                        let m = owner(l);
                        let mut k = layer(&m, l);
                        k.id = Some(out.new_layer_id(&m));
                        k.location = Some(loc);
                        out.layers.push(k);
                    }
                    None => extra_layers += 1,
                }
            }
            for l in &src.bracket_layers {
                // Bracket layer: axis rules in the font's axis order.
                let ranges: Vec<AxisRange> = tags
                    .iter()
                    .zip(&l.attributes.axis_rules)
                    .filter(|(_, r)| r.min.is_some() || r.max.is_some())
                    .map(|(t, r)| AxisRange {
                        tag: t.clone(),
                        min: r.min.map(|v| v.0),
                        max: r.max.map(|v| v.0),
                    })
                    .collect();
                if ranges.is_empty() {
                    extra_layers += 1;
                    continue;
                }
                let m = owner(l);
                if out
                    .bracket_layers()
                    .any(|b| b.master_id == m && b.bracket == ranges)
                {
                    extra_layers += 1;
                    continue;
                }
                let mut k = layer(&m, l);
                k.id = Some(out.new_layer_id(&m));
                k.bracket = ranges;
                out.layers.push(k);
            }
            out
        })
        .collect();
    if extra_layers > 0 {
        warnings.push(format!(
            "{extra_layers} extra layer{} (not a master, brace or bracket layer) left out.",
            if extra_layers == 1 { " was" } else { "s were" }
        ));
    }
    if skipped_braces > 0 {
        warnings.push(format!(
            "{skipped_braces} brace layer{} sat on a master or another brace layer and {} left out.",
            plural(skipped_braces),
            if skipped_braces == 1 { "was" } else { "were" }
        ));
    }

    // Kerning: Glyphs writes groups as @MMK_L_name (left side) and @MMK_R_name (right side).
    let side = |s: &str| -> String {
        s.strip_prefix("@MMK_L_")
            .or_else(|| s.strip_prefix("@MMK_R_"))
            .map(|g| format!("@{g}"))
            .unwrap_or_else(|| s.to_string())
    };
    let mut kerning = BTreeMap::new();
    for (mid, pairs) in g.kerning_ltr.iter() {
        let Some(id) = ids.get(mid.as_str()) else {
            continue;
        };
        let pairs: Vec<KernPair> = pairs
            .iter()
            .map(|((l, r), v)| KernPair {
                left: side(l),
                right: side(r),
                value: v.0,
            })
            .collect();
        if !pairs.is_empty() {
            kerning.insert(id.clone(), sorted_pairs(pairs));
        }
    }
    if g.kerning_rtl.iter().any(|(_, p)| !p.is_empty()) {
        warnings.push(
            "Right-to-left kerning was left out; Kernest supports left-to-right kerning only."
                .into(),
        );
    }

    let mut variable_settings = 0;
    let instances: Vec<Instance> = g
        .instances
        .iter()
        .filter(|i| {
            let single = i.type_ == InstanceType::Single;
            variable_settings += usize::from(!single);
            single
        })
        .map(|i| Instance {
            name: i.name.clone(),
            location: location(&i.axes_values),
            export: i.active,
        })
        .collect();
    if variable_settings > 0 {
        warnings.push("Variable font export settings were left out.".into());
    }

    let features = g
        .features
        .iter()
        .filter_map(|f| f.str_if_enabled())
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(Imported {
        font: Font {
            info,
            axes,
            masters,
            instances,
            glyphs,
            kerning,
            features,
        },
        warnings,
    })
}

fn layer(id: &str, l: &glyphs_reader::Layer) -> Layer {
    let mut out = Layer::new(id, l.width.0);
    for s in &l.shapes {
        match s {
            GShape::Path(p) => {
                let nodes = p
                    .nodes
                    .iter()
                    .map(|n| {
                        let (kind, smooth) = match n.node_type {
                            NodeType::Line => (NodeKind::Line, false),
                            NodeType::LineSmooth => (NodeKind::Line, true),
                            NodeType::Curve => (NodeKind::Curve, false),
                            NodeType::CurveSmooth => (NodeKind::Curve, true),
                            NodeType::QCurve => (NodeKind::QCurve, false),
                            NodeType::QCurveSmooth => (NodeKind::QCurve, true),
                            NodeType::OffCurve => (NodeKind::OffCurve, false),
                        };
                        Node {
                            x: n.pt.x,
                            y: n.pt.y,
                            kind,
                            smooth,
                        }
                    })
                    .collect();
                out.shapes.push(Shape::Path(KPath {
                    closed: p.closed,
                    nodes,
                }));
            }
            GShape::Component(c) => out.shapes.push(Shape::Component {
                base: c.name.to_string(),
                transform: c.transform.as_coeffs(),
            }),
        }
    }
    for a in &l.anchors {
        out.anchors.push(Anchor {
            name: a.name.to_string(),
            x: a.pos.x,
            y: a.pos.y,
        });
    }
    out
}
