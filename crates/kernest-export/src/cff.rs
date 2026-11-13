//! PostScript-flavoured OpenType (`.otf`): a CFF table built from the source's cubic
//! outlines, swapped into the TrueType font fontc compiled (which provides every other
//! table: cmap, metrics, names, GSUB/GPOS…).
//!
//! The outlines are written unhinted, one operator per segment, with no subroutines.
//! That keeps the table simple and exact; files are a little larger than a fully
//! optimised CFF.

use std::collections::{BTreeMap, HashMap};

use norad::{Glyph, PointType};
use write_fonts::read::ps::Sid;
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::types::Tag;
use write_fonts::FontBuilder;

use crate::charstring::{self, Format, GlyphOps, Ops};

pub(crate) type Pt = (f64, f64);

/// One closed contour: a start point and the segments that follow it.
pub(crate) struct Contour {
    pub(crate) start: Pt,
    pub(crate) segs: Vec<Seg>,
}

pub(crate) enum Seg {
    Line(Pt),
    Cubic(Pt, Pt, Pt),
}

pub(crate) fn apply(t: &[f64; 6], (x, y): Pt) -> Pt {
    (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
}

pub(crate) fn compose(o: &[f64; 6], i: &[f64; 6]) -> [f64; 6] {
    [
        o[0] * i[0] + o[2] * i[1],
        o[1] * i[0] + o[3] * i[1],
        o[0] * i[2] + o[2] * i[3],
        o[1] * i[2] + o[3] * i[3],
        o[0] * i[4] + o[2] * i[5] + o[4],
        o[1] * i[4] + o[3] * i[5] + o[5],
    ]
}

fn lerp(a: Pt, b: Pt, t: f64) -> Pt {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// A quadratic piece as a cubic (exact).
fn quad_to_cubic(p0: Pt, q: Pt, p2: Pt) -> Seg {
    Seg::Cubic(lerp(p0, q, 2.0 / 3.0), lerp(p2, q, 2.0 / 3.0), p2)
}

/// A closed contour from its points (UFO point types), starting on an on-curve point;
/// quadratic splines become cubics. `None` for open paths, which CFF can't hold.
pub(crate) fn ring_contour(pts: Vec<(PointType, Pt)>) -> Option<Contour> {
    if pts.is_empty() || pts[0].0 == PointType::Move {
        return None; // CFF has no open paths
    }
    // Start on an on-curve point; an all-off-curve (TrueType) contour starts at the
    // implied midpoint of its last and first points.
    let (start, ring): (Pt, Vec<(PointType, Pt)>) =
        match pts.iter().position(|(k, _)| *k != PointType::OffCurve) {
            Some(i) => {
                let mut r = pts[i + 1..].to_vec();
                r.extend_from_slice(&pts[..=i]);
                (pts[i].1, r)
            }
            None => {
                let s = lerp(pts[pts.len() - 1].1, pts[0].1, 0.5);
                let mut r = pts.clone();
                r.push((PointType::QCurve, s));
                (s, r)
            }
        };
    let mut segs = Vec::new();
    let mut cur = start;
    let mut offs: Vec<Pt> = Vec::new();
    for (kind, p) in ring {
        match kind {
            PointType::OffCurve => offs.push(p),
            PointType::Curve if offs.len() >= 2 => {
                segs.push(Seg::Cubic(offs[0], offs[offs.len() - 1], p));
                offs.clear();
                cur = p;
            }
            PointType::Curve | PointType::QCurve if !offs.is_empty() => {
                // Quadratic spline: implied on-curve points between handles.
                for (k, q) in offs.iter().enumerate() {
                    let end = match offs.get(k + 1) {
                        Some(next) => lerp(*q, *next, 0.5),
                        None => p,
                    };
                    segs.push(quad_to_cubic(cur, *q, end));
                    cur = end;
                }
                offs.clear();
            }
            _ => {
                segs.push(Seg::Line(p));
                offs.clear();
                cur = p;
            }
        }
    }
    Some(Contour { start, segs })
}

/// Every closed contour of `g` with components decomposed, transformed by `t`.
fn contours(ufo: &norad::Font, g: &Glyph, t: [f64; 6], depth: usize, out: &mut Vec<Contour>) {
    if depth > 32 {
        return;
    }
    for c in &g.contours {
        let pts: Vec<(PointType, Pt)> = c
            .points
            .iter()
            .map(|p| (p.typ, apply(&t, (p.x, p.y))))
            .collect();
        if let Some(c) = ring_contour(pts) {
            out.push(c);
        }
    }
    for comp in &g.components {
        let ct = &comp.transform;
        let inner = [
            ct.x_scale,
            ct.xy_scale,
            ct.yx_scale,
            ct.y_scale,
            ct.x_offset,
            ct.y_offset,
        ];
        if let Some(base) = ufo.get_glyph(comp.base.as_str()) {
            contours(ufo, base, compose(&t, &inner), depth + 1, out);
        }
    }
}

/// A Type 2 charstring / DICT integer operand.
pub(crate) fn int(v: i32, out: &mut Vec<u8>) {
    match v {
        -107..=107 => out.push((v + 139) as u8),
        108..=1131 => {
            let w = v - 108;
            out.extend([((w >> 8) + 247) as u8, (w & 0xff) as u8]);
        }
        -1131..=-108 => {
            let w = -v - 108;
            out.extend([((w >> 8) + 251) as u8, (w & 0xff) as u8]);
        }
        _ => {
            let v = v.clamp(-32768, 32767) as i16;
            out.push(28);
            out.extend(v.to_be_bytes());
        }
    }
}

/// A DICT integer that always takes five bytes, so offsets can be filled in later.
pub(crate) fn int5(v: i32, out: &mut Vec<u8>) {
    out.push(29);
    out.extend(v.to_be_bytes());
}

pub(crate) fn op(code: u8, out: &mut Vec<u8>) {
    out.push(code);
}

/// The charstring for one glyph, and its points' bounds.
/// A glyph's plain operations (rmoveto / rlineto / rrcurveto with relative, rounded
/// operands) and the bounds of its points.
fn glyph_ops(contours: &[Contour]) -> (Ops, Option<[i32; 4]>) {
    let mut ops: Ops = Vec::new();
    let mut bounds: Option<[i32; 4]> = None;
    let mut grow = |x: i32, y: i32| {
        let b = bounds.get_or_insert([x, y, x, y]);
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    };
    let r = |p: Pt| (p.0.round() as i32, p.1.round() as i32);
    let (mut px, mut py) = (0, 0);
    for c in contours {
        let (sx, sy) = r(c.start);
        ops.push((21, vec![vec![sx - px], vec![sy - py]]));
        grow(sx, sy);
        (px, py) = (sx, sy);
        let n = c.segs.len();
        for (i, s) in c.segs.iter().enumerate() {
            match s {
                Seg::Line(p) => {
                    let (x, y) = r(*p);
                    // The path closes itself with a line back to its start.
                    if i + 1 == n && (x, y) == (sx, sy) {
                        continue;
                    }
                    ops.push((5, vec![vec![x - px], vec![y - py]]));
                    grow(x, y);
                    (px, py) = (x, y);
                }
                Seg::Cubic(a, b, p) => {
                    let mut args = Vec::with_capacity(6);
                    for q in [*a, *b, *p] {
                        let (x, y) = r(q);
                        args.push(vec![x - px]);
                        args.push(vec![y - py]);
                        grow(x, y);
                        (px, py) = (x, y);
                    }
                    ops.push((8, args));
                }
            }
        }
    }
    (ops, bounds)
}

/// The CFF string id (SID) of each standard string, such as "A" or "exclam".
fn standard_sids() -> HashMap<&'static [u8], u16> {
    (0..391u16)
        .filter_map(|i| Sid::new(i).resolve_standard().ok().map(|s| (s, i)))
        .collect()
}

/// A charset in format 2: runs of consecutive string ids (gid 0, .notdef, is implied).
fn charset_ranges(sids: &[u16]) -> Vec<u8> {
    let mut out = vec![2u8];
    let mut i = 0;
    while i < sids.len() {
        let mut j = i;
        while j + 1 < sids.len() && sids[j + 1] == sids[j] + 1 {
            j += 1;
        }
        out.extend(sids[i].to_be_bytes());
        out.extend(((j - i) as u16).to_be_bytes());
        i = j + 1;
    }
    out
}

/// The most common value (the smallest among ties), or 0.
pub(crate) fn most_common(values: &[i32]) -> i32 {
    let mut counts: BTreeMap<i32, usize> = BTreeMap::new();
    for v in values {
        *counts.entry(*v).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(v, _)| v)
        .unwrap_or(0)
}

/// A CFF INDEX of `items`.
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let total: usize = items.iter().map(Vec::len).sum::<usize>() + 1;
    let off_size: u8 = match total {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x10000..=0xff_ffff => 3,
        _ => 4,
    };
    let mut out = Vec::with_capacity(3 + (items.len() + 1) * off_size as usize + total);
    out.extend((items.len() as u16).to_be_bytes());
    out.push(off_size);
    let mut offset = 1usize;
    let push = |out: &mut Vec<u8>, v: usize| {
        let b = (v as u32).to_be_bytes();
        out.extend(&b[4 - off_size as usize..]);
    };
    push(&mut out, offset);
    for it in items {
        offset += it.len();
        push(&mut out, offset);
    }
    for it in items {
        out.extend(it);
    }
    out
}

/// Builds a CFF table for the glyphs in `order` (glyph ids follow it), taking outlines
/// from `ufo` and advance widths from `widths`.
pub fn build_cff(
    ufo: &norad::Font,
    order: &[String],
    widths: &[u16],
    ps_name: &str,
    full_name: &str,
    family_name: &str,
) -> Result<Vec<u8>, String> {
    build_cff_with(ufo, order, widths, ps_name, full_name, family_name, true)
}

/// [`build_cff`], with size optimisation (compact operators, subroutines) on or off.
pub(crate) fn build_cff_with(
    ufo: &norad::Font,
    order: &[String],
    widths: &[u16],
    ps_name: &str,
    full_name: &str,
    family_name: &str,
    optimize: bool,
) -> Result<Vec<u8>, String> {
    // Strings: standard names use their built-in ids; others get ids from 391 up.
    let standard = standard_sids();
    let mut strings: Vec<Vec<u8>> = Vec::new();
    let mut sid = |s: &str| -> u16 {
        if let Some(&i) = standard.get(s.as_bytes()) {
            return i;
        }
        strings.push(s.as_bytes().to_vec());
        390 + strings.len() as u16
    };
    let full_sid = sid(full_name);
    let family_sid = sid(family_name);

    // The most common width is the default (left out of charstrings) and the base
    // the others are written relative to.
    let widths: Vec<i32> = (0..order.len())
        .map(|gid| i32::from(*widths.get(gid).unwrap_or(&0)))
        .collect();
    let (default_width, nominal_width) = if optimize {
        let w = most_common(&widths);
        (w, w)
    } else {
        (0, 0)
    };

    let mut glyphs = Vec::with_capacity(order.len());
    let mut glyph_sids = Vec::with_capacity(order.len());
    let mut bbox: Option<[i32; 4]> = None;
    for (gid, name) in order.iter().enumerate() {
        let g = ufo
            .get_glyph(name.as_str())
            .ok_or_else(|| format!("glyph \"{name}\" is missing from the source"))?;
        let mut cs = Vec::new();
        contours(ufo, g, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0, &mut cs);
        let (ops, b) = glyph_ops(&cs);
        if let Some(b) = b {
            let bb = bbox.get_or_insert(b);
            *bb = [
                bb[0].min(b[0]),
                bb[1].min(b[1]),
                bb[2].max(b[2]),
                bb[3].max(b[3]),
            ];
        }
        let w = widths[gid];
        glyphs.push(GlyphOps {
            ops,
            width: (w != default_width).then_some(w - nominal_width),
        });
        if gid > 0 {
            glyph_sids.push(sid(name));
        }
    }
    let (charstrings, subrs) = charstring::encode(&glyphs, Format::Cff, 0, optimize);
    let charset = charset_ranges(&glyph_sids);
    let bbox = bbox.unwrap_or([0, 0, 0, 0]);

    // Private DICT: widths, and the local subroutines placed right after it.
    let mut private = Vec::new();
    int(default_width, &mut private);
    op(20, &mut private); // defaultWidthX
    int(nominal_width, &mut private);
    op(21, &mut private); // nominalWidthX
    if !subrs.is_empty() {
        let len = private.len() + 6;
        int5(len as i32, &mut private);
        op(19, &mut private); // Subrs, relative to the Private DICT
    }
    let subrs_index = if subrs.is_empty() {
        Vec::new()
    } else {
        index(&subrs)
    };

    let top = |charset_off: i32, cs_off: i32, priv_off: i32| -> Vec<u8> {
        let mut d = Vec::new();
        int(i32::from(full_sid), &mut d);
        op(2, &mut d); // FullName
        int(i32::from(family_sid), &mut d);
        op(3, &mut d); // FamilyName
        for v in bbox {
            int(v, &mut d);
        }
        op(5, &mut d); // FontBBox
        int5(charset_off, &mut d);
        op(15, &mut d); // charset
        int5(cs_off, &mut d);
        op(17, &mut d); // CharStrings
        int5(private.len() as i32, &mut d);
        int5(priv_off, &mut d);
        op(18, &mut d); // Private
        d
    };
    let header = [1u8, 0, 4, 4];
    let name_index = index(&[ps_name.as_bytes().to_vec()]);
    let string_index = index(&strings);
    let gsubrs = index(&[]);
    let charstrings_index = index(&charstrings);
    let top_len = index(&[top(0, 0, 0)]).len();
    let charset_off = header.len() + name_index.len() + top_len + string_index.len() + gsubrs.len();
    let cs_off = charset_off + charset.len();
    let priv_off = cs_off + charstrings_index.len();
    let top_index = index(&[top(charset_off as i32, cs_off as i32, priv_off as i32)]);
    debug_assert_eq!(top_index.len(), top_len);

    let mut out = Vec::with_capacity(priv_off + private.len() + subrs_index.len());
    out.extend(header);
    out.extend(name_index);
    out.extend(top_index);
    out.extend(string_index);
    out.extend(gsubrs);
    out.extend(charset);
    out.extend(charstrings_index);
    out.extend(private);
    out.extend(subrs_index);
    Ok(out)
}

/// A PostScript name: printable ASCII without spaces or `[](){}<>/%`, at most 63 bytes.
pub fn ps_name(s: &str) -> String {
    let n: String = s
        .chars()
        .filter(|c| c.is_ascii_graphic() && !"[](){}<>/%".contains(*c))
        .take(63)
        .collect();
    if n.is_empty() {
        "Untitled".into()
    } else {
        n
    }
}

/// Turns a static TrueType font compiled from `ufo` into a CFF-flavoured OpenType font.
pub fn ttf_to_otf(
    ttf: &[u8],
    ufo: &norad::Font,
    family: &str,
    style: &str,
) -> Result<Vec<u8>, String> {
    let font = FontRef::new(ttf).map_err(|e| format!("could not read the compiled font: {e}"))?;
    let num_glyphs = font
        .maxp()
        .map_err(|e| format!("the compiled font has no maxp table: {e}"))?
        .num_glyphs();
    let post = font
        .post()
        .map_err(|e| format!("the compiled font has no post table: {e}"))?;
    let order: Vec<String> = (0..num_glyphs)
        .map(|gid| {
            post.glyph_name(write_fonts::types::GlyphId16::new(gid))
                .map(|n| n.to_string())
                .ok_or_else(|| format!("glyph {gid} has no name in the compiled font"))
        })
        .collect::<Result<_, _>>()?;
    let hmtx = font
        .hmtx()
        .map_err(|e| format!("the compiled font has no hmtx table: {e}"))?;
    let widths: Vec<u16> = (0..num_glyphs)
        .map(|gid| {
            hmtx.advance(write_fonts::types::GlyphId::new(u32::from(gid)))
                .unwrap_or(0)
        })
        .collect();
    let full = format!("{family} {style}");
    let cff = build_cff(
        ufo,
        &order,
        &widths,
        &ps_name(&format!("{family}-{style}")),
        &full,
        family,
    )?;
    // maxp version 0.5: just the glyph count.
    let mut maxp = vec![0x00, 0x00, 0x50, 0x00];
    maxp.extend(num_glyphs.to_be_bytes());

    let skip = [
        b"glyf", b"loca", b"cvt ", b"fpgm", b"prep", b"gasp", b"maxp",
    ];
    let mut b = FontBuilder::new();
    for rec in font.table_directory().table_records() {
        let tag = rec.tag();
        if skip.iter().any(|s| tag == Tag::new(s)) {
            continue;
        }
        if let Some(data) = font.table_data(tag) {
            b.add_raw(tag, data.as_bytes().to_vec());
        }
    }
    b.add_raw(Tag::new(b"CFF "), cff);
    b.add_raw(Tag::new(b"maxp"), maxp);
    Ok(b.build())
}
