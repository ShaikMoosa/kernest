//! Variable PostScript-flavoured OpenType (`-VF.otf`): a CFF2 table whose charstrings
//! blend between sources, swapped into the variable TrueType font fontc compiled
//! (which provides fvar, HVAR, STAT, GSUB/GPOS and the rest).
//!
//! Every master and every brace-layer location is a source, so intermediate layers
//! shape the outlines just as in the variable TrueType font. Bracket alternates are
//! separate glyphs (`A.BRACKET.1`), swapped in by GSUB as before.

use std::collections::BTreeMap;

use kernest_core::designspace::{normalize, Location, VariationModel};
use kernest_core::interpolate::Interpolator;
use kernest_core::{AxisRange, Font, Layer, Node, NodeKind, Shape};
use norad::PointType;
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::tables::variations::{
    ItemVariationData, ItemVariationStore, RegionAxisCoordinates, VariationRegion,
    VariationRegionList,
};
use write_fonts::types::{F2Dot14, Tag};
use write_fonts::FontBuilder;

use crate::cff::{apply, compose, int, int5, op, ring_contour, Contour, Pt, Seg};

/// CFF2 allows 513 operands on the stack; a curve with k regions needs 6(k+1)+1.
const MAX_REGIONS: usize = 84;

/// Where a glyph's outlines come from in each source.
pub enum GlyphSource<'a> {
    /// A Kernest glyph, normal design.
    Glyph(&'a str),
    /// A Kernest glyph's alternate (bracket layers with these ranges).
    Alternate(&'a str, &'a [AxisRange]),
    /// Fixed outlines that don't vary (such as a generated `.notdef`).
    Static(&'a norad::Glyph),
}

fn kind(k: NodeKind) -> PointType {
    match k {
        NodeKind::Line => PointType::Line,
        NodeKind::Curve => PointType::Curve,
        NodeKind::QCurve => PointType::QCurve,
        NodeKind::OffCurve => PointType::OffCurve,
    }
}

/// The closed contours of a Kernest layer in one source, components decomposed with
/// their base glyphs' normal design in that same source.
fn layer_contours(
    font: &Font,
    interp: &Interpolator,
    layer: &Layer,
    t: [f64; 6],
    depth: usize,
    out: &mut Vec<Contour>,
) -> Result<(), String> {
    if depth > 32 {
        return Ok(());
    }
    for s in &layer.shapes {
        match s {
            Shape::Path(p) => {
                if !p.closed {
                    continue;
                }
                let pts: Vec<(PointType, Pt)> = p
                    .nodes
                    .iter()
                    .map(|n: &Node| (kind(n.kind), apply(&t, (n.x, n.y))))
                    .collect();
                if let Some(c) = ring_contour(pts) {
                    out.push(c);
                }
            }
            Shape::Component { base, transform } => {
                let bl = interp.layer_design(font, base, None)?;
                layer_contours(font, interp, &bl, compose(&t, transform), depth + 1, out)?;
            }
        }
    }
    Ok(())
}

fn static_contours(g: &norad::Glyph) -> Vec<Contour> {
    g.contours
        .iter()
        .filter_map(|c| ring_contour(c.points.iter().map(|p| (p.typ, (p.x, p.y))).collect()))
        .collect()
}

/// A glyph's outline as a list of operations, each with its operands (coordinates
/// relative to the previous point, rounded to units): (operator, operands).
fn operations(contours: &[Contour]) -> Vec<(u8, Vec<f64>)> {
    let mut ops = Vec::new();
    let (mut px, mut py) = (0.0, 0.0);
    let rel = |p: Pt, px: &mut f64, py: &mut f64| {
        let (x, y) = (p.0.round(), p.1.round());
        let d = [x - *px, y - *py];
        (*px, *py) = (x, y);
        d
    };
    for c in contours {
        ops.push((21, rel(c.start, &mut px, &mut py).to_vec())); // rmoveto
        for s in &c.segs {
            match s {
                Seg::Line(p) => ops.push((5, rel(*p, &mut px, &mut py).to_vec())), // rlineto
                Seg::Cubic(a, b, p) => {
                    let mut v = rel(*a, &mut px, &mut py).to_vec();
                    v.extend(rel(*b, &mut px, &mut py));
                    v.extend(rel(*p, &mut px, &mut py));
                    ops.push((8, v)); // rrcurveto
                }
            }
        }
    }
    ops
}

/// A CFF2 INDEX (32-bit count).
fn index2(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = (items.len() as u32).to_be_bytes().to_vec();
    if items.is_empty() {
        return out;
    }
    let total: usize = items.iter().map(Vec::len).sum::<usize>() + 1;
    let off_size: u8 = match total {
        0..=0xff => 1,
        0x100..=0xffff => 2,
        0x10000..=0xff_ffff => 3,
        _ => 4,
    };
    out.push(off_size);
    let push = |out: &mut Vec<u8>, v: usize| {
        let b = (v as u32).to_be_bytes();
        out.extend(&b[4 - off_size as usize..]);
    };
    let mut offset = 1usize;
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

/// The source locations: every master, then every distinct brace-layer location.
fn source_locations(font: &Font) -> Vec<Location> {
    let mut locs: Vec<Location> = font.masters.iter().map(|m| m.location.clone()).collect();
    for g in &font.glyphs {
        for l in g.brace_layers() {
            let loc = l.location.clone().unwrap_or_default();
            if !locs
                .iter()
                .any(|x| normalize(font, x) == normalize(font, &loc))
            {
                locs.push(loc);
            }
        }
    }
    locs
}

/// Builds the CFF2 table for the glyphs `order` (glyph ids follow it).
pub fn build_cff2(font: &Font, order: &[GlyphSource]) -> Result<Vec<u8>, String> {
    let sources = source_locations(font);
    let axis_order: Vec<String> = font.axes.iter().map(|a| a.tag.clone()).collect();
    let norm: Vec<Location> = sources.iter().map(|l| normalize(font, l)).collect();
    let model = VariationModel::new(&norm, &axis_order)?;
    let regions: Vec<_> = model.supports().iter().skip(1).cloned().collect();
    if regions.len() > MAX_REGIONS {
        return Err(format!(
            "variable OTF supports up to {MAX_REGIONS} variation regions; this font has {}.",
            regions.len()
        ));
    }
    let interps: Vec<Interpolator> = sources
        .iter()
        .map(|l| Interpolator::new(font, l))
        .collect::<Result<_, _>>()?;

    let mut charstrings = Vec::with_capacity(order.len());
    for src in order {
        // The outline's operations in every source.
        let per_source: Vec<Vec<(u8, Vec<f64>)>> = match src {
            GlyphSource::Static(g) => vec![operations(&static_contours(g)); sources.len()],
            GlyphSource::Glyph(name) | GlyphSource::Alternate(name, _) => {
                let alt = match src {
                    GlyphSource::Alternate(_, r) => Some(*r),
                    _ => None,
                };
                interps
                    .iter()
                    .map(|i| {
                        let layer = i.layer_design(font, name, alt)?;
                        let mut cs = Vec::new();
                        layer_contours(font, i, &layer, kernest_core::IDENTITY, 0, &mut cs)?;
                        Ok(operations(&cs))
                    })
                    .collect::<Result<_, String>>()?
            }
        };
        let name = match src {
            GlyphSource::Glyph(n) | GlyphSource::Alternate(n, _) => *n,
            GlyphSource::Static(g) => g.name().as_str(),
        };
        let base = &per_source[0];
        if per_source.iter().any(|ops| {
            ops.len() != base.len()
                || ops
                    .iter()
                    .zip(base)
                    .any(|(a, b)| a.0 != b.0 || a.1.len() != b.1.len())
        }) {
            return Err(format!(
                "\"{name}\" has different outlines in different masters, so it can't vary."
            ));
        }
        // Flatten each source's operands, get the deltas per region, and write each
        // operation as defaults (+ deltas and `blend` where anything varies).
        let flat: Vec<Vec<f64>> = per_source
            .iter()
            .map(|ops| ops.iter().flat_map(|(_, v)| v.iter().copied()).collect())
            .collect();
        let deltas = model.deltas(&flat);
        let mut cs = Vec::new();
        let mut at = 0;
        for (code, args) in base {
            let n = args.len();
            let defaults: Vec<i32> = (0..n).map(|k| deltas[0][at + k].round() as i32).collect();
            let region_deltas: Vec<Vec<i32>> = (0..n)
                .map(|k| {
                    deltas[1..]
                        .iter()
                        .map(|d| d[at + k].round() as i32)
                        .collect()
                })
                .collect();
            for v in &defaults {
                int(*v, &mut cs);
            }
            if region_deltas.iter().flatten().any(|d| *d != 0) {
                for d in region_deltas.iter().flatten() {
                    int(*d, &mut cs);
                }
                int(n as i32, &mut cs);
                op(16, &mut cs); // blend
            }
            op(*code, &mut cs);
            at += n;
        }
        charstrings.push(cs);
    }

    // Variation store: all regions, in fvar axis order; one data block using them all.
    let region_list = VariationRegionList::new(
        font.axes.len() as u16,
        regions
            .iter()
            .map(|r| {
                VariationRegion::new(
                    font.axes
                        .iter()
                        .map(|a| {
                            let (s, p, e) = r.get(&a.tag).copied().unwrap_or((0.0, 0.0, 0.0));
                            RegionAxisCoordinates::new(
                                F2Dot14::from_f32(s as f32),
                                F2Dot14::from_f32(p as f32),
                                F2Dot14::from_f32(e as f32),
                            )
                        })
                        .collect(),
                )
            })
            .collect(),
    );
    let data = ItemVariationData::new(0, 0, (0..regions.len() as u16).collect(), Vec::new());
    let store = ItemVariationStore::new(region_list, vec![Some(data)]);
    let store_bytes = write_fonts::dump_table(&store)
        .map_err(|e| format!("could not write the variation store: {e}"))?;
    let mut vstore = (store_bytes.len() as u16).to_be_bytes().to_vec();
    vstore.extend(store_bytes);

    let private: Vec<u8> = vec![139, 22]; // vsindex 0
    let top = |cs_off: i32, fd_off: i32, vs_off: i32| -> Vec<u8> {
        let mut d = Vec::new();
        int5(cs_off, &mut d);
        op(17, &mut d); // CharStrings
        int5(fd_off, &mut d);
        d.extend([12, 36]); // FDArray
        int5(vs_off, &mut d);
        op(24, &mut d); // vstore
        d
    };
    let font_dict = |priv_off: i32| -> Vec<u8> {
        let mut d = Vec::new();
        int5(private.len() as i32, &mut d);
        int5(priv_off, &mut d);
        op(18, &mut d); // Private
        d
    };
    let top_len = top(0, 0, 0).len();
    let header_len = 5;
    let gsubrs = index2(&[]);
    let cs_index = index2(&charstrings);
    let vs_off = header_len + top_len + gsubrs.len();
    let cs_off = vs_off + vstore.len();
    let fd_off = cs_off + cs_index.len();
    let fd_len = index2(&[font_dict(0)]).len();
    let priv_off = fd_off + fd_len;

    let mut out = vec![2, 0, header_len as u8];
    out.extend((top_len as u16).to_be_bytes());
    out.extend(top(cs_off as i32, fd_off as i32, vs_off as i32));
    out.extend(gsubrs);
    out.extend(vstore);
    out.extend(cs_index);
    out.extend(index2(&[font_dict(priv_off as i32)]));
    out.extend(private);
    Ok(out)
}

/// Turns the variable TrueType font fontc compiled into a CFF2-flavoured variable
/// OpenType font. `alternates` maps generated alternate names to their glyph and
/// ranges; `statics` holds fixed glyphs such as the generated `.notdef`.
pub fn vf_to_otf(
    vf: &[u8],
    font: &Font,
    alternates: &BTreeMap<String, (String, Vec<AxisRange>)>,
    statics: &norad::Font,
) -> Result<Vec<u8>, String> {
    let ttf = FontRef::new(vf).map_err(|e| format!("could not read the variable font: {e}"))?;
    let num_glyphs = ttf
        .maxp()
        .map_err(|e| format!("the variable font has no maxp table: {e}"))?
        .num_glyphs();
    let post = ttf
        .post()
        .map_err(|e| format!("the variable font has no post table: {e}"))?;
    let names: Vec<String> = (0..num_glyphs)
        .map(|gid| {
            post.glyph_name(write_fonts::types::GlyphId16::new(gid))
                .map(|n| n.to_string())
                .ok_or_else(|| format!("glyph {gid} has no name in the variable font"))
        })
        .collect::<Result<_, _>>()?;
    let order: Vec<GlyphSource> = names
        .iter()
        .map(|n| {
            if font.glyph(n).is_some() {
                Ok(GlyphSource::Glyph(n.as_str()))
            } else if let Some((base, r)) = alternates.get(n) {
                Ok(GlyphSource::Alternate(base.as_str(), r.as_slice()))
            } else {
                statics
                    .get_glyph(n.as_str())
                    .map(GlyphSource::Static)
                    .ok_or_else(|| format!("glyph \"{n}\" is missing from the sources"))
            }
        })
        .collect::<Result<_, String>>()?;
    let cff2 = build_cff2(font, &order)?;

    let mut maxp = vec![0x00, 0x00, 0x50, 0x00];
    maxp.extend(num_glyphs.to_be_bytes());
    let skip = [
        b"glyf", b"loca", b"gvar", b"cvar", b"cvt ", b"fpgm", b"prep", b"gasp", b"maxp",
    ];
    let mut b = FontBuilder::new();
    for rec in ttf.table_directory().table_records() {
        let tag = rec.tag();
        if skip.iter().any(|s| tag == Tag::new(s)) {
            continue;
        }
        if let Some(data) = ttf.table_data(tag) {
            b.add_raw(tag, data.as_bytes().to_vec());
        }
    }
    b.add_raw(Tag::new(b"CFF2"), cff2);
    b.add_raw(Tag::new(b"maxp"), maxp);
    Ok(b.build())
}
