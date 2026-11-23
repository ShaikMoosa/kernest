//! Export-specific feature code (SMO-23): the Glyphs parameters an instance or a
//! variable font uses to change the feature code it's built with.
//!
//! - `Replace Feature`: `tag;` then the new code of that feature.
//! - `Remove Features`: feature tags.
//! - `Replace Prefix`: `name;` then the new code of that prefix.
//! - `Remove Prefixes`: prefix names.
//! - `Replace Class`: `name;` then the class's new glyphs.
//! - `Remove Classes`: class names.
//!
//! They work on the font's whole feature file: the generated features and the
//! designer's own. Prefixes are the `# Prefix: name` sections Glyphs files bring in;
//! classes are top-level `@Name = [ … ];` definitions. Something a parameter names
//! but the code doesn't have gives a warning and is skipped; the export goes on.

use crate::model::{CustomParameter, Font};

/// The parameter names, in the order the parameter editor offers them.
pub const PARAMETERS: &[&str] = &[
    "Replace Feature",
    "Remove Features",
    "Replace Prefix",
    "Remove Prefixes",
    "Replace Class",
    "Remove Classes",
];

/// Whether `name` is one of the feature code parameters.
pub fn is_feature_parameter(name: &str) -> bool {
    PARAMETERS.contains(&name)
}

/// One parsed parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    ReplaceFeature { tag: String, code: String },
    RemoveFeatures(Vec<String>),
    ReplacePrefix { name: String, code: String },
    RemovePrefixes(Vec<String>),
    ReplaceClass { name: String, code: String },
    RemoveClasses(Vec<String>),
}

/// The names a list parameter holds: separated by commas, semicolons or white space.
pub fn list_items(value: &str) -> Vec<String> {
    value
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .map(|s| s.trim().trim_start_matches('@').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `name;` and the code after it, as Glyphs writes the replace parameters; `help`
/// is the message when the value doesn't start that way.
fn name_and_code(value: &str, help: &str) -> Result<(String, String), String> {
    let Some((name, code)) = value.split_once(';') else {
        return Err(help.to_string());
    };
    let name = name.trim().trim_start_matches('@').to_string();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(help.to_string());
    }
    Ok((name, code.trim().to_string()))
}

/// Parses one parameter. `Ok(None)` for parameters that aren't about feature code.
pub fn parse(p: &CustomParameter) -> Result<Option<Edit>, String> {
    let edit = match p.name.as_str() {
        "Replace Feature" => {
            let (tag, code) = name_and_code(
                &p.value,
                "Replace Feature: start with the feature tag and a semicolon, such as \"liga; sub f i by f_i;\". It was ignored.",
            )?;
            if tag.len() > 4 || !tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(format!(
                    "Replace Feature: \"{tag}\" isn't a feature tag; use up to four letters or digits. It was ignored."
                ));
            }
            Edit::ReplaceFeature { tag, code }
        }
        "Replace Prefix" => {
            let (name, code) = name_and_code(
                &p.value,
                "Replace Prefix: start with the prefix name and a semicolon, such as \"Languagesystems; languagesystem DFLT dflt;\". It was ignored.",
            )?;
            Edit::ReplacePrefix { name, code }
        }
        "Replace Class" => {
            let (name, code) = name_and_code(
                &p.value,
                "Replace Class: start with the class name and a semicolon, such as \"Figures; one.osf two.osf\". It was ignored.",
            )?;
            Edit::ReplaceClass { name, code }
        }
        "Remove Features" => Edit::RemoveFeatures(list_items(&p.value)),
        "Remove Prefixes" => Edit::RemovePrefixes(list_items(&p.value)),
        "Remove Classes" => Edit::RemoveClasses(list_items(&p.value)),
        _ => return Ok(None),
    };
    Ok(Some(edit))
}

/// A top-level piece of feature code.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Item {
    /// `feature tag { … } tag;`: the whole span and the span of the code inside.
    Feature {
        tag: String,
        span: (usize, usize),
        body: (usize, usize),
    },
    /// `@Name = [ … ];`: the whole span and the span between the brackets.
    Class {
        name: String,
        span: (usize, usize),
        body: (usize, usize),
    },
    /// A `# Prefix: name` line: where it starts and where the line ends.
    PrefixHeader {
        name: String,
        start: usize,
        eol: usize,
    },
}

/// Skips a comment, string or plain character at `i`; returns the next index.
pub(crate) fn skip_one(b: &[u8], i: usize) -> usize {
    match b[i] {
        b'#' => b[i..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(b.len(), |p| i + p),
        b'"' => b[i + 1..]
            .iter()
            .position(|&c| c == b'"')
            .map_or(b.len(), |p| i + p + 2),
        _ => i + 1,
    }
}

/// The index of the brace closing the one at `open`, skipping comments and strings.
fn matching(b: &[u8], open: usize, close: u8) -> Option<usize> {
    let opener = b[open];
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            c if c == opener => depth += 1,
            c if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'#' | b'"' => {
                i = skip_one(b, i);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn ident_end(b: &[u8], i: usize) -> usize {
    let mut j = i;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'.' | b'-')) {
        j += 1;
    }
    j
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// The top-level feature blocks, class definitions and prefix headers, in order.
pub(crate) fn scan(text: &str) -> Vec<Item> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let at_line_start = |i: usize| {
        text[..i]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .trim()
            .is_empty()
    };
    while i < b.len() {
        match b[i] {
            b'#' => {
                let end = skip_one(b, i);
                if at_line_start(i) {
                    let line = text[i + 1..end].trim_start();
                    if let Some(name) = line.strip_prefix("Prefix:") {
                        out.push(Item::PrefixHeader {
                            name: name.trim().to_string(),
                            start: i,
                            eol: end,
                        });
                    }
                }
                i = end;
            }
            b'"' => i = skip_one(b, i),
            b'{' => {
                // A block that isn't a feature (a lookup, a table): skip it whole.
                i = matching(b, i, b'}').map_or(b.len(), |c| c + 1);
            }
            b'f' if text[i..].starts_with("feature")
                && (i == 0 || !b[i - 1].is_ascii_alphanumeric() && b[i - 1] != b'_') =>
            {
                let t0 = skip_ws(b, i + "feature".len());
                let t1 = ident_end(b, t0);
                let open = skip_ws(b, t1);
                if t1 == t0 || t0 == i + "feature".len() || open >= b.len() || b[open] != b'{' {
                    i += 1;
                    continue;
                }
                let tag = text[t0..t1].to_string();
                let Some(close) = matching(b, open, b'}') else {
                    break;
                };
                // The trailing ` tag;`.
                let mut end = close + 1;
                let after = skip_ws(b, end);
                if text[after..].starts_with(&tag) {
                    let semi = skip_ws(b, after + tag.len());
                    if semi < b.len() && b[semi] == b';' {
                        end = semi + 1;
                    }
                }
                out.push(Item::Feature {
                    tag,
                    span: (i, end),
                    body: (open + 1, close),
                });
                i = end;
            }
            b'@' => {
                let n1 = ident_end(b, i + 1);
                let eq = skip_ws(b, n1);
                let open = skip_ws(b, eq + 1);
                if n1 > i + 1 && eq < b.len() && b[eq] == b'=' && open < b.len() && b[open] == b'['
                {
                    if let Some(close) = matching(b, open, b']') {
                        let semi = skip_ws(b, close + 1);
                        let end = if semi < b.len() && b[semi] == b';' {
                            semi + 1
                        } else {
                            close + 1
                        };
                        out.push(Item::Class {
                            name: text[i + 1..n1].to_string(),
                            span: (i, end),
                            body: (open + 1, close),
                        });
                        i = end;
                        continue;
                    }
                }
                i = n1.max(i + 1);
            }
            _ => i += 1,
        }
    }
    out
}

/// Where the prefix whose header starts at `start` ends: at the next prefix header
/// or feature block, or the end of the text.
fn prefix_end(items: &[Item], start: usize, len: usize) -> usize {
    items
        .iter()
        .filter_map(|it| match it {
            Item::PrefixHeader { start: s, .. } if *s > start => Some(*s),
            Item::Feature { span, .. } if span.0 > start => Some(span.0),
            _ => None,
        })
        .min()
        .unwrap_or(len)
}

/// Replaces byte ranges in `text` (non-overlapping, any order).
pub(crate) fn splice(text: &str, mut edits: Vec<((usize, usize), String)>) -> String {
    edits.sort_by_key(|((s, _), _)| std::cmp::Reverse(*s));
    let mut out = text.to_string();
    for ((s, e), with) in edits {
        out.replace_range(s..e, &with);
    }
    out
}

/// `feature tag;` references (in `aalt`) to a removed feature.
pub(crate) fn drop_references(text: &str, tag: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        let is_ref = t
            .strip_prefix("feature")
            .map(str::trim_start)
            .and_then(|r| r.strip_prefix(tag))
            .is_some_and(|r| r.trim() == ";");
        if !is_ref {
            out.push_str(line);
        }
    }
    out
}

/// Applies one edit; a warning when the code doesn't have what it names.
fn apply_one(text: &str, edit: &Edit, warnings: &mut Vec<String>) -> String {
    let items = scan(text);
    match edit {
        Edit::ReplaceFeature { tag, code } => {
            let blocks: Vec<(usize, usize)> = items
                .iter()
                .filter_map(|it| match it {
                    Item::Feature { tag: t, span, .. } if t == tag => Some(*span),
                    _ => None,
                })
                .collect();
            let Some(first) = blocks.first() else {
                warnings.push(format!(
                    "Replace Feature: there is no {tag} feature; it was added."
                ));
                return format!("{}\nfeature {tag} {{\n{code}\n}} {tag};\n", text.trim_end());
            };
            // The first block takes the new code; any further blocks of that tag go.
            let mut edits = vec![(*first, format!("feature {tag} {{\n{code}\n}} {tag};"))];
            edits.extend(blocks[1..].iter().map(|s| (*s, String::new())));
            splice(text, edits)
        }
        Edit::RemoveFeatures(tags) => {
            let mut out = text.to_string();
            for tag in tags {
                let items = scan(&out);
                let spans: Vec<((usize, usize), String)> = items
                    .iter()
                    .filter_map(|it| match it {
                        Item::Feature { tag: t, span, .. } if t == tag => {
                            Some((*span, String::new()))
                        }
                        _ => None,
                    })
                    .collect();
                if spans.is_empty() {
                    warnings.push(format!(
                        "Remove Features: there is no {tag} feature; it was ignored."
                    ));
                    continue;
                }
                out = drop_references(&splice(&out, spans), tag);
            }
            out
        }
        Edit::ReplacePrefix { name, code } => {
            let Some((start, eol)) = items.iter().find_map(|it| match it {
                Item::PrefixHeader {
                    name: n,
                    start,
                    eol,
                } if n == name => Some((*start, *eol)),
                _ => None,
            }) else {
                warnings.push(format!(
                    "Replace Prefix: there is no prefix named {name}; it was ignored."
                ));
                return text.to_string();
            };
            let end = prefix_end(&items, start, text.len());
            splice(text, vec![((eol, end), format!("\n{code}\n\n"))])
        }
        Edit::RemovePrefixes(names) => {
            let mut edits = Vec::new();
            for name in names {
                let found: Vec<usize> = items
                    .iter()
                    .filter_map(|it| match it {
                        Item::PrefixHeader { name: n, start, .. } if n == name => Some(*start),
                        _ => None,
                    })
                    .collect();
                if found.is_empty() {
                    warnings.push(format!(
                        "Remove Prefixes: there is no prefix named {name}; it was ignored."
                    ));
                }
                for start in found {
                    edits.push((
                        (start, prefix_end(&items, start, text.len())),
                        String::new(),
                    ));
                }
            }
            splice(text, edits)
        }
        Edit::ReplaceClass { name, code } => {
            let bodies: Vec<((usize, usize), String)> = items
                .iter()
                .filter_map(|it| match it {
                    Item::Class { name: n, body, .. } if n == name => {
                        Some((*body, format!(" {code} ")))
                    }
                    _ => None,
                })
                .collect();
            if bodies.is_empty() {
                warnings.push(format!(
                    "Replace Class: there is no class named @{name}; it was ignored."
                ));
                return text.to_string();
            }
            splice(text, bodies)
        }
        Edit::RemoveClasses(names) => {
            let mut edits = Vec::new();
            for name in names {
                let spans: Vec<(usize, usize)> = items
                    .iter()
                    .filter_map(|it| match it {
                        Item::Class { name: n, span, .. } if n == name => Some(*span),
                        _ => None,
                    })
                    .collect();
                if spans.is_empty() {
                    warnings.push(format!(
                        "Remove Classes: there is no class named @{name}; it was ignored."
                    ));
                }
                edits.extend(spans.into_iter().map(|s| (s, String::new())));
            }
            splice(text, edits)
        }
    }
}

/// The feature code with the parameters applied in order, and warnings for what
/// they name but the code doesn't have (or parameters that can't be read).
/// Switched-off parameters are skipped.
pub fn apply(text: &str, params: &[CustomParameter]) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let mut warnings = Vec::new();
    for p in params.iter().filter(|p| !p.disabled) {
        match parse(p) {
            Ok(Some(edit)) => out = apply_one(&out, &edit, &mut warnings),
            Ok(None) => {}
            Err(e) => warnings.push(e),
        }
    }
    (out, warnings)
}

/// `font` with its feature code changed by `params`, for one export: the generated
/// and hand-written code become one hand-written text (so a parameter can remove or
/// replace a generated feature too) and generation is switched off. Unchanged when
/// there are no parameters.
pub fn for_export(font: &Font, params: &[CustomParameter]) -> (Font, Vec<String>) {
    let mut out = font.clone();
    if params.iter().all(|p| p.disabled) {
        return (out, Vec::new());
    }
    let (text, warnings) = apply(&crate::features::combined(font), params);
    out.features = text;
    out.feature_settings.automatic.clear();
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, value: &str) -> CustomParameter {
        CustomParameter {
            name: name.into(),
            value: value.into(),
            disabled: false,
        }
    }

    const CODE: &str = "@Figures = [ zero one two\n];\n\n# Prefix: Languagesystems\nlanguagesystem DFLT dflt;\nlanguagesystem latn dflt;\n\n# Prefix: Lookups\nlookup TEST {\n  sub a by a.alt;\n} TEST;\n\nfeature aalt {\nfeature liga;\nfeature salt;\n} aalt;\n\nfeature liga {\n# automatic\nsub f i by f_i; # a {brace} in a comment\n} liga;\n\nfeature salt {\nsub a by a.alt;\n} salt;\n";

    #[test]
    fn scan_finds_classes_prefixes_and_features() {
        let items = scan(CODE);
        let names: Vec<String> = items
            .iter()
            .map(|it| match it {
                Item::Feature { tag, .. } => format!("feature {tag}"),
                Item::Class { name, .. } => format!("class {name}"),
                Item::PrefixHeader { name, .. } => format!("prefix {name}"),
            })
            .collect();
        assert_eq!(
            names,
            [
                "class Figures",
                "prefix Languagesystems",
                "prefix Lookups",
                "feature aalt",
                "feature liga",
                "feature salt"
            ]
        );
    }

    #[test]
    fn remove_features_drops_the_block_and_aalt_references() {
        let (out, w) = apply(CODE, &[p("Remove Features", "liga")]);
        assert!(w.is_empty(), "{w:?}");
        assert!(!out.contains("feature liga {"), "{out}");
        assert!(!out.contains("feature liga;"), "{out}");
        assert!(out.contains("feature salt;") && out.contains("feature salt {"));
    }

    #[test]
    fn replace_feature_swaps_its_code() {
        let (out, w) = apply(CODE, &[p("Replace Feature", "liga;\nsub f i by f_i.alt;")]);
        assert!(w.is_empty(), "{w:?}");
        assert!(
            out.contains("feature liga {\nsub f i by f_i.alt;\n} liga;"),
            "{out}"
        );
        assert!(!out.contains("sub f i by f_i;"), "{out}");
    }

    #[test]
    fn prefixes_are_replaced_and_removed_by_name() {
        let (out, w) = apply(
            CODE,
            &[p(
                "Replace Prefix",
                "Lookups;\nlookup TEST {\n  sub b by b.alt;\n} TEST;",
            )],
        );
        assert!(w.is_empty(), "{w:?}");
        assert!(
            out.contains("# Prefix: Lookups\nlookup TEST {\n  sub b by b.alt;\n} TEST;"),
            "{out}"
        );
        assert!(!out.contains("sub a by a.alt;\n} TEST"), "{out}");
        assert!(out.contains("languagesystem latn dflt;"));
        let (out, _) = apply(CODE, &[p("Remove Prefixes", "Lookups")]);
        assert!(
            !out.contains("lookup TEST") && !out.contains("# Prefix: Lookups"),
            "{out}"
        );
        assert!(out.contains("# Prefix: Languagesystems") && out.contains("feature aalt"));
    }

    #[test]
    fn classes_are_replaced_and_removed() {
        let (out, w) = apply(CODE, &[p("Replace Class", "@Figures; one.osf two.osf")]);
        assert!(w.is_empty(), "{w:?}");
        assert!(out.starts_with("@Figures = [ one.osf two.osf ];"), "{out}");
        let (out, _) = apply(CODE, &[p("Remove Classes", "Figures")]);
        assert!(!out.contains("@Figures"), "{out}");
    }

    #[test]
    fn unknown_names_warn_and_the_rest_still_applies() {
        let (out, w) = apply(
            CODE,
            &[
                p("Remove Features", "dlig, salt"),
                p("Remove Prefixes", "Nope"),
                p("Replace Class", "Caps; A B"),
            ],
        );
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(w[0].contains("no dlig feature"), "{w:?}");
        assert!(!out.contains("feature salt {"), "salt still removed: {out}");
        // A malformed value is reported, not applied.
        let (_, w) = apply(CODE, &[p("Replace Feature", "no semicolon here")]);
        assert!(
            w[0].starts_with("Replace Feature: start with the feature tag"),
            "{w:?}"
        );
        // Switched-off parameters do nothing.
        let mut off = p("Remove Features", "liga");
        off.disabled = true;
        assert_eq!(apply(CODE, &[off]).0, CODE);
    }

    #[test]
    fn replacing_a_missing_feature_adds_it() {
        let (out, w) = apply(CODE, &[p("Replace Feature", "ss01; sub a by a.alt;")]);
        assert!(
            out.ends_with("feature ss01 {\nsub a by a.alt;\n} ss01;\n"),
            "{out}"
        );
        assert!(w[0].contains("it was added"), "{w:?}");
    }

    #[test]
    fn generated_features_can_be_removed_for_one_export() {
        let mut f = Font::new("Test", [0x61, 0x66, 0x69]);
        for n in ["a.ss01", "f_i"] {
            f.glyphs.push(crate::model::Glyph::new(n, vec![]));
        }
        f.feature_settings.automatic = crate::features::available(&f);
        let (g, w) = for_export(&f, &[p("Remove Features", "liga")]);
        assert!(w.is_empty(), "{w:?}");
        let code = crate::features::combined(&g);
        assert!(!code.contains("feature liga"), "{code}");
        assert!(code.contains("feature ss01"), "{code}");
        assert_eq!(
            code.matches("languagesystem DFLT dflt;").count(),
            1,
            "{code}"
        );
        // Nothing to apply: the font is left alone.
        assert_eq!(for_export(&f, &[]).0, f);
    }
}
