//! Commands the TypeScript UI calls through Tauri IPC.
//!
//! The open document lives in Rust; the UI receives read-only views shaped for
//! each screen. Errors are returned as user-facing strings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kernest_core::geometry::{sidebearings, svg_path};
use kernest_core::glyphdata::{codepoints_for, GLYPH_SETS};
use kernest_core::{package, sample, Font, Metrics};
use serde::Serialize;
use tauri::State;

pub struct Document {
    pub font: Font,
    pub path: Option<PathBuf>,
}

#[derive(Default)]
pub struct AppState(Mutex<Option<Document>>);

type CmdResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphSetInfo {
    id: &'static str,
    name: &'static str,
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterInfo {
    id: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphSummary {
    name: String,
    unicode: Option<String>,
    character: Option<String>,
    category: &'static str,
    export: bool,
    width: f64,
    lsb: Option<f64>,
    rsb: Option<f64>,
    /// Outline as SVG path data in font units (y up), empty when not drawn yet.
    svg: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontView {
    family_name: String,
    path: Option<String>,
    units_per_em: u16,
    masters: Vec<MasterInfo>,
    master_id: String,
    metrics: Metrics,
    glyphs: Vec<GlyphSummary>,
}

fn view(doc: &Document, master_id: Option<&str>) -> CmdResult<FontView> {
    let font = &doc.font;
    let master = match master_id {
        Some(id) => font
            .master(id)
            .ok_or_else(|| format!("No master with id \"{id}\""))?,
        None => &font.masters[0],
    };
    let glyphs = font
        .glyphs
        .iter()
        .map(|g| {
            let layer = g.layer(&master.id);
            let sb = layer.and_then(|l| sidebearings(font, l));
            let cp = g.unicodes.first();
            GlyphSummary {
                name: g.name.clone(),
                unicode: cp.map(|c| c.to_string()),
                character: cp.and_then(|c| c.char()).map(String::from),
                category: g.category().label(),
                export: g.export,
                width: layer.map_or(0.0, |l| l.width),
                lsb: sb.map(|s| s.0),
                rsb: sb.map(|s| s.1),
                svg: layer.map(|l| svg_path(font, l)).unwrap_or_default(),
            }
        })
        .collect();
    Ok(FontView {
        family_name: font.info.family_name.clone(),
        path: doc.path.as_ref().map(|p| p.display().to_string()),
        units_per_em: font.info.units_per_em,
        masters: font
            .masters
            .iter()
            .map(|m| MasterInfo {
                id: m.id.clone(),
                name: m.name.clone(),
            })
            .collect(),
        master_id: master.id.clone(),
        metrics: master.metrics,
        glyphs,
    })
}

fn replace(state: &AppState, doc: Document) -> CmdResult<FontView> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "Internal error: state lock poisoned".to_string())?;
    let v = view(&doc, None)?;
    *guard = Some(doc);
    Ok(v)
}

#[tauri::command]
pub fn glyph_sets() -> Vec<GlyphSetInfo> {
    GLYPH_SETS
        .iter()
        .map(|s| GlyphSetInfo {
            id: s.id,
            name: s.name,
            count: codepoints_for(&[s.id]).len(),
        })
        .collect()
}

#[tauri::command]
pub fn new_font(
    state: State<AppState>,
    family_name: String,
    glyph_sets: Vec<String>,
) -> CmdResult<FontView> {
    let family = family_name.trim();
    if family.is_empty() {
        return Err("Enter a family name.".into());
    }
    let ids: Vec<&str> = glyph_sets.iter().map(String::as_str).collect();
    let font = Font::new(family, codepoints_for(&ids));
    replace(&state, Document { font, path: None })
}

#[tauri::command]
pub fn open_sample(state: State<AppState>) -> CmdResult<FontView> {
    replace(
        &state,
        Document {
            font: sample::sample_font(),
            path: None,
        },
    )
}

#[tauri::command]
pub fn open_font(state: State<AppState>, path: String) -> CmdResult<FontView> {
    let mut dir = PathBuf::from(&path);
    // Accept the package folder or any file inside it (e.g. font.json picked in a file dialog).
    if dir.is_file() {
        dir = dir.parent().map(Path::to_path_buf).unwrap_or(dir);
        if dir.file_name().is_some_and(|n| n == "glyphs") {
            dir.pop();
        }
    }
    let font = package::open(&dir).map_err(|e| e.to_string())?;
    replace(
        &state,
        Document {
            font,
            path: Some(dir),
        },
    )
}

/// Saves to `path` if given (Save As), otherwise to the document's current path.
/// Returns the path saved to.
#[tauri::command]
pub fn save_font(state: State<AppState>, path: Option<String>) -> CmdResult<String> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "Internal error: state lock poisoned".to_string())?;
    let doc = guard.as_mut().ok_or("No font is open.")?;
    let target = match path {
        Some(p) => {
            let mut p = PathBuf::from(p);
            if p.extension().is_none_or(|e| e != package::EXTENSION) {
                p.set_extension(package::EXTENSION);
            }
            p
        }
        None => doc
            .path
            .clone()
            .ok_or("This font has not been saved yet. Use Save As.")?,
    };
    package::save(&doc.font, &target).map_err(|e| e.to_string())?;
    doc.path = Some(target.clone());
    Ok(target.display().to_string())
}

#[tauri::command]
pub fn font_view(state: State<AppState>, master_id: Option<String>) -> CmdResult<FontView> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "Internal error: state lock poisoned".to_string())?;
    let doc = guard.as_ref().ok_or("No font is open.")?;
    view(doc, master_id.as_deref())
}
