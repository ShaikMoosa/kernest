//! Colour fonts: the COLR (v1, plus v0 where it's enough) and CPAL tables.
//!
//! Before compiling, colour layers become ordinary glyphs (`A.color0`, …; see
//! [`kernest_core::color::split_color_layers`]). After compiling, these tables say how
//! to draw each colour glyph: its layer glyphs, bottom to top, each filled with a
//! palette colour or a gradient.

use std::collections::HashMap;

use kernest_core::color::{Color, ColorGlyph, ColorStop, Paint};
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::tables::colr::{
    BaseGlyph, BaseGlyphList, BaseGlyphPaint, ColorLine, ColorStop as WStop, Colr, Extend,
    Layer as V0Layer, LayerList, Paint as WPaint,
};
use write_fonts::tables::cpal::{ColorRecord, Cpal};
use write_fonts::types::{F2Dot14, FWord, GlyphId16, Tag, UfWord};
use write_fonts::FontBuilder;

/// Glyph ids by name, from a compiled TrueType font's `post` table.
pub fn glyph_ids(ttf: &[u8]) -> Result<HashMap<String, u16>, String> {
    let f = FontRef::new(ttf).map_err(|e| format!("could not read the compiled font: {e}"))?;
    let post = f
        .post()
        .map_err(|e| format!("the compiled font has no post table: {e}"))?;
    let n = f
        .maxp()
        .map_err(|e| format!("the compiled font has no maxp table: {e}"))?
        .num_glyphs();
    Ok((0..n)
        .filter_map(|g| Some((post.glyph_name(GlyphId16::new(g))?.to_string(), g)))
        .collect())
}

/// The COLR and CPAL tables for these colour glyphs.
pub fn tables(
    colored: &[ColorGlyph],
    palettes: &[Vec<Color>],
    ids: &HashMap<String, u16>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let gid = |name: &str| {
        ids.get(name)
            .copied()
            .map(GlyphId16::new)
            .ok_or_else(|| format!("glyph \"{name}\" is missing from the compiled font"))
    };
    // Base glyph records must be sorted by glyph id.
    let mut glyphs: Vec<(GlyphId16, &ColorGlyph)> = colored
        .iter()
        .filter(|c| !c.layers.is_empty())
        .map(|c| Ok((gid(&c.glyph)?, c)))
        .collect::<Result<_, String>>()?;
    glyphs.sort_by_key(|(g, _)| *g);

    let mut v0_bases = Vec::new();
    let mut v0_layers = Vec::new();
    let mut v1_bases = Vec::new();
    let mut v1_layers = Vec::new();
    for (base, c) in &glyphs {
        if c.layers.len() > 255 {
            return Err(format!("\"{}\" has more than 255 colour layers.", c.glyph));
        }
        let first = v1_layers.len() as u32;
        for (name, paint) in &c.layers {
            v1_layers.push(WPaint::glyph(paint_table(paint), gid(name)?));
        }
        v1_bases.push(BaseGlyphPaint::new(
            *base,
            WPaint::colr_layers(c.layers.len() as u8, first),
        ));
        // COLRv0 (Windows 10 and older apps) when every layer is an opaque colour.
        let solid: Option<Vec<(GlyphId16, u16)>> = c
            .layers
            .iter()
            .map(|(name, p)| match p {
                Paint::Solid { color, alpha } if *alpha >= 1.0 => {
                    Some(gid(name).ok().map(|g| (g, *color)))
                }
                _ => None,
            })
            .collect::<Option<Option<Vec<_>>>>()
            .flatten();
        if let Some(layers) = solid {
            v0_bases.push(BaseGlyph::new(
                *base,
                v0_layers.len() as u16,
                layers.len() as u16,
            ));
            v0_layers.extend(layers.into_iter().map(|(g, i)| V0Layer::new(g, i)));
        }
    }
    let mut colr = Colr::new(
        v0_bases.len() as u16,
        (!v0_bases.is_empty()).then_some(v0_bases),
        (!v0_layers.is_empty()).then_some(v0_layers.clone()),
        v0_layers.len() as u16,
    );
    colr.base_glyph_list = Some(BaseGlyphList::new(v1_bases.len() as u32, v1_bases)).into();
    colr.layer_list = Some(LayerList::new(v1_layers.len() as u32, v1_layers)).into();
    let colr =
        write_fonts::dump_table(&colr).map_err(|e| format!("could not write COLR: {e:?}"))?;

    let size = palettes.first().map_or(0, Vec::len);
    let records: Vec<ColorRecord> = palettes
        .iter()
        .flatten()
        .map(|c| ColorRecord::new(c.b, c.g, c.r, c.a))
        .collect();
    let cpal = Cpal::new(
        size as u16,
        palettes.len() as u16,
        records.len() as u16,
        Some(records),
        (0..palettes.len()).map(|i| (i * size) as u16).collect(),
    );
    let cpal =
        write_fonts::dump_table(&cpal).map_err(|e| format!("could not write CPAL: {e:?}"))?;
    Ok((colr, cpal))
}

fn f2(v: f64) -> F2Dot14 {
    F2Dot14::from_f32(v.clamp(-2.0, 1.99) as f32)
}

fn fw(v: f64) -> FWord {
    FWord::new(v.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16)
}

fn line(stops: &[ColorStop]) -> ColorLine {
    let mut stops: Vec<&ColorStop> = stops.iter().collect();
    stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    let stops: Vec<WStop> = stops
        .into_iter()
        .map(|s| WStop::new(f2(s.offset), s.color, f2(s.alpha)))
        .collect();
    ColorLine::new(Extend::Pad, stops.len() as u16, stops)
}

fn paint_table(p: &Paint) -> WPaint {
    match p {
        Paint::Solid { color, alpha } => WPaint::solid(*color, f2(*alpha)),
        Paint::Linear { start, end, stops } => {
            // p2 sets the stripes' angle: square to the gradient for a plain gradient.
            let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
            WPaint::linear_gradient(
                line(stops),
                fw(start[0]),
                fw(start[1]),
                fw(end[0]),
                fw(end[1]),
                fw(start[0] - dy),
                fw(start[1] + dx),
            )
        }
        Paint::Radial {
            center,
            radius,
            focus,
            stops,
        } => {
            let f = focus.unwrap_or(*center);
            WPaint::radial_gradient(
                line(stops),
                fw(f[0]),
                fw(f[1]),
                UfWord::new(0),
                fw(center[0]),
                fw(center[1]),
                UfWord::new(radius.round().clamp(0.0, u16::MAX as f64) as u16),
            )
        }
    }
}

/// The font with the COLR and CPAL tables added (or replaced).
pub fn add_tables(font: &[u8], colr: &[u8], cpal: &[u8]) -> Result<Vec<u8>, String> {
    let f = FontRef::new(font).map_err(|e| format!("not a valid font: {e}"))?;
    let mut b = FontBuilder::new();
    for r in f.table_directory.table_records() {
        let t = r.tag();
        if t == Tag::new(b"COLR") || t == Tag::new(b"CPAL") {
            continue;
        }
        if let Some(d) = f.table_data(t) {
            b.add_raw(t, d.as_bytes().to_vec());
        }
    }
    b.add_raw(Tag::new(b"COLR"), colr.to_vec());
    b.add_raw(Tag::new(b"CPAL"), cpal.to_vec());
    Ok(b.build())
}
