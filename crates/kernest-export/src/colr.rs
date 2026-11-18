//! Colour fonts: the COLR (v1, plus v0 where it's enough) and CPAL tables.
//!
//! Before compiling, colour layers become ordinary glyphs (`A.color0`, …; see
//! [`kernest_core::color::split_color_layers`]). After compiling, these tables say how
//! to draw each colour glyph: its layer glyphs, bottom to top, each filled with a
//! palette colour or a gradient.
//!
//! When colour differs between masters the variable font carries `PaintVar*` paints
//! whose geometry, stop positions and opacities vary through an ItemVariationStore.
//! A palette colour's RGB cannot vary (CPAL indices are static), so a colour that
//! changes between masters is cross-faded: stacked layers whose opacities fade from
//! one to the next, exact at every master and blended in between (owner decision).

use std::collections::HashMap;

use kernest_core::color::{Color, ColorGlyph, ColorLayer, ColorStop, Paint};
use kernest_core::designspace::{normalize, VariationModel};
use kernest_core::model::Font;
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::tables::colr::{
    BaseGlyph, BaseGlyphList, BaseGlyphPaint, ColorLine, ColorStop as WStop, Colr, Extend,
    Layer as V0Layer, LayerList, Paint as WPaint, VarColorLine, VarColorStop,
};
use write_fonts::tables::cpal::{ColorRecord, Cpal};
use write_fonts::tables::variations::{
    DeltaSetIndexMap, ItemVariationData, ItemVariationStore, RegionAxisCoordinates,
    VariationRegion, VariationRegionList,
};
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

/// The static COLR and CPAL tables for these colour glyphs, using the default master.
pub fn tables(
    colored: &[ColorGlyph],
    palettes: &[Vec<Color>],
    ids: &HashMap<String, u16>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    build_static(colored, palettes, ids, ColorLayer::default_paint)
}

/// The static COLR and CPAL tables for a font exported at one master's location: that
/// master's paints and palette override (so a Bold static keeps the Bold colours).
pub fn tables_at(
    font: &Font,
    colored: &[ColorGlyph],
    ids: &HashMap<String, u16>,
    master_id: &str,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let palettes = kernest_core::color::effective_palettes(font, master_id);
    build_static(colored, palettes, ids, |l| l.paint(master_id))
}

fn build_static(
    colored: &[ColorGlyph],
    palettes: &[Vec<Color>],
    ids: &HashMap<String, u16>,
    pick: impl Fn(&ColorLayer) -> Option<&Paint>,
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
        let mut count = 0u8;
        for layer in &c.layers {
            let Some(paint) = pick(layer) else { continue };
            v1_layers.push(WPaint::glyph(paint_table(paint), gid(&layer.glyph)?));
            count += 1;
        }
        v1_bases.push(BaseGlyphPaint::new(
            *base,
            WPaint::colr_layers(count, first),
        ));
        // COLRv0 (Windows 10 and older apps) when every layer is an opaque colour.
        let solid: Option<Vec<(GlyphId16, u16)>> = c
            .layers
            .iter()
            .map(|layer| match pick(layer) {
                Some(Paint::Solid { color, alpha }) if *alpha >= 1.0 => {
                    Some(gid(&layer.glyph).ok().map(|g| (g, *color)))
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
    let cpal = cpal_bytes(palettes)?;
    Ok((colr, cpal))
}

fn cpal_bytes(palettes: &[Vec<Color>]) -> Result<Vec<u8>, String> {
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
    write_fonts::dump_table(&cpal).map_err(|e| format!("could not write CPAL: {e:?}"))
}

fn f2(v: f64) -> F2Dot14 {
    F2Dot14::from_f32(v.clamp(-2.0, 1.99) as f32)
}

fn fw(v: f64) -> FWord {
    FWord::new(v.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16)
}

fn sorted_stops(stops: &[ColorStop]) -> Vec<&ColorStop> {
    let mut s: Vec<&ColorStop> = stops.iter().collect();
    s.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    s
}

fn line(stops: &[ColorStop]) -> ColorLine {
    let stops: Vec<WStop> = sorted_stops(stops)
        .into_iter()
        .map(|s| WStop::new(f2(s.offset), s.color, f2(s.alpha)))
        .collect();
    ColorLine::new(Extend::Pad, stops.len() as u16, stops)
}

/// The six geometry numbers a gradient paint stores, in paint order. For a linear
/// gradient p2 sits square to the line so the stripes run along it; for a radial one
/// the focus is the inner circle (radius 0) and the centre the outer circle.
fn geometry(p: &Paint) -> [f64; 6] {
    match p {
        Paint::Linear { start, end, .. } => {
            let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
            [
                start[0],
                start[1],
                end[0],
                end[1],
                start[0] - dy,
                start[1] + dx,
            ]
        }
        Paint::Radial {
            center,
            radius,
            focus,
            ..
        } => {
            let f = focus.unwrap_or(*center);
            [f[0], f[1], 0.0, center[0], center[1], *radius]
        }
        Paint::Solid { .. } => [0.0; 6],
    }
}

fn paint_table(p: &Paint) -> WPaint {
    match p {
        Paint::Solid { color, alpha } => WPaint::solid(*color, f2(*alpha)),
        Paint::Linear { stops, .. } => {
            let g = geometry(p);
            WPaint::linear_gradient(
                line(stops),
                fw(g[0]),
                fw(g[1]),
                fw(g[2]),
                fw(g[3]),
                fw(g[4]),
                fw(g[5]),
            )
        }
        Paint::Radial { stops, .. } => {
            let g = geometry(p);
            WPaint::radial_gradient(
                line(stops),
                fw(g[0]),
                fw(g[1]),
                UfWord::new(0),
                fw(g[3]),
                fw(g[4]),
                UfWord::new(g[5].round().clamp(0.0, u16::MAX as f64) as u16),
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

// --- Variable COLRv1 -------------------------------------------------------------

/// Whether any colour layer's paint or resolved palette colour differs across masters.
fn colored_varies(font: &Font, colored: &[ColorGlyph]) -> bool {
    if !font.master_palettes.is_empty() {
        return true;
    }
    colored.iter().any(|c| {
        c.layers.iter().any(|layer| {
            let paints: Vec<&Paint> = font
                .masters
                .iter()
                .filter_map(|m| layer.paint(&m.id))
                .collect();
            paints.windows(2).any(|w| w[0] != w[1])
        })
    })
}

/// Accumulates the ItemVariationStore. One ItemVariationData subtable holds every
/// quantity as a row of 32-bit deltas, one per non-default region; the DeltaSetIndexMap
/// is the identity (quantity i → outer 0, inner i), so a paint's `var_index_base` is
/// simply the first quantity of its consecutive block.
struct VarStore {
    regions: Vec<VariationRegion>,
    axis_count: u16,
    rows: Vec<Vec<i32>>,
    model: VariationModel,
}

const NO_VARIATION: u32 = 0xFFFF_FFFF;

impl VarStore {
    fn new(font: &Font) -> Result<Self, String> {
        let locs: Vec<_> = font
            .masters
            .iter()
            .map(|m| normalize(font, &m.location))
            .collect();
        let order: Vec<String> = font.axes.iter().map(|a| a.tag.clone()).collect();
        let model = VariationModel::new(&locs, &order)
            .map_err(|e| format!("could not model the colour variation: {e}"))?;
        let regions = model.supports()[1..]
            .iter()
            .map(|region| {
                VariationRegion::new(
                    order
                        .iter()
                        .map(|tag| {
                            let (lo, peak, hi) =
                                region.get(tag).copied().unwrap_or((0.0, 0.0, 0.0));
                            RegionAxisCoordinates::new(f2(lo), f2(peak), f2(hi))
                        })
                        .collect(),
                )
            })
            .collect();
        Ok(Self {
            regions,
            axis_count: order.len() as u16,
            rows: Vec::new(),
            model,
        })
    }

    /// Adds one quantity (a value per master, in master order) and returns its index.
    /// `scale` converts the real-unit delta to the integer the paint field expects:
    /// 1 for font-unit coordinates, 16384 for F2Dot14 opacities and stop positions
    /// (the region scalar is 16.16 while F2Dot14 applies deltas at 1/16384).
    fn add(&mut self, master_values: &[f64], scale: f64) -> u32 {
        let per_master: Vec<Vec<f64>> = master_values.iter().map(|v| vec![*v]).collect();
        let deltas = self.model.deltas(&per_master);
        let row: Vec<i32> = deltas[1..]
            .iter()
            .map(|d| (d[0] * scale).round() as i32)
            .collect();
        self.rows.push(row);
        (self.rows.len() - 1) as u32
    }

    /// Adds a consecutive block of quantities (a paint's fields) and returns the base
    /// index. Fields keep their order even when some do not vary.
    fn block(&mut self, fields: &[(&[f64], f64)]) -> u32 {
        let base = self.rows.len() as u32;
        for (values, scale) in fields {
            self.add(values, *scale);
        }
        base
    }

    fn finish(self) -> (ItemVariationStore, DeltaSetIndexMap) {
        let n = self.rows.len();
        let region_count = self.regions.len();
        // Every delta is a 32-bit word: long-words flag set, one word per region.
        let word_delta_count = 0x8000 | region_count as u16;
        let mut delta_sets = Vec::with_capacity(n * region_count * 4);
        for row in &self.rows {
            for d in row {
                delta_sets.extend_from_slice(&d.to_be_bytes());
            }
        }
        let data = ItemVariationData::new(
            n as u16,
            word_delta_count,
            (0..region_count as u16).collect(),
            delta_sets,
        );
        let store = ItemVariationStore::new(
            VariationRegionList::new(self.axis_count, self.regions),
            vec![Some(data)],
        );
        let map: DeltaSetIndexMap = (0..n as u32).collect();
        (store, map)
    }
}

/// CPAL palettes extended with the extra colours cross-fade branches need. Columns are
/// appended to every palette at once so an index means the same slot in each palette.
struct CpalColumns {
    palettes: Vec<Vec<Color>>,
}

impl CpalColumns {
    fn new(palettes: &[Vec<Color>]) -> Self {
        Self {
            palettes: palettes.to_vec(),
        }
    }

    /// The index of the column whose colour in palette k is `colors[k]`, appending it
    /// if new. `colors` has one entry per palette.
    fn index(&mut self, colors: &[Color]) -> u16 {
        if let Some(j) = (0..self.palettes.first().map_or(0, Vec::len)).find(|&j| {
            self.palettes
                .iter()
                .zip(colors)
                .all(|(p, c)| p.get(j) == Some(c))
        }) {
            return j as u16;
        }
        let j = self.palettes.first().map_or(0, Vec::len);
        for (p, c) in self.palettes.iter_mut().zip(colors) {
            p.push(*c);
        }
        j as u16
    }
}

/// The resolved colour of palette slot `slot` in master `m`, one entry per palette.
fn resolved_column(font: &Font, m: &str, slot: u16) -> Vec<Color> {
    let pal = kernest_core::color::effective_palettes(font, m);
    pal.iter()
        .map(|p| p.get(slot as usize).copied().unwrap_or(Color::BLACK))
        .collect()
}

/// The colour signature of a master's paint: its kind, stop count and resolved colours.
/// Masters with the same signature share one (possibly variable) paint; different
/// signatures cross-fade.
/// A branch's colour identity: paint kind, number of colour slots, resolved colours.
type ColourSignature = (u8, usize, Vec<Vec<Color>>);

fn signature(font: &Font, m: &str, p: &Paint) -> ColourSignature {
    let (kind, slots): (u8, Vec<u16>) = match p {
        Paint::Solid { color, .. } => (0, vec![*color]),
        Paint::Linear { stops, .. } => (1, stops.iter().map(|s| s.color).collect()),
        Paint::Radial { stops, .. } => (2, stops.iter().map(|s| s.color).collect()),
    };
    let cols = slots.iter().map(|&s| resolved_column(font, m, s)).collect();
    (kind, slots.len(), cols)
}

/// The variable COLR and CPAL tables. Falls back to the static tables when nothing
/// varies, so an ordinary colour font keeps its COLRv0 fallback.
pub fn variable_tables(
    font: &Font,
    colored: &[ColorGlyph],
    ids: &HashMap<String, u16>,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if !colored_varies(font, colored) {
        return tables(colored, &font.palettes, ids);
    }
    let gid = |name: &str| {
        ids.get(name)
            .copied()
            .map(GlyphId16::new)
            .ok_or_else(|| format!("glyph \"{name}\" is missing from the compiled font"))
    };
    let mut glyphs: Vec<(GlyphId16, &ColorGlyph)> = colored
        .iter()
        .filter(|c| !c.layers.is_empty())
        .map(|c| Ok((gid(&c.glyph)?, c)))
        .collect::<Result<_, String>>()?;
    glyphs.sort_by_key(|(g, _)| *g);

    let mut store = VarStore::new(font)?;
    let mut cpal = CpalColumns::new(&font.palettes);
    let masters: Vec<String> = font.masters.iter().map(|m| m.id.clone()).collect();
    let mut v1_bases = Vec::new();
    let mut v1_layers = Vec::new();

    for (base, c) in &glyphs {
        let first = v1_layers.len() as u32;
        let mut count = 0u32;
        for layer in &c.layers {
            let paints: Vec<&Paint> = masters
                .iter()
                .map(|m| {
                    layer
                        .paint(m)
                        .ok_or_else(|| format!("\"{}\" has no colour layer in master {m}", c.glyph))
                })
                .collect::<Result<_, String>>()?;
            let layer_gid = gid(&layer.glyph)?;
            let branches = build_branches(font, &masters, &paints, &mut store, &mut cpal)?;
            for paint in branches {
                v1_layers.push(WPaint::glyph(paint, layer_gid));
                count += 1;
            }
        }
        if count > u8::MAX as u32 {
            return Err(format!("\"{}\" has more than 255 colour layers.", c.glyph));
        }
        v1_bases.push(BaseGlyphPaint::new(
            *base,
            WPaint::colr_layers(count as u8, first),
        ));
    }

    let (ivs, map) = store.finish();
    let mut colr = Colr::new(0, None, None, 0);
    colr.base_glyph_list = Some(BaseGlyphList::new(v1_bases.len() as u32, v1_bases)).into();
    colr.layer_list = Some(LayerList::new(v1_layers.len() as u32, v1_layers)).into();
    colr.var_index_map = Some(map).into();
    colr.item_variation_store = Some(ivs).into();
    let colr =
        write_fonts::dump_table(&colr).map_err(|e| format!("could not write COLR: {e:?}"))?;
    let cpal = cpal_bytes(&cpal.palettes)?;
    Ok((colr, cpal))
}

/// Builds the paint(s) for one colour layer across masters. Masters that resolve to the
/// same colours share a variable paint; differing colours become cross-fade branches.
fn build_branches(
    font: &Font,
    masters: &[String],
    paints: &[&Paint],
    store: &mut VarStore,
    cpal: &mut CpalColumns,
) -> Result<Vec<WPaint>, String> {
    let n = masters.len();
    // Group masters by colour signature, preserving order.
    let mut groups: Vec<(Vec<usize>, ColourSignature)> = Vec::new();
    for (i, (m, p)) in masters.iter().zip(paints).enumerate() {
        let sig = signature(font, m, p);
        if let Some(g) = groups.iter_mut().find(|(_, s)| *s == sig) {
            g.0.push(i);
        } else {
            groups.push((vec![i], sig));
        }
    }

    // Geometry and stop positions are shared by every branch (same kind and stop count
    // within a layer is the normal case); only opacity carries the cross-fade gate.
    let uniform = paints.windows(2).all(|w| {
        std::mem::discriminant(w[0]) == std::mem::discriminant(w[1])
            && stop_count(w[0]) == stop_count(w[1])
    });

    let geom: Vec<[f64; 6]> = paints.iter().map(|p| geometry(p)).collect();
    let geom_vals: Vec<Vec<f64>> = (0..6)
        .map(|k| geom.iter().map(|g| g[k]).collect())
        .collect();
    let geom_block = if uniform {
        let refs: Vec<(&[f64], f64)> = geom_vals.iter().map(|v| (v.as_slice(), 1.0)).collect();
        Some(store.block(&refs))
    } else {
        None
    };

    let mut out = Vec::new();
    for (members, sig) in &groups {
        let gate: Vec<f64> = (0..n)
            .map(|i| if members.contains(&i) { 1.0 } else { 0.0 })
            .collect();
        let representative = paints[members[0]];
        // Palette indices for this branch's resolved colours.
        let indices: Vec<u16> = sig.2.iter().map(|col| cpal.index(col)).collect();
        let paint = match representative {
            Paint::Solid { .. } => {
                let alphas: Vec<f64> = paints
                    .iter()
                    .zip(&gate)
                    .map(|(p, g)| match p {
                        Paint::Solid { alpha, .. } => g * alpha,
                        _ => 0.0,
                    })
                    .collect();
                let base = store.block(&[(&alphas, 16384.0)]);
                let default_alpha = alphas[default_index(font, masters)];
                WPaint::var_solid(indices[0], f2(default_alpha), base)
            }
            Paint::Linear { stops, .. } | Paint::Radial { stops, .. } => {
                let stops = sorted_stops(stops);
                let var_stops = var_color_line(
                    font, masters, paints, &gate, &stops, store, &indices, uniform,
                )?;
                let g0 = geom[default_index(font, masters)];
                let base = geom_block.unwrap_or(NO_VARIATION);
                if matches!(representative, Paint::Linear { .. }) {
                    WPaint::var_linear_gradient(
                        var_stops,
                        fw(g0[0]),
                        fw(g0[1]),
                        fw(g0[2]),
                        fw(g0[3]),
                        fw(g0[4]),
                        fw(g0[5]),
                        base,
                    )
                } else {
                    WPaint::var_radial_gradient(
                        var_stops,
                        fw(g0[0]),
                        fw(g0[1]),
                        UfWord::new(g0[2].round().clamp(0.0, u16::MAX as f64) as u16),
                        fw(g0[3]),
                        fw(g0[4]),
                        UfWord::new(g0[5].round().clamp(0.0, u16::MAX as f64) as u16),
                        base,
                    )
                }
            }
        };
        out.push(paint);
    }
    Ok(out)
}

fn stop_count(p: &Paint) -> usize {
    match p {
        Paint::Solid { .. } => 0,
        Paint::Linear { stops, .. } | Paint::Radial { stops, .. } => stops.len(),
    }
}

/// The default master's index in `masters` (the one normalised to the origin).
fn default_index(font: &Font, masters: &[String]) -> usize {
    masters
        .iter()
        .position(|m| {
            font.master(m)
                .map(|mm| normalize(font, &mm.location).is_empty())
                .unwrap_or(false)
        })
        .unwrap_or(0)
}

/// The variable colour line for a gradient branch: stop positions vary with the master,
/// stop opacities carry the cross-fade gate. Each stop gets a consecutive (offset, alpha)
/// block so its `var_index_base` covers both fields.
#[allow(clippy::too_many_arguments)]
fn var_color_line(
    font: &Font,
    masters: &[String],
    paints: &[&Paint],
    gate: &[f64],
    stops: &[&ColorStop],
    store: &mut VarStore,
    indices: &[u16],
    uniform: bool,
) -> Result<VarColorLine, String> {
    let default = default_index(font, masters);
    let mut var_stops = Vec::new();
    for (si, stop) in stops.iter().enumerate() {
        // Per-master offset and alpha for this stop (stops are sorted alike in every
        // master when uniform; otherwise each branch keeps its own representative's).
        let offsets: Vec<f64> = paints
            .iter()
            .map(|p| nth_stop(p, si).map(|s| s.offset).unwrap_or(stop.offset))
            .collect();
        let alphas: Vec<f64> = paints
            .iter()
            .zip(gate)
            .map(|(p, g)| g * nth_stop(p, si).map(|s| s.alpha).unwrap_or(stop.alpha))
            .collect();
        let base = if uniform {
            store.block(&[(&offsets, 16384.0), (&alphas, 16384.0)])
        } else {
            NO_VARIATION
        };
        let palette_index = indices.get(si).copied().unwrap_or(stop.color);
        var_stops.push(VarColorStop::new(
            f2(offsets[default]),
            palette_index,
            f2(alphas[default]),
            base,
        ));
    }
    Ok(VarColorLine::new(
        Extend::Pad,
        var_stops.len() as u16,
        var_stops,
    ))
}

fn nth_stop(p: &Paint, i: usize) -> Option<&ColorStop> {
    match p {
        Paint::Linear { stops, .. } | Paint::Radial { stops, .. } => {
            sorted_stops(stops).into_iter().nth(i)
        }
        Paint::Solid { .. } => None,
    }
}
