//! Bitmap and SVG colour glyphs (S63): the `sbix` and `SVG ` tables.
//!
//! sbix holds PNG pictures of glyphs at fixed pixel sizes. The SVG table holds one SVG
//! document per glyph: from the glyph's own SVG file, from its colour layers ("Color
//! Layers to SVG") or from one of its bitmaps ("SBIX to SVG"). Pictures and SVG files
//! are referenced by path, relative to the font file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use kernest_core::color::{effective_palettes, Color, ColorGlyph, ColorStop, Paint};
use kernest_core::model::{Font, GlyphSvg};
use write_fonts::read::{FontRef, TableProvider};
use write_fonts::types::Tag;
use write_fonts::FontBuilder;

/// The export switches, read from the font's custom parameters (as in Glyphs).
#[derive(Debug, Clone, PartialEq)]
pub struct Switches {
    /// "Export sbix Table": on unless set to 0/false.
    pub sbix: bool,
    /// "SBIX to SVG": the pixel size whose bitmaps become SVG documents.
    pub sbix_to_svg: Option<u16>,
    /// "Color Layers to SVG": build SVG documents from colour layers.
    pub color_to_svg: bool,
}

pub fn switches(font: &Font) -> Switches {
    let param = |name: &str| {
        font.info
            .custom_parameters
            .iter()
            .find(|p| !p.disabled && p.name.eq_ignore_ascii_case(name))
            .map(|p| p.value.trim().to_string())
    };
    let truthy = |v: &str| {
        !matches!(
            v.to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    };
    Switches {
        sbix: param("Export sbix Table").is_none_or(|v| truthy(&v)),
        sbix_to_svg: param("SBIX to SVG")
            .and_then(|v| v.parse().ok())
            .filter(|&p| p > 0),
        color_to_svg: param("Color Layers to SVG").is_some_and(|v| truthy(&v)),
    }
}

/// Whether the font needs any of these tables.
pub fn needed(font: &Font, colored: &[ColorGlyph]) -> bool {
    let s = switches(font);
    font.glyphs
        .iter()
        .any(|g| g.svg.is_some() || (s.sbix && !g.bitmaps.is_empty()))
        || (s.color_to_svg && !colored.is_empty())
        || (s.sbix_to_svg.is_some() && font.glyphs.iter().any(|g| !g.bitmaps.is_empty()))
}

/// A referenced file: as stored if absolute, otherwise next to the font file.
pub fn resolve(base: Option<&Path>, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    match base {
        Some(b) if p.is_relative() => b.join(p),
        _ => p.to_path_buf(),
    }
}

fn read(base: Option<&Path>, stored: &str) -> Result<Vec<u8>, String> {
    let path = resolve(base, stored);
    std::fs::read(&path).map_err(|e| format!("could not read {}: {e}", path.display()))
}

/// Width and height of a PNG, from its header.
pub fn png_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || &data[..8] != b"\x89PNG\r\n\x1a\n" || &data[12..16] != b"IHDR" {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
    Some((be(16), be(20)))
}

/// One glyph's picture in a strike: origin offset in pixels and the PNG.
type Picture = (i16, i16, Vec<u8>);

/// Builds the sbix table: one strike per pixel size used by any glyph.
pub fn sbix_table(
    font: &Font,
    ids: &HashMap<String, u16>,
    num_glyphs: u16,
    base: Option<&Path>,
) -> Result<Option<Vec<u8>>, String> {
    let upm = font.info.units_per_em.max(1) as f64;
    // ppem -> glyph id -> (x, y, png)
    let mut strikes: BTreeMap<u16, BTreeMap<u16, Picture>> = BTreeMap::new();
    for g in font.glyphs.iter().filter(|g| g.export) {
        let Some(&gid) = ids.get(&g.name) else {
            continue;
        };
        for b in &g.bitmaps {
            let png = read(base, &b.path)?;
            if png_size(&png).is_none() {
                return Err(format!("{}: {} is not a PNG picture", g.name, b.path));
            }
            let px = |v: f64| (v * b.ppem as f64 / upm).round().clamp(-32768.0, 32767.0) as i16;
            strikes
                .entry(b.ppem)
                .or_default()
                .insert(gid, (px(b.x), px(b.y), png));
        }
    }
    if strikes.is_empty() {
        return Ok(None);
    }
    let n = num_glyphs as usize;
    let mut out = Vec::new();
    out.extend(1u16.to_be_bytes()); // version
    out.extend(1u16.to_be_bytes()); // flags: bit 0 must be set; outlines are not drawn
    out.extend((strikes.len() as u32).to_be_bytes());
    let header = 8 + 4 * strikes.len();
    let mut bodies: Vec<Vec<u8>> = Vec::new();
    for (ppem, glyphs) in &strikes {
        let mut s = Vec::new();
        s.extend(ppem.to_be_bytes());
        s.extend(72u16.to_be_bytes());
        let offsets_len = 4 + 4 * (n + 1);
        let mut data = Vec::new();
        let mut offsets = Vec::with_capacity(n + 1);
        for gid in 0..n as u16 {
            offsets.push((offsets_len + data.len()) as u32);
            if let Some((x, y, png)) = glyphs.get(&gid) {
                data.extend(x.to_be_bytes());
                data.extend(y.to_be_bytes());
                data.extend(b"png ");
                data.extend(png);
            }
        }
        offsets.push((offsets_len + data.len()) as u32);
        for o in offsets {
            s.extend(o.to_be_bytes());
        }
        s.extend(data);
        bodies.push(s);
    }
    let mut at = header;
    for b in &bodies {
        out.extend((at as u32).to_be_bytes());
        at += b.len();
    }
    for b in bodies {
        out.extend(b);
    }
    Ok(Some(out))
}

fn n(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

fn hex(c: &Color) -> (String, f64) {
    (
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b),
        c.a as f64 / 255.0,
    )
}

/// An attribute value from an SVG start tag (`name="…"` or `name='…'`).
fn attr(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().last();
        let after = &rest[i + name.len()..];
        let after = after.trim_start();
        if before.is_some_and(|c| c.is_whitespace()) && after.starts_with('=') {
            let v = after[1..].trim_start();
            let q = v.chars().next()?;
            if q == '"' || q == '\'' {
                let end = v[1..].find(q)?;
                return Some(v[1..1 + end].to_string());
            }
        }
        rest = &rest[i + name.len()..];
    }
    None
}

fn length(v: &str) -> Option<f64> {
    let v = v.trim().trim_end_matches("px");
    v.parse().ok()
}

/// The inner markup of an SVG file and its user-space box (x, y, width, height).
pub fn svg_contents(text: &str) -> Result<(String, [f64; 4]), String> {
    let start = text.find("<svg").ok_or("this file has no <svg> element")?;
    let open_end = start
        + text[start..]
            .find('>')
            .ok_or("the <svg> element is not closed")?;
    let tag = &text[start..open_end];
    if tag.ends_with('/') {
        return Ok((String::new(), [0.0, 0.0, 0.0, 0.0]));
    }
    let close = text
        .rfind("</svg>")
        .ok_or("the <svg> element is not closed")?;
    let inner = text[open_end + 1..close].to_string();
    let vb = attr(tag, "viewBox").and_then(|v| {
        let p: Vec<f64> = v
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        (p.len() == 4).then(|| [p[0], p[1], p[2], p[3]])
    });
    let w = attr(tag, "width").and_then(|v| length(&v));
    let h = attr(tag, "height").and_then(|v| length(&v));
    let bx = match (vb, w, h) {
        (Some(v), _, _) => v,
        (None, Some(w), Some(h)) => [0.0, 0.0, w, h],
        _ => return Err("the SVG needs a viewBox or a width and height".into()),
    };
    if bx[2] <= 0.0 || bx[3] <= 0.0 {
        return Err("the SVG has no size".into());
    }
    let lower = inner.to_ascii_lowercase();
    if lower.contains("<script") || lower.contains("<foreignobject") {
        return Err("the SVG contains scripts or embedded HTML, which fonts can't use".into());
    }
    Ok((inner, bx))
}

/// The SVG table document for a glyph's own SVG file. Font units are y-up with the
/// origin on the baseline; the document's y axis points down.
fn file_document(gid: u16, svg: &GlyphSvg, text: &str) -> Result<String, String> {
    let (inner, [vx, vy, _, vh]) = svg_contents(text)?;
    let s = svg.scale;
    // SVG point (u, v) -> font (x + s(u - vx), y + s(vy + vh - v)) -> doc (X, -Y).
    let tx = svg.x - s * vx;
    let ty = -(svg.y + s * (vy + vh));
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><g id=\"glyph{gid}\" transform=\"matrix({} 0 0 {} {} {})\">{inner}</g></svg>",
        n(s),
        n(s),
        n(tx),
        n(ty)
    ))
}

fn stops(stops: &[ColorStop], palette: &[Color]) -> String {
    let mut out = String::new();
    for st in stops {
        let c = palette
            .get(st.color as usize)
            .copied()
            .unwrap_or(Color::BLACK);
        let (h, a) = hex(&c);
        out.push_str(&format!(
            "<stop offset=\"{}\" stop-color=\"{h}\" stop-opacity=\"{}\"/>",
            n(st.offset),
            n(a * st.alpha)
        ));
    }
    out
}

/// The SVG document drawn from a colour glyph's layers (its default-master paints).
fn color_document(font: &Font, gid: u16, cg: &ColorGlyph, master_id: &str) -> String {
    let palette: Vec<Color> = effective_palettes(font, master_id)
        .first()
        .cloned()
        .unwrap_or_default();
    let mut defs = String::new();
    let mut body = String::new();
    for (i, layer) in cg.layers.iter().enumerate() {
        let Some(g) = font.glyph(&layer.glyph) else {
            continue;
        };
        let Some(l) = g.layer(master_id).or_else(|| g.layers.first()) else {
            continue;
        };
        let d = kernest_core::geometry::svg_path(font, l);
        if d.is_empty() {
            continue;
        }
        let paint = layer
            .paint(master_id)
            .or_else(|| layer.paints.first().map(|(_, p)| p));
        let fill = match paint {
            Some(Paint::Solid { color, alpha }) => {
                let c = palette
                    .get(*color as usize)
                    .copied()
                    .unwrap_or(Color::BLACK);
                let (h, a) = hex(&c);
                format!("fill=\"{h}\" fill-opacity=\"{}\"", n(a * alpha))
            }
            Some(Paint::Linear {
                start,
                end,
                stops: s,
            }) => {
                defs.push_str(&format!(
                    "<linearGradient id=\"g{gid}p{i}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{}</linearGradient>",
                    n(start[0]), n(start[1]), n(end[0]), n(end[1]), stops(s, &palette)
                ));
                format!("fill=\"url(#g{gid}p{i})\"")
            }
            Some(Paint::Radial {
                center,
                radius,
                focus,
                stops: s,
            }) => {
                let f = focus.unwrap_or(*center);
                defs.push_str(&format!(
                    "<radialGradient id=\"g{gid}p{i}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\" fx=\"{}\" fy=\"{}\">{}</radialGradient>",
                    n(center[0]), n(center[1]), n(*radius), n(f[0]), n(f[1]), stops(s, &palette)
                ));
                format!("fill=\"url(#g{gid}p{i})\"")
            }
            None => "fill=\"#000000\"".into(),
        };
        body.push_str(&format!("<path d=\"{d}\" {fill}/>"));
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>{defs}</defs><g id=\"glyph{gid}\" transform=\"scale(1 -1)\">{body}</g></svg>"
    )
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(A[(v >> 18) as usize & 63] as char);
        out.push(A[(v >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            A[(v >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[v as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The SVG document showing one bitmap ("SBIX to SVG").
fn bitmap_document(font: &Font, gid: u16, ppem: u16, x: f64, y: f64, png: &[u8]) -> String {
    let (w, h) = png_size(png).unwrap_or((0, 0));
    let unit = font.info.units_per_em as f64 / ppem.max(1) as f64;
    let (w, h) = (w as f64 * unit, h as f64 * unit);
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><image id=\"glyph{gid}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" xlink:href=\"data:image/png;base64,{}\"/></svg>",
        n(x),
        n(-(y + h)),
        n(w),
        n(h),
        base64(png)
    )
}

/// Builds the SVG table, or `None` when no glyph has an SVG document.
pub fn svg_table(
    font: &Font,
    colored: &[ColorGlyph],
    ids: &HashMap<String, u16>,
    master_id: &str,
    base: Option<&Path>,
) -> Result<Option<Vec<u8>>, String> {
    let s = switches(font);
    let mut docs: BTreeMap<u16, String> = BTreeMap::new();
    for g in font.glyphs.iter().filter(|g| g.export) {
        let Some(&gid) = ids.get(&g.name) else {
            continue;
        };
        if let Some(svg) = &g.svg {
            let bytes = read(base, &svg.path)?;
            let text = String::from_utf8(bytes)
                .map_err(|_| format!("{}: {} is not a text SVG file", g.name, svg.path))?;
            let doc = file_document(gid, svg, &text).map_err(|e| format!("{}: {e}", g.name))?;
            docs.insert(gid, doc);
        } else if let Some(ppem) = s.sbix_to_svg {
            // The bitmap at that size, or the nearest larger (else the largest).
            let mut bs: Vec<_> = g.bitmaps.iter().collect();
            bs.sort_by_key(|b| b.ppem);
            if let Some(b) = bs.iter().find(|b| b.ppem >= ppem).or(bs.last()) {
                let png = read(base, &b.path)?;
                docs.insert(gid, bitmap_document(font, gid, b.ppem, b.x, b.y, &png));
            }
        }
    }
    if s.color_to_svg {
        for cg in colored {
            if let Some(&gid) = ids.get(&cg.glyph) {
                docs.entry(gid)
                    .or_insert_with(|| color_document(font, gid, cg, master_id));
            }
        }
    }
    if docs.is_empty() {
        return Ok(None);
    }
    let count = docs.len();
    let list_header = 2 + 12 * count;
    let mut records: Vec<u8> = Vec::new();
    let mut bodies: Vec<u8> = Vec::new();
    for (gid, doc) in &docs {
        let offset = list_header + bodies.len();
        records.extend(gid.to_be_bytes());
        records.extend(gid.to_be_bytes());
        records.extend((offset as u32).to_be_bytes());
        records.extend((doc.len() as u32).to_be_bytes());
        bodies.extend(doc.as_bytes());
    }
    let mut out = Vec::new();
    out.extend(0u16.to_be_bytes());
    out.extend(10u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend((count as u16).to_be_bytes());
    out.extend(records);
    out.extend(bodies);
    Ok(Some(out))
}

/// The font with these tables added (or replaced).
pub fn add_tables(font: &[u8], tables: &[(Tag, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let f = FontRef::new(font).map_err(|e| format!("not a valid font: {e}"))?;
    let mut b = FontBuilder::new();
    for r in f.table_directory.table_records() {
        let t = r.tag();
        if tables.iter().any(|(tag, _)| *tag == t) {
            continue;
        }
        if let Some(d) = f.table_data(t) {
            b.add_raw(t, d.as_bytes().to_vec());
        }
    }
    for (t, d) in tables {
        b.add_raw(*t, d.clone());
    }
    Ok(b.build())
}

/// Adds sbix and SVG tables to a compiled font, when the font has any.
pub fn add_to(
    font: &Font,
    colored: &[ColorGlyph],
    ids_from: &[u8],
    bytes: Vec<u8>,
    master_id: &str,
    base: Option<&Path>,
) -> Result<Vec<u8>, String> {
    if !needed(font, colored) {
        return Ok(bytes);
    }
    let ids = crate::colr::glyph_ids(ids_from)?;
    let num_glyphs = FontRef::new(&bytes)
        .and_then(|f| f.maxp())
        .map(|m| m.num_glyphs())
        .map_err(|e| format!("could not read the compiled font: {e}"))?;
    let mut tables = Vec::new();
    if switches(font).sbix {
        if let Some(t) = sbix_table(font, &ids, num_glyphs, base)? {
            tables.push((Tag::new(b"sbix"), t));
        }
    }
    if let Some(t) = svg_table(font, colored, &ids, master_id, base)? {
        tables.push((Tag::new(b"SVG "), t));
    }
    if tables.is_empty() {
        return Ok(bytes);
    }
    add_tables(&bytes, &tables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(128u32.to_be_bytes());
        png.extend(64u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((128, 64)));
        assert_eq!(png_size(b"GIF89a"), None);
    }

    #[test]
    fn svg_contents_uses_the_view_box() {
        let (inner, bx) =
            svg_contents("<?xml version=\"1.0\"?><svg xmlns=\"x\" viewBox=\"0 0 100 50\"><rect width=\"10\" height=\"10\"/></svg>")
                .unwrap();
        assert_eq!(bx, [0.0, 0.0, 100.0, 50.0]);
        assert!(inner.contains("<rect"));
        let (_, bx) = svg_contents("<svg width=\"20px\" height='30'></svg>").unwrap();
        assert_eq!(bx, [0.0, 0.0, 20.0, 30.0]);
        assert!(svg_contents("<svg viewBox=\"0 0 1 1\"><script/></svg>").is_err());
    }

    #[test]
    fn file_document_places_the_drawing() {
        let svg = GlyphSvg {
            path: "a.svg".into(),
            x: 50.0,
            y: -100.0,
            scale: 2.0,
        };
        let doc = file_document(
            3,
            &svg,
            "<svg viewBox=\"0 0 100 400\"><path d=\"M0 0\"/></svg>",
        )
        .unwrap();
        // Top edge: y + scale * height = -100 + 800 = 700 font units -> -700.
        assert!(
            doc.contains("id=\"glyph3\" transform=\"matrix(2 0 0 2 50 -700)\""),
            "{doc}"
        );
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn export_adds_sbix_and_svg_tables() {
        use kernest_core::model::{CustomParameter, GlyphBitmap};
        let assets = tempfile::tempdir().unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(16u32.to_be_bytes());
        png.extend(16u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        std::fs::write(assets.path().join("a16.png"), &png).unwrap();
        std::fs::write(
            assets.path().join("b.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"><rect width=\"10\" height=\"10\" fill=\"red\"/></svg>",
        )
        .unwrap();
        let mut f = kernest_core::sample::sample_font();
        f.glyph_mut("A").unwrap().bitmaps.push(GlyphBitmap {
            ppem: 16,
            path: "a16.png".into(),
            x: 0.0,
            y: 0.0,
        });
        f.glyph_mut("B").unwrap().svg = Some(GlyphSvg {
            path: "b.svg".into(),
            x: 0.0,
            y: 0.0,
            scale: 50.0,
        });
        f.info.custom_parameters.push(CustomParameter {
            name: "SBIX to SVG".into(),
            value: "16".into(),
            disabled: false,
        });
        let out = tempfile::tempdir().unwrap();
        let mut opts = crate::ExportOptions::ttf(false, true, out.path());
        opts.assets_dir = Some(assets.path().to_path_buf());
        let r = crate::export(&f, &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let ids = crate::colr::glyph_ids(&bytes).unwrap();
        let sbix = font.table_data(Tag::new(b"sbix")).expect("sbix table");
        let sbix = sbix.as_bytes();
        assert_eq!(&sbix[0..4], &[0, 1, 0, 1]);
        assert_eq!(u32::from_be_bytes(sbix[4..8].try_into().unwrap()), 1);
        let svg = font.table_data(Tag::new(b"SVG ")).expect("SVG table");
        let svg = String::from_utf8_lossy(svg.as_bytes()).to_string();
        assert!(svg.contains(&format!("id=\"glyph{}\"", ids["B"])));
        assert!(
            svg.contains(&format!("id=\"glyph{}\"", ids["A"])),
            "SBIX to SVG"
        );
        assert!(svg.contains("data:image/png;base64,"));
    }
}
