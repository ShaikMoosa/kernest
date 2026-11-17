//! Minimal glyph data: production names and categories for common code points,
//! plus the built-in glyph sets offered by File › New.
//!
//! This is intentionally small for now; a full Glyph Info database (S34) replaces it later.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Category {
    Uppercase,
    Lowercase,
    Letter,
    Number,
    Punctuation,
    Symbol,
    Separator,
    Mark,
    Other,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Uppercase => "Uppercase",
            Category::Lowercase => "Lowercase",
            Category::Letter => "Letter",
            Category::Number => "Number",
            Category::Punctuation => "Punctuation",
            Category::Symbol => "Symbol",
            Category::Separator => "Separator",
            Category::Mark => "Mark",
            Category::Other => "Other",
        }
    }
}

const ASCII_NAMES: &[(u32, &str)] = &[
    (0x20, "space"),
    (0x21, "exclam"),
    (0x22, "quotedbl"),
    (0x23, "numbersign"),
    (0x24, "dollar"),
    (0x25, "percent"),
    (0x26, "ampersand"),
    (0x27, "quotesingle"),
    (0x28, "parenleft"),
    (0x29, "parenright"),
    (0x2A, "asterisk"),
    (0x2B, "plus"),
    (0x2C, "comma"),
    (0x2D, "hyphen"),
    (0x2E, "period"),
    (0x2F, "slash"),
    (0x30, "zero"),
    (0x31, "one"),
    (0x32, "two"),
    (0x33, "three"),
    (0x34, "four"),
    (0x35, "five"),
    (0x36, "six"),
    (0x37, "seven"),
    (0x38, "eight"),
    (0x39, "nine"),
    (0x3A, "colon"),
    (0x3B, "semicolon"),
    (0x3C, "less"),
    (0x3D, "equal"),
    (0x3E, "greater"),
    (0x3F, "question"),
    (0x40, "at"),
    (0x5B, "bracketleft"),
    (0x5C, "backslash"),
    (0x5D, "bracketright"),
    (0x5E, "asciicircum"),
    (0x5F, "underscore"),
    (0x60, "grave"),
    (0x7B, "braceleft"),
    (0x7C, "bar"),
    (0x7D, "braceright"),
    (0x7E, "asciitilde"),
    (0xA0, "nbspace"),
    (0xA9, "copyright"),
    (0xAE, "registered"),
    (0xB0, "degree"),
    (0x2013, "endash"),
    (0x2014, "emdash"),
    (0x2018, "quoteleft"),
    (0x2019, "quoteright"),
    (0x201C, "quotedblleft"),
    (0x201D, "quotedblright"),
    (0x2022, "bullet"),
    (0x2026, "ellipsis"),
    (0x20AC, "Euro"),
];

/// Names kept for letters whose production name is not a single character, so
/// composite recipes and Add Glyphs can refer to them (`AEacute` uses `AE`).
const EXTRA_NAMES: &[(u32, &str)] = &[
    (0xC6, "AE"),
    (0xE6, "ae"),
    (0xD8, "Oslash"),
    (0xF8, "oslash"),
    (0x1B7, "Ezh"),
    (0x292, "ezh"),
];

/// Combining marks keep their production names (`acutecomb`) rather than `uni0301`,
/// so adding a mark by name or by code point lands on the same encoded glyph and
/// composite recipes can find it.
const MARK_NAMES: &[(u32, &str)] = &[
    (0x300, "gravecomb"),
    (0x301, "acutecomb"),
    (0x302, "circumflexcomb"),
    (0x303, "tildecomb"),
    (0x304, "macroncomb"),
    (0x306, "brevecomb"),
    (0x307, "dotaccentcomb"),
    (0x308, "dieresiscomb"),
    (0x309, "hookabovecomb"),
    (0x30A, "ringcomb"),
    (0x30B, "doubleacutecomb"),
    (0x30C, "caroncomb"),
    (0x30F, "inverteddoublegravecomb"),
    (0x311, "invertedbrevecomb"),
    (0x31B, "horncomb"),
    (0x323, "dotbelowcomb"),
    (0x324, "dieresisbelowcomb"),
    (0x325, "ringbelowcomb"),
    (0x326, "commaaccentcomb"),
    (0x327, "cedillacomb"),
    (0x328, "ogonekcomb"),
    (0x32D, "circumflexbelowcomb"),
    (0x32E, "brevebelowcomb"),
    (0x330, "tildebelowcomb"),
    (0x331, "macronbelowcomb"),
];

/// Production-style glyph name for a code point (`A`, `zero`, `uni0416`, …).
pub fn name_for(cp: u32) -> String {
    if let Some((_, n)) = ASCII_NAMES.iter().find(|(c, _)| *c == cp) {
        return (*n).to_string();
    }
    if let Some((_, n)) = MARK_NAMES.iter().find(|(c, _)| *c == cp) {
        return (*n).to_string();
    }
    match char::from_u32(cp) {
        Some(c) if c.is_ascii_alphabetic() => c.to_string(),
        _ if cp > 0xFFFF => format!("u{cp:05X}"),
        _ => format!("uni{cp:04X}"),
    }
}

/// The code point a production-style name stands for: `A`, `zero`, `uni0416`, `u1F600`,
/// and composite names such as `Aacute`. Suffixed names such as `a.sc` have none.
pub fn codepoint_for_name(name: &str) -> Option<u32> {
    if let Some((cp, _)) = ASCII_NAMES.iter().find(|(_, n)| *n == name) {
        return Some(*cp);
    }
    if let Some((cp, _)) = EXTRA_NAMES.iter().find(|(_, n)| *n == name) {
        return Some(*cp);
    }
    if let Some((cp, _)) = MARK_NAMES.iter().find(|(_, n)| *n == name) {
        return Some(*cp);
    }
    if let Some(cp) = crate::decompose::codepoint(name) {
        return Some(cp);
    }
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return Some(c as u32);
        }
    }
    let hex = name
        .strip_prefix("uni")
        .filter(|h| h.len() == 4)
        .or_else(|| {
            name.strip_prefix('u')
                .filter(|h| (4..=6).contains(&h.len()))
        })?;
    if !hex
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
    {
        return None;
    }
    u32::from_str_radix(hex, 16)
        .ok()
        .filter(|&cp| char::from_u32(cp).is_some())
}

/// Category for a code point using the standard library's Unicode tables.
pub fn category(cp: u32) -> Category {
    let Some(c) = char::from_u32(cp) else {
        return Category::Other;
    };
    if (0x0300..=0x036F).contains(&cp)
        || (0x0610..=0x061A).contains(&cp)
        || (0x064B..=0x065F).contains(&cp)
        || cp == 0x0670
        || (0x06D6..=0x06DC).contains(&cp)
        || (0x06DF..=0x06E4).contains(&cp)
        || (0x06E7..=0x06E8).contains(&cp)
        || (0x06EA..=0x06ED).contains(&cp)
    {
        Category::Mark
    } else if c.is_uppercase() {
        Category::Uppercase
    } else if c.is_lowercase() {
        Category::Lowercase
    } else if c.is_alphabetic() {
        Category::Letter
    } else if c.is_numeric() {
        Category::Number
    } else if c.is_whitespace() {
        Category::Separator
    } else if c.is_ascii_punctuation() && !"$+<=>^`|~".contains(c)
        || matches!(cp, 0x2013..=0x2026 | 0xA1 | 0xBF)
    {
        Category::Punctuation
    } else if c.is_control() {
        Category::Other
    } else {
        Category::Symbol
    }
}

/// Best-effort category for unencoded glyphs from their name (`a.sc`, `f_i`, `acutecomb`).
pub fn category_for_name(name: &str) -> Category {
    let base = name.split('.').next().unwrap_or(name);
    if base.ends_with("comb") {
        return Category::Mark;
    }
    let first = base.split('_').next().unwrap_or(base);
    if first.chars().count() == 1 {
        if let Some(c) = first.chars().next() {
            return category(c as u32);
        }
    }
    Category::Other
}

/// Whether a base letter commonly takes an ogonek: a, e, i, o, u and their capitals.
pub fn takes_ogonek(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name);
    matches!(
        base,
        "a" | "e" | "i" | "o" | "u" | "A" | "E" | "I" | "O" | "U"
    )
}

/// Whether an Arabic letter joins the letter before it (right-to-left: on its right).
/// Hamza joins neither side; every other letter in the basic range joins on the right.
pub fn arabic_joins_right(cp: u32) -> bool {
    (0x0620..=0x064A).contains(&cp) && cp != 0x0621
}

/// Whether an Arabic letter joins the letter after it (on its left). Alef and its
/// variants, teh marbuta, dal, dhal, ra, zain, waw and alef maksura join only on the right.
pub fn arabic_joins_left(cp: u32) -> bool {
    arabic_joins_right(cp)
        && !matches!(
            cp,
            0x0622
                | 0x0623
                | 0x0624
                | 0x0625
                | 0x0627
                | 0x0629
                | 0x062F
                | 0x0630
                | 0x0631
                | 0x0632
                | 0x0648
                | 0x0649
        )
}

/// Whether a combining mark sits below its base (so it attaches with `_bottom`).
pub fn is_below_mark(name: &str, cp: Option<u32>) -> bool {
    let base = name.split('.').next().unwrap_or(name);
    base.contains("below")
        || base.contains("bottom")
        || matches!(base, "cedillacomb" | "ogonekcomb" | "commaaccentcomb")
        || cp.is_some_and(|c| {
            matches!(
                c,
                0x0326 | 0x0327 | 0x0328 | 0x064D | 0x0650 | 0x0655 | 0x0656
            )
        })
}

/// A named set of code points offered when creating a font.
pub struct GlyphSet {
    pub id: &'static str,
    pub name: &'static str,
    pub ranges: &'static [(u32, u32)],
}

pub const GLYPH_SETS: &[GlyphSet] = &[
    GlyphSet {
        id: "latin-basic",
        name: "Latin Basic (ASCII)",
        ranges: &[(0x20, 0x7E)],
    },
    GlyphSet {
        id: "latin-1",
        name: "Latin-1 Supplement",
        ranges: &[(0xA0, 0xFF)],
    },
    GlyphSet {
        id: "punctuation",
        name: "General Punctuation",
        ranges: &[
            (0x2013, 0x2014),
            (0x2018, 0x2019),
            (0x201C, 0x201D),
            (0x2022, 0x2022),
            (0x2026, 0x2026),
            (0x20AC, 0x20AC),
        ],
    },
    GlyphSet {
        id: "cyrillic",
        name: "Cyrillic Basic",
        ranges: &[(0x0410, 0x044F)],
    },
];

/// Code points of the given glyph sets, de-duplicated, in set order.
pub fn codepoints_for(set_ids: &[&str]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for set in GLYPH_SETS.iter().filter(|s| set_ids.contains(&s.id)) {
        for &(a, b) in set.ranges {
            for cp in a..=b {
                if !out.contains(&cp) && char::from_u32(cp).is_some_and(|c| !c.is_control()) {
                    out.push(cp);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(name_for(0x41), "A");
        assert_eq!(name_for(0x30), "zero");
        assert_eq!(name_for(0x416), "uni0416");
        assert_eq!(name_for(0x1F600), "u1F600");
    }

    #[test]
    fn categories() {
        assert_eq!(category('A' as u32), Category::Uppercase);
        assert_eq!(category('ж' as u32), Category::Lowercase);
        assert_eq!(category('7' as u32), Category::Number);
        assert_eq!(category(',' as u32), Category::Punctuation);
        assert_eq!(category('+' as u32), Category::Symbol);
        assert_eq!(category(' ' as u32), Category::Separator);
        assert_eq!(category_for_name("a.sc"), Category::Lowercase);
        assert_eq!(category_for_name("acutecomb"), Category::Mark);
    }

    #[test]
    fn composite_and_extra_names() {
        assert_eq!(codepoint_for_name("Aacute"), Some(0xC1));
        assert_eq!(codepoint_for_name("AE"), Some(0xC6));
        assert_eq!(codepoint_for_name("a.sc"), None);
    }

    #[test]
    fn mark_names_round_trip() {
        for (cp, name) in MARK_NAMES {
            assert_eq!(name_for(*cp), *name);
            assert_eq!(codepoint_for_name(name), Some(*cp));
        }
        // Every mark a composite recipe asks for must be encodable by name.
        for (_, _, parts) in crate::decompose::DECOMPOSITIONS {
            for part in *parts {
                if part.ends_with("comb") {
                    assert!(
                        codepoint_for_name(part).is_some(),
                        "{part} has no code point"
                    );
                }
            }
        }
    }

    #[test]
    fn arabic_data() {
        assert_eq!(category(0x064E), Category::Mark);
        assert_eq!(category(0x0628), Category::Letter);
        assert!(arabic_joins_right(0x0628) && arabic_joins_left(0x0628));
        assert!(arabic_joins_right(0x0627) && !arabic_joins_left(0x0627));
        assert!(!arabic_joins_right(0x0621));
        assert!(is_below_mark("dotbelowcomb", None));
        assert!(is_below_mark("uni0650", Some(0x650)));
        assert!(!is_below_mark("acutecomb", Some(0x301)));
        assert!(takes_ogonek("a") && takes_ogonek("U") && !takes_ogonek("b"));
    }

    #[test]
    fn glyph_sets() {
        let ascii = codepoints_for(&["latin-basic"]);
        assert_eq!(ascii.len(), 95);
        let both = codepoints_for(&["latin-basic", "latin-1"]);
        assert_eq!(both.len(), 95 + 96);
    }
}
