//! Localized family names for an instance (Glyphs "Localized Family Names").
//!
//! The compiler writes the English (Windows, US English) names. Each other language
//! gets copies of the family-bearing records (name IDs 1, 4 and 16) with the English
//! family replaced by that language's family name, under its Windows language ID.

use std::collections::BTreeMap;

use write_fonts::read::{FontRef, TableProvider};
use write_fonts::tables::name::{Name, NameRecord};
use write_fonts::types::{NameId, Tag};
use write_fonts::{dump_table, FontBuilder};

/// The Windows language ID for a Glyphs (OpenType) language tag.
pub fn windows_language(tag: &str) -> Option<u16> {
    Some(match tag.trim().to_ascii_uppercase().as_str() {
        "ENG" | "DFLT" => 0x0409,
        "ARA" => 0x0401,
        "BGR" => 0x0402,
        "CAT" => 0x0403,
        "ZHT" => 0x0404,
        "CSY" => 0x0405,
        "DAN" => 0x0406,
        "DEU" => 0x0407,
        "ELL" => 0x0408,
        "FIN" => 0x040B,
        "FRA" => 0x040C,
        "IWR" | "HEB" => 0x040D,
        "HUN" => 0x040E,
        "ISL" => 0x040F,
        "ITA" => 0x0410,
        "JAN" => 0x0411,
        "KOR" => 0x0412,
        "NLD" => 0x0413,
        "NOR" => 0x0414,
        "PLK" => 0x0415,
        "PTB" => 0x0416,
        "ROM" => 0x0418,
        "RUS" => 0x0419,
        "HRV" => 0x041A,
        "SKY" => 0x041B,
        "SQI" => 0x041C,
        "SVE" => 0x041D,
        "THA" => 0x041E,
        "TRK" => 0x041F,
        "URD" => 0x0420,
        "IND" => 0x0421,
        "UKR" => 0x0422,
        "BEL" => 0x0423,
        "SLV" => 0x0424,
        "ETI" => 0x0425,
        "LVI" => 0x0426,
        "LTH" => 0x0427,
        "FAR" => 0x0429,
        "VIT" => 0x042A,
        "HIN" => 0x0439,
        "ZHS" => 0x0804,
        "PTG" => 0x0816,
        "SRB" => 0x0C1A,
        "ZHH" => 0x0C04,
        "ESP" => 0x0C0A,
        _ => return None,
    })
}

/// Adds the localized family names to the compiled font's name table. Returns the
/// new font and the language tags that were skipped (no Windows language ID).
pub fn add_localized_families(
    bytes: &[u8],
    names: &BTreeMap<String, String>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    if names.is_empty() {
        return Ok((bytes.to_vec(), Vec::new()));
    }
    let f = FontRef::new(bytes).map_err(|e| format!("not a valid font: {e}"))?;
    let name = f
        .name()
        .map_err(|e| format!("the compiled font has no name table: {e}"))?;
    let mut records: Vec<NameRecord> = Vec::new();
    let mut english: BTreeMap<u16, String> = BTreeMap::new();
    for rec in name.name_record() {
        let id = u16::from_be_bytes(rec.name_id().to_be_bytes());
        let string = rec
            .string(name.string_data())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if rec.platform_id() == 3 && rec.language_id() == 0x0409 {
            english.insert(id, string.clone());
        }
        records.push(NameRecord::new(
            rec.platform_id(),
            rec.encoding_id(),
            rec.language_id(),
            NameId::new(id),
            string.into(),
        ));
    }
    // The English family as it appears in these records.
    let Some(family) = english.get(&16).or_else(|| english.get(&1)).cloned() else {
        return Ok((bytes.to_vec(), Vec::new()));
    };
    let mut skipped = Vec::new();
    for (lang, local) in names {
        let Some(lcid) = windows_language(lang) else {
            skipped.push(lang.clone());
            continue;
        };
        for id in [1u16, 4, 16] {
            let Some(s) = english.get(&id) else { continue };
            let Some(rest) = s.strip_prefix(family.as_str()) else {
                continue;
            };
            records.retain(|r| {
                !(r.platform_id == 3 && r.language_id == lcid && r.name_id == NameId::new(id))
            });
            records.push(NameRecord::new(
                3,
                1,
                lcid,
                NameId::new(id),
                format!("{local}{rest}").into(),
            ));
        }
    }
    records.sort_by_key(|r| (r.platform_id, r.encoding_id, r.language_id, r.name_id));
    let mut b = FontBuilder::new();
    for r in f.table_directory.table_records() {
        let t = r.tag();
        if t == Tag::new(b"name") {
            continue;
        }
        if let Some(d) = f.table_data(t) {
            b.add_raw(t, d.as_bytes().to_vec());
        }
    }
    b.add_raw(
        Tag::new(b"name"),
        dump_table(&Name::new(records))
            .map_err(|e| format!("could not write the name table: {e}"))?,
    );
    Ok((b.build(), skipped))
}
