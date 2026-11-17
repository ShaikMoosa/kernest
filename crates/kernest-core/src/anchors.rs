//! Anchors: editing them, sensible defaults, and placing components by them.
//!
//! Like the other core edits, every function returns an edited copy of the font; the
//! caller stores it through history so the change can be undone. Alignment is kept
//! separate from the edits so a command can do both in one undo step.

use std::collections::BTreeMap;

use crate::geometry::bounds;
use crate::glyphdata::{self, Category};
use crate::interpolate::check_glyph;
use crate::model::{Anchor, Font, Glyph, Layer, Shape, IDENTITY};

type R<T> = Result<T, String>;

fn no_glyph(glyph: &str) -> String {
    format!("There is no glyph named \"{glyph}\".")
}

fn no_layer(glyph: &str, key: &str) -> String {
    format!("\"{glyph}\" has no layer \"{key}\".")
}

fn check_name(name: &str) -> R<()> {
    if name.is_empty() {
        return Err("An anchor name can't be empty.".into());
    }
    if name.chars().any(char::is_whitespace) {
        return Err("Anchor names can't contain spaces.".into());
    }
    Ok(())
}

/// The layers an "all masters" edit touches: every master and brace layer, but only
/// when a master layer is edited and the glyph is compatible across masters.
fn target_keys(font: &Font, glyph: &str, key: &str, all_masters: bool) -> R<Vec<String>> {
    let g = font.glyph(glyph).ok_or_else(|| no_glyph(glyph))?;
    let layer = g.layer(key).ok_or_else(|| no_layer(glyph, key))?;
    let spread = all_masters
        && layer.is_master()
        && font.masters.len() > 1
        && check_glyph(font, glyph).is_empty();
    Ok(if spread {
        g.layers
            .iter()
            .filter(|l| l.is_master() || l.location.is_some())
            .map(|l| l.key().to_string())
            .collect()
    } else {
        vec![key.to_string()]
    })
}

fn edit_layers(font: &Font, glyph: &str, keys: &[String], f: impl Fn(&mut Layer)) -> R<Font> {
    let mut out = font.clone();
    let g = out.glyph_mut(glyph).ok_or_else(|| no_glyph(glyph))?;
    for key in keys {
        f(g.layer_mut(key).ok_or_else(|| no_layer(glyph, key))?);
    }
    Ok(out)
}

/// Replaces a layer's anchors. With `all_masters`, every master gets the same anchors.
pub fn set_anchors(
    font: &Font,
    glyph: &str,
    key: &str,
    anchors: &[Anchor],
    all_masters: bool,
) -> R<Font> {
    for a in anchors {
        check_name(&a.name)?;
        if anchors.iter().filter(|o| o.name == a.name).count() > 1 {
            return Err(format!("\"{}\" is used for two anchors.", a.name));
        }
    }
    let keys = target_keys(font, glyph, key, all_masters)?;
    edit_layers(font, glyph, &keys, |l| l.anchors = anchors.to_vec())
}

/// Adds an anchor, or moves it when the name is already taken.
pub fn add_anchor(
    font: &Font,
    glyph: &str,
    key: &str,
    anchor: &Anchor,
    all_masters: bool,
) -> R<Font> {
    check_name(&anchor.name)?;
    let keys = target_keys(font, glyph, key, all_masters)?;
    edit_layers(font, glyph, &keys, |l| {
        match l.anchors.iter_mut().find(|a| a.name == anchor.name) {
            Some(a) => {
                a.x = anchor.x;
                a.y = anchor.y;
            }
            None => l.anchors.push(anchor.clone()),
        }
    })
}

/// Moves anchors by `(dx, dy)` in font units. Names a layer doesn't have are skipped,
/// so one drag can cover masters whose anchors differ.
pub fn move_anchors(
    font: &Font,
    glyph: &str,
    key: &str,
    moves: &[(String, f64, f64)],
    all_masters: bool,
) -> R<Font> {
    let keys = target_keys(font, glyph, key, all_masters)?;
    edit_layers(font, glyph, &keys, |l| {
        for (name, dx, dy) in moves {
            if let Some(a) = l.anchors.iter_mut().find(|a| &a.name == name) {
                a.x += dx;
                a.y += dy;
            }
        }
    })
}

/// Renames an anchor in every targeted layer.
pub fn rename_anchor(
    font: &Font,
    glyph: &str,
    key: &str,
    old: &str,
    new: &str,
    all_masters: bool,
) -> R<Font> {
    check_name(new)?;
    let g = font.glyph(glyph).ok_or_else(|| no_glyph(glyph))?;
    let layer = g.layer(key).ok_or_else(|| no_layer(glyph, key))?;
    if !layer.anchors.iter().any(|a| a.name == old) {
        return Err(format!("\"{glyph}\" has no anchor named \"{old}\"."));
    }
    if old != new && layer.anchors.iter().any(|a| a.name == new) {
        return Err(format!(
            "\"{glyph}\" already has an anchor named \"{new}\"."
        ));
    }
    let keys = target_keys(font, glyph, key, all_masters)?;
    edit_layers(font, glyph, &keys, |l| {
        for a in &mut l.anchors {
            if a.name == old {
                a.name = new.to_string();
            }
        }
    })
}

/// Removes anchors by name.
pub fn remove_anchors(
    font: &Font,
    glyph: &str,
    key: &str,
    names: &[String],
    all_masters: bool,
) -> R<Font> {
    let keys = target_keys(font, glyph, key, all_masters)?;
    edit_layers(font, glyph, &keys, |l| {
        l.anchors.retain(|a| !names.contains(&a.name));
    })
}

// ---------- Alignment ----------

fn apply(t: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
    (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
}

/// The base glyph's layer that a component draws from: the same layer key when the
/// base has one (a matching brace layer), otherwise its master layer.
fn base_layer_for(
    font: &Font,
    base: &str,
    owner: &Layer,
    smart: &BTreeMap<String, f64>,
) -> Option<Layer> {
    let g = font.glyph(base)?;
    if g.is_smart() {
        return crate::smart::instance_for(font, base, owner, smart);
    }
    g.layer(owner.key())
        .or_else(|| g.layer(&owner.master_id))
        .cloned()
}

/// The anchor a mark attaches to: the exact name first — the most recently added
/// one, so a mark stacks on the mark before it rather than the base — then the
/// nearest ligature slot (`top_1`, `top_2`, …) by horizontal distance.
fn find_target<'a>(
    available: &'a [(String, f64, f64)],
    name: &str,
    x: f64,
) -> Option<&'a (String, f64, f64)> {
    if let Some(t) = available.iter().rev().find(|(n, _, _)| n == name) {
        return Some(t);
    }
    let prefix = format!("{name}_");
    available
        .iter()
        .filter(|(n, _, _)| {
            n.strip_prefix(&prefix).is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
            })
        })
        .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
}

/// Aligns one layer in place: each component after the first that is set to align
/// attaches its `_name` anchor to the matching anchor of the shapes before it.
/// A mark's own anchors join the targets, so marks can stack on marks.
fn align_layer_in_place(font: &mut Font, glyph: &str, key: &str) -> R<()> {
    let g = font.glyph(glyph).ok_or_else(|| no_glyph(glyph))?;
    let layer = g.layer(key).ok_or_else(|| no_layer(glyph, key))?;
    let bases: Vec<Option<Layer>> = layer
        .shapes
        .iter()
        .map(|s| match s {
            Shape::Component { base, smart, .. } => base_layer_for(font, base, layer, smart),
            Shape::Path(_) => None,
        })
        .collect();
    let own = layer.anchors.clone();
    let l = font.glyph_mut(glyph).unwrap().layer_mut(key).unwrap();
    let mut available: Vec<(String, f64, f64)> = Vec::new();
    let mut own_added = false;
    for (i, shape) in l.shapes.iter_mut().enumerate() {
        match shape {
            Shape::Path(_) => {
                if !own_added {
                    available.extend(own.iter().map(|a| (a.name.clone(), a.x, a.y)));
                    own_added = true;
                }
            }
            Shape::Component {
                transform, align, ..
            } => {
                let Some(bl) = &bases[i] else { continue };
                if *align && i > 0 {
                    for a in bl
                        .anchors
                        .iter()
                        .filter(|a| a.name.len() > 1 && a.name.starts_with('_'))
                    {
                        let (ax, ay) = apply(transform, a.x, a.y);
                        if let Some((_, tx, ty)) = find_target(&available, &a.name[1..], ax) {
                            transform[4] += tx - ax;
                            transform[5] += ty - ay;
                            break;
                        }
                    }
                }
                for a in bl.anchors.iter().filter(|a| !a.name.starts_with('_')) {
                    let (x, y) = apply(transform, a.x, a.y);
                    available.push((a.name.clone(), x, y));
                }
            }
        }
    }
    Ok(())
}

/// Aligns one layer's components by its anchors.
pub fn align_layer(font: &Font, glyph: &str, key: &str) -> R<Font> {
    let mut out = font.clone();
    align_layer_in_place(&mut out, glyph, key)?;
    Ok(out)
}

fn align_glyph_in_place(font: &mut Font, glyph: &str) -> R<()> {
    let keys: Vec<String> = font
        .glyph(glyph)
        .ok_or_else(|| no_glyph(glyph))?
        .layers
        .iter()
        .map(|l| l.key().to_string())
        .collect();
    for key in keys {
        align_layer_in_place(font, glyph, &key)?;
    }
    Ok(())
}

/// Aligns every layer of a glyph.
pub fn align_glyph(font: &Font, glyph: &str) -> R<Font> {
    let mut out = font.clone();
    align_glyph_in_place(&mut out, glyph)?;
    Ok(out)
}

/// Aligns a glyph and every glyph that uses it as a component, however deeply nested.
/// Call this after editing a glyph's outlines or anchors.
pub fn align_dependents(font: &Font, changed: &str) -> R<Font> {
    if font.glyph(changed).is_none() {
        return Err(no_glyph(changed));
    }
    let mut names = vec![changed.to_string()];
    let mut i = 0;
    while i < names.len() {
        let current = names[i].clone();
        for g in &font.glyphs {
            // Locked glyphs keep their outlines even when a base they use moves.
            if names.contains(&g.name) || g.locked {
                continue;
            }
            let uses = g.layers.iter().any(|l| {
                l.shapes
                    .iter()
                    .any(|s| matches!(s, Shape::Component { base, .. } if *base == current))
            });
            if uses {
                names.push(g.name.clone());
            }
        }
        i += 1;
    }
    let mut out = font.clone();
    for name in &names {
        align_glyph_in_place(&mut out, name)?;
    }
    Ok(out)
}

/// Aligns every glyph; used before export so saved positions are always current.
pub fn align_all(font: &Font) -> Font {
    let mut out = font.clone();
    let names: Vec<String> = out.glyphs.iter().map(|g| g.name.clone()).collect();
    for name in names {
        let _ = align_glyph_in_place(&mut out, &name);
    }
    out
}

// ---------- Defaults ----------

/// Sensible anchors for a glyph in one layer, from its metrics and outline: letters
/// get top and bottom (and ogonek where it belongs), marks get their attaching
/// `_top`/`_bottom` plus an anchor for stacking, Arabic letters also entry and exit.
/// Glyphs that take no anchors (digits, punctuation, spaces) get an empty list.
pub fn suggested_anchors(font: &Font, glyph: &Glyph, layer: &Layer) -> Vec<Anchor> {
    let metrics = font
        .masters
        .iter()
        .find(|m| m.id == layer.master_id)
        .map(|m| m.metrics)
        .unwrap_or_default();
    let b = bounds(font, layer);
    let cx = b
        .map(|b| (b.min_x + b.max_x) / 2.0)
        .unwrap_or(layer.width / 2.0);
    let cp = glyph.unicodes.first().map(|c| c.0);
    let mut out: Vec<Anchor> = Vec::new();
    let mut push = |name: &str, x: f64, y: f64| {
        out.push(Anchor {
            name: name.to_string(),
            x,
            y,
        });
    };
    match glyph.category() {
        Category::Mark => {
            let (min_y, max_y) = b.map(|b| (b.min_y, b.max_y)).unwrap_or((0.0, 0.0));
            if glyphdata::is_below_mark(&glyph.name, cp) {
                push("_bottom", cx, max_y);
                push("bottom", cx, min_y);
            } else {
                push("_top", cx, min_y);
                push("top", cx, max_y);
            }
        }
        cat @ (Category::Uppercase | Category::Lowercase | Category::Letter) => {
            let arabic = cp.is_some_and(|c| (0x0620..=0x064A).contains(&c));
            if cp != Some(0x0640) {
                // Tatweel is a joining stroke, not a letter that takes marks.
                let top_y = if cat == Category::Uppercase {
                    metrics.cap_height
                } else if arabic {
                    b.map(|b| b.max_y).unwrap_or(metrics.x_height)
                } else {
                    metrics.x_height
                };
                push("top", layer.width / 2.0, top_y);
                push("bottom", layer.width / 2.0, 0.0);
            }
            if glyphdata::takes_ogonek(&glyph.name) {
                push(
                    "ogonek",
                    b.map(|b| b.max_x).unwrap_or(layer.width / 2.0),
                    0.0,
                );
            }
            if let Some(c) = cp {
                // Right-to-left: a letter joins the previous one at its right edge.
                if glyphdata::arabic_joins_right(c) {
                    push("entry", b.map(|b| b.max_x).unwrap_or(layer.width), 0.0);
                }
                if glyphdata::arabic_joins_left(c) {
                    push("exit", b.map(|b| b.min_x).unwrap_or(0.0), 0.0);
                }
            }
            let base = glyph.name.split('.').next().unwrap_or(&glyph.name);
            let parts = base.split('_').count();
            if parts > 1 && !base.starts_with('_') {
                for i in 1..parts {
                    push(
                        &format!("caret_{i}"),
                        layer.width * i as f64 / parts as f64,
                        0.0,
                    );
                }
            }
        }
        _ => {}
    }
    out
}

/// Sets the default anchors on every layer of the named glyphs. Glyphs that take no
/// anchors keep whatever they have.
pub fn set_default_anchors(font: &Font, names: &[String]) -> R<Font> {
    let mut out = font.clone();
    for name in names {
        if out.glyph(name).ok_or_else(|| no_glyph(name))?.locked {
            continue;
        }
        let keys: Vec<String> = out
            .glyph(name)
            .ok_or_else(|| no_glyph(name))?
            .layers
            .iter()
            .map(|l| l.key().to_string())
            .collect();
        for key in keys {
            let g = out.glyph(name).unwrap();
            let anchors = suggested_anchors(&out, g, g.layer(&key).unwrap());
            if anchors.is_empty() {
                continue;
            }
            out.glyph_mut(name)
                .unwrap()
                .layer_mut(&key)
                .unwrap()
                .anchors = anchors;
        }
    }
    Ok(out)
}

// ---------- Composites ----------

/// The parts of a composite glyph when Unicode gives it a canonical decomposition:
/// `Aacute` gives `["A", "acutecomb"]`.
pub fn recipe(name: &str) -> Option<Vec<String>> {
    crate::decompose::parts(name).map(|p| p.iter().map(|s| (*s).to_string()).collect())
}

/// The result of building composites.
#[derive(Debug, Clone)]
pub struct Composites {
    pub font: Font,
    /// Names that were built, in order.
    pub built: Vec<String>,
    /// Names without a recipe, without their parts, or that already have outlines.
    pub skipped: Vec<String>,
}

/// Builds glyphs from their recipes: one component per part, the base's width and
/// anchors, aligned in every master. Missing glyphs are added; glyphs that already
/// have outlines are left alone.
pub fn create_composites(font: &Font, names: &[String]) -> R<Composites> {
    let mut out = font.clone();
    let (mut built, mut skipped) = (Vec::new(), Vec::new());
    for name in names {
        let Some(parts) = recipe(name) else {
            skipped.push(name.clone());
            continue;
        };
        if parts.iter().any(|p| out.glyph(p).is_none()) {
            skipped.push(name.clone());
            continue;
        }
        match out.glyph(name) {
            Some(g) if g.locked || g.layers.iter().any(|l| !l.shapes.is_empty()) => {
                skipped.push(name.clone());
                continue;
            }
            Some(_) => {}
            None => {
                let ng = crate::glyphset::NewGlyph {
                    name: name.clone(),
                    codepoint: glyphdata::codepoint_for_name(name),
                };
                let r = crate::glyphset::add_glyphs(&out, &[ng])?;
                if r.added.is_empty() {
                    skipped.push(name.clone());
                    continue;
                }
                out = r.font;
            }
        }
        let masters: Vec<String> = out.masters.iter().map(|m| m.id.clone()).collect();
        for m in &masters {
            let (width, anchors) = out
                .glyph(&parts[0])
                .and_then(|b| b.layer(m))
                .map(|l| (l.width, l.anchors.clone()))
                .unwrap_or((f64::from(out.info.units_per_em) * 0.6, Vec::new()));
            if let Some(l) = out.glyph_mut(name).unwrap().layer_mut(m) {
                l.width = width;
                l.shapes = parts
                    .iter()
                    .map(|p| Shape::component(p.clone(), IDENTITY))
                    .collect();
                l.anchors = anchors;
            }
        }
        align_glyph_in_place(&mut out, name)?;
        built.push(name.clone());
    }
    Ok(Composites {
        font: out,
        built,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyphset::{add_glyphs, parse_glyph_list, NewGlyph};
    use crate::model::{Axis, Master, Metrics, Node, NodeKind, Path};
    use crate::sample::sample_font;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
        Shape::Path(Path {
            locked: false,
            closed: true,
            nodes: vec![
                Node::new(x0, y0, NodeKind::Line),
                Node::new(x1, y0, NodeKind::Line),
                Node::new(x1, y1, NodeKind::Line),
                Node::new(x0, y1, NodeKind::Line),
            ],
        })
    }

    fn anchor(layer: &Layer, name: &str) -> (f64, f64) {
        let a = layer.anchors.iter().find(|a| a.name == name).unwrap();
        (a.x, a.y)
    }

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
            guides: Vec::new(),
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

    /// A font with a drawn A (top anchor per master) and a drawn acutecomb.
    fn with_a_and_acute() -> Font {
        let mut f = two_masters();
        for (m, (tx, ty)) in [("m01", (300.0, 700.0)), ("m02", (350.0, 750.0))] {
            let l = f.glyph_mut("A").unwrap().layer_mut(m).unwrap();
            l.shapes = vec![rect(0.0, 0.0, 600.0, 700.0)];
            l.anchors = vec![Anchor {
                name: "top".into(),
                x: tx,
                y: ty,
            }];
        }
        let mut f = add_glyphs(&f, &parse_glyph_list("acutecomb").unwrap())
            .unwrap()
            .font;
        for m in ["m01", "m02"] {
            let l = f.glyph_mut("acutecomb").unwrap().layer_mut(m).unwrap();
            l.width = 0.0;
            l.shapes = vec![rect(-80.0, 720.0, 80.0, 800.0)];
            l.anchors = vec![Anchor {
                name: "_top".into(),
                x: 0.0,
                y: 720.0,
            }];
        }
        f
    }

    #[test]
    fn anchor_edits_apply_to_all_masters() {
        let f = two_masters();
        let a = Anchor {
            name: "top".into(),
            x: 300.0,
            y: 700.0,
        };
        let f = add_anchor(&f, "H", "m01", &a, true).unwrap();
        assert_eq!(
            f.glyph("H").unwrap().layer("m02").unwrap().anchors,
            vec![a.clone()]
        );
        let f = move_anchors(&f, "H", "m01", &[("top".into(), 10.0, -20.0)], true).unwrap();
        for m in ["m01", "m02"] {
            assert_eq!(
                anchor(f.glyph("H").unwrap().layer(m).unwrap(), "top"),
                (310.0, 680.0)
            );
        }
        let f = rename_anchor(&f, "H", "m01", "top", "top_1", true).unwrap();
        assert_eq!(
            f.glyph("H").unwrap().layer("m02").unwrap().anchors[0].name,
            "top_1"
        );
        assert!(rename_anchor(&f, "H", "m01", "nope", "x", true).is_err());
        let f = remove_anchors(&f, "H", "m01", &["top_1".into()], true).unwrap();
        assert!(f
            .glyph("H")
            .unwrap()
            .layer("m02")
            .unwrap()
            .anchors
            .is_empty());
        // set_anchors replaces the whole list and rejects duplicate names.
        let f = set_anchors(&f, "H", "m01", std::slice::from_ref(&a), true).unwrap();
        assert_eq!(
            f.glyph("H").unwrap().layer("m02").unwrap().anchors,
            vec![a.clone()]
        );
        assert!(set_anchors(&f, "H", "m01", &[a.clone(), a.clone()], false).is_err());
        // Without the flag only the edited layer changes.
        let f = remove_anchors(&f, "H", "m01", &["top".into()], true).unwrap();
        let f = add_anchor(&f, "H", "m01", &a, false).unwrap();
        assert!(f
            .glyph("H")
            .unwrap()
            .layer("m02")
            .unwrap()
            .anchors
            .is_empty());
        assert!(add_anchor(&f, "nope", "m01", &a, false).is_err());
        assert!(add_anchor(
            &f,
            "H",
            "m01",
            &Anchor {
                name: "".into(),
                x: 0.0,
                y: 0.0
            },
            false
        )
        .is_err());
    }

    #[test]
    fn default_anchors_follow_metrics_and_category() {
        let f = sample_font();
        let f = set_default_anchors(&f, &["A".into(), "n".into(), "period".into()]).unwrap();
        let a = f.glyph("A").unwrap().layer("m01").unwrap();
        assert_eq!(anchor(a, "top"), (300.0, 700.0));
        assert_eq!(anchor(a, "bottom"), (300.0, 0.0));
        assert!(a.anchors.iter().any(|x| x.name == "ogonek"));
        let n = f.glyph("n").unwrap().layer("m01").unwrap();
        assert_eq!(anchor(n, "top"), (300.0, 500.0));
        assert!(!n.anchors.iter().any(|x| x.name == "ogonek"));
        assert!(f
            .glyph("period")
            .unwrap()
            .layer("m01")
            .unwrap()
            .anchors
            .is_empty());
    }

    #[test]
    fn mark_defaults_attach_and_stack() {
        let f = sample_font();
        let mut f = add_glyphs(&f, &parse_glyph_list("acutecomb dotbelowcomb").unwrap())
            .unwrap()
            .font;
        let l = f.glyph_mut("acutecomb").unwrap().layer_mut("m01").unwrap();
        l.width = 0.0;
        l.shapes = vec![rect(-80.0, 720.0, 80.0, 800.0)];
        let l = f
            .glyph_mut("dotbelowcomb")
            .unwrap()
            .layer_mut("m01")
            .unwrap();
        l.width = 0.0;
        l.shapes = vec![rect(-40.0, -150.0, 40.0, -70.0)];
        let f = set_default_anchors(&f, &["acutecomb".into(), "dotbelowcomb".into()]).unwrap();
        let a = f.glyph("acutecomb").unwrap().layer("m01").unwrap();
        assert_eq!(anchor(a, "_top"), (0.0, 720.0));
        assert_eq!(anchor(a, "top"), (0.0, 800.0));
        let d = f.glyph("dotbelowcomb").unwrap().layer("m01").unwrap();
        assert_eq!(anchor(d, "_bottom"), (0.0, -70.0));
        assert_eq!(anchor(d, "bottom"), (0.0, -150.0));
    }

    #[test]
    fn arabic_defaults_get_entry_and_exit() {
        let f = sample_font();
        let mut f = add_glyphs(
            &f,
            &[
                NewGlyph {
                    name: "uni0628".into(),
                    codepoint: Some(0x628),
                },
                NewGlyph {
                    name: "uni0627".into(),
                    codepoint: Some(0x627),
                },
            ],
        )
        .unwrap()
        .font;
        for (n, x0, x1) in [("uni0628", 50.0, 550.0), ("uni0627", 200.0, 300.0)] {
            f.glyph_mut(n).unwrap().layer_mut("m01").unwrap().shapes =
                vec![rect(x0, 0.0, x1, 500.0)];
        }
        let f = set_default_anchors(&f, &["uni0628".into(), "uni0627".into()]).unwrap();
        let beh = f.glyph("uni0628").unwrap().layer("m01").unwrap();
        assert_eq!(anchor(beh, "entry"), (550.0, 0.0));
        assert_eq!(anchor(beh, "exit"), (50.0, 0.0));
        assert_eq!(anchor(beh, "top"), (300.0, 500.0));
        let alef = f.glyph("uni0627").unwrap().layer("m01").unwrap();
        assert!(alef.anchors.iter().any(|a| a.name == "entry"));
        assert!(!alef.anchors.iter().any(|a| a.name == "exit"));
    }

    #[test]
    fn composites_align_in_every_master() {
        let f = with_a_and_acute();
        let r = create_composites(&f, &["Aacute".into()]).unwrap();
        assert_eq!(r.built, ["Aacute"]);
        let f = r.font;
        assert_eq!(f.glyph("Aacute").unwrap().unicodes[0].0, 0xC1);
        for (m, expect) in [("m01", (300.0, -20.0)), ("m02", (350.0, 30.0))] {
            let l = f.glyph("Aacute").unwrap().layer(m).unwrap();
            assert_eq!(l.width, 600.0);
            let Shape::Component {
                transform, align, ..
            } = &l.shapes[1]
            else {
                panic!("expected a component")
            };
            assert!(*align);
            assert_eq!((transform[4], transform[5]), expect);
            // The base's anchors were copied for further marks.
            assert!(l.anchors.iter().any(|a| a.name == "top"));
        }
        f.validate().unwrap();
    }

    #[test]
    fn marks_stack_on_marks() {
        let f = sample_font();
        let mut f = add_glyphs(&f, &parse_glyph_list("a circumflexcomb acutecomb").unwrap())
            .unwrap()
            .font;
        let l = f.glyph_mut("a").unwrap().layer_mut("m01").unwrap();
        l.width = 580.0;
        l.shapes = vec![rect(0.0, 0.0, 580.0, 500.0)];
        l.anchors = vec![Anchor {
            name: "top".into(),
            x: 290.0,
            y: 500.0,
        }];
        let l = f
            .glyph_mut("circumflexcomb")
            .unwrap()
            .layer_mut("m01")
            .unwrap();
        l.width = 0.0;
        l.shapes = vec![rect(-100.0, 500.0, 100.0, 650.0)];
        l.anchors = vec![
            Anchor {
                name: "_top".into(),
                x: 0.0,
                y: 500.0,
            },
            Anchor {
                name: "top".into(),
                x: 0.0,
                y: 650.0,
            },
        ];
        let l = f.glyph_mut("acutecomb").unwrap().layer_mut("m01").unwrap();
        l.width = 0.0;
        l.shapes = vec![rect(-80.0, 700.0, 80.0, 780.0)];
        l.anchors = vec![Anchor {
            name: "_top".into(),
            x: 0.0,
            y: 700.0,
        }];
        let r = create_composites(&f, &["acircumflexacute".into()]).unwrap();
        assert_eq!(r.built, ["acircumflexacute"]);
        let l = r
            .font
            .glyph("acircumflexacute")
            .unwrap()
            .layer("m01")
            .unwrap();
        let Shape::Component {
            transform: circ, ..
        } = &l.shapes[1]
        else {
            panic!()
        };
        assert_eq!((circ[4], circ[5]), (290.0, 0.0));
        let Shape::Component {
            transform: acute, ..
        } = &l.shapes[2]
        else {
            panic!()
        };
        // The acute sits on the circumflex's own top anchor, not the a's.
        assert_eq!((acute[4], acute[5]), (290.0, -50.0));
        r.font.validate().unwrap();
    }

    #[test]
    fn hand_placed_components_stay_put() {
        let f = with_a_and_acute();
        let mut f = create_composites(&f, &["Aacute".into()]).unwrap().font;
        let l = f.glyph_mut("Aacute").unwrap().layer_mut("m01").unwrap();
        let Shape::Component {
            transform, align, ..
        } = &mut l.shapes[1]
        else {
            panic!()
        };
        *align = false;
        *transform = [1.0, 0.0, 0.0, 1.0, 100.0, 20.0];
        let f = align_glyph(&f, "Aacute").unwrap();
        let l = f.glyph("Aacute").unwrap().layer("m01").unwrap();
        let Shape::Component {
            transform, align, ..
        } = &l.shapes[1]
        else {
            panic!()
        };
        assert!(!*align);
        assert_eq!((transform[4], transform[5]), (100.0, 20.0));
    }

    #[test]
    fn ligature_anchors_pick_the_nearest_slot() {
        let f = sample_font();
        let mut f = add_glyphs(&f, &parse_glyph_list("f_i acutecomb").unwrap())
            .unwrap()
            .font;
        let l = f.glyph_mut("f_i").unwrap().layer_mut("m01").unwrap();
        l.width = 600.0;
        l.shapes = vec![
            rect(50.0, 0.0, 150.0, 700.0),
            rect(400.0, 0.0, 500.0, 700.0),
        ];
        l.anchors = vec![
            Anchor {
                name: "top_1".into(),
                x: 100.0,
                y: 700.0,
            },
            Anchor {
                name: "top_2".into(),
                x: 450.0,
                y: 700.0,
            },
        ];
        let l = f.glyph_mut("acutecomb").unwrap().layer_mut("m01").unwrap();
        l.width = 0.0;
        l.shapes = vec![rect(-80.0, 720.0, 80.0, 800.0)];
        l.anchors = vec![Anchor {
            name: "_top".into(),
            x: 0.0,
            y: 720.0,
        }];
        let l = f.glyph_mut("f_i").unwrap().layer_mut("m01").unwrap();
        l.shapes.push(Shape::component(
            "acutecomb",
            [1.0, 0.0, 0.0, 1.0, 400.0, 0.0],
        ));
        let f = align_glyph(&f, "f_i").unwrap();
        let l = f.glyph("f_i").unwrap().layer("m01").unwrap();
        let Shape::Component { transform, .. } = l.shapes.last().unwrap() else {
            panic!()
        };
        assert_eq!((transform[4], transform[5]), (450.0, -20.0));
    }

    #[test]
    fn composites_need_their_parts_and_keep_existing_work() {
        let f = sample_font();
        let r = create_composites(&f, &["Aacute".into(), "A".into()]).unwrap();
        assert!(r.built.is_empty());
        assert_eq!(r.skipped, ["Aacute", "A"]);
        // A glyph that already has outlines is never rebuilt.
        let mut f = with_a_and_acute();
        f = add_glyphs(&f, &parse_glyph_list("Aacute").unwrap())
            .unwrap()
            .font;
        f.glyph_mut("Aacute")
            .unwrap()
            .layer_mut("m01")
            .unwrap()
            .shapes = vec![rect(0.0, 0.0, 10.0, 10.0)];
        let r = create_composites(&f, &["Aacute".into()]).unwrap();
        assert_eq!(r.skipped, ["Aacute"]);
    }

    #[test]
    fn moving_a_base_anchor_realigns_composites() {
        let f = with_a_and_acute();
        let f = create_composites(&f, &["Aacute".into()]).unwrap().font;
        let f = move_anchors(&f, "A", "m01", &[("top".into(), 50.0, 10.0)], true).unwrap();
        let f = align_dependents(&f, "A").unwrap();
        for (m, expect) in [("m01", (350.0, -10.0)), ("m02", (400.0, 40.0))] {
            let l = f.glyph("Aacute").unwrap().layer(m).unwrap();
            let Shape::Component { transform, .. } = &l.shapes[1] else {
                panic!()
            };
            assert_eq!((transform[4], transform[5]), expect);
        }
    }

    #[test]
    fn recipes_come_from_unicode() {
        assert_eq!(recipe("Aacute"), Some(vec!["A".into(), "acutecomb".into()]));
        assert_eq!(
            recipe("uni00C1"),
            Some(vec!["A".into(), "acutecomb".into()])
        );
        assert_eq!(
            recipe("Acircumflexacute"),
            Some(vec![
                "A".into(),
                "circumflexcomb".into(),
                "acutecomb".into()
            ])
        );
        assert_eq!(recipe("A"), None);
    }
}
