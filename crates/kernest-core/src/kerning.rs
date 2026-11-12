//! Kerning lookup and editing.
//!
//! A pair's sides are a glyph name or `@group`. For a pair of glyphs (L, R):
//! - L's side uses L's **right** group (the side facing R),
//! - R's side uses R's **left** group.
//!
//! Lookup order, most specific first: glyph–glyph, glyph–@group, @group–glyph,
//! @group–@group. A glyph-level pair is an *exception* to its group kerning.

use serde::Serialize;

use crate::model::{Font, KernPair};

/// Where a kerning value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PairKind {
    /// Both sides are groups.
    Group,
    /// At least one side is a specific glyph.
    Exception,
}

/// Which side of a pair uses its specific glyph instead of its group when editing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// Use groups on both sides where the glyphs have them.
    Group,
    /// Left side is the glyph, right side its group.
    LeftException,
    /// Left side is the group, right side the glyph.
    RightException,
    /// Both sides are glyphs.
    Exception,
}

/// The kerning key for a glyph on the left side of a pair (its right group, or itself).
pub fn left_key(font: &Font, glyph: &str, use_group: bool) -> String {
    match font.glyph(glyph).and_then(|g| g.right_group.as_ref()) {
        Some(g) if use_group => format!("@{g}"),
        _ => glyph.to_string(),
    }
}

/// The kerning key for a glyph on the right side of a pair (its left group, or itself).
pub fn right_key(font: &Font, glyph: &str, use_group: bool) -> String {
    match font.glyph(glyph).and_then(|g| g.left_group.as_ref()) {
        Some(g) if use_group => format!("@{g}"),
        _ => glyph.to_string(),
    }
}

/// The keys an edit of the pair (left, right) writes to, for the given mode.
pub fn edit_keys(font: &Font, left: &str, right: &str, mode: EditMode) -> (String, String) {
    let (lg, rg) = match mode {
        EditMode::Group => (true, true),
        EditMode::LeftException => (false, true),
        EditMode::RightException => (true, false),
        EditMode::Exception => (false, false),
    };
    (left_key(font, left, lg), right_key(font, right, rg))
}

fn find(pairs: &[KernPair], l: &str, r: &str) -> Option<f64> {
    pairs
        .iter()
        .find(|p| p.left == l && p.right == r)
        .map(|p| p.value)
}

/// Kerning between two glyphs in a master, with where it came from.
pub fn pair_value(
    font: &Font,
    master_id: &str,
    left: &str,
    right: &str,
) -> Option<(f64, PairKind, String, String)> {
    let pairs = font.kerning.get(master_id)?;
    let (lg, rg) = (left_key(font, left, true), right_key(font, right, true));
    let candidates = [
        (left.to_string(), right.to_string()),
        (left.to_string(), rg.clone()),
        (lg.clone(), right.to_string()),
        (lg, rg),
    ];
    let mut seen = std::collections::HashSet::new();
    for (l, r) in candidates {
        if !seen.insert((l.clone(), r.clone())) {
            continue;
        }
        if let Some(v) = find(pairs, &l, &r) {
            let kind = if l.starts_with('@') && r.starts_with('@') {
                PairKind::Group
            } else {
                PairKind::Exception
            };
            return Some((v, kind, l, r));
        }
    }
    None
}

/// Returns a copy of `pairs` with (left, right) set to `value`; a value of 0 removes the pair.
/// New pairs are appended so files stay stable and diffs small.
pub fn with_pair(pairs: &[KernPair], left: &str, right: &str, value: f64) -> Vec<KernPair> {
    let mut out: Vec<KernPair> = pairs.to_vec();
    let value = value.round();
    match out.iter().position(|p| p.left == left && p.right == right) {
        Some(i) if value == 0.0 => {
            out.remove(i);
        }
        Some(i) => out[i].value = value,
        None if value != 0.0 => out.push(KernPair {
            left: left.into(),
            right: right.into(),
            value,
        }),
        None => {}
    }
    out
}

/// Validates a pair side typed by a user: an existing glyph name or `@group`.
pub fn validate_side(font: &Font, side: &str) -> Result<(), String> {
    let s = side.trim();
    if s.is_empty() {
        return Err("A kerning pair needs a glyph or @group on both sides.".into());
    }
    if let Some(g) = s.strip_prefix('@') {
        if g.is_empty() || g.chars().any(char::is_whitespace) {
            return Err(format!("\"{s}\" is not a valid group name."));
        }
        return Ok(());
    }
    if font.glyph(s).is_none() {
        return Err(format!("There is no glyph named \"{s}\"."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    fn font() -> Font {
        let mut f = sample_font();
        for (g, l, r) in [("T", "T", "T"), ("o", "o", "o"), ("L", "H", "L")] {
            let g = f.glyph_mut(g).unwrap();
            g.left_group = Some(l.into());
            g.right_group = Some(r.into());
        }
        f.kerning.insert(
            "m01".into(),
            vec![
                KernPair {
                    left: "@T".into(),
                    right: "@o".into(),
                    value: -60.0,
                },
                KernPair {
                    left: "T".into(),
                    right: "o".into(),
                    value: -40.0,
                },
                KernPair {
                    left: "@L".into(),
                    right: "T".into(),
                    value: -80.0,
                },
            ],
        );
        f
    }

    #[test]
    fn lookup_precedence() {
        let f = font();
        // Glyph exception wins over the group pair.
        assert_eq!(
            pair_value(&f, "m01", "T", "o").map(|v| (v.0, v.1)),
            Some((-40.0, PairKind::Exception))
        );
        // Group-to-glyph.
        assert_eq!(
            pair_value(&f, "m01", "L", "T").map(|v| (v.0, v.1)),
            Some((-80.0, PairKind::Exception))
        );
        // Nothing for unrelated glyphs.
        assert_eq!(pair_value(&f, "m01", "H", "H"), None);
        assert_eq!(pair_value(&f, "m02", "T", "o"), None);
    }

    #[test]
    fn edit_keys_follow_mode() {
        let f = font();
        assert_eq!(
            edit_keys(&f, "T", "o", EditMode::Group),
            ("@T".into(), "@o".into())
        );
        assert_eq!(
            edit_keys(&f, "T", "o", EditMode::LeftException),
            ("T".into(), "@o".into())
        );
        assert_eq!(
            edit_keys(&f, "T", "o", EditMode::RightException),
            ("@T".into(), "o".into())
        );
        assert_eq!(
            edit_keys(&f, "T", "o", EditMode::Exception),
            ("T".into(), "o".into())
        );
        // Glyphs without groups fall back to themselves.
        assert_eq!(
            edit_keys(&f, "H", "I", EditMode::Group),
            ("H".into(), "I".into())
        );
    }

    #[test]
    fn set_and_remove_pairs() {
        let f = font();
        let pairs = &f.kerning["m01"];
        let p = with_pair(pairs, "@T", "@o", -70.4);
        assert_eq!(p[0].value, -70.0);
        let p = with_pair(&p, "H", "H", 10.0);
        assert_eq!(p.len(), 4);
        let p = with_pair(&p, "H", "H", 0.0);
        assert_eq!(p.len(), 3);
        let p = with_pair(&p, "x", "y", 0.0);
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn side_validation() {
        let f = font();
        assert!(validate_side(&f, "T").is_ok());
        assert!(validate_side(&f, "@anything").is_ok());
        assert!(validate_side(&f, "@").is_err());
        assert!(validate_side(&f, "nope").is_err());
        assert!(validate_side(&f, " ").is_err());
    }
}
