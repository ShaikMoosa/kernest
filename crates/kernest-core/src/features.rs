//! OpenType features: generation from glyph names and checking of hand-written code.
//!
//! Generated code is kept separate from `Font.features` so a feature the designer
//! writes by hand always wins: a tag present in the hand-written text is never
//! generated, and export compiles `generated + "\n" + features`.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::Font;

/// A problem in the hand-written feature text. `line` and `column` are 1-based and
/// relative to the user's text, not to any generated preamble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// Order the generated blocks are emitted in, so the output is stable and readable.
const ORDER: &[&str] = &[
    "liga", "locl", "smcp", "c2sc", "case", "tnum", "onum", "lnum", "sups", "subs", "salt", "ss01",
    "ss02", "ss03", "ss04", "ss05", "ss06", "ss07", "ss08", "ss09", "ss10", "ss11", "ss12", "ss13",
    "ss14", "ss15", "ss16", "ss17", "ss18", "ss19", "ss20", "aalt",
];

/// Tags the designer wrote by hand. Anything here is left out of generation.
pub fn handwritten_tags(font: &Font) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in font.features.lines() {
        let mut words = line.split_whitespace();
        if words.next() == Some("feature") {
            if let Some(tag) = words.next() {
                let tag = tag.trim_end_matches('{');
                if tag.len() == 4 && tag.chars().all(|c| c.is_ascii_alphanumeric()) {
                    out.insert(tag.to_string());
                }
            }
        }
    }
    out
}

/// Substitution lines per feature tag, including the `aalt` aggregate.
fn collect(font: &Font) -> BTreeMap<String, Vec<String>> {
    let mut blocks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut alternates: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let has = |n: &str| font.glyph(n).is_some();
    fn put(blocks: &mut BTreeMap<String, Vec<String>>, tag: &str, base: &str, alt: &str) {
        blocks
            .entry(tag.to_string())
            .or_default()
            .push(format!("  sub {base} by {alt};"));
    }

    for g in &font.glyphs {
        let name = g.name.as_str();
        if let Some((base, suf)) = name.rsplit_once('.') {
            if !has(base) {
                continue;
            }
            match suf {
                "sc" => {
                    // Uppercase small caps are the capital-to-small-cap feature.
                    let tag = if base.chars().next().is_some_and(|c| c.is_uppercase()) {
                        "c2sc"
                    } else {
                        "smcp"
                    };
                    put(&mut blocks, tag, base, name);
                }
                "c2sc" => put(&mut blocks, "c2sc", base, name),
                "case" => put(&mut blocks, "case", base, name),
                "tf" => put(&mut blocks, "tnum", base, name),
                "osf" => put(&mut blocks, "onum", base, name),
                "lf" => put(&mut blocks, "lnum", base, name),
                "sups" => put(&mut blocks, "sups", base, name),
                "subs" => put(&mut blocks, "subs", base, name),
                "alt" => {
                    put(&mut blocks, "salt", base, name);
                    alternates
                        .entry(base.to_string())
                        .or_default()
                        .push(name.to_string());
                }
                s if s.len() == 4
                    && s.starts_with("ss")
                    && s[2..].chars().all(|c| c.is_ascii_digit())
                    && (1..=20).contains(&s[2..].parse::<u8>().unwrap_or(0)) =>
                {
                    put(&mut blocks, s, base, name);
                    alternates
                        .entry(base.to_string())
                        .or_default()
                        .push(name.to_string());
                }
                _ => {}
            }
        } else if name.contains('_') {
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 2 && parts.iter().all(|p| has(p)) {
                blocks
                    .entry("liga".to_string())
                    .or_default()
                    .push(format!("  sub {} by {name};", parts.join(" ")));
            }
        }
    }

    if has("i") && has("idotaccent") {
        blocks.insert(
            "locl".to_string(),
            vec![
                "  script latn;".into(),
                "  language TRK;".into(),
                "  sub i by idotaccent;".into(),
                "  language AZE;".into(),
                "  sub i by idotaccent;".into(),
            ],
        );
    }

    if !alternates.is_empty() {
        let lines: Vec<String> = alternates
            .iter()
            .map(|(base, alts)| format!("  sub {base} from [{}];", alts.join(" ")))
            .collect();
        blocks.insert("aalt".to_string(), lines);
    }

    blocks
}

/// The feature tags Kernest can generate from this font's glyph names, excluding
/// any the designer already wrote by hand. This is the catalogue the UI lists.
pub fn available(font: &Font) -> BTreeSet<String> {
    let blocks = collect(font);
    let hand = handwritten_tags(font);
    blocks
        .keys()
        .filter(|t| !hand.contains(*t))
        .cloned()
        .collect()
}

/// The generated code for the automatic features that are switched on. Tags the
/// designer wrote by hand are never generated, so hand-written code always wins.
pub fn generate(font: &Font) -> String {
    let blocks = collect(font);
    let hand = handwritten_tags(font);
    let mut out = String::new();
    for tag in ORDER {
        if !font.feature_settings.automatic.contains(*tag) || hand.contains(*tag) {
            continue;
        }
        let Some(lines) = blocks.get(*tag) else {
            continue;
        };
        if lines.is_empty() {
            continue;
        }
        out.push_str(&format!("feature {tag} {{\n"));
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
        out.push_str(&format!("}} {tag};\n"));
    }
    if out.is_empty() {
        return out;
    }
    // Without these, features land only under the DFLT script and shapers never
    // apply them to Latin text. They are positional, so when the designer's own
    // code already declares language systems we must not repeat them: a second
    // "languagesystem DFLT dflt;" is a validation error, not a no-op.
    if declares_language_systems(&font.features) {
        out
    } else {
        format!("languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n{out}")
    }
}

/// True when the text has a top-level `languagesystem` statement.
fn declares_language_systems(text: &str) -> bool {
    text.lines()
        .any(|l| l.split_whitespace().next() == Some("languagesystem"))
}

/// The full feature file for export: generated code plus the designer's own.
/// Language systems apply only to feature blocks that follow them, so when the
/// designer's text declares them it comes first (and generation skips its header);
/// otherwise the generated header leads and covers the designer's features too.
pub fn combined(font: &Font) -> String {
    let generated = generate(font);
    if generated.is_empty() {
        return font.features.clone();
    }
    if font.features.trim().is_empty() {
        return generated;
    }
    if declares_language_systems(&font.features) {
        format!("{}\n{generated}", font.features)
    } else {
        format!("{generated}\n{}", font.features)
    }
}

/// Checks the hand-written feature text against the font's glyph order and returns
/// errors with 1-based line/column positions relative to that text.
pub fn check(font: &Font) -> Vec<Diagnostic> {
    let text = font.features.as_str();
    if text.trim().is_empty() {
        return Vec::new();
    }
    // parse_string panics on include statements; report them instead of crashing.
    if let Some(pos) = text.find("include(") {
        let (line, column) = line_col(text, pos);
        return vec![Diagnostic {
            line,
            column,
            message: "include statements are not supported; paste the feature code here".into(),
        }];
    }

    let (tree, parse_errors) = fea_rs::parse::parse_string(text.to_string());
    let mut sets = vec![parse_errors];
    if !sets[0].has_errors() {
        let glyph_map = match fea_rs::GlyphMap::new(font.glyphs.iter().map(|g| g.name.as_str())) {
            Ok(m) => m,
            Err(e) => {
                return vec![Diagnostic {
                    line: 1,
                    column: 1,
                    message: e.to_string(),
                }]
            }
        };
        let validated = fea_rs::compile::validate(
            &tree,
            &glyph_map,
            None::<&fea_rs::compile::NopVariationInfo>,
        );
        if validated.has_errors() {
            sets.push(validated);
        } else {
            let compiled = fea_rs::compile::compile(
                &tree,
                &glyph_map,
                None::<&fea_rs::compile::NopVariationInfo>,
                None::<&fea_rs::compile::NopFeatureProvider>,
                fea_rs::compile::Opts::new(),
            );
            if let Err(errors) = compiled {
                sets.push(errors);
            }
        }
    }

    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for set in &sets {
        for d in set.diagnostics() {
            if !d.is_error() {
                continue;
            }
            let (line, column) = line_col(text, d.span().start);
            let mut message = d.text().to_string();
            // fea-rs messages are generic ("glyph not in font"); name the culprit.
            let span = d.span();
            let snippet = text[span.start.min(text.len())..span.end.min(text.len())].trim();
            if !snippet.is_empty()
                && !snippet.contains('\n')
                && snippet.chars().count() <= 40
                && !message.contains(snippet)
            {
                message = format!("{message}: {snippet}");
            }
            if seen.insert((line, column, message.clone())) {
                out.push(Diagnostic {
                    line,
                    column,
                    message,
                });
            }
        }
    }
    out.sort_by_key(|d| (d.line, d.column));
    out
}

/// 1-based line and column of a byte offset in `text`.
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let prefix = &text[..offset.min(text.len())];
    let line = prefix.matches('\n').count() + 1;
    let column = match prefix.rfind('\n') {
        Some(i) => prefix[i + 1..].chars().count() + 1,
        None => prefix.chars().count() + 1,
    };
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Glyph;

    fn font_with(names: &[&str]) -> Font {
        let mut f = Font::new("Test", std::iter::empty());
        for n in names {
            f.glyphs.push(Glyph::new(n.to_string(), vec![]));
        }
        f
    }

    fn enable_all(f: &mut Font) {
        f.feature_settings.automatic = available(f);
    }

    #[test]
    fn suffix_rules_generate_their_features() {
        let mut f = font_with(&[
            "a",
            "a.ss01",
            "a.alt",
            "a.sc",
            "A",
            "A.sc",
            "f",
            "i",
            "l",
            "f_i",
            "f_l",
            "one",
            "one.tf",
            "one.osf",
            "one.lf",
            "a.sups",
            "a.subs",
            "parenleft",
            "parenleft.case",
            "idotaccent",
        ]);
        enable_all(&mut f);
        let code = generate(&f);
        // Features must be registered for Latin text, not only the DFLT script.
        assert!(code.starts_with("languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n"));
        for expected in [
            "feature ss01 {",
            "  sub a by a.ss01;",
            "feature smcp {",
            "  sub a by a.sc;",
            "feature c2sc {",
            "  sub A by A.sc;",
            "feature liga {",
            "  sub f i by f_i;",
            "  sub f l by f_l;",
            "feature tnum {",
            "  sub one by one.tf;",
            "feature onum {",
            "  sub one by one.osf;",
            "feature lnum {",
            "  sub one by one.lf;",
            "feature sups {",
            "  sub a by a.sups;",
            "feature subs {",
            "  sub a by a.subs;",
            "feature salt {",
            "  sub a by a.alt;",
            "feature case {",
            "  sub parenleft by parenleft.case;",
            "feature locl {",
            "  language TRK;",
            "  sub i by idotaccent;",
            "feature aalt {",
            "  sub a from [a.ss01 a.alt];",
        ] {
            assert!(code.contains(expected), "missing {expected:?} in:\n{code}");
        }
    }

    #[test]
    fn hand_written_feature_is_not_generated() {
        let mut f = font_with(&["a", "a.ss01", "a.alt"]);
        f.features = "feature ss01 {\n  sub a by a.alt;\n} ss01;\n".into();
        enable_all(&mut f);
        assert!(!available(&f).contains("ss01"));
        let code = generate(&f);
        assert!(!code.contains("feature ss01"), "{code}");
        assert!(code.contains("feature salt"), "{code}");
        assert!(code.contains("feature aalt"), "{code}");
    }

    #[test]
    fn switched_off_features_generate_nothing() {
        let f = font_with(&["a", "a.ss01"]);
        assert!(available(&f).contains("ss01"));
        assert_eq!(generate(&f), "");
    }

    #[test]
    fn designers_language_systems_lead_the_combined_file() {
        let mut f = font_with(&["a", "a.ss01"]);
        f.features = "languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n".into();
        enable_all(&mut f);
        // No second header (a duplicate DFLT dflt is a validation error), and the
        // designer's statements stay ahead of every feature block.
        let code = combined(&f);
        assert_eq!(code.matches("languagesystem DFLT dflt;").count(), 1);
        assert!(code.starts_with("languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n"));
        assert!(code.find("feature ss01").unwrap() > code.find("languagesystem latn").unwrap());
    }

    #[test]
    fn generated_header_covers_hand_written_features() {
        let mut f = font_with(&["a", "a.ss01", "b", "b.alt"]);
        f.features = "feature salt {\n  sub b by b.alt;\n} salt;\n".into();
        enable_all(&mut f);
        let code = combined(&f);
        assert!(code.starts_with("languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n"));
        assert!(code.contains("feature ss01") && code.contains("feature salt"));
    }

    #[test]
    fn only_ss01_to_ss20_are_stylistic_sets() {
        let mut f = font_with(&["a", "a.ss01", "a.ss20", "a.ss21", "a.ss00"]);
        enable_all(&mut f);
        let avail = available(&f);
        assert!(avail.contains("ss01") && avail.contains("ss20"));
        assert!(!avail.contains("ss21") && !avail.contains("ss00"));
    }

    #[test]
    fn ligatures_need_every_part() {
        let mut f = font_with(&["f", "f_i"]);
        enable_all(&mut f);
        assert!(!available(&f).contains("liga"));
        f.glyphs.push(Glyph::new("i", vec![]));
        enable_all(&mut f);
        assert!(available(&f).contains("liga"));
    }

    #[test]
    fn locl_needs_both_glyphs() {
        let mut f = font_with(&["i"]);
        enable_all(&mut f);
        assert!(!available(&f).contains("locl"));
        f.glyphs.push(Glyph::new("idotaccent", vec![]));
        enable_all(&mut f);
        assert!(available(&f).contains("locl"));
    }

    #[test]
    fn clean_feature_text_has_no_diagnostics() {
        let mut f = font_with(&["a", "b", "f_i", "f", "i"]);
        f.features = "feature liga {\n  sub f i by f_i;\n} liga;\n".into();
        assert!(check(&f).is_empty(), "{:?}", check(&f));
    }

    #[test]
    fn unknown_glyph_reports_its_line() {
        let mut f = font_with(&["a"]);
        f.features = "feature ss01 {\n\n  sub a by nope;\n} ss01;\n".into();
        let diags = check(&f);
        assert!(!diags.is_empty());
        assert_eq!(diags[0].line, 3, "{diags:?}");
    }

    #[test]
    fn syntax_error_reports_its_line() {
        let mut f = font_with(&["a"]);
        f.features = "feature ss01 {\n  sub a by ;\n} ss01;\n".into();
        let diags = check(&f);
        assert!(diags.iter().any(|d| d.line == 2), "{diags:?}");
    }

    #[test]
    fn include_is_reported_not_panicking() {
        let mut f = font_with(&["a"]);
        f.features = "include(other.fea);\n".into();
        let diags = check(&f);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 1);
    }
}
