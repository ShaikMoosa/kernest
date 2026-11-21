//! A simplified bidirectional algorithm for the text view (SMO-33).
//!
//! Each item gets a strong direction (right-to-left letters, left-to-right letters and
//! digits) or none (spaces, punctuation, marks). Neutrals between two items of the
//! same direction take it; other neutrals take the paragraph direction. Lines are then
//! reordered run by run: two embedding levels, enough for Arabic or Hebrew text with
//! Latin words and numbers in it.

use crate::glyphdata::{self, Category};
use crate::model::{Glyph, WritingDirection};

/// The writing direction of a text tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    Ltr,
    Rtl,
    Vertical,
}

impl TextDirection {
    /// From "ltr", "rtl" or "vertical"; anything else is `None` (follow the text).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ltr" => Some(Self::Ltr),
            "rtl" => Some(Self::Rtl),
            "vertical" => Some(Self::Vertical),
            _ => None,
        }
    }

    /// The paragraph direction for horizontal text: the chosen one, or the first
    /// strong item's when none was chosen. Vertical text counts as left to right.
    pub fn paragraph_rtl(dir: Option<Self>, strong: &[Option<bool>]) -> bool {
        match dir {
            Some(Self::Rtl) => true,
            Some(Self::Ltr | Self::Vertical) => false,
            None => first_strong(strong).unwrap_or(false),
        }
    }
}

/// True for code points in the right-to-left blocks (Hebrew, Arabic, Syriac, Thaana,
/// NKo, Samaritan, Mandaic and their presentation forms, plus the supplementary RTL
/// planes).
fn rtl_block(cp: u32) -> bool {
    (0x0590..=0x08FF).contains(&cp)
        || (0xFB1D..=0xFDFF).contains(&cp)
        || (0xFE70..=0xFEFF).contains(&cp)
        || (0x10800..=0x10FFF).contains(&cp)
        || (0x1E800..=0x1EFFF).contains(&cp)
}

/// The strong direction of a code point: `Some(true)` right to left, `Some(false)`
/// left to right (letters and digits), `None` for neutrals and marks.
pub fn strong_codepoint(cp: u32) -> Option<bool> {
    let c = char::from_u32(cp)?;
    if c.is_alphabetic() && glyphdata::category(cp) != Category::Mark {
        Some(rtl_block(cp))
    } else if c.is_numeric() {
        // Digits read left to right, also inside right-to-left text.
        Some(false)
    } else {
        None
    }
}

/// The strong direction of a glyph: its writing direction when set (BIDI is neutral),
/// else its code point, else its name (`beh-ar.init` is Arabic).
pub fn strong_glyph(g: &Glyph) -> Option<bool> {
    match g.direction {
        Some(WritingDirection::Ltr) => return Some(false),
        Some(WritingDirection::Rtl) => return Some(true),
        Some(WritingDirection::Bidi) => return None,
        None => {}
    }
    if let Some(cp) = g
        .unicodes
        .first()
        .map(|c| c.0)
        .or_else(|| glyphdata::codepoint_for_name(&g.name))
    {
        return strong_codepoint(cp);
    }
    let base = g.name.split('.').next().unwrap_or(&g.name);
    let suffix = base.rsplit_once('-').map(|(_, s)| s);
    if matches!(suffix, Some("ar" | "hb" | "syriac" | "farsi" | "urdu")) {
        return Some(true);
    }
    match glyphdata::script_for_name(&g.name) {
        "Arabic" | "Hebrew" | "Syriac" => Some(true),
        "Latin" | "Greek" | "Cyrillic" => Some(false),
        _ => None,
    }
}

/// The direction of the first strong item, if any.
pub fn first_strong(strong: &[Option<bool>]) -> Option<bool> {
    strong.iter().flatten().next().copied()
}

/// Resolves each item to right to left (`true`) or left to right, for a paragraph of
/// the given direction.
pub fn resolve(strong: &[Option<bool>], rtl: bool) -> Vec<bool> {
    (0..strong.len())
        .map(|i| {
            strong[i].unwrap_or_else(|| {
                let before = strong[..i].iter().rev().flatten().next().copied();
                let after = strong[i + 1..].iter().flatten().next().copied();
                match (before, after) {
                    (Some(a), Some(b)) if a == b => a,
                    _ => rtl,
                }
            })
        })
        .collect()
}

/// The visual order (left to right) of a line's items, from their resolved directions.
pub fn visual_order(resolved: &[bool], rtl: bool) -> Vec<usize> {
    let mut runs: Vec<(usize, usize, bool)> = Vec::new();
    for (i, &r) in resolved.iter().enumerate() {
        match runs.last_mut() {
            Some(last) if last.2 == r => last.1 = i + 1,
            _ => runs.push((i, i + 1, r)),
        }
    }
    if rtl {
        runs.reverse();
    }
    runs.into_iter()
        .flat_map(|(a, b, r)| {
            let run: Vec<usize> = (a..b).collect();
            if r {
                run.into_iter().rev().collect::<Vec<_>>()
            } else {
                run
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strong(s: &str) -> Vec<Option<bool>> {
        s.chars().map(|c| strong_codepoint(c as u32)).collect()
    }

    #[test]
    fn arabic_with_latin_and_digits() {
        // "سل ab 12" in a right-to-left paragraph.
        let s = strong("\u{633}\u{644} ab 12");
        assert_eq!(first_strong(&s), Some(true));
        let r = resolve(&s, true);
        // The spaces between Arabic and Latin take the paragraph direction; the one
        // between two left-to-right runs stays left to right.
        assert_eq!(r, [true, true, true, false, false, false, false, false]);
        // Latin and digits keep their order, at the left; the Arabic reads from the right.
        assert_eq!(visual_order(&r, true), [3, 4, 5, 6, 7, 2, 1, 0]);
    }

    #[test]
    fn left_to_right_paragraph_with_an_arabic_word() {
        let s = strong("a \u{633}\u{644} b");
        let r = resolve(&s, false);
        assert_eq!(r, [false, false, true, true, false, false]);
        assert_eq!(visual_order(&r, false), [0, 1, 3, 2, 4, 5]);
    }

    #[test]
    fn glyph_directions() {
        let g = |name: &str, cps: &[u32]| {
            Glyph::new(
                name,
                cps.iter().map(|c| crate::model::Codepoint(*c)).collect(),
            )
        };
        assert_eq!(strong_glyph(&g("beh-ar", &[0x628])), Some(true));
        assert_eq!(strong_glyph(&g("beh-ar.init", &[])), Some(true));
        assert_eq!(strong_glyph(&g("a.sc", &[])), Some(false));
        assert_eq!(strong_glyph(&g("space", &[0x20])), None);
        let mut a = g("a", &[0x61]);
        a.direction = Some(WritingDirection::Rtl);
        assert_eq!(strong_glyph(&a), Some(true));
    }
}
