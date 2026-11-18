//! The STAT table: the axis value names apps show for a variable font's styles.
//!
//! fontc 2.0.1 doesn't turn designspace `<labels>` into STAT axis values, so the
//! table is added here after compiling, with write-fonts. Axis names reuse the fvar
//! name records; value names reuse any existing name record with the same string and
//! otherwise get a new one in the font-specific range (nameID >= 256), like fontTools.

use std::collections::HashMap;
use std::str::FromStr;

use kernest_core::Font;
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::tables::name::{Name, NameRecord};
use write_fonts::tables::stat::{AxisRecord, AxisValue, AxisValueTableFlags, Stat};
use write_fonts::types::{Fixed, NameId, Tag};
use write_fonts::{dump_table, FontBuilder};

/// Adds a STAT table built from the axes' value labels. A font with no labels is
/// returned unchanged.
pub fn add_stat(font: &Font, bytes: &[u8]) -> Result<Vec<u8>, String> {
    if !font.axes.iter().any(|a| !a.labels.is_empty()) {
        return Ok(bytes.to_vec());
    }
    let f = FontRef::new(bytes).map_err(|e| format!("not a valid font: {e}"))?;
    let name = f
        .name()
        .map_err(|e| format!("the compiled font has no name table: {e}"))?;

    // Existing records, kept as they are; new names are added as Windows Unicode.
    let mut records: Vec<NameRecord> = Vec::new();
    let mut by_string: HashMap<String, NameId> = HashMap::new();
    let mut max_id: u16 = 255;
    for rec in name.name_record() {
        let id = u16::from_be_bytes(rec.name_id().to_be_bytes());
        max_id = max_id.max(id);
        let string = rec
            .string(name.string_data())
            .map(|s| s.to_string())
            .unwrap_or_default();
        by_string
            .entry(string.clone())
            .or_insert_with(|| NameId::new(id));
        records.push(NameRecord::new(
            rec.platform_id(),
            rec.encoding_id(),
            rec.language_id(),
            NameId::new(id),
            string.into(),
        ));
    }
    let mut next_id = max_id.max(255) + 1;
    let mut intern = |s: &str| -> NameId {
        if let Some(&id) = by_string.get(s) {
            return id;
        }
        let id = NameId::new(next_id);
        next_id += 1;
        by_string.insert(s.to_string(), id);
        records.push(NameRecord::new(3, 1, 0x0409, id, s.to_string().into()));
        id
    };

    // Axis names reuse the fvar record for the same tag; a pruned axis gets a new one.
    let mut fvar_names: HashMap<String, NameId> = HashMap::new();
    if let Ok(fvar) = f.fvar() {
        if let Ok(axes) = fvar.axes() {
            for a in axes.iter() {
                fvar_names.insert(
                    a.axis_tag().to_string(),
                    NameId::new(u16::from_be_bytes(a.axis_name_id().to_be_bytes())),
                );
            }
        }
    }

    let mut design_axes = Vec::new();
    let mut axis_values = Vec::new();
    for (idx, a) in font.axes.iter().enumerate() {
        let idx = idx as u16;
        let tag = Tag::from_str(&a.tag)
            .map_err(|_| format!("axis tag \"{}\" isn't a valid tag", a.tag))?;
        let axis_name_id = match fvar_names.get(&a.tag) {
            Some(&id) => id,
            None => intern(&a.name),
        };
        design_axes.push(AxisRecord::new(tag, axis_name_id, idx));
        for l in &a.labels {
            let name_id = intern(&l.name);
            let flags = if l.elidable {
                AxisValueTableFlags::ELIDABLE_AXIS_VALUE_NAME
            } else {
                AxisValueTableFlags::empty()
            };
            let value = Fixed::from_f64(l.value);
            let av = if let Some(linked) = l.linked_value {
                AxisValue::format_3(idx, flags, name_id, value, Fixed::from_f64(linked))
            } else if l.min.is_some() || l.max.is_some() {
                let min = l.min.map(Fixed::from_f64).unwrap_or(Fixed::MIN);
                let max = l.max.map(Fixed::from_f64).unwrap_or(Fixed::MAX);
                AxisValue::format_2(idx, flags, name_id, value, min, max)
            } else {
                AxisValue::format_1(idx, flags, name_id, value)
            };
            axis_values.push(av);
        }
    }

    let stat = Stat::new(design_axes, axis_values, NameId::SUBFAMILY_NAME);
    records.sort_by_key(|r| (r.platform_id, r.encoding_id, r.language_id, r.name_id));
    let new_name = Name::new(records);

    let mut b = FontBuilder::new();
    for r in f.table_directory.table_records() {
        let t = r.tag();
        if t == Tag::new(b"name") || t == Tag::new(b"STAT") {
            continue;
        }
        if let Some(d) = f.table_data(t) {
            b.add_raw(t, d.as_bytes().to_vec());
        }
    }
    b.add_raw(
        Tag::new(b"name"),
        dump_table(&new_name).map_err(|e| format!("could not write the name table: {e}"))?,
    );
    b.add_raw(
        Tag::new(b"STAT"),
        dump_table(&stat).map_err(|e| format!("could not write the STAT table: {e}"))?,
    );
    Ok(b.build())
}
