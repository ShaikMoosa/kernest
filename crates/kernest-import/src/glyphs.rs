//! Glyphs (`.glyphs`, `.glyphspackage`) import, via glyphs-reader.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use glyphs_reader::{Font as GFont, InstanceType, NodeType, Shape as GShape};
use kernest_core::color::{Color as KColor, Paint};
use kernest_core::designspace::design_to_user;
use kernest_core::{
    Anchor, Axis, AxisRange, Codepoint, Font, FontInfo, Glyph, Instance, KernPair, Layer, Master,
    Metrics, Node, NodeKind, Path as KPath, Shape, SmartAxis, StyleLink,
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

    // Axes: Glyphs only stores master positions (design coordinates), so the design
    // range is what the masters span; "Axis Mappings"/"Axis Location" (already merged
    // by glyphs-reader) give the user range and the avar map.
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
            let mut axis = Axis {
                name: a.name.clone(),
                tag: a.tag.clone(),
                min: vals.iter().copied().fold(dflt, f64::min),
                default: dflt,
                max: vals.iter().copied().fold(dflt, f64::max),
                hidden: a.hidden.unwrap_or(false),
                map: Vec::new(),
                labels: Vec::new(),
            };
            if let Some(m) = g.axis_mappings.get(&a.name) {
                let mut pts: Vec<(f64, f64)> = m.iter().map(|(u, d)| (u.0, d.0)).collect();
                pts.sort_by(|x, y| x.0.total_cmp(&y.0));
                if !pts.iter().all(|(u, d)| u == d) {
                    // The mapping defines the full range; masters may span only part of it.
                    let dmin = pts.iter().map(|(_, d)| *d).fold(axis.min, f64::min);
                    let dmax = pts.iter().map(|(_, d)| *d).fold(axis.max, f64::max);
                    axis.map = pts;
                    axis.min = design_to_user(&axis, dmin);
                    axis.default = design_to_user(&axis, axis.default);
                    axis.max = design_to_user(&axis, dmax);
                }
            }
            axis
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
            guides: Vec::new(),
            alignment_zones: Vec::new(),
            stems: Vec::new(),
            vertical: vertical_metrics(&m.custom_parameters, &g.custom_parameters),
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
        designer_url: g.get_default_name("designerURL").map(String::from),
        manufacturer: g.get_default_name("manufacturers").map(String::from),
        manufacturer_url: g.get_default_name("manufacturerURL").map(String::from),
        license: g.get_default_name("licenses").map(String::from),
        license_url: g.get_default_name("licenseURL").map(String::from),
        description: g.get_default_name("descriptions").map(String::from),
        trademark: g.get_default_name("trademarks").map(String::from),
        sample_text: g.get_default_name("sampleTexts").map(String::from),
        hinting: None,
        ps_hinting: None,
        control_instructions: String::new(),
        grid: None,
        use_typo_metrics: g.custom_parameters.use_typo_metrics.unwrap_or(false),
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
    // (glyph, Glyphs layer id) -> Kernest layer key, for what is read from the source later.
    let mut layer_keys: HashMap<(String, String), String> = HashMap::new();
    // Colour palettes ("Color Palettes"); colours used by full-colour layers that the
    // palettes lack are added to them.
    let mut palettes: Vec<Vec<KColor>> = g
        .custom_parameters
        .color_palettes
        .as_ref()
        .map(|p| {
            p.iter()
                .map(|pal| pal.iter().map(k_color).collect())
                .collect()
        })
        .unwrap_or_default();
    let mut foreground_layers = 0;
    let glyphs: Vec<Glyph> = order
        .iter()
        .map(|name| {
            let src = &g.glyphs[*name];
            let mut out = Glyph::new(
                name.to_string(),
                src.unicode.iter().map(|&u| Codepoint(u)).collect(),
            );
            out.export = src.export;
            out.smart_axes = src
                .smart_component_axes
                .iter()
                .map(|(n, r)| SmartAxis {
                    name: n.to_string(),
                    min: *r.start() as f64,
                    max: *r.end() as f64,
                })
                .collect();
            out.left_group = src.left_kern.as_ref().map(|s| s.to_string());
            out.right_group = src.right_kern.as_ref().map(|s| s.to_string());
            // The reader guesses production names from its glyph database; keep only
            // ones that differ from the glyph's own name, so ordinary glyphs stay clean.
            out.production_name = src
                .production_name
                .as_deref()
                .filter(|p| p != name)
                .map(String::from);
            let owner = |l: &glyphs_reader::Layer| {
                l.associated_master_id
                    .as_deref()
                    .and_then(|m| ids.get(m))
                    .cloned()
                    .unwrap_or_else(|| default_id.clone())
            };
            for l in &src.layers {
                match ids.get(l.layer_id.as_str()) {
                    Some(id) if out.layer(id).is_none() => {
                        out.layers.push(layer(id, l));
                        layer_keys.insert((name.to_string(), l.layer_id.clone()), id.clone());
                    }
                    Some(_) => extra_layers += 1,
                    None if !l.smart_component_positions.is_empty()
                        && !out.smart_axes.is_empty() =>
                    {
                        // Smart pole layer: at the minimum or maximum of each smart axis.
                        let m = owner(l);
                        let mut k = layer(&m, l);
                        k.id = Some(out.new_layer_id(&m));
                        k.smart = Some(pole_position(&out.smart_axes, l));
                        layer_keys
                            .insert((name.to_string(), l.layer_id.clone()), k.key().to_string());
                        out.layers.push(k);
                    }
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
                        layer_keys
                            .insert((name.to_string(), l.layer_id.clone()), k.key().to_string());
                        out.layers.push(k);
                    }
                    None if l.is_color() => {
                        let m = owner(l);
                        let base = layer(&m, l);
                        let paints: Vec<(Paint, Vec<Shape>)> = match l.attributes.color_palette {
                            Some(0xFFFF) => {
                                foreground_layers += 1;
                                continue;
                            }
                            Some(i) => vec![(
                                Paint::Solid {
                                    color: i.clamp(0, 0xFFFE) as u16,
                                    alpha: 1.0,
                                },
                                base.shapes.clone(),
                            )],
                            None => {
                                // A full-colour layer: each shape has its own fill;
                                // neighbouring shapes with the same fill share a layer.
                                let bbox = crate::glyphs_write::point_box(&base.shapes);
                                let mut runs: Vec<(Paint, Vec<Shape>)> = Vec::new();
                                for (gs, ks) in l.shapes.iter().zip(&base.shapes) {
                                    let p = shape_paint(gs.attributes(), bbox, &mut palettes);
                                    match runs.last_mut() {
                                        Some((last, list)) if *last == p => list.push(ks.clone()),
                                        _ => runs.push((p, vec![ks.clone()])),
                                    }
                                }
                                runs
                            }
                        };
                        for (paint, shapes) in paints {
                            let mut k = Layer::new(m.clone(), base.width);
                            k.id = Some(out.new_layer_id(&m));
                            k.shapes = shapes;
                            k.color = Some(kernest_core::color::ColorInfo { paint });
                            out.layers.push(k);
                        }
                    }
                    None => extra_layers += 1,
                }
            }
            // Kernest's master layer is the pole at every axis' minimum; Glyphs may put
            // a master layer at another pole, so swap it with the minimum pole.
            if !out.smart_axes.is_empty() {
                for l in &src.layers {
                    let Some(id) = ids.get(l.layer_id.as_str()) else {
                        continue;
                    };
                    let here = pole_position(&out.smart_axes, l);
                    if out.smart_axes.iter().all(|a| here[&a.name] == a.min) {
                        continue;
                    }
                    let min: BTreeMap<String, f64> = out
                        .smart_axes
                        .iter()
                        .map(|a| (a.name.clone(), a.min))
                        .collect();
                    let Some(pi) = out
                        .layers
                        .iter()
                        .position(|p| &p.master_id == id && p.smart.as_ref() == Some(&min))
                    else {
                        continue;
                    };
                    let mi = out
                        .layers
                        .iter()
                        .position(|p| p.is_master() && &p.master_id == id)
                        .expect("master layer");
                    let (shapes, width, anchors) = (
                        out.layers[pi].shapes.clone(),
                        out.layers[pi].width,
                        out.layers[pi].anchors.clone(),
                    );
                    out.layers[pi].shapes = std::mem::replace(&mut out.layers[mi].shapes, shapes);
                    out.layers[pi].width = std::mem::replace(&mut out.layers[mi].width, width);
                    out.layers[pi].anchors =
                        std::mem::replace(&mut out.layers[mi].anchors, anchors);
                    out.layers[pi].smart = Some(here);
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
                layer_keys.insert((name.to_string(), l.layer_id.clone()), k.key().to_string());
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
    if foreground_layers > 0 {
        warnings.push(format!(
            "{foreground_layers} colour layer{} in the text colour (colorPalette *) {} left out.",
            plural(foreground_layers),
            if foreground_layers == 1 {
                "was"
            } else {
                "were"
            }
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
            weight_class: i.weight_class,
            width_class: i.width_class,
            style_link: if i.is_bold || i.is_italic || i.link_style.is_some() {
                Some(StyleLink {
                    bold: i.is_bold,
                    italic: i.is_italic,
                    base: i.link_style.clone().unwrap_or_default(),
                })
            } else {
                None
            },
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

    let mut font = Font {
        info,
        axes,
        masters,
        instances,
        glyphs,
        kerning,
        features,
        notes: String::new(),
        feature_settings: Default::default(),
        master_palettes: g
            .masters
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let own: Vec<Vec<KColor>> = m
                    .custom_parameters
                    .color_palettes
                    .as_ref()?
                    .iter()
                    .map(|pal| pal.iter().map(k_color).collect())
                    .collect();
                (own != palettes).then(|| (master_id(i), own))
            })
            .collect(),
        palettes,
        filters: Vec::new(),
        variable_fonts: Vec::new(),
        export_presets: Vec::new(),
    };
    let master_ids: HashMap<String, String> = ids
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    match read_metrics_keys(path) {
        Ok(keys) => apply_metrics_keys(&mut font, &keys, &master_ids, &mut warnings),
        Err(e) => warnings.push(format!("Metrics keys couldn't be read: {e}")),
    }
    font.info.hinting = read_hinting(path);
    match read_source_hinting(path) {
        Ok(h) => {
            for m in &mut font.masters {
                let Some(gid) = ids
                    .iter()
                    .find(|(_, v)| *v == &m.id)
                    .map(|(k, _)| k.to_string())
                else {
                    continue;
                };
                if let Some((zones, stems)) = h.masters.get(&gid) {
                    m.alignment_zones = zones.clone();
                    m.stems = stems.clone();
                }
            }
            if let Some(text) = h.control_instructions {
                font.info.control_instructions = text;
            }
            font.info.ps_hinting = h.ps.filter(|p| *p != kernest_core::PsHinting::default());
        }
        Err(e) => warnings.push(format!("Alignment zones and stems couldn't be read: {e}")),
    }
    match read_glyph_dicts(path) {
        Ok(dicts) => {
            if font.glyphs.iter().any(|g| g.is_smart()) {
                restore_smart_components(&mut font, &dicts, &master_ids);
            }
            read_extras(&mut font, &dicts, &layer_keys);
        }
        Err(e) => warnings.push(format!(
            "Guides, backgrounds and tags couldn't be read: {e}"
        )),
    }
    match read_master_guides(path) {
        Ok(guides) => {
            for (gid, list) in guides {
                if let Some(m) = master_ids
                    .get(&gid)
                    .and_then(|id| font.masters.iter_mut().find(|m| &m.id == id))
                {
                    m.guides = list;
                }
            }
        }
        Err(e) => warnings.push(format!("Master guides couldn't be read: {e}")),
    }
    Ok(Imported { font, warnings })
}

fn k_color(c: &glyphs_reader::Color) -> KColor {
    let b = |v: i64| v.clamp(0, 255) as u8;
    KColor {
        r: b(c.r),
        g: b(c.g),
        b: b(c.b),
        a: b(c.a),
    }
}

/// The palette index and opacity for a colour: the first palette colour with the same
/// red, green and blue, else a new colour added to every palette.
fn palette_color(palettes: &mut Vec<Vec<KColor>>, c: &glyphs_reader::Color) -> (u16, f64) {
    let c = k_color(c);
    if let Some(i) = palettes.first().and_then(|p| {
        p.iter()
            .position(|p| (p.r, p.g, p.b) == (c.r, c.g, c.b) && p.a > 0)
    }) {
        let base = f64::from(palettes[0][i].a);
        return (i as u16, (f64::from(c.a) / base).min(1.0));
    }
    let opaque = KColor { a: 255, ..c };
    if palettes.is_empty() {
        palettes.push(Vec::new());
    }
    for p in palettes.iter_mut() {
        p.push(opaque);
    }
    ((palettes[0].len() - 1) as u16, f64::from(c.a) / 255.0)
}

/// A Glyphs full-colour shape's fill as a Kernest paint. Gradient positions are
/// relative to the layer's point box (see `glyphs_write::point_box`).
fn shape_paint(
    attr: &glyphs_reader::ShapeAttributes,
    bbox: Option<(f64, f64, f64, f64)>,
    palettes: &mut Vec<Vec<KColor>>,
) -> Paint {
    let Some(gr) = &attr.gradient else {
        let (color, alpha) = attr
            .fill_color
            .as_ref()
            .map(|c| palette_color(palettes, c))
            .unwrap_or_else(|| palette_color(palettes, &glyphs_reader::Color::rgba(0, 0, 0, 255)));
        return Paint::Solid { color, alpha };
    };
    let (x0, y0, x1, y1) = bbox.unwrap_or((0.0, 0.0, 1.0, 1.0));
    let abs = |v: &[OrderedFloat<f64>]| -> [f64; 2] {
        let (u, w) = (
            v.first().map_or(0.0, |v| v.0),
            v.get(1).map_or(0.0, |v| v.0),
        );
        [x0 + u * (x1 - x0), y0 + w * (y1 - y0)]
    };
    let stops: Vec<kernest_core::color::ColorStop> = gr
        .colors
        .iter()
        .map(|st| {
            let (color, alpha) = palette_color(palettes, &st.color);
            kernest_core::color::ColorStop {
                offset: st.stop_offset.0.clamp(0.0, 1.0),
                color,
                alpha,
            }
        })
        .collect();
    let (start, end) = (abs(&gr.start), abs(&gr.end));
    if gr.style == "circle" {
        let radius = ((end[0] - start[0]).powi(2) + (end[1] - start[1]).powi(2)).sqrt();
        Paint::Radial {
            center: start,
            radius: if radius > 0.0 { radius } else { 1.0 },
            focus: None,
            stops,
        }
    } else {
        Paint::Linear { start, end, stops }
    }
}

/// A point written as `(x,y)` (Glyphs 3) or `"{x, y}"` (Glyphs 2).
fn point(p: &glyphs_reader::Plist) -> Option<(f64, f64)> {
    if let Some(a) = p.as_array() {
        return Some((num(a.first()?)?, num(a.get(1)?)?));
    }
    let s = p
        .as_str()?
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}');
    let mut it = s.split(',').map(|v| v.trim().parse::<f64>());
    Some((it.next()?.ok()?, it.next()?.ok()?))
}

/// A guide from a Glyphs `guides` (3) or `guideLines` (2) entry.
fn guide(d: &glyphs_reader::Plist) -> Option<kernest_core::Guide> {
    let (x, y) = d
        .get("pos")
        .or_else(|| d.get("position"))
        .and_then(point)
        .unwrap_or((0.0, 0.0));
    Some(kernest_core::Guide {
        x,
        y,
        angle: d.get("angle").and_then(num).unwrap_or(0.0),
        name: d.get("name").and_then(|n| n.as_str()).map(String::from),
        locked: d.get("locked").and_then(num).is_some_and(|v| v != 0.0),
    })
}

fn guides_in(d: &glyphs_reader::Plist) -> Vec<kernest_core::Guide> {
    d.get("guides")
        .or_else(|| d.get("guideLines"))
        .and_then(|g| g.as_array())
        .map(|list| list.iter().filter_map(guide).collect())
        .unwrap_or_default()
}

fn anchors_in(d: &glyphs_reader::Plist) -> Vec<Anchor> {
    d.get("anchors")
        .and_then(|a| a.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let name = a.get("name")?.as_str()?.to_string();
                    let (x, y) = a
                        .get("pos")
                        .or_else(|| a.get("position"))
                        .and_then(point)
                        .unwrap_or((0.0, 0.0));
                    Some(Anchor { name, x, y })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What glyphs-reader leaves out: glyph tags, notes and locks, and each layer's
/// guides and background.
fn read_extras(
    font: &mut Font,
    dicts: &HashMap<String, glyphs_reader::Plist>,
    layer_keys: &HashMap<(String, String), String>,
) {
    for g in &mut font.glyphs {
        let Some(d) = dicts.get(&g.name) else {
            continue;
        };
        if let Some(tags) = d.get("tags").and_then(|t| t.as_array()) {
            g.tags = tags
                .iter()
                .filter_map(|t| t.as_str().map(|s| s.trim().to_string()))
                .filter(|t| !t.is_empty())
                .collect();
        }
        if let Some(note) = d.get("note").and_then(|n| n.as_str()) {
            if !note.trim().is_empty() {
                g.note = Some(note.to_string());
            }
        }
        if d.get("locked").and_then(num).is_some_and(|v| v != 0.0) {
            g.locked = true;
        }
        for ld in d.get("layers").and_then(|l| l.as_array()).unwrap_or(&[]) {
            let Some(key) = ld
                .get("layerId")
                .and_then(|i| i.as_str())
                .and_then(|i| layer_keys.get(&(g.name.clone(), i.to_string())))
            else {
                continue;
            };
            let Some(l) = g.layer_mut(key) else {
                continue;
            };
            l.guides = guides_in(ld);
            if let Some(bg) = ld.get("background") {
                let shapes: Vec<Shape> = match bg.get("shapes").and_then(|s| s.as_array()) {
                    Some(list) => list.iter().filter_map(shape_v3).collect(),
                    None => shapes_v2(bg),
                };
                let anchors = anchors_in(bg);
                if !shapes.is_empty() || !anchors.is_empty() {
                    l.background = Some(kernest_core::Background { shapes, anchors });
                }
            }
        }
    }
}

/// Each master's global guides, by Glyphs master id.
fn read_master_guides(path: &Path) -> Result<Vec<(String, Vec<kernest_core::Guide>)>, String> {
    let file = if path.is_dir() {
        path.join("fontinfo.plist")
    } else {
        path.to_path_buf()
    };
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let root =
        glyphs_reader::Plist::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(root
        .get("fontMaster")
        .and_then(|m| m.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|m| Some((m.get("id")?.as_str()?.to_string(), guides_in(m))))
        .collect())
}

/// Line spacing and underline values from a master's custom parameters, falling back
/// on the font's.
fn vertical_metrics(
    m: &glyphs_reader::CustomParameters,
    f: &glyphs_reader::CustomParameters,
) -> kernest_core::VerticalMetrics {
    let i = |a: Option<i64>, b: Option<i64>| a.or(b).map(|v| v as f64);
    let o = |a: Option<OrderedFloat<f64>>, b: Option<OrderedFloat<f64>>| a.or(b).map(|v| v.0);
    kernest_core::VerticalMetrics {
        typo_ascender: i(m.typo_ascender, f.typo_ascender),
        typo_descender: i(m.typo_descender, f.typo_descender),
        typo_line_gap: i(m.typo_line_gap, f.typo_line_gap),
        hhea_ascender: i(m.hhea_ascender, f.hhea_ascender),
        hhea_descender: i(m.hhea_descender, f.hhea_descender),
        hhea_line_gap: i(m.hhea_line_gap, f.hhea_line_gap),
        win_ascent: i(m.win_ascent, f.win_ascent),
        win_descent: i(m.win_descent, f.win_descent).map(f64::abs),
        underline_thickness: o(m.underline_thickness, f.underline_thickness),
        underline_position: o(m.underline_position, f.underline_position),
    }
}

/// Metrics keys found in a Glyphs source: per glyph, the glyph-wide keys and the keys
/// of individual layers (by Glyphs layer id), each as (left, right, width).
type Keys = [Option<String>; 3];
struct GlyphKeys {
    glyph: Keys,
    layers: Vec<(String, Keys)>,
}

/// Glyphs 3 writes `metricLeft`, `metricRight`, `metricWidth`; Glyphs 2 writes
/// `leftMetricsKey`, `rightMetricsKey`, `widthMetricsKey`.
fn keys_of(d: &glyphs_reader::Plist) -> Keys {
    let get = |a: &str, b: &str| {
        d.get(a)
            .or_else(|| d.get(b))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s.starts_with('=') {
                    s.to_string()
                } else {
                    format!("={s}")
                }
            })
    };
    [
        get("metricLeft", "leftMetricsKey"),
        get("metricRight", "rightMetricsKey"),
        get("metricWidth", "widthMetricsKey"),
    ]
}

fn glyph_keys(d: &glyphs_reader::Plist) -> Option<(String, GlyphKeys)> {
    let name = d.get("glyphname")?.as_str()?.to_string();
    let layers = d
        .get("layers")
        .and_then(|l| l.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|l| {
            let id = l.get("layerId")?.as_str()?.to_string();
            let k = keys_of(l);
            k.iter().any(Option::is_some).then_some((id, k))
        })
        .collect();
    Some((
        name,
        GlyphKeys {
            glyph: keys_of(d),
            layers,
        },
    ))
}

/// A layer's position on a smart glyph's axes (Glyphs stores min or max poles).
fn pole_position(axes: &[SmartAxis], l: &glyphs_reader::Layer) -> BTreeMap<String, f64> {
    axes.iter()
        .map(|a| {
            let at_max = l
                .smart_component_positions
                .get(a.name.as_str())
                .is_some_and(|p| format!("{p:?}") == "Max");
            (a.name.clone(), if at_max { a.max } else { a.min })
        })
        .collect()
}

/// Every glyph's dictionary from the source, by name.
fn read_glyph_dicts(path: &Path) -> Result<HashMap<String, glyphs_reader::Plist>, String> {
    let parse = |p: &Path| -> Result<glyphs_reader::Plist, String> {
        let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
        glyphs_reader::Plist::parse(&text).map_err(|e| format!("{}: {e}", p.display()))
    };
    let mut out = HashMap::new();
    let mut add = |d: glyphs_reader::Plist| {
        if let Some(n) = d.get("glyphname").and_then(|n| n.as_str()) {
            out.insert(n.to_string(), d.clone());
        }
    };
    if path.is_dir() {
        let dir = path.join("glyphs");
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
                let p = entry.map_err(|e| e.to_string())?.path();
                if p.extension().is_some_and(|e| e == "glyph") {
                    add(parse(&p)?);
                }
            }
        }
    } else {
        let root = parse(path)?;
        for g in root.get("glyphs").and_then(|g| g.as_array()).unwrap_or(&[]) {
            add(g.clone());
        }
    }
    Ok(out)
}

fn num(p: &glyphs_reader::Plist) -> Option<f64> {
    p.as_f64()
        .or_else(|| p.as_i64().map(|v| v as f64))
        .or_else(|| p.as_str()?.trim().parse().ok())
}

fn piece(d: &glyphs_reader::Plist) -> BTreeMap<String, f64> {
    d.get("piece")
        .and_then(|p| p.as_dict())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.to_string(), num(v)?)))
                .collect()
        })
        .unwrap_or_default()
}

/// A path or component from a Glyphs 3 `shapes` entry.
fn shape_v3(d: &glyphs_reader::Plist) -> Option<Shape> {
    if let Some(base) = d.get("ref").and_then(|r| r.as_str()) {
        let pair = |k: &str, dflt: (f64, f64)| {
            d.get(k)
                .and_then(|v| v.as_array())
                .and_then(|a| Some((num(a.first()?)?, num(a.get(1)?)?)))
                .unwrap_or(dflt)
        };
        let (x, y) = pair("pos", (0.0, 0.0));
        let (sx, sy) = pair("scale", (1.0, 1.0));
        let angle = d.get("angle").and_then(num).unwrap_or(0.0).to_radians();
        let (c, s) = (angle.cos(), angle.sin());
        return Some(Shape::Component {
            base: base.to_string(),
            transform: [sx * c, sx * s, -sy * s, sy * c, x, y],
            smart: piece(d),
            align: true,
        });
    }
    let nodes = d.get("nodes")?.as_array()?;
    let nodes = nodes
        .iter()
        .filter_map(|n| {
            let a = n.as_array()?;
            let (x, y) = (num(a.first()?)?, num(a.get(1)?)?);
            let (kind, smooth) = match a.get(2)?.as_str()? {
                "l" => (NodeKind::Line, false),
                "ls" => (NodeKind::Line, true),
                "c" => (NodeKind::Curve, false),
                "cs" => (NodeKind::Curve, true),
                "q" => (NodeKind::QCurve, false),
                "qs" => (NodeKind::QCurve, true),
                _ => (NodeKind::OffCurve, false),
            };
            Some(Node { x, y, kind, smooth })
        })
        .collect();
    Some(Shape::Path(KPath {
        locked: false,
        closed: d.get("closed").and_then(|c| c.as_i64()).unwrap_or(1) != 0,
        nodes,
    }))
}

/// Paths and components of a Glyphs 2 layer (`paths`, then `components`).
fn shapes_v2(d: &glyphs_reader::Plist) -> Vec<Shape> {
    let mut out = Vec::new();
    for p in d.get("paths").and_then(|p| p.as_array()).unwrap_or(&[]) {
        let nodes = p
            .get("nodes")
            .and_then(|n| n.as_array())
            .unwrap_or(&[])
            .iter()
            .filter_map(|n| {
                let mut it = n.as_str()?.split_whitespace();
                let (x, y) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?);
                let kind = match it.next()? {
                    "LINE" => NodeKind::Line,
                    "CURVE" => NodeKind::Curve,
                    "QCURVE" => NodeKind::QCurve,
                    _ => NodeKind::OffCurve,
                };
                Some(Node {
                    x,
                    y,
                    kind,
                    smooth: it.next() == Some("SMOOTH"),
                })
            })
            .collect();
        out.push(Shape::Path(KPath {
            locked: false,
            closed: p.get("closed").and_then(|c| c.as_i64()).unwrap_or(1) != 0,
            nodes,
        }));
    }
    for c in d
        .get("components")
        .and_then(|c| c.as_array())
        .unwrap_or(&[])
    {
        let Some(base) = c.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let t: Vec<f64> = c
            .get("transform")
            .and_then(|t| t.as_str())
            .map(|s| {
                s.trim_matches(|ch| ch == '{' || ch == '}')
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let transform = if t.len() == 6 {
            [t[0], t[1], t[2], t[3], t[4], t[5]]
        } else {
            kernest_core::IDENTITY
        };
        out.push(Shape::Component {
            base: base.to_string(),
            transform,
            smart: piece(c),
            align: true,
        });
    }
    out
}

/// glyphs-reader turns smart components into outlines; Kernest keeps them smart, so
/// master layers that use a smart glyph are read again from the source.
fn restore_smart_components(
    font: &mut Font,
    dicts: &HashMap<String, glyphs_reader::Plist>,
    master_ids: &HashMap<String, String>,
) {
    let smart: std::collections::HashSet<String> = font
        .glyphs
        .iter()
        .filter(|g| g.is_smart())
        .map(|g| g.name.clone())
        .collect();
    for g in &mut font.glyphs {
        let Some(d) = dicts.get(&g.name) else {
            continue;
        };
        for ld in d.get("layers").and_then(|l| l.as_array()).unwrap_or(&[]) {
            let Some(id) = ld
                .get("layerId")
                .and_then(|i| i.as_str())
                .and_then(|i| master_ids.get(i))
            else {
                continue;
            };
            let shapes: Vec<Shape> = match ld.get("shapes").and_then(|s| s.as_array()) {
                Some(list) => list.iter().filter_map(shape_v3).collect(),
                None => shapes_v2(ld),
            };
            let uses_smart = shapes
                .iter()
                .any(|s| matches!(s, Shape::Component { base, .. } if smart.contains(base)));
            if !uses_smart {
                continue;
            }
            if let Some(l) = g.layer_mut(id) {
                l.shapes = shapes;
            }
        }
    }
}

/// Reads metrics keys straight from the source (glyphs-reader doesn't keep them).
fn read_metrics_keys(path: &Path) -> Result<HashMap<String, GlyphKeys>, String> {
    let parse = |p: &Path| -> Result<glyphs_reader::Plist, String> {
        let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
        glyphs_reader::Plist::parse(&text).map_err(|e| format!("{}: {e}", p.display()))
    };
    let mut out = HashMap::new();
    if path.is_dir() {
        let dir = path.join("glyphs");
        if dir.is_dir() {
            for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
                let p = entry.map_err(|e| e.to_string())?.path();
                if p.extension().is_some_and(|e| e == "glyph") {
                    if let Some((n, k)) = glyph_keys(&parse(&p)?) {
                        out.insert(n, k);
                    }
                }
            }
        }
    } else {
        let root = parse(path)?;
        for g in root.get("glyphs").and_then(|g| g.as_array()).unwrap_or(&[]) {
            if let Some((n, k)) = glyph_keys(g) {
                out.insert(n, k);
            }
        }
    }
    Ok(out)
}

/// Autohinting settings from the "TTFAutohint options" custom parameter (on the font,
/// or else on the first instance that has it).
fn read_hinting(path: &Path) -> Option<kernest_core::Hinting> {
    let file = if path.is_dir() {
        path.join("fontinfo.plist")
    } else {
        path.to_path_buf()
    };
    let root = glyphs_reader::Plist::parse(&std::fs::read_to_string(file).ok()?).ok()?;
    let option = |d: &glyphs_reader::Plist| -> Option<String> {
        d.get("customParameters")?.as_array()?.iter().find_map(|p| {
            (p.get("name")?.as_str()? == "TTFAutohint options")
                .then(|| p.get("value")?.as_str().map(str::to_string))?
        })
    };
    let text =
        option(&root).or_else(|| root.get("instances")?.as_array()?.iter().find_map(option))?;
    parse_ttfautohint_options(&text)
}

/// Reads the ttfautohint options Kernest keeps (`-l`, `-r`, `-x`, `-c` and their long
/// forms) from a command line such as `-l 8 -r 50 -x 14 -D latn`.
pub fn parse_ttfautohint_options(text: &str) -> Option<kernest_core::Hinting> {
    let mut h = kernest_core::Hinting::default();
    let mut found = false;
    let mut words = text.split_whitespace().peekable();
    while let Some(w) = words.next() {
        let (flag, inline) = match w.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ if w.len() > 2 && w.starts_with('-') && !w.starts_with("--") => {
                (w[..2].to_string(), Some(w[2..].to_string()))
            }
            _ => (w.to_string(), None),
        };
        let slot = match flag.as_str() {
            "-l" | "--hinting-range-min" => &mut h.range_min,
            "-r" | "--hinting-range-max" => &mut h.range_max,
            "-x" | "--increase-x-height" => &mut h.increase_x_height,
            "-c" | "--composites" => {
                h.hint_composites = true;
                found = true;
                continue;
            }
            _ => continue,
        };
        let value = inline.or_else(|| words.next().map(str::to_string));
        if let Some(v) = value.and_then(|v| v.trim_matches('"').parse().ok()) {
            *slot = v;
            found = true;
        }
    }
    found.then_some(h)
}

/// Alignment zones, stems and hinting settings read straight from the source
/// (glyphs-reader doesn't keep them). Masters are keyed by Glyphs master id.
struct SourceHinting {
    masters: HashMap<String, (Vec<kernest_core::Zone>, Vec<kernest_core::Stem>)>,
    control_instructions: Option<String>,
    ps: Option<kernest_core::PsHinting>,
}

fn read_source_hinting(path: &Path) -> Result<SourceHinting, String> {
    let file = if path.is_dir() {
        path.join("fontinfo.plist")
    } else {
        path.to_path_buf()
    };
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    let root =
        glyphs_reader::Plist::parse(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let num = |p: Option<&glyphs_reader::Plist>| p.and_then(|p| p.as_f64());
    let v3 = root
        .get(".formatVersion")
        .or_else(|| root.get("formatVersion"))
        .and_then(|p| p.as_i64())
        .is_some_and(|v| v >= 3);
    let mut out = SourceHinting {
        masters: HashMap::new(),
        control_instructions: None,
        ps: None,
    };
    let empty = Vec::new();
    let masters = root
        .get("fontMaster")
        .and_then(|m| m.as_array())
        .unwrap_or(&empty);
    if v3 {
        // Zones are overshoots on the font's metrics; stems are font-wide names
        // with per-master widths, both as parallel arrays.
        let metrics = root
            .get("metrics")
            .and_then(|m| m.as_array())
            .unwrap_or(&empty);
        let stems = root
            .get("stems")
            .and_then(|s| s.as_array())
            .unwrap_or(&empty);
        for m in masters {
            let Some(id) = m.get("id").and_then(|i| i.as_str()) else {
                continue;
            };
            let mut zones = Vec::new();
            if let Some(values) = m.get("metricValues").and_then(|v| v.as_array()) {
                for (metric, value) in metrics.iter().zip(values) {
                    let kind = metric.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if kind == "italic angle" {
                        continue;
                    }
                    let over = num(value.get("over")).unwrap_or(0.0);
                    if over != 0.0 {
                        zones.push(kernest_core::Zone {
                            position: num(value.get("pos")).unwrap_or(0.0),
                            size: over,
                        });
                    }
                }
            }
            let mut stem_list = Vec::new();
            if let Some(values) = m.get("stemValues").and_then(|v| v.as_array()) {
                for (i, (def, value)) in stems.iter().zip(values).enumerate() {
                    let Some(width) = num(Some(value)) else {
                        continue;
                    };
                    if width <= 0.0 {
                        continue;
                    }
                    let horizontal = def.get("horizontal").and_then(|h| h.as_i64()) == Some(1);
                    let name = def
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("{}{i}", if horizontal { "h" } else { "v" }));
                    stem_list.push(kernest_core::Stem {
                        name,
                        horizontal,
                        value: width,
                    });
                }
            }
            out.masters.insert(id.to_string(), (zones, stem_list));
        }
    } else {
        for m in masters {
            let Some(id) = m.get("id").and_then(|i| i.as_str()) else {
                continue;
            };
            let mut zones = Vec::new();
            for z in m
                .get("alignmentZones")
                .and_then(|z| z.as_array())
                .unwrap_or(&empty)
            {
                let Some(s) = z.as_str() else { continue };
                let s = s.trim_matches(['{', '}', ' ']);
                let Some((p, o)) = s.split_once(',') else {
                    continue;
                };
                if let (Ok(position), Ok(size)) = (p.trim().parse(), o.trim().parse()) {
                    if size != 0.0 {
                        zones.push(kernest_core::Zone { position, size });
                    }
                }
            }
            let mut stem_list = Vec::new();
            for (key, horizontal) in [("horizontalStems", true), ("verticalStems", false)] {
                for (i, v) in m
                    .get(key)
                    .and_then(|s| s.as_array())
                    .unwrap_or(&empty)
                    .iter()
                    .enumerate()
                {
                    if let Some(value) = num(Some(v)) {
                        if value > 0.0 {
                            stem_list.push(kernest_core::Stem {
                                name: format!("{}{i}", if horizontal { "h" } else { "v" }),
                                horizontal,
                                value,
                            });
                        }
                    }
                }
            }
            out.masters.insert(id.to_string(), (zones, stem_list));
        }
    }
    // Font-level custom parameters: control instructions and the blue controls.
    // Values arrive as text or numbers depending on the parameter.
    let param = |name: &str| -> Option<String> {
        root.get("customParameters")?
            .as_array()?
            .iter()
            .find_map(|p| {
                if p.get("name")?.as_str()? != name {
                    return None;
                }
                let v = p.get("value")?;
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_f64().map(|n| n.to_string()))
            })
    };
    out.control_instructions = param("TTFAutohint control instructions");
    let mut ps = kernest_core::PsHinting::default();
    let mut any = false;
    for (name, slot) in [
        ("BlueScale", &mut ps.blue_scale),
        ("BlueShift", &mut ps.blue_shift),
        ("BlueFuzz", &mut ps.blue_fuzz),
    ] {
        if let Some(v) = param(name).and_then(|s| s.trim().parse::<f64>().ok()) {
            *slot = v;
            any = true;
        }
    }
    out.ps = any.then_some(ps);
    Ok(out)
}

/// Puts metrics keys on the imported layers: glyph-wide keys on every layer, layer keys
/// on that master's layer. Keys Kernest can't read are left out and counted.
fn apply_metrics_keys(
    font: &mut Font,
    keys: &HashMap<String, GlyphKeys>,
    master_ids: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) {
    let mut skipped = Vec::new();
    for g in &mut font.glyphs {
        let Some(k) = keys.get(&g.name) else { continue };
        let ok = |s: &Option<String>, skipped: &mut Vec<String>| match s {
            Some(v) if kernest_core::metricskeys::validate(v).is_ok() => Some(v.clone()),
            Some(v) => {
                skipped.push(format!("{} {v}", g.name));
                None
            }
            None => None,
        };
        let glyph_wide: Vec<Option<String>> = k.glyph.iter().map(|s| ok(s, &mut skipped)).collect();
        let per_layer: Vec<(String, Vec<Option<String>>)> = k
            .layers
            .iter()
            .filter_map(|(id, ks)| {
                let m = master_ids.get(id)?.clone();
                Some((m, ks.iter().map(|s| ok(s, &mut skipped)).collect()))
            })
            .collect();
        for l in &mut g.layers {
            let own = per_layer
                .iter()
                .find(|(m, _)| l.is_master() && *m == l.master_id)
                .map(|(_, ks)| ks.clone());
            let pick = |i: usize| {
                own.as_ref()
                    .and_then(|o| o[i].clone())
                    .or_else(|| glyph_wide[i].clone())
            };
            l.metrics_keys.left = pick(0);
            l.metrics_keys.right = pick(1);
            l.metrics_keys.width = pick(2);
        }
    }
    if !skipped.is_empty() {
        warnings.push(format!(
            "{} metrics key{} couldn't be read and {} left out: {}",
            skipped.len(),
            plural(skipped.len()),
            if skipped.len() == 1 { "was" } else { "were" },
            crate::list(&skipped)
        ));
    }
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
                    locked: false,
                    closed: p.closed,
                    nodes,
                }));
            }
            GShape::Component(c) => out.shapes.push(Shape::component(
                c.name.to_string(),
                c.transform.as_coeffs(),
            )),
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
