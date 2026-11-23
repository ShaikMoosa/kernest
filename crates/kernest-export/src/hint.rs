//! TrueType autohinting with tilvisan (a Rust port of ttfautohint). It works on static
//! and variable TrueType fonts; PostScript (CFF) outlines aren't hinted this way.
//! Zones and stems map to what ttfautohint accepts: the first horizontal stem as the
//! fallback stem width, x-height zone overshoot as snapping exceptions, and the
//! control instructions field as a control file. Manual zones it can't use are ignored.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use kernest_core::designspace::Location;
use kernest_core::{Font, Hinting, Master};
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::tables::post::Post as WPost;
use write_fonts::types::{GlyphId16, Tag};
use write_fonts::FontBuilder;

/// Adds TrueType hinting instructions to a compiled `.ttf`. `tmp` is a scratch folder
/// (tilvisan reads its input from a file). `location` picks the master whose zones and
/// stems apply; `None` (variable fonts) uses the default master.
pub fn autohint(
    ttf: &[u8],
    hinting: &Hinting,
    font: &Font,
    location: Option<&Location>,
    tmp: &Path,
    name: &str,
) -> Result<Vec<u8>, String> {
    let compiled = FontRef::new(ttf).map_err(|e| format!("not a valid font: {e}"))?;
    let post = compiled
        .table_data(Tag::new(b"post"))
        .map(|d| d.as_bytes().to_vec());
    let names: Option<Vec<String>> = post.as_ref().filter(|p| p.len() > 32).and_then(|_| {
        let p = compiled.post().ok()?;
        let n = compiled.maxp().ok()?.num_glyphs();
        (0..n)
            .map(|g| p.glyph_name(GlyphId16::new(g)).map(str::to_string))
            .collect()
    });
    let master = location
        .and_then(|l| font.masters.iter().find(|m| &m.location == l))
        .unwrap_or_else(|| kernest_core::interpolate::default_master(font));
    // ttfautohint takes one fallback stem width, not a list: the first horizontal
    // stem stands in for the master's stem widths.
    let fallback_stem_width = hinting.fallback_stem_width.map_or_else(
        || {
            master
                .stems
                .iter()
                .find(|s| s.horizontal)
                .map(|s| s.value.round().max(0.0) as u32)
                .unwrap_or(0)
        },
        u32::from,
    );
    let control = tmp.join(format!("{name}-control.txt"));
    let has_control = !font.info.control_instructions.trim().is_empty();
    if has_control {
        // The input font has no glyph names, so name references in the control
        // instructions become glyph indices first.
        let text = match &names {
            Some(n) => control_with_indices(&font.info.control_instructions, n),
            None => font.info.control_instructions.clone(),
        };
        std::fs::write(&control, text)
            .map_err(|e| format!("could not write control instructions: {e}"))?;
    }
    // tilvisan can't write back a `post` table with glyph names (format 2), so it
    // gets the font without them and the names are put back afterwards.
    let input_bytes = match &post {
        Some(p) if p.len() >= 32 => {
            let mut v3 = p[..32].to_vec();
            v3[..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
            replace_table(ttf, b"post", &v3)?
        }
        _ => ttf.to_vec(),
    };
    let input = tmp.join(format!("{name}-unhinted.ttf"));
    std::fs::write(&input, &input_bytes)
        .map_err(|e| format!("could not write a temporary font: {e}"))?;
    let defaults = tilvisan::Args::default();
    let script = |tag: &Option<String>, fallback| {
        tag.as_deref()
            .and_then(|t| tilvisan::ScriptClassIndex::from_tag(t).ok())
            .unwrap_or(fallback)
    };
    let args = tilvisan::Args {
        input: input.display().to_string(),
        hinting_range_min: u32::from(hinting.range_min),
        hinting_range_max: u32::from(hinting.range_max),
        hinting_limit: hinting
            .hinting_limit
            .map_or_else(|| u32::from(hinting.range_max).max(200), u32::from),
        increase_x_height: u32::from(hinting.increase_x_height),
        composites: hinting.hint_composites,
        fallback_stem_width,
        x_height_snapping_exceptions: hinting
            .x_height_snapping_exceptions
            .clone()
            .unwrap_or_else(|| x_height_exceptions(font, master, hinting)),
        control_file: has_control.then(|| control.clone()),
        default_script: script(&hinting.default_script, defaults.default_script),
        fallback_script: script(&hinting.fallback_script, defaults.fallback_script),
        windows_compatibility: hinting.windows_compatibility,
        symbol: hinting.symbol,
        adjust_subglyphs: hinting.adjust_subglyphs,
        fallback_scaling: hinting.fallback_scaling,
        ttfa_table: hinting.ttfa_table,
        // Record the settings in the version string, as ttfautohint does by default.
        detailed_info: !hinting.no_info,
        no_info: hinting.no_info,
        // The designer is exporting their own font.
        ignore_restrictions: true,
        ..defaults
    };
    let result = catch_unwind(AssertUnwindSafe(|| tilvisan::autohint(&args)));
    let _ = std::fs::remove_file(&input);
    if has_control {
        let _ = std::fs::remove_file(&control);
    }
    let hinted = match result {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => return Err(format!("autohinting failed: {e}")),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            return Err(format!("autohinting failed unexpectedly: {msg}"));
        }
    };
    // A hinted font rounds sizes to whole pixels (head flags bit 3), as hinting assumes.
    // tilvisan also stamps head.modified with the current time; the compiled font's
    // date is kept instead, which follows SOURCE_DATE_EPOCH, so exports can be
    // reproduced byte for byte (SMO-84).
    let hinted = match FontRef::new(&hinted)
        .ok()
        .and_then(|f| f.table_data(Tag::new(b"head")))
    {
        Some(head) if head.len() >= 36 => {
            let mut head = head.as_bytes().to_vec();
            let flags = u16::from_be_bytes([head[16], head[17]]) | (1 << 3);
            head[16..18].copy_from_slice(&flags.to_be_bytes());
            if let Some(original) = FontRef::new(ttf)
                .ok()
                .and_then(|f| f.table_data(Tag::new(b"head")))
                .filter(|h| h.len() >= 36)
            {
                head[28..36].copy_from_slice(&original.as_bytes()[28..36]);
            }
            replace_table(&hinted, b"head", &head)?
        }
        _ => hinted,
    };
    let (Some(post), Some(mut names)) = (post, names) else {
        return Ok(hinted);
    };
    // Hinting composites adds one glyph (".ttfautohint") at the end.
    let count = FontRef::new(&hinted)
        .and_then(|f| f.maxp())
        .map(|m| m.num_glyphs() as usize)
        .map_err(|e| format!("autohinting made an invalid font: {e}"))?;
    let restored = if count == names.len() {
        post
    } else {
        names.resize_with(count, || ".ttfautohint".to_string());
        let mut v2 = write_fonts::dump_table(&WPost::new_v2(names.iter().map(String::as_str)))
            .map_err(|e| format!("could not write glyph names: {e:?}"))?;
        v2[4..32].copy_from_slice(&post[4..32]);
        v2
    };
    replace_table(&hinted, b"post", &restored)
}

/// Warnings for hinting settings the autohinter can't use: a stem width mode other
/// than qsq (tilvisan has no way to set it) and unknown script tags.
pub fn option_warnings(hinting: &Hinting) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(m) = hinting.stem_width_mode.as_deref().filter(|m| *m != "qsq") {
        out.push(format!(
            "Stem width mode \"{m}\" isn't supported by Kernest's autohinter; qsq was used."
        ));
    }
    for tag in [&hinting.default_script, &hinting.fallback_script]
        .into_iter()
        .flatten()
    {
        if tilvisan::ScriptClassIndex::from_tag(tag).is_err() {
            out.push(format!(
                "The autohinter has no script \"{tag}\"; its default was used."
            ));
        }
    }
    out
}

/// The font with a table set: replaced if it's there, added if not.
pub(crate) fn put_table(font: &[u8], tag: &[u8; 4], data: &[u8]) -> Result<Vec<u8>, String> {
    let f = FontRef::new(font).map_err(|e| format!("not a valid font: {e}"))?;
    let mut b = FontBuilder::new();
    for r in f.table_directory.table_records() {
        let t = r.tag();
        if t != Tag::new(tag) {
            if let Some(d) = f.table_data(t) {
                b.add_raw(t, d.as_bytes().to_vec());
            }
        }
    }
    b.add_raw(Tag::new(tag), data.to_vec());
    Ok(b.build())
}

/// Sizes where the x-height zone's overshoot rounds to a pixel more than the flat
/// x-height: snapping there would flatten the overshoot, so those sizes are exempted
/// from x-height snapping (ttfautohint's `-X` list). Consecutive sizes become ranges.
fn x_height_exceptions(font: &Font, master: &Master, hinting: &Hinting) -> String {
    let xh = master.metrics.x_height;
    let Some(zone) = master
        .alignment_zones
        .iter()
        .find(|z| z.position == xh && z.size > 0.0)
    else {
        return String::new();
    };
    let upem = f64::from(font.info.units_per_em.max(1));
    let text = |a: u32, b: u32| {
        if a == b {
            a.to_string()
        } else {
            format!("{a}-{b}")
        }
    };
    let mut parts: Vec<String> = Vec::new();
    let mut run: Option<(u32, u32)> = None;
    // tilvisan's exception set starts at 6 ppem.
    for ppem in u32::from(hinting.range_min).max(6)..=u32::from(hinting.range_max) {
        let p = f64::from(ppem);
        let exception = ((xh + zone.size) * p / upem).round() > (xh * p / upem).round();
        if exception {
            run = Some(match run {
                Some((a, b)) if b + 1 == ppem => (a, ppem),
                _ => (ppem, ppem),
            });
        } else if let Some((a, b)) = run.take() {
            parts.push(text(a, b));
        }
    }
    if let Some((a, b)) = run {
        parts.push(text(a, b));
    }
    parts.join(", ")
}

#[derive(Clone, Copy, PartialEq)]
enum Tok {
    Name,
    Int,
    Real,
    At,
    Comma,
    LParen,
    RParen,
    Plus,
    Minus,
    Star,
    Sep,
}

/// Splits control instructions the way tilvisan's lexer does: names, numbers and
/// punctuation, skipping comments and line continuations, with `;` and newlines
/// as entry separators.
fn lex_control(text: &str) -> Vec<(Tok, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let bytes = rest.as_bytes();
        let c = bytes[0] as char;
        match c {
            ' ' | '\t' | '\r' | '\x0c' => rest = &rest[1..],
            '\n' | ';' => {
                out.push((Tok::Sep, String::new()));
                rest = &rest[1..];
            }
            '#' => rest = rest.split_once('\n').map(|(_, after)| after).unwrap_or(""),
            '\\' => {
                let after = &rest[1..];
                let after = after.strip_prefix('\r').unwrap_or(after);
                rest = after.strip_prefix('\n').unwrap_or(after);
            }
            '@' | ',' | '(' | ')' | '+' | '-' | '*' => {
                let t = match c {
                    '@' => Tok::At,
                    ',' => Tok::Comma,
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    '+' => Tok::Plus,
                    '-' => Tok::Minus,
                    _ => Tok::Star,
                };
                out.push((t, c.to_string()));
                rest = &rest[1..];
            }
            _ if c.is_ascii_digit()
                || (c == '.' && bytes.get(1).is_some_and(|b| b.is_ascii_digit())) =>
            {
                let mut end = 0;
                let mut real = false;
                if bytes.len() > 1 && bytes[0] == b'0' && (bytes[1] | 0x20) == b'x' {
                    end = 2;
                    while end < bytes.len() && (bytes[end] as char).is_ascii_hexdigit() {
                        end += 1;
                    }
                } else {
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        end += 1;
                    }
                    if end < bytes.len() && bytes[end] == b'.' {
                        real = true;
                        end += 1;
                        while end < bytes.len() && bytes[end].is_ascii_digit() {
                            end += 1;
                        }
                    }
                }
                out.push((
                    if real { Tok::Real } else { Tok::Int },
                    rest[..end].to_string(),
                ));
                rest = &rest[end..];
            }
            _ => {
                let mut end = 0;
                while end < bytes.len() {
                    let ch = bytes[end] as char;
                    if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
                        end += 1;
                    } else {
                        break;
                    }
                }
                if end == 0 {
                    rest = &rest[1..];
                } else {
                    out.push((Tok::Name, rest[..end].to_string()));
                    rest = &rest[end..];
                }
            }
        }
    }
    out
}

/// Control instructions with glyph names replaced by their indices, for tilvisan's
/// name-less input font. Follows the control grammar: an optional font index, then
/// either a glyph adjustment (one glyph reference) or a style/width adjustment
/// (script and feature names stay; glyph sets after `@` are mapped). Unknown names
/// are left for tilvisan to report.
fn control_with_indices(text: &str, names: &[String]) -> String {
    let gid: std::collections::HashMap<&str, usize> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let mut entries: Vec<Vec<(Tok, String)>> = vec![Vec::new()];
    for t in lex_control(text) {
        if t.0 == Tok::Sep {
            entries.push(Vec::new());
        } else {
            entries.last_mut().unwrap().push(t);
        }
    }
    let adjust = [
        "point", "p", "touch", "t", "left", "l", "right", "r", "nodir", "n",
    ];
    let mut lines = Vec::new();
    for e in entries {
        if e.is_empty() {
            continue;
        }
        let kind = |k: usize| e.get(k).map(|t| t.0);
        let is_kw = |k: usize, list: &[&str]| matches!(e.get(k), Some((Tok::Name, s)) if list.contains(&s.as_str()));
        // A leading integer is a font index only when the tokens after it look
        // like an adjustment (the same rule tilvisan's parser uses).
        let mut g = 0;
        if kind(0) == Some(Tok::Int) {
            let glyph_adjust = matches!(kind(1), Some(Tok::Name | Tok::Int)) && is_kw(2, &adjust);
            let named_adjust = kind(1) == Some(Tok::Name)
                && kind(2) == Some(Tok::Name)
                && (kind(3) == Some(Tok::At) || is_kw(3, &["width", "w"]));
            let star_width = kind(1) == Some(Tok::Star)
                && kind(2) == Some(Tok::Name)
                && is_kw(3, &["width", "w"]);
            if glyph_adjust || named_adjust || star_width {
                g = 1;
            }
        }
        let style_at = kind(g) == Some(Tok::Name)
            && kind(g + 1) == Some(Tok::Name)
            && kind(g + 2) == Some(Tok::At);
        let width = matches!(kind(g), Some(Tok::Name | Tok::Star))
            && kind(g + 1) == Some(Tok::Name)
            && is_kw(g + 2, &["width", "w"]);
        let parts: Vec<String> = e
            .iter()
            .enumerate()
            .map(|(k, (t, s))| {
                let map = (style_at && k > g + 2 && *t == Tok::Name)
                    || (!style_at && !width && k == g && *t == Tok::Name);
                if map {
                    gid.get(s.as_str())
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| s.clone())
                } else {
                    s.clone()
                }
            })
            .collect();
        lines.push(parts.join(" "));
    }
    lines.join("\n")
}

/// The font with one table's data replaced.
fn replace_table(font: &[u8], tag: &[u8; 4], data: &[u8]) -> Result<Vec<u8>, String> {
    let f = FontRef::new(font).map_err(|e| format!("not a valid font: {e}"))?;
    let mut b = FontBuilder::new();
    for r in f.table_directory.table_records() {
        let t = r.tag();
        if t == Tag::new(tag) {
            b.add_raw(t, data.to_vec());
        } else if let Some(d) = f.table_data(t) {
            b.add_raw(t, d.as_bytes().to_vec());
        }
    }
    Ok(b.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{export, ExportOptions};
    use kernest_core::sample::sample_font;

    fn hinted_tables(bytes: &[u8]) -> Vec<&'static str> {
        let f = FontRef::new(bytes).unwrap();
        ["fpgm", "prep", "cvt "]
            .into_iter()
            .filter(|t| {
                f.table_data(Tag::new_checked(t.as_bytes()).unwrap())
                    .is_some()
            })
            .collect()
    }

    #[test]
    fn hints_a_static_font() {
        let f = sample_font();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let plain = std::fs::read(&r.files[0].path).unwrap();
        assert!(hinted_tables(&plain).is_empty(), "fontc doesn't hint");
        let tmp = tempfile::tempdir().unwrap();
        let hinted = autohint(&plain, &Hinting::default(), &f, None, tmp.path(), "t").unwrap();
        assert_eq!(hinted_tables(&hinted), ["fpgm", "prep", "cvt "]);
        let f = FontRef::new(&hinted).unwrap();
        assert_eq!(f.maxp().unwrap().num_glyphs(), 96);
        assert!(f.gpos().is_ok(), "the rest of the font is kept");
        let gid = f.cmap().unwrap().map_codepoint('H').unwrap();
        assert_eq!(
            f.post().unwrap().glyph_name(gid.try_into().unwrap()),
            Some("H")
        );
        assert!(instructions(&hinted, 'H') > 0, "H has hinting instructions");
    }

    /// Bytes of hinting instructions on a character's glyph.
    fn instruction_bytes(bytes: &[u8], ch: char) -> Vec<u8> {
        use write_fonts::read::tables::glyf::Glyph;
        let f = FontRef::new(bytes).unwrap();
        let gid = f.cmap().unwrap().map_codepoint(ch).unwrap();
        let glyf = f.glyf().unwrap();
        match f.loca(None).unwrap().get_glyf(gid, &glyf).unwrap() {
            Some(Glyph::Simple(g)) => g.instructions().to_vec(),
            Some(Glyph::Composite(g)) => g.instructions().map(|i| i.to_vec()).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    fn instructions(bytes: &[u8], ch: char) -> usize {
        instruction_bytes(bytes, ch).len()
    }

    #[test]
    fn hints_a_variable_font() {
        use kernest_core::{structure, Axis};
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        };
        let f = structure::set_axes(&sample_font(), vec![wght]).unwrap();
        let (f, _) = structure::add_master(
            &f,
            "m01",
            "Bold",
            &std::collections::BTreeMap::from([("wght".into(), 700.0)]),
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        let vf = std::fs::read(&r.files[0].path).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let hinted = autohint(&vf, &Hinting::default(), &f, None, tmp.path(), "vf").unwrap();
        assert_eq!(hinted_tables(&hinted), ["fpgm", "prep", "cvt "]);
        let font = FontRef::new(&hinted).unwrap();
        assert!(font.fvar().is_ok() && font.gvar().is_ok(), "still variable");
        assert!(instructions(&hinted, 'H') > 0);
    }

    #[test]
    fn variable_export_keeps_cvar_valid() {
        // The cvar check (SMO-17) leaves a valid hinted variable font alone; the
        // malformed case is tested with the audit's font in kernest-import's tests.
        use kernest_core::{structure, Axis};
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        };
        let f = structure::set_axes(&sample_font(), vec![wght]).unwrap();
        let (f, _) = structure::add_master(
            &f,
            "m01",
            "Bold",
            &std::collections::BTreeMap::from([("wght".into(), 700.0)]),
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut opts = ExportOptions::ttf(true, false, out.path());
        opts.autohint = true;
        let r = export(&f, &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(
            !r.warnings.iter().any(|w| w.contains("cvar")),
            "{:?}",
            r.warnings
        );
        let vf = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&vf).unwrap();
        assert!(font.table_data(Tag::new(b"fpgm")).is_some(), "still hinted");
        // Whatever cvar is left decodes in full.
        if let Some(cvar) = font.table_data(Tag::new(b"cvar")) {
            let cvt = font.table_data(Tag::new(b"cvt ")).unwrap().len() / 2;
            let (_, repair) = crate::cvar::repair_table(cvar.as_bytes(), cvt, 1).unwrap();
            assert_eq!(repair, crate::cvar::Repair::Unchanged);
        }

        // SMO-84: a second export gives the same tables. head only differs in its
        // dates (and so its checksum) when they come from the clock.
        let again_dir = tempfile::tempdir().unwrap();
        let again = export(
            &f,
            &ExportOptions {
                out_dir: again_dir.path().to_path_buf(),
                ..opts.clone()
            },
        )
        .unwrap();
        let again = std::fs::read(&again.files[0].path).unwrap();
        let other = FontRef::new(&again).unwrap();
        let tags: Vec<Tag> = font
            .table_directory
            .table_records()
            .iter()
            .map(|r| r.tag())
            .collect();
        assert_eq!(
            tags,
            other
                .table_directory
                .table_records()
                .iter()
                .map(|r| r.tag())
                .collect::<Vec<_>>()
        );
        for tag in tags {
            let mut x = font.table_data(tag).unwrap().as_bytes().to_vec();
            let mut y = other.table_data(tag).unwrap().as_bytes().to_vec();
            if tag == Tag::new(b"head") {
                for h in [&mut x, &mut y] {
                    h[8..12].fill(0);
                    h[20..36].fill(0);
                }
            }
            assert_eq!(x, y, "{tag} differs between two exports");
        }
    }

    #[test]
    fn hinting_keeps_the_compiled_modified_date() {
        let out = tempfile::tempdir().unwrap();
        let f = sample_font();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let plain = std::fs::read(&r.files[0].path).unwrap();
        // An old date, as SOURCE_DATE_EPOCH would give.
        let head = FontRef::new(&plain)
            .unwrap()
            .table_data(Tag::new(b"head"))
            .unwrap();
        let mut head = head.as_bytes().to_vec();
        head[28..36].copy_from_slice(&1_000_000i64.to_be_bytes());
        let plain = replace_table(&plain, b"head", &head).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let hinted = autohint(&plain, &Hinting::default(), &f, None, tmp.path(), "d").unwrap();
        let h = FontRef::new(&hinted).unwrap().head().unwrap();
        assert_eq!(h.modified().as_secs(), 1_000_000);
    }

    #[test]
    fn hinting_composites_keeps_glyph_names() {
        let out = tempfile::tempdir().unwrap();
        let f = crate::tests::with_composites(sample_font());
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let plain = std::fs::read(&r.files[0].path).unwrap();
        let before = FontRef::new(&plain).unwrap().maxp().unwrap().num_glyphs();
        let opts = Hinting {
            hint_composites: true,
            ..Hinting::default()
        };
        let tmp = tempfile::tempdir().unwrap();
        let hinted = autohint(&plain, &opts, &f, None, tmp.path(), "c").unwrap();
        let f = FontRef::new(&hinted).unwrap();
        let after = f.maxp().unwrap().num_glyphs();
        assert!(after == before || after == before + 1);
        let names = |b: &[u8], n: u16| -> Vec<String> {
            let p = FontRef::new(b).unwrap().post().unwrap();
            (0..n)
                .map(|g| p.glyph_name(GlyphId16::new(g)).unwrap_or("?").to_string())
                .collect()
        };
        let post = f.post().unwrap();
        assert_eq!(names(&hinted, before), names(&plain, before));
        assert!(names(&plain, before).contains(&"Oacute".to_string()));
        assert_eq!(after, before + 1, "ttfautohint adds a glyph for composites");
        {
            assert_eq!(
                post.glyph_name(GlyphId16::new(before)),
                Some(".ttfautohint")
            );
        }
    }

    /// The version string (name ID 5), where tilvisan records the hinting settings.
    fn version_string(bytes: &[u8]) -> String {
        let f = FontRef::new(bytes).unwrap();
        let name = f.name().unwrap();
        let data = name.string_data();
        for r in name.name_record() {
            if r.name_id() == write_fonts::types::NameId::VERSION_STRING {
                if let Ok(s) = r.string(data) {
                    return s.chars().collect();
                }
            }
        }
        String::new()
    }

    #[test]
    fn zones_and_stems_map_to_tilvisan_settings() {
        use kernest_core::{Stem, Zone};
        let mut f = sample_font();
        f.masters[0].stems = vec![Stem {
            name: "h".into(),
            horizontal: true,
            value: 100.0,
        }];
        // x-height 500 with 40 overshoot: the overshoot rounds to a pixel more than
        // the flat x-height at 14, 16, 18, 20, 22 and every size from 24 up.
        f.masters[0].alignment_zones = vec![Zone {
            position: 500.0,
            size: 40.0,
        }];
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let plain = std::fs::read(&r.files[0].path).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let hinted = autohint(&plain, &Hinting::default(), &f, None, tmp.path(), "s").unwrap();
        let v = version_string(&hinted);
        assert!(v.contains("-H 100"), "fallback stem width recorded: {v}");
        let x = v
            .split("-X \"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or_else(|| panic!("no x-height exceptions in {v}"));
        assert_eq!(x, "14, 16, 18, 20, 22, 24-50");
    }

    #[test]
    fn control_names_become_indices() {
        let names: Vec<String> = [".notdef", "H", "O", "a.sups"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            control_with_indices("H point 0 yshift 0.5 @ 12", &names),
            "1 point 0 yshift 0.5 @ 12"
        );
        assert_eq!(
            control_with_indices("0 O touch 2-4 @ 12,13", &names),
            "0 2 touch 2 - 4 @ 12 , 13"
        );
        assert_eq!(
            control_with_indices("latn sups @ a.sups, H", &names),
            "latn sups @ 3 , 1"
        );
        assert_eq!(
            control_with_indices("* dflt width 100, 80", &names),
            "* dflt width 100 , 80"
        );
        assert_eq!(
            control_with_indices(
                concat!("# note\nH point 0 @ 12;", "\\", "\n", "O left 1"),
                &names
            ),
            "1 point 0 @ 12\n2 left 1"
        );
        assert_eq!(
            control_with_indices("X point 0 @ 12", &names),
            "X point 0 @ 12",
            "unknown names are left for tilvisan to report"
        );
    }

    #[test]
    fn control_instructions_reach_the_glyphs() {
        let mut f = sample_font();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let plain = std::fs::read(&r.files[0].path).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let without = autohint(&plain, &Hinting::default(), &f, None, tmp.path(), "a").unwrap();
        f.info.control_instructions = "H point 0 yshift 0.5 @ 12".into();
        let with = autohint(&plain, &Hinting::default(), &f, None, tmp.path(), "b").unwrap();
        assert_ne!(
            instruction_bytes(&without, 'H'),
            instruction_bytes(&with, 'H'),
            "the point delta changes H's instructions"
        );
        f.info.control_instructions = "H point 9999 @ 12".into();
        let err = autohint(&plain, &Hinting::default(), &f, None, tmp.path(), "c").unwrap_err();
        assert!(err.contains("autohinting failed"), "{err}");
    }
}
