//! Editing the font's structure: family info, axes, masters and instances.
//!
//! Each function returns an edited copy of the font; the caller stores it through
//! `History::apply_font` so the change can be undone.

use std::collections::{BTreeMap, HashSet};

use crate::designspace::normalize;
use crate::model::{Axis, Font, Instance, Metrics};

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
            loc.entry(a.tag.clone()).or_insert(a.default);
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
        let v = loc.get(&a.tag).copied().unwrap_or(a.default);
        if !v.is_finite() || v < a.min || v > a.max {
            return Err(format!(
                "{} must be between {} and {}.",
                a.name, a.min, a.max
            ));
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
    });
    Ok((f, id))
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
        if !names.insert(i.name.clone()) {
            return Err(format!("Instance name \"{}\" is used twice.", i.name));
        }
        i.location = check_location(font, &i.location)?;
        out.push(i);
    }
    let mut f = font.clone();
    f.instances = out;
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
        };
        let f = set_instances(&f, vec![i("Light", 300.0), i("Bold", 700.0)]).unwrap();
        assert_eq!(f.instances.len(), 2);
        assert!(set_instances(&f, vec![i("A", 300.0), i("A", 400.0)]).is_err());
        assert!(set_instances(&f, vec![i(" ", 300.0)]).is_err());
        assert!(set_instances(&f, vec![i("X", 2000.0)]).is_err());
        let f = set_info(&f, " Kernest Sans ", 2048, 1, 5).unwrap();
        assert_eq!(
            (f.info.family_name.as_str(), f.info.units_per_em),
            ("Kernest Sans", 2048)
        );
        assert!(set_info(&f, "", 1000, 1, 0).is_err());
        assert!(set_info(&f, "X", 8, 1, 0).is_err());
    }
}
