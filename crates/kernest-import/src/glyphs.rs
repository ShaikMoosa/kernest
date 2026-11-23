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
        custom_parameters: Vec::new(),
        virtual_masters: Vec::new(),
        numbers: BTreeMap::new(),
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
    // Right-to-left pairs (kerningRTL) join the same list, in the order Glyphs stores
    // them: fontc and glyphsLib compile them that way, flipping only the group sides,
    // which Kernest's `@group` names don't record. An RTL pair wins over the same LTR one.
    let mut kerning = BTreeMap::new();
    for (mid, ltr) in g.kerning_ltr.iter() {
        let Some(id) = ids.get(mid.as_str()) else {
            continue;
        };
        let pairs: Vec<KernPair> = ltr
            .iter()
            .map(|((l, r), v)| KernPair {
                left: side(l),
                right: side(r),
                value: v.0,
            })
            .collect();
        if !pairs.is_empty() {
            kerning.insert(id.clone(), pairs);
        }
    }
    for (mid, rtl) in g.kerning_rtl.iter() {
        let Some(id) = ids.get(mid.as_str()) else {
            continue;
        };
        let list: &mut Vec<KernPair> = kerning.entry(id.clone()).or_default();
        for ((l, r), v) in rtl.iter() {
            let (left, right) = (side(l), side(r));
            list.retain(|p| !(p.left == left && p.right == right));
            list.push(KernPair {
                left,
                right,
                value: v.0,
            });
        }
    }
    let kerning: BTreeMap<String, Vec<KernPair>> = kerning
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k, sorted_pairs(v)))
        .collect();

    // Variable font settings are read from the source later (variable_settings_from).
    let instances: Vec<Instance> = g
        .instances
        .iter()
        .filter(|i| i.type_ == InstanceType::Single)
        .map(|i| Instance {
            processing: Default::default(),
            feature_parameters: Vec::new(),
            names: Default::default(),
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
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
            disabled_masters: Vec::new(),
            family_name: None,
            localized_family_names: Default::default(),
        })
        .collect();

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
            let skipped = read_extras(&mut font, &dicts, &layer_keys, &master_ids);
            if skipped > 0 {
                warnings.push(format!(
                    "{skipped} cap or segment component{} from Glyphs {} left out (Kernest places corner components only from Glyphs files).",
                    plural(skipped),
                    if skipped == 1 { "was" } else { "were" }
                ));
            }
        }
        Err(e) => warnings.push(format!(
            "Guides, backgrounds and tags couldn't be read: {e}"
        )),
    }
    match read_root(path) {
        Ok(root) => {
            for m in root
                .get("fontMaster")
                .and_then(|m| m.as_array())
                .unwrap_or(&[])
            {
                let Some(gid) = m.get("id").and_then(|i| i.as_str()) else {
                    continue;
                };
                if let Some(km) = master_ids
                    .get(gid)
                    .and_then(|id| font.masters.iter_mut().find(|k| &k.id == id))
                {
                    km.guides = guides_in(m);
                }
            }
            font.info.custom_parameters = custom_parameters(&root);
            font.variable_fonts = variable_settings_from(&root, &font, &mut warnings);
            read_virtual_masters(&root, &mut font);
            read_numbers(&root, &mut font, &master_ids);
            read_instance_swaps(&root, &mut font, &mut warnings);
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
        show_measurement: d
            .get("showMeasurement")
            .and_then(num)
            .is_some_and(|v| v != 0.0),
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
    master_ids: &HashMap<String, String>,
) -> usize {
    let mut skipped_parts = 0;
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
        if let Some(dir) = d.get("direction").and_then(|v| v.as_str()) {
            g.direction = kernest_core::WritingDirection::parse(dir);
        }
        g.color_label = d.get("color").and_then(color_label);
        // Properties the source sets itself (glyphs-reader fills in glyph data too, so
        // read them from the file to tell overrides apart). Bad values are skipped.
        for key in kernest_core::glyphprops::PROPERTIES {
            let Some(v) = d.get(key).and_then(|v| v.as_str()) else {
                continue;
            };
            if let Ok(Some(v)) = kernest_core::glyphprops::normalize(key, v) {
                match *key {
                    "category" => g.category_override = Some(v),
                    "subCategory" => g.subcategory_override = Some(v),
                    "case" => g.case_override = Some(v),
                    "script" => g.script_override = Some(v),
                    _ => g.sort_name = Some(v),
                }
            }
        }
        for ld in d.get("layers").and_then(|l| l.as_array()).unwrap_or(&[]) {
            let lid = ld.get("layerId").and_then(|i| i.as_str()).unwrap_or("");
            // sbix ("iColor 128") and SVG layers carry a picture, not an outline.
            if let Some(p) = picture_layer(ld) {
                match p {
                    Picture::Bitmap(b) => {
                        if !g.bitmaps.iter().any(|x| x.ppem == b.ppem) {
                            g.bitmaps.push(b);
                        }
                    }
                    Picture::Svg(svg) => {
                        g.svg.get_or_insert(svg);
                    }
                }
                continue;
            }
            let Some(key) = layer_keys.get(&(g.name.clone(), lid.to_string())) else {
                if let Some(b) = backup_layer(ld, master_ids) {
                    let mut b = b;
                    b.id = Some(g.new_layer_id(&b.master_id));
                    g.layers.push(b);
                }
                continue;
            };
            let Some(l) = g.layer_mut(key) else {
                continue;
            };
            l.color_label = ld.get("color").and_then(color_label);
            skipped_parts += restore_corners(l, ld);
            restore_strokes(l, ld);
            restore_masks(l, ld);
            l.annotations = annotations_in(ld);
            l.image = image_in(ld);
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
    skipped_parts
}

/// A layer glyphs-reader leaves out as a draft (a backup such as "legacy" or a dated
/// copy): one tied to a master that isn't a colour, intermediate, alternate or smart
/// layer. `None` for anything else.
fn backup_layer(ld: &glyphs_reader::Plist, master_ids: &HashMap<String, String>) -> Option<Layer> {
    let master = master_ids.get(ld.get("associatedMasterId")?.as_str()?)?;
    let name = ld
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if let Some(attr) = ld.get("attr") {
        if ["color", "colorPalette", "coordinates", "axisRules"]
            .iter()
            .any(|k| attr.get(k).is_some())
        {
            return None;
        }
    }
    let smart = ld.get("partSelection").is_some()
        || ld
            .get("userData")
            .and_then(|u| u.get("PartSelection"))
            .is_some();
    // Glyphs 2 marks intermediate, alternate and colour layers by their names.
    let v2_special = name.contains('{')
        || name.contains('[')
        || name.contains(']')
        || name.starts_with("Color ")
        || name == "Color";
    if smart || v2_special {
        return None;
    }
    let width = ld.get("width").and_then(num).unwrap_or(0.0);
    let mut l = Layer::new(master.clone(), width);
    l.shapes = match ld.get("shapes").and_then(|s| s.as_array()) {
        Some(list) => list.iter().filter_map(shape_v3).collect(),
        None => shapes_v2(ld),
    };
    l.anchors = anchors_in(ld);
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
    l.backup = Some(if name.is_empty() {
        "Backup".into()
    } else {
        name
    });
    Some(l)
}

/// Glyphs corner components as live Kernest corners: glyphs-reader has already
/// drawn them into the outline, so a layer with corner hints takes its own paths
/// back from the source and gets the hints as parts. Returns how many cap or
/// segment hints were left out.
fn restore_corners(l: &mut Layer, ld: &glyphs_reader::Plist) -> usize {
    let Some(hints) = ld.get("hints").and_then(|h| h.as_array()) else {
        return 0;
    };
    let kind = |h: &glyphs_reader::Plist| h.get("type").and_then(|t| t.as_str()).map(String::from);
    let others = hints
        .iter()
        .filter(|h| matches!(kind(h).as_deref(), Some("Cap") | Some("Segment")))
        .count();
    let corners: Vec<&glyphs_reader::Plist> = hints
        .iter()
        .filter(|h| kind(h).as_deref() == Some("Corner"))
        .collect();
    if corners.is_empty() {
        return others;
    }
    // The layer's own shapes, before glyphs-reader inserted the corners.
    let (raw, v3): (Vec<Shape>, bool) = match ld.get("shapes").and_then(|s| s.as_array()) {
        Some(list) => (list.iter().filter_map(shape_v3).collect(), true),
        None => (shapes_v2(ld), false),
    };
    let raw_shapes = ld.get("shapes").and_then(|s| s.as_array());
    let pair =
        |h: &glyphs_reader::Plist, key: &str| -> Option<(f64, f64)> { h.get(key).and_then(point) };
    let mut parts = Vec::new();
    for h in corners {
        let Some(name) = h.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let Some((si, ni)) = pair(h, "origin") else {
            continue;
        };
        let (si, ni) = (si as usize, ni as usize);
        // Glyphs 3 counts paths and components together; Kernest counts paths.
        let path = if v3 {
            let Some(list) = raw_shapes else { continue };
            if list.get(si).and_then(|s| s.get("ref")).is_some() {
                continue;
            }
            list[..si.min(list.len())]
                .iter()
                .filter(|s| s.get("ref").is_none())
                .count()
        } else {
            si
        };
        let mut part = kernest_core::parts::PathPart::new(
            kernest_core::parts::PartKind::Corner,
            name,
            path,
            ni,
        );
        if let Some((x, y)) = pair(h, "scale") {
            part.scale = [x, y];
        }
        part.align = match h.get("options").and_then(num).map(|v| v as i64) {
            Some(1) => kernest_core::parts::CornerAlign::In,
            Some(2) => kernest_core::parts::CornerAlign::Middle,
            Some(4) => kernest_core::parts::CornerAlign::Unaligned,
            _ => kernest_core::parts::CornerAlign::Out,
        };
        parts.push(part);
    }
    if !parts.is_empty() {
        // Keep the components Kernest already has (smart ones restored), take the
        // paths from the source.
        let comps: Vec<Shape> = l
            .shapes
            .iter()
            .filter(|s| matches!(s, Shape::Component { .. }))
            .cloned()
            .collect();
        let mut shapes: Vec<Shape> = raw
            .into_iter()
            .filter(|s| matches!(s, Shape::Path(_)))
            .collect();
        shapes.extend(comps);
        l.shapes = shapes;
        l.parts = parts;
    }
    others
}

/// Puts back the stroke settings Kernest keeps in a layer's userData (see
/// `glyphs_write::stroke_data`), by path number.
fn restore_strokes(l: &mut Layer, ld: &glyphs_reader::Plist) {
    if let Some(curves) = ld
        .get("userData")
        .and_then(|u| u.get(crate::glyphs_write::HOI_KEY))
        .and_then(|v| v.as_str())
        .and_then(|t| serde_json::from_str::<Vec<kernest_core::hoi::HoiCurve>>(t).ok())
    {
        l.hoi = curves;
    }
    let Some(text) = ld
        .get("userData")
        .and_then(|u| u.get(crate::glyphs_write::STROKE_KEY))
        .and_then(|v| v.as_str())
    else {
        return;
    };
    let Ok(list) = serde_json::from_str::<Vec<(usize, kernest_core::stroke::PathStroke)>>(text)
    else {
        return;
    };
    let mut paths: Vec<&mut KPath> = l
        .shapes
        .iter_mut()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .collect();
    for (i, s) in list {
        if let Some(p) = paths.get_mut(i) {
            p.stroke = Some(s);
        }
    }
}

/// "Rename Glyphs" values (`a=b` strings) as swap pairs.
fn swaps_in(p: &glyphs_reader::Plist) -> Vec<[String; 2]> {
    p.get("value")
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|x| {
            let (a, b) = x.as_str()?.split_once('=')?;
            Some([a.trim().to_string(), b.trim().to_string()])
        })
        .filter(|[a, b]| !a.is_empty() && !b.is_empty())
        .collect()
}

/// Language-tagged names from a Glyphs list of `{language; value}` entries.
fn localized_names(list: &glyphs_reader::Plist) -> BTreeMap<String, String> {
    list.as_array()
        .unwrap_or(&[])
        .iter()
        .filter_map(|e| {
            let lang = e.get("language").and_then(|l| l.as_str())?.trim();
            let value = e.get("value").and_then(|v| v.as_str())?.trim();
            (!lang.is_empty() && !value.is_empty()).then(|| (lang.to_string(), value.to_string()))
        })
        .collect()
}

/// A naming property's value: its `value`, or the default (else English, else first)
/// of its localized `values`.
fn property_value(prop: &glyphs_reader::Plist) -> Option<String> {
    if let Some(v) = prop.get("value").and_then(|v| v.as_str()) {
        return Some(v.trim().to_string()).filter(|v| !v.is_empty());
    }
    split_family_names(localized_names(prop.get("values")?)).0
}

/// Splits localized family names into the default name ("dflt", else English) and
/// the other languages.
fn split_family_names(
    mut names: BTreeMap<String, String>,
) -> (Option<String>, BTreeMap<String, String>) {
    let default = names
        .remove("dflt")
        .or_else(|| names.remove("ENG"))
        .or_else(|| names.values().next().cloned());
    (default, names)
}

/// A Glyphs list parameter's strings (`value = (a, b);`).
fn string_list(p: &glyphs_reader::Plist) -> Vec<String> {
    p.get("value")
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|x| x.as_str().map(String::from))
        .collect()
}

/// Whether a Glyphs boolean parameter is off (`value = 0;`).
fn is_off(p: &glyphs_reader::Plist) -> bool {
    match p.get("value") {
        Some(v) => v.as_str().map_or_else(
            || num(v) == Some(0.0),
            |s| s.trim() == "0" || s.eq_ignore_ascii_case("false"),
        ),
        None => false,
    }
}

/// Whether a Glyphs boolean parameter is on (`value = 1;`).
fn is_on(p: &glyphs_reader::Plist) -> bool {
    p.get("value").is_some() && !is_off(p)
}

/// A feature code parameter (SMO-23) from a Glyphs custom parameter, keeping its
/// switched-off state: list values joined with commas, others as written.
fn feature_parameter(p: &glyphs_reader::Plist, name: &str) -> kernest_core::CustomParameter {
    let value = match p.get("value") {
        Some(v) => match v.as_array() {
            Some(items) => items
                .iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            None => v.as_str().unwrap_or_default().trim().to_string(),
        },
        None => String::new(),
    };
    kernest_core::CustomParameter {
        name: name.to_string(),
        value,
        disabled: p.get("disabled").and_then(num).is_some_and(|v| v != 0.0),
    }
}

/// The feature code parameters among a Glyphs instance's custom parameters.
fn feature_parameters_of(inst: &glyphs_reader::Plist) -> Vec<kernest_core::CustomParameter> {
    inst.get("customParameters")
        .and_then(|p| p.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| {
            let name = p.get("name").and_then(|n| n.as_str())?;
            kernest_core::featureedits::is_feature_parameter(name)
                .then(|| feature_parameter(p, name))
        })
        .collect()
}

/// Per-instance settings Glyphs keeps outside the parsed instance: "Rename Glyphs",
/// "Remove Glyphs" and the instance's own family names (the `familyNames` property in
/// Glyphs 3; the `familyName` and `Localized Family Names` parameters in Glyphs 2).
/// Static instances are matched by position, since several can share a name (one
/// "Bold" per family).
fn read_instance_swaps(root: &glyphs_reader::Plist, font: &mut Font, warnings: &mut Vec<String>) {
    let font_family = font.info.family_name.clone();
    let statics = root
        .get("instances")
        .and_then(|i| i.as_array())
        .unwrap_or(&[])
        .iter()
        .filter(|inst| inst.get("type").and_then(|t| t.as_str()) != Some("variable"));
    for (inst, target) in statics.zip(font.instances.iter_mut()) {
        if inst.get("name").and_then(|n| n.as_str()) != Some(target.name.as_str()) {
            continue;
        }
        let mut names = BTreeMap::new();
        for prop in inst
            .get("properties")
            .and_then(|p| p.as_array())
            .unwrap_or(&[])
        {
            let key = prop.get("key").and_then(|k| k.as_str());
            if key == Some("familyNames") {
                if let Some(v) = prop.get("values") {
                    names.extend(localized_names(v));
                }
                if let Some(v) = prop.get("value").and_then(|v| v.as_str()) {
                    names.insert("dflt".into(), v.trim().to_string());
                }
            }
            // Glyphs 3 keeps the other naming overrides as properties too.
            let slot = match key {
                Some("postscriptFontName") => &mut target.names.postscript_font_name,
                Some("postscriptFullNames" | "postscriptFullName") => &mut target.names.full_name,
                Some("compatibleFullNames" | "compatibleFullName") => {
                    &mut target.names.compatible_full_name
                }
                Some("styleMapFamilyNames" | "styleMapFamilyName") => {
                    &mut target.names.style_map_family_name
                }
                _ => continue,
            };
            *slot = property_value(prop);
        }
        for p in inst
            .get("customParameters")
            .and_then(|p| p.as_array())
            .unwrap_or(&[])
        {
            if p.get("disabled").and_then(num).is_some_and(|v| v != 0.0) {
                continue;
            }
            match p.get("name").and_then(|n| n.as_str()) {
                Some("familyName") => {
                    if let Some(v) = p.get("value").and_then(|v| v.as_str()) {
                        names.insert("dflt".into(), v.trim().to_string());
                    }
                }
                Some("Localized Family Names") => {
                    if let Some(v) = p.get("value") {
                        for (lang, name) in localized_names(v) {
                            names.entry(lang).or_insert(name);
                        }
                    }
                }
                _ => {}
            }
        }
        names.retain(|_, v| !v.is_empty());
        let (family, others) = split_family_names(names);
        target.family_name = family.filter(|f| *f != font_family || !others.is_empty());
        target.localized_family_names = others;
        target.feature_parameters = feature_parameters_of(inst);
        for p in inst
            .get("customParameters")
            .and_then(|p| p.as_array())
            .unwrap_or(&[])
        {
            if p.get("disabled").and_then(num).is_some_and(|v| v != 0.0) {
                continue;
            }
            let text = || {
                p.get("value")
                    .and_then(|v| v.as_str())
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
            };
            match p.get("name").and_then(|n| n.as_str()) {
                Some("postscriptFontName") => target.names.postscript_font_name = text(),
                Some("postscriptFullName") => target.names.full_name = text(),
                Some("compatibleFullName") => target.names.compatible_full_name = text(),
                Some("styleMapFamilyName") => target.names.style_map_family_name = text(),
                Some("fileName") => target.names.file_name = text(),
                Some("Name Table Entry") => {
                    match kernest_core::NameRecord::parse(&text().unwrap_or_default()) {
                        Ok(r) => target.names.records.push(r),
                        Err(e) => warnings.push(format!("{}: Name Table Entry: {e}", target.name)),
                    }
                }
                Some("Rename Glyphs") => target.rename_glyphs = swaps_in(p),
                Some("Disable Masters") => {
                    target.disabled_masters = p
                        .get("value")
                        .and_then(|v| v.as_array())
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                }
                Some("Remove Glyphs") => {
                    target.remove_glyphs = p
                        .get("value")
                        .and_then(|v| v.as_array())
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                }
                // Glyph processing (SMO-28).
                Some("Keep Glyphs") => target.processing.keep_glyphs = string_list(p),
                Some("Decompose Glyphs") => target.processing.decompose_glyphs = string_list(p),
                Some("Filter") => target.processing.filters.extend(text()),
                Some("PreFilter") => target.processing.pre_filters.extend(text()),
                Some("Remove Overlap" | "Remove overlap" | "removeOverlap") => {
                    target.processing.keep_overlaps = is_off(p)
                }
                _ => {}
            }
        }
    }
}

/// The "Virtual Master" parameter: locations by axis name. The axes widen to take
/// them in, since Glyphs takes axis ranges from the masters.
/// Number values (S64): the font's `numbers` names and each master's `numberValues`.
fn read_numbers(
    root: &glyphs_reader::Plist,
    font: &mut Font,
    master_ids: &HashMap<String, String>,
) {
    let names: Vec<String> = root
        .get("numbers")
        .and_then(|n| n.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|n| n.get("name").and_then(|v| v.as_str()).map(str::to_string))
        .collect();
    if names.is_empty() {
        return;
    }
    for m in root
        .get("fontMaster")
        .and_then(|m| m.as_array())
        .unwrap_or(&[])
    {
        let Some(id) = m
            .get("id")
            .and_then(|i| i.as_str())
            .and_then(|g| master_ids.get(g))
        else {
            continue;
        };
        let values: Vec<f64> = m
            .get("numberValues")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().map(|v| num(v).unwrap_or(0.0)).collect())
            .unwrap_or_default();
        for (i, name) in names.iter().enumerate() {
            font.info
                .numbers
                .entry(name.clone())
                .or_default()
                .insert(id.clone(), values.get(i).copied().unwrap_or(0.0));
        }
    }
}

fn read_virtual_masters(root: &glyphs_reader::Plist, font: &mut Font) {
    let params = root
        .get("customParameters")
        .and_then(|p| p.as_array())
        .unwrap_or(&[]);
    let mut n = 0;
    for p in params {
        if p.get("name").and_then(|x| x.as_str()) != Some("Virtual Master") {
            continue;
        }
        let mut location = BTreeMap::new();
        for entry in p.get("value").and_then(|v| v.as_array()).unwrap_or(&[]) {
            let (Some(axis), Some(v)) = (
                entry.get("Axis").and_then(|a| a.as_str()),
                entry.get("Location").and_then(num),
            ) else {
                continue;
            };
            if let Some(a) = font
                .axes
                .iter_mut()
                .find(|a| a.name == axis || a.tag == axis)
            {
                location.insert(a.tag.clone(), v);
                let user = if a.map.is_empty() {
                    v
                } else {
                    design_to_user(a, v)
                };
                a.min = a.min.min(user);
                a.max = a.max.max(user);
            }
        }
        if location.is_empty() {
            continue;
        }
        n += 1;
        font.info.virtual_masters.push(kernest_core::VirtualMaster {
            name: format!("Virtual master {n}"),
            location,
        });
    }
}

/// Glyphs path attributes `mask = 1`: the masks of a layer, by path number.
fn restore_masks(l: &mut Layer, ld: &glyphs_reader::Plist) {
    let Some(list) = ld.get("shapes").and_then(|s| s.as_array()) else {
        return;
    };
    let masks: Vec<bool> = list
        .iter()
        .filter(|s| s.get("ref").is_none())
        .map(|s| {
            s.get("attr")
                .and_then(|a| a.get("mask"))
                .and_then(num)
                .is_some_and(|v| v != 0.0)
        })
        .collect();
    if !masks.contains(&true) {
        return;
    }
    for (p, m) in l
        .shapes
        .iter_mut()
        .filter_map(|s| match s {
            Shape::Path(p) => Some(p),
            _ => None,
        })
        .zip(masks)
    {
        p.mask = m;
    }
}

/// A layer's annotations (Glyphs 3 names the type; Glyphs 2 numbers it).
fn annotations_in(ld: &glyphs_reader::Plist) -> Vec<kernest_core::Annotation> {
    use kernest_core::AnnotationKind as K;
    ld.get("annotations")
        .and_then(|a| a.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|a| {
            let kind = match a.get("type") {
                Some(t) => match t.as_str().map(str::to_ascii_lowercase).as_deref() {
                    Some("text") => K::Text,
                    Some("arrow") => K::Arrow,
                    Some("circle") => K::Circle,
                    Some("plus") => K::Plus,
                    Some("minus") => K::Minus,
                    _ => match num(t).map(|v| v as i64) {
                        Some(1) => K::Text,
                        Some(2) => K::Arrow,
                        Some(3) => K::Circle,
                        Some(4) => K::Plus,
                        Some(5) => K::Minus,
                        _ => return None,
                    },
                },
                None => K::Text,
            };
            let (x, y) = a
                .get("pos")
                .or_else(|| a.get("position"))
                .and_then(point)
                .unwrap_or((0.0, 0.0));
            Some(kernest_core::Annotation {
                kind,
                x,
                y,
                text: a
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                angle: a.get("angle").and_then(num).unwrap_or(0.0),
                size: a.get("width").and_then(num).unwrap_or(0.0),
            })
        })
        .collect()
}

enum Picture {
    Bitmap(kernest_core::GlyphBitmap),
    Svg(kernest_core::GlyphSvg),
}

/// An sbix layer (`attr.sbixSize`, or a Glyphs 2 name like "iColor 128") or an SVG
/// layer (named "svg"), with its picture.
fn picture_layer(ld: &glyphs_reader::Plist) -> Option<Picture> {
    let name = ld.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
    let size = ld
        .get("attr")
        .and_then(|a| a.get("sbixSize"))
        .and_then(num)
        .or_else(|| {
            name.strip_prefix("iColor")
                .and_then(|r| r.trim().parse::<f64>().ok())
        });
    let svg = name.eq_ignore_ascii_case("svg");
    if size.is_none() && !svg {
        return None;
    }
    let img = image_in(ld)?;
    if svg || img.path.to_ascii_lowercase().ends_with(".svg") {
        return Some(Picture::Svg(kernest_core::GlyphSvg {
            path: img.path,
            x: img.x,
            y: img.y,
            scale: img.scale_x,
        }));
    }
    let ppem = size?.round().clamp(1.0, 2048.0) as u16;
    Some(Picture::Bitmap(kernest_core::GlyphBitmap {
        ppem,
        path: img.path,
        x: img.x,
        y: img.y,
    }))
}

/// A layer's `backgroundImage`: path, transform (or pos/scale/angle) and crop.
fn image_in(ld: &glyphs_reader::Plist) -> Option<kernest_core::LayerImage> {
    let bi = ld.get("backgroundImage")?;
    let path = bi.get("imagePath").and_then(|p| p.as_str())?.to_string();
    let nums = |v: &glyphs_reader::Plist| -> Vec<f64> {
        match v.as_array() {
            Some(a) => a.iter().filter_map(num).collect(),
            None => v
                .as_str()
                .unwrap_or("")
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
                .filter_map(|x| x.parse().ok())
                .collect(),
        }
    };
    let (mut x, mut y, mut sx, mut sy, mut angle) = (0.0, 0.0, 1.0, 1.0, 0.0);
    if let Some(t) = bi.get("transform").map(nums).filter(|t| t.len() == 6) {
        sx = t[0].hypot(t[1]);
        sy = t[2].hypot(t[3]);
        angle = t[1].atan2(t[0]).to_degrees();
        x = t[4];
        y = t[5];
    }
    if let Some((px, py)) = bi.get("pos").and_then(point) {
        x = px;
        y = py;
    }
    if let Some(s) = bi.get("scale").map(nums) {
        if s.len() == 2 {
            sx = s[0];
            sy = s[1];
        }
    }
    if let Some(a) = bi.get("angle").and_then(num) {
        angle = a;
    }
    let crop = bi
        .get("crop")
        .map(nums)
        .filter(|c| c.len() == 4 && c[2] > 0.0 && c[3] > 0.0)
        .map(|c| [c[0], c[1], c[2], c[3]]);
    Some(kernest_core::LayerImage {
        path,
        x,
        y,
        scale_x: if sx == 0.0 { 1.0 } else { sx },
        scale_y: if sy == 0.0 { 1.0 } else { sy },
        angle,
        crop,
        locked: bi.get("locked").and_then(num).is_some_and(|v| v != 0.0),
    })
}

/// The font-level dictionary of a `.glyphs` file (or a package's `fontinfo.plist`).
fn read_root(path: &Path) -> Result<glyphs_reader::Plist, String> {
    let file = if path.is_dir() {
        path.join("fontinfo.plist")
    } else {
        path.to_path_buf()
    };
    let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
    glyphs_reader::Plist::parse(&text).map_err(|e| format!("{}: {e}", file.display()))
}

/// Font custom parameters Kernest reads into its own settings; every other one is
/// kept as it is in Font Info › Custom parameters.
pub(crate) const HANDLED_PARAMETERS: &[&str] = &[
    "glyphOrder",
    "Variable Font Origin",
    "Variable Font Family Name",
    "Variable Font File Name",
    "vendorID",
    "openTypeOS2VendorID",
    "TTFAutohint options",
    "TTFAutohint control instructions",
    "BlueScale",
    "BlueShift",
    "BlueFuzz",
    "Axis Mappings",
    "Axis Location",
    "Use Typo Metrics",
    "Color Palettes",
    "typoAscender",
    "typoDescender",
    "typoLineGap",
    "hheaAscender",
    "hheaDescender",
    "hheaLineGap",
    "winAscent",
    "winDescent",
    "underlineThickness",
    "underlinePosition",
    "openTypeOS2TypoAscender",
    "openTypeOS2TypoDescender",
    "openTypeOS2TypoLineGap",
    "openTypeHheaAscender",
    "openTypeHheaDescender",
    "openTypeHheaLineGap",
    "openTypeOS2WinAscent",
    "openTypeOS2WinDescent",
    "postscriptUnderlineThickness",
    "postscriptUnderlinePosition",
    // Glyphs 2 keeps axes and name fields as font parameters; Kernest reads them into
    // its axes and Font Info names.
    "Axes",
    "Virtual Master",
    "copyright",
    "description",
    "designer",
    "designerURL",
    "license",
    "licenseURL",
    "manufacturer",
    "manufacturerURL",
    "trademark",
    "sampleText",
];

/// The font's custom parameters that Kernest has no setting for, value text intact.
fn custom_parameters(root: &glyphs_reader::Plist) -> Vec<kernest_core::CustomParameter> {
    root.get("customParameters")
        .and_then(|p| p.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?.to_string();
            if HANDLED_PARAMETERS.contains(&name.as_str()) {
                return None;
            }
            Some(kernest_core::CustomParameter {
                value: p.get("value").map(|v| v.to_string()).unwrap_or_default(),
                disabled: p.get("disabled").and_then(num).is_some_and(|v| v != 0.0),
                name,
            })
        })
        .collect()
}

/// Whether `name` matches a Glyphs glyph pattern (`*` for any run of characters).
fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            name.starts_with(head)
                && (0..=name.len() - head.len())
                    .filter(|&i| name.is_char_boundary(head.len() + i))
                    .any(|i| glob(tail, &name[head.len() + i..]))
        }
    }
}

/// Glyphs "Variable Font Setting" instances as Kernest variable fonts: the name, the
/// `fileName` parameter and the glyphs kept by "Keep Glyphs" / "Remove Glyphs".
fn variable_settings_from(
    root: &glyphs_reader::Plist,
    font: &Font,
    warnings: &mut Vec<String>,
) -> Vec<kernest_core::VariableFontSetting> {
    let family: String = font.info.family_name.split_whitespace().collect();
    let mut out: Vec<kernest_core::VariableFontSetting> = Vec::new();
    let mut ignored: Vec<String> = Vec::new();
    for inst in root
        .get("instances")
        .and_then(|i| i.as_array())
        .unwrap_or(&[])
    {
        if inst.get("type").and_then(|t| t.as_str()) != Some("variable") {
            continue;
        }
        let name = inst
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Variable")
            .to_string();
        let mut file_name = None;
        let mut vf_family = inst
            .get("properties")
            .and_then(|p| p.as_array())
            .unwrap_or(&[])
            .iter()
            .find(|p| p.get("key").and_then(|k| k.as_str()) == Some("familyNames"))
            .and_then(property_value);
        let mut records = Vec::new();
        let mut glyphs: Vec<String> = Vec::new();
        let mut rename: Vec<[String; 2]> = Vec::new();
        let mut disabled: Vec<String> = Vec::new();
        let mut processing = kernest_core::GlyphProcessing::default();
        for p in inst
            .get("customParameters")
            .and_then(|p| p.as_array())
            .unwrap_or(&[])
        {
            if p.get("disabled").and_then(num).is_some_and(|v| v != 0.0) {
                continue;
            }
            let Some(pname) = p.get("name").and_then(|n| n.as_str()) else {
                continue;
            };
            let list = || -> Vec<String> {
                p.get("value")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            match pname {
                "fileName" | "Variable Font File Name" | "variableFileName" => {
                    file_name = p.get("value").and_then(|v| v.as_str()).map(String::from)
                }
                "Variable Font Family Name" | "familyName" => {
                    vf_family = p
                        .get("value")
                        .and_then(|v| v.as_str())
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                }
                "Name Table Entry" => match kernest_core::NameRecord::parse(
                    p.get("value").and_then(|v| v.as_str()).unwrap_or_default(),
                ) {
                    Ok(r) => records.push(r),
                    Err(e) => warnings.push(format!("{name}: Name Table Entry: {e}")),
                },
                "Keep Glyphs" => {
                    let keep = list();
                    glyphs = font
                        .glyphs
                        .iter()
                        .filter(|g| keep.iter().any(|k| glob(k, &g.name)))
                        .map(|g| g.name.clone())
                        .collect();
                }
                "Remove Glyphs" => {
                    let remove = list();
                    let base: Vec<String> = if glyphs.is_empty() {
                        font.glyphs.iter().map(|g| g.name.clone()).collect()
                    } else {
                        glyphs.clone()
                    };
                    glyphs = base
                        .into_iter()
                        .filter(|n| !remove.iter().any(|r| glob(r, n)))
                        .collect();
                }
                // The default master is the font's own origin in Kernest.
                "Variable Font Origin" => {}
                "Rename Glyphs" => rename = swaps_in(p),
                "Disable Masters" => disabled = list(),
                n if kernest_core::featureedits::is_feature_parameter(n) => {}
                "Decompose Glyphs" => processing.decompose_glyphs = list(),
                "Decompose Components in Variable Font" => {
                    processing.decompose_components = is_on(p)
                }
                other => {
                    if !ignored.iter().any(|x| x == other) {
                        ignored.push(other.to_string());
                    }
                }
            }
        }
        let file_name = file_name.unwrap_or_else(|| {
            if out.is_empty() {
                format!("{family}-VF")
            } else {
                format!(
                    "{family}-{}-VF",
                    name.split_whitespace().collect::<String>()
                )
            }
        });
        out.push(kernest_core::VariableFontSetting {
            processing,
            feature_parameters: feature_parameters_of(inst),
            family_name: vf_family,
            name_records: records,
            name,
            file_name,
            axes: Vec::new(),
            glyphs,
            rename_glyphs: rename,
            remove_glyphs: Vec::new(),
            disabled_masters: disabled,
        });
    }
    // Glyphs 2 names the variable font with font parameters instead.
    if out.is_empty() {
        let param = |n: &str| {
            root.get("customParameters")
                .and_then(|p| p.as_array())
                .unwrap_or(&[])
                .iter()
                .filter(|p| p.get("disabled").and_then(num).is_none_or(|v| v == 0.0))
                .find(|p| p.get("name").and_then(|x| x.as_str()) == Some(n))
                .and_then(|p| p.get("value").and_then(|v| v.as_str()))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let vf_family = param("Variable Font Family Name");
        let vf_file = param("Variable Font File Name");
        if vf_family.is_some() || vf_file.is_some() {
            out.push(kernest_core::VariableFontSetting {
                processing: Default::default(),
                feature_parameters: Vec::new(),
                name: "Variable".into(),
                file_name: vf_file.unwrap_or_else(|| format!("{family}-VF")),
                family_name: vf_family,
                name_records: Vec::new(),
                axes: Vec::new(),
                glyphs: Vec::new(),
                rename_glyphs: Vec::new(),
                remove_glyphs: Vec::new(),
                disabled_masters: Vec::new(),
            });
        }
    }
    if !ignored.is_empty() {
        warnings.push(format!(
            "Variable font settings: these parameters were left out: {}",
            ignored.join(", ")
        ));
    }
    out
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
        vhea_ascender: i(m.vhea_ascender, f.vhea_ascender),
        vhea_descender: i(m.vhea_descender, f.vhea_descender),
        vhea_line_gap: i(m.vhea_line_gap, f.vhea_line_gap),
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

/// A colour label: an index 0–11, or an `(r, g, b, a)` array (0–255; Glyphs 2 may
/// write the alpha as 0–1).
fn color_label(p: &glyphs_reader::Plist) -> Option<kernest_core::ColorLabel> {
    if let Some(i) = num(p) {
        let label = kernest_core::ColorLabel::Index(i as u8);
        return (i >= 0.0 && i.fract() == 0.0 && label.is_valid()).then_some(label);
    }
    let parts: Vec<f64> = p.as_array()?.iter().filter_map(num).collect();
    let [r, g, b, a] = parts[..] else { return None };
    let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    let alpha = if a <= 1.0 { byte(a * 255.0) } else { byte(a) };
    Some(kernest_core::ColorLabel::from_rgba([
        byte(r),
        byte(g),
        byte(b),
        alpha,
    ]))
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
            Some(Node {
                x,
                y,
                kind,
                smooth,
                g2: false,
            })
        })
        .collect();
    Some(Shape::Path(KPath {
        locked: false,
        stroke: None,
        mask: false,
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
                    g2: false,
                })
            })
            .collect();
        out.push(Shape::Path(KPath {
            locked: false,
            stroke: None,
            mask: false,
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
    out.vert_width = l.vert_width.map(|v| v.0);
    out.vert_origin = l.vert_origin.map(|v| v.0);
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
                            g2: false,
                        }
                    })
                    .collect();
                out.shapes.push(Shape::Path(KPath {
                    locked: false,
                    stroke: None,
                    mask: false,
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
