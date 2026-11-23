//! Keeping hand-written feature code in step with glyphs an export removes or
//! renames (SMO-28, Glyphs' Remove Glyphs, Keep Glyphs and Rename Glyphs).
//!
//! - [`prune`] takes removed glyphs out of glyph classes and drops the rules that
//!   still need them, then classes, lookups and features left empty.
//! - [`rename`] renames glyphs wherever the code names them.
//!
//! Generated features don't need this: they are built from the glyphs that remain.

use std::collections::{BTreeMap, HashSet};

use crate::featureedits::{drop_references, scan, skip_one, splice, Item};

fn ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-' | b'\\')
}

/// A name token: a glyph name (with an optional leading `\`), or a class (`@X`).
#[derive(Debug, Clone)]
struct Token {
    start: usize,
    end: usize,
    /// The name without a leading `\` or `@`.
    name: String,
    class: bool,
    /// Bracket group index within the statement, if inside `[ … ]`.
    group: Option<usize>,
}

/// One `;`-terminated statement: its span (from just after the previous `;`, `{`
/// or `}` to the `;`), its name tokens and its bracket groups' spans.
#[derive(Debug, Clone)]
struct Statement {
    start: usize,
    end: usize,
    tokens: Vec<Token>,
    groups: Vec<(usize, usize)>,
}

/// The statements of feature code, skipping comments and strings.
fn statements(text: &str) -> Vec<Statement> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut cur = Statement {
        start: 0,
        end: 0,
        tokens: Vec::new(),
        groups: Vec::new(),
    };
    let mut group_open: Option<usize> = None;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' | b'"' => {
                i = skip_one(b, i);
                continue;
            }
            b'{' | b'}' => {
                cur = Statement {
                    start: i + 1,
                    end: 0,
                    tokens: Vec::new(),
                    groups: Vec::new(),
                };
                group_open = None;
            }
            b';' => {
                cur.end = i + 1;
                out.push(std::mem::replace(
                    &mut cur,
                    Statement {
                        start: i + 1,
                        end: 0,
                        tokens: Vec::new(),
                        groups: Vec::new(),
                    },
                ));
                group_open = None;
            }
            b'[' => group_open = Some(i),
            b']' => {
                if let Some(open) = group_open.take() {
                    cur.groups.push((open, i + 1));
                }
            }
            c if ident_byte(c) || c == b'@' => {
                let start = i;
                let class = c == b'@';
                let mut j = i + 1;
                while j < b.len() && ident_byte(b[j]) {
                    j += 1;
                }
                let raw = &text[start..j];
                let name = raw.trim_start_matches(['@', '\\']).to_string();
                if !name.is_empty() {
                    cur.tokens.push(Token {
                        start,
                        end: j,
                        name,
                        class,
                        group: group_open.map(|_| cur.groups.len()),
                    });
                }
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "sub"
            | "substitute"
            | "by"
            | "from"
            | "pos"
            | "position"
            | "rsub"
            | "reversesub"
            | "ignore"
            | "enum"
            | "enumerate"
            | "lookup"
            | "lookupflag"
            | "markClass"
            | "base"
            | "ligature"
            | "mark"
            | "anchor"
            | "NULL"
            | "script"
            | "language"
            | "languagesystem"
            | "feature"
            | "include"
            | "exclude_dflt"
            | "include_dflt"
            | "required"
            | "useExtension"
            | "subtable"
            | "parameters"
            | "sizemenuname"
            | "featureNames"
            | "name"
            | "cvParameters"
    )
}

/// The class a definition statement (`@X = [ … ];`) defines, if it is one.
fn defined_class(text: &str, st: &Statement) -> Option<String> {
    let first = st.tokens.first()?;
    if !first.class {
        return None;
    }
    text[first.end..st.end]
        .trim_start()
        .starts_with('=')
        .then(|| first.name.clone())
}

/// The mark class a `markClass … @M;` statement adds to.
fn mark_class(st: &Statement) -> Option<String> {
    if st.tokens.first()?.name != "markClass" {
        return None;
    }
    st.tokens
        .iter()
        .rev()
        .find(|t| t.class)
        .map(|t| t.name.clone())
}

/// Removes `gone` glyphs from feature code. Inside `[ … ]` the name is taken out (a
/// `sub [a b] by [a.sc b.sc]` loses both partners); a rule that names a removed glyph
/// outside brackets, or whose group or class becomes empty, is dropped. Classes,
/// mark classes, lookups and features left with nothing are removed too, with the
/// rules that use them.
pub fn prune(text: &str, gone: &HashSet<String>) -> String {
    if gone.is_empty() {
        return text.to_string();
    }
    let mut text = text.to_string();
    // Classes and lookups that disappear make the rules using them go too, which
    // may empty further classes: repeat until nothing changes.
    let mut empty_classes: HashSet<String> = HashSet::new();
    let mut empty_lookups: HashSet<String> = HashSet::new();
    for _ in 0..8 {
        let before = text.clone();
        text = prune_once(&text, gone, &mut empty_classes, &empty_lookups);
        let (t, lookups) = drop_empty_blocks(&text);
        text = t;
        empty_lookups.extend(lookups);
        if text == before {
            break;
        }
    }
    text
}

fn prune_once(
    text: &str,
    gone: &HashSet<String>,
    empty_classes: &mut HashSet<String>,
    empty_lookups: &HashSet<String>,
) -> String {
    let stmts = statements(text);
    let mut edits: Vec<((usize, usize), String)> = Vec::new();
    // Mark classes: how many definitions each keeps.
    let mut mark_defs: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut dead_marks: Vec<usize> = Vec::new();
    for (si, st) in stmts.iter().enumerate() {
        let glyph = |t: &Token| !t.class && !is_keyword(&t.name) && gone.contains(&t.name);
        let uses_lookup = st
            .tokens
            .windows(2)
            .any(|w| w[0].name == "lookup" && !w[0].class && empty_lookups.contains(&w[1].name));
        let uses_empty_class = st.tokens.iter().enumerate().any(|(k, t)| {
            t.class
                && empty_classes.contains(&t.name)
                && !(k == 0 && defined_class(text, st).is_some())
        });
        let bare_gone = st.tokens.iter().any(|t| glyph(t) && t.group.is_none());
        let mut drop = uses_lookup || uses_empty_class || bare_gone;
        // Names to take out of groups. A substitution between two groups of the same
        // length (sub [a b] by [a.sc b.sc]) loses both partners.
        let mut remove: HashSet<usize> = HashSet::new();
        if !drop {
            let in_group = |g: usize| -> Vec<usize> {
                st.tokens
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.group == Some(g))
                    .map(|(k, _)| k)
                    .collect()
            };
            let paired = st.groups.len() == 2
                && st
                    .tokens
                    .iter()
                    .any(|t| matches!(t.name.as_str(), "by" | "from"))
                && in_group(0).len() == in_group(1).len()
                && st.tokens.iter().all(|t| t.group.is_some() || !t.class);
            if paired {
                let (g0, g1) = (in_group(0), in_group(1));
                for (x, y) in g0.iter().zip(&g1) {
                    if glyph(&st.tokens[*x]) || glyph(&st.tokens[*y]) {
                        remove.insert(*x);
                        remove.insert(*y);
                    }
                }
            } else {
                for (k, t) in st.tokens.iter().enumerate() {
                    if t.group.is_some() && glyph(t) {
                        remove.insert(k);
                    }
                }
            }
            // A group left with nothing makes the rule meaningless.
            for g in 0..st.groups.len() {
                let members = in_group(g);
                if !members.is_empty() && members.iter().all(|k| remove.contains(k)) {
                    drop = true;
                }
            }
        }
        if let Some(m) = mark_class(st) {
            let e = mark_defs.entry(m).or_default();
            e.0 += 1;
            if drop {
                e.1 += 1;
                dead_marks.push(si);
            }
        }
        if drop {
            if let Some(c) = defined_class(text, st) {
                empty_classes.insert(c);
            }
            edits.push(((st.start, st.end), String::new()));
        } else {
            for k in remove {
                let t = &st.tokens[k];
                edits.push(((t.start, t.end), String::new()));
            }
        }
    }
    // A mark class that lost every definition is empty.
    for (m, (all, dead)) in mark_defs {
        if all == dead {
            empty_classes.insert(m);
        }
    }
    let _ = dead_marks;
    // Leave the line a dropped statement sat on tidy: keep its leading line break.
    let edits = edits
        .into_iter()
        .map(|((s, e), w)| {
            if w.is_empty() && text[s..e].contains(';') {
                let lead = text[s..e].len() - text[s..e].trim_start().len();
                let lead_ws = &text[s..s + lead];
                let keep = lead_ws.rfind('\n').map_or(0, |p| p + 1);
                ((s + keep.min(lead), e), w)
            } else {
                ((s, e), w)
            }
        })
        .collect();
    splice(text, edits)
}

/// Whether a block body holds any statement (a `;` outside comments and strings).
fn has_statement(body: &str) -> bool {
    let b = body.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' | b'"' => {
                i = skip_one(b, i);
                continue;
            }
            b';' => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Removes feature and lookup blocks left without statements (and `aalt`'s
/// references to such features); returns the code and the lookups removed.
fn drop_empty_blocks(text: &str) -> (String, Vec<String>) {
    let mut out = text.to_string();
    let empty_features: Vec<(String, (usize, usize))> = scan(&out)
        .into_iter()
        .filter_map(|it| match it {
            Item::Feature { tag, span, body } if !has_statement(&out[body.0..body.1]) => {
                Some((tag, span))
            }
            _ => None,
        })
        .collect();
    if !empty_features.is_empty() {
        out = splice(
            &out,
            empty_features
                .iter()
                .map(|(_, s)| (*s, String::new()))
                .collect(),
        );
        for (tag, _) in &empty_features {
            out = drop_references(&out, tag);
        }
    }
    // Lookup blocks, anywhere: `lookup NAME { … } NAME;`.
    let mut lookups = Vec::new();
    let mut edits = Vec::new();
    let b = out.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'#' | b'"' => {
                i = skip_one(b, i);
                continue;
            }
            b'l' if out[i..].starts_with("lookup")
                && (i == 0 || !ident_byte(b[i - 1]))
                && b.get(i + 6).is_some_and(|c| c.is_ascii_whitespace()) =>
            {
                let n0 = i + 6 + out[i + 6..].len() - out[i + 6..].trim_start().len();
                let mut n1 = n0;
                while n1 < b.len() && ident_byte(b[n1]) {
                    n1 += 1;
                }
                let name = &out[n0..n1];
                let after = out[n1..].trim_start();
                let open = out.len() - after.len();
                // `lookup NAME useExtension {` is a block too.
                let brace = after.find('{').filter(|p| !after[..*p].contains(';'));
                if let (false, Some(p)) = (name.is_empty(), brace) {
                    let open = open + p;
                    if let Some(close) = matching_brace(b, open) {
                        let mut end = close + 1;
                        let tail = out[end..].trim_start();
                        if let Some(rest) = tail.strip_prefix(name) {
                            if let Some(rest) = rest.trim_start().strip_prefix(';') {
                                end = out.len() - rest.len();
                            }
                        }
                        if !has_statement(&out[open + 1..close]) {
                            lookups.push(name.to_string());
                            edits.push(((i, end), String::new()));
                        }
                        i = end;
                        continue;
                    }
                }
                i = n1.max(i + 1);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if !edits.is_empty() {
        out = splice(&out, edits);
    }
    (out, lookups)
}

fn matching_brace(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
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

/// Renames glyphs in feature code: every glyph name `map` has a key for becomes its
/// value, at once (so a swap `a`↔`a.alt` works). Classes, keywords, comments and
/// strings are left alone.
pub fn rename(text: &str, map: &BTreeMap<String, String>) -> String {
    if map.is_empty() {
        return text.to_string();
    }
    let mut edits = Vec::new();
    for st in statements(text) {
        for t in &st.tokens {
            if t.class || is_keyword(&t.name) {
                continue;
            }
            if let Some(to) = map.get(&t.name) {
                let escaped = text[t.start..].starts_with('\\');
                edits.push((
                    (t.start, t.end),
                    if escaped {
                        format!("\\{to}")
                    } else {
                        to.clone()
                    },
                ));
            }
        }
    }
    splice(text, edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rules_and_group_members_are_pruned() {
        let code = "@Alts = [ a.ss01 b.ss01 ];\nfeature ss01 {\n  sub a by a.ss01;\n  sub b by b.ss01;\n} ss01;\nfeature smcp {\n  sub [a b c] by [a.sc b.sc c.sc];\n} smcp;\n";
        let out = prune(code, &set(&["a.ss01", "b.sc"]));
        assert!(out.contains("@Alts = [  b.ss01 ];"), "{out}");
        assert!(!out.contains("sub a by a.ss01"), "{out}");
        assert!(out.contains("sub b by b.ss01;"), "{out}");
        // Both partners leave a paired substitution.
        assert!(out.contains("sub [a  c] by [a.sc  c.sc];"), "{out}");
    }

    #[test]
    fn empty_classes_features_and_lookups_go_with_their_users() {
        let code = "@Ss = [ a.ss01 ];\nlookup ALT {\n  sub a by a.ss01;\n} ALT;\nfeature aalt {\nfeature ss01;\nfeature salt;\n} aalt;\nfeature ss01 {\n  sub a by @Ss;\n} ss01;\nfeature salt {\n  sub a' lookup ALT b;\n  sub b by b.alt;\n} salt;\n";
        let out = prune(code, &set(&["a.ss01"]));
        assert!(!out.contains("@Ss"), "{out}");
        assert!(
            !out.contains("feature ss01"),
            "the feature and its aalt reference: {out}"
        );
        assert!(!out.contains("lookup ALT"), "{out}");
        assert!(
            out.contains("feature salt;") && out.contains("sub b by b.alt;"),
            "{out}"
        );
    }

    #[test]
    fn mark_classes_and_comments() {
        let code = "markClass [acutecomb] <anchor 0 500> @TOP;\nmarkClass [gravecomb] <anchor 0 500> @TOP;\nfeature mark {\n  # sub acutecomb is only a comment\n  pos base [a] <anchor 250 500> mark @TOP;\n} mark;\n";
        let out = prune(code, &set(&["acutecomb"]));
        assert!(!out.contains("[acutecomb]"), "{out}");
        assert!(out.contains("# sub acutecomb is only a comment"), "{out}");
        assert!(
            out.contains("pos base [a]"),
            "@TOP still has gravecomb: {out}"
        );
        let out = prune(code, &set(&["acutecomb", "gravecomb"]));
        assert!(!out.contains("pos base"), "@TOP is empty: {out}");
        assert!(!out.contains("feature mark"), "{out}");
    }

    #[test]
    fn names_are_renamed_together() {
        let map: BTreeMap<String, String> = [("a", "a.alt"), ("a.alt", "a")]
            .iter()
            .map(|(x, y)| (x.to_string(), y.to_string()))
            .collect();
        let out = rename(
            "feature ss01 {\n  sub a by a.alt; # a stays\n  sub \\a by @a;\n} ss01;\n",
            &map,
        );
        assert_eq!(
            out,
            "feature ss01 {\n  sub a.alt by a; # a stays\n  sub \\a.alt by @a;\n} ss01;\n"
        );
    }

    #[test]
    fn pruned_code_still_compiles() {
        let mut f = crate::model::Font::new("Test", [0x61, 0x62]);
        for n in ["a.ss01", "b.ss01", "acutecomb"] {
            f.glyphs.push(crate::model::Glyph::new(n, vec![]));
        }
        f.features = "@Alts = [ a.ss01 ];\nmarkClass [acutecomb] <anchor 0 500> @TOP;\nfeature ss01 {\n  sub [a b] by [a.ss01 b.ss01];\n  sub a by @Alts;\n} ss01;\nfeature mark {\n  pos base [a b] <anchor 250 500> mark @TOP;\n} mark;\n".into();
        assert!(
            crate::features::check(&f).is_empty(),
            "{:?}",
            crate::features::check(&f)
        );
        let gone = set(&["a.ss01", "acutecomb"]);
        f.features = prune(&f.features, &gone);
        f.glyphs.retain(|g| !gone.contains(&g.name));
        assert!(
            crate::features::check(&f).is_empty(),
            "{:?}\n{}",
            crate::features::check(&f),
            f.features
        );
        assert!(
            f.features.contains("sub [ b] by [ b.ss01];"),
            "{}",
            f.features
        );
    }
}
