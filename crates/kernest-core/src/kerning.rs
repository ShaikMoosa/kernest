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

/// What Compress or Clean Up changed in one master.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KernTidy {
    /// Pairs moved one step up to a group (Compress).
    pub moved: usize,
    /// Pairs removed: redundant, zero, or naming nothing.
    pub removed: usize,
    /// Glyph-level pairs kept as real exceptions (their value differs from the group's).
    pub kept: usize,
}

impl std::ops::AddAssign for KernTidy {
    fn add_assign(&mut self, o: Self) {
        self.moved += o.moved;
        self.removed += o.removed;
        self.kept += o.kept;
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

/// Compress (S56), one step: each pair with a glyph side moves up to the glyph's
/// group, left side first (T–m → @T–m, then @T–@n). A pair the group kerning already
/// gives is removed; one that differs from it stays as a real exception.
pub fn compress_pairs(font: &Font, pairs: &[KernPair]) -> (Vec<KernPair>, KernTidy) {
    let mut out: Vec<KernPair> = pairs.to_vec();
    let mut tidy = KernTidy::default();
    let lookup = |list: &[KernPair], l: &str, r: &str| find(list, l, r);
    let mut i = 0;
    while i < out.len() {
        let p = out[i].clone();
        let (l_glyph, r_glyph) = (!p.left.starts_with('@'), !p.right.starts_with('@'));
        if !l_glyph && !r_glyph {
            i += 1;
            continue;
        }
        let lg = if l_glyph {
            left_key(font, &p.left, true)
        } else {
            p.left.clone()
        };
        let rg = if r_glyph {
            right_key(font, &p.right, true)
        } else {
            p.right.clone()
        };
        if lg == p.left && rg == p.right {
            // No groups to move to.
            i += 1;
            continue;
        }
        // The group pair the glyphs fall under decides whether this is an exception.
        if let Some(v) = lookup(&out, &lg, &rg)
            .filter(|_| (lg.clone(), rg.clone()) != (p.left.clone(), p.right.clone()))
        {
            if close(v, p.value) {
                out.remove(i);
                tidy.removed += 1;
            } else {
                tidy.kept += 1;
                i += 1;
            }
            continue;
        }
        // One step up: the left side if it can move, else the right.
        let (tl, tr) = if lg != p.left {
            (lg, p.right.clone())
        } else {
            (p.left.clone(), rg)
        };
        match lookup(&out, &tl, &tr) {
            Some(v) if close(v, p.value) => {
                out.remove(i);
                tidy.removed += 1;
            }
            Some(_) => {
                tidy.kept += 1;
                i += 1;
            }
            None => {
                out[i].left = tl;
                out[i].right = tr;
                tidy.moved += 1;
                i += 1;
            }
        }
    }
    (out, tidy)
}

/// Clean Up (S56): removes pairs that do nothing: zero values, sides naming a glyph or
/// group that doesn't exist, and glyph-level pairs equal to the group kerning they
/// fall under.
pub fn clean_up_pairs(font: &Font, pairs: &[KernPair]) -> (Vec<KernPair>, KernTidy) {
    let left_groups: std::collections::HashSet<&str> = font
        .glyphs
        .iter()
        .filter_map(|g| g.right_group.as_deref())
        .collect();
    let right_groups: std::collections::HashSet<&str> = font
        .glyphs
        .iter()
        .filter_map(|g| g.left_group.as_deref())
        .collect();
    let exists = |side: &str, groups: &std::collections::HashSet<&str>| match side.strip_prefix('@')
    {
        Some(g) => groups.contains(g),
        None => font.glyph(side).is_some(),
    };
    let mut tidy = KernTidy::default();
    let mut out: Vec<KernPair> = Vec::with_capacity(pairs.len());
    for p in pairs {
        if close(p.value, 0.0) || !exists(&p.left, &left_groups) || !exists(&p.right, &right_groups)
        {
            tidy.removed += 1;
            continue;
        }
        out.push(p.clone());
    }
    let snapshot = out.clone();
    out.retain(|p| {
        if p.left.starts_with('@') && p.right.starts_with('@') {
            return true;
        }
        let lg = if p.left.starts_with('@') {
            p.left.clone()
        } else {
            left_key(font, &p.left, true)
        };
        let rg = if p.right.starts_with('@') {
            p.right.clone()
        } else {
            right_key(font, &p.right, true)
        };
        // The most general pair that applies, other than this one.
        let general = [
            (lg.clone(), rg.clone()),
            (lg.clone(), p.right.clone()),
            (p.left.clone(), rg.clone()),
        ]
        .into_iter()
        .filter(|(l, r)| (l, r) != (&p.left, &p.right))
        .find_map(|(l, r)| find(&snapshot, &l, &r));
        match general {
            Some(v) if close(v, p.value) => {
                tidy.removed += 1;
                false
            }
            _ => true,
        }
    });
    (out, tidy)
}

/// Kerning pasted into a master: `overwrite` replaces pairs already there, otherwise
/// they keep their values and only new pairs are added.
pub fn paste_pairs(existing: &[KernPair], pasted: &[KernPair], overwrite: bool) -> Vec<KernPair> {
    let mut out = existing.to_vec();
    for p in pasted {
        match out
            .iter()
            .position(|q| q.left == p.left && q.right == p.right)
        {
            Some(i) if overwrite => out[i].value = p.value,
            Some(_) => {}
            None => out.push(p.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kp(l: &str, r: &str, v: f64) -> KernPair {
        KernPair {
            left: l.into(),
            right: r.into(),
            value: v,
        }
    }

    fn grouped() -> Font {
        let mut f = crate::sample::sample_font();
        for (name, left, right) in [
            ("T", "T", "T"),
            ("m", "n", "n"),
            ("n", "n", "n"),
            ("o", "o", "o"),
        ] {
            let g = f.glyph_mut(name).unwrap();
            g.left_group = Some(left.into());
            g.right_group = Some(right.into());
        }
        f
    }

    #[test]
    fn compress_moves_one_step_and_keeps_real_exceptions() {
        let f = grouped();
        let pairs = vec![
            kp("T", "m", -60.0),
            kp("@T", "@o", -70.0),
            kp("T", "o", -40.0),
            kp("T", "n", -60.0),
        ];
        let (once, t1) = compress_pairs(&f, &pairs);
        // T–m moves to @T–m; T–o differs from @T–@o (kept); T–n moves to @T–n.
        assert!(once.contains(&kp("@T", "m", -60.0)), "{once:?}");
        assert!(once.contains(&kp("T", "o", -40.0)));
        assert_eq!(t1.kept, 1);
        let (twice, _) = compress_pairs(&f, &once);
        // @T–m becomes @T–@n; @T–n then equals it and is removed.
        assert!(twice.contains(&kp("@T", "@n", -60.0)), "{twice:?}");
        assert!(!twice.iter().any(|p| p.left == "@T" && p.right == "n"));
        assert!(twice.contains(&kp("T", "o", -40.0)));
    }

    #[test]
    fn clean_up_removes_pairs_that_do_nothing() {
        let f = grouped();
        let pairs = vec![
            kp("@T", "@o", -70.0),
            kp("T", "o", -70.0),
            kp("T", "o.missing", -10.0),
            kp("@nobody", "o", -10.0),
            kp("T", "m", 0.0),
            kp("T", "n", -30.0),
        ];
        let (out, tidy) = clean_up_pairs(&f, &pairs);
        assert_eq!(out, vec![kp("@T", "@o", -70.0), kp("T", "n", -30.0)]);
        assert_eq!(tidy.removed, 4);
    }

    #[test]
    fn paste_overwrites_or_keeps() {
        let a = vec![kp("A", "V", -50.0)];
        let b = vec![kp("A", "V", -80.0), kp("T", "o", -40.0)];
        assert_eq!(
            paste_pairs(&a, &b, true),
            vec![kp("A", "V", -80.0), kp("T", "o", -40.0)]
        );
        assert_eq!(
            paste_pairs(&a, &b, false),
            vec![kp("A", "V", -50.0), kp("T", "o", -40.0)]
        );
    }
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
