//! TrueType autohinting with tilvisan (a Rust port of ttfautohint). It works on static
//! and variable TrueType fonts; PostScript (CFF) outlines aren't hinted this way.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use kernest_core::Hinting;
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::tables::post::Post as WPost;
use write_fonts::types::{GlyphId16, Tag};
use write_fonts::FontBuilder;

/// Adds TrueType hinting instructions to a compiled `.ttf`. `tmp` is a scratch folder
/// (tilvisan reads its input from a file).
pub fn autohint(ttf: &[u8], hinting: &Hinting, tmp: &Path, name: &str) -> Result<Vec<u8>, String> {
    // tilvisan can't write back a `post` table with glyph names (format 2), so it
    // gets the font without them and the names are put back afterwards.
    let font = FontRef::new(ttf).map_err(|e| format!("not a valid font: {e}"))?;
    let post = font
        .table_data(Tag::new(b"post"))
        .map(|d| d.as_bytes().to_vec());
    let names: Option<Vec<String>> = post.as_ref().filter(|p| p.len() > 32).and_then(|_| {
        let p = font.post().ok()?;
        let n = font.maxp().ok()?.num_glyphs();
        (0..n)
            .map(|g| p.glyph_name(GlyphId16::new(g)).map(str::to_string))
            .collect()
    });
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
    let args = tilvisan::Args {
        input: input.display().to_string(),
        hinting_range_min: u32::from(hinting.range_min),
        hinting_range_max: u32::from(hinting.range_max),
        hinting_limit: u32::from(hinting.range_max).max(200),
        increase_x_height: u32::from(hinting.increase_x_height),
        composites: hinting.hint_composites,
        // The designer is exporting their own font.
        ignore_restrictions: true,
        ..Default::default()
    };
    let result = catch_unwind(AssertUnwindSafe(|| tilvisan::autohint(&args)));
    let _ = std::fs::remove_file(&input);
    let hinted = match result {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => return Err(format!("autohinting failed: {e}")),
        Err(_) => return Err("autohinting failed unexpectedly".into()),
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
        let out = tempfile::tempdir().unwrap();
        let r = export(&sample_font(), &ExportOptions::ttf(false, true, out.path())).unwrap();
        let plain = std::fs::read(&r.files[0].path).unwrap();
        assert!(hinted_tables(&plain).is_empty(), "fontc doesn't hint");
        let tmp = tempfile::tempdir().unwrap();
        let hinted = autohint(&plain, &Hinting::default(), tmp.path(), "t").unwrap();
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
    fn instructions(bytes: &[u8], ch: char) -> usize {
        use write_fonts::read::tables::glyf::Glyph;
        let f = FontRef::new(bytes).unwrap();
        let gid = f.cmap().unwrap().map_codepoint(ch).unwrap();
        let glyf = f.glyf().unwrap();
        match f.loca(None).unwrap().get_glyf(gid, &glyf).unwrap() {
            Some(Glyph::Simple(g)) => g.instructions().len(),
            Some(Glyph::Composite(g)) => g.instructions().map_or(0, |i| i.len()),
            None => 0,
        }
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
        let hinted = autohint(&vf, &Hinting::default(), tmp.path(), "vf").unwrap();
        assert_eq!(hinted_tables(&hinted), ["fpgm", "prep", "cvt "]);
        let font = FontRef::new(&hinted).unwrap();
        assert!(font.fvar().is_ok() && font.gvar().is_ok(), "still variable");
        assert!(instructions(&hinted, 'H') > 0);
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
        let hinted = autohint(&plain, &opts, tmp.path(), "c").unwrap();
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
}
