//! The edit view's pixel preview: hinted and unhinted outlines at text sizes,
//! so the effect of zones and stems is visible before exporting. The hinted
//! outlines come from the same TrueType compile and hinting as a TTF export,
//! grid-fitted by skrifa; the frontend rasterises them on a pixel grid.

use std::collections::HashMap;

use kernest_core::designspace::Location;
use serde::Serialize;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{pen::PathElement, DrawSettings, HintingInstance, HintingMode};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use tauri::State;

use crate::commands::{doc_mut, lock, status, AppState, CmdResult, DocStatus, Document};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelGlyph {
    /// The character as typed; repeats are kept so the string reads naturally.
    ch: String,
    name: String,
    /// Advance width in pixels at this size.
    advance: f64,
    /// SVG path data in pixels, y down, baseline at 0.
    hinted: String,
    unhinted: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelSize {
    ppem: u16,
    glyphs: Vec<PixelGlyph>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelPreviewView {
    units_per_em: u16,
    sizes: Vec<PixelSize>,
    /// Why hinting could not be applied (empty when it could). The hinted
    /// column then shows the unhinted outlines.
    problem: String,
    status: DocStatus,
}

/// Hinted and unhinted outlines of `text` at each pixel size, for the current
/// master or an interpolated location. Read-only: nothing to undo.
#[tauri::command]
pub fn pixel_preview(
    state: State<AppState>,
    text: String,
    master_id: String,
    location: Option<Location>,
    sizes: Vec<u16>,
) -> CmdResult<PixelPreviewView> {
    let mut guard = lock(&state)?;
    preview(
        doc_mut(&mut guard)?,
        &text,
        &master_id,
        location.as_ref(),
        &sizes,
    )
}

fn preview(
    doc: &mut Document,
    text: &str,
    master_id: &str,
    location: Option<&Location>,
    sizes: &[u16],
) -> CmdResult<PixelPreviewView> {
    if text.is_empty() {
        return Err("Type some text to preview.".into());
    }
    if sizes.is_empty() {
        return Err("Choose at least one pixel size.".into());
    }
    if sizes.iter().any(|&s| s == 0 || s > 512) {
        return Err("Pixel sizes must be between 1 and 512.".into());
    }
    let mut ppems = Vec::new();
    for &s in sizes {
        if !ppems.contains(&s) {
            ppems.push(s);
        }
    }
    // The panel shows a sample, not a paragraph.
    let chars: Vec<char> = text.chars().take(100).collect();

    let (bytes, order) = crate::features::preview_font(doc, master_id, location)?;
    let (hinted_bytes, problem) = match kernest_export::hint_preview(&bytes, &doc.font, location) {
        Ok(h) => (h, String::new()),
        Err(e) => (bytes.clone(), e),
    };
    let plain =
        FontRef::new(&bytes).map_err(|e| format!("could not read the preview font: {e}"))?;
    let hinted = FontRef::new(&hinted_bytes)
        .map_err(|e| format!("could not read the hinted preview font: {e}"))?;
    let plain_outlines = plain.outline_glyphs();
    let hinted_outlines = hinted.outline_glyphs();
    let charmap = plain.charmap();

    let mut out_sizes = Vec::new();
    for ppem in ppems {
        let size = Size::new(f32::from(ppem));
        // Strong hinting matches the aliased pixel grid the panel draws.
        let instance = if problem.is_empty() {
            HintingInstance::new(
                &hinted_outlines,
                size,
                LocationRef::default(),
                HintingMode::Strong,
            )
            .ok()
        } else {
            None
        };
        let metrics = hinted.glyph_metrics(size, LocationRef::default());
        let mut drawn: HashMap<char, PixelGlyph> = HashMap::new();
        let mut glyphs = Vec::new();
        for &ch in &chars {
            if let Some(hit) = drawn.get(&ch) {
                glyphs.push(PixelGlyph {
                    ch: hit.ch.clone(),
                    name: hit.name.clone(),
                    advance: hit.advance,
                    hinted: hit.hinted.clone(),
                    unhinted: hit.unhinted.clone(),
                });
                continue;
            }
            let gid = charmap.map(ch).unwrap_or(GlyphId::new(0));
            let name = order
                .get(gid.to_u32() as usize)
                .cloned()
                .unwrap_or_else(|| format!("gid{}", gid.to_u32()));
            let advance = f64::from(metrics.advance_width(gid).unwrap_or(0.0));
            let unhinted = draw_svg(
                &plain_outlines,
                gid,
                DrawSettings::unhinted(size, LocationRef::default()),
            );
            let hinted_path = match &instance {
                Some(inst) => draw_svg(&hinted_outlines, gid, DrawSettings::hinted(inst, false)),
                None => unhinted.clone(),
            };
            let g = PixelGlyph {
                ch: ch.to_string(),
                name,
                advance,
                hinted: hinted_path,
                unhinted,
            };
            drawn.insert(ch, g.clone());
            glyphs.push(g);
        }
        out_sizes.push(PixelSize { ppem, glyphs });
    }
    Ok(PixelPreviewView {
        units_per_em: doc.font.info.units_per_em,
        sizes: out_sizes,
        problem,
        status: status(doc),
    })
}

/// One glyph's outline as SVG path data in pixels, y down (screen space).
fn draw_svg(
    outlines: &skrifa::outline::OutlineGlyphCollection,
    gid: GlyphId,
    settings: DrawSettings,
) -> String {
    let Some(g) = outlines.get(gid) else {
        return String::new();
    };
    let mut elems: Vec<PathElement> = Vec::new();
    if g.draw(settings, &mut elems).is_err() {
        return String::new();
    }
    let n = |v: f32| format!("{v:.2}");
    let mut s = String::new();
    for e in &elems {
        match e {
            PathElement::MoveTo { x, y } => s.push_str(&format!("M{} {} ", n(*x), n(-y))),
            PathElement::LineTo { x, y } => s.push_str(&format!("L{} {} ", n(*x), n(-y))),
            PathElement::QuadTo { cx0, cy0, x, y } => {
                s.push_str(&format!("Q{} {} {} {} ", n(*cx0), n(-cy0), n(*x), n(-y)))
            }
            PathElement::CurveTo {
                cx0,
                cy0,
                cx1,
                cy1,
                x,
                y,
            } => s.push_str(&format!(
                "C{} {} {} {} {} {} ",
                n(*cx0),
                n(-cy0),
                n(*cx1),
                n(-cy1),
                n(*x),
                n(-y)
            )),
            PathElement::Close => s.push('Z'),
        }
    }
    s.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;
    use kernest_core::{Stem, Zone};

    fn hinted_doc() -> Document {
        let mut f = sample_font();
        let id = f.masters[0].id.clone();
        f = kernest_core::hinting::set_stems(
            &f,
            &id,
            vec![Stem {
                name: "h".into(),
                horizontal: true,
                value: 100.0,
            }],
        )
        .unwrap();
        f = kernest_core::hinting::set_alignment_zones(
            &f,
            &id,
            vec![
                Zone {
                    position: 0.0,
                    size: -10.0,
                },
                Zone {
                    position: 700.0,
                    size: 10.0,
                },
            ],
        )
        .unwrap();
        Document::new(f, None)
    }

    #[test]
    fn hinted_outlines_differ_from_unhinted_at_12_ppem() {
        let mut doc = hinted_doc();
        let v = preview(&mut doc, "H", "m01", None, &[12]).unwrap();
        assert!(v.problem.is_empty(), "{}", v.problem);
        assert_eq!(v.sizes.len(), 1);
        let g = &v.sizes[0].glyphs[0];
        assert_eq!(g.name, "H");
        assert!(!g.unhinted.is_empty());
        assert_ne!(g.hinted, g.unhinted, "hinting must move edges at 12 ppem");
        assert!(g.advance > 0.0);
    }

    #[test]
    fn bad_control_instructions_report_a_problem() {
        let mut doc = hinted_doc();
        doc.font.info.control_instructions = "H point 9999 @ 12".into();
        let v = preview(&mut doc, "H", "m01", None, &[12]).unwrap();
        assert!(!v.problem.is_empty());
        let g = &v.sizes[0].glyphs[0];
        assert_eq!(g.hinted, g.unhinted, "the hinted column falls back");
    }

    #[test]
    fn text_and_sizes_are_checked() {
        let mut doc = hinted_doc();
        assert!(preview(&mut doc, "", "m01", None, &[12]).is_err());
        assert!(preview(&mut doc, "H", "m01", None, &[]).is_err());
        assert!(preview(&mut doc, "H", "m01", None, &[0]).is_err());
        assert!(preview(&mut doc, "H", "m01", None, &[600]).is_err());
    }

    #[test]
    fn repeated_characters_and_spaces_keep_their_place() {
        let mut doc = hinted_doc();
        let v = preview(&mut doc, "H H", "m01", None, &[12, 12, 24]).unwrap();
        assert_eq!(v.sizes.len(), 2, "duplicate sizes are merged");
        let names: Vec<&str> = v.sizes[0].glyphs.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["H", "space", "H"]);
        assert!(v.sizes[0].glyphs[1].hinted.is_empty());
        assert!(v.sizes[0].glyphs[1].advance > 0.0);
        assert_eq!(v.sizes[0].glyphs[0].hinted, v.sizes[0].glyphs[2].hinted);
    }
}
