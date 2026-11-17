//! Adding and removing glyphs.
//!
//! Like the structural edits, each function returns an edited copy of the font; the
//! caller stores it through `History::apply_font` so the change can be undone.

use std::collections::HashSet;

use crate::glyphdata::{codepoint_for_name, name_for};
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KernPair, NodeKind, IDENTITY};
    use crate::sample::sample_font;

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
