//! Glyph order: the working order of `Font.glyphs` is the order the font exports in.
//!
//! Reordering is a whole-font permutation so a drag in Font View and the order text in
//! Font Info share one path. `.notdef` must stay first: it is glyph id 0 in every
//! compiled format, so an order that lists it later is pinned back to the front.

use std::collections::{HashMap, HashSet};

use crate::model::Font;

type R<T> = Result<T, String>;

/// Reorders the glyphs to `order`, which must list every glyph exactly once.
pub fn set_order(font: &Font, order: &[String]) -> R<Font> {
    let mut seen = HashSet::new();
    for name in order {
        if !seen.insert(name.as_str()) {
            return Err(format!("\"{name}\" appears twice in the order."));
        }
        if font.glyph(name).is_none() {
            return Err(format!("There is no glyph named \"{name}\"."));
        }
    }
    for g in &font.glyphs {
        if !seen.contains(g.name.as_str()) {
            return Err(format!("\"{}\" is missing from the order.", g.name));
        }
    }
    let mut names: Vec<&str> = order.iter().map(|s| s.as_str()).collect();
    if let Some(i) = names.iter().position(|n| *n == ".notdef") {
        if i != 0 {
            names.remove(i);
            names.insert(0, ".notdef");
        }
    }
    let pos: HashMap<&str, usize> = names.into_iter().enumerate().map(|(i, n)| (n, i)).collect();
    let mut next = font.clone();
    next.glyphs.sort_by_key(|g| pos[g.name.as_str()]);
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Codepoint, Glyph};
    use crate::sample::sample_font;

    fn names(f: &Font) -> Vec<String> {
        f.glyphs.iter().map(|g| g.name.clone()).collect()
    }

    #[test]
    fn reorders_and_keeps_every_glyph() {
        let f = sample_font();
        let mut order = names(&f);
        order.reverse();
        let out = set_order(&f, &order).unwrap();
        assert_eq!(names(&out), order);
        assert_eq!(out.glyphs.len(), f.glyphs.len());
    }

    #[test]
    fn notdef_stays_first() {
        let mut f = sample_font();
        f.glyphs.push(Glyph::new(".notdef", vec![Codepoint(0)]));
        let mut order = names(&f);
        // Ask for .notdef in the middle; it must come back at the front.
        let i = order.iter().position(|n| n == ".notdef").unwrap();
        order.swap(i, 3);
        let out = set_order(&f, &order).unwrap();
        assert_eq!(out.glyphs[0].name, ".notdef");
        assert_eq!(out.glyphs.len(), f.glyphs.len());
    }

    #[test]
    fn duplicates_unknown_and_missing_names_error() {
        let f = sample_font();
        let mut order = names(&f);
        order[1] = order[0].clone();
        assert!(set_order(&f, &order).unwrap_err().contains("appears twice"));
        let mut order = names(&f);
        order[0] = "nothere".into();
        assert!(set_order(&f, &order)
            .unwrap_err()
            .contains("no glyph named"));
        let order = names(&f)[1..].to_vec();
        assert!(set_order(&f, &order)
            .unwrap_err()
            .contains("missing from the order"));
    }
}
