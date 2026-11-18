//! WOFF2 web fonts: every table, untransformed, compressed together with Brotli.
//!
//! The `glyf` and `loca` tables use WOFF2's "null transform" (version 3), which the
//! format allows; browsers decode it like any other WOFF2.

use std::io::Write as _;

use write_fonts::read::FontRef;

/// Tags with a one-byte code in the WOFF2 table directory, in spec order.
const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm",
    b"glyf", b"loca", b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern",
    b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC",
    b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar",
    b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty",
    b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

/// UIntBase128: big-endian groups of 7 bits, high bit set on all but the last byte.
fn base128(mut v: u32, out: &mut Vec<u8>) {
    let mut bytes = vec![(v & 0x7f) as u8];
    v >>= 7;
    while v > 0 {
        bytes.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    bytes.reverse();
    out.extend(bytes);
}

/// Converts a TrueType or CFF OpenType font to WOFF2.
pub fn to_woff2(sfnt: &[u8]) -> Result<Vec<u8>, String> {
    let font = FontRef::new(sfnt).map_err(|e| format!("could not read the font: {e}"))?;
    let flavor = font.table_directory().sfnt_version();
    let mut tables: Vec<([u8; 4], &[u8])> = font
        .table_directory()
        .table_records()
        .iter()
        .filter_map(|r| {
            let tag = r.tag();
            font.table_data(tag)
                .map(|d| (tag.to_be_bytes(), d.as_bytes()))
        })
        .collect();
    tables.sort_by_key(|t| t.0);
    // WOFF2 requires loca to immediately follow glyf in the table directory; sorting
    // by tag alone leaves head/hhea/hmtx between them.
    if let Some(g) = tables.iter().position(|t| t.0 == *b"glyf") {
        if let Some(l) = tables.iter().position(|t| t.0 == *b"loca") {
            if l != g + 1 {
                let loca = tables.remove(l);
                tables.insert(g + 1, loca);
            }
        }
    }

    let mut directory = Vec::new();
    let mut stream = Vec::new();
    let mut sfnt_size = 12 + 16 * tables.len();
    for (tag, data) in &tables {
        let known = KNOWN_TAGS.iter().position(|k| *k == tag);
        // Transform version: 0 is "none" except for glyf/loca, where 3 is "none".
        let null_transform: u8 = if tag == b"glyf" || tag == b"loca" {
            3
        } else {
            0
        };
        match known {
            Some(i) => directory.push(i as u8 | (null_transform << 6)),
            None => {
                directory.push(63 | (null_transform << 6));
                directory.extend(tag);
            }
        }
        base128(data.len() as u32, &mut directory);
        stream.extend_from_slice(data);
        sfnt_size += data.len().div_ceil(4) * 4;
    }

    let mut compressed = Vec::new();
    {
        let params = brotli::enc::BrotliEncoderParams {
            quality: 11,
            lgwin: 22,
            mode: brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_FONT,
            ..Default::default()
        };
        let mut w = brotli::CompressorWriter::with_params(&mut compressed, 4096, &params);
        w.write_all(&stream)
            .map_err(|e| format!("could not compress the font: {e}"))?;
    }

    let header_len = 48;
    let mut total = header_len + directory.len() + compressed.len();
    total = total.div_ceil(4) * 4;
    let mut out = Vec::with_capacity(total);
    out.extend(b"wOF2");
    out.extend(flavor.to_be_bytes());
    out.extend((total as u32).to_be_bytes());
    out.extend((tables.len() as u16).to_be_bytes());
    out.extend(0u16.to_be_bytes()); // reserved
    out.extend((sfnt_size as u32).to_be_bytes());
    out.extend((compressed.len() as u32).to_be_bytes());
    out.extend(1u16.to_be_bytes()); // major version (of the font file; any value)
    out.extend(0u16.to_be_bytes());
    out.extend([0u8; 20]); // no metadata or private block
    out.extend(directory);
    out.extend(compressed);
    out.resize(total, 0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base128_encoding() {
        let enc = |v| {
            let mut o = Vec::new();
            base128(v, &mut o);
            o
        };
        assert_eq!(enc(0), [0]);
        assert_eq!(enc(127), [0x7f]);
        assert_eq!(enc(128), [0x81, 0x00]);
        assert_eq!(enc(63 * 128 + 1), [0xbf, 0x01]);
    }
}
