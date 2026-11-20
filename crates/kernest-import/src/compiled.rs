//! Compiled-font import (`.otf`, `.ttf`, `.woff2`) via skrifa and read-fonts.
//!
//! A compiled font is a finished font, not an editable source: it has no masters,
//! components, brace layers or feature code. We draw each glyph's outline (TrueType
//! quadratics stay quadratic, CFF cubics stay cubic), read its advance width, unicode
//! and name, and recover kerning and mark anchors from GPOS. A variable font becomes
//! a master at the default location plus one at each axis extreme, drawn with skrifa.
//! What cannot be recovered is listed in [`Imported::warnings`].

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use kernest_core::{
    Anchor, Axis, AxisValue, Codepoint, Font, FontInfo, Glyph, Instance, KernPair, Layer, Master,
    Metrics, Node, NodeKind, Path as KPath, Shape,
};
use skrifa::instance::{Location, Size};
use skrifa::outline::pen::PathElement;
use skrifa::outline::DrawSettings;
use skrifa::raw::tables::gpos::{PairPos, PositionSubtables};
use skrifa::raw::TableProvider;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::{master_id, set_group, sorted_pairs, Imported};

/// Reads a compiled font into a new, unsaved Kernest font.
pub fn import_compiled(path: &Path) -> Result<Imported, String> {
    let raw =
        std::fs::read(path).map_err(|e| format!("could not read \"{}\": {e}", path.display()))?;
    let data = if raw.starts_with(b"wOF2") {
        crate::woff2::decode(&raw)?
    } else {
        raw
    };
    let font = FontRef::new(&data)
        .map_err(|e| format!("\"{}\" isn't a font Kernest can read: {e}", path.display()))?;
    build(&font)
}

/// One master to draw: a name, its user-space location and the normalized location
/// skrifa draws at.
struct MasterPlan {
    name: String,
    location: BTreeMap<String, f64>,
    norm: Location,
}

fn build(font: &FontRef) -> Result<Imported, String> {
    let mut warnings = Vec::new();
    let upem = font
        .head()
        .map_err(|_| "the font has no head table.".to_string())?
        .units_per_em();
    if upem == 0 {
        return Err("the font has an invalid units-per-em value.".into());
    }

    let axes = font.axes();
    let variable = !axes.is_empty();
    let kernest_axes: Vec<Axis> = axes
        .iter()
        .map(|a| Axis {
            name: axis_name(font, a.name_id(), a.tag().to_string()),
            tag: a.tag().to_string(),
            min: a.min_value() as f64,
            default: a.default_value() as f64,
            max: a.max_value() as f64,
            hidden: a.is_hidden(),
            map: axis_map(font, &a),
            labels: axis_labels(font, &a.tag().to_string()),
        })
        .collect();
    if let Ok(avar) = font.avar() {
        if avar.version().major >= 2 && avar.var_store().is_some() {
            warnings.push(
                "This font's axis mappings depend on other axes (avar2) and were left out.".into(),
            );
        }
    }
    let plans = master_plans(font, &axes, &kernest_axes);
    let default_metrics = global_metrics(font);

    let masters: Vec<Master> = plans
        .iter()
        .enumerate()
        .map(|(i, p)| Master {
            id: master_id(i),
            name: p.name.clone(),
            location: p.location.clone(),
            metrics: default_metrics,
            guides: Vec::new(),
            alignment_zones: Vec::new(),
            stems: Vec::new(),
            vertical: Default::default(),
        })
        .collect();

    let instances = named_instances(font, &kernest_axes);

    // Glyph names and unicodes, in glyph-id order.
    let names = font.glyph_names();
    let count = font.maxp().map(|m| m.num_glyphs()).unwrap_or(0) as u32;
    let mut unicodes: HashMap<u32, Vec<Codepoint>> = HashMap::new();
    for (cp, gid) in font.charmap().mappings() {
        unicodes
            .entry(gid.to_u32())
            .or_default()
            .push(Codepoint(cp));
    }
    for cps in unicodes.values_mut() {
        cps.sort_by_key(|c| c.0);
        cps.dedup();
    }

    let outlines = font.outline_glyphs();
    let anchors = collect_anchors(font, &mut warnings);
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(count as usize);
    let mut index: HashMap<u32, usize> = HashMap::new();
    let mut drawn = 0usize;
    for gid in 0..count {
        let id = GlyphId::new(gid);
        let name = names
            .get(id)
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| format!("gid{gid}"));
        index.insert(gid, glyphs.len());
        let mut g = Glyph::new(name, unicodes.remove(&gid).unwrap_or_default());
        for (i, p) in plans.iter().enumerate() {
            let gm = font.glyph_metrics(Size::unscaled(), &p.norm);
            let width = gm.advance_width(id).unwrap_or(0.0) as f64;
            let mut l = Layer::new(master_id(i), width);
            if let Some(og) = outlines.get(id) {
                let mut elems: Vec<PathElement> = Vec::new();
                if og
                    .draw(
                        DrawSettings::unhinted(Size::unscaled(), &p.norm),
                        &mut elems,
                    )
                    .is_ok()
                {
                    l.shapes = shapes_from(&elems);
                    drawn += 1;
                }
            }
            if let Some(anch) = anchors.get(&gid) {
                l.anchors = anch.clone();
            }
            g.layers.push(l);
        }
        glyphs.push(g);
    }

    let gid_names: HashMap<u32, String> = index
        .iter()
        .map(|(&gid, &i)| (gid, glyphs[i].name.clone()))
        .collect();
    let kerning_pairs = collect_kerning(font, &index, &gid_names, &mut glyphs, &mut warnings);
    let mut kerning = BTreeMap::new();
    if !kerning_pairs.is_empty() {
        let pairs = sorted_pairs(kerning_pairs);
        for m in &masters {
            kerning.insert(m.id.clone(), pairs.clone());
        }
    }

    if drawn == 0 {
        warnings.push("No outlines could be drawn from this font.".into());
    }
    warnings.push(
        "Compiled fonts have no editable components or feature code; outlines were imported as paths and features were left out."
            .into(),
    );
    if variable {
        warnings.push(format!(
            "This is a variable font. Kernest drew {} masters (the default and each axis extreme); intermediate masters and named instances beyond these are approximations.",
            masters.len()
        ));
    }
    warnings.push(
        "Glyph categories (GDEF classes) are re-derived by Kernest from names and unicodes, not imported."
            .into(),
    );

    let info = font_info(font, upem);
    Ok(Imported {
        font: Font {
            info,
            axes: kernest_axes,
            masters,
            instances,
            glyphs,
            kerning,
            features: String::new(),
            notes: String::new(),
            feature_settings: Default::default(),
            palettes: Vec::new(),
            master_palettes: Default::default(),
            filters: Vec::new(),
            variable_fonts: Vec::new(),
            export_presets: Vec::new(),
        },
        warnings,
    })
}

/// The default location plus each axis at its minimum and maximum (2N+1 masters).
fn master_plans(
    font: &FontRef,
    axes: &skrifa::AxisCollection,
    kernest_axes: &[Axis],
) -> Vec<MasterPlan> {
    // Master locations are design-space; skrifa draws at normalized user coordinates.
    let to_design = |tag: &str, v: f64| {
        kernest_axes
            .iter()
            .find(|a| a.tag == tag)
            .map(|a| kernest_core::designspace::user_to_design(a, v))
            .unwrap_or(v)
    };
    let defaults = || -> BTreeMap<String, f64> {
        axes.iter()
            .map(|a| {
                let tag = a.tag().to_string();
                let v = to_design(&tag, a.default_value() as f64);
                (tag, v)
            })
            .collect()
    };
    let mut plans = vec![MasterPlan {
        name: "Default".into(),
        location: defaults(),
        norm: Location::new(axes.len()),
    }];
    if axes.is_empty() {
        plans[0].name = "Regular".into();
        return plans;
    }
    for a in axes.iter() {
        let tag = a.tag().to_string();
        let label = axis_name(font, a.name_id(), tag.clone());
        for (suffix, value) in [("min", a.min_value()), ("max", a.max_value())] {
            let mut location = defaults();
            location.insert(tag.clone(), to_design(&tag, value as f64));
            plans.push(MasterPlan {
                name: format!("{label} {suffix}"),
                location,
                norm: axes.location([(a.tag(), value)]),
            });
        }
    }
    plans
}

/// The avar segment map of one axis as user → design points. A compiled font only
/// keeps the *normalized* mapping, so the original design scale is gone; both sides
/// are put back on the fvar user scale, which reproduces the same interpolation.
/// An identity map becomes no map at all.
fn axis_map(font: &FontRef, a: &skrifa::Axis) -> Vec<(f64, f64)> {
    let Ok(avar) = font.avar() else {
        return Vec::new();
    };
    let Some(maps) = avar.axis_segment_maps().get(a.index()).and_then(|m| m.ok()) else {
        return Vec::new();
    };
    let (min, default, max) = (
        a.min_value() as f64,
        a.default_value() as f64,
        a.max_value() as f64,
    );
    let denorm = |n: f64| {
        if n < 0.0 {
            default + n * (default - min)
        } else {
            default + n * (max - default)
        }
    };
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for m in maps.axis_value_maps() {
        let u = denorm(m.from_coordinate.get().to_f64());
        let d = denorm(m.to_coordinate.get().to_f64());
        // min == default collapses the -1 and 0 segments onto one user value.
        if pts.last().is_some_and(|(pu, _)| *pu == u) {
            continue;
        }
        pts.push((u, d));
    }
    if pts.iter().all(|(u, d)| u == d) {
        return Vec::new();
    }
    pts
}

/// The STAT axis value labels of one axis, read back as user-space names. Format 4
/// (whole-location) labels are left out; Kernest names values per axis.
fn axis_labels(font: &FontRef, tag: &str) -> Vec<AxisValue> {
    use skrifa::raw::tables::stat::{AxisValue as StatValue, AxisValueTableFlags};
    let Ok(stat) = font.stat() else {
        return Vec::new();
    };
    let Ok(axes) = stat.design_axes() else {
        return Vec::new();
    };
    let Some(idx) = axes.iter().position(|a| a.axis_tag() == tag) else {
        return Vec::new();
    };
    let idx = idx as u16;
    let Some(Ok(values)) = stat.offset_to_axis_values() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for v in values.axis_values().iter().filter_map(|v| v.ok()) {
        if v.axis_index() != Some(idx) {
            continue;
        }
        let Some(value) = v.value() else {
            continue;
        };
        let name = font
            .localized_strings(v.value_name_id())
            .english_or_first()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let (min, max) = match &v {
            StatValue::Format2(f2) => (
                Some(f2.range_min_value().to_f64()),
                Some(f2.range_max_value().to_f64()),
            ),
            _ => (None, None),
        };
        out.push(AxisValue {
            name,
            value: value.to_f64(),
            min,
            max,
            elidable: v
                .flags()
                .contains(AxisValueTableFlags::ELIDABLE_AXIS_VALUE_NAME),
            linked_value: v.linked_value().map(|x| x.to_f64()),
        });
    }
    out
}

fn axis_name(font: &FontRef, name_id: StringId, fallback: String) -> String {
    font.localized_strings(name_id)
        .english_or_first()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}

fn named_instances(font: &FontRef, kernest_axes: &[Axis]) -> Vec<Instance> {
    font.named_instances()
        .iter()
        .map(|inst| {
            let coords: Vec<f32> = inst.user_coords().collect();
            let location = kernest_axes
                .iter()
                .zip(&coords)
                .map(|(a, v)| {
                    (
                        a.tag.clone(),
                        kernest_core::designspace::user_to_design(a, *v as f64),
                    )
                })
                .collect();
            let name = font
                .localized_strings(inst.subfamily_name_id())
                .english_or_first()
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Instance".into());
            Instance {
                name,
                location,
                export: true,
                weight_class: None,
                width_class: None,
                style_link: None,
                rename_glyphs: Vec::new(),
                remove_glyphs: Vec::new(),
            }
        })
        .collect()
}

fn global_metrics(font: &FontRef) -> Metrics {
    let d = Metrics::default();
    let hhea = font.hhea().ok();
    let os2 = font.os2().ok();
    // The typographic ascender/descender (OS/2 sTypoAscender/Descender) are what font
    // editors store; hhea often holds the larger Windows clipping metrics instead.
    let ascender = os2
        .as_ref()
        .map(|o| o.s_typo_ascender() as f64)
        .or_else(|| hhea.as_ref().map(|h| h.ascender().to_i16() as f64))
        .unwrap_or(d.ascender);
    let descender = os2
        .as_ref()
        .map(|o| o.s_typo_descender() as f64)
        .or_else(|| hhea.as_ref().map(|h| h.descender().to_i16() as f64))
        .unwrap_or(d.descender);
    let cap_height = os2
        .as_ref()
        .and_then(|o| o.s_cap_height())
        .map(|v| v as f64)
        .unwrap_or(d.cap_height);
    let x_height = os2
        .as_ref()
        .and_then(|o| o.sx_height())
        .map(|v| v as f64)
        .unwrap_or(d.x_height);
    // post italicAngle is counter-clockwise; Kernest stores the slant (positive leans right).
    let italic_angle = font
        .post()
        .map(|p| -p.italic_angle().to_f64())
        .unwrap_or(0.0);
    Metrics {
        ascender,
        cap_height,
        x_height,
        descender,
        italic_angle,
    }
}

fn font_info(font: &FontRef, upem: u16) -> FontInfo {
    let s = |id: StringId| {
        font.localized_strings(id)
            .english_or_first()
            .map(|x| x.to_string())
            .filter(|x| !x.is_empty())
    };
    let version = s(StringId::VERSION_STRING);
    let (version_major, version_minor) = parse_version(version.as_deref());
    let vendor_id = font
        .os2()
        .ok()
        .map(|o| o.ach_vend_id().to_string())
        .filter(|v| !v.trim().is_empty() && v != "\0\0\0\0");
    FontInfo {
        family_name: s(StringId::FAMILY_NAME).unwrap_or_else(|| "Untitled".into()),
        units_per_em: upem,
        version_major,
        version_minor,
        designer: s(StringId::DESIGNER),
        copyright: s(StringId::COPYRIGHT_NOTICE),
        vendor_id,
        designer_url: s(StringId::DESIGNER_URL),
        manufacturer: s(StringId::MANUFACTURER),
        manufacturer_url: s(StringId::VENDOR_URL),
        license: s(StringId::LICENSE_DESCRIPTION),
        license_url: s(StringId::LICENSE_URL),
        description: s(StringId::DESCRIPTION),
        trademark: s(StringId::TRADEMARK),
        sample_text: s(StringId::SAMPLE_TEXT),
        hinting: None,
        grid: None,
        use_typo_metrics: false,
        custom_parameters: Vec::new(),
        ps_hinting: None,
        control_instructions: String::new(),
        virtual_masters: Vec::new(),
        numbers: BTreeMap::new(),
    }
}

/// Pulls "Version 1.002" into (1, 2); defaults to (1, 0).
fn parse_version(v: Option<&str>) -> (u16, u16) {
    let Some(v) = v else { return (1, 0) };
    let num = v
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|p| !p.is_empty() && p.contains('.'))
        .or_else(|| {
            v.split_whitespace()
                .find(|p| p.chars().any(|c| c.is_ascii_digit()))
        });
    let Some(num) = num else { return (1, 0) };
    let mut parts = num.split('.');
    let major = parts.next().and_then(|p| p.parse().ok()).unwrap_or(1);
    let minor = parts
        .next()
        .map(|p| {
            let digits: String = p.chars().take(3).collect();
            let n: u16 = digits.parse().unwrap_or(0);
            // "002" means 2 thousandths; keep three digits as the minor value.
            n
        })
        .unwrap_or(0);
    (major, minor)
}

/// Converts drawn path elements into Kernest paths. Quadratics stay QCurve, cubics
/// stay Curve. A closed contour's final on-curve point repeats the start; dropping it
/// leaves any trailing off-curves as the closing segment back to the first node.
fn shapes_from(elems: &[PathElement]) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut start: Option<(f64, f64)> = None;
    let flush = |nodes: &mut Vec<Node>, shapes: &mut Vec<Shape>| {
        if nodes.is_empty() {
            return;
        }
        shapes.push(Shape::Path(KPath {
            closed: true,
            nodes: std::mem::take(nodes),
            locked: false,
            stroke: None,
            mask: false,
        }));
    };
    for e in elems {
        match *e {
            PathElement::MoveTo { x, y } => {
                flush(&mut nodes, &mut shapes);
                start = Some((x as f64, y as f64));
                nodes.push(Node::new(x as f64, y as f64, NodeKind::Line));
            }
            PathElement::LineTo { x, y } => {
                nodes.push(Node::new(x as f64, y as f64, NodeKind::Line))
            }
            PathElement::QuadTo { cx0, cy0, x, y } => {
                nodes.push(Node::new(cx0 as f64, cy0 as f64, NodeKind::OffCurve));
                nodes.push(Node::new(x as f64, y as f64, NodeKind::QCurve));
            }
            PathElement::CurveTo {
                cx0,
                cy0,
                cx1,
                cy1,
                x,
                y,
            } => {
                nodes.push(Node::new(cx0 as f64, cy0 as f64, NodeKind::OffCurve));
                nodes.push(Node::new(cx1 as f64, cy1 as f64, NodeKind::OffCurve));
                nodes.push(Node::new(x as f64, y as f64, NodeKind::Curve));
            }
            PathElement::Close => {
                if let (Some((sx, sy)), Some(last)) = (start, nodes.last()) {
                    if last.on_curve() && last.x == sx && last.y == sy && nodes.len() > 1 {
                        nodes.pop();
                    }
                }
                flush(&mut nodes, &mut shapes);
                start = None;
            }
        }
    }
    flush(&mut nodes, &mut shapes);
    shapes
}

/// Recovers kerning from GPOS PairPos: format 1 gives glyph pairs, format 2 gives
/// class kerning, which becomes kerning groups. Only the first glyph's x-advance is
/// used (horizontal kerning).
fn collect_kerning(
    font: &FontRef,
    index: &HashMap<u32, usize>,
    names: &HashMap<u32, String>,
    glyphs: &mut [Glyph],
    warnings: &mut Vec<String>,
) -> Vec<KernPair> {
    let mut pairs = Vec::new();
    let Ok(gpos) = font.gpos() else { return pairs };
    let Ok(list) = gpos.lookup_list() else {
        return pairs;
    };
    let name = |gid: u32| -> Option<String> { names.get(&gid).cloned() };
    let mut class_zero = 0;
    for lookup in list.lookups().iter().flatten() {
        if lookup.lookup_type() != 2 {
            continue;
        }
        let Ok(PositionSubtables::Pair(subs)) = lookup.subtables() else {
            continue;
        };
        for sub in subs.iter().flatten() {
            match sub {
                PairPos::Format1(t) => {
                    let Ok(cov) = t.coverage() else { continue };
                    let cov: Vec<u32> = cov.iter().map(|g| g.to_u32()).collect();
                    for (i, set) in t.pair_sets().iter().enumerate() {
                        let Some(&first) = cov.get(i) else { continue };
                        let Ok(set) = set else { continue };
                        let Some(left) = name(first) else { continue };
                        for rec in set.pair_value_records().iter().flatten() {
                            let Some(dx) = rec.value_record1().x_advance() else {
                                continue;
                            };
                            if dx == 0 {
                                continue;
                            }
                            if let Some(right) = name(rec.second_glyph().to_u32()) {
                                pairs.push(KernPair {
                                    left: left.clone(),
                                    right,
                                    value: dx as f64,
                                });
                            }
                        }
                    }
                }
                PairPos::Format2(t) => {
                    let (Ok(cd1), Ok(cd2)) = (t.class_def1(), t.class_def2()) else {
                        continue;
                    };
                    let values = t.class_value_records();
                    for c1 in 0..t.class1_count() {
                        for c2 in 0..t.class2_count() {
                            let Some([v1, _]) = values.get(c1, c2) else {
                                continue;
                            };
                            let Some(dx) = v1.x_advance() else { continue };
                            if dx == 0 {
                                continue;
                            }
                            if c1 == 0 || c2 == 0 {
                                class_zero += 1;
                                continue;
                            }
                            pairs.push(KernPair {
                                left: format!("@kern1_{c1}"),
                                right: format!("@kern2_{c2}"),
                                value: dx as f64,
                            });
                        }
                    }
                    for (gid, class) in cd1.iter() {
                        if class == 0 {
                            continue;
                        }
                        if let Some(&i) = index.get(&gid.to_u32()) {
                            set_group(&mut glyphs[i], true, &format!("kern1_{class}"), warnings);
                        }
                    }
                    for (gid, class) in cd2.iter() {
                        if class == 0 {
                            continue;
                        }
                        if let Some(&i) = index.get(&gid.to_u32()) {
                            set_group(&mut glyphs[i], false, &format!("kern2_{class}"), warnings);
                        }
                    }
                }
            }
        }
    }
    if class_zero > 0 {
        warnings.push(format!(
            "{class_zero} kerning value{} used class 0 (no group) and {} left out.",
            crate::plural(class_zero),
            if class_zero == 1 { "was" } else { "were" }
        ));
    }
    pairs
}

/// Recovers mark anchors from GPOS mark-attachment lookups. GPOS anchors are
/// class-indexed, not named, so names are generated (mark0, mark1, …) and prefixed
/// with "_" on the mark glyph itself.
fn collect_anchors(font: &FontRef, warnings: &mut Vec<String>) -> HashMap<u32, Vec<Anchor>> {
    let mut out: HashMap<u32, Vec<Anchor>> = HashMap::new();
    let Ok(gpos) = font.gpos() else { return out };
    let Ok(list) = gpos.lookup_list() else {
        return out;
    };
    let mut found = false;
    for lookup in list.lookups().iter().flatten() {
        match lookup.lookup_type() {
            4 => {
                let Ok(PositionSubtables::MarkToBase(subs)) = lookup.subtables() else {
                    continue;
                };
                for t in subs.iter().flatten() {
                    let (Ok(marks), Ok(bases)) = (t.mark_coverage(), t.base_coverage()) else {
                        continue;
                    };
                    let marks: Vec<u32> = marks.iter().map(|g| g.to_u32()).collect();
                    let bases: Vec<u32> = bases.iter().map(|g| g.to_u32()).collect();
                    let (Ok(ma), Ok(ba)) = (t.mark_array(), t.base_array()) else {
                        continue;
                    };
                    found = true;
                    add_mark_anchors(&ma, &marks, &mut out);
                    for (i, rec) in ba.base_records().iter().enumerate() {
                        let Some(&gid) = bases.get(i) else { continue };
                        let Ok(rec) = rec else { continue };
                        for (class, anchor) in rec.base_anchors(ba.offset_data()).iter().enumerate()
                        {
                            if let Some(Ok(a)) = anchor {
                                add_anchor(
                                    &mut out,
                                    gid,
                                    format!("mark{class}"),
                                    a.x_coordinate(),
                                    a.y_coordinate(),
                                );
                            }
                        }
                    }
                }
            }
            5 => {
                let Ok(PositionSubtables::MarkToLig(subs)) = lookup.subtables() else {
                    continue;
                };
                for t in subs.iter().flatten() {
                    let (Ok(marks), Ok(ligs)) = (t.mark_coverage(), t.ligature_coverage()) else {
                        continue;
                    };
                    let marks: Vec<u32> = marks.iter().map(|g| g.to_u32()).collect();
                    let ligs: Vec<u32> = ligs.iter().map(|g| g.to_u32()).collect();
                    let (Ok(ma), Ok(la)) = (t.mark_array(), t.ligature_array()) else {
                        continue;
                    };
                    found = true;
                    add_mark_anchors(&ma, &marks, &mut out);
                    for (i, attach) in la.ligature_attaches().iter().enumerate() {
                        let Some(&gid) = ligs.get(i) else { continue };
                        let Ok(attach) = attach else { continue };
                        for (comp, rec) in attach.component_records().iter().enumerate() {
                            let Ok(rec) = rec else { continue };
                            for (class, anchor) in rec
                                .ligature_anchors(attach.offset_data())
                                .iter()
                                .enumerate()
                            {
                                if let Some(Ok(a)) = anchor {
                                    add_anchor(
                                        &mut out,
                                        gid,
                                        format!("mark{class}_{}", comp + 1),
                                        a.x_coordinate(),
                                        a.y_coordinate(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if found {
        warnings.push(
            "Mark anchors were imported from GPOS with generated names (mark0, mark1, …); rename them to match your anchor scheme."
                .into(),
        );
    }
    out
}

fn add_anchor(out: &mut HashMap<u32, Vec<Anchor>>, gid: u32, name: String, x: i16, y: i16) {
    let entry = out.entry(gid).or_default();
    if entry.iter().any(|a| a.name == name) {
        return;
    }
    entry.push(Anchor {
        name,
        x: x as f64,
        y: y as f64,
    });
}

/// Mark glyphs carry their own attaching anchor, one per mark record.
fn add_mark_anchors(
    ma: &skrifa::raw::tables::gpos::MarkArray,
    marks: &[u32],
    out: &mut HashMap<u32, Vec<Anchor>>,
) {
    for (i, rec) in ma.mark_records().iter().enumerate() {
        let Some(&gid) = marks.get(i) else { continue };
        if let Ok(a) = rec.mark_anchor(ma.offset_data()) {
            add_anchor(
                out,
                gid,
                format!("_mark{}", rec.mark_class()),
                a.x_coordinate(),
                a.y_coordinate(),
            );
        }
    }
}
