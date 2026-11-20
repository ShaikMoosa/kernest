//! Editing the font's structure: family info, axes, masters and instances.
//!
//! Each function returns an edited copy of the font; the caller stores it through
//! `History::apply_font` so the change can be undone.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use crate::designspace::{normalize, Location};
use crate::model::{Axis, Font, Instance, KernPair, Layer, Master, Metrics, VariableFontSetting};

type R<T> = Result<T, String>;

/// The longest family name accepted. Font names can technically be longer, but
/// many apps truncate long names and PostScript names stop at 63 characters.
pub const MAX_FAMILY_NAME: usize = 64;

/// A trimmed, non-empty family name of reasonable length.
pub fn check_family(name: &str) -> R<&str> {
    let family = name.trim();
    if family.is_empty() {
        return Err("Enter a family name.".into());
    }
    if family.chars().count() > MAX_FAMILY_NAME {
        return Err(format!(
            "A family name can be at most {MAX_FAMILY_NAME} characters ({} entered).",
            family.chars().count()
        ));
    }
    Ok(family)
}

pub fn set_info(
    font: &Font,
    family_name: &str,
    units_per_em: u16,
    version_major: u16,
    version_minor: u16,
) -> R<Font> {
    let family = check_family(family_name)?;
    if !(16..=16384).contains(&units_per_em) {
        return Err("Units per em must be between 16 and 16384.".into());
    }
    let mut f = font.clone();
    f.info.family_name = family.into();
    f.info.units_per_em = units_per_em;
    f.info.version_major = version_major;
    f.info.version_minor = version_minor;
    Ok(f)
}

/// Sets the TrueType autohinting settings.
pub fn set_hinting(font: &Font, hinting: crate::Hinting) -> R<Font> {
    if !(1..=1000).contains(&hinting.range_min) || !(1..=1000).contains(&hinting.range_max) {
        return Err("Hinting sizes must be between 1 and 1000 pixels per em.".into());
    }
    if hinting.range_min > hinting.range_max {
        return Err("The smallest hinted size can't be larger than the largest.".into());
    }
    if hinting.increase_x_height != 0 && !(6..=1000).contains(&hinting.increase_x_height) {
        return Err("Increase x-height must be 0 (off) or between 6 and 1000.".into());
    }
    let mut f = font.clone();
    f.info.hinting = (hinting != crate::Hinting::default()).then_some(hinting);
    Ok(f)
}

/// Guides need a finite position and angle; anything else would draw off-canvas.
pub fn check_guides(guides: &[crate::Guide]) -> R<()> {
    for g in guides {
        if !g.x.is_finite() || !g.y.is_finite() || !g.angle.is_finite() {
            return Err("A guide has an invalid position or angle.".into());
        }
    }
    Ok(())
}

/// Sets (or clears, with `None` or a zero spacing) the edit view's snap grid.
pub fn set_grid(font: &Font, grid: Option<crate::Grid>) -> R<Font> {
    let mut f = font.clone();
    f.info.grid = grid.filter(|g| g.step().is_some());
    Ok(f)
}

/// Replaces a master's global guides, drawn in every glyph of that master.
pub fn set_master_guides(font: &Font, master_id: &str, guides: Vec<crate::Guide>) -> R<Font> {
    check_guides(&guides)?;
    let mut f = font.clone();
    let m = f
        .masters
        .iter_mut()
        .find(|m| m.id == master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    m.guides = guides;
    Ok(f)
}

/// Sets a master's line spacing and underline values; `None` leaves a value to the
/// compiler. Win values are distances, so they can't be negative.
pub fn set_vertical_metrics(
    font: &Font,
    master_id: &str,
    vertical: crate::VerticalMetrics,
) -> R<Font> {
    let v = &vertical;
    let all = [
        v.typo_ascender,
        v.typo_descender,
        v.typo_line_gap,
        v.hhea_ascender,
        v.hhea_descender,
        v.hhea_line_gap,
        v.win_ascent,
        v.win_descent,
        v.underline_thickness,
        v.underline_position,
    ];
    if all
        .iter()
        .flatten()
        .any(|x| !x.is_finite() || x.abs() > 32767.0)
    {
        return Err("Line spacing values must be numbers between -32767 and 32767.".into());
    }
    if [
        v.win_ascent,
        v.win_descent,
        v.typo_line_gap,
        v.hhea_line_gap,
        v.underline_thickness,
    ]
    .iter()
    .flatten()
    .any(|x| *x < 0.0)
    {
        return Err(
            "Win ascent, win descent, line gaps and underline thickness can't be negative.".into(),
        );
    }
    let mut f = font.clone();
    let m = f
        .masters
        .iter_mut()
        .find(|m| m.id == master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
    m.vertical = vertical;
    Ok(f)
}

/// Replaces the kept custom parameters. Names are trimmed and can't be empty; the
/// same name may appear more than once (Glyphs allows it, e.g. "Name Table Entry").
pub fn set_custom_parameters(font: &Font, list: Vec<crate::CustomParameter>) -> R<Font> {
    let mut clean = Vec::with_capacity(list.len());
    for mut p in list {
        p.name = p.name.trim().to_string();
        if p.name.is_empty() {
            return Err("A custom parameter needs a name.".into());
        }
        clean.push(p);
    }
    let mut f = font.clone();
    f.info.custom_parameters = clean;
    Ok(f)
}

/// Replaces the virtual masters. Each needs a name (unique) and a location on the
/// font's axes, inside each axis' range (extend the axis first if needed).
/// Replaces the number values for feature tokens (S64): by name, then by master id.
/// Names are like `padding` (letters, digits and _, not starting with a digit).
pub fn set_numbers(
    font: &Font,
    numbers: std::collections::BTreeMap<String, std::collections::BTreeMap<String, f64>>,
) -> R<Font> {
    let mut out = std::collections::BTreeMap::new();
    for (name, by_master) in numbers {
        let n = name.trim().to_string();
        let ok = n
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok {
            return Err(format!(
                "\"{n}\" can't be a number name: use letters, digits and _, starting with a letter."
            ));
        }
        let mut values = std::collections::BTreeMap::new();
        for m in &font.masters {
            let v = by_master.get(&m.id).copied().unwrap_or(0.0);
            if !v.is_finite() {
                return Err(format!("{n}: enter a number for {}.", m.name));
            }
            values.insert(m.id.clone(), v);
        }
        out.insert(n, values);
    }
    let mut f = font.clone();
    f.info.numbers = out;
    Ok(f)
}

pub fn set_virtual_masters(font: &Font, list: Vec<crate::VirtualMaster>) -> R<Font> {
    let mut names = std::collections::HashSet::new();
    for vm in &list {
        let name = vm.name.trim();
        if name.is_empty() {
            return Err("A virtual master needs a name.".into());
        }
        if !names.insert(name.to_string()) {
            return Err(format!("There are two virtual masters named \"{name}\"."));
        }
        for (tag, v) in &vm.location {
            let a = font
                .axes
                .iter()
                .find(|a| &a.tag == tag)
                .ok_or_else(|| format!("{name}: there is no axis \"{tag}\"."))?;
            let (lo, _, hi) = crate::designspace::design_bounds(a);
            if !v.is_finite() || *v < lo.min(hi) || *v > lo.max(hi) {
                return Err(format!(
                    "{name}: {} {v} is outside the axis range; widen the axis in Font Info › Axes first.",
                    a.name
                ));
            }
        }
    }
    let mut f = font.clone();
    f.info.virtual_masters = list
        .into_iter()
        .map(|mut vm| {
            vm.name = vm.name.trim().to_string();
            vm
        })
        .collect();
    Ok(f)
}

/// Turns "Use Typo Metrics" (OS/2 fsSelection bit 7) on or off for the whole font.
pub fn set_use_typo_metrics(font: &Font, on: bool) -> R<Font> {
    let mut f = font.clone();
    f.info.use_typo_metrics = on;
    Ok(f)
}

/// Moves one guide between a layer's local list and its master's global list,
/// keeping its position, angle, name and lock. Appended to the target list.
pub fn move_guide(font: &Font, glyph: &str, key: &str, index: usize, to_global: bool) -> R<Font> {
    let mut f = font.clone();
    let guide = if to_global {
        let layer = f
            .glyph_mut(glyph)
            .and_then(|g| g.layer_mut(key))
            .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
        if index >= layer.guides.len() {
            return Err("No guide at that position.".into());
        }
        layer.guides.remove(index)
    } else {
        let m = f
            .masters
            .iter_mut()
            .find(|m| m.id == key)
            .ok_or_else(|| format!("No master with id \"{key}\""))?;
        if index >= m.guides.len() {
            return Err("No guide at that position.".into());
        }
        m.guides.remove(index)
    };
    if to_global {
        let m = f
            .masters
            .iter_mut()
            .find(|m| m.id == key)
            .ok_or_else(|| format!("No master with id \"{key}\""))?;
        m.guides.push(guide);
    } else {
        let layer = f
            .glyph_mut(glyph)
            .and_then(|g| g.layer_mut(key))
            .ok_or_else(|| format!("No layer for \"{glyph}\""))?;
        layer.guides.push(guide);
    }
    Ok(f)
}

fn check_axes(axes: &[Axis]) -> R<()> {
    let mut tags = HashSet::new();
    for a in axes {
        if a.name.trim().is_empty() {
            return Err("Every axis needs a name.".into());
        }
        if a.tag.len() != 4 || !a.tag.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
            return Err(format!(
                "Axis tag \"{}\" must be 4 ASCII characters, like wght.",
                a.tag
            ));
        }
        if !tags.insert(a.tag.as_str()) {
            return Err(format!("Axis tag \"{}\" is used twice.", a.tag));
        }
        if !(a.min.is_finite() && a.default.is_finite() && a.max.is_finite())
            || !(a.min <= a.default && a.default <= a.max)
        {
            return Err(format!(
                "Axis \"{}\" needs minimum ≤ default ≤ maximum.",
                a.name
            ));
        }
        let mut prev: Option<(f64, f64)> = None;
        for &(u, d) in &a.map {
            if !(u.is_finite() && d.is_finite()) || u < a.min || u > a.max {
                return Err(format!(
                    "Axis \"{}\" mapping points must be finite user values inside the axis range.",
                    a.name
                ));
            }
            if let Some((pu, pd)) = prev {
                if u <= pu || d < pd {
                    return Err(format!(
                        "Axis \"{}\" mapping must rise: user values strictly up, design values never down.",
                        a.name
                    ));
                }
            }
            prev = Some((u, d));
        }
        for l in &a.labels {
            if l.name.trim().is_empty() {
                return Err(format!(
                    "Every value label on axis \"{}\" needs a name.",
                    a.name
                ));
            }
            let in_range = |v: f64| v.is_finite() && v >= a.min && v <= a.max;
            if !in_range(l.value) {
                return Err(format!(
                    "Axis \"{}\" value label \"{}\" must sit inside the axis range.",
                    a.name, l.name
                ));
            }
            if let (Some(lo), Some(hi)) = (l.min, l.max) {
                if !in_range(lo) || !in_range(hi) || lo > l.value || l.value > hi {
                    return Err(format!(
                        "Axis \"{}\" value label \"{}\" needs a range around its value.",
                        a.name, l.name
                    ));
                }
            } else if l.min.is_some() != l.max.is_some() {
                return Err(format!(
                    "Axis \"{}\" value label \"{}\" needs both ends of a range, or neither.",
                    a.name, l.name
                ));
            }
            if let Some(linked) = l.linked_value {
                if !in_range(linked) {
                    return Err(format!(
                        "Axis \"{}\" value label \"{}\" links to a value outside the axis range.",
                        a.name, l.name
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Replaces the axes. Masters and instances lose locations on removed axes and get
/// the default on new axes.
pub fn set_axes(font: &Font, axes: Vec<Axis>) -> R<Font> {
    check_axes(&axes)?;
    let mut f = font.clone();
    let fix = |loc: &mut BTreeMap<String, f64>| {
        loc.retain(|tag, _| axes.iter().any(|a| &a.tag == tag));
        for a in &axes {
            // Locations are design-space; the default a new axis gets is the mapped one.
            let dflt = crate::designspace::design_bounds(a).1;
            loc.entry(a.tag.clone()).or_insert(dflt);
        }
    };
    f.masters.iter_mut().for_each(|m| fix(&mut m.location));
    f.instances.iter_mut().for_each(|i| fix(&mut i.location));
    // Brace layers move like masters (and go when there are no axes left); bracket
    // layers lose ranges on removed axes, and are dropped if no range is left.
    for g in &mut f.glyphs {
        for l in &mut g.layers {
            if let Some(loc) = &mut l.location {
                fix(loc);
            }
            l.bracket.retain(|r| axes.iter().any(|a| a.tag == r.tag));
        }
        g.layers.retain(|l| {
            l.is_master()
                || (l.location.is_some() && !axes.is_empty())
                || !l.bracket.is_empty()
                || l.smart.is_some()
                || l.color.is_some()
        });
    }
    f.axes = axes;
    Ok(f)
}

fn check_location(font: &Font, loc: &BTreeMap<String, f64>) -> R<BTreeMap<String, f64>> {
    let mut out = BTreeMap::new();
    for a in &font.axes {
        let (min, dflt, max) = crate::designspace::design_bounds(a);
        let v = loc.get(&a.tag).copied().unwrap_or(dflt);
        if !v.is_finite() || v < min || v > max {
            return Err(format!("{} must be between {} and {}.", a.name, min, max));
        }
        out.insert(a.tag.clone(), v);
    }
    Ok(out)
}

fn new_master_id(font: &Font) -> String {
    (1..)
        .map(|n| format!("m{n:02}"))
        .find(|id| font.master(id).is_none())
        .expect("ids are unbounded")
}

/// Adds a master that starts as a copy of `copy_from` (outlines, metrics and kerning).
/// Returns the new font and the new master's id.
pub fn add_master(
    font: &Font,
    copy_from: &str,
    name: &str,
    location: &BTreeMap<String, f64>,
) -> R<(Font, String)> {
    let src = font
        .master(copy_from)
        .ok_or_else(|| format!("No master with id \"{copy_from}\"."))?
        .clone();
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a master name.".into());
    }
    let location = check_location(font, location)?;
    let mut f = font.clone();
    let id = new_master_id(&f);
    for g in &mut f.glyphs {
        if let Some(l) = g.layer(copy_from).cloned() {
            let mut l = l;
            l.master_id = id.clone();
            g.layers.push(l);
        }
    }
    if let Some(k) = f.kerning.get(copy_from).cloned() {
        f.kerning.insert(id.clone(), k);
    }
    f.masters.push(crate::model::Master {
        id: id.clone(),
        name: name.into(),
        location,
        metrics: src.metrics,
        guides: src.guides.clone(),
        alignment_zones: src.alignment_zones.clone(),
        stems: src.stems.clone(),
        vertical: src.vertical,
    });
    Ok((f, id))
}

/// A glyph in the current font whose incoming layer can't interpolate with the default.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignGlyphIssue {
    pub glyph: String,
    pub issues: Vec<String>,
}

/// What adding one master from another font would do, shown before the user commits:
/// how many glyphs the source master carries, how many line up with this font, which
/// glyphs are missing (they become empty layers) and which won't interpolate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignMasterPreview {
    pub master_id: String,
    pub name: String,
    pub location: Location,
    /// Glyphs the source master has a layer for.
    pub glyph_count: usize,
    /// Current glyphs that also exist in the source master.
    pub matched: usize,
    /// Current glyphs the source master lacks; they are added as empty layers.
    pub missing: Vec<String>,
    pub incompatible: Vec<ForeignGlyphIssue>,
}

/// Reports compatibility of one `source` master against the current font's default master,
/// without changing anything. The reference is the current default layer of each glyph.
pub fn preview_foreign_master(
    current: &Font,
    source: &Font,
    master_id: &str,
) -> R<ForeignMasterPreview> {
    let src = source
        .master(master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\" in the source font."))?;
    let def = crate::interpolate::default_master(current);
    let glyph_count = source
        .glyphs
        .iter()
        .filter(|g| g.layer(master_id).is_some())
        .count();
    let mut matched = 0;
    let mut missing = Vec::new();
    let mut incompatible = Vec::new();
    for g in &current.glyphs {
        let Some(reference) = g.layer(&def.id) else {
            continue;
        };
        match source.glyph(&g.name).and_then(|sg| sg.layer(master_id)) {
            None => missing.push(g.name.clone()),
            Some(layer) => {
                matched += 1;
                let issues = crate::interpolate::compare(&src.name, reference, &def.name, layer);
                if !issues.is_empty() {
                    incompatible.push(ForeignGlyphIssue {
                        glyph: g.name.clone(),
                        issues,
                    });
                }
            }
        }
    }
    Ok(ForeignMasterPreview {
        master_id: master_id.into(),
        name: src.name.clone(),
        location: src.location.clone(),
        glyph_count,
        matched,
        missing,
        incompatible,
    })
}

/// Adds a master copied from another font's `master_id`, placed at `location` in this
/// font's design space. Glyphs the source lacks become empty layers (returned as the
/// warning list); kerning is kept only for pairs whose sides exist in this font.
/// Returns the new font and the names of the glyphs filled as empty layers.
pub fn add_master_from(
    current: &Font,
    source: &Font,
    master_id: &str,
    name: &str,
    location: &Location,
) -> R<(Font, Vec<String>)> {
    let src = source
        .master(master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\" in the source font."))?;
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a master name.".into());
    }
    let location = check_location(current, location)?;
    let def = crate::interpolate::default_master(current);
    let mut f = current.clone();
    let id = new_master_id(&f);
    let mut missing = Vec::new();
    for g in &mut f.glyphs {
        match source.glyph(&g.name).and_then(|sg| sg.layer(master_id)) {
            Some(layer) => {
                let mut l = layer.clone();
                l.master_id = id.clone();
                g.layers.push(l);
            }
            None => {
                let width = g.layer(&def.id).map_or(0.0, |l| l.width);
                g.layers.push(Layer::new(id.clone(), width));
                missing.push(g.name.clone());
            }
        }
    }
    // Keep kerning only for sides this font knows: a glyph name, or a group some glyph uses.
    let glyphs: HashSet<&str> = current.glyphs.iter().map(|g| g.name.as_str()).collect();
    let mut groups: HashSet<&str> = HashSet::new();
    for g in &current.glyphs {
        if let Some(l) = &g.left_group {
            groups.insert(l);
        }
        if let Some(r) = &g.right_group {
            groups.insert(r);
        }
    }
    let resolves = |side: &str| match side.strip_prefix('@') {
        Some(g) => groups.contains(g),
        None => glyphs.contains(side),
    };
    if let Some(pairs) = source.kerning.get(master_id) {
        let kept: Vec<KernPair> = pairs
            .iter()
            .filter(|p| resolves(&p.left) && resolves(&p.right))
            .cloned()
            .collect();
        if !kept.is_empty() {
            f.kerning.insert(id.clone(), kept);
        }
    }
    f.masters.push(Master {
        id,
        name: name.into(),
        location,
        metrics: src.metrics,
        guides: src.guides.clone(),
        alignment_zones: src.alignment_zones.clone(),
        stems: src.stems.clone(),
        vertical: src.vertical,
    });
    Ok((f, missing))
}

pub fn update_master(
    font: &Font,
    id: &str,
    name: &str,
    location: &BTreeMap<String, f64>,
    metrics: Metrics,
) -> R<Font> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a master name.".into());
    }
    let location = check_location(font, location)?;
    let m = [
        metrics.ascender,
        metrics.cap_height,
        metrics.x_height,
        metrics.descender,
        metrics.italic_angle,
    ];
    if m.iter().any(|v| !v.is_finite()) {
        return Err("Metrics must be numbers.".into());
    }
    let mut f = font.clone();
    let master = f
        .masters
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| format!("No master with id \"{id}\"."))?;
    master.name = name.into();
    master.location = location;
    master.metrics = metrics;
    Ok(f)
}

/// Removes a master with its layers and kerning. The last master can't be removed.
pub fn remove_master(font: &Font, id: &str) -> R<Font> {
    if font.master(id).is_none() {
        return Err(format!("No master with id \"{id}\"."));
    }
    if font.masters.len() == 1 {
        return Err("A font needs at least one master.".into());
    }
    let mut f = font.clone();
    f.masters.retain(|m| m.id != id);
    f.glyphs
        .iter_mut()
        .for_each(|g| g.layers.retain(|l| l.master_id != id));
    f.kerning.remove(id);
    Ok(f)
}

pub fn set_instances(font: &Font, instances: Vec<Instance>) -> R<Font> {
    let mut names = HashSet::new();
    let mut out = Vec::with_capacity(instances.len());
    for mut i in instances {
        i.name = i.name.trim().to_string();
        if i.name.is_empty() {
            return Err("Every instance needs a name.".into());
        }
        // A family name of its own; the font's family (or nothing) means none.
        i.family_name = i
            .family_name
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty() && *f != font.info.family_name);
        if let Some(f) = &i.family_name {
            if f.chars().count() > 63 {
                return Err(format!(
                    "Family name \"{f}\" is too long; use at most 63 characters."
                ));
            }
        }
        // Names only need to be unique within a family: one source can export a
        // "Bold" for each of its families.
        if !names.insert((i.family_name.clone(), i.name.clone())) {
            return Err(match &i.family_name {
                Some(f) => format!(
                    "Instance name \"{}\" is used twice in family \"{f}\".",
                    i.name
                ),
                None => format!("Instance name \"{}\" is used twice.", i.name),
            });
        }
        i.location = check_location(font, &i.location)?;
        out.push(i);
    }
    let mut f = font.clone();
    f.instances = out;
    Ok(f)
}

/// Replaces the family's variable fonts. Empty means one variable font at full range.
pub fn set_variable_fonts(font: &Font, list: Vec<VariableFontSetting>) -> R<Font> {
    let mut names = HashSet::new();
    let mut files = HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for mut s in list {
        s.name = s.name.trim().to_string();
        s.file_name = s.file_name.trim().to_string();
        if s.name.is_empty() {
            return Err("Every variable font needs a name.".into());
        }
        if !names.insert(s.name.clone()) {
            return Err(format!("Variable font name \"{}\" is used twice.", s.name));
        }
        if s.file_name.is_empty() {
            return Err(format!("Variable font \"{}\" needs a file name.", s.name));
        }
        if s.file_name.contains(['/', '\\', ':']) {
            return Err(format!(
                "File name \"{}\" must not contain a path.",
                s.file_name
            ));
        }
        if !files.insert(s.file_name.clone()) {
            return Err(format!("File name \"{}\" is used twice.", s.file_name));
        }
        let mut tags = HashSet::new();
        for sub in &s.axes {
            let Some(a) = font.axes.iter().find(|a| a.tag == sub.tag) else {
                return Err(format!(
                    "Variable font \"{}\" subsets unknown axis \"{}\".",
                    s.name, sub.tag
                ));
            };
            if !tags.insert(sub.tag.clone()) {
                return Err(format!(
                    "Variable font \"{}\" lists axis \"{}\" twice.",
                    s.name, sub.tag
                ));
            }
            let lo = sub.min.unwrap_or(a.min);
            let hi = sub.max.unwrap_or(a.max);
            if !(lo.is_finite() && hi.is_finite()) || lo > hi || lo < a.min || hi > a.max {
                return Err(format!(
                    "Variable font \"{}\" range on \"{}\" must sit inside the axis range, minimum ≤ maximum.",
                    s.name, a.name
                ));
            }
        }
        for g in &s.glyphs {
            if font.glyph(g).is_none() {
                return Err(format!(
                    "Variable font \"{}\" keeps unknown glyph \"{}\".",
                    s.name, g
                ));
            }
        }
        out.push(s);
    }
    let mut f = font.clone();
    f.variable_fonts = out;
    Ok(f)
}

/// Problems with the design space that block interpolation or export.
pub fn warnings(font: &Font) -> Vec<String> {
    let mut w = Vec::new();
    if font.masters.len() > 1 && font.axes.is_empty() {
        w.push("There are several masters but no axes. Add an axis in Font Info › Axes.".into());
    }
    if !font.axes.is_empty()
        && !font
            .masters
            .iter()
            .any(|m| normalize(font, &m.location).is_empty())
    {
        w.push(
            "No master sits at the default location of every axis; interpolation needs one.".into(),
        );
    }
    let locs: Vec<_> = font
        .masters
        .iter()
        .map(|m| normalize(font, &m.location))
        .collect();
    for (i, l) in locs.iter().enumerate() {
        if let Some(j) = locs[..i].iter().position(|o| o == l) {
            w.push(format!(
                "{} and {} are at the same location.",
                font.masters[j].name, font.masters[i].name
            ));
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    #[test]
    fn sets_line_spacing_and_refuses_bad_values() {
        let f = crate::sample::sample_font();
        let v = crate::VerticalMetrics {
            win_ascent: Some(1100.0),
            typo_line_gap: Some(200.0),
            ..Default::default()
        };
        let g = set_vertical_metrics(&f, "m01", v).unwrap();
        assert_eq!(g.masters[0].vertical, v);
        let bad = crate::VerticalMetrics {
            win_descent: Some(-5.0),
            ..Default::default()
        };
        assert!(set_vertical_metrics(&f, "m01", bad).is_err());
        assert!(set_vertical_metrics(&f, "nope", v).is_err());
        assert!(
            set_use_typo_metrics(&f, true)
                .unwrap()
                .info
                .use_typo_metrics
        );
    }

    #[test]
    fn hinting_settings() {
        use crate::Hinting;
        let f = sample_font();
        let h = Hinting {
            range_min: 10,
            ..Hinting::default()
        };
        let g = set_hinting(&f, h).unwrap();
        assert_eq!(g.info.hinting, Some(h));
        assert_eq!(
            set_hinting(&g, Hinting::default()).unwrap().info.hinting,
            None,
            "defaults aren't stored"
        );
        let bad = |h: Hinting| set_hinting(&f, h).unwrap_err();
        assert!(bad(Hinting {
            range_min: 60,
            ..Hinting::default()
        })
        .contains("larger"));
        assert!(bad(Hinting {
            range_max: 0,
            ..Hinting::default()
        })
        .contains("between 1 and 1000"));
        assert!(bad(Hinting {
            increase_x_height: 3,
            ..Hinting::default()
        })
        .contains("0 (off)"));
    }

    fn wght() -> Axis {
        Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 400.0,
            max: 900.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        }
    }

    #[test]
    fn axes_update_locations() {
        let f = set_axes(&sample_font(), vec![wght()]).unwrap();
        assert_eq!(f.masters[0].location.get("wght"), Some(&400.0));
        let f = set_axes(&f, vec![]).unwrap();
        assert!(f.masters[0].location.is_empty());
        let mut bad = wght();
        bad.tag = "weight".into();
        assert!(set_axes(&f, vec![bad]).is_err());
        let mut bad = wght();
        bad.min = 500.0;
        assert!(set_axes(&f, vec![bad]).is_err());
        assert!(set_axes(&f, vec![wght(), wght()]).is_err());
    }

    #[test]
    fn axis_map_validation_and_default_fill() {
        let mut a = wght();
        a.map = vec![(100.0, 20.0), (400.0, 300.0), (900.0, 950.0)];
        // A new axis fills missing master locations with the design default (300).
        let f = set_axes(&sample_font(), vec![a.clone()]).unwrap();
        assert_eq!(f.masters[0].location.get("wght"), Some(&300.0));

        a.map = vec![(400.0, 300.0), (100.0, 20.0)];
        assert!(set_axes(&sample_font(), vec![a.clone()]).is_err());
        a.map = vec![(100.0, 300.0), (900.0, 20.0)];
        assert!(set_axes(&sample_font(), vec![a.clone()]).is_err());
        a.map = vec![(50.0, 20.0)];
        assert!(set_axes(&sample_font(), vec![a.clone()]).is_err());
        a.map = vec![(100.0, 20.0), (100.0, 30.0)];
        assert!(set_axes(&sample_font(), vec![a]).is_err());
    }

    #[test]
    fn axis_label_validation() {
        use crate::AxisValue;
        let label = |name: &str, value: f64| AxisValue {
            name: name.into(),
            value,
            min: None,
            max: None,
            elidable: false,
            linked_value: None,
        };
        let mut a = wght();
        a.labels = vec![label("Regular", 400.0), label("Bold", 700.0)];
        set_axes(&sample_font(), vec![a]).unwrap();

        let mut bad = wght();
        bad.labels = vec![label("", 400.0)];
        assert!(set_axes(&sample_font(), vec![bad])
            .unwrap_err()
            .contains("needs a name"));

        let mut bad = wght();
        bad.labels = vec![label("Too heavy", 950.0)];
        assert!(set_axes(&sample_font(), vec![bad])
            .unwrap_err()
            .contains("inside the axis range"));

        let mut bad = wght();
        bad.labels = vec![AxisValue {
            min: Some(500.0),
            ..label("Range", 600.0)
        }];
        assert!(set_axes(&sample_font(), vec![bad])
            .unwrap_err()
            .contains("both ends"));

        let mut bad = wght();
        bad.labels = vec![AxisValue {
            min: Some(700.0),
            max: Some(800.0),
            ..label("Range", 600.0)
        }];
        assert!(set_axes(&sample_font(), vec![bad])
            .unwrap_err()
            .contains("range around its value"));

        let mut bad = wght();
        bad.labels = vec![AxisValue {
            linked_value: Some(950.0),
            ..label("Bold", 700.0)
        }];
        assert!(set_axes(&sample_font(), vec![bad])
            .unwrap_err()
            .contains("outside the axis range"));
    }

    #[test]
    fn variable_font_validation() {
        use crate::model::{VariableAxisSubset, VariableFontSetting};
        let f = set_axes(&sample_font(), vec![wght()]).unwrap();
        let vf = |name: &str, file: &str| VariableFontSetting {
            name: name.into(),
            file_name: file.into(),
            axes: vec![VariableAxisSubset {
                tag: "wght".into(),
                min: Some(400.0),
                max: Some(700.0),
            }],
            glyphs: Vec::new(),
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
        };
        let ok =
            set_variable_fonts(&f, vec![vf("Light", "F-Light"), vf("Bold", "F-Bold")]).unwrap();
        assert_eq!(ok.variable_fonts.len(), 2);
        assert!(set_variable_fonts(&f, vec![])
            .unwrap()
            .variable_fonts
            .is_empty());

        assert!(set_variable_fonts(&f, vec![vf("", "F")])
            .unwrap_err()
            .contains("needs a name"));
        assert!(set_variable_fonts(&f, vec![vf("A", "F"), vf("A", "G")])
            .unwrap_err()
            .contains("used twice"));
        assert!(set_variable_fonts(&f, vec![vf("A", "")])
            .unwrap_err()
            .contains("needs a file name"));
        assert!(set_variable_fonts(&f, vec![vf("A", "dir/F")])
            .unwrap_err()
            .contains("must not contain a path"));
        assert!(set_variable_fonts(&f, vec![vf("A", "F"), vf("B", "F")])
            .unwrap_err()
            .contains("used twice"));

        let mut bad = vf("A", "F");
        bad.axes[0].tag = "wdth".into();
        assert!(set_variable_fonts(&f, vec![bad])
            .unwrap_err()
            .contains("unknown axis"));

        let mut bad = vf("A", "F");
        bad.axes[0].max = Some(950.0);
        assert!(set_variable_fonts(&f, vec![bad])
            .unwrap_err()
            .contains("inside the axis range"));

        let mut bad = vf("A", "F");
        bad.axes[0].min = Some(800.0);
        bad.axes[0].max = Some(500.0);
        assert!(set_variable_fonts(&f, vec![bad]).is_err());

        let mut bad = vf("A", "F");
        bad.glyphs = vec!["nope".into()];
        assert!(set_variable_fonts(&f, vec![bad])
            .unwrap_err()
            .contains("unknown glyph"));
    }

    #[test]
    fn add_update_remove_master() {
        let f = set_axes(&sample_font(), vec![wght()]).unwrap();
        let (f, id) =
            add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)])).unwrap();
        assert_eq!(id, "m02");
        f.validate().unwrap();
        assert_eq!(f.glyph("H").unwrap().layers.len(), 2);
        assert_eq!(f.kerning.get("m02").map(Vec::len), Some(2));
        assert!(warnings(&f).is_empty());
        assert!(add_master(
            &f,
            "m01",
            "Too heavy",
            &BTreeMap::from([("wght".into(), 1000.0)])
        )
        .is_err());

        let f = update_master(
            &f,
            "m02",
            "Black",
            &BTreeMap::from([("wght".into(), 900.0)]),
            Metrics::default(),
        )
        .unwrap();
        assert_eq!(f.master("m02").unwrap().name, "Black");

        let f2 = remove_master(&f, "m02").unwrap();
        f2.validate().unwrap();
        assert_eq!(f2.glyph("H").unwrap().layers.len(), 1);
        assert!(!f2.kerning.contains_key("m02"));
        assert!(remove_master(&f2, "m01").is_err());
    }

    #[test]
    fn design_space_warnings() {
        let f = set_axes(&sample_font(), vec![wght()]).unwrap();
        // Move the only master off the default: no default master.
        let f = update_master(
            &f,
            "m01",
            "Regular",
            &BTreeMap::from([("wght".into(), 500.0)]),
            Metrics::default(),
        )
        .unwrap();
        assert_eq!(warnings(&f).len(), 1);
        let (f, _) =
            add_master(&f, "m01", "Copy", &BTreeMap::from([("wght".into(), 500.0)])).unwrap();
        assert!(warnings(&f).iter().any(|w| w.contains("same location")));
    }

    #[test]
    fn instances_and_info() {
        let f = set_axes(&sample_font(), vec![wght()]).unwrap();
        let i = |n: &str, v: f64| Instance {
            name: n.into(),
            location: BTreeMap::from([("wght".into(), v)]),
            export: true,
            weight_class: None,
            width_class: None,
            style_link: None,
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
            family_name: None,
            localized_family_names: Default::default(),
        };
        let f = set_instances(&f, vec![i("Light", 300.0), i("Bold", 700.0)]).unwrap();
        assert_eq!(f.instances.len(), 2);
        assert!(set_instances(&f, vec![i("A", 300.0), i("A", 400.0)]).is_err());
        assert!(set_instances(&f, vec![i(" ", 300.0)]).is_err());
        assert!(set_instances(&f, vec![i("X", 2000.0)]).is_err());
        // Names are unique per family: one "Bold" in each family is fine.
        let fam = |mut x: Instance, f: &str| {
            x.family_name = Some(f.into());
            x
        };
        let g =
            set_instances(&f, vec![i("Bold", 700.0), fam(i("Bold", 700.0), " Other ")]).unwrap();
        assert_eq!(g.instances[1].family_name.as_deref(), Some("Other"));
        let err = set_instances(
            &f,
            vec![fam(i("A", 300.0), "Other"), fam(i("A", 400.0), "Other")],
        )
        .unwrap_err();
        assert!(err.contains("in family \"Other\""), "{err}");
        // The font's own family, or an empty one, means none.
        let own = f.info.family_name.clone();
        let g =
            set_instances(&f, vec![fam(i("A", 300.0), &own), fam(i("B", 400.0), "  ")]).unwrap();
        assert!(g.instances.iter().all(|x| x.family_name.is_none()));
        assert!(set_instances(&f, vec![i("A", 300.0), fam(i("A", 400.0), &own)]).is_err());
        assert!(set_instances(&f, vec![fam(i("A", 300.0), &"x".repeat(64))]).is_err());
        let f = set_info(&f, " Kernest Sans ", 2048, 1, 5).unwrap();
        assert_eq!(
            (f.info.family_name.as_str(), f.info.units_per_em),
            ("Kernest Sans", 2048)
        );
        assert!(set_info(&f, "", 1000, 1, 0).is_err());
        assert!(set_info(&f, "X", 8, 1, 0).is_err());
    }

    #[test]
    fn add_master_from_other_font_reports_compatibility() {
        use crate::model::{Node, NodeKind, Shape};
        let current = set_axes(&sample_font(), vec![wght()]).unwrap();
        // A source font with a Bold master, one incompatible glyph and one glyph removed.
        let (mut source, _) = add_master(
            &set_axes(&sample_font(), vec![wght()]).unwrap(),
            "m01",
            "Bold",
            &BTreeMap::from([("wght".into(), 700.0)]),
        )
        .unwrap();
        if let Shape::Path(p) = &mut source
            .glyph_mut("H")
            .unwrap()
            .layer_mut("m02")
            .unwrap()
            .shapes[0]
        {
            p.nodes.push(Node::new(0.0, 0.0, NodeKind::Line));
        }
        source.glyphs.retain(|g| g.name != "I");

        let p = preview_foreign_master(&current, &source, "m02").unwrap();
        assert_eq!(p.name, "Bold");
        assert_eq!(p.location.get("wght"), Some(&700.0));
        assert_eq!(p.missing, vec!["I".to_string()]);
        assert_eq!(p.matched, current.glyphs.len() - 1);
        assert_eq!(p.glyph_count, source.glyphs.len());
        assert!(p.incompatible.iter().any(|i| i.glyph == "H"));
        assert!(preview_foreign_master(&current, &source, "nope").is_err());

        let (f, missing) = add_master_from(
            &current,
            &source,
            "m02",
            "Bold",
            &BTreeMap::from([("wght".into(), 700.0)]),
        )
        .unwrap();
        assert_eq!(missing, vec!["I".to_string()]);
        f.validate().unwrap();
        assert_eq!(f.master("m02").unwrap().name, "Bold");
        // The missing glyph becomes an empty layer keeping the current advance.
        let i = f.glyph("I").unwrap().layer("m02").unwrap();
        assert!(i.shapes.is_empty());
        assert_eq!(
            i.width,
            current.glyph("I").unwrap().layer("m01").unwrap().width
        );
        // The incompatible glyph is still copied; the report warned about it.
        assert!(f.glyph("H").unwrap().layer("m02").is_some());
        // Kerning keeps the pairs whose sides exist in this font.
        assert_eq!(f.kerning.get("m02").map(Vec::len), Some(2));
        assert!(add_master_from(
            &current,
            &source,
            "m02",
            " ",
            &BTreeMap::from([("wght".into(), 700.0)])
        )
        .is_err());
        assert!(add_master_from(
            &current,
            &source,
            "m02",
            "X",
            &BTreeMap::from([("wght".into(), 2000.0)])
        )
        .is_err());
    }
}
