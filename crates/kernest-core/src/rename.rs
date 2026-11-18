//! Batch glyph rename: changes glyph names and every reference to them.
//!
//! The find/replace pattern (plain or regular expression) runs in the frontend;
//! this module receives the resulting old→new map and updates the glyphs plus
//! every place a name is used: components, kerning pairs, metrics keys, list
//! filters and feature code. Bracket alternates live inside their glyph, so they
//! follow the rename on their own. Feature code is the one place a rename can
//! surprise the designer, so the lines it changes are returned for a warning.

use std::collections::BTreeMap;

use crate::glyphset::valid_name;
use crate::metricskeys;
use crate::model::{Font, Shape};

type R<T> = Result<T, String>;

/// Renames glyphs and updates every reference. Returns the new font and the
/// feature-code lines that changed (new text, for a warning). One undoable step
/// upstream. Locked glyphs are refused by the command before this runs.
pub fn rename_glyphs(font: &Font, renames: &BTreeMap<String, String>) -> R<(Font, Vec<String>)> {
    if renames.is_empty() {
        return Ok((font.clone(), Vec::new()));
    }
    for (old, new) in renames {
        if font.glyph(old).is_none() {
            return Err(format!("There is no glyph named \"{old}\"."));
        }
        if old != new && !valid_name(new) {
            return Err(format!("\"{new}\" can't be used as a glyph name."));
        }
    }
    // The names left after the rename must stay unique (this also catches a new
    // name clashing with a glyph that is not being renamed).
    let mut final_names: Vec<&str> = font
        .glyphs
        .iter()
        .map(|g| renames.get(&g.name).map(String::as_str).unwrap_or(&g.name))
        .collect();
    final_names.sort_unstable();
    for pair in final_names.windows(2) {
        if pair[0] == pair[1] {
            return Err(format!("Two glyphs can't both be called \"{}\".", pair[0]));
        }
    }

    let mut next = font.clone();
    for g in &mut next.glyphs {
        if let Some(new) = renames.get(&g.name) {
            g.name = new.clone();
        }
        for layer in &mut g.layers {
            for shape in &mut layer.shapes {
                if let Shape::Component { base, .. } = shape {
                    if let Some(new) = renames.get(base) {
                        *base = new.clone();
                    }
                }
            }
            for text in [
                &mut layer.metrics_keys.left,
                &mut layer.metrics_keys.right,
                &mut layer.metrics_keys.width,
            ]
            .into_iter()
            .flatten()
            {
                if let Some(rewritten) = metricskeys::rename_in_key(text, renames) {
                    *text = rewritten;
                }
            }
        }
    }
    for pairs in next.kerning.values_mut() {
        for pair in pairs {
            // Group references start with '@' and are never glyph-name keys.
            if let Some(new) = renames.get(&pair.left) {
                pair.left = new.clone();
            }
            if let Some(new) = renames.get(&pair.right) {
                pair.right = new.clone();
            }
        }
    }
    for filter in &mut next.filters {
        for name in &mut filter.glyphs {
            if let Some(new) = renames.get(name) {
                *name = new.clone();
            }
        }
    }
    let (features, changed) = rename_in_features(&next.features, renames);
    next.features = features;
    Ok((next, changed))
}

/// Rewrites glyph-name tokens in feature code, leaving class names (`@…`),
/// comments and quoted strings alone. Returns the new code and the changed lines.
fn rename_in_features(code: &str, renames: &BTreeMap<String, String>) -> (String, Vec<String>) {
    let mut out = String::with_capacity(code.len());
    let mut changed = Vec::new();
    for line in code.split_inclusive('\n') {
        let rewritten = rename_line(line, renames);
        if rewritten != line {
            changed.push(rewritten.trim_end_matches(['\n', '\r']).to_string());
        }
        out.push_str(&rewritten);
    }
    (out, changed)
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

fn rename_line(line: &str, renames: &BTreeMap<String, String>) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' {
            // A comment runs to the end of the line; leave it untouched.
            out.extend(&chars[i..]);
            break;
        }
        if c == '"' {
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                if chars[i] == '"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if is_name_char(c) {
            let start = i;
            while i < chars.len() && is_name_char(chars[i]) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            let is_class = start > 0 && chars[start - 1] == '@';
            if !is_class {
                if let Some(new) = renames.get(&token) {
                    out.push_str(new);
                    continue;
                }
            }
            out.push_str(&token);
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{KernPair, MetricsKeys};
    use crate::sample::sample_font;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn renames_glyphs_and_every_reference() {
        let mut font = sample_font();
        // A component of H, a kerning pair, a metrics key and a list filter all
        // reference H; feature code uses H and a class named @H.
        let a = font.glyph_mut("A").unwrap();
        a.layers[0]
            .shapes
            .push(Shape::component("H", crate::model::IDENTITY));
        a.layers[0].metrics_keys = MetricsKeys {
            left: Some("=H".into()),
            right: Some("=|H+10".into()),
            width: None,
        };
        font.kerning
            .entry("m01".into())
            .or_default()
            .push(KernPair {
                left: "H".into(),
                right: "A".into(),
                value: -20.0,
            });
        font.filters.push(crate::smartfilter::Filter {
            name: "My list".into(),
            kind: crate::smartfilter::FilterKind::List,
            rules: Default::default(),
            glyphs: vec!["H".into(), "A".into()],
        });
        font.features =
            "feature kern {\n  pos H A -20;\n  @H = [H A];\n  sub H' by H.alt;\n} kern;\n".into();

        let (next, changed) = rename_glyphs(&font, &map(&[("H", "Hh")])).unwrap();
        assert!(next.glyph("Hh").is_some() && next.glyph("H").is_none());
        // Component base follows.
        let a = next.glyph("A").unwrap();
        assert!(matches!(&a.layers[0].shapes[0], Shape::Component { base, .. } if base == "Hh"));
        // Metrics keys follow, keeping the opposite side and the arithmetic.
        assert_eq!(a.layers[0].metrics_keys.left.as_deref(), Some("=Hh"));
        assert_eq!(a.layers[0].metrics_keys.right.as_deref(), Some("=|Hh+10"));
        // Kerning pair follows.
        assert!(next.kerning["m01"]
            .iter()
            .any(|p| p.left == "Hh" && p.right == "A" && p.value == -20.0));
        // List filter follows.
        assert_eq!(next.filters[0].glyphs, ["Hh", "A"]);
        // Feature code: the glyph tokens change, the class name @H stays.
        assert!(next.features.contains("pos Hh A -20;"));
        assert!(next.features.contains("@H = [Hh A];"));
        assert!(next.features.contains("sub Hh' by H.alt;"));
        assert_eq!(changed.len(), 3, "{changed:?}");
    }

    #[test]
    fn leaves_unrelated_names_and_comments_alone() {
        let font = sample_font();
        let code = "# rename a here\nfeature liga {\n  sub f i by fi;\n  sub a by a.sc;\n} liga;\n";
        let (next, changed) = rename_glyphs(
            &{
                let mut f = font.clone();
                f.features = code.into();
                f
            },
            &map(&[("a", "alpha")]),
        )
        .unwrap();
        // "a" changes, "a.sc" and the comment's "a" do not; "fi"/"f"/"i" untouched.
        assert!(next.features.contains("sub alpha by a.sc;"));
        assert!(next.features.contains("# rename a here"));
        assert!(next.features.contains("sub f i by fi;"));
        assert_eq!(changed, ["  sub alpha by a.sc;"]);
    }

    #[test]
    fn refuses_bad_names_clashes_and_missing_glyphs() {
        let font = sample_font();
        assert!(rename_glyphs(&font, &map(&[("H", "9bad")])).is_err());
        // Clashes with a kept glyph.
        assert!(rename_glyphs(&font, &map(&[("H", "A")])).is_err());
        // Two renamed glyphs landing on the same name.
        assert!(rename_glyphs(&font, &map(&[("H", "X"), ("I", "X")])).is_err());
        assert!(rename_glyphs(&font, &map(&[("nope", "X")])).is_err());
        // A swap is fine: both final names are unique.
        let (swapped, _) = rename_glyphs(&font, &map(&[("H", "I"), ("I", "H")])).unwrap();
        assert!(swapped.glyph("H").is_some() && swapped.glyph("I").is_some());
        // Empty map is a no-op.
        let (same, changed) = rename_glyphs(&font, &BTreeMap::new()).unwrap();
        assert!(changed.is_empty());
        assert_eq!(same.glyphs.len(), font.glyphs.len());
    }
}
