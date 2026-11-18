//! Importing hand-written feature code from a `.fea` file.
//!
//! The merge works per feature tag: an incoming `feature tag { … } tag;` block
//! replaces the current block for that tag, new tags are appended, and everything
//! outside feature blocks (language systems, lookups, comments) is kept. Repeated
//! `languagesystem` lines are dropped, since declaring one twice is a compile error.

use std::collections::BTreeSet;

/// The result of merging a `.fea` file into the current feature text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureMerge {
    /// The merged feature text, ready to store in `Font.features`.
    pub text: String,
    /// Tags the file adds that the font didn't have.
    pub added: Vec<String>,
    /// Tags the file replaces because the font already had them.
    pub replaced: Vec<String>,
}

struct Block {
    tag: String,
    text: String,
}

/// Merges `incoming` feature code into `current`, reporting what is new and what
/// is replaced. Errors when the incoming text is empty or has an unclosed block.
pub fn merge_features(current: &str, incoming: &str) -> Result<FeatureMerge, String> {
    if incoming.contains("include(") {
        return Err("include statements are not supported; paste the feature code here".into());
    }
    let (cur_pre, cur_blocks) = split(current)?;
    let (inc_pre, inc_blocks) = split(incoming)?;
    if inc_blocks.is_empty() && !has_code(&inc_pre) {
        return Err("the file has no feature code to import.".into());
    }

    let cur_tags: BTreeSet<&str> = cur_blocks.iter().map(|b| b.tag.as_str()).collect();
    let inc_tags: BTreeSet<&str> = inc_blocks.iter().map(|b| b.tag.as_str()).collect();
    let added: Vec<String> = inc_tags
        .difference(&cur_tags)
        .map(|t| t.to_string())
        .collect();
    let replaced: Vec<String> = inc_tags
        .intersection(&cur_tags)
        .map(|t| t.to_string())
        .collect();

    // Current blocks keep their order; a replaced tag takes the incoming blocks at
    // the position of its first current block. New tags go at the end.
    let mut merged: Vec<&Block> = Vec::new();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    for blk in &cur_blocks {
        if inc_tags.contains(blk.tag.as_str()) {
            if done.insert(blk.tag.as_str()) {
                merged.extend(inc_blocks.iter().filter(|b| b.tag == blk.tag));
            }
        } else {
            merged.push(blk);
        }
    }
    for blk in &inc_blocks {
        if !cur_tags.contains(blk.tag.as_str()) && done.insert(blk.tag.as_str()) {
            merged.extend(inc_blocks.iter().filter(|b| b.tag == blk.tag));
        }
    }

    let mut text = String::new();
    let pre = merge_preamble(&cur_pre, &inc_pre);
    if !pre.trim().is_empty() {
        text.push_str(pre.trim_end());
        text.push('\n');
    }
    for blk in merged {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(blk.text.trim_end());
        text.push('\n');
    }
    Ok(FeatureMerge {
        text,
        added,
        replaced,
    })
}

/// Keeps the current preamble, then the incoming lines, dropping `languagesystem`
/// statements the current text already declares (a duplicate is a compile error).
fn merge_preamble(current: &str, incoming: &str) -> String {
    let declared: BTreeSet<String> = current
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("languagesystem"))
        .map(str::to_string)
        .collect();
    let mut out = current.trim_end().to_string();
    for line in incoming.lines() {
        let t = line.trim();
        if t.starts_with("languagesystem") && declared.contains(t) {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// True when the text has a line that is neither blank nor a comment.
fn has_code(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .any(|l| !l.is_empty() && !l.starts_with('#'))
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// True when `kw` starts at `i` as a whole word.
fn keyword_at(text: &str, i: usize, kw: &str) -> bool {
    if !text[i..].starts_with(kw) {
        return false;
    }
    let before = i == 0 || !is_word_char(text[..i].chars().next_back().unwrap());
    let after = i + kw.len();
    let after_ok = after >= text.len() || !is_word_char(text[after..].chars().next().unwrap());
    before && after_ok
}

fn skip_ws(text: &str, mut i: usize) -> usize {
    let b = text.as_bytes();
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// The end of a `#` comment (the newline stays outside).
fn comment_end(text: &str, i: usize) -> usize {
    text[i..].find('\n').map(|p| i + p).unwrap_or(text.len())
}

/// The end of a quoted string, just past the closing quote.
fn string_end(text: &str, i: usize) -> usize {
    let b = text.as_bytes();
    let mut j = i + 1;
    while j < b.len() {
        if b[j] == b'\\' {
            j += 2;
            continue;
        }
        if b[j] == b'"' {
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

/// Splits feature text into the preamble (everything outside feature blocks) and
/// the `feature tag { … } tag;` blocks, in order. Brace depth is tracked so nested
/// `lookup` blocks stay inside their feature; comments and strings are skipped.
fn split(text: &str) -> Result<(String, Vec<Block>), String> {
    let b = text.as_bytes();
    let mut preamble = String::new();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' {
            let end = comment_end(text, i);
            preamble.push_str(&text[i..end]);
            i = end;
            continue;
        }
        if b[i] == b'"' {
            let end = string_end(text, i);
            preamble.push_str(&text[i..end]);
            i = end;
            continue;
        }
        if keyword_at(text, i, "feature") {
            if let Some((block, end)) = parse_block(text, i)? {
                blocks.push(block);
                i = end;
                continue;
            }
        }
        let len = text[i..].chars().next().unwrap().len_utf8();
        preamble.push_str(&text[i..i + len]);
        i += len;
    }
    Ok((preamble, blocks))
}

/// Reads one feature block starting at the `feature` keyword. Returns `None` when
/// the text isn't a block opener (e.g. the word "feature" in a name), so the caller
/// keeps it as ordinary text.
fn parse_block(text: &str, start: usize) -> Result<Option<(Block, usize)>, String> {
    let b = text.as_bytes();
    let mut i = skip_ws(text, start + "feature".len());
    let tag_start = i;
    while i < b.len() && b[i].is_ascii_alphanumeric() {
        i += 1;
    }
    let tag = &text[tag_start..i];
    if tag.is_empty() {
        return Ok(None);
    }
    i = skip_ws(text, i);
    if i >= b.len() || b[i] != b'{' {
        return Ok(None);
    }
    let tag = tag.to_string();

    let mut depth = 1;
    i += 1;
    while i < b.len() && depth > 0 {
        match b[i] {
            b'#' => i = comment_end(text, i),
            b'"' => i = string_end(text, i),
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    if depth > 0 {
        return Err(format!(
            "the feature block for \"{tag}\" is missing its closing \"}}\"."
        ));
    }
    i = skip_ws(text, i);
    let close_start = i;
    while i < b.len() && is_word_char(text[i..].chars().next().unwrap()) {
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    let close_tag = &text[close_start..i];
    i = skip_ws(text, i);
    if close_tag != tag || i >= b.len() || b[i] != b';' {
        return Err(format!(
            "the feature block for \"{tag}\" must end with \"}} {tag};\"."
        ));
    }
    i += 1;
    Ok(Some((
        Block {
            tag,
            text: text[start..i].to_string(),
        },
        i,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIGA: &str = "feature liga {\n  sub f i by f_i;\n} liga;\n";
    const SS01: &str = "feature ss01 {\n  sub a by a.ss01;\n} ss01;\n";

    #[test]
    fn adds_a_new_feature() {
        let m = merge_features(LIGA, SS01).unwrap();
        assert_eq!(m.added, vec!["ss01"]);
        assert!(m.replaced.is_empty());
        assert!(m.text.contains("sub f i by f_i;"));
        assert!(m.text.contains("sub a by a.ss01;"));
    }

    #[test]
    fn replaces_an_existing_feature() {
        let new_liga = "feature liga {\n  sub f i by f_i.liga;\n} liga;\n";
        let m = merge_features(LIGA, new_liga).unwrap();
        assert!(m.added.is_empty());
        assert_eq!(m.replaced, vec!["liga"]);
        assert!(m.text.contains("f_i.liga"));
        assert!(!m.text.contains("by f_i;"));
    }

    #[test]
    fn nested_lookup_braces_stay_in_the_block() {
        let incoming = "feature salt {\n  lookup S {\n    sub a by a.alt;\n  } S;\n} salt;\n";
        let m = merge_features("", incoming).unwrap();
        assert_eq!(m.added, vec!["salt"]);
        assert!(m.text.contains("lookup S {"));
        assert!(m.text.contains("} salt;"));
    }

    #[test]
    fn duplicate_language_systems_are_dropped() {
        let current = "languagesystem DFLT dflt;\nlanguagesystem latn dflt;\n";
        let incoming = "languagesystem DFLT dflt;\nlanguagesystem cyrl dflt;\nfeature liga {\n  sub f i by f_i;\n} liga;\n";
        let m = merge_features(current, incoming).unwrap();
        assert_eq!(m.text.matches("languagesystem DFLT dflt;").count(), 1);
        assert_eq!(m.text.matches("languagesystem latn dflt;").count(), 1);
        assert_eq!(m.text.matches("languagesystem cyrl dflt;").count(), 1);
    }

    #[test]
    fn comments_and_strings_are_not_blocks() {
        let incoming = "# feature liga { not a block\nfeature ss01 {\n  sub a by a.ss01; # } tricky\n} ss01;\n";
        let m = merge_features("", incoming).unwrap();
        assert_eq!(m.added, vec!["ss01"]);
        assert!(m.text.contains("# feature liga { not a block"));
    }

    #[test]
    fn several_incoming_blocks_replace_all_current_ones() {
        let current = "feature ss01 {\n  sub a by a.old;\n} ss01;\nfeature ss01 {\n  sub b by b.old;\n} ss01;\n";
        let incoming = "feature ss01 {\n  sub a by a.new;\n} ss01;\n";
        let m = merge_features(current, incoming).unwrap();
        assert_eq!(m.replaced, vec!["ss01"]);
        assert_eq!(m.text.matches("feature ss01 {").count(), 1);
        assert!(m.text.contains("a.new"));
        assert!(!m.text.contains("a.old") && !m.text.contains("b.old"));
    }

    #[test]
    fn empty_and_unclosed_files_are_refused() {
        assert!(merge_features(LIGA, "  \n# only a comment\n").is_err());
        assert!(merge_features(LIGA, "feature liga {\n  sub f i by f_i;\n").is_err());
        assert!(merge_features(LIGA, "feature liga {\n} wrong;\n").is_err());
        assert!(merge_features(LIGA, "include(other.fea);").is_err());
    }

    #[test]
    fn preamble_only_files_import() {
        let m = merge_features(LIGA, "lookup L {\n  sub a by a.alt;\n} L;\n").unwrap();
        assert!(m.added.is_empty() && m.replaced.is_empty());
        assert!(m.text.contains("lookup L {"));
        assert!(m.text.contains("sub f i by f_i;"));
    }
}
