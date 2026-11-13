//! Commands for adding and removing glyphs (undoable as one font-wide change each).

use kernest_core::glyphdata::{codepoints_for, name_for};
use kernest_core::glyphset::{
    add_glyphs as add, parse_glyph_list, remove_glyphs as remove, NewGlyph,
};
use serde::Serialize;
use tauri::State;

use crate::commands::{doc_mut, lock, status, AppState, CmdResult, DocStatus};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddGlyphsResult {
    added: Vec<String>,
    /// Already in the font (same name or code point).
    skipped: Vec<String>,
    status: DocStatus,
}

/// Adds the glyphs typed in `text` (names, characters, U+ code points) plus every
/// code point of the chosen glyph sets.
#[tauri::command]
pub fn add_glyphs(
    state: State<AppState>,
    text: String,
    glyph_sets: Vec<String>,
) -> CmdResult<AddGlyphsResult> {
    let mut list = parse_glyph_list(&text)?;
    let ids: Vec<&str> = glyph_sets.iter().map(String::as_str).collect();
    for cp in codepoints_for(&ids) {
        let name = name_for(cp);
        if !list.iter().any(|g| g.name == name) {
            list.push(NewGlyph {
                name,
                codepoint: Some(cp),
            });
        }
    }
    if list.is_empty() {
        return Err("Type glyph names or characters, or choose a glyph set.".into());
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let r = add(&doc.font, &list)?;
    if doc
        .history
        .apply_font(&mut doc.font, r.font)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(AddGlyphsResult {
        added: r.added,
        skipped: r.skipped,
        status: status(doc),
    })
}

/// Sets whether glyphs are exported (unexported glyphs, such as parts used only as
/// components, are left out of compiled fonts). One undoable step.
#[tauri::command]
pub fn set_glyph_export(
    state: State<AppState>,
    names: Vec<String>,
    export: bool,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let mut next = doc.font.clone();
    for n in &names {
        next.glyph_mut(n)
            .ok_or_else(|| format!("There is no glyph named \"{n}\"."))?
            .export = export;
    }
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Removes glyphs; components using them are decomposed in the glyphs that remain.
#[tauri::command]
pub fn remove_glyphs(state: State<AppState>, names: Vec<String>) -> CmdResult<DocStatus> {
    if names.is_empty() {
        return Err("Select the glyphs to remove.".into());
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = remove(&doc.font, &names)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}
