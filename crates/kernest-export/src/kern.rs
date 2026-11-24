//! The legacy `kern` table ("Export kern Table"): one format 0 subtable of glyph
//! pairs, for apps that read kerning only from there. Group kerning is expanded
//! into glyph pairs; a glyph's own pair wins over its group's, as in GPOS.

use std::collections::{BTreeMap, HashMap};

use write_fonts::read::{FontRef, TableProvider};
use write_fonts::types::GlyphId16;

/// The most pairs one format 0 subtable can hold (its length is 16 bits).
const MAX_PAIRS: usize = (u16::MAX as usize - 14) / 6;

/// Pairs by glyph name from a UFO's kerning and groups, most specific first:
/// glyph–glyph, then glyph–group or group–glyph, then group–group. Zero values
/// stay in so they can cancel group kerning, and are dropped at the end.
fn resolved_pairs(ufo: &norad::Font) -> BTreeMap<(String, String), f64> {
    let members = |side: &str| -> Vec<String> {
        match ufo.groups.get(side) {
            Some(g) if side.starts_with("public.kern") => g.iter().map(|n| n.to_string()).collect(),
            _ => vec![side.to_string()],
        }
    };
    let is_group = |s: &str| s.starts_with("public.kern");
    let mut best: BTreeMap<(String, String), (u8, f64)> = BTreeMap::new();
    for (first, row) in ufo.kerning.iter() {
        for (second, value) in row {
            let rank = u8::from(is_group(first)) + u8::from(is_group(second));
            for a in members(first) {
                for b in members(second) {
                    match best.get(&(a.clone(), b.clone())) {
                        Some((r, _)) if *r <= rank => {}
                        _ => {
                            best.insert((a.clone(), b), (rank, *value));
                        }
                    }
                }
            }
        }
    }
    best.into_iter()
        .filter(|(_, (_, v))| v.round() != 0.0)
        .map(|(k, (_, v))| (k, v))
        .collect()
}

/// The kern table for a compiled static font, from the UFO it was built from.
/// `names` maps the compiled font's glyph names back to the UFO's (production
/// names). Returns the table and, when there were too many pairs, how many were
/// left out. `None` when the font has no kerning.
pub fn kern_table(
    compiled: &[u8],
    ufo: &norad::Font,
    names: &HashMap<String, String>,
) -> Result<Option<(Vec<u8>, usize)>, String> {
    let pairs = resolved_pairs(ufo);
    if pairs.is_empty() {
        return Ok(None);
    }
    let f = FontRef::new(compiled).map_err(|e| format!("not a valid font: {e}"))?;
    let post = f.post().map_err(|e| format!("no post table: {e}"))?;
    let count = f
        .maxp()
        .map_err(|e| format!("no maxp table: {e}"))?
        .num_glyphs();
    let mut gid: HashMap<String, u16> = HashMap::new();
    for g in 0..count {
        if let Some(n) = post.glyph_name(GlyphId16::new(g)) {
            let n = n.to_string();
            gid.insert(names.get(&n).cloned().unwrap_or(n), g);
        }
    }
    let mut list: Vec<(u16, u16, i16)> = pairs
        .iter()
        .filter_map(|((a, b), v)| {
            Some((
                *gid.get(a)?,
                *gid.get(b)?,
                v.round().clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16,
            ))
        })
        .collect();
    // When there are too many, the strongest kerns stay: they matter most.
    let dropped = list.len().saturating_sub(MAX_PAIRS);
    if dropped > 0 {
        list.sort_by_key(|p| (std::cmp::Reverse(p.2.unsigned_abs()), p.0, p.1));
        list.truncate(MAX_PAIRS);
    }
    list.sort_by_key(|p| (p.0, p.1));
    let n = list.len();
    // Binary search fields: the largest power of two not above n.
    let mut pow = 1usize;
    let mut log = 0u16;
    while pow * 2 <= n {
        pow *= 2;
        log += 1;
    }
    let search_range = (pow * 6) as u16;
    let range_shift = (n * 6) as u16 - search_range;
    let length = (14 + 6 * n) as u16;
    let mut out = Vec::with_capacity(4 + length as usize);
    for v in [
        0u16,
        1,
        0,
        length,
        0x0001,
        n as u16,
        search_range,
        log,
        range_shift,
    ] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    for (a, b, v) in list {
        out.extend_from_slice(&a.to_be_bytes());
        out.extend_from_slice(&b.to_be_bytes());
        out.extend_from_slice(&v.to_be_bytes());
    }
    Ok(Some((out, dropped)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_pairs_win_over_group_pairs() {
        let nm = |s: &str| norad::Name::new(s).unwrap();
        let mut ufo = norad::Font::new();
        ufo.groups
            .insert(nm("public.kern1.O"), vec![nm("O"), nm("D")]);
        ufo.groups
            .insert(nm("public.kern2.A"), vec![nm("A"), nm("V")]);
        let mut row = BTreeMap::new();
        row.insert(nm("public.kern2.A"), -40.0);
        ufo.kerning.insert(nm("public.kern1.O"), row);
        let mut row = BTreeMap::new();
        row.insert(nm("V"), -60.0);
        row.insert(nm("A"), 0.0);
        ufo.kerning.insert(nm("D"), row);
        let p = resolved_pairs(&ufo);
        let get = |a: &str, b: &str| p.get(&(a.to_string(), b.to_string())).copied();
        assert_eq!(get("O", "A"), Some(-40.0));
        assert_eq!(get("O", "V"), Some(-40.0));
        assert_eq!(get("D", "V"), Some(-60.0));
        // A zero exception cancels the group value.
        assert_eq!(get("D", "A"), None);
    }
}
