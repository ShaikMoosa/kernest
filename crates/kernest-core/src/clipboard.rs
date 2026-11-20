//! Paste Special: whole glyphs (every master's layer) copied from one font into another.
//!
//! Master layers pair up by position in the master list, so a copy from a font with
//! different master ids still lands on the right masters; brace, bracket and smart
//! layers match by their position in the design space. Pasting as components adds any
//! copied glyph the font lacks (a name clash gets `.001`) and references it.

use crate::model::{Font, Glyph, Layer, Shape, IDENTITY};

type R<T> = Result<T, String>;

const LOCKED: &str = "This glyph is locked. Unlock it to edit.";

/// The copied layer that belongs on `target`, or `None` to leave the target as it is.
fn source_for<'a>(font: &Font, target: &Layer, clip: &'a Glyph) -> Option<&'a Layer> {
    if target.is_master() {
        let idx = font.masters.iter().position(|m| m.id == target.master_id)?;
        clip.layers.iter().filter(|l| l.is_master()).nth(idx)
    } else {
        clip.layers.iter().find(|l| {
            !l.is_master()
                && l.location == target.location
                && l.bracket == target.bracket
                && l.smart == target.smart
        })
    }
}

/// The target glyph with the copied outlines, widths, anchors and metrics keys on every
/// layer that has a counterpart; its own guides and background stay.
fn replace_layers(font: &Font, target: &Glyph, clip: &Glyph) -> Glyph {
    let mut g = target.clone();
    for l in &mut g.layers {
        if let Some(src) = source_for(font, l, clip) {
            let mut next = src.clone();
            next.master_id = l.master_id.clone();
            next.id = l.id.clone();
            next.guides = l.guides.clone();
            next.background = l.background.clone();
            *l = next;
        }
    }
    g
}

/// Re-points a copied glyph's layers at this font's masters, by position; layers of
/// masters this font does not have are dropped.
fn remap_masters(g: &mut Glyph, font: &Font) {
    let src: Vec<String> = g
        .layers
        .iter()
        .filter(|l| l.is_master())
        .map(|l| l.master_id.clone())
        .collect();
    let mut kept = Vec::new();
    for mut l in g.layers.drain(..) {
        if let Some(i) = src.iter().position(|m| *m == l.master_id) {
            if i < font.masters.len() {
                l.master_id = font.masters[i].id.clone();
                kept.push(l);
            }
        }
    }
    g.layers = kept;
}

/// Glyph › Duplicate Glyph: a copy of each glyph right after it, named `a.001`
/// (then `.002`, …), without its Unicode values so the original keeps them.
/// Returns the font and the new names.
pub fn duplicate(font: &Font, names: &[String]) -> Result<(Font, Vec<String>), String> {
    let mut f = font.clone();
    let mut added = Vec::new();
    for name in names {
        let i = f
            .glyphs
            .iter()
            .position(|g| &g.name == name)
            .ok_or_else(|| format!("There is no glyph named \"{name}\"."))?;
        let mut copy = f.glyphs[i].clone();
        copy.name = unique_name(&f, name);
        copy.unicodes.clear();
        copy.production_name = None;
        copy.locked = false;
        added.push(copy.name.clone());
        f.glyphs.insert(i + 1, copy);
    }
    Ok((f, added))
}

/// A free name for a copied glyph: `base`, then `base.001`, `base.002`, …
fn unique_name(font: &Font, base: &str) -> String {
    if font.glyph(base).is_none() {
        return base.to_string();
    }
    for i in 1..1000 {
        let n = format!("{base}.{i:03}");
        if font.glyph(&n).is_none() {
            return n;
        }
    }
    format!("{base}.999")
}

/// Paste into glyphs of the same name; copied glyphs the font lacks are ignored.
pub fn paste_same_named(font: &Font, glyphs: &[Glyph]) -> R<Font> {
    let mut f = font.clone();
    for clip in glyphs {
        let Some(t) = f.glyph_mut(&clip.name) else {
            continue;
        };
        if t.locked {
            return Err(LOCKED.to_string());
        }
        *t = replace_layers(font, t, clip);
    }
    Ok(f)
}

/// Paste onto the selected glyphs in order, keeping their names and code points.
/// Extra copied glyphs or extra selected glyphs are left alone.
pub fn paste_into_selection(font: &Font, targets: &[String], glyphs: &[Glyph]) -> R<Font> {
    let mut f = font.clone();
    for (name, clip) in targets.iter().zip(glyphs) {
        let t = f
            .glyph_mut(name)
            .ok_or_else(|| format!("There is no glyph named \"{name}\"."))?;
        if t.locked {
            return Err(LOCKED.to_string());
        }
        *t = replace_layers(font, t, clip);
    }
    Ok(f)
}

/// Add the copied glyphs as components of every target glyph, in every layer. A copied
/// glyph the font already has is referenced as it is; a missing one is added first.
pub fn paste_as_components(font: &Font, targets: &[String], glyphs: &[Glyph]) -> R<Font> {
    let mut f = font.clone();
    let mut bases: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    for clip in glyphs {
        // A name this paste already added clashes too, so two copies of one missing
        // glyph become `name` and `name.001` rather than one shared base.
        let name = if f.glyph(&clip.name).is_some() && !added.contains(&clip.name) {
            clip.name.clone()
        } else {
            let n = unique_name(&f, &clip.name);
            let mut g = clip.clone();
            g.name = n.clone();
            g.unicodes.clear();
            remap_masters(&mut g, &f);
            f.glyphs.push(g);
            added.push(n.clone());
            n
        };
        bases.push(name);
    }
    for name in targets {
        let t = f
            .glyph_mut(name)
            .ok_or_else(|| format!("There is no glyph named \"{name}\"."))?;
        if t.locked {
            return Err(LOCKED.to_string());
        }
        for l in &mut t.layers {
            for b in &bases {
                l.shapes.push(Shape::component(b, IDENTITY));
            }
        }
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    fn clip_of(f: &Font, name: &str) -> Glyph {
        f.glyph(name).unwrap().clone()
    }

    #[test]
    fn same_named_paste_replaces_outlines_and_keeps_guides() {
        let mut f = sample_font();
        let m = f.masters[0].id.clone();
        let mut clip = clip_of(&f, "H");
        clip.layers[0].width = 999.0;
        // A guide on the target must survive the paste.
        f.glyph_mut("H").unwrap().layers[0]
            .guides
            .push(crate::model::Guide {
                x: 0.0,
                y: 100.0,
                angle: 0.0,
                name: None,
                locked: false,
            });
        let out = paste_same_named(&f, &[clip]).unwrap();
        let l = out.glyph("H").unwrap().layer(&m).unwrap();
        assert_eq!(l.width, 999.0);
        assert_eq!(l.guides.len(), 1);
        // A copied glyph the font lacks changes nothing.
        let mut stranger = clip_of(&f, "o");
        stranger.name = "nothere".into();
        let out2 = paste_same_named(&f, &[stranger]).unwrap();
        assert_eq!(out2, f);
    }

    #[test]
    fn selection_paste_keeps_target_names_and_pairs_in_order() {
        let f = sample_font();
        let m = f.masters[0].id.clone();
        let clips = [clip_of(&f, "H"), clip_of(&f, "o")];
        let out = paste_into_selection(&f, &["l".into(), "n".into()], &clips).unwrap();
        assert_eq!(
            out.glyph("l").unwrap().layer(&m).unwrap().width,
            f.glyph("H").unwrap().layer(&m).unwrap().width
        );
        assert_eq!(
            out.glyph("n").unwrap().layer(&m).unwrap().width,
            f.glyph("o").unwrap().layer(&m).unwrap().width
        );
        assert_eq!(
            out.glyph("l").unwrap().unicodes,
            f.glyph("l").unwrap().unicodes
        );
        // More targets than copies: the rest stay as they were.
        let out2 = paste_into_selection(&f, &["l".into(), "n".into()], &clips[..1]).unwrap();
        assert_eq!(out2.glyph("n").unwrap(), f.glyph("n").unwrap());
    }

    #[test]
    fn components_paste_adds_missing_bases_with_suffix_and_references_existing() {
        let f = sample_font();
        let m = f.masters[0].id.clone();
        let mut stranger = clip_of(&f, "o");
        stranger.name = "brandnew".into();
        let clips = [clip_of(&f, "H"), stranger];
        let out = paste_as_components(&f, &["A".into()], &clips).unwrap();
        let a = out.glyph("A").unwrap().layer(&m).unwrap();
        let bases: Vec<&str> = a
            .shapes
            .iter()
            .filter_map(|s| match s {
                Shape::Component { base, .. } => Some(base.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            bases.contains(&"H") && bases.contains(&"brandnew"),
            "{bases:?}"
        );
        assert!(out.glyph("brandnew").is_some());
        // Two copied glyphs with the same missing name: the second gets .001.
        let mut dup = clip_of(&f, "o");
        dup.name = "brandnew".into();
        let out2 = paste_as_components(&f, &["A".into()], &[dup.clone(), dup]).unwrap();
        assert!(out2.glyph("brandnew").is_some() && out2.glyph("brandnew.001").is_some());
        out2.validate().unwrap();
    }

    #[test]
    fn locked_targets_refuse_every_mode() {
        let mut f = sample_font();
        f.glyph_mut("H").unwrap().locked = true;
        let clip = clip_of(&f, "o");
        assert!(paste_same_named(
            &f,
            &[{
                let mut c = clip.clone();
                c.name = "H".into();
                c
            }]
        )
        .is_err());
        assert!(paste_into_selection(&f, &["H".into()], std::slice::from_ref(&clip)).is_err());
        assert!(paste_as_components(&f, &["H".into()], &[clip]).is_err());
    }
}
