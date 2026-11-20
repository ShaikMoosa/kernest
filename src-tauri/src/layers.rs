//! Commands for brace (intermediate) and bracket (alternate) layers. Each edit is one
//! undoable font-wide change.

use std::collections::BTreeMap;

use kernest_core::color::{self, Paint};
use kernest_core::{layers, smart, AxisRange, Font, SmartAxis};
use serde::Serialize;
use tauri::State;

use crate::commands::{
    css_palette_at, doc_mut, ensure_glyph_unlocked, lock, status, AppState, CmdResult, DocStatus,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    /// What Edit View and the other layer commands use to address this layer.
    key: String,
    /// "master" | "brace" | "bracket" | "smart" | "color"
    kind: &'static str,
    /// e.g. "Regular", "{wght 550}", "[wght 600–]"
    label: String,
    master_id: String,
    master_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<BTreeMap<String, f64>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bracket: Vec<AxisRange>,
    /// A colour layer's paint.
    #[serde(skip_serializing_if = "Option::is_none")]
    paint: Option<Paint>,
}

/// Every layer of a glyph: master layers in master order, then brace and bracket layers.
#[tauri::command]
pub fn glyph_layers(state: State<AppState>, glyph: String) -> CmdResult<Vec<LayerInfo>> {
    let mut guard = lock(&state)?;
    let font = &doc_mut(&mut guard)?.font;
    let g = font
        .glyph(&glyph)
        .ok_or_else(|| format!("No glyph named \"{glyph}\""))?;
    let master_name = |id: &str| font.master(id).map(|m| m.name.clone()).unwrap_or_default();
    let mut out: Vec<LayerInfo> = font
        .masters
        .iter()
        .filter_map(|m| g.layer(&m.id))
        .map(|l| LayerInfo {
            key: l.key().to_string(),
            kind: "master",
            label: master_name(&l.master_id),
            master_id: l.master_id.clone(),
            master_name: master_name(&l.master_id),
            location: None,
            bracket: Vec::new(),
            paint: None,
        })
        .collect();
    out.extend(g.layers.iter().filter(|l| !l.is_master()).map(|l| {
        LayerInfo {
            key: l.key().to_string(),
            kind: if l.location.is_some() {
                "brace"
            } else if l.backup.is_some() {
                "backup"
            } else if l.smart.is_some() {
                "smart"
            } else if l.color.is_some() {
                "color"
            } else {
                "bracket"
            },
            // Colour layers are numbered from the bottom of their master's stack.
            label: match &l.color {
                Some(_) => g
                    .color_layers(&l.master_id)
                    .position(|x| x.key() == l.key())
                    .map(|i| (i + 1).to_string())
                    .unwrap_or_default(),
                None => l.backup.clone().or_else(|| l.label()).unwrap_or_default(),
            },
            master_id: l.master_id.clone(),
            master_name: master_name(&l.master_id),
            location: l.location.clone(),
            bracket: l.bracket.clone(),
            paint: l.color.as_ref().map(|c| c.paint.clone()),
        }
    }));
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerAdded {
    key: String,
    status: DocStatus,
}

fn apply(
    state: &AppState,
    glyph: &str,
    f: impl FnOnce(&Font) -> Result<(Font, String), String>,
) -> CmdResult<LayerAdded> {
    let mut guard = lock(state)?;
    let doc = doc_mut(&mut guard)?;
    ensure_glyph_unlocked(&doc.font, glyph)?;
    let (next, key) = f(&doc.font)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(LayerAdded {
        key,
        status: status(doc),
    })
}

/// Adds a brace layer at `location` (design units by axis tag), shown with `master_id`.
#[tauri::command]
pub fn add_brace_layer(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    location: BTreeMap<String, f64>,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        layers::add_brace_layer(f, &glyph, &master_id, &location)
    })
}

/// Adds an alternate: bracket layers with `ranges` for every master.
#[tauri::command]
pub fn add_bracket_layers(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    ranges: Vec<AxisRange>,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        layers::add_bracket_layers(f, &glyph, &master_id, &ranges)
    })
}

/// Moves a brace layer (`location`) or changes an alternate's ranges (`ranges`).
#[tauri::command]
pub fn update_layer_position(
    state: State<AppState>,
    glyph: String,
    key: String,
    location: Option<BTreeMap<String, f64>>,
    ranges: Option<Vec<AxisRange>>,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        let next =
            layers::update_layer_position(f, &glyph, &key, location.as_ref(), ranges.as_deref())?;
        Ok((next, key.clone()))
    })
}

/// Gives a glyph smart axes (an empty list makes it an ordinary glyph again).
#[tauri::command]
pub fn set_smart_axes(
    state: State<AppState>,
    glyph: String,
    axes: Vec<SmartAxis>,
) -> CmdResult<DocStatus> {
    let r = apply(&state, &glyph, |f| {
        Ok((smart::set_smart_axes(f, &glyph, axes)?, String::new()))
    })?;
    Ok(r.status)
}

/// Adds a smart pole layer for `master_id` at `pole` (axis name → value).
#[tauri::command]
pub fn add_smart_pole(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    pole: BTreeMap<String, f64>,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        smart::add_pole(f, &glyph, &master_id, &pole)
    })
}

/// Removes a brace layer, or a whole alternate.
#[tauri::command]
pub fn remove_layer(state: State<AppState>, glyph: String, key: String) -> CmdResult<DocStatus> {
    let r = apply(&state, &glyph, |f| {
        Ok((layers::remove_layer(f, &glyph, &key)?, String::new()))
    })?;
    Ok(r.status)
}

/// Saves the master layer's drawing as a backup named `name`.
#[tauri::command]
pub fn add_backup_layer(
    state: State<AppState>,
    glyph: String,
    master_id: String,
    name: String,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        layers::add_backup(f, &glyph, &master_id, &name)
    })
}

/// Swaps a backup's drawing with its master's.
#[tauri::command]
pub fn use_backup_layer(
    state: State<AppState>,
    glyph: String,
    key: String,
) -> CmdResult<DocStatus> {
    let r = apply(&state, &glyph, |f| {
        Ok((layers::use_backup(f, &glyph, &key)?, String::new()))
    })?;
    Ok(r.status)
}

/// Adds a colour layer on top of the glyph's others, in every master.
#[tauri::command]
pub fn add_color_layer(
    state: State<AppState>,
    glyph: String,
    master_id: String,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        color::add_color_layer(f, &glyph, &master_id)
    })
}

/// Sets a colour layer's paint (in every master).
#[tauri::command]
pub fn set_color_paint(
    state: State<AppState>,
    glyph: String,
    key: String,
    paint: Paint,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        Ok((color::set_paint(f, &glyph, &key, paint)?, key.clone()))
    })
}

/// Sets a colour layer's paint in one master only, so colours can differ between masters.
#[tauri::command]
pub fn set_master_paint(
    state: State<AppState>,
    glyph: String,
    key: String,
    master_id: String,
    paint: Paint,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        Ok((
            color::set_master_paint(f, &glyph, &key, &master_id, paint)?,
            key.clone(),
        ))
    })
}

/// A colour layer's paint in one master, with that master's effective palette.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterPaint {
    paint: Paint,
    palette: Vec<String>,
}

/// Reads one master's paint for the colour layer at `key`'s stack position.
#[tauri::command]
pub fn master_color_paint(
    state: State<AppState>,
    glyph: String,
    key: String,
    master_id: String,
) -> CmdResult<MasterPaint> {
    let mut guard = lock(&state)?;
    let font = &doc_mut(&mut guard)?.font;
    let paint = color::master_paint(font, &glyph, &key, &master_id)?;
    Ok(MasterPaint {
        paint,
        palette: css_palette_at(font, &master_id),
    })
}

/// Moves a colour layer up or down the stack (in every master).
#[tauri::command]
pub fn move_color_layer(
    state: State<AppState>,
    glyph: String,
    key: String,
    up: bool,
) -> CmdResult<LayerAdded> {
    apply(&state, &glyph, |f| {
        Ok((color::move_color_layer(f, &glyph, &key, up)?, key.clone()))
    })
}
