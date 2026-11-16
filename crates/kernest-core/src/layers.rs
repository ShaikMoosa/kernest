//! Brace (intermediate) and bracket (alternate) layers.
//!
//! - A **brace layer** is an extra source at its own design-space location, for glyphs
//!   that need a correction between masters (like Glyphs' `{550}` layers).
//! - **Bracket layers** are an alternate design used wherever the location is inside
//!   their axis ranges (like Glyphs' `[600]` layers): one per master, all with the
//!   same ranges. A master without one falls back to its own layer.
//!
//! Like the structural edits, each function returns an edited copy of the font for
//! `History::apply_font`.

use std::collections::BTreeMap;

use crate::interpolate::Interpolator;
use crate::model::{AxisRange, Font, Layer};

type R<T> = Result<T, String>;

fn full_location(font: &Font, loc: &BTreeMap<String, f64>) -> R<BTreeMap<String, f64>> {
    if font.axes.is_empty() {
        return Err(
            "Add an axis in Font Info first: intermediate layers sit between masters on an axis."
                .into(),
        );
    }
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
    if let Some(m) = font.masters.iter().find(|m| {
        font.axes
            .iter()
            .all(|a| m.location.get(&a.tag).copied().unwrap_or(a.default) == out[&a.tag])
    }) {
        return Err(format!("{} is already at that location.", m.name));
    }
    if let Some(k) = loc.keys().find(|k| !font.axes.iter().any(|a| &a.tag == *k)) {
        return Err(format!("There is no axis \"{k}\"."));
    }
    Ok(out)
}

/// Adds a brace layer to `glyph` at `location`, shown with `master_id`. It starts as the
/// glyph interpolated at that location (or a copy of the master's layer if the glyph
/// can't be interpolated yet). Returns the font and the new layer's key.
pub fn add_brace_layer(
    font: &Font,
    glyph: &str,
    master_id: &str,
    location: &BTreeMap<String, f64>,
) -> R<(Font, String)> {
    let loc = full_location(font, location)?;
    let g = font
        .glyph(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    if g.brace_layers().any(|l| l.location.as_ref() == Some(&loc)) {
        return Err("This glyph already has an intermediate layer there.".into());
    }
    let master_layer = g
        .layer(master_id)
        .ok_or_else(|| format!("\"{glyph}\" has no layer for that master."))?;
    let mut layer = Interpolator::new(font, &loc)
        .and_then(|i| i.layer(font, glyph))
        .unwrap_or_else(|_| master_layer.clone());
    let id = g.new_layer_id(master_id);
    layer.master_id = master_id.into();
    layer.id = Some(id.clone());
    layer.location = Some(loc);
    layer.bracket.clear();
    let mut f = font.clone();
    f.glyph_mut(glyph).expect("glyph exists").layers.push(layer);
    Ok((f, id))
}

fn check_ranges(font: &Font, ranges: &[AxisRange]) -> R<Vec<AxisRange>> {
    if ranges.is_empty() {
        return Err("Give at least one axis range.".into());
    }
    let mut out = Vec::new();
    for r in ranges {
        let a = font
            .axes
            .iter()
            .find(|a| a.tag == r.tag)
            .ok_or_else(|| format!("There is no axis \"{}\".", r.tag))?;
        if r.min.is_none() && r.max.is_none() {
            return Err(format!("Give a minimum or a maximum for {}.", a.name));
        }
        if let (Some(lo), Some(hi)) = (r.min, r.max) {
            if lo >= hi {
                return Err(format!(
                    "{}: the minimum must be below the maximum.",
                    a.name
                ));
            }
        }
        if out.iter().any(|o: &AxisRange| o.tag == r.tag) {
            return Err(format!("{} is used twice.", a.name));
        }
        out.push(r.clone());
    }
    out.sort_by(|a, b| a.tag.cmp(&b.tag));
    Ok(out)
}

/// Adds an alternate (bracket layers with `ranges`) to `glyph`: one layer per master,
/// each starting as a copy of that master's layer. Returns the font and the key of the
/// layer belonging to `master_id`.
pub fn add_bracket_layers(
    font: &Font,
    glyph: &str,
    master_id: &str,
    ranges: &[AxisRange],
) -> R<(Font, String)> {
    let ranges = check_ranges(font, ranges)?;
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    if g.bracket_groups().contains(&ranges) {
        return Err("This glyph already has an alternate for those ranges.".into());
    }
    let mut key = None;
    for m in &font.masters {
        let Some(base) = g.layer(&m.id).cloned() else {
            continue;
        };
        let id = g.new_layer_id(&m.id);
        let mut l: Layer = base;
        l.id = Some(id.clone());
        l.location = None;
        l.bracket = ranges.clone();
        if m.id == master_id {
            key = Some(id);
        }
        g.layers.push(l);
    }
    let key = key.ok_or_else(|| format!("\"{glyph}\" has no layer for that master."))?;
    Ok((f, key))
}

/// Moves a brace layer to a new location, or gives a bracket layer new ranges (every
/// layer of the same alternate changes with it, so they stay one alternate).
pub fn update_layer_position(
    font: &Font,
    glyph: &str,
    key: &str,
    location: Option<&BTreeMap<String, f64>>,
    ranges: Option<&[AxisRange]>,
) -> R<Font> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let layer = g
        .layer(key)
        .filter(|l| !l.is_master())
        .ok_or("Only intermediate and alternate layers can be moved.")?
        .clone();
    if layer.location.is_some() {
        let loc = full_location(font, location.ok_or("Give a location.")?)?;
        if g.brace_layers()
            .any(|l| l.key() != key && l.location.as_ref() == Some(&loc))
        {
            return Err("This glyph already has an intermediate layer there.".into());
        }
        g.layer_mut(key).expect("layer exists").location = Some(loc);
    } else {
        let new = check_ranges(font, ranges.ok_or("Give the axis ranges.")?)?;
        if new != layer.bracket && g.bracket_groups().contains(&new) {
            return Err("This glyph already has an alternate for those ranges.".into());
        }
        for l in g.layers.iter_mut().filter(|l| l.bracket == layer.bracket) {
            l.bracket = new.clone();
        }
    }
    Ok(f)
}

/// Removes a brace layer, or a whole alternate (every bracket layer with the same
/// ranges as `key`'s).
pub fn remove_layer(font: &Font, glyph: &str, key: &str) -> R<Font> {
    let mut f = font.clone();
    let g = f
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let layer = g
        .layer(key)
        .filter(|l| !l.is_master())
        .ok_or("A master's own layer can't be removed here; remove the master in Font Info.")?
        .clone();
    if layer.location.is_some() {
        g.layers.retain(|l| l.key() != key);
    } else {
        g.layers.retain(|l| l.bracket != layer.bracket);
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Axis, Master, Metrics, Shape};
    use crate::sample::sample_font;
    use crate::{edit::translate_x, interpolate::check_glyph};

    /// Sample font with wght 400–700 and a Bold master whose H is 100 units wider.
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
                l.width += 100.0;
            }
            g.layers.push(l);
        }
        f
    }

    fn width_at(f: &Font, wght: f64) -> f64 {
        let i = Interpolator::new(f, &BTreeMap::from([("wght".into(), wght)])).unwrap();
        i.layer(f, "H").unwrap().width
    }

    #[test]
    fn brace_layer_bends_the_interpolation() {
        let f = two_masters();
        let w0 = f.glyph("H").unwrap().layer("m01").unwrap().width;
        assert_eq!(
            width_at(&f, 550.0),
            w0 + 50.0,
            "straight line between masters"
        );
        let (mut f, key) =
            add_brace_layer(&f, "H", "m01", &BTreeMap::from([("wght".into(), 550.0)])).unwrap();
        assert_eq!(key, "m01~1");
        f.validate().unwrap();
        // It starts as the interpolation, so nothing changes until it's edited.
        assert_eq!(width_at(&f, 550.0), w0 + 50.0);
        f.glyph_mut("H").unwrap().layer_mut(&key).unwrap().width = w0 + 80.0;
        assert_eq!(
            width_at(&f, 550.0),
            w0 + 80.0,
            "the brace layer is a source"
        );
        assert_eq!(width_at(&f, 400.0), w0, "masters are unchanged");
        assert_eq!(width_at(&f, 700.0), w0 + 100.0);
        assert!(check_glyph(&f, "H").is_empty());
        // Making it incompatible is reported.
        let l = f.glyph_mut("H").unwrap().layer_mut(&key).unwrap();
        l.shapes.pop();
        assert!(check_glyph(&f, "H")
            .iter()
            .any(|i| i.contains("{wght 550}")));
        // Not on a master, and not twice.
        assert!(
            add_brace_layer(&f, "H", "m01", &BTreeMap::from([("wght".into(), 700.0)])).is_err()
        );
        assert!(
            add_brace_layer(&f, "H", "m01", &BTreeMap::from([("wght".into(), 550.0)])).is_err()
        );
        let f = remove_layer(&f, "H", &key).unwrap();
        assert_eq!(f.glyph("H").unwrap().layers.len(), 2);
    }

    #[test]
    fn bracket_layers_switch_designs() {
        let f = two_masters();
        let r = [AxisRange {
            tag: "wght".into(),
            min: Some(600.0),
            max: None,
        }];
        let (mut f, key) = add_bracket_layers(&f, "H", "m02", &r).unwrap();
        f.validate().unwrap();
        assert_eq!(key, "m02~1");
        let g = f.glyph("H").unwrap();
        assert_eq!(g.bracket_layers().count(), 2, "one per master");
        // Shift the alternate's outlines in both masters.
        for l in f
            .glyph_mut("H")
            .unwrap()
            .layers
            .iter_mut()
            .filter(|l| !l.bracket.is_empty())
        {
            translate_x(l, 30.0);
        }
        let x_at = |f: &Font, w: f64| {
            let i = Interpolator::new(f, &BTreeMap::from([("wght".into(), w)])).unwrap();
            let l = i.layer(f, "H").unwrap();
            let Shape::Path(p) = &l.shapes[0] else {
                panic!()
            };
            p.nodes[0].x
        };
        let x0 = x_at(&f, 400.0);
        assert_eq!(x_at(&f, 599.0), x0, "below the range: the normal design");
        assert_eq!(
            x_at(&f, 600.0),
            x0 + 30.0,
            "inside the range: the alternate"
        );
        // Static fonts at the Bold master use the alternate too.
        let bold = f.static_layer("H", "m02").unwrap();
        assert!(!bold.bracket.is_empty());
        assert!(f.static_layer("H", "m01").unwrap().is_master());
        // New ranges move the whole alternate; removing removes all of it.
        let r2 = [AxisRange {
            tag: "wght".into(),
            min: Some(500.0),
            max: Some(650.0),
        }];
        let f2 = update_layer_position(&f, "H", &key, None, Some(&r2)).unwrap();
        assert!(f2
            .glyph("H")
            .unwrap()
            .bracket_layers()
            .all(|l| l.bracket == r2));
        let f3 = remove_layer(&f2, "H", &key).unwrap();
        assert_eq!(f3.glyph("H").unwrap().bracket_layers().count(), 0);
        assert!(remove_layer(&f, "H", "m01").is_err(), "master layers stay");
    }

    #[test]
    fn removing_an_axis_cleans_up_layers() {
        let f = two_masters();
        let r = [AxisRange {
            tag: "wght".into(),
            min: Some(600.0),
            max: None,
        }];
        let (f, _) = add_bracket_layers(&f, "H", "m02", &r).unwrap();
        let (f, _) =
            add_brace_layer(&f, "H", "m01", &BTreeMap::from([("wght".into(), 550.0)])).unwrap();
        let f = crate::structure::set_axes(&f, vec![]).unwrap();
        let g = f.glyph("H").unwrap();
        assert_eq!(g.bracket_layers().count(), 0);
        assert_eq!(g.brace_layers().count(), 0);
    }

    #[test]
    fn ranges_include_both_ends() {
        let r = AxisRange {
            tag: "wght".into(),
            min: Some(600.0),
            max: Some(700.0),
        };
        assert!(!r.contains(599.9));
        assert!(r.contains(600.0) && r.contains(650.0) && r.contains(700.0));
        assert!(!r.contains(700.1));
    }
}
