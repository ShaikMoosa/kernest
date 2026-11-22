//! Commands for adding and removing glyphs (undoable as one font-wide change each).

use kernest_core::glyphdata::{codepoints_for, name_for};
use kernest_core::glyphset::{
    add_glyphs as add, missing_glyphs as missing, parse_glyph_list, remove_glyphs as remove,
    valid_name, NewGlyph,
};
use kernest_core::model::Font;
use kernest_core::rename;
use kernest_core::smartfilter::{self, Filter};
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::State;

use crate::commands::{
    doc_mut, ensure_glyph_unlocked, lock, status, AppState, CmdResult, DocStatus,
};

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageCoverage {
    language: &'static str,
    present: usize,
    total: usize,
}

/// Per-language coverage for the Font View sidebar; read-only.
#[tauri::command]
pub fn language_coverage(state: State<AppState>) -> CmdResult<Vec<LanguageCoverage>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    Ok(kernest_core::glyphset::coverage(&doc.font)
        .into_iter()
        .map(|c| LanguageCoverage {
            language: c.language,
            present: c.present,
            total: c.total,
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingGlyph {
    name: String,
    unicode: String,
}

/// The glyphs a language still needs; read-only, loaded when its row is opened.
#[tauri::command]
pub fn missing_glyphs(state: State<AppState>, language: String) -> CmdResult<Vec<MissingGlyph>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    Ok(missing(&doc.font, &language)?
        .into_iter()
        .map(|g| MissingGlyph {
            name: g.name,
            unicode: g
                .codepoint
                .map(|cp| format!("{cp:04X}"))
                .unwrap_or_default(),
        })
        .collect())
}

/// Adds every glyph a language is missing, as one undoable step.
#[tauri::command]
pub fn add_missing(state: State<AppState>, language: String) -> CmdResult<AddGlyphsResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let list = missing(&doc.font, &language)?;
    if list.is_empty() {
        return Ok(AddGlyphsResult {
            added: Vec::new(),
            skipped: Vec::new(),
            status: status(doc),
        });
    }
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

/// Sets a glyph's tags, as typed comma-separated in the info box or the list view.
/// One undoable step.
#[tauri::command]
pub fn set_glyph_tags(
    state: State<AppState>,
    glyph: String,
    tags: Vec<String>,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = with_tags(&doc.font, &glyph, &tags)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Sets a glyph's Unicode values, as typed in the info box or the list view (empty
/// leaves it unencoded). With `take`, values used by other glyphs move over.
/// One undoable step.
#[tauri::command]
pub fn set_glyph_unicodes(
    state: State<AppState>,
    glyph: String,
    unicodes: String,
    take: bool,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = kernest_core::glyphset::set_unicodes(&doc.font, &glyph, &unicodes, take)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Tags are trimmed; empty ones and repeats (first spelling wins) are dropped.
fn with_tags(font: &Font, name: &str, tags: &[String]) -> Result<Font, String> {
    let mut next = font.clone();
    let g = next
        .glyph_mut(name)
        .ok_or_else(|| format!("There is no glyph named \"{name}\"."))?;
    let mut clean: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !clean.iter().any(|c| c == tag) {
            clean.push(tag.to_string());
        }
    }
    g.tags = clean;
    Ok(next)
}

/// Sets a glyph's production name, as typed in the info box (empty clears it).
/// One undoable step.
#[tauri::command]
pub fn set_production_name(
    state: State<AppState>,
    glyph: String,
    name: String,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let next = with_production_name(&doc.font, &glyph, &name)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Sets a glyph property override (category, subCategory, case, script or sortName)
/// on the given glyphs; an empty value goes back to automatic. One undoable step.
#[tauri::command]
pub fn set_glyph_property(
    state: State<AppState>,
    glyphs: Vec<String>,
    property: String,
    value: String,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = kernest_core::glyphprops::set(&doc.font, &glyphs, &property, &value)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Sets the colour label of the given glyphs, or of their layer `layer` (a master id
/// or layer key); `None` clears it. One undoable step.
#[tauri::command]
pub fn set_color_label(
    state: State<AppState>,
    glyphs: Vec<String>,
    layer: Option<String>,
    label: Option<kernest_core::ColorLabel>,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next =
        kernest_core::glyphprops::set_color_label(&doc.font, &glyphs, layer.as_deref(), label)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Sets a glyph's writing direction (`LTR`, `RTL`, `BIDI`; empty for automatic).
/// One undoable step.
#[tauri::command]
pub fn set_writing_direction(
    state: State<AppState>,
    glyph: String,
    direction: String,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let dir =
        match direction.trim() {
            "" => None,
            d => Some(kernest_core::WritingDirection::parse(d).ok_or_else(|| {
                format!("\"{d}\" isn't a writing direction; use LTR, RTL or BIDI.")
            })?),
        };
    let mut next = doc.font.clone();
    next.glyph_mut(&glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?
        .direction = dir;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// Sets one vertical metric of a glyph in a master: `advance` (vertical width),
/// `tsb` (moves the vertical origin) or `bsb` (changes the advance). `reset` clears
/// the layer's own values so the defaults apply. One undoable step.
#[tauri::command]
pub fn set_vertical_metric(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    which: String,
    value: Option<f64>,
) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, &glyph)?;
    let master = doc
        .font
        .master(&master_id)
        .ok_or_else(|| format!("No master with id \"{master_id}\""))?
        .clone();
    let mut next = doc.font.clone();
    let font = doc.font.clone();
    let layer = next
        .glyph_mut(&glyph)
        .ok_or_else(|| format!("There is no glyph named \"{glyph}\"."))?
        .layer_mut(&master_id)
        .ok_or_else(|| format!("Glyph \"{glyph}\" has no layer for this master."))?;
    let need = |v: Option<f64>| -> Result<f64, String> {
        v.filter(|v| v.is_finite())
            .ok_or_else(|| "Enter a number.".to_string())
    };
    let empty =
        || format!("Glyph \"{glyph}\" has no outline, so it has no top or bottom sidebearing.");
    match which.as_str() {
        "advance" => {
            let v = need(value)?;
            if v < 0.0 {
                return Err("The vertical width can't be negative.".into());
            }
            layer.vert_width = Some(v);
        }
        "tsb" => kernest_core::vertical::set_tsb(&font, &master, layer, need(value)?)
            .ok_or_else(empty)?,
        "bsb" => kernest_core::vertical::set_bsb(&font, &master, layer, need(value)?)
            .ok_or_else(empty)?,
        "reset" => {
            layer.vert_width = None;
            layer.vert_origin = None;
        }
        other => return Err(format!("Unknown vertical metric \"{other}\".")),
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

/// Production names follow the glyph-name rules and must not clash with the name
/// any other glyph exports under (its production name, or its own name).
fn with_production_name(font: &Font, name: &str, production: &str) -> Result<Font, String> {
    let production = production.trim();
    if font.glyph(name).is_none() {
        return Err(format!("There is no glyph named \"{name}\"."));
    }
    if !production.is_empty() {
        if !valid_name(production) {
            return Err(format!(
                "\"{production}\" can't be used as a production name."
            ));
        }
        if font.glyphs.iter().any(|o| {
            o.name != name && o.production_name.as_deref().unwrap_or(&o.name) == production
        }) {
            return Err(format!(
                "\"{production}\" is already used as a glyph name or production name."
            ));
        }
    }
    let mut next = font.clone();
    if let Some(g) = next.glyph_mut(name) {
        g.production_name = (!production.is_empty()).then(|| production.to_string());
    }
    Ok(next)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateResult {
    added: Vec<String>,
    status: DocStatus,
}

/// Glyph › Duplicate Glyph: copies named `a.001` right after each glyph.
#[tauri::command]
pub fn duplicate_glyphs(state: State<AppState>, names: Vec<String>) -> CmdResult<DuplicateResult> {
    if names.is_empty() {
        return Err("Select the glyphs to duplicate.".into());
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let (next, added) = kernest_core::clipboard::duplicate(&doc.font, &names)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(DuplicateResult {
        added,
        status: status(doc),
    })
}

/// Removes glyphs; components using them are decomposed in the glyphs that remain.
#[tauri::command]
pub fn remove_glyphs(state: State<AppState>, names: Vec<String>) -> CmdResult<DocStatus> {
    if names.is_empty() {
        return Err("Select the glyphs to remove.".into());
    }
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    for n in &names {
        ensure_glyph_unlocked(&doc.font, n)?;
    }
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

/// Replaces the font's saved smart and list filters. One undoable step.
#[tauri::command]
pub fn set_filters(state: State<AppState>, filters: Vec<Filter>) -> CmdResult<DocStatus> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    let next = smartfilter::set_filters(&doc.font, filters)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(status(doc))
}

/// The glyph names a filter matches, for the rule builder's live count. Read-only:
/// the filter being edited is passed in, not taken from the font.
#[tauri::command]
pub fn evaluate_filter(state: State<AppState>, filter: Filter) -> CmdResult<Vec<String>> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    Ok(smartfilter::evaluate(&doc.font, &filter))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameResult {
    status: DocStatus,
    /// Feature-code lines the rename rewrote, shown as a warning so the designer
    /// can check them.
    changed_feature_lines: Vec<String>,
}

/// Renames glyphs from an old→new map (the find/replace pattern runs in the
/// frontend) and updates every reference. One undoable step.
#[tauri::command]
pub fn rename_glyphs(
    state: State<AppState>,
    renames: BTreeMap<String, String>,
) -> CmdResult<RenameResult> {
    let mut guard = lock(&state)?;
    let doc = doc_mut(&mut guard)?;
    for old in renames.keys() {
        ensure_glyph_unlocked(&doc.font, old)?;
    }
    let (next, changed) = rename::rename_glyphs(&doc.font, &renames)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(RenameResult {
        status: status(doc),
        changed_feature_lines: changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;

    #[test]
    fn tags_are_trimmed_deduped_and_clearable() {
        let font = sample_font();
        let next = with_tags(
            &font,
            "H",
            &["  red ".into(), "red".into(), " ".into(), "blue".into()],
        )
        .unwrap();
        assert_eq!(next.glyph("H").unwrap().tags, ["red", "blue"]);
        assert!(with_tags(&font, "nope", &["x".into()]).is_err());
        let cleared = with_tags(&next, "H", &[]).unwrap();
        assert!(cleared.glyph("H").unwrap().tags.is_empty());
    }

    #[test]
    fn production_names_validate_and_clear() {
        let font = sample_font();
        let next = with_production_name(&font, "H", " Hprod ").unwrap();
        assert_eq!(
            next.glyph("H").unwrap().production_name.as_deref(),
            Some("Hprod")
        );
        assert!(with_production_name(&font, "H", "9bad").is_err());
        assert!(with_production_name(&font, "nope", "x").is_err());
        // Clashes with another glyph's name or production name are refused.
        assert!(with_production_name(&font, "H", "O").is_err());
        assert!(with_production_name(&next, "I", "Hprod").is_err());
        // A name another glyph exports under a different production name is free.
        let moved = with_production_name(&font, "O", "Oh").unwrap();
        assert!(with_production_name(&moved, "H", "O").is_ok());
        let cleared = with_production_name(&next, "H", "  ").unwrap();
        assert!(cleared.glyph("H").unwrap().production_name.is_none());
    }
}
