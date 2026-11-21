//! Repairs the `cvar` table the TrueType autohinter writes for variable fonts.
//!
//! When a font's hinting doesn't vary between masters, tilvisan can write a tuple
//! that says "all CVT entries" and then has no deltas for them, which fontTools and
//! strict parsers (OTS in browsers) reject. Tuples whose deltas are missing or all
//! zero change nothing, so they are dropped; with none left the table is removed.
//!
//! tilvisan also writes the variations in an order that changes from run to run
//! (SMO-84), so the kept ones are sorted by region: peak, then peak-only before
//! intermediate regions, then the intermediate bounds. Exporting twice then gives the
//! same table. A valid table that is already in order is returned byte for byte.
//!
//! Format: <https://learn.microsoft.com/en-us/typography/opentype/spec/cvar> and
//! <https://learn.microsoft.com/en-us/typography/opentype/spec/otvarcommonformats>.

use write_fonts::read::{FontRef, TableProvider};
use write_fonts::types::Tag;
use write_fonts::FontBuilder;

const SHARED_POINT_NUMBERS: u16 = 0x8000;
const COUNT_MASK: u16 = 0x0FFF;
const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;
const INTERMEDIATE_REGION: u16 = 0x4000;
const PRIVATE_POINT_NUMBERS: u16 = 0x2000;

/// What a font's `cvar` needed.
#[derive(Debug, PartialEq, Eq)]
pub enum Repair {
    /// No `cvar`, or nothing to change.
    Unchanged,
    /// Some tuples were dropped: how many had no usable deltas (malformed) and how
    /// many only had zero deltas.
    Dropped { malformed: usize, zero: usize },
    /// Every tuple was dropped, so the table was removed.
    Removed { malformed: usize, zero: usize },
    /// Nothing was dropped, but the tuples were put in their stable order.
    Sorted,
}

impl Repair {
    /// The export warning to show, if any: only malformed tuples are worth one.
    pub fn warning(&self) -> Option<String> {
        match self {
            Repair::Dropped { malformed, .. } | Repair::Removed { malformed, .. }
                if *malformed > 0 =>
            {
                Some(format!(
                    "cvar: dropped {malformed} empty variation{} from the autohinter.",
                    if *malformed == 1 { "" } else { "s" }
                ))
            }
            _ => None,
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes([self.u8()?, self.u8()?]))
    }
    fn skip(&mut self, n: usize) -> Option<()> {
        (self.pos + n <= self.data.len()).then(|| self.pos += n)
    }
}

/// Packed point numbers: `None` means "all points", otherwise the count.
fn point_count(r: &mut Reader) -> Option<Option<usize>> {
    let first = r.u8()?;
    if first == 0 {
        return Some(None);
    }
    let count = if first & 0x80 != 0 {
        (usize::from(first & 0x7F) << 8) | usize::from(r.u8()?)
    } else {
        usize::from(first)
    };
    let mut read = 0;
    while read < count {
        let control = r.u8()?;
        let run = usize::from(control & 0x7F) + 1;
        let size = if control & 0x80 != 0 { 2 } else { 1 };
        r.skip(run * size)?;
        read += run;
    }
    (read == count).then_some(Some(count))
}

/// Reads `count` packed deltas. `None` if the data runs out; otherwise whether any
/// delta is non-zero.
fn any_nonzero_delta(r: &mut Reader, count: usize) -> Option<bool> {
    let mut read = 0;
    let mut nonzero = false;
    while read < count {
        let control = r.u8()?;
        let run = usize::from(control & 0x3F) + 1;
        match control & 0xC0 {
            0x80 => {}
            0x40 => {
                for _ in 0..run {
                    nonzero |= r.u16()? != 0;
                }
            }
            0xC0 => {
                for _ in 0..run {
                    nonzero |= u32::from_be_bytes([r.u8()?, r.u8()?, r.u8()?, r.u8()?]) != 0;
                }
            }
            _ => {
                for _ in 0..run {
                    nonzero |= r.u8()? != 0;
                }
            }
        }
        read += run;
    }
    (read == count).then_some(nonzero)
}

enum Tuple {
    Keep,
    Zero,
    Malformed,
}

/// Checks the `cvar` table of `cvar_data` against `cvt_count` CVT entries and
/// `axis_count` axes. Returns the new table (`None` to remove it) and what was done.
pub fn repair_table(
    cvar_data: &[u8],
    cvt_count: usize,
    axis_count: usize,
) -> Option<(Option<Vec<u8>>, Repair)> {
    let mut r = Reader {
        data: cvar_data,
        pos: 4,
    };
    let count_field = r.u16()?;
    let data_offset = usize::from(r.u16()?);
    let shared = count_field & SHARED_POINT_NUMBERS != 0;
    let count = usize::from(count_field & COUNT_MASK);

    // Tuple headers, kept as raw bytes with their data sizes.
    let mut headers: Vec<(&[u8], usize, bool)> = Vec::with_capacity(count);
    let mut keys: Vec<Vec<i32>> = Vec::with_capacity(count);
    for _ in 0..count {
        let start = r.pos;
        let size = usize::from(r.u16()?);
        let index = r.u16()?;
        let mut coords = 0;
        if index & EMBEDDED_PEAK_TUPLE != 0 {
            coords += axis_count;
        }
        if index & INTERMEDIATE_REGION != 0 {
            coords += 2 * axis_count;
        }
        // The sort key: peak coordinates, then whether there's an intermediate
        // region (peak-only first), then its start and end coordinates. cvar always
        // embeds its peaks; a shared-tuple index (not allowed here) sorts last by number.
        let mut values = Vec::with_capacity(coords);
        for _ in 0..coords {
            values.push(i32::from(r.u16()? as i16));
        }
        let peak_len = if index & EMBEDDED_PEAK_TUPLE != 0 {
            axis_count
        } else {
            0
        };
        let mut key = Vec::with_capacity(coords + 2);
        if peak_len == 0 {
            key.push(1 + i32::from(index & 0x0FFF));
        } else {
            key.push(0);
        }
        key.extend_from_slice(&values[..peak_len]);
        key.push(i32::from(index & INTERMEDIATE_REGION != 0));
        key.extend_from_slice(&values[peak_len..]);
        keys.push(key);
        headers.push((
            &cvar_data[start..r.pos],
            size,
            index & PRIVATE_POINT_NUMBERS != 0,
        ));
    }

    // Serialized data: shared point numbers, then each tuple's data in order.
    let mut d = Reader {
        data: cvar_data,
        pos: data_offset,
    };
    let shared_points = if shared {
        let s = d.pos;
        let p = point_count(&mut d)?;
        Some((p, &cvar_data[s..d.pos]))
    } else {
        None
    };
    let mut verdicts = Vec::with_capacity(count);
    let mut tuple_data: Vec<&[u8]> = Vec::with_capacity(count);
    for &(_, size, private) in &headers {
        let start = d.pos;
        let end = (start + size).min(cvar_data.len());
        tuple_data.push(&cvar_data[start..end]);
        let mut t = Reader {
            data: &cvar_data[..end],
            pos: start,
        };
        let points = if private {
            point_count(&mut t)
        } else {
            shared_points.map(|(p, _)| p)
        };
        let verdict = match points.map(|p| p.unwrap_or(cvt_count)) {
            Some(n) => match any_nonzero_delta(&mut t, n) {
                Some(true) => Tuple::Keep,
                Some(false) => Tuple::Zero,
                None => Tuple::Malformed,
            },
            None => Tuple::Malformed,
        };
        verdicts.push(verdict);
        d.pos = start + size;
    }

    let malformed = verdicts
        .iter()
        .filter(|v| matches!(v, Tuple::Malformed))
        .count();
    let zero = verdicts.iter().filter(|v| matches!(v, Tuple::Zero)).count();
    let mut kept: Vec<usize> = (0..count)
        .filter(|&i| matches!(verdicts[i], Tuple::Keep))
        .collect();
    if kept.is_empty() {
        return Some((None, Repair::Removed { malformed, zero }));
    }
    // Stable order; equal regions (not expected) keep their data order.
    kept.sort_by(|&a, &b| {
        keys[a]
            .cmp(&keys[b])
            .then_with(|| tuple_data[a].cmp(tuple_data[b]))
    });
    let in_order = kept.windows(2).all(|w| w[0] < w[1]);
    if malformed == 0 && zero == 0 && in_order {
        return Some((Some(cvar_data.to_vec()), Repair::Unchanged));
    }

    let header_len: usize = kept.iter().map(|&i| headers[i].0.len()).sum();
    let mut out = cvar_data[..4].to_vec();
    let flags = if shared { SHARED_POINT_NUMBERS } else { 0 };
    out.extend_from_slice(&(flags | kept.len() as u16).to_be_bytes());
    out.extend_from_slice(&((8 + header_len) as u16).to_be_bytes());
    for &i in &kept {
        out.extend_from_slice(headers[i].0);
    }
    if let Some((_, bytes)) = shared_points {
        out.extend_from_slice(bytes);
    }
    for &i in &kept {
        out.extend_from_slice(tuple_data[i]);
    }
    let repair = if malformed == 0 && zero == 0 {
        Repair::Sorted
    } else {
        Repair::Dropped { malformed, zero }
    };
    Some((Some(out), repair))
}

/// Repairs the `cvar` table of a compiled font. A font without `cvar` comes back as is.
pub fn repair(font: &[u8]) -> Result<(Vec<u8>, Repair), String> {
    let f = FontRef::new(font).map_err(|e| format!("not a valid font: {e}"))?;
    let Some(cvar) = f.table_data(Tag::new(b"cvar")) else {
        return Ok((font.to_vec(), Repair::Unchanged));
    };
    let cvt_count = f
        .table_data(Tag::new(b"cvt "))
        .map_or(0, |d| d.as_bytes().len() / 2);
    let axis_count = f.fvar().map(|v| usize::from(v.axis_count())).unwrap_or(0);
    // A table that can't even be read is malformed as a whole.
    let (table, repair) = repair_table(cvar.as_bytes(), cvt_count, axis_count).unwrap_or((
        None,
        Repair::Removed {
            malformed: 1,
            zero: 0,
        },
    ));
    if repair == Repair::Unchanged {
        return Ok((font.to_vec(), repair));
    }
    let mut b = FontBuilder::new();
    for rec in f.table_directory.table_records() {
        let t = rec.tag();
        if t == Tag::new(b"cvar") {
            continue;
        }
        if let Some(d) = f.table_data(t) {
            b.add_raw(t, d.as_bytes().to_vec());
        }
    }
    if let Some(table) = table {
        b.add_raw(Tag::new(b"cvar"), table);
    }
    Ok((b.build(), repair))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table the audit found (B2): one tuple, peak 1.0, "all points", no deltas.
    const TRUNCATED: [u8; 15] = [
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x0e, 0x00, 0x01, 0xa0, 0x00, 0x40, 0x00, 0x00,
    ];

    /// A tuple: peak `peak`, private "all points", then `deltas` as packed bytes.
    fn tuple(peak: u16, deltas: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut data = vec![0x00];
        data.extend_from_slice(deltas);
        let mut header = (data.len() as u16).to_be_bytes().to_vec();
        header.extend_from_slice(&0xA000u16.to_be_bytes());
        header.extend_from_slice(&peak.to_be_bytes());
        (header, data)
    }

    fn table(tuples: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
        let header_len: usize = tuples.iter().map(|t| t.0.len()).sum();
        let mut out = vec![0, 1, 0, 0];
        out.extend_from_slice(&(tuples.len() as u16).to_be_bytes());
        out.extend_from_slice(&((8 + header_len) as u16).to_be_bytes());
        for t in tuples {
            out.extend_from_slice(&t.0);
        }
        for t in tuples {
            out.extend_from_slice(&t.1);
        }
        out
    }

    #[test]
    fn truncated_tuple_is_removed() {
        let (t, r) = repair_table(&TRUNCATED, 24, 1).unwrap();
        assert_eq!(t, None);
        assert_eq!(
            r,
            Repair::Removed {
                malformed: 1,
                zero: 0
            }
        );
        assert!(r.warning().unwrap().contains("dropped 1 empty variation"));
    }

    #[test]
    fn valid_table_is_unchanged() {
        // 3 CVT entries: deltas 5, 0, -2 as one byte run.
        let t = table(&[tuple(0x4000, &[0x02, 0x05, 0x00, 0xFE])]);
        let (out, r) = repair_table(&t, 3, 1).unwrap();
        assert_eq!(r, Repair::Unchanged);
        assert_eq!(out.unwrap(), t);
    }

    #[test]
    fn empty_and_zero_tuples_are_dropped_and_good_ones_kept() {
        // A run of three word deltas: 1, 0, -1.
        let good = tuple(0x4000, &[0x42, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF]);
        // A run of three zero deltas (peak -1.0).
        let zero = tuple(0xC000, &[0x82]);
        // "All points" and no deltas, as tilvisan wrote it (peak 0.5).
        let empty = tuple(0x2000, &[]);
        let t = table(&[zero, good.clone(), empty]);
        let (out, r) = repair_table(&t, 3, 1).unwrap();
        assert_eq!(
            r,
            Repair::Dropped {
                malformed: 1,
                zero: 1
            }
        );
        assert_eq!(out.unwrap(), table(&[good]));
        // Only the malformed one is worth a warning.
        assert!(r.warning().is_some());
        let (_, r) = repair_table(&table(&[tuple(0x4000, &[0x82])]), 3, 1).unwrap();
        assert_eq!(
            r,
            Repair::Removed {
                malformed: 0,
                zero: 1
            }
        );
        assert_eq!(r.warning(), None);
    }

    /// Like `tuple`, with an intermediate region from `start` to `end`.
    fn intermediate(peak: u16, start: u16, end: u16, deltas: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let (mut header, data) = tuple(peak, deltas);
        header[2..4].copy_from_slice(&0xE000u16.to_be_bytes());
        header.extend_from_slice(&start.to_be_bytes());
        header.extend_from_slice(&end.to_be_bytes());
        (header, data)
    }

    #[test]
    fn tuples_are_put_in_a_stable_order() {
        // Deltas 1, 2, 3 (one byte run).
        let d = [0x02, 0x01, 0x02, 0x03];
        let neg = tuple(0xC000, &d); // peak -1.0
        let half = tuple(0x2000, &d); // peak 0.5
        let full = tuple(0x4000, &d); // peak 1.0
        let mid = intermediate(0x4000, 0x2000, 0x4000, &d); // peak 1.0 from 0.5
        let sorted = table(&[neg.clone(), half.clone(), full.clone(), mid.clone()]);
        // Already in order: returned as is.
        let (out, r) = repair_table(&sorted, 3, 1).unwrap();
        assert_eq!(r, Repair::Unchanged);
        assert_eq!(out.unwrap(), sorted);
        // Any other order comes out the same, with no warning.
        for shuffled in [
            table(&[mid.clone(), full.clone(), half.clone(), neg.clone()]),
            table(&[full.clone(), neg.clone(), mid.clone(), half.clone()]),
        ] {
            let (out, r) = repair_table(&shuffled, 3, 1).unwrap();
            assert_eq!(r, Repair::Sorted);
            assert_eq!(r.warning(), None);
            assert_eq!(out.unwrap(), sorted);
        }
        // Dropping and sorting together.
        let zero = tuple(0x1000, &[0x82]);
        let (out, r) = repair_table(&table(&[full, zero, neg.clone(), mid, half]), 3, 1).unwrap();
        assert_eq!(
            r,
            Repair::Dropped {
                malformed: 0,
                zero: 1
            }
        );
        assert_eq!(out.unwrap(), sorted);
    }

    #[test]
    fn shared_point_numbers_stay_first_when_sorting() {
        // Shared "all points"; each tuple has just its deltas.
        let entry = |peak: u16, delta: u8| {
            let data = vec![0x00, delta];
            let mut header = (data.len() as u16).to_be_bytes().to_vec();
            header.extend_from_slice(&0x8000u16.to_be_bytes());
            header.extend_from_slice(&peak.to_be_bytes());
            (header, data)
        };
        let build = |tuples: &[(Vec<u8>, Vec<u8>)]| {
            let mut t = table(tuples);
            let header_end = 8 + tuples.iter().map(|t| t.0.len()).sum::<usize>();
            t[4] |= 0x80; // shared point numbers
            t.insert(header_end, 0x00); // all points
            t
        };
        let a = entry(0xC000, 4);
        let b = entry(0x4000, 9);
        let (out, r) = repair_table(&build(&[b.clone(), a.clone()]), 1, 1).unwrap();
        assert_eq!(r, Repair::Sorted);
        assert_eq!(out.unwrap(), build(&[a, b]));
    }

    #[test]
    fn explicit_point_numbers_set_the_delta_count() {
        // Two points (1 and 3), then two byte deltas.
        let mut data = vec![0x02, 0x01, 0x01, 0x02];
        data.extend_from_slice(&[0x01, 0x07, 0x00]);
        let mut header = (data.len() as u16).to_be_bytes().to_vec();
        header.extend_from_slice(&0xA000u16.to_be_bytes());
        header.extend_from_slice(&0x4000u16.to_be_bytes());
        let t = table(&[(header, data)]);
        assert_eq!(repair_table(&t, 24, 1).unwrap().1, Repair::Unchanged);
    }
}
