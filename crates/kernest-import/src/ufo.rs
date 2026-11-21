//! UFO and designspace import, via norad.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use kernest_core::designspace::{design_bounds, user_to_design};
use kernest_core::{
    Anchor, Axis, AxisRange, AxisValue, Codepoint, Font, FontInfo, Glyph, Instance, KernPair,
    Layer, Master, Metrics, Node, NodeKind, Path as KPath, Shape, VariableAxisSubset,
    VariableFontSetting,
};
use norad::designspace::{DesignSpaceDocument, Dimension, SubsetValue};
use norad::PointType;

use crate::{master_id, set_group, sorted_pairs, Imported};

fn load(path: &Path) -> Result<norad::Font, String> {
    norad::Font::load(path)
        .map_err(|e| format!("could not read the UFO \"{}\": {e}", path.display()))
}

/// Imports a single UFO as a one-master font.
pub fn import_ufo(path: &Path) -> Result<Imported, String> {
    let ufo = load(path)?;
    let style = ufo
        .font_info
        .style_name
        .clone()
        .unwrap_or_else(|| "Regular".into());
    let mut warnings = Vec::new();
    let font = build(
        &[(style, BTreeMap::new(), &ufo)],
        0,
        Vec::new(),
        Vec::new(),
        &mut warnings,
    );
    Ok(Imported { font, warnings })
}

/// Imports a designspace: its axes, every full UFO source as a master, and its instances.
pub fn import_designspace(path: &Path) -> Result<Imported, String> {
    let ds = DesignSpaceDocument::load(path)
        .map_err(|e| format!("could not read the designspace \"{}\": {e}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut warnings = Vec::new();

    let axes: Vec<Axis> = ds
        .axes
        .iter()
        .map(|a| {
            let mut map: Vec<(f64, f64)> = a
                .map
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|m| (m.input, m.output))
                .collect();
            map.sort_by(|x, y| x.0.total_cmp(&y.0));
            let labels = a
                .axis_labels
                .iter()
                .map(|l| AxisValue {
                    name: l.name.clone(),
                    value: l.user_value,
                    min: l.user_minimum,
                    max: l.user_maximum,
                    elidable: l.elidable,
                    linked_value: l.linked_user_value,
                })
                .collect();
            Axis {
                name: a.name.clone(),
                tag: a.tag.clone(),
                min: a.minimum.unwrap_or(a.default),
                default: a.default,
                max: a.maximum.unwrap_or(a.default),
                hidden: a.hidden,
                map,
                labels,
            }
        })
        .collect();
    if ds.axes.iter().any(|a| a.values.is_some()) {
        warnings.push("Discrete axes were imported as continuous axes.".into());
    }
    let location = |dims: &[Dimension]| -> BTreeMap<String, f64> {
        let mut loc = BTreeMap::new();
        for a in &axes {
            let v = dims
                .iter()
                .find(|d| d.name == a.name)
                .and_then(|d| {
                    d.xvalue
                        .or_else(|| d.uservalue.map(|u| user_to_design(a, u)))
                })
                .unwrap_or_else(|| design_bounds(a).1);
            loc.insert(a.tag.clone(), v);
        }
        loc
    };

    // Load each UFO once. Sources that point at a layer of a UFO (sparse sources) become
    // brace layers once the masters are built.
    let mut ufos: HashMap<String, norad::Font> = HashMap::new();
    let mut sources = Vec::new();
    let mut sparse = Vec::new();
    for s in &ds.sources {
        if !ufos.contains_key(&s.filename) {
            ufos.insert(s.filename.clone(), load(&dir.join(&s.filename))?);
        }
        if let Some(layer) = &s.layer {
            sparse.push((s.filename.clone(), layer.clone(), location(&s.location)));
            continue;
        }
        let ufo = &ufos[&s.filename];
        let name = s
            .stylename
            .clone()
            .or_else(|| ufo.font_info.style_name.clone())
            .or_else(|| s.name.clone())
            .unwrap_or_else(|| format!("Master {}", sources.len() + 1));
        sources.push((name, location(&s.location), s.filename.clone()));
    }
    if sources.is_empty() {
        return Err("the designspace has no full UFO sources.".into());
    }
    let default_loc: BTreeMap<String, f64> = axes
        .iter()
        .map(|a| (a.tag.clone(), design_bounds(a).1))
        .collect();
    let default_idx = sources
        .iter()
        .position(|(_, loc, _)| *loc == default_loc)
        .unwrap_or(0);

    let instances = ds
        .instances
        .iter()
        .enumerate()
        .map(|(i, inst)| Instance {
            names: Default::default(),
            name: inst
                .stylename
                .clone()
                .or_else(|| inst.name.clone())
                .unwrap_or_else(|| format!("Instance {}", i + 1)),
            location: location(&inst.location),
            export: true,
            weight_class: None,
            width_class: None,
            style_link: None,
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
            disabled_masters: Vec::new(),
            family_name: inst
                .familyname
                .as_deref()
                .map(str::trim)
                .filter(|f| !f.is_empty())
                .map(String::from),
            localized_family_names: Default::default(),
        })
        .collect();

    let refs: Vec<(String, BTreeMap<String, f64>, &norad::Font)> = sources
        .iter()
        .map(|(n, l, f)| (n.clone(), l.clone(), &ufos[f]))
        .collect();
    let mut font = build(&refs, default_idx, axes, instances, &mut warnings);
    // An instance family that matches the font's family is the font's own.
    let family = font.info.family_name.clone();
    for i in &mut font.instances {
        if i.family_name.as_deref() == Some(family.as_str()) {
            i.family_name = None;
        }
    }
    add_sparse_sources(&mut font, &ufos, &sparse, &mut warnings);
    add_rules(&mut font, &ds, &mut warnings);
    font.variable_fonts = ds
        .variable_fonts
        .iter()
        .map(|vf| VariableFontSetting {
            family_name: None,
            name_records: Vec::new(),
            name: vf.name.clone(),
            file_name: vf.filename.clone().unwrap_or_else(|| vf.name.clone()),
            axes: vf
                .axis_subsets
                .iter()
                .filter_map(|s| {
                    let tag = font.axes.iter().find(|a| a.name == s.name)?.tag.clone();
                    let (min, max) = match s.value {
                        SubsetValue::Range {
                            minimum, maximum, ..
                        } => (minimum, maximum),
                        SubsetValue::Discrete(v) => (Some(v), Some(v)),
                    };
                    Some(VariableAxisSubset { tag, min, max })
                })
                .collect(),
            glyphs: Vec::new(),
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
            disabled_masters: Vec::new(),
        })
        .collect();
    Ok(Imported { font, warnings })
}

/// Sparse sources (a layer of a UFO at its own location) become brace layers of the
/// glyphs they contain, shown with the default master.
fn add_sparse_sources(
    font: &mut Font,
    ufos: &HashMap<String, norad::Font>,
    sparse: &[(String, String, BTreeMap<String, f64>)],
    warnings: &mut Vec<String>,
) {
    let master_locs: Vec<BTreeMap<String, f64>> =
        font.masters.iter().map(|m| m.location.clone()).collect();
    let default_id = font.masters[0].id.clone();
    let (mut added, mut skipped) = (0, 0);
    let mut open_points = 0;
    for (file, layer_name, loc) in sparse {
        let Some(layer) = ufos.get(file).and_then(|u| u.layers.get(layer_name)) else {
            skipped += 1;
            continue;
        };
        if master_locs.contains(loc) {
            skipped += 1;
            continue;
        }
        for ug in layer.iter() {
            let Some(g) = font.glyph_mut(ug.name()) else {
                continue;
            };
            if g.brace_layers().any(|b| b.location.as_ref() == Some(loc)) {
                continue;
            }
            let mut l = self::layer(&default_id, ug, &mut open_points);
            l.id = Some(g.new_layer_id(&default_id));
            l.location = Some(loc.clone());
            g.layers.push(l);
            added += 1;
        }
    }
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} sparse source{} couldn't be used (missing layer, or on a master) and {} left out.",
            crate::plural(skipped),
            if skipped == 1 { "was" } else { "were" }
        ));
    }
    let _ = added;
}

/// Designspace rules become bracket layers: where a rule swaps `A` for `A.alt`, `A` gets
/// an alternate made from `A.alt`'s outlines in every master. Substitute glyphs with
/// generated names (`*.BRACKET.*`, `*.rvrn`) are then removed.
fn add_rules(font: &mut Font, ds: &DesignSpaceDocument, warnings: &mut Vec<String>) {
    let mut converted = 0;
    let mut left_out = 0;
    let mut generated = Vec::new();
    for rule in &ds.rules.rules {
        let [set] = rule.condition_sets.as_slice() else {
            left_out += rule.substitutions.len();
            continue;
        };
        let ranges: Option<Vec<AxisRange>> = set
            .conditions
            .iter()
            .map(|c| {
                font.axes
                    .iter()
                    .find(|a| a.name == c.name)
                    .map(|a| AxisRange {
                        tag: a.tag.clone(),
                        // Conditions are user-space; bracket ranges are design units.
                        min: c.minimum.map(|v| user_to_design(a, v)),
                        max: c.maximum.map(|v| user_to_design(a, v)),
                    })
            })
            .collect();
        let Some(mut ranges) = ranges.filter(|r| !r.is_empty()) else {
            left_out += rule.substitutions.len();
            continue;
        };
        ranges.sort_by(|a, b| a.tag.cmp(&b.tag));
        for sub in &rule.substitutions {
            let (name, with) = (sub.name.as_str(), sub.with.as_str());
            let Some(alt) = font.glyph(with).cloned() else {
                left_out += 1;
                continue;
            };
            let masters: Vec<String> = font.masters.iter().map(|m| m.id.clone()).collect();
            let Some(g) = font.glyph_mut(name) else {
                left_out += 1;
                continue;
            };
            if g.bracket_groups().contains(&ranges) {
                left_out += 1;
                continue;
            }
            for m in &masters {
                if let Some(src) = alt.layer(m) {
                    let mut l = src.clone();
                    l.id = Some(g.new_layer_id(m));
                    l.bracket = ranges.clone();
                    g.layers.push(l);
                }
            }
            converted += 1;
            if with.contains(".BRACKET.") || with.ends_with(".rvrn") {
                generated.push(with.to_string());
            }
        }
    }
    let used_as_component: HashSet<String> = font
        .glyphs
        .iter()
        .flat_map(|g| g.layers.iter())
        .flat_map(|l| l.shapes.iter())
        .filter_map(|s| match s {
            kernest_core::Shape::Component { base, .. } => Some(base.clone()),
            _ => None,
        })
        .collect();
    font.glyphs
        .retain(|g| !(generated.contains(&g.name) && !used_as_component.contains(&g.name)));
    if left_out > 0 {
        warnings.push(format!(
            "{left_out} designspace rule substitution{} couldn't become alternate layers and {} left out.",
            crate::plural(left_out),
            if left_out == 1 { "was" } else { "were" }
        ));
    }
    let _ = converted;
}

fn metrics(info: &norad::FontInfo) -> Metrics {
    let d = Metrics::default();
    Metrics {
        ascender: info.ascender.unwrap_or(d.ascender),
        cap_height: info.cap_height.unwrap_or(d.cap_height),
        x_height: info.x_height.unwrap_or(d.x_height),
        descender: info.descender.unwrap_or(d.descender),
        // UFO angles are counter-clockwise; Kernest stores the slant (positive leans right).
        italic_angle: info.italic_angle.map(|a| -a).unwrap_or(0.0),
    }
}

/// OpenType line spacing and underline values from a UFO's font info.
fn vertical_metrics(fi: &norad::FontInfo) -> kernest_core::VerticalMetrics {
    kernest_core::VerticalMetrics {
        typo_ascender: fi.open_type_os2_typo_ascender.map(|v| v as f64),
        typo_descender: fi.open_type_os2_typo_descender.map(|v| v as f64),
        typo_line_gap: fi.open_type_os2_typo_line_gap.map(|v| v as f64),
        hhea_ascender: fi.open_type_hhea_ascender.map(|v| v as f64),
        hhea_descender: fi.open_type_hhea_descender.map(|v| v as f64),
        hhea_line_gap: fi.open_type_hhea_line_gap.map(|v| v as f64),
        win_ascent: fi.open_type_os2_win_ascent.map(|v| v as f64),
        win_descent: fi.open_type_os2_win_descent.map(|v| v as f64),
        underline_thickness: fi.postscript_underline_thickness,
        underline_position: fi.postscript_underline_position,
        vhea_ascender: fi.open_type_vhea_vert_typo_ascender.map(|v| v as f64),
        vhea_descender: fi.open_type_vhea_vert_typo_descender.map(|v| v as f64),
        vhea_line_gap: fi.open_type_vhea_vert_typo_line_gap.map(|v| v as f64),
    }
}

/// UFO guidelines as Kernest guides. Vertical ones (only `x`) and horizontal ones
/// (only `y`) get the angle they imply.
fn guides(list: &[norad::Guideline]) -> Vec<kernest_core::Guide> {
    use norad::Line;
    list.iter()
        .map(|g| {
            let (x, y, angle) = match g.line {
                Line::Vertical(x) => (x, 0.0, 90.0),
                Line::Horizontal(y) => (0.0, y, 0.0),
                Line::Angle { x, y, degrees } => (x, y, degrees),
            };
            kernest_core::Guide {
                x,
                y,
                angle,
                name: g.name.as_ref().map(|n| n.to_string()),
                locked: false,
            }
        })
        .collect()
}

/// Alignment zones and stems from a UFO's PostScript hinting fields: blue value
/// pairs become zones, StemSnapH/V become named stems.
fn zones_and_stems(fi: &norad::FontInfo) -> (Vec<kernest_core::Zone>, Vec<kernest_core::Stem>) {
    let blue = fi.postscript_blue_values.clone().unwrap_or_default();
    let other = fi.postscript_other_blues.clone().unwrap_or_default();
    let zones = kernest_core::hinting::zones_from_blues(&blue, &other);
    let mut stems = Vec::new();
    for (values, horizontal) in [
        (fi.postscript_stem_snap_h.clone().unwrap_or_default(), true),
        (fi.postscript_stem_snap_v.clone().unwrap_or_default(), false),
    ] {
        for (i, value) in values.into_iter().enumerate() {
            if value > 0.0 {
                stems.push(kernest_core::Stem {
                    name: format!("{}{i}", if horizontal { "h" } else { "v" }),
                    horizontal,
                    value,
                });
            }
        }
    }
    (zones, stems)
}

/// The blue zone controls, when the UFO sets any of them away from the defaults.
fn ps_hinting(fi: &norad::FontInfo) -> Option<kernest_core::PsHinting> {
    let d = kernest_core::PsHinting::default();
    let ps = kernest_core::PsHinting {
        blue_scale: fi.postscript_blue_scale.unwrap_or(d.blue_scale),
        blue_shift: fi.postscript_blue_shift.unwrap_or(d.blue_shift),
        blue_fuzz: fi.postscript_blue_fuzz.unwrap_or(d.blue_fuzz),
    };
    (ps != d).then_some(ps)
}

fn lib_strings(ufo: &norad::Font, key: &str) -> Vec<String> {
    ufo.lib
        .get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_string().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Builds a Kernest font from UFO sources `(style name, location, ufo)`; info, glyph
/// order, groups and features come from the default source.
fn build(
    sources: &[(String, BTreeMap<String, f64>, &norad::Font)],
    default_idx: usize,
    axes: Vec<Axis>,
    instances: Vec<Instance>,
    warnings: &mut Vec<String>,
) -> Font {
    let def = sources[default_idx].2;
    let fi = &def.font_info;
    let info = FontInfo {
        family_name: fi.family_name.clone().unwrap_or_else(|| "Untitled".into()),
        units_per_em: fi
            .units_per_em
            .map(|u| (*u).round().clamp(16.0, 16384.0) as u16)
            .unwrap_or(1000),
        version_major: fi
            .version_major
            .map(|v| v.clamp(0, 65535) as u16)
            .unwrap_or(1),
        version_minor: fi.version_minor.map(|v| v.min(65535) as u16).unwrap_or(0),
        designer: fi.open_type_name_designer.clone(),
        copyright: fi.copyright.clone(),
        vendor_id: fi.open_type_os2_vendor_id.clone(),
        designer_url: fi.open_type_name_designer_url.clone(),
        manufacturer: fi.open_type_name_manufacturer.clone(),
        manufacturer_url: fi.open_type_name_manufacturer_url.clone(),
        license: fi.open_type_name_license.clone(),
        license_url: fi.open_type_name_license_url.clone(),
        description: fi.open_type_name_description.clone(),
        trademark: fi.trademark.clone(),
        sample_text: fi.open_type_name_sample_text.clone(),
        hinting: None,
        ps_hinting: ps_hinting(fi),
        control_instructions: String::new(),
        grid: None,
        use_typo_metrics: fi
            .open_type_os2_selection
            .as_ref()
            .is_some_and(|s| s.contains(&7)),
        custom_parameters: Vec::new(),
        virtual_masters: Vec::new(),
        numbers: BTreeMap::new(),
    };

    // Masters, with the default one first so it is the reference for empty layers.
    let mut order: Vec<usize> = (0..sources.len()).collect();
    order.retain(|i| *i != default_idx);
    order.insert(0, default_idx);
    let masters: Vec<Master> = order
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let (alignment_zones, stems) = zones_and_stems(&sources[i].2.font_info);
            Master {
                id: master_id(k),
                name: sources[i].0.clone(),
                location: sources[i].1.clone(),
                metrics: metrics(&sources[i].2.font_info),
                guides: guides(sources[i].2.font_info.guidelines.as_deref().unwrap_or(&[])),
                alignment_zones,
                stems,
                vertical: vertical_metrics(&sources[i].2.font_info),
            }
        })
        .collect();

    // Glyph order: public.glyphOrder of the default source, then everything else sorted.
    let mut names: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for n in lib_strings(def, "public.glyphOrder") {
        if def.get_glyph(&n).is_some() && seen.insert(n.clone()) {
            names.push(n);
        }
    }
    let mut rest: Vec<String> = sources
        .iter()
        .flat_map(|(_, _, u)| u.default_layer().iter().map(|g| g.name().to_string()))
        .filter(|n| !seen.contains(n))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    names.append(&mut rest);
    let skip: HashSet<String> = lib_strings(def, "public.skipExportGlyphs")
        .into_iter()
        .collect();
    let ps_names: HashMap<String, String> = def
        .lib
        .get("public.postscriptNames")
        .and_then(|v| v.as_dictionary())
        .map(|d| {
            d.iter()
                .filter_map(|(k, v)| v.as_string().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let mut open_points = 0;
    let mut glyphs: Vec<Glyph> = names
        .iter()
        .map(|n| {
            let src = sources
                .iter()
                .find_map(|(_, _, u)| u.get_glyph(n))
                .expect("name came from a source");
            let mut g = Glyph::new(
                n.clone(),
                src.codepoints.iter().map(|c| Codepoint(c as u32)).collect(),
            );
            g.export = !skip.contains(n);
            g.note = src.note.clone().filter(|s| !s.is_empty());
            g.production_name = ps_names.get(n).cloned();
            for (k, &i) in order.iter().enumerate() {
                if let Some(ug) = sources[i].2.get_glyph(n) {
                    let mut l = layer(&master_id(k), ug, &mut open_points);
                    read_vertical(&mut l, ug, &sources[i].2.font_info);
                    l.guides = guides(&ug.guidelines);
                    // The "public.background" layer holds the reference drawing.
                    if let Some(bg) = sources[i]
                        .2
                        .layers
                        .get("public.background")
                        .and_then(|b| b.get_glyph(n))
                    {
                        let b = layer(&master_id(k), bg, &mut open_points);
                        if !b.shapes.is_empty() || !b.anchors.is_empty() {
                            l.background = Some(kernest_core::Background {
                                shapes: b.shapes,
                                anchors: b.anchors,
                            });
                        }
                    }
                    g.layers.push(l);
                }
            }
            g
        })
        .collect();
    if open_points > 0 {
        warnings.push(format!(
            "{open_points} contour{} only off-curve points and {} left out.",
            if open_points == 1 { " had" } else { "s had" },
            if open_points == 1 { "was" } else { "were" }
        ));
    }

    // Kerning groups (only public.kern1/kern2 groups are kerning groups).
    let index: HashMap<String, usize> = glyphs
        .iter()
        .enumerate()
        .map(|(i, g)| (g.name.clone(), i))
        .collect();
    let mut other_groups = 0;
    for (gname, members) in &def.groups {
        let (first, group) = if let Some(g) = gname.strip_prefix("public.kern1.") {
            (true, g)
        } else if let Some(g) = gname.strip_prefix("public.kern2.") {
            (false, g)
        } else {
            other_groups += 1;
            continue;
        };
        for m in members {
            if let Some(&i) = index.get(m.as_str()) {
                set_group(&mut glyphs[i], first, group, warnings);
            }
        }
    }
    if other_groups > 0 {
        warnings.push(format!(
            "{other_groups} group{} that {} not kerning groups {} left out.",
            crate::plural(other_groups),
            if other_groups == 1 { "is" } else { "are" },
            if other_groups == 1 { "was" } else { "were" }
        ));
    }
    let side = |s: &str| -> String {
        s.strip_prefix("public.kern1.")
            .or_else(|| s.strip_prefix("public.kern2."))
            .map(|g| format!("@{g}"))
            .unwrap_or_else(|| s.to_string())
    };
    let mut kerning = BTreeMap::new();
    for (k, &i) in order.iter().enumerate() {
        let pairs: Vec<KernPair> = sources[i]
            .2
            .kerning
            .iter()
            .flat_map(|(l, rs)| {
                rs.iter().map(move |(r, v)| KernPair {
                    left: side(l),
                    right: side(r),
                    value: *v,
                })
            })
            .collect();
        if !pairs.is_empty() {
            kerning.insert(master_id(k), sorted_pairs(pairs));
        }
    }

    Font {
        info,
        axes,
        masters,
        instances,
        glyphs,
        kerning,
        features: def.features.clone(),
        notes: String::new(),
        feature_settings: Default::default(),
        palettes: Vec::new(),
        master_palettes: Default::default(),
        filters: Vec::new(),
        variable_fonts: Vec::new(),
        export_presets: Vec::new(),
    }
}

/// A glyph's vertical advance and origin (UFO `height` and `public.verticalOrigin`),
/// in Glyphs' terms: `vertOrigin` is how far the origin sits below the typo ascender.
/// Read only when the UFO uses vertical metrics (vhea values or an origin on the
/// glyph), since many UFOs carry an arbitrary height.
fn read_vertical(l: &mut Layer, g: &norad::Glyph, fi: &norad::FontInfo) {
    let origin = g.lib.get("public.verticalOrigin").and_then(|v| {
        v.as_real()
            .or_else(|| v.as_signed_integer().map(|i| i as f64))
    });
    if origin.is_none() && fi.open_type_vhea_vert_typo_ascender.is_none() {
        return;
    }
    let asc = fi
        .open_type_os2_typo_ascender
        .map(|v| v as f64)
        .or(fi.ascender)
        .unwrap_or(0.0);
    if g.height > 0.0 {
        l.vert_width = Some(g.height);
    }
    l.vert_origin = origin.map(|o| asc - o).filter(|v| *v != 0.0);
}

fn layer(id: &str, g: &norad::Glyph, skipped: &mut usize) -> Layer {
    let mut l = Layer::new(id, g.width);
    for c in &g.contours {
        if c.points.is_empty() {
            continue;
        }
        if c.points.iter().all(|p| p.typ == PointType::OffCurve) {
            *skipped += 1;
            continue;
        }
        let closed = c.points[0].typ != PointType::Move;
        let nodes = c
            .points
            .iter()
            .map(|p| Node {
                x: p.x,
                y: p.y,
                kind: match p.typ {
                    PointType::Move | PointType::Line => NodeKind::Line,
                    PointType::Curve => NodeKind::Curve,
                    PointType::QCurve => NodeKind::QCurve,
                    PointType::OffCurve => NodeKind::OffCurve,
                },
                smooth: p.smooth,
                g2: false,
            })
            .collect();
        l.shapes.push(Shape::Path(KPath {
            locked: false,
            stroke: None,
            mask: false,
            closed,
            nodes,
        }));
    }
    for c in &g.components {
        let t = &c.transform;
        l.shapes.push(Shape::component(
            c.base.to_string(),
            [
                t.x_scale, t.xy_scale, t.yx_scale, t.y_scale, t.x_offset, t.y_offset,
            ],
        ));
    }
    for a in &g.anchors {
        if let Some(name) = &a.name {
            l.anchors.push(Anchor {
                name: name.to_string(),
                x: a.x,
                y: a.y,
            });
        }
    }
    l
}
