//! Adding and removing glyphs.
//!
//! Like the structural edits, each function returns an edited copy of the font; the
//! caller stores it through `History::apply_font` so the change can be undone.

use std::collections::HashSet;

use crate::glyphdata::{codepoint_for_name, codepoints_for, name_for, LANGUAGES};
use crate::model::{Codepoint, Font, Glyph, Layer, Node, Path, Shape};

type R<T> = Result<T, String>;

/// A glyph to add: its name and, if it has one, its code point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGlyph {
    pub name: String,
    pub codepoint: Option<u32>,
}

/// Whether `name` can be used as a glyph name: ASCII letters, digits, `.`, `_` and `-`,
/// not starting with a digit, `-` or `.` (except `.notdef` and `.null`).
pub fn valid_name(name: &str) -> bool {
    if matches!(name, ".notdef" | ".null") {
        return true;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && name.len() <= 63
}

/// Parses what someone types into Add Glyphs: glyph names (`A.alt`, `uni0416`), single
/// characters (`Ж`, `%`) and code points (`U+0416`), separated by spaces, commas or lines.
pub fn parse_glyph_list(text: &str) -> R<Vec<NewGlyph>> {
    let mut out: Vec<NewGlyph> = Vec::new();
    let mut bad = Vec::new();
    for token in text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
    {
        let mut chars = token.chars();
        let entry = if let (Some(c), None) = (chars.next(), chars.next()) {
            // A single character stands for itself.
            Some(NewGlyph {
                name: name_for(c as u32),
                codepoint: Some(c as u32),
            })
        } else if let Some(hex) = token
            .strip_prefix("U+")
            .or_else(|| token.strip_prefix("u+"))
        {
            u32::from_str_radix(hex, 16)
                .ok()
                .filter(|&cp| char::from_u32(cp).is_some_and(|c| !c.is_control()))
                .map(|cp| NewGlyph {
                    name: name_for(cp),
                    codepoint: Some(cp),
                })
        } else if valid_name(token) {
            Some(NewGlyph {
                name: token.to_string(),
                codepoint: codepoint_for_name(token),
            })
        } else {
            None
        };
        match entry {
            Some(e) if !out.iter().any(|o| o.name == e.name) => out.push(e),
            Some(_) => {}
            None => bad.push(token.to_string()),
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "These aren't glyph names, characters or code points: {}. Names use letters, digits, \".\", \"_\" and \"-\" and start with a letter.",
            bad.join(", ")
        ));
    }
    Ok(out)
}

/// The result of adding glyphs.
#[derive(Debug, Clone)]
pub struct Added {
    pub font: Font,
    /// Names of the glyphs that were added, in order.
    pub added: Vec<String>,
    /// Names that were skipped because the font already has that glyph or code point.
    pub skipped: Vec<String>,
}

/// Appends new, empty glyphs with a layer for every master. A suffixed glyph such as
/// `a.sc` starts with the width of its base glyph `a`; others get 60% of the em
/// (25% for spaces).
pub fn add_glyphs(font: &Font, glyphs: &[NewGlyph]) -> R<Added> {
    let mut f = font.clone();
    let mut names: HashSet<String> = f.glyphs.iter().map(|g| g.name.clone()).collect();
    let mut cps: HashSet<u32> = f
        .glyphs
        .iter()
        .flat_map(|g| g.unicodes.iter().map(|c| c.0))
        .collect();
    let (mut added, mut skipped) = (Vec::new(), Vec::new());
    let upm = f64::from(f.info.units_per_em);
    for ng in glyphs {
        if !valid_name(&ng.name) {
            return Err(format!("\"{}\" can't be used as a glyph name.", ng.name));
        }
        if names.contains(&ng.name) || ng.codepoint.is_some_and(|cp| cps.contains(&cp)) {
            skipped.push(ng.name.clone());
            continue;
        }
        let base = ng
            .name
            .split_once('.')
            .and_then(|(b, _)| font.glyph(b))
            .filter(|_| !ng.name.starts_with('.'));
        let mut g = Glyph::new(
            ng.name.clone(),
            ng.codepoint.map(Codepoint).into_iter().collect(),
        );
        // Parts such as `_stem` or `_corner.serif` only serve other glyphs (Handbook
        // 9.1.14), so they start out not exported.
        g.export = !ng.name.starts_with('_');
        for m in &f.masters {
            let width = base
                .and_then(|b| b.layer(&m.id))
                .map(|l| l.width)
                .unwrap_or(
                    if ng
                        .codepoint
                        .and_then(char::from_u32)
                        .is_some_and(char::is_whitespace)
                    {
                        (upm * 0.25).round()
                    } else {
                        (upm * 0.6).round()
                    },
                );
            g.layers.push(Layer::new(&m.id, width));
        }
        names.insert(ng.name.clone());
        if let Some(cp) = ng.codepoint {
            cps.insert(cp);
        }
        added.push(ng.name.clone());
        f.glyphs.push(g);
    }
    Ok(Added {
        font: f,
        added,
        skipped,
    })
}

/// Removes glyphs. Components that use a removed glyph are decomposed into outlines
/// first, so other glyphs keep their shape; kerning pairs naming a removed glyph are
/// dropped.
pub fn remove_glyphs(font: &Font, names: &[String]) -> R<Font> {
    let gone: HashSet<&str> = names.iter().map(String::as_str).collect();
    if let Some(n) = names.iter().find(|n| font.glyph(n).is_none()) {
        return Err(format!("There is no glyph named \"{n}\"."));
    }
    // Removing a base decomposes it inside the glyphs that use it; a locked glyph
    // must not change, so refuse instead.
    if let Some(g) = font.glyphs.iter().find(|g| {
        g.locked
            && !gone.contains(g.name.as_str())
            && g.layers.iter().any(|l| {
                l.shapes.iter().any(
                    |s| matches!(s, Shape::Component { base, .. } if gone.contains(base.as_str())),
                )
            })
    }) {
        return Err(format!(
            "\"{}\" is locked and uses a glyph being removed. Unlock it first.",
            g.name
        ));
    }
    let mut f = font.clone();
    for g in &mut f.glyphs {
        if gone.contains(g.name.as_str()) {
            continue;
        }
        for l in &mut g.layers {
            if !l
                .shapes
                .iter()
                .any(|s| matches!(s, Shape::Component { base, .. } if gone.contains(base.as_str())))
            {
                continue;
            }
            let mut shapes = Vec::new();
            for s in &l.shapes {
                match s {
                    Shape::Component {
                        base, transform, ..
                    } if gone.contains(base.as_str()) => {
                        flatten(font, &l.master_id, base, *transform, &gone, &mut shapes, 0)
                    }
                    other => shapes.push(other.clone()),
                }
            }
            l.shapes = shapes;
        }
    }
    f.glyphs.retain(|g| !gone.contains(g.name.as_str()));
    for pairs in f.kerning.values_mut() {
        pairs.retain(|p| !gone.contains(p.left.as_str()) && !gone.contains(p.right.as_str()));
    }
    f.kerning.retain(|_, p| !p.is_empty());
    Ok(f)
}

/// Per-language coverage for the Languages sidebar: how many of a language's
/// drawable code points the font encodes, and which ones are missing.
pub struct Coverage {
    pub language: &'static str,
    pub present: usize,
    pub total: usize,
    pub missing: Vec<u32>,
}

fn encoded(font: &Font) -> HashSet<u32> {
    font.glyphs
        .iter()
        .flat_map(|g| g.unicodes.iter().map(|c| c.0))
        .collect()
}

pub fn coverage(font: &Font) -> Vec<Coverage> {
    let present = encoded(font);
    LANGUAGES
        .iter()
        .map(|l| {
            let cps = codepoints_for(l.set_ids);
            let missing: Vec<u32> = cps
                .iter()
                .copied()
                .filter(|cp| !present.contains(cp))
                .collect();
            Coverage {
                language: l.name,
                present: cps.len() - missing.len(),
                total: cps.len(),
                missing,
            }
        })
        .collect()
}

/// The glyphs a language still needs, ready to add; unknown languages error.
pub fn missing_glyphs(font: &Font, language: &str) -> R<Vec<NewGlyph>> {
    let lang = LANGUAGES
        .iter()
        .find(|l| l.name == language)
        .ok_or_else(|| format!("There is no language named \"{language}\"."))?;
    let present = encoded(font);
    Ok(codepoints_for(lang.set_ids)
        .iter()
        .copied()
        .filter(|cp| !present.contains(cp))
        .map(|cp| NewGlyph {
            name: name_for(cp),
            codepoint: Some(cp),
        })
        .collect())
}

/// `outer ∘ inner` for transforms `[xx, xy, yx, yy, dx, dy]`.
fn compose(o: [f64; 6], i: [f64; 6]) -> [f64; 6] {
    [
        o[0] * i[0] + o[2] * i[1],
        o[1] * i[0] + o[3] * i[1],
        o[0] * i[2] + o[2] * i[3],
        o[1] * i[2] + o[3] * i[3],
        o[0] * i[4] + o[2] * i[5] + o[4],
        o[1] * i[4] + o[3] * i[5] + o[5],
    ]
}

/// Copies `base`'s shapes in master `master_id`, transformed, into `out`. Components
/// of glyphs that stay are kept as components; removed ones are flattened too.
fn flatten(
    font: &Font,
    master_id: &str,
    base: &str,
    t: [f64; 6],
    gone: &HashSet<&str>,
    out: &mut Vec<Shape>,
    depth: usize,
) {
    let Some(layer) = font.glyph(base).and_then(|g| g.layer(master_id)) else {
        return;
    };
    if depth > 32 {
        return;
    }
    for s in &layer.shapes {
        match s {
            Shape::Path(p) => out.push(Shape::Path(Path {
                locked: false,
                stroke: None,
                mask: false,
                closed: p.closed,
                nodes: p
                    .nodes
                    .iter()
                    .map(|n| Node {
                        x: t[0] * n.x + t[2] * n.y + t[4],
                        y: t[1] * n.x + t[3] * n.y + t[5],
                        ..*n
                    })
                    .collect(),
            })),
            Shape::Component {
                base: inner,
                transform,
                smart,
                align,
            } => {
                let ct = compose(t, *transform);
                if gone.contains(inner.as_str()) {
                    flatten(font, master_id, inner, ct, gone, out, depth + 1);
                } else {
                    out.push(Shape::Component {
                        base: inner.clone(),
                        transform: ct,
                        smart: smart.clone(),
                        align: *align,
                    });
                }
            }
        }
    }
}

/// Reads Unicode values as typed: hex (`0041`), `U+0041`, `uni0041` or the character
/// itself, separated by commas or spaces. Repeats are dropped; order is kept.
pub fn parse_unicodes(text: &str) -> R<Vec<Codepoint>> {
    let mut out: Vec<Codepoint> = Vec::new();
    for item in text.split(|c: char| c == ',' || c.is_whitespace()) {
        if item.is_empty() {
            continue;
        }
        let mut chars = item.chars();
        let single = match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_ascii_hexdigit() => Some(c as u32),
            _ => None,
        };
        let value = match single {
            Some(v) => v,
            None => {
                let upper = item.to_ascii_uppercase();
                let hex = upper
                    .strip_prefix("U+")
                    .or_else(|| upper.strip_prefix("UNI"))
                    .or_else(|| upper.strip_prefix("0X"))
                    .unwrap_or(&upper);
                if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(format!("\"{item}\" isn't a Unicode value. Type it like 0041 or U+0041, or type the character."));
                }
                u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?
            }
        };
        if value > 0x10FFFF || (0xD800..=0xDFFF).contains(&value) {
            return Err(format!("\"{item}\" isn't a valid Unicode value."));
        }
        if !out.iter().any(|c| c.0 == value) {
            out.push(Codepoint(value));
        }
    }
    Ok(out)
}

/// Gives `glyph` the Unicode values in `text` (empty leaves it unencoded). A value that
/// already belongs to another glyph is refused with that glyph's name, unless
/// `take_from_others` is set: then it is moved over.
pub fn set_unicodes(font: &Font, glyph: &str, text: &str, take_from_others: bool) -> R<Font> {
    let wanted = parse_unicodes(text)?;
    if font.glyph(glyph).is_none() {
        return Err(format!("There is no glyph named \"{glyph}\"."));
    }
    let mut next = font.clone();
    for g in next.glyphs.iter_mut().filter(|g| g.name != glyph) {
        if let Some(c) = g.unicodes.iter().find(|c| wanted.contains(c)) {
            if !take_from_others {
                return Err(format!("U+{c} already belongs to \"{}\".", g.name));
            }
            g.unicodes.retain(|c| !wanted.contains(c));
        }
    }
    next.glyph_mut(glyph).expect("checked above").unicodes = wanted;
    Ok(next)
}

/// Glyph swaps for one export (S39, "Rename Glyphs" and "Remove Glyphs" in
/// Glyphs): each pair trades drawings (layers), keeping names, Unicode values and
/// kerning groups, so `dollar=dollar.alt` makes `dollar` look like `dollar.alt`.
/// Then the glyphs in `remove` are left out, components using them decomposed.
pub fn apply_swaps(font: &Font, rename: &[[String; 2]], remove: &[String]) -> R<Font> {
    let mut f = font.clone();
    for [a, b] in rename {
        if a == b {
            continue;
        }
        let ia = f
            .glyphs
            .iter()
            .position(|g| &g.name == a)
            .ok_or_else(|| format!("Rename Glyphs: there is no glyph named \"{a}\"."))?;
        let ib = f
            .glyphs
            .iter()
            .position(|g| &g.name == b)
            .ok_or_else(|| format!("Rename Glyphs: there is no glyph named \"{b}\"."))?;
        let la = std::mem::take(&mut f.glyphs[ia].layers);
        let lb = std::mem::replace(&mut f.glyphs[ib].layers, la);
        f.glyphs[ia].layers = lb;
    }
    let present: Vec<String> = remove
        .iter()
        .filter(|n| f.glyph(n).is_some())
        .cloned()
        .collect();
    if present.is_empty() {
        return Ok(f);
    }
    remove_glyphs(&f, &present)
}

/// Parses swap lines as typed in Font Info: `a=b` per line (or comma-separated).
pub fn parse_swaps(text: &str) -> R<Vec<[String; 2]>> {
    text.split(['\n', ','])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (a, b) = l
                .split_once('=')
                .ok_or_else(|| format!("\"{l}\": write a swap as glyph=otherglyph."))?;
            let (a, b) = (a.trim(), b.trim());
            if a.is_empty() || b.is_empty() {
                return Err(format!("\"{l}\": write a swap as glyph=otherglyph."));
            }
            Ok([a.to_string(), b.to_string()])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KernPair, NodeKind, IDENTITY};
    use crate::sample::sample_font;

    /// Handbook 9.1.14 (Underscore Components): glyphs whose names start with an
    /// underscore are parts for other glyphs, so they start out not exported.
    #[test]
    fn underscore_glyphs_start_out_not_exported() {
        let new = |name: &str| NewGlyph {
            name: name.into(),
            codepoint: None,
        };
        let r = add_glyphs(
            &sample_font(),
            &[
                new("_part.shoulder"),
                new("_corner.serif"),
                new("f_f"),
                new("a.ss01"),
            ],
        )
        .unwrap();
        let export = |n: &str| r.font.glyph(n).unwrap().export;
        assert!(!export("_part.shoulder") && !export("_corner.serif"));
        // Underscores elsewhere in the name, as in ligatures, export as usual.
        assert!(export("f_f") && export("a.ss01"));
        // The part can still be switched on by hand.
        let mut f = r.font;
        f.glyph_mut("_part.shoulder").unwrap().export = true;
        assert!(f.validate().is_ok());
    }

    #[test]
    fn swaps_trade_drawings_and_removals_drop_glyphs() {
        let f = sample_font();
        let (h, o) = (
            f.glyph("H").unwrap().layers.clone(),
            f.glyph("O").unwrap().layers.clone(),
        );
        let s = apply_swaps(&f, &[["H".into(), "O".into()]], &["O".into()]).unwrap();
        assert_eq!(s.glyph("H").unwrap().layers, o, "H now has O's drawing");
        assert!(s.glyph("O").is_none(), "O is removed");
        assert_eq!(
            s.glyph("H").unwrap().unicodes,
            f.glyph("H").unwrap().unicodes
        );
        let _ = h;
        assert!(apply_swaps(&f, &[["H".into(), "nope".into()]], &[]).is_err());
        assert_eq!(
            parse_swaps("dollar=dollar.alt\n a = b ").unwrap(),
            vec![
                ["dollar".to_string(), "dollar.alt".to_string()],
                ["a".to_string(), "b".to_string()]
            ]
        );
        assert!(parse_swaps("dollar").is_err());
    }

    #[test]
    fn parses_unicode_values_in_every_spelling() {
        let v = parse_unicodes("0041, U+00e9 uni0416 Ж 0x1F600 0041").unwrap();
        let v: Vec<u32> = v.iter().map(|c| c.0).collect();
        assert_eq!(v, [0x41, 0xE9, 0x416, 0x1F600]);
        // A single hex digit is a value, not a character.
        assert_eq!(parse_unicodes("A").unwrap()[0].0, 0xA);
        assert_eq!(parse_unicodes("é").unwrap()[0].0, 0xE9);
        assert!(parse_unicodes("D800").is_err());
        assert!(parse_unicodes("110000").is_err());
        assert!(parse_unicodes("hello").unwrap_err().contains("hello"));
        assert!(parse_unicodes("  ").unwrap().is_empty());
    }

    #[test]
    fn set_unicodes_refuses_or_moves_a_taken_value() {
        let font = sample_font();
        let a = font
            .glyphs
            .iter()
            .find(|g| !g.unicodes.is_empty())
            .unwrap()
            .clone();
        let b = font
            .glyphs
            .iter()
            .find(|g| g.name != a.name)
            .unwrap()
            .name
            .clone();
        let taken = format!("U+{}", a.unicodes[0]);
        let e = set_unicodes(&font, &b, &taken, false).unwrap_err();
        assert!(e.contains(&a.name), "{e}");
        let moved = set_unicodes(&font, &b, &taken, true).unwrap();
        assert_eq!(moved.glyph(&b).unwrap().unicodes, vec![a.unicodes[0]]);
        assert!(!moved
            .glyph(&a.name)
            .unwrap()
            .unicodes
            .contains(&a.unicodes[0]));
        let cleared = set_unicodes(&font, &a.name, "", false).unwrap();
        assert!(cleared.glyph(&a.name).unwrap().unicodes.is_empty());
        assert!(set_unicodes(&font, "nope", "0041", false).is_err());
    }

    #[test]
    fn parses_names_characters_and_codepoints() {
        let g = parse_glyph_list("A.alt, Ж  U+00E9\nuni0416 zero % a.sc").unwrap();
        let pairs: Vec<(&str, Option<u32>)> =
            g.iter().map(|g| (g.name.as_str(), g.codepoint)).collect();
        assert_eq!(
            pairs,
            [
                ("A.alt", None),
                ("uni0416", Some(0x416)),
                ("uni00E9", Some(0xE9)),
                ("zero", Some(0x30)),
                ("percent", Some(0x25)),
                ("a.sc", None),
            ]
        );
        let e = parse_glyph_list("ok 9bad a b").unwrap_err();
        assert!(e.contains("9bad"), "{e}");
    }

    #[test]
    fn adds_glyphs_and_skips_existing() {
        let font = sample_font();
        let list = parse_glyph_list("a.sc A Aacute U+0041 ellipsis").unwrap();
        let r = add_glyphs(&font, &list).unwrap();
        assert_eq!(r.added, ["a.sc", "Aacute", "ellipsis"]);
        assert_eq!(r.skipped, ["A"]);
        let sc = r.font.glyph("a.sc").unwrap();
        assert_eq!(sc.layers[0].width, font.glyph("a").unwrap().layers[0].width);
        assert_eq!(r.font.glyph("ellipsis").unwrap().unicodes[0].0, 0x2026);
        assert_eq!(r.font.glyphs.last().unwrap().name, "ellipsis");
        r.font.validate().unwrap();
    }

    #[test]
    fn language_coverage_and_add_missing() {
        let font = sample_font();
        let cov = coverage(&font);
        assert_eq!(cov.len(), LANGUAGES.len());
        let latin = cov.iter().find(|c| c.language == "Latin").unwrap();
        assert!(latin.present > 0 && latin.present < latin.total);
        assert!(!latin.missing.contains(&('A' as u32)));
        let greek = cov.iter().find(|c| c.language == "Greek").unwrap();
        assert_eq!(greek.present, 0);
        assert!(greek.total > 100);
        // Adding the missing Greek glyphs fills the coverage.
        let list = missing_glyphs(&font, "Greek").unwrap();
        assert_eq!(list.len(), greek.missing.len());
        let added = add_glyphs(&font, &list).unwrap();
        let cov2 = coverage(&added.font);
        let greek2 = cov2.iter().find(|c| c.language == "Greek").unwrap();
        assert_eq!(greek2.present, greek2.total);
        assert!(greek2.missing.is_empty());
        assert!(missing_glyphs(&added.font, "Greek").unwrap().is_empty());
        assert!(missing_glyphs(&font, "Klingon").is_err());
    }

    #[test]
    fn removing_decomposes_components_and_drops_kerning() {
        let mut font = sample_font();
        let m = font.masters[0].id.clone();
        let h = font.glyph("H").unwrap().layer(&m).unwrap().clone();
        let r = add_glyphs(
            &font,
            &[NewGlyph {
                name: "H.alt".into(),
                codepoint: None,
            }],
        )
        .unwrap();
        font = r.font;
        font.glyph_mut("H.alt")
            .unwrap()
            .layer_mut(&m)
            .unwrap()
            .shapes = vec![
            Shape::component("H", [1.0, 0.0, 0.0, 1.0, 10.0, 20.0]),
            Shape::component("o", IDENTITY),
        ];
        font.kerning.get_mut(&m).unwrap().push(KernPair {
            left: "H".into(),
            right: "o".into(),
            value: -5.0,
        });
        let f = remove_glyphs(&font, &["H".into()]).unwrap();
        f.validate().unwrap();
        assert!(f.glyph("H").is_none());
        let alt = f.glyph("H.alt").unwrap().layer(&m).unwrap();
        let Shape::Path(p) = &alt.shapes[0] else {
            panic!("H became an outline")
        };
        let Shape::Path(orig) = &h.shapes[0] else {
            panic!()
        };
        assert_eq!(p.nodes[0].x, orig.nodes[0].x + 10.0);
        assert_eq!(p.nodes[0].y, orig.nodes[0].y + 20.0);
        assert_eq!(p.nodes[0].kind, NodeKind::Line);
        assert!(
            matches!(&alt.shapes.last().unwrap(), Shape::Component { base, .. } if base == "o")
        );
        assert!(f.kerning[&m]
            .iter()
            .all(|p| p.left != "H" && p.right != "H"));
        assert!(remove_glyphs(&font, &["nope".into()]).is_err());
    }

    #[test]
    fn composes_transforms() {
        let scale = [2.0, 0.0, 0.0, 2.0, 0.0, 0.0];
        let shift = [1.0, 0.0, 0.0, 1.0, 5.0, 0.0];
        // Shift first, then scale: x' = 2(x + 5).
        assert_eq!(compose(scale, shift), [2.0, 0.0, 0.0, 2.0, 10.0, 0.0]);
    }
}
