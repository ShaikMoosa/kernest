//! Images placed on glyph layers (S33) and annotations (S32).
//!
//! An image is stored by path, relative to the font file where it sits next to it,
//! so a folder of scans travels with the font. The editor gets the picture as a
//! data URL, since the web view can't read local files itself.

use std::path::{Path, PathBuf};

use kernest_core::{Annotation, GlyphBitmap, GlyphSvg, LayerImage};
use serde::Serialize;
use tauri::State;

use crate::commands::{
    doc_mut, ensure_glyph_unlocked, layer_view_of, lock, AppState, CmdResult, Document, LayerView,
};

/// Replaces one layer's annotations (one undoable step).
#[tauri::command]
pub fn set_annotations(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    annotations: Vec<Annotation>,
) -> CmdResult<LayerView> {
    if annotations.iter().any(|a| {
        !a.x.is_finite()
            || !a.y.is_finite()
            || !a.angle.is_finite()
            || !a.size.is_finite()
            || a.size < 0.0
    }) {
        return Err("An annotation has an invalid position or size.".into());
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    edit_layer(doc, &glyph, &master_id, |l| l.annotations = annotations)
}

/// Sets or (with `None`) removes a layer's image.
#[tauri::command]
pub fn set_layer_image(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    image: Option<LayerImage>,
) -> CmdResult<LayerView> {
    if let Some(i) = &image {
        let nums = [i.x, i.y, i.scale_x, i.scale_y, i.angle];
        if nums.iter().any(|v| !v.is_finite()) || i.scale_x == 0.0 || i.scale_y == 0.0 {
            return Err("The image needs a size above 0.".into());
        }
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    edit_layer(doc, &glyph, &master_id, |l| l.image = image)
}

/// Glyph › Add Image…: places the image file at the origin, one image point per
/// font unit.
#[tauri::command]
pub fn add_layer_image(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    file: String,
) -> CmdResult<LayerView> {
    let bytes = std::fs::read(&file).map_err(|e| format!("Couldn't read the image: {e}"))?;
    let (w, h) = image_size(&bytes).ok_or("Kernest can show PNG, JPEG and GIF images.")?;
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let stored = relative_to_font(doc.path.as_deref(), Path::new(&file));
    // Points: images carry no reliable resolution here, so one pixel is one point.
    let _ = (w, h);
    let image = LayerImage {
        path: stored,
        x: 0.0,
        y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        angle: 0.0,
        crop: None,
        locked: false,
    };
    edit_layer(doc, &glyph, &master_id, |l| l.image = Some(image))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    /// The picture as a data URL, or `None` when the file can't be found.
    data_url: Option<String>,
    width: u32,
    height: u32,
    /// The file it was looked for at.
    file: String,
}

/// The layer image's picture and pixel size, for drawing.
#[tauri::command]
pub fn layer_image_data(
    state: State<AppState>,
    glyph: String,
    master_id: String,
) -> CmdResult<Option<ImageData>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let Some(img) = doc
        .font
        .glyph(&glyph)
        .and_then(|g| g.layer(&master_id))
        .and_then(|l| l.image.clone())
    else {
        return Ok(None);
    };
    let file = resolve(doc.path.as_deref(), &img.path);
    let read = std::fs::read(&file).ok();
    let (data_url, (width, height)) =
        match read.as_deref().and_then(|b| image_size(b).map(|s| (b, s))) {
            Some((b, size)) => (Some(format!("data:{};base64,{}", mime(b), base64(b))), size),
            None => (None, (0, 0)),
        };
    Ok(Some(ImageData {
        data_url,
        width,
        height,
        file: file.display().to_string(),
    }))
}

// --- sbix bitmaps and SVG drawings (S63) ----------------------------------------

/// One of a glyph's colour pictures, for the editor: where it sits and what it shows.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphPicture {
    /// "bitmap" or "svg".
    kind: &'static str,
    /// For bitmaps: the pixel size.
    ppem: u16,
    path: String,
    x: f64,
    y: f64,
    /// Size in font units.
    width: f64,
    height: f64,
    /// The SVG's scale (1 for bitmaps).
    scale: f64,
    /// The picture as a data URL, or `None` when the file can't be read.
    data_url: Option<String>,
    file: String,
}

/// A glyph's sbix bitmaps (smallest first) and its SVG drawing.
#[tauri::command]
pub fn glyph_pictures(state: State<AppState>, glyph: String) -> CmdResult<Vec<GlyphPicture>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    pictures(doc, &glyph)
}

fn pictures(doc: &Document, glyph: &str) -> CmdResult<Vec<GlyphPicture>> {
    let g = doc
        .font
        .glyph(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    let upm = doc.font.info.units_per_em as f64;
    let mut out = Vec::new();
    let mut bitmaps = g.bitmaps.clone();
    bitmaps.sort_by_key(|b| b.ppem);
    for b in bitmaps {
        let file = resolve(doc.path.as_deref(), &b.path);
        let bytes = std::fs::read(&file).ok();
        let (data_url, (w, h)) = match bytes.as_deref().and_then(|d| image_size(d).map(|s| (d, s)))
        {
            Some((d, s)) => (Some(format!("data:{};base64,{}", mime(d), base64(d))), s),
            None => (None, (0, 0)),
        };
        let unit = upm / b.ppem.max(1) as f64;
        out.push(GlyphPicture {
            kind: "bitmap",
            ppem: b.ppem,
            path: b.path,
            x: b.x,
            y: b.y,
            width: w as f64 * unit,
            height: h as f64 * unit,
            scale: 1.0,
            data_url,
            file: file.display().to_string(),
        });
    }
    if let Some(svg) = &g.svg {
        let file = resolve(doc.path.as_deref(), &svg.path);
        let text = std::fs::read_to_string(&file).ok();
        let parsed = text
            .as_deref()
            .and_then(|t| kernest_export::bitmap::svg_contents(t).ok());
        let (data_url, w, h) = match (text, parsed) {
            (Some(t), Some((_, b))) => (
                Some(format!(
                    "data:image/svg+xml;base64,{}",
                    base64(t.as_bytes())
                )),
                b[2],
                b[3],
            ),
            _ => (None, 0.0, 0.0),
        };
        out.push(GlyphPicture {
            kind: "svg",
            ppem: 0,
            path: svg.path.clone(),
            x: svg.x,
            y: svg.y,
            width: w * svg.scale,
            height: h * svg.scale,
            scale: svg.scale,
            data_url,
            file: file.display().to_string(),
        });
    }
    Ok(out)
}

fn edit_glyph(
    doc: &mut Document,
    glyph: &str,
    f: impl FnOnce(&mut kernest_core::Glyph) -> CmdResult<()>,
) -> CmdResult<Vec<GlyphPicture>> {
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let mut next = doc.font.clone();
    let g = next
        .glyph_mut(glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?;
    f(g)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    pictures(doc, glyph)
}

/// Adds a PNG as the glyph's bitmap for one pixel size (replacing one already there),
/// standing on the baseline at the left edge.
#[tauri::command]
pub fn add_glyph_bitmap(
    state: State<AppState>,
    glyph: String,
    file: String,
    ppem: u16,
) -> CmdResult<Vec<GlyphPicture>> {
    if !(1..=2048).contains(&ppem) {
        return Err("Choose a pixel size between 1 and 2048.".into());
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("Couldn't read the picture: {e}"))?;
    if kernest_export::bitmap::png_size(&bytes).is_none() {
        return Err("sbix bitmaps must be PNG pictures.".into());
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let stored = relative_to_font(doc.path.as_deref(), Path::new(&file));
    edit_glyph(doc, &glyph, |g| {
        g.bitmaps.retain(|b| b.ppem != ppem);
        g.bitmaps.push(GlyphBitmap {
            ppem,
            path: stored,
            x: 0.0,
            y: 0.0,
        });
        Ok(())
    })
}

/// Sets the glyph's SVG drawing from a file, scaled so it is as tall as the em.
#[tauri::command]
pub fn add_glyph_svg(
    state: State<AppState>,
    glyph: String,
    file: String,
) -> CmdResult<Vec<GlyphPicture>> {
    let text = std::fs::read_to_string(&file).map_err(|e| format!("Couldn't read the SVG: {e}"))?;
    let (_, bx) = kernest_export::bitmap::svg_contents(&text)?;
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let stored = relative_to_font(doc.path.as_deref(), Path::new(&file));
    let master = doc.font.masters.first().map(|m| m.metrics);
    let (desc, asc) = master.map_or((-200.0, 800.0), |m| (m.descender, m.ascender));
    let scale = if bx[3] > 0.0 {
        (asc - desc) / bx[3]
    } else {
        1.0
    };
    edit_glyph(doc, &glyph, |g| {
        g.svg = Some(GlyphSvg {
            path: stored,
            x: 0.0,
            y: desc,
            scale,
        });
        Ok(())
    })
}

/// Moves a picture: the bitmap for `ppem`, or the SVG when `ppem` is 0. For the SVG,
/// `scale` (when given) resizes it.
#[tauri::command]
pub fn place_glyph_picture(
    state: State<AppState>,
    glyph: String,
    ppem: u16,
    x: f64,
    y: f64,
    scale: Option<f64>,
) -> CmdResult<Vec<GlyphPicture>> {
    if !x.is_finite() || !y.is_finite() || scale.is_some_and(|s| !s.is_finite() || s <= 0.0) {
        return Err("Enter a position and a size above 0.".into());
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    edit_glyph(doc, &glyph, |g| {
        if ppem == 0 {
            let svg = g.svg.as_mut().ok_or("This glyph has no SVG drawing.")?;
            svg.x = x;
            svg.y = y;
            if let Some(s) = scale {
                svg.scale = s;
            }
        } else {
            let b = g
                .bitmaps
                .iter_mut()
                .find(|b| b.ppem == ppem)
                .ok_or("This glyph has no bitmap at that size.")?;
            b.x = x;
            b.y = y;
        }
        Ok(())
    })
}

/// Removes a picture: the bitmap for `ppem`, or the SVG when `ppem` is 0.
#[tauri::command]
pub fn remove_glyph_picture(
    state: State<AppState>,
    glyph: String,
    ppem: u16,
) -> CmdResult<Vec<GlyphPicture>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    edit_glyph(doc, &glyph, |g| {
        if ppem == 0 {
            g.svg = None;
        } else {
            g.bitmaps.retain(|b| b.ppem != ppem);
        }
        Ok(())
    })
}

fn edit_layer(
    doc: &mut Document,
    glyph: &str,
    key: &str,
    f: impl FnOnce(&mut kernest_core::Layer),
) -> CmdResult<LayerView> {
    let mut layer = doc
        .font
        .glyph(glyph)
        .and_then(|g| g.layer(key))
        .ok_or_else(|| format!("\"{glyph}\" has no layer \"{key}\"."))?
        .clone();
    f(&mut layer);
    if doc
        .history
        .apply(&mut doc.font, glyph, key, layer)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    layer_view_of(doc, glyph, key)
}

/// The folder a font's relative image paths start from: next to the `.kernest`
/// package (or other source file).
fn base_dir(font_path: Option<&Path>) -> Option<PathBuf> {
    font_path.and_then(|p| p.parent()).map(Path::to_path_buf)
}

/// The image path to store: relative to the font's folder when the image is inside
/// it, otherwise as given.
pub(crate) fn relative_to_font(font_path: Option<&Path>, image: &Path) -> String {
    if let Some(base) = base_dir(font_path) {
        if let Ok(rel) = image.strip_prefix(&base) {
            return rel.to_string_lossy().replace('\\', "/");
        }
    }
    image.to_string_lossy().to_string()
}

fn resolve(font_path: Option<&Path>, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    match base_dir(font_path) {
        Some(base) => base.join(p),
        None => p.to_path_buf(),
    }
}

fn mime(b: &[u8]) -> &'static str {
    if b.starts_with(b"\x89PNG") {
        "image/png"
    } else if b.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

/// Pixel width and height of a PNG, JPEG or GIF file.
pub(crate) fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() >= 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if b.len() >= 10 && b.starts_with(b"GIF8") {
        return Some((
            u16::from_le_bytes([b[6], b[7]]) as u32,
            u16::from_le_bytes([b[8], b[9]]) as u32,
        ));
    }
    if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            // Start-of-frame markers (not DHT, JPG or DAC).
            if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = match chunk.len() {
            3 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32,
            2 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8,
            _ => (chunk[0] as u32) << 16,
        };
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn reads_png_and_gif_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(300u32.to_be_bytes());
        png.extend(200u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((300, 200)));
        let gif = b"GIF89a\x40\x01\xc8\x00";
        assert_eq!(image_size(gif), Some((320, 200)));
        assert_eq!(image_size(b"nope"), None);
    }

    #[test]
    fn paths_next_to_the_font_are_stored_relative() {
        let font = Path::new("C:/fonts/Sans.kernest");
        assert_eq!(
            relative_to_font(Some(font), Path::new("C:/fonts/scans/g.jpg")),
            "scans/g.jpg"
        );
        assert_eq!(
            relative_to_font(Some(font), Path::new("D:/other/g.jpg")),
            "D:/other/g.jpg"
        );
        assert_eq!(
            resolve(Some(font), "scans/g.jpg"),
            PathBuf::from("C:/fonts/scans/g.jpg")
        );
    }
}
