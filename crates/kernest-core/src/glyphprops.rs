//! Glyph property overrides (Glyphs Handbook 5.3): category, subcategory, case, script
//! and sort name, set on one or more glyphs at once. An empty value goes back to what
//! glyph data says.

use crate::model::{Font, GLYPH_CASES, GLYPH_CATEGORIES};

/// The properties that can be overridden, by their Glyphs key.
pub const PROPERTIES: &[&str] = &["category", "subCategory", "case", "script", "sortName"];

/// The value to store for `property`, or `None` for automatic; an error explains what
/// is wrong with it.
pub fn normalize(property: &str, value: &str) -> Result<Option<String>, String> {
    let v = value.trim();
    if v.is_empty() {
        return Ok(None);
    }
    let ok = match property {
        "category" => GLYPH_CATEGORIES.contains(&v),
        "case" => GLYPH_CASES.contains(&v),
        "subCategory" => {
            v.chars().count() <= 40
                && v.chars()
                    .all(|c| c.is_alphanumeric() || c == ' ' || c == '-')
        }
        "script" => {
            v.len() <= 32
                && v.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        }
        "sortName" => {
            v.chars().count() <= 63 && !v.chars().any(|c| c.is_whitespace() || c.is_control())
        }
        other => return Err(format!("Unknown glyph property \"{other}\".")),
    };
    if !ok {
        return Err(match property {
            "category" => format!(
                "\"{v}\" isn't a category; use one of {}.",
                GLYPH_CATEGORIES.join(", ")
            ),
            "case" => format!(
                "\"{v}\" isn't a case; use one of {}.",
                GLYPH_CASES.join(", ")
            ),
            "subCategory" => format!(
                "\"{v}\" can't be used as a subcategory: use letters and spaces, at most 40."
            ),
            "script" => format!(
                "\"{v}\" can't be used as a script: use letters and digits, like latin or arabic."
            ),
            _ => format!("\"{v}\" can't be used as a sort name: no spaces, at most 63 characters."),
        });
    }
    // Glyphs writes scripts in lower case.
    Ok(Some(if property == "script" {
        v.to_ascii_lowercase()
    } else {
        v.to_string()
    }))
}

/// `font` with `property` set to `value` (empty: automatic) on every glyph in `glyphs`.
pub fn set(font: &Font, glyphs: &[String], property: &str, value: &str) -> Result<Font, String> {
    let v = normalize(property, value)?;
    if glyphs.is_empty() {
        return Err("Select glyphs first.".into());
    }
    let mut next = font.clone();
    for name in glyphs {
        let g = next
            .glyph_mut(name)
            .ok_or_else(|| format!("There is no glyph named \"{name}\"."))?;
        if g.locked {
            return Err(format!("\"{name}\" is locked. Unlock it to edit."));
        }
        let field = match property {
            "category" => &mut g.category_override,
            "subCategory" => &mut g.subcategory_override,
            "case" => &mut g.case_override,
            "script" => &mut g.script_override,
            _ => &mut g.sort_name,
        };
        *field = v.clone();
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyphdata::Category;
    use crate::model::{Codepoint, Glyph};

    fn font() -> Font {
        let mut f = Font::new("T", []);
        let mut a = Glyph::new("A", vec![Codepoint(0x41)]);
        a.layers.clear();
        f.glyphs = vec![a, Glyph::new("dotcomb", vec![]), Glyph::new("f_i", vec![])];
        f
    }

    #[test]
    fn overrides_change_what_the_glyph_reports() {
        let f = font();
        assert_eq!(f.glyph("dotcomb").unwrap().category(), Category::Mark);
        let f = set(&f, &["dotcomb".into()], "category", "Symbol").unwrap();
        assert_eq!(f.glyph("dotcomb").unwrap().category(), Category::Symbol);
        // Case splits letters.
        let f = set(&f, &["A".into()], "case", "lower").unwrap();
        assert_eq!(f.glyph("A").unwrap().category(), Category::Lowercase);
        let f = set(&f, &["A".into()], "case", "smallCaps").unwrap();
        assert_eq!(f.glyph("A").unwrap().category(), Category::Letter);
        // Case doesn't turn a mark into a letter.
        let f = set(&f, &["dotcomb".into()], "category", "Mark").unwrap();
        let f = set(&f, &["dotcomb".into()], "case", "upper").unwrap();
        assert_eq!(f.glyph("dotcomb").unwrap().category(), Category::Mark);
        // Script and subcategory; several glyphs at once; empty goes back to automatic.
        let both = ["A".to_string(), "f_i".to_string()];
        let f = set(&f, &both, "script", "Arabic").unwrap();
        assert_eq!(f.glyph("A").unwrap().script(), "Arabic");
        assert_eq!(
            f.glyph("A").unwrap().script_override.as_deref(),
            Some("arabic")
        );
        assert_eq!(f.glyph("f_i").unwrap().subcategory(), "Ligature");
        let f = set(&f, &both, "subCategory", "Fraction").unwrap();
        assert_eq!(f.glyph("f_i").unwrap().subcategory(), "Fraction");
        let f = set(&f, &both, "script", "  ").unwrap();
        assert_eq!(f.glyph("A").unwrap().script(), "Latin");
        let f = set(&f, &["f_i".into()], "sortName", "f.lig").unwrap();
        assert_eq!(f.glyph("f_i").unwrap().sort_name.as_deref(), Some("f.lig"));
    }

    #[test]
    fn bad_values_are_refused() {
        let f = font();
        assert!(set(&f, &["A".into()], "category", "Letters").is_err());
        assert!(set(&f, &["A".into()], "case", "Upper").is_err());
        assert!(set(&f, &["A".into()], "script", "latin script").is_err());
        assert!(set(&f, &["A".into()], "sortName", "a b").is_err());
        assert!(set(&f, &["A".into()], "colour", "red").is_err());
        assert!(set(&f, &["nope".into()], "category", "Mark").is_err());
        let mut locked = font();
        locked.glyph_mut("A").unwrap().locked = true;
        assert!(set(&locked, &["A".into()], "category", "Mark").is_err());
    }
}
