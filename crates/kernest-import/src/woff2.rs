//! WOFF2 decoding: Brotli-decompress the table stream and rebuild an sfnt font.
//!
//! Handles the null transform (tables copied as-is, as Kernest's own WOFF2 export
//! uses) and the standard `glyf`/`loca` transform that web fonts use. The transform
//! follows the WOFF2 specification; the coordinate lookup table is from the WOFF2
//! spec (as tabulated by Allsorts, Apache-2.0).

use std::io::Cursor;

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

/// A bounds-checked big-endian reader over a byte slice.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or("the WOFF2 font is truncated.")?;
        if end > self.data.len() {
            return Err("the WOFF2 font is truncated.".into());
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// UIntBase128: big-endian groups of 7 bits, high bit set on all but the last byte.
    fn base128(&mut self) -> Result<u32, String> {
        let mut accum = 0u32;
        for i in 0..5 {
            let byte = self.u8()?;
            if i == 0 && byte == 0x80 {
                return Err("invalid WOFF2 length (leading zero).".into());
            }
            if accum >> 25 != 0 {
                return Err("invalid WOFF2 length (overflow).".into());
            }
            accum = (accum << 7) | (byte & 0x7f) as u32;
            if byte & 0x80 == 0 {
                return Ok(accum);
            }
        }
        Err("invalid WOFF2 length (too many bytes).".into())
    }

    /// A uint16 stored as a byte, with 253/254/255 escapes for larger values.
    fn u255(&mut self) -> Result<u16, String> {
        let code = self.u8()?;
        match code {
            253 => self.u16(),
            255 => Ok(self.u8()? as u16 + 253),
            254 => Ok(self.u8()? as u16 + 506),
            _ => Ok(code as u16),
        }
    }
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend(v.to_be_bytes());
}

fn put_i16(out: &mut Vec<u8>, v: i16) {
    out.extend(v.to_be_bytes());
}

fn pad4(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// Bit `i` of an MSB-first bitmap (as used by the bbox and overlap bitmaps).
fn bit(bitmap: &[u8], i: usize) -> bool {
    bitmap
        .get(i / 8)
        .is_some_and(|b| b & (0x80 >> (i % 8)) != 0)
}

/// Bytes needed for a bitmap of `n` bits, rounded up to a multiple of four.
fn bitmap_bytes(n: u16) -> usize {
    (n as usize).div_ceil(32) * 4
}

/// Converts a WOFF2 font into an sfnt (TrueType or CFF OpenType) font.
pub fn decode(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut r = Reader::new(data);
    if r.take(4)? != b"wOF2" {
        return Err("not a WOFF2 font.".into());
    }
    let flavor = r.u32()?;
    let _length = r.u32()?;
    let num_tables = r.u16()?;
    let _reserved = r.u16()?;
    let _total_sfnt_size = r.u32()?;
    let total_compressed_size = r.u32()?;
    let _major = r.u16()?;
    let _minor = r.u16()?;
    // Metadata and private block offsets/lengths (unused).
    for _ in 0..5 {
        let _ = r.u32()?;
    }
    if flavor == u32::from_be_bytes(*b"ttcf") {
        return Err("WOFF2 font collections aren't supported.".into());
    }

    struct Entry {
        tag: [u8; 4],
        transformed: bool,
        src_len: usize,
        src_offset: usize,
    }
    let mut entries = Vec::with_capacity(num_tables as usize);
    let mut src_offset = 0usize;
    for _ in 0..num_tables {
        let flags = r.u8()?;
        let transform_version = flags >> 6;
        let tag_ref = (flags & 0x3f) as usize;
        let tag = if tag_ref == 63 {
            let t = r.take(4)?;
            [t[0], t[1], t[2], t[3]]
        } else {
            *KNOWN_TAGS[tag_ref]
        };
        let orig_len = r.base128()?;
        // glyf/loca use version 3 for the null transform; other tables use version 0.
        let is_null = if tag == *b"glyf" || tag == *b"loca" {
            transform_version == 3
        } else {
            transform_version == 0
        };
        let src_len = if is_null {
            orig_len as usize
        } else {
            r.base128()? as usize
        };
        entries.push(Entry {
            tag,
            transformed: !is_null,
            src_len,
            src_offset,
        });
        src_offset += src_len;
    }

    let compressed = r.take(total_compressed_size as usize)?;
    let mut stream = Vec::with_capacity(src_offset);
    brotli_decompressor::BrotliDecompress(&mut Cursor::new(compressed), &mut stream)
        .map_err(|e| format!("could not decompress the WOFF2 font: {e}"))?;
    if stream.len() < src_offset {
        return Err("the WOFF2 table stream is truncated.".into());
    }

    // Rebuild each table. A transformed glyf also yields loca; a null-transform loca
    // is copied on its own. glyf and loca need not be adjacent in the directory.
    let mut tables: Vec<([u8; 4], Vec<u8>)> = Vec::new();
    let mut loca_done = false;
    for e in &entries {
        let src = &stream[e.src_offset..e.src_offset + e.src_len];
        if e.tag == *b"glyf" {
            if e.transformed {
                let (glyf, loca) = decode_glyf(src)?;
                tables.push((*b"glyf", glyf));
                tables.push((*b"loca", loca));
                loca_done = true;
            } else {
                tables.push((*b"glyf", src.to_vec()));
            }
        } else if e.tag == *b"loca" {
            if loca_done {
                continue;
            }
            if e.transformed {
                return Err("the WOFF2 loca table is transformed without a glyf table.".into());
            }
            tables.push((*b"loca", src.to_vec()));
        } else if e.tag == *b"hmtx" && e.transformed {
            return Err("transformed WOFF2 hmtx tables aren't supported.".into());
        } else {
            tables.push((e.tag, src.to_vec()));
        }
    }

    tables.sort_by_key(|t| t.0);
    let num = tables.len();
    let header_len = 12 + 16 * num;
    let mut body = Vec::new();
    let mut records = Vec::with_capacity(num);
    for (tag, tdata) in &tables {
        records.push((
            *tag,
            checksum(tdata),
            (header_len + body.len()) as u32,
            tdata.len() as u32,
        ));
        body.extend_from_slice(tdata);
        pad4(&mut body);
    }

    let mut out = Vec::with_capacity(header_len + body.len());
    out.extend(flavor.to_be_bytes());
    out.extend((num as u16).to_be_bytes());
    let entry_selector = (num as f64).log2().floor() as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = (num as u16) * 16 - search_range;
    out.extend(search_range.to_be_bytes());
    out.extend(entry_selector.to_be_bytes());
    out.extend(range_shift.to_be_bytes());
    for (tag, cs, offset, len) in records {
        out.extend(tag);
        out.extend(cs.to_be_bytes());
        out.extend(offset.to_be_bytes());
        out.extend(len.to_be_bytes());
    }
    out.extend(body);
    Ok(out)
}

/// Sum of big-endian 32-bit words, zero-padded (the sfnt table checksum).
fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    let (chunks, rem) = data.as_chunks::<4>();
    for c in chunks {
        sum = sum.wrapping_add(u32::from_be_bytes(*c));
    }
    if !rem.is_empty() {
        let mut b = [0u8; 4];
        b[..rem.len()].copy_from_slice(rem);
        sum = sum.wrapping_add(u32::from_be_bytes(b));
    }
    sum
}

/// (byte_count, x_bits, y_bits, delta_x, delta_y, x_negative, y_negative) per flag.
#[rustfmt::skip]
const COORD_LUT: [(u8, u8, u8, u16, u16, bool, bool); 128] = [
    (1, 0, 8, 0, 0, false, true),
    (1, 0, 8, 0, 0, false, false),
    (1, 0, 8, 0, 256, false, true),
    (1, 0, 8, 0, 256, false, false),
    (1, 0, 8, 0, 512, false, true),
    (1, 0, 8, 0, 512, false, false),
    (1, 0, 8, 0, 768, false, true),
    (1, 0, 8, 0, 768, false, false),
    (1, 0, 8, 0, 1024, false, true),
    (1, 0, 8, 0, 1024, false, false),
    (1, 8, 0, 0, 0, true, false),
    (1, 8, 0, 0, 0, false, false),
    (1, 8, 0, 256, 0, true, false),
    (1, 8, 0, 256, 0, false, false),
    (1, 8, 0, 512, 0, true, false),
    (1, 8, 0, 512, 0, false, false),
    (1, 8, 0, 768, 0, true, false),
    (1, 8, 0, 768, 0, false, false),
    (1, 8, 0, 1024, 0, true, false),
    (1, 8, 0, 1024, 0, false, false),
    (1, 4, 4, 1, 1, true, true),
    (1, 4, 4, 1, 1, false, true),
    (1, 4, 4, 1, 1, true, false),
    (1, 4, 4, 1, 1, false, false),
    (1, 4, 4, 1, 17, true, true),
    (1, 4, 4, 1, 17, false, true),
    (1, 4, 4, 1, 17, true, false),
    (1, 4, 4, 1, 17, false, false),
    (1, 4, 4, 1, 33, true, true),
    (1, 4, 4, 1, 33, false, true),
    (1, 4, 4, 1, 33, true, false),
    (1, 4, 4, 1, 33, false, false),
    (1, 4, 4, 1, 49, true, true),
    (1, 4, 4, 1, 49, false, true),
    (1, 4, 4, 1, 49, true, false),
    (1, 4, 4, 1, 49, false, false),
    (1, 4, 4, 17, 1, true, true),
    (1, 4, 4, 17, 1, false, true),
    (1, 4, 4, 17, 1, true, false),
    (1, 4, 4, 17, 1, false, false),
    (1, 4, 4, 17, 17, true, true),
    (1, 4, 4, 17, 17, false, true),
    (1, 4, 4, 17, 17, true, false),
    (1, 4, 4, 17, 17, false, false),
    (1, 4, 4, 17, 33, true, true),
    (1, 4, 4, 17, 33, false, true),
    (1, 4, 4, 17, 33, true, false),
    (1, 4, 4, 17, 33, false, false),
    (1, 4, 4, 17, 49, true, true),
    (1, 4, 4, 17, 49, false, true),
    (1, 4, 4, 17, 49, true, false),
    (1, 4, 4, 17, 49, false, false),
    (1, 4, 4, 33, 1, true, true),
    (1, 4, 4, 33, 1, false, true),
    (1, 4, 4, 33, 1, true, false),
    (1, 4, 4, 33, 1, false, false),
    (1, 4, 4, 33, 17, true, true),
    (1, 4, 4, 33, 17, false, true),
    (1, 4, 4, 33, 17, true, false),
    (1, 4, 4, 33, 17, false, false),
    (1, 4, 4, 33, 33, true, true),
    (1, 4, 4, 33, 33, false, true),
    (1, 4, 4, 33, 33, true, false),
    (1, 4, 4, 33, 33, false, false),
    (1, 4, 4, 33, 49, true, true),
    (1, 4, 4, 33, 49, false, true),
    (1, 4, 4, 33, 49, true, false),
    (1, 4, 4, 33, 49, false, false),
    (1, 4, 4, 49, 1, true, true),
    (1, 4, 4, 49, 1, false, true),
    (1, 4, 4, 49, 1, true, false),
    (1, 4, 4, 49, 1, false, false),
    (1, 4, 4, 49, 17, true, true),
    (1, 4, 4, 49, 17, false, true),
    (1, 4, 4, 49, 17, true, false),
    (1, 4, 4, 49, 17, false, false),
    (1, 4, 4, 49, 33, true, true),
    (1, 4, 4, 49, 33, false, true),
    (1, 4, 4, 49, 33, true, false),
    (1, 4, 4, 49, 33, false, false),
    (1, 4, 4, 49, 49, true, true),
    (1, 4, 4, 49, 49, false, true),
    (1, 4, 4, 49, 49, true, false),
    (1, 4, 4, 49, 49, false, false),
    (2, 8, 8, 1, 1, true, true),
    (2, 8, 8, 1, 1, false, true),
    (2, 8, 8, 1, 1, true, false),
    (2, 8, 8, 1, 1, false, false),
    (2, 8, 8, 1, 257, true, true),
    (2, 8, 8, 1, 257, false, true),
    (2, 8, 8, 1, 257, true, false),
    (2, 8, 8, 1, 257, false, false),
    (2, 8, 8, 1, 513, true, true),
    (2, 8, 8, 1, 513, false, true),
    (2, 8, 8, 1, 513, true, false),
    (2, 8, 8, 1, 513, false, false),
    (2, 8, 8, 257, 1, true, true),
    (2, 8, 8, 257, 1, false, true),
    (2, 8, 8, 257, 1, true, false),
    (2, 8, 8, 257, 1, false, false),
    (2, 8, 8, 257, 257, true, true),
    (2, 8, 8, 257, 257, false, true),
    (2, 8, 8, 257, 257, true, false),
    (2, 8, 8, 257, 257, false, false),
    (2, 8, 8, 257, 513, true, true),
    (2, 8, 8, 257, 513, false, true),
    (2, 8, 8, 257, 513, true, false),
    (2, 8, 8, 257, 513, false, false),
    (2, 8, 8, 513, 1, true, true),
    (2, 8, 8, 513, 1, false, true),
    (2, 8, 8, 513, 1, true, false),
    (2, 8, 8, 513, 1, false, false),
    (2, 8, 8, 513, 257, true, true),
    (2, 8, 8, 513, 257, false, true),
    (2, 8, 8, 513, 257, true, false),
    (2, 8, 8, 513, 257, false, false),
    (2, 8, 8, 513, 513, true, true),
    (2, 8, 8, 513, 513, false, true),
    (2, 8, 8, 513, 513, true, false),
    (2, 8, 8, 513, 513, false, false),
    (3, 12, 12, 0, 0, true, true),
    (3, 12, 12, 0, 0, false, true),
    (3, 12, 12, 0, 0, true, false),
    (3, 12, 12, 0, 0, false, false),
    (4, 16, 16, 0, 0, true, true),
    (4, 16, 16, 0, 0, false, true),
    (4, 16, 16, 0, 0, true, false),
    (4, 16, 16, 0, 0, false, false),
];

/// The seven streams (plus bitmaps) of a transformed glyf table.
struct GlyfStreams<'a> {
    num_glyphs: u16,
    index_format: u16,
    n_contour: Reader<'a>,
    n_points: Reader<'a>,
    flag: Reader<'a>,
    glyph: Reader<'a>,
    composite: Reader<'a>,
    bbox_bitmap: &'a [u8],
    bbox: Reader<'a>,
    instruction: Reader<'a>,
    overlap_bitmap: Option<&'a [u8]>,
}

/// Rebuilds the `glyf` and `loca` tables from a transformed glyf table.
fn decode_glyf(table: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut h = Reader::new(table);
    let _version = h.u16()?;
    let option_flags = h.u16()?;
    let num_glyphs = h.u16()?;
    let index_format = h.u16()?;
    let n_contour_size = h.u32()? as usize;
    let n_points_size = h.u32()? as usize;
    let flag_size = h.u32()? as usize;
    let glyph_size = h.u32()? as usize;
    let composite_size = h.u32()? as usize;
    let bbox_bitmap_size = bitmap_bytes(num_glyphs);
    let bbox_size = (h.u32()? as usize)
        .checked_sub(bbox_bitmap_size)
        .ok_or("invalid WOFF2 bbox stream size.")?;
    let instruction_size = h.u32()? as usize;
    let overlap_size = if option_flags & 1 != 0 {
        bitmap_bytes(num_glyphs)
    } else {
        0
    };

    let n_contour = h.take(n_contour_size)?;
    let n_points = h.take(n_points_size)?;
    let flag = h.take(flag_size)?;
    let glyph = h.take(glyph_size)?;
    let composite = h.take(composite_size)?;
    let bbox_bitmap = h.take(bbox_bitmap_size)?;
    let bbox = h.take(bbox_size)?;
    let instruction = h.take(instruction_size)?;
    let overlap_bitmap = if overlap_size > 0 {
        Some(h.take(overlap_size)?)
    } else {
        None
    };

    let mut s = GlyfStreams {
        num_glyphs,
        index_format,
        n_contour: Reader::new(n_contour),
        n_points: Reader::new(n_points),
        flag: Reader::new(flag),
        glyph: Reader::new(glyph),
        composite: Reader::new(composite),
        bbox_bitmap,
        bbox: Reader::new(bbox),
        instruction: Reader::new(instruction),
        overlap_bitmap,
    };

    let long_loca = s.index_format > 0;
    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u8> = Vec::new();
    for gid in 0..s.num_glyphs {
        if long_loca {
            loca.extend((glyf.len() as u32).to_be_bytes());
        } else {
            put_u16(&mut loca, (glyf.len() / 2) as u16);
        }
        let n = s.n_contour.i16()?;
        if n > 0 {
            s.simple_glyph(n, gid, &mut glyf)?;
        } else if n < 0 {
            s.composite_glyph(gid, &mut glyf)?;
        }
        pad4(&mut glyf);
    }
    if long_loca {
        loca.extend((glyf.len() as u32).to_be_bytes());
    } else {
        if glyf.len() % 2 == 1 {
            glyf.push(0);
        }
        put_u16(&mut loca, (glyf.len() / 2) as u16);
    }
    Ok((glyf, loca))
}

impl GlyfStreams<'_> {
    fn simple_glyph(&mut self, contours: i16, gid: u16, out: &mut Vec<u8>) -> Result<(), String> {
        let mut end_points = Vec::new();
        let mut flags = Vec::new();
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let mut total_points: u16 = 0;
        let overlap_flag = if self.overlap_bitmap.is_some_and(|b| bit(b, gid as usize)) {
            0x40
        } else {
            0x00
        };
        let (mut x_min, mut y_min, mut x_max, mut y_max) = (0i16, 0i16, 0i16, 0i16);
        let mut extents_set = false;
        let (mut x, mut y) = (0i16, 0i16);

        for _ in 0..contours {
            let points = self.n_points.u255()?;
            total_points += points;
            put_u16(&mut end_points, total_points - 1);
            for _ in 0..points {
                let f = self.flag.u8()?;
                let (bc, xb, yb, dx0, dy0, xneg, yneg) = COORD_LUT[(f & 0x7f) as usize];
                let data = match bc {
                    1 => self.glyph.u8()? as u32,
                    2 => self.glyph.u16()? as u32,
                    3 => ((self.glyph.u8()? as u32) << 16) | self.glyph.u16()? as u32,
                    _ => self.glyph.u32()?,
                };
                let xmask = (1u32 << xb) - 1;
                let ymask = (1u32 << yb) - 1;
                let dx_raw = ((data >> (bc * 8 - xb)) & xmask) + dx0 as u32;
                let dy_raw = ((data >> (bc * 8 - xb - yb)) & ymask) + dy0 as u32;
                let dx = if xneg {
                    -(dx_raw as i16)
                } else {
                    dx_raw as i16
                };
                let dy = if yneg {
                    -(dy_raw as i16)
                } else {
                    dy_raw as i16
                };
                x += dx;
                y += dy;
                if extents_set {
                    x_min = x_min.min(x);
                    y_min = y_min.min(y);
                    x_max = x_max.max(x);
                    y_max = y_max.max(y);
                } else {
                    (x_min, x_max, y_min, y_max) = (x, x, y, y);
                    extents_set = true;
                }

                let on_curve = if f & 0x80 == 0 { 0x01 } else { 0x00 };
                let (x_short, x_same) = match dx {
                    0 => (0x00, 0x10),
                    1..=255 => {
                        xs.push(dx as u8);
                        (0x02, 0x10)
                    }
                    -255..=-1 => {
                        xs.push((-dx) as u8);
                        (0x02, 0x00)
                    }
                    _ => {
                        put_i16(&mut xs, dx);
                        (0x00, 0x00)
                    }
                };
                let (y_short, y_same) = match dy {
                    0 => (0x00, 0x20),
                    1..=255 => {
                        ys.push(dy as u8);
                        (0x04, 0x20)
                    }
                    -255..=-1 => {
                        ys.push((-dy) as u8);
                        (0x04, 0x00)
                    }
                    _ => {
                        put_i16(&mut ys, dy);
                        (0x00, 0x00)
                    }
                };
                flags.push(on_curve | x_short | y_short | x_same | y_same | overlap_flag);
            }
        }

        let instruction_length = self.glyph.u255()?;
        let instructions = self.instruction.take(instruction_length as usize)?;
        if bit(self.bbox_bitmap, gid as usize) {
            x_min = self.bbox.i16()?;
            y_min = self.bbox.i16()?;
            x_max = self.bbox.i16()?;
            y_max = self.bbox.i16()?;
        }

        put_i16(out, contours);
        put_i16(out, x_min);
        put_i16(out, y_min);
        put_i16(out, x_max);
        put_i16(out, y_max);
        out.extend_from_slice(&end_points);
        put_u16(out, instruction_length);
        out.extend_from_slice(instructions);
        out.extend_from_slice(&flags);
        out.extend_from_slice(&xs);
        out.extend_from_slice(&ys);
        Ok(())
    }

    fn composite_glyph(&mut self, gid: u16, out: &mut Vec<u8>) -> Result<(), String> {
        put_i16(out, -1);
        if !bit(self.bbox_bitmap, gid as usize) {
            return Err("a WOFF2 composite glyph has no bounding box.".into());
        }
        put_i16(out, self.bbox.i16()?);
        put_i16(out, self.bbox.i16()?);
        put_i16(out, self.bbox.i16()?);
        put_i16(out, self.bbox.i16()?);

        let mut have_instructions = false;
        loop {
            let flag_word = self.composite.u16()?;
            let mut num_bytes = 4usize;
            if flag_word & 0x0001 != 0 {
                num_bytes += 2;
            }
            if flag_word & 0x0008 != 0 {
                num_bytes += 2;
            } else if flag_word & 0x0040 != 0 {
                num_bytes += 4;
            } else if flag_word & 0x0080 != 0 {
                num_bytes += 8;
            }
            put_u16(out, flag_word);
            out.extend_from_slice(self.composite.take(num_bytes)?);
            if flag_word & 0x0100 != 0 {
                have_instructions = true;
            }
            if flag_word & 0x0020 == 0 {
                break;
            }
        }
        if have_instructions {
            let instruction_length = self.glyph.u255()?;
            put_u16(out, instruction_length);
            out.extend_from_slice(self.instruction.take(instruction_length as usize)?);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base128_decoding() {
        let dec = |b: &[u8]| Reader::new(b).base128().unwrap();
        assert_eq!(dec(&[0]), 0);
        assert_eq!(dec(&[0x7f]), 127);
        assert_eq!(dec(&[0x81, 0x00]), 128);
        assert_eq!(dec(&[0xbf, 0x01]), 63 * 128 + 1);
    }

    #[test]
    fn u255_decoding() {
        let dec = |b: &[u8]| Reader::new(b).u255().unwrap();
        assert_eq!(dec(&[0]), 0);
        assert_eq!(dec(&[128]), 128);
        assert_eq!(dec(&[255, 253]), 506);
        assert_eq!(dec(&[254, 0]), 506);
        assert_eq!(dec(&[253, 1, 250]), 506);
    }
}
