//! Glyph data: production names, categories, scripts and subcategories for common
//! code points, the built-in glyph sets offered by File › New and Add Glyphs, and
//! the language coverage shown by the Languages sidebar (S34).

use serde::Serialize;

use crate::model::Glyph;

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

/// The script a code point belongs to, for the Languages sidebar and filter rules.
/// Characters shared between scripts (digits, punctuation, spaces, marks) are "Common".
pub fn script(cp: u32) -> &'static str {
    if (0x0370..=0x03FF).contains(&cp) || (0x1F00..=0x1FFF).contains(&cp) {
        return "Greek";
    }
    if (0x0400..=0x052F).contains(&cp)
        || (0x2DE0..=0x2DFF).contains(&cp)
        || (0xA640..=0xA69F).contains(&cp)
    {
        return "Cyrillic";
    }
    if (0x0590..=0x05FF).contains(&cp) || (0xFB1D..=0xFB4F).contains(&cp) {
        return "Hebrew";
    }
    if (0x0600..=0x06FF).contains(&cp)
        || (0x0750..=0x077F).contains(&cp)
        || (0x08A0..=0x08FF).contains(&cp)
        || (0xFB50..=0xFDFF).contains(&cp)
        || (0xFE70..=0xFEFF).contains(&cp)
    {
        return "Arabic";
    }
    let latin_letter =
        (0x00C0..=0x02AF).contains(&cp) && char::from_u32(cp).is_some_and(|c| c.is_alphabetic());
    if (0x0041..=0x005A).contains(&cp)
        || (0x0061..=0x007A).contains(&cp)
        || latin_letter
        || (0x1E00..=0x1EFF).contains(&cp)
        || (0x2C60..=0x2C7F).contains(&cp)
        || (0xA720..=0xA7FF).contains(&cp)
        || (0xFB00..=0xFB06).contains(&cp)
    {
        return "Latin";
    }
    match category(cp) {
        Category::Number
        | Category::Punctuation
        | Category::Separator
        | Category::Mark
        | Category::Symbol => "Common",
        _ => "Other",
    }
}

/// Best-effort script for a glyph name: encoded names (including composites such as
/// `Aacute` and `uni0416`) use their code point; `acutecomb` is Common; `a.sc` is Latin.
pub fn script_for_name(name: &str) -> &'static str {
    if let Some(cp) = codepoint_for_name(name) {
        return script(cp);
    }
    let base = name.split('.').next().unwrap_or(name);
    if base.ends_with("comb") {
        return "Common";
    }
    let first = base.split('_').next().unwrap_or(base);
    match first.chars().next() {
        Some(c) if c.is_alphabetic() => script(c as u32),
        _ => "Other",
    }
}

/// A finer label within a category, for the list view and filter rules: ligatures,
/// fractions, dashes, currency and so on. Empty when the category says it all.
/// The script a glyph belongs to: its first Unicode, else the code point its name
/// stands for, else naming conventions (suffixes keep the base glyph's script).
pub fn script_of(g: &Glyph) -> &'static str {
    match g
        .unicodes
        .first()
        .map(|c| c.0)
        .or_else(|| codepoint_for_name(&g.name))
    {
        Some(cp) => script(cp),
        None => script_for_name(&g.name),
    }
}

pub fn subcategory(cp: Option<u32>, name: &str) -> &'static str {
    let base = name.split('.').next().unwrap_or(name);
    if base.contains('_') || matches!(base, "AE" | "ae" | "OE" | "oe") {
        return "Ligature";
    }
    let Some(cp) = cp else {
        return "";
    };
    if (0xFB00..=0xFB06).contains(&cp) {
        return "Ligature";
    }
    match category(cp) {
        Category::Mark => "Combining",
        Category::Number => {
            if (0x00BC..=0x00BE).contains(&cp) || (0x2150..=0x215E).contains(&cp) {
                "Fraction"
            } else if matches!(cp, 0x00B2 | 0x00B3 | 0x00B9) || (0x2070..=0x2079).contains(&cp) {
                "Superscript"
            } else if (0x2080..=0x2089).contains(&cp) {
                "Subscript"
            } else if (0x30..=0x39).contains(&cp)
                || (0x0660..=0x0669).contains(&cp)
                || (0x06F0..=0x06F9).contains(&cp)
            {
                "Decimal"
            } else {
                "Other"
            }
        }
        Category::Punctuation => {
            if matches!(
                cp,
                0x002D | 0x2010..=0x2015 | 0x2212 | 0xFE58 | 0xFE63 | 0xFF0D
            ) {
                "Dash"
            } else if matches!(
                cp,
                0x0022 | 0x0027 | 0x00AB | 0x00BB | 0x2018..=0x201F | 0x2039 | 0x203A
            ) {
                "Quote"
            } else if matches!(cp, 0x0028 | 0x0029 | 0x207D | 0x207E | 0x208D | 0x208E) {
                "Parenthesis"
            } else if matches!(cp, 0x005B | 0x005D | 0x007B | 0x007D | 0x2045 | 0x2046) {
                "Bracket"
            } else {
                "Other"
            }
        }
        Category::Symbol => {
            if matches!(cp, 0x0024 | 0x00A2..=0x00A5 | 0x20A0..=0x20C0) {
                "Currency"
            } else if matches!(cp, 0x002B | 0x003C..=0x003E | 0x00B1 | 0x00D7 | 0x00F7 | 0x2200..=0x22FF)
            {
                "Math"
            } else if (0x2190..=0x21FF).contains(&cp) {
                "Arrow"
            } else {
                "Other"
            }
        }
        Category::Separator => "Space",
        _ => "",
    }
}

/// Whether a code point is a drawable character: assigned, and not a control or an
/// invisible format character (soft hyphen, zero-width joiners, bidi marks, …).
/// Unassigned code points fail every character property, so they stay out of glyph
/// sets; the explicit ranges catch assigned punctuation and symbols that have no
/// alphabetic, numeric or whitespace property.
pub fn is_drawable(cp: u32) -> bool {
    let Some(c) = char::from_u32(cp) else {
        return false;
    };
    if c.is_control() {
        return false;
    }
    if matches!(cp,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x180E
        | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F
        | 0xFEFF | 0xFFF9..=0xFFFB
    ) {
        return false;
    }
    if c.is_alphabetic() || c.is_numeric() || c.is_whitespace() || c.is_ascii_punctuation() {
        return true;
    }
    matches!(cp,
        0x00A1..=0x00BF | 0x00D7 | 0x00F7
        | 0x0375 | 0x037E | 0x0384..=0x0385 | 0x0387 | 0x03F6
        | 0x0488..=0x0489
        | 0x05BE | 0x05C0 | 0x05C3 | 0x05C6 | 0x05F3..=0x05F4
        | 0x0606..=0x060B | 0x060D..=0x060F | 0x061B | 0x061D..=0x061F
        | 0x066A..=0x066D | 0x06D4 | 0x06DE | 0x06E9 | 0x06FD..=0x06FE
        | 0x2010..=0x2027 | 0x2030..=0x205E
        | 0x2070 | 0x2074..=0x207E | 0x2080..=0x208E
        | 0x20A0..=0x20C0 | 0x2190..=0x21FF | 0x2200..=0x22FF
        | 0x25A0..=0x25FF | 0x2600..=0x26FF
    )
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
    GlyphSet {
        id: "latin-ext-a",
        name: "Latin Extended-A",
        ranges: &[(0x0100, 0x017F)],
    },
    GlyphSet {
        id: "latin-ext-additional",
        name: "Latin Extended Additional",
        ranges: &[(0x1E00, 0x1EFF)],
    },
    GlyphSet {
        id: "greek",
        name: "Greek",
        ranges: &[(0x0370, 0x03FF)],
    },
    GlyphSet {
        id: "cyrillic-extended",
        name: "Cyrillic Extended",
        ranges: &[(0x0400, 0x04FF), (0x0500, 0x052F)],
    },
    GlyphSet {
        id: "arabic",
        name: "Arabic",
        ranges: &[(0x0600, 0x06FF), (0xFB50, 0xFDFF), (0xFE70, 0xFEFF)],
    },
    GlyphSet {
        id: "hebrew",
        name: "Hebrew",
        ranges: &[(0x0590, 0x05FF)],
    },
];

/// A language the Languages sidebar tracks: the glyph sets that cover its script.
pub struct Language {
    pub id: &'static str,
    pub name: &'static str,
    pub set_ids: &'static [&'static str],
}

pub const LANGUAGES: &[Language] = &[
    Language {
        id: "latin",
        name: "Latin",
        set_ids: &[
            "latin-basic",
            "latin-1",
            "latin-ext-a",
            "latin-ext-additional",
        ],
    },
    Language {
        id: "greek",
        name: "Greek",
        set_ids: &["greek"],
    },
    Language {
        id: "cyrillic",
        name: "Cyrillic",
        set_ids: &["cyrillic", "cyrillic-extended"],
    },
    Language {
        id: "arabic",
        name: "Arabic",
        set_ids: &["arabic"],
    },
    Language {
        id: "hebrew",
        name: "Hebrew",
        set_ids: &["hebrew"],
    },
];

/// Code points of the given glyph sets, de-duplicated, in set order.
/// Only drawable characters are included: no controls, invisible format characters
/// or unassigned code points.
pub fn codepoints_for(set_ids: &[&str]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for set in GLYPH_SETS.iter().filter(|s| set_ids.contains(&s.id)) {
        for &(a, b) in set.ranges {
            for cp in a..=b {
                if !out.contains(&cp) && is_drawable(cp) {
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
        // Latin-1 without the soft hyphen, which is invisible and not drawable.
        let both = codepoints_for(&["latin-basic", "latin-1"]);
        assert_eq!(both.len(), 95 + 95);
        assert!(!both.contains(&0xAD));
    }

    #[test]
    fn new_sets_skip_unassigned_and_format_characters() {
        let greek = codepoints_for(&["greek"]);
        assert!(greek.contains(&0x391) && greek.contains(&0x3C9));
        for cp in [0x378, 0x379, 0x380, 0x38B, 0x38D, 0x3A2] {
            assert!(!greek.contains(&cp), "unassigned {cp:04X} in Greek set");
        }
        let hebrew = codepoints_for(&["hebrew"]);
        assert!(hebrew.contains(&0x5D0) && hebrew.contains(&0x5F4));
        for cp in [0x5C8, 0x5EB, 0x5F5] {
            assert!(!hebrew.contains(&cp), "unassigned {cp:04X} in Hebrew set");
        }
        let arabic = codepoints_for(&["arabic"]);
        assert!(arabic.contains(&0x628) && arabic.contains(&0x660));
        assert!(arabic.contains(&0xFB50) && arabic.contains(&0xFE8D));
        // Noncharacters and invisible format characters stay out.
        for cp in [0x600, 0x61C, 0x6DD, 0xFDD0, 0xFDEF, 0xFEFF] {
            assert!(!arabic.contains(&cp), "invisible {cp:04X} in Arabic set");
        }
        let cyr = codepoints_for(&["cyrillic-extended"]);
        assert!(cyr.contains(&0x401) && cyr.contains(&0x490) && cyr.contains(&0x510));
        let ext_a = codepoints_for(&["latin-ext-a"]);
        assert_eq!(ext_a.len(), 128);
        let viet = codepoints_for(&["latin-ext-additional"]);
        assert!(viet.contains(&0x1EA0) && viet.contains(&0x1EF9));
        assert_eq!(viet.len(), 256);
    }

    #[test]
    fn scripts() {
        assert_eq!(script('A' as u32), "Latin");
        assert_eq!(script(0x1EF9), "Latin");
        assert_eq!(script(0x3B1), "Greek");
        assert_eq!(script(0x416), "Cyrillic");
        assert_eq!(script(0x628), "Arabic");
        assert_eq!(script(0xFE8D), "Arabic");
        assert_eq!(script(0x5D0), "Hebrew");
        assert_eq!(script('7' as u32), "Common");
        assert_eq!(script(0x301), "Common");
        assert_eq!(script(0x4E2D), "Other");
        assert_eq!(script_for_name("a.sc"), "Latin");
        assert_eq!(script_for_name("uni0416"), "Cyrillic");
        assert_eq!(script_for_name("Aacute"), "Latin");
        assert_eq!(script_for_name("acutecomb"), "Common");
        assert_eq!(script_for_name("f_i"), "Latin");
    }

    #[test]
    fn subcategories() {
        assert_eq!(subcategory(Some('A' as u32), "A"), "");
        assert_eq!(subcategory(None, "f_i"), "Ligature");
        assert_eq!(subcategory(Some(0xC6), "AE"), "Ligature");
        assert_eq!(subcategory(Some(0xFB01), "uniFB01"), "Ligature");
        assert_eq!(subcategory(Some(0x301), "acutecomb"), "Combining");
        assert_eq!(subcategory(Some('7' as u32), "seven"), "Decimal");
        assert_eq!(subcategory(Some(0x660), "uni0660"), "Decimal");
        assert_eq!(subcategory(Some(0xBD), "onehalf"), "Fraction");
        assert_eq!(subcategory(Some(0xB2), "twosuperior"), "Superscript");
        assert_eq!(subcategory(Some(0x2082), "uni2082"), "Subscript");
        assert_eq!(subcategory(Some(0x2014), "emdash"), "Dash");
        assert_eq!(subcategory(Some(0x201C), "quotedblleft"), "Quote");
        assert_eq!(subcategory(Some('(' as u32), "parenleft"), "Parenthesis");
        assert_eq!(subcategory(Some('[' as u32), "bracketleft"), "Bracket");
        assert_eq!(subcategory(Some('$' as u32), "dollar"), "Currency");
        assert_eq!(subcategory(Some(0x20AC), "Euro"), "Currency");
        assert_eq!(subcategory(Some('+' as u32), "plus"), "Math");
        assert_eq!(subcategory(Some(0x2192), "arrowright"), "Arrow");
        assert_eq!(subcategory(Some(' ' as u32), "space"), "Space");
    }

    #[test]
    fn drawable() {
        assert!(is_drawable('A' as u32) && is_drawable(0x3B1) && is_drawable(0xFE8D));
        assert!(is_drawable(0xA9) && is_drawable(0x20AC) && is_drawable(0x384));
        assert!(!is_drawable(0xAD) && !is_drawable(0x200B) && !is_drawable(0x600));
        assert!(!is_drawable(0x378) && !is_drawable(0x5C8) && !is_drawable(0xFDD0));
        assert!(!is_drawable(0x0) && !is_drawable(0x1F));
    }
}
