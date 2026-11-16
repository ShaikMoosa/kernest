//! Commands for brace (intermediate) and bracket (alternate) layers. Each edit is one
//! undoable font-wide change.

use std::collections::BTreeMap;

use kernest_core::{layers, smart, AxisRange, Font, SmartAxis};
use serde::Serialize;
use tauri::State;

use crate::commands::{doc_mut, lock, status, AppState, CmdResult, DocStatus};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerInfo {
    /// What Edit View and the other layer commands use to address this layer.
    key: String,
    /// "master" | "brace" | "bracket"
    kind: &'static str,
    /// e.g. "Regular", "{wght 550}", "[wght 600–]"
    label: String,
    master_id: String,
    master_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<BTreeMap<String, f64>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    bracket: Vec<AxisRange>,
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
        })
        .collect();
    out.extend(
        g.layers
            .iter()
            .filter(|l| !l.is_master())
            .map(|l| LayerInfo {
                key: l.key().to_string(),
                kind: if l.location.is_some() {
                    "brace"
                } else if l.smart.is_some() {
                    "smart"
                } else {
                    "bracket"
                },
                label: l.label().unwrap_or_default(),
                master_id: l.master_id.clone(),
                master_name: master_name(&l.master_id),
                location: l.location.clone(),
                bracket: l.bracket.clone(),
            }),
    );
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
    f: impl FnOnce(&Font) -> Result<(Font, String), String>,
) -> CmdResult<LayerAdded> {
    let mut guard = lock(state)?;
    let doc = doc_mut(&mut guard)?;
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
    apply(&state, |f| {
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
    apply(&state, |f| {
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
    apply(&state, |f| {
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
    let r = apply(&state, |f| {
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
    apply(&state, |f| smart::add_pole(f, &glyph, &master_id, &pole))
}

/// Removes a brace layer, or a whole alternate.
#[tauri::command]
pub fn remove_layer(state: State<AppState>, glyph: String, key: String) -> CmdResult<DocStatus> {
    let r = apply(&state, |f| {
        Ok((layers::remove_layer(f, &glyph, &key)?, String::new()))
    })?;
    Ok(r.status)
}
