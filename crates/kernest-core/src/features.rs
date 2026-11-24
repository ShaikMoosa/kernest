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
/// Composition first, then the positional forms of joining scripts in the order
/// Glyphs writes them, then the rest.
const ORDER: &[&str] = &[
    "ccmp", "isol", "fina", "fin2", "fin3", "medi", "med2", "init", "liga", "locl", "smcp", "c2sc",
    "case", "tnum", "onum", "lnum", "sups", "subs", "salt", "ss01", "ss02", "ss03", "ss04", "ss05",
    "ss06", "ss07", "ss08", "ss09", "ss10", "ss11", "ss12", "ss13", "ss14", "ss15", "ss16", "ss17",
    "ss18", "ss19", "ss20", "rlig", "dlig", "hlig", "pnum", "sinf", "numr", "dnom", "frac", "afrc",
    "ordn", "zero", "titl", "swsh", "cswh", "pcap", "c2pc", "cpsp", "ornm", "hist", "aalt",
];

/// The ten figures, by glyph name.
const FIGURES: &[&str] = &[
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
];

/// Suffixes that substitute the base glyph in the feature of the same name.
const SAME_TAG: &[&str] = &[
    "sinf", "ordn", "titl", "swsh", "cswh", "ornm", "hist", "dlig", "hlig", "rlig", "afrc",
];

/// Suffixes of ligature-like glyphs (`f_f_i.dlig`, `one_fraction_two.afrc`) whose
/// parts are substituted by them.
const LIGATURE_TAGS: &[&str] = &["dlig", "hlig", "rlig", "afrc"];

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

/// The positional features of joining scripts, by glyph-name suffix (Glyphs names
/// them `beh-ar.init`, `alaph-syriac.fin2` and so on).
const POSITIONAL: &[&str] = &["isol", "fina", "fin2", "fin3", "medi", "med2", "init"];

/// The OpenType script tag for a glyph data script name (`None` for scripts with no
/// tag, such as Common: those rules go under the default script).
fn ot_script(script: &str) -> Option<&'static str> {
    Some(match script {
        "Arabic" => "arab",
        "Syriac" => "syrc",
        "Hebrew" => "hebr",
        "Latin" => "latn",
        "Greek" => "grek",
        "Cyrillic" => "cyrl",
        _ => return None,
    })
}

/// Positional substitutions (`sub beh-ar by beh-ar.init;`) per feature tag, grouped
/// under `script …;` statements by the base glyph's script.
fn positional(font: &Font) -> BTreeMap<String, Vec<String>> {
    // tag → script (None = default) → lines
    let mut by_tag: BTreeMap<&str, BTreeMap<Option<&str>, Vec<String>>> = BTreeMap::new();
    for g in font.glyphs.iter().filter(|g| g.export) {
        let Some((base, suf)) = g.name.rsplit_once('.') else {
            continue;
        };
        let Some(tag) = POSITIONAL.iter().find(|t| **t == suf) else {
            continue;
        };
        let Some(b) = font.glyph(base).filter(|b| b.export) else {
            continue;
        };
        let script = ot_script(&b.script());
        by_tag
            .entry(tag)
            .or_default()
            .entry(script)
            .or_default()
            .push(format!("  sub {base} by {};", g.name));
    }
    by_tag
        .into_iter()
        .map(|(tag, scripts)| {
            let mut lines = Vec::new();
            // Rules with no script first: a script statement applies to what follows.
            for (script, subs) in scripts {
                if let Some(s) = script {
                    lines.push(format!("  script {s};"));
                }
                lines.extend(subs);
            }
            (tag.to_string(), lines)
        })
        .collect()
}

/// The scripts the positional features use, so the generated header declares them.
fn positional_scripts(font: &Font) -> BTreeSet<&'static str> {
    font.glyphs
        .iter()
        .filter(|g| g.export)
        .filter_map(|g| {
            let (base, suf) = g.name.rsplit_once('.')?;
            POSITIONAL.contains(&suf).then_some(())?;
            ot_script(&font.glyph(base)?.script())
        })
        .filter(|s| *s != "latn")
        .collect()
}

/// `ccmp`: `i` and `j` become dotless before a top mark, as Glyphs generates it. The
/// marks are the exporting glyphs with a `_top` anchor.
fn ccmp(font: &Font) -> Option<Vec<String>> {
    let pairs: Vec<(&str, &str)> = [("i", "idotless"), ("j", "jdotless")]
        .into_iter()
        .filter(|(a, b)| {
            font.glyph(a).is_some_and(|g| g.export) && font.glyph(b).is_some_and(|g| g.export)
        })
        .collect();
    if pairs.is_empty() {
        return None;
    }
    let marks: Vec<&str> = font
        .glyphs
        .iter()
        .filter(|g| {
            g.export
                && g.layers
                    .iter()
                    .any(|l| l.anchors.iter().any(|a| a.name == "_top"))
        })
        .map(|g| g.name.as_str())
        .collect();
    if marks.is_empty() {
        return None;
    }
    let (from, to): (Vec<&str>, Vec<&str>) = pairs.into_iter().unzip();
    Some(vec![
        format!("  @CombiningTopAccents = [{}];", marks.join(" ")),
        format!(
            "  sub [{}]' @CombiningTopAccents by [{}];",
            from.join(" "),
            to.join(" ")
        ),
    ])
}

/// The exporting glyph with this name, or else the one encoded `cp` (a fraction slash
/// made from its code point is called `uni2044`, not `fraction`).
fn named_or_encoded<'a>(font: &'a Font, name: &str, cp: u32) -> Option<&'a str> {
    font.glyph(name)
        .or_else(|| {
            font.glyphs
                .iter()
                .find(|g| g.unicodes.iter().any(|c| c.0 == cp))
        })
        .filter(|g| g.export)
        .map(|g| g.name.as_str())
}

/// The figure's numerator and denominator glyphs (`one.numr`/`one.dnom`, or the
/// longer `.numerator`/`.denominator` names).
fn numr_dnom<'a>(font: &'a Font, fig: &str) -> Option<(&'a str, &'a str)> {
    let find = |sufs: [&str; 2]| {
        sufs.iter()
            .find_map(|s| font.glyph(&format!("{fig}.{s}")))
            .map(|g| g.name.as_str())
    };
    Some((find(["numr", "numerator"])?, find(["dnom", "denominator"])?))
}

/// `frac`, the Glyphs and Adobe recipe: a slash becomes the fraction slash, every
/// figure becomes a numerator, and figures after the fraction slash (or after a
/// denominator) become denominators. Needs `fraction` and at least one figure with
/// both a numerator and a denominator.
fn frac(font: &Font) -> Option<Vec<String>> {
    let fraction = named_or_encoded(font, "fraction", 0x2044)?;
    let figs: Vec<(&str, &str, &str)> = FIGURES
        .iter()
        .filter(|f| font.glyph(f).is_some())
        .filter_map(|f| numr_dnom(font, f).map(|(n, d)| (*f, n, d)))
        .collect();
    if figs.is_empty() {
        return None;
    }
    let join = |v: Vec<&str>| v.join(" ");
    let base = join(figs.iter().map(|f| f.0).collect());
    let numr = join(figs.iter().map(|f| f.1).collect());
    let dnom = join(figs.iter().map(|f| f.2).collect());
    let mut lines = Vec::new();
    if font.glyph("slash").is_some() {
        lines.extend([
            "  lookup FractionBar {".to_string(),
            format!("    sub slash by {fraction};"),
            "  } FractionBar;".into(),
        ]);
    }
    lines.extend([
        "  lookup Numerators {".to_string(),
        format!("    sub [{base}] by [{numr}];"),
        "  } Numerators;".into(),
        "  lookup Denominators {".into(),
        format!("    sub {fraction} [{numr}]' by [{dnom}];"),
        format!("    sub [{dnom}] [{numr}]' by [{dnom}];"),
        "  } Denominators;".into(),
    ]);
    Some(lines)
}

/// `ordn`'s contextual part: `ordfeminine` and `ordmasculine` after figures, and the
/// numero sign from `N o period`. Suffixed `.ordn` glyphs are added by the suffix rules.
fn ordn(font: &Font) -> Option<Vec<String>> {
    let has = |n: &str| font.glyph(n).is_some();
    let figs: Vec<&str> = FIGURES.iter().copied().filter(|f| has(f)).collect();
    let mut lines = Vec::new();
    if !figs.is_empty() {
        for (ord, cp, letters) in [
            ("ordfeminine", 0xAA, ["A", "a"]),
            ("ordmasculine", 0xBA, ["O", "o"]),
        ] {
            let after: Vec<&str> = letters.into_iter().filter(|l| has(l)).collect();
            if let Some(ord) = named_or_encoded(font, ord, cp).filter(|_| !after.is_empty()) {
                lines.push(format!(
                    "  sub [{}] [{}]' by {ord};",
                    figs.join(" "),
                    after.join(" ")
                ));
            }
        }
    }
    if let Some(numero) = named_or_encoded(font, "numero", 0x2116) {
        if ["N", "o", "period"].iter().all(|n| has(n)) {
            lines.push(format!("  sub N o period by {numero};"));
        }
    }
    (!lines.is_empty()).then_some(lines)
}

/// `cpsp`: extra space around capitals, from the number value named `cpsp` (that much
/// on each side), so it varies like the value does. Without that value there's nothing.
fn cpsp(font: &Font) -> Option<Vec<String>> {
    if !font.info.numbers.contains_key("cpsp") {
        return None;
    }
    let caps: Vec<&str> = font
        .glyphs
        .iter()
        .filter(|g| g.export && g.category() == crate::glyphdata::Category::Uppercase)
        .map(|g| g.name.as_str())
        .collect();
    if caps.is_empty() {
        return None;
    }
    Some(vec![
        format!("  @CapitalSpacing = [{}];", caps.join(" ")),
        "  pos @CapitalSpacing <$cpsp 0 ${cpsp*2} 0>;".into(),
    ])
}

/// Substitution lines per feature tag, including the `aalt` aggregate.
fn collect(font: &Font) -> BTreeMap<String, Vec<String>> {
    let mut blocks: BTreeMap<String, Vec<String>> = positional(font);
    if let Some(lines) = ccmp(font) {
        blocks.insert("ccmp".into(), lines);
    }
    let mut alternates: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let has = |n: &str| font.glyph(n).is_some();
    fn put(blocks: &mut BTreeMap<String, Vec<String>>, tag: &str, base: &str, alt: &str) {
        blocks
            .entry(tag.to_string())
            .or_default()
            .push(format!("  sub {base} by {alt};"));
    }

    fn put_line(blocks: &mut BTreeMap<String, Vec<String>>, tag: &str, line: String) {
        blocks.entry(tag.to_string()).or_default().push(line);
    }

    for g in &font.glyphs {
        let name = g.name.as_str();
        if let Some((base, suf)) = name.rsplit_once('.') {
            // f_f_i.dlig, one_fraction_two.afrc: the parts become the ligature.
            if LIGATURE_TAGS.contains(&suf) && !has(base) && base.contains('_') {
                let parts: Vec<&str> = base.split('_').collect();
                if parts.len() >= 2 && parts.iter().all(|p| has(p)) {
                    put_line(
                        &mut blocks,
                        suf,
                        format!("  sub {} by {name};", parts.join(" ")),
                    );
                }
                continue;
            }
            if !has(base) {
                continue;
            }
            let alt = |s: &str| format!("{base}.{s}");
            match suf {
                "numr" | "numerator" => put(&mut blocks, "numr", base, name),
                "dnom" | "denominator" => put(&mut blocks, "dnom", base, name),
                "zero" if base.starts_with("zero") => put(&mut blocks, "zero", base, name),
                // zero.slash only when there's no zero.zero for the same base.
                "slash" if base.starts_with("zero") && !has(&alt("zero")) => {
                    put(&mut blocks, "zero", base, name)
                }
                "pc" => {
                    let tag = if base.chars().next().is_some_and(|c| c.is_uppercase()) {
                        "c2pc"
                    } else {
                        "pcap"
                    };
                    put(&mut blocks, tag, base, name);
                }
                "pf" | "pnum" => put(&mut blocks, "pnum", base, name),
                // Tabular oldstyle: reached from tabular lining by onum and from
                // proportional oldstyle by tnum; pnum and lnum lead back.
                "tosf" => {
                    if has(&alt("tf")) {
                        put(&mut blocks, "onum", &alt("tf"), name);
                        put(&mut blocks, "lnum", name, &alt("tf"));
                    }
                    if has(&alt("osf")) {
                        put(&mut blocks, "tnum", &alt("osf"), name);
                        put(&mut blocks, "pnum", name, &alt("osf"));
                    }
                }
                s if SAME_TAG.contains(&s) => put(&mut blocks, s, base, name),
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
                // Default figures are proportional when tabular ones are suffixed, so
                // pnum turns tabular back into the default.
                "tf" => {
                    put(&mut blocks, "tnum", base, name);
                    put(&mut blocks, "pnum", name, base);
                }
                // Each figure style leads back to the default style from the other
                // one, when the font has no explicit alternate for that direction.
                "osf" => {
                    put(&mut blocks, "onum", base, name);
                    if !has(&alt("lf")) {
                        put(&mut blocks, "lnum", name, base);
                    }
                }
                "lf" => {
                    put(&mut blocks, "lnum", base, name);
                    if !has(&alt("osf")) {
                        put(&mut blocks, "onum", name, base);
                    }
                }
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

    if let Some(lines) = frac(font) {
        blocks.insert("frac".into(), lines);
    }
    if let Some(lines) = ordn(font) {
        blocks.entry("ordn".into()).or_default().splice(0..0, lines);
    }
    if let Some(lines) = cpsp(font) {
        blocks.insert("cpsp".into(), lines);
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
        format!("{}{out}", language_header(font))
    }
}

/// The language systems Kernest declares when the designer's code doesn't: DFLT and
/// latn, plus the joining scripts the positional features use.
fn language_header(font: &Font) -> String {
    let mut header = String::from("languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n");
    for s in positional_scripts(font) {
        header.push_str(&format!("languagesystem {s} dflt;\n"));
    }
    header
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
        // Hand-written code without language systems would land only under DFLT,
        // and shapers skip it for Latin text once kerning adds latn (Handbook 8.2.7).
        if font.features.trim().is_empty() || declares_language_systems(&font.features) {
            return font.features.clone();
        }
        return format!("{}{}", language_header(font), font.features);
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

/// The feature file for one build, with tokens and conditions expanded (S64).
pub fn combined_for(font: &Font, ctx: &crate::featuretokens::Context) -> Result<String, String> {
    crate::featuretokens::expand(font, &combined(font), ctx)
        .map_err(|d| format!("feature code: {}", d.message))
}

/// Checks the hand-written feature text against the font's glyph order and returns
/// errors with 1-based line/column positions relative to that text.
pub fn check(font: &Font) -> Vec<Diagnostic> {
    if font.features.trim().is_empty() {
        return Vec::new();
    }
    // Tokens and conditions (S64): expanded as for the default master, which keeps
    // every line in place; a variable expansion must work too.
    let expanded = match crate::featuretokens::expand(
        font,
        &font.features,
        &crate::featuretokens::Context::DEFAULT,
    ) {
        Ok(t) => t,
        Err(d) => return vec![d],
    };
    if font.masters.len() > 1 {
        if let Err(d) = crate::featuretokens::expand(
            font,
            &font.features,
            &crate::featuretokens::Context {
                variable: true,
                location: None,
            },
        ) {
            return vec![d];
        }
    }
    let text = expanded.as_str();
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

    /// A font with encoded base glyphs and their positional forms.
    fn joining_font() -> Font {
        let mut f = Font::new("Test", std::iter::empty());
        let enc = |n: &str, cp: u32| Glyph::new(n, vec![crate::model::Codepoint(cp)]);
        f.glyphs.push(enc("beh-ar", 0x0628));
        f.glyphs.push(enc("alef-ar", 0x0627));
        f.glyphs.push(enc("alaph-syriac", 0x0710));
        for n in [
            "beh-ar.init",
            "beh-ar.medi",
            "beh-ar.fina",
            "alef-ar.fina",
            "alaph-syriac.fin2",
            "alaph-syriac.fin3",
            "alaph-syriac.med2",
        ] {
            f.glyphs.push(Glyph::new(n, vec![]));
        }
        f
    }

    #[test]
    fn positional_forms_generate_joining_features() {
        let mut f = joining_font();
        enable_all(&mut f);
        let a = available(&f);
        for tag in ["init", "medi", "fina", "fin2", "fin3", "med2"] {
            assert!(a.contains(tag), "{tag} in {a:?}");
        }
        assert!(!a.contains("isol"), "no .isol glyphs");
        let out = generate(&f);
        assert!(out.contains("languagesystem arab dflt;\n"), "{out}");
        assert!(out.contains("languagesystem syrc dflt;\n"), "{out}");
        assert!(
            out.contains("feature init {\n  script arab;\n  sub beh-ar by beh-ar.init;\n} init;")
        );
        assert!(out.contains(
            "feature fina {\n  script arab;\n  sub beh-ar by beh-ar.fina;\n  sub alef-ar by alef-ar.fina;\n} fina;"
        ));
        assert!(out.contains(
            "feature fin2 {\n  script syrc;\n  sub alaph-syriac by alaph-syriac.fin2;\n} fin2;"
        ));
        // Glyphs' order: fina, fin2, fin3, medi, med2, init.
        let pos = |t: &str| out.find(&format!("feature {t} ")).unwrap();
        assert!(pos("fina") < pos("fin2") && pos("fin3") < pos("medi"));
        assert!(pos("medi") < pos("med2") && pos("med2") < pos("init"));
    }

    #[test]
    fn positional_features_respect_hand_code_switches_and_export() {
        let mut f = joining_font();
        f.glyph_mut("beh-ar.medi").unwrap().export = false;
        f.features = "feature init {\n  sub beh-ar by beh-ar.init;\n} init;\n".into();
        enable_all(&mut f);
        let a = available(&f);
        assert!(!a.contains("init"), "hand-written init wins");
        assert!(!a.contains("medi"), "only a non-exporting .medi");
        f.feature_settings.automatic.remove("fina");
        let out = generate(&f);
        assert!(!out.contains("feature fina"), "switched off");
        assert!(!out.contains("feature init"));
        // The designer's own language systems are left alone.
        f.features = format!("languagesystem DFLT dflt;\n{}", f.features);
        assert!(!generate(&f).contains("languagesystem"));
    }

    #[test]
    fn ccmp_makes_i_and_j_dotless_before_top_marks() {
        let mut f = font_with(&[
            "i",
            "j",
            "idotless",
            "jdotless",
            "acutecomb",
            "dotbelowcomb",
        ]);
        let mut top = crate::model::Layer::new("m01", 0.0);
        top.anchors.push(crate::model::Anchor {
            name: "_top".into(),
            x: 0.0,
            y: 500.0,
        });
        f.glyph_mut("acutecomb").unwrap().layers.push(top);
        enable_all(&mut f);
        let out = generate(&f);
        assert!(
            out.contains("feature ccmp {\n  @CombiningTopAccents = [acutecomb];\n  sub [i j]' @CombiningTopAccents by [idotless jdotless];\n} ccmp;"),
            "{out}"
        );
        // Without dotless forms or top marks there's nothing to do.
        let plain = font_with(&["i", "acutecomb"]);
        assert!(!available(&plain).contains("ccmp"));
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
    fn hand_written_code_alone_gets_language_systems() {
        // Without automatic features, hand-written code still needs latn, or kerning
        // (which the compiler registers under latn) hides it from Latin text.
        let mut f = font_with(&["a", "b"]);
        f.features = "feature kern {\n  pos a b -20;\n} kern;\n".into();
        assert!(generate(&f).is_empty());
        let code = combined(&f);
        assert!(code.starts_with("languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n"));
        assert!(code.ends_with(&f.features));
        // Code that declares its own is left as written; no code stays empty.
        f.features = format!("languagesystem DFLT dflt;\n{}", f.features);
        assert_eq!(combined(&f), f.features);
        f.features.clear();
        assert_eq!(combined(&f), "");
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

    /// The ten figures with numerators and denominators, a slash and a fraction slash.
    fn fraction_font() -> Font {
        let mut names: Vec<String> = vec!["slash".into(), "fraction".into()];
        for f in FIGURES {
            names.push(f.to_string());
            names.push(format!("{f}.numr"));
            names.push(format!("{f}.dnom"));
        }
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        font_with(&refs)
    }

    #[test]
    fn fractions_generate_frac_numr_and_dnom() {
        let mut f = fraction_font();
        enable_all(&mut f);
        let a = available(&f);
        for t in ["frac", "numr", "dnom"] {
            assert!(a.contains(t), "{t} in {a:?}");
        }
        let code = generate(&f);
        for expected in [
            "feature numr {\n  sub zero by zero.numr;",
            "feature dnom {\n  sub zero by zero.dnom;",
            "    sub slash by fraction;",
            "    sub [zero one two three four five six seven eight nine] by [zero.numr one.numr two.numr three.numr four.numr five.numr six.numr seven.numr eight.numr nine.numr];",
            "    sub fraction [zero.numr one.numr two.numr three.numr four.numr five.numr six.numr seven.numr eight.numr nine.numr]' by [zero.dnom one.dnom two.dnom three.dnom four.dnom five.dnom six.dnom seven.dnom eight.dnom nine.dnom];",
            "    sub [zero.dnom one.dnom two.dnom three.dnom four.dnom five.dnom six.dnom seven.dnom eight.dnom nine.dnom] [zero.numr",
        ] {
            assert!(code.contains(expected), "missing {expected:?} in:\n{code}");
        }
        // numr and dnom come before frac, which runs last of the three; zero runs after
        // frac, as in Glyphs, so a zero in a fraction still becomes a numerator.
        let pos = |t: &str| code.find(&format!("feature {t} ")).unwrap_or(usize::MAX);
        assert!(pos("numr") < pos("frac") && pos("dnom") < pos("frac"));
        f.glyphs.push(Glyph::new("zero.zero", vec![]));
        enable_all(&mut f);
        let code = generate(&f);
        let pos = |t: &str| code.find(&format!("feature {t} ")).unwrap();
        assert!(pos("frac") < pos("zero"), "{code}");
    }

    #[test]
    fn partial_figures_and_a_missing_fraction_slash() {
        // Only one and two have both forms; three has a numerator only.
        let mut f = font_with(&[
            "fraction",
            "one",
            "one.numr",
            "one.dnom",
            "two",
            "two.numerator",
            "two.denominator",
            "three",
            "three.numr",
        ]);
        enable_all(&mut f);
        let code = generate(&f);
        assert!(
            code.contains("    sub [one two] by [one.numr two.numerator];"),
            "{code}"
        );
        assert!(!code.contains("sub slash by fraction"), "no slash glyph");
        assert!(
            code.contains("  sub three by three.numr;"),
            "numr maps every pair"
        );
        // Without the fraction slash, numr and dnom are still made but frac isn't.
        f.glyphs.retain(|g| g.name != "fraction");
        enable_all(&mut f);
        let a = available(&f);
        assert!(!a.contains("frac") && a.contains("numr") && a.contains("dnom"));
    }

    #[test]
    fn suffixed_ligatures_and_suffix_features() {
        let mut f = font_with(&[
            "f",
            "i",
            "f_f_i.dlig",
            "c",
            "t",
            "c_t.hlig",
            "x",
            "x_x.rlig",
            "one",
            "fraction",
            "two",
            "one_fraction_two.afrc",
            "a",
            "a.sinf",
            "a.ordn",
            "A",
            "A.titl",
            "A.swsh",
            "A.cswh",
            "bullet",
            "bullet.ornm",
            "s",
            "s.hist",
            "zero",
            "zero.zero",
            "zero.slash",
            "zero.tf",
            "zero.tf.zero",
            "b",
            "b.pc",
            "B",
            "B.pc",
        ]);
        enable_all(&mut f);
        let code = generate(&f);
        for expected in [
            "feature dlig {\n  sub f f i by f_f_i.dlig;\n} dlig;",
            "feature hlig {\n  sub c t by c_t.hlig;\n} hlig;",
            "feature rlig {\n  sub x x by x_x.rlig;\n} rlig;",
            "feature afrc {\n  sub one fraction two by one_fraction_two.afrc;\n} afrc;",
            "feature sinf {\n  sub a by a.sinf;",
            "  sub a by a.ordn;",
            "feature titl {\n  sub A by A.titl;",
            "feature swsh {\n  sub A by A.swsh;",
            "feature cswh {\n  sub A by A.cswh;",
            "feature ornm {\n  sub bullet by bullet.ornm;",
            "feature hist {\n  sub s by s.hist;",
            "  sub zero by zero.zero;",
            "  sub zero.tf by zero.tf.zero;",
            "feature pcap {\n  sub b by b.pc;",
            "feature c2pc {\n  sub B by B.pc;",
        ] {
            assert!(code.contains(expected), "missing {expected:?} in:\n{code}");
        }
        // zero.zero wins over zero.slash for the same figure.
        assert!(!code.contains("zero.slash"), "{code}");
        // A suffixed ligature isn't also a plain liga.
        assert!(!code.contains("feature liga"), "{code}");
    }

    #[test]
    fn zero_slash_is_used_without_zero_zero() {
        let mut f = font_with(&["zero", "zero.slash"]);
        enable_all(&mut f);
        assert!(generate(&f).contains("feature zero {\n  sub zero by zero.slash;\n} zero;"));
    }

    #[test]
    fn figure_styles_combine_like_glyphs() {
        let mut f = font_with(&["one", "one.tf", "one.osf", "one.tosf"]);
        enable_all(&mut f);
        let code = generate(&f);
        for expected in [
            "feature tnum {\n  sub one by one.tf;\n  sub one.osf by one.tosf;\n} tnum;",
            "feature onum {\n  sub one by one.osf;\n  sub one.tf by one.tosf;\n} onum;",
            "feature lnum {\n  sub one.osf by one;\n  sub one.tosf by one.tf;\n} lnum;",
            "feature pnum {\n  sub one.tf by one;\n  sub one.tosf by one.osf;\n} pnum;",
        ] {
            assert!(code.contains(expected), "missing {expected:?} in:\n{code}");
        }
        // Oldstyle default with lining alternates: onum leads back to the default.
        let mut g = font_with(&["one", "one.lf"]);
        enable_all(&mut g);
        let code = generate(&g);
        assert!(
            code.contains("feature lnum {\n  sub one by one.lf;\n} lnum;"),
            "{code}"
        );
        assert!(
            code.contains("feature onum {\n  sub one.lf by one;\n} onum;"),
            "{code}"
        );
        // An explicit proportional alternate.
        let mut h = font_with(&["one", "one.pf"]);
        enable_all(&mut h);
        assert!(generate(&h).contains("feature pnum {\n  sub one by one.pf;\n} pnum;"));
    }

    #[test]
    fn ordinals_after_figures_and_the_numero_sign() {
        let mut f = font_with(&[
            "one",
            "two",
            "a",
            "A",
            "o",
            "N",
            "period",
            "ordfeminine",
            "ordmasculine",
            "numero",
            "a.ordn",
        ]);
        enable_all(&mut f);
        let code = generate(&f);
        assert!(
            code.contains("feature ordn {\n  sub [one two] [A a]' by ordfeminine;\n  sub [one two] [o]' by ordmasculine;\n  sub N o period by numero;\n  sub a by a.ordn;\n} ordn;"),
            "{code}"
        );
    }

    #[test]
    fn capital_spacing_needs_the_cpsp_number_value() {
        let mut f = Font::new("Test", [0x41, 0x42, 0x61]);
        enable_all(&mut f);
        assert!(!available(&f).contains("cpsp"));
        let m = f.masters[0].id.clone();
        f.info
            .numbers
            .insert("cpsp".into(), BTreeMap::from([(m, 5.0)]));
        enable_all(&mut f);
        let code = generate(&f);
        assert!(
            code.contains("feature cpsp {\n  @CapitalSpacing = [A B];\n  pos @CapitalSpacing <$cpsp 0 ${cpsp*2} 0>;\n} cpsp;"),
            "{code}"
        );
        // The token expands to the master's value on export.
        let out = combined_for(&f, &crate::featuretokens::Context::DEFAULT).unwrap();
        assert!(out.contains("pos @CapitalSpacing <5 0 10 0>;"), "{out}");
    }

    #[test]
    fn an_encoded_fraction_slash_is_found_by_code_point() {
        let mut f = Font::new("Test", [0x2044, 0x31]);
        f.glyphs.push(Glyph::new("one.numr", vec![]));
        f.glyphs.push(Glyph::new("one.dnom", vec![]));
        enable_all(&mut f);
        let code = generate(&f);
        assert!(
            code.contains("    sub uni2044 [one.numr]' by [one.dnom];"),
            "{code}"
        );
    }

    #[test]
    fn hand_written_frac_wins() {
        let mut f = fraction_font();
        f.features = "feature frac {\n  sub slash by fraction;\n} frac;\n".into();
        enable_all(&mut f);
        assert!(!available(&f).contains("frac"));
        let code = generate(&f);
        assert!(!code.contains("feature frac"), "{code}");
        assert!(code.contains("feature numr"), "{code}");
    }

    #[test]
    fn every_generated_feature_compiles() {
        let mut f = fraction_font();
        for n in [
            "f",
            "i",
            "f_f_i.dlig",
            "a",
            "A",
            "o",
            "N",
            "period",
            "ordfeminine",
            "ordmasculine",
            "numero",
            "a.ordn",
            "a.sinf",
            "A.titl",
            "A.swsh",
            "zero.zero",
            "one.tf",
            "one.osf",
            "one.tosf",
            "b",
            "b.pc",
            "B",
            "B.pc",
            "one_fraction_two.afrc",
        ] {
            f.glyphs.push(Glyph::new(n, vec![]));
        }
        enable_all(&mut f);
        // check() compiles the designer's text; run the generated code through it.
        f.features = generate(&f);
        f.feature_settings.automatic.clear();
        assert!(check(&f).is_empty(), "{:?}\n{}", check(&f), f.features);
    }
}
