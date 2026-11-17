//! Glyphs (`.glyphs`, `.glyphspackage`) import, via glyphs-reader.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use glyphs_reader::{Font as GFont, InstanceType, NodeType, Shape as GShape};
use kernest_core::{
    Anchor, Axis, AxisRange, Codepoint, Font, FontInfo, Glyph, Instance, KernPair, Layer, Master,
    Metrics, Node, NodeKind, Path as KPath, Shape, SmartAxis,
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
        hinting: None,
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
                    None if !l.smart_component_positions.is_empty()
                        && !out.smart_axes.is_empty() =>
                    {
                        // Smart pole layer: at the minimum or maximum of each smart axis.
                        let m = owner(l);
                        let mut k = layer(&m, l);
                        k.id = Some(out.new_layer_id(&m));
                        k.smart = Some(pole_position(&out.smart_axes, l));
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
                        out.layers.push(k);
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

    let mut font = Font {
        info,
        axes,
        masters,
        instances,
        glyphs,
        kerning,
        features,
        palettes: Vec::new(),
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
    if font.glyphs.iter().any(|g| g.is_smart()) {
        match read_glyph_dicts(path) {
            Ok(dicts) => restore_smart_components(&mut font, &dicts, &master_ids),
            Err(e) => warnings.push(format!("Smart components couldn't be read: {e}")),
        }
    }
    Ok(Imported { font, warnings })
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
