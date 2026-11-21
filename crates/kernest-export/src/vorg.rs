//! The VORG table: each glyph's vertical origin, for CFF and CFF2 fonts set
//! vertically. fontc writes vhea and vmtx but not VORG, so it is added to OTF exports
//! here.
//!
//! The origins come from the compiled TrueType font, which has the same glyph order:
//! a glyph's origin y is its top sidebearing (vmtx) plus the top of its bounds (glyf).
//! <https://learn.microsoft.com/en-us/typography/opentype/spec/vorg>

use std::collections::HashMap;

use write_fonts::read::tables::glyf::Glyph;
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::types::{GlyphId, Tag};
use write_fonts::FontBuilder;

/// The vertical origin y of every glyph of a TrueType font with vmtx, by glyph id.
fn origins(ttf: &[u8]) -> Option<Vec<i16>> {
    let f = FontRef::new(ttf).ok()?;
    let vmtx = f.vmtx().ok()?;
    let glyf = f.glyf().ok()?;
    let loca = f.loca(None).ok()?;
    let count = f.maxp().ok()?.num_glyphs();
    (0..count)
        .map(|gid| {
            let id = GlyphId::new(u32::from(gid));
            let tsb = vmtx.side_bearing(id).unwrap_or(0);
            let top = match loca.get_glyf(id, &glyf) {
                Ok(Some(Glyph::Simple(g))) => g.y_max(),
                Ok(Some(Glyph::Composite(g))) => g.y_max(),
                _ => 0,
            };
            Some(tsb.saturating_add(top))
        })
        .collect()
}

/// The VORG table for these origins: the most common one is the default, the others
/// are listed by glyph id.
fn table(origins: &[i16]) -> Vec<u8> {
    let mut counts: HashMap<i16, usize> = HashMap::new();
    for o in origins {
        *counts.entry(*o).or_default() += 1;
    }
    let default = counts
        .iter()
        .max_by_key(|(v, n)| (**n, std::cmp::Reverse(**v)))
        .map(|(v, _)| *v)
        .unwrap_or(0);
    let others: Vec<(u16, i16)> = origins
        .iter()
        .enumerate()
        .filter(|(_, o)| **o != default)
        .map(|(gid, o)| (gid as u16, *o))
        .collect();
    let mut out = Vec::with_capacity(8 + others.len() * 4);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&default.to_be_bytes());
    out.extend_from_slice(&(others.len() as u16).to_be_bytes());
    for (gid, o) in others {
        out.extend_from_slice(&gid.to_be_bytes());
        out.extend_from_slice(&o.to_be_bytes());
    }
    out
}

/// Adds VORG to an OTF made from `ttf`. Fonts without vertical metrics (no vmtx in
/// the TrueType font) come back unchanged.
pub fn add(otf: &[u8], ttf: &[u8]) -> Result<Vec<u8>, String> {
    let Some(origins) = origins(ttf) else {
        return Ok(otf.to_vec());
    };
    let f = FontRef::new(otf).map_err(|e| format!("not a valid font: {e}"))?;
    let mut b = FontBuilder::new();
    for r in f.table_directory.table_records() {
        let t = r.tag();
        if t == Tag::new(b"VORG") {
            continue;
        }
        if let Some(d) = f.table_data(t) {
            b.add_raw(t, d.as_bytes().to_vec());
        }
    }
    b.add_raw(Tag::new(b"VORG"), table(&origins));
    Ok(b.build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_most_common_origin() {
        let t = table(&[880, 880, 900, 880, 700]);
        assert_eq!(
            t,
            [
                0, 1, 0, 0, // version 1.0
                0x03, 0x70, // default 880
                0, 2, // two others
                0, 2, 0x03, 0x84, // glyph 2: 900
                0, 4, 0x02, 0xBC, // glyph 4: 700
            ]
        );
    }
}
