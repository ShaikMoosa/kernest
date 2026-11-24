//! WOFF 1.0 (the W3C Web Open Font Format, version 1): every table compressed with
//! zlib on its own, kept uncompressed when that isn't smaller. Older browsers and
//! some tools read WOFF but not WOFF2.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use write_fonts::read::FontRef;

/// The checksum of a table's data, as in the sfnt table directory.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut word = [0u8; 4];
        word[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

/// A `.woff` file holding the same tables as the sfnt `font`.
pub fn to_woff(font: &[u8]) -> Result<Vec<u8>, String> {
    let f = FontRef::new(font).map_err(|e| format!("not a valid font: {e}"))?;
    let flavor = u32::from_be_bytes(
        font.get(..4)
            .ok_or("the font is too short")?
            .try_into()
            .expect("four bytes"),
    );
    // Tables in tag order, each with its original and stored bytes.
    let mut tables: Vec<([u8; 4], Vec<u8>, Vec<u8>)> = Vec::new();
    for r in f.table_directory.table_records() {
        let tag = r.tag().into_bytes();
        let Some(data) = f.table_data(r.tag()) else {
            continue;
        };
        let orig = data.as_bytes().to_vec();
        let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
        z.write_all(&orig)
            .map_err(|e| format!("could not compress a table: {e}"))?;
        let packed = z
            .finish()
            .map_err(|e| format!("could not compress a table: {e}"))?;
        let stored = if packed.len() < orig.len() {
            packed
        } else {
            orig.clone()
        };
        tables.push((tag, orig, stored));
    }
    tables.sort_by_key(|t| t.0);
    let n = tables.len();
    let total_sfnt = 12 + 16 * n + tables.iter().map(|t| pad4(t.1.len())).sum::<usize>();
    let mut offset = 44 + 20 * n;
    let mut dir = Vec::with_capacity(20 * n);
    let mut data = Vec::new();
    for (tag, orig, stored) in &tables {
        dir.extend_from_slice(tag);
        dir.extend_from_slice(&(offset as u32).to_be_bytes());
        dir.extend_from_slice(&(stored.len() as u32).to_be_bytes());
        dir.extend_from_slice(&(orig.len() as u32).to_be_bytes());
        dir.extend_from_slice(&checksum(orig).to_be_bytes());
        data.extend_from_slice(stored);
        data.resize(pad4(data.len()), 0);
        offset += pad4(stored.len());
    }
    let length = 44 + dir.len() + data.len();
    // The font version (head.fontRevision) as the WOFF version.
    let (major, minor) = f
        .table_data(write_fonts::types::Tag::new(b"head"))
        .filter(|h| h.len() >= 8)
        .map(|h| {
            let b = h.as_bytes();
            (
                u16::from_be_bytes([b[4], b[5]]),
                u16::from_be_bytes([b[6], b[7]]),
            )
        })
        .unwrap_or((1, 0));
    let mut out = Vec::with_capacity(length);
    out.extend_from_slice(b"wOFF");
    out.extend_from_slice(&flavor.to_be_bytes());
    out.extend_from_slice(&(length as u32).to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // reserved
    out.extend_from_slice(&(total_sfnt as u32).to_be_bytes());
    out.extend_from_slice(&major.to_be_bytes());
    out.extend_from_slice(&minor.to_be_bytes());
    out.extend_from_slice(&[0u8; 20]); // no metadata, no private data
    out.extend_from_slice(&dir);
    out.extend_from_slice(&data);
    Ok(out)
}

#[cfg(test)]
pub(crate) fn tables_of_woff(woff: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    use std::io::Read;
    let be32 = |at: usize| u32::from_be_bytes(woff[at..at + 4].try_into().unwrap()) as usize;
    let n = u16::from_be_bytes([woff[12], woff[13]]) as usize;
    (0..n)
        .map(|i| {
            let e = 44 + 20 * i;
            let tag: [u8; 4] = woff[e..e + 4].try_into().unwrap();
            let (off, comp, orig) = (be32(e + 4), be32(e + 8), be32(e + 12));
            let stored = &woff[off..off + comp];
            let data = if comp < orig {
                let mut v = Vec::new();
                flate2::read::ZlibDecoder::new(stored)
                    .read_to_end(&mut v)
                    .unwrap();
                v
            } else {
                stored.to_vec()
            };
            assert_eq!(data.len(), orig);
            assert_eq!(checksum(&data), be32(e + 16) as u32, "checksum");
            (tag, data)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn woff_holds_every_table_of_the_font() {
        let font = kernest_core::sample::sample_font();
        let out = tempfile::tempdir().unwrap();
        let r = crate::export(&font, &crate::ExportOptions::ttf(false, true, out.path())).unwrap();
        let ttf = std::fs::read(&r.files[0].path).unwrap();
        let woff = to_woff(&ttf).unwrap();
        assert_eq!(&woff[..4], b"wOFF");
        assert_eq!(&woff[4..8], &ttf[..4], "flavor");
        assert_eq!(
            u32::from_be_bytes(woff[8..12].try_into().unwrap()) as usize,
            woff.len()
        );
        assert!(woff.len() < ttf.len(), "compressed");
        let f = FontRef::new(&ttf).unwrap();
        let tables = tables_of_woff(&woff);
        assert_eq!(tables.len(), f.table_directory.table_records().len());
        for (tag, data) in tables {
            let original = f
                .table_data(write_fonts::types::Tag::new(&tag))
                .unwrap()
                .as_bytes()
                .to_vec();
            assert_eq!(data, original, "{}", String::from_utf8_lossy(&tag));
        }
    }
}
