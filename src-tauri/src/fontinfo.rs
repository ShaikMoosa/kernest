//! Commands for Font Info (family, axes, masters, instances) and compatibility.

use std::collections::BTreeMap;

use kernest_core::interpolate::{check_glyph, default_master};
use kernest_core::{structure, Axis, Font, Hinting, Instance, Metrics};
use serde::Serialize;
use tauri::State;

use crate::commands::{doc_mut, lock, status, AppState, CmdResult, DocStatus, Document};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterView {
    id: String,
    name: String,
    location: BTreeMap<String, f64>,
    metrics: Metrics,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontInfoView {
    family_name: String,
    units_per_em: u16,
    version_major: u16,
    version_minor: u16,
    axes: Vec<Axis>,
    masters: Vec<MasterView>,
    instances: Vec<Instance>,
    /// TrueType autohinting settings (the defaults if the font has none).
    hinting: Hinting,
    default_master_id: String,
    /// Design-space problems that block interpolation or export.
    warnings: Vec<String>,
    /// Number of glyphs whose masters can't be interpolated.
    incompatible: usize,
    status: DocStatus,
}

fn info_view(doc: &Document) -> FontInfoView {
    let f = &doc.font;
    FontInfoView {
        family_name: f.info.family_name.clone(),
        units_per_em: f.info.units_per_em,
        version_major: f.info.version_major,
        version_minor: f.info.version_minor,
        axes: f.axes.clone(),
        masters: f
            .masters
            .iter()
            .map(|m| MasterView {
                id: m.id.clone(),
                name: m.name.clone(),
                location: m.location.clone(),
                metrics: m.metrics,
            })
            .collect(),
        instances: f.instances.clone(),
        hinting: f.info.hinting.unwrap_or_default(),
        default_master_id: default_master(f).id.clone(),
        warnings: structure::warnings(f),
        incompatible: f
            .glyphs
            .iter()
            .filter(|g| !check_glyph(f, &g.name).is_empty())
            .count(),
        status: status(doc),
    }
}

/// Applies a structural edit as one undoable step and returns the new Font Info.
fn edit(
    state: &AppState,
    f: impl FnOnce(&Font) -> Result<Font, String>,
) -> CmdResult<FontInfoView> {
    let mut guard = lock(state)?;
    let doc = doc_mut(&mut guard)?;
    let next = f(&doc.font)?;
    if doc
        .history
        .apply_font(&mut doc.font, next)
        .map_err(|e| e.to_string())?
    {
        doc.revision += 1;
    }
    Ok(info_view(doc))
}

#[tauri::command]
pub fn font_info(state: State<AppState>) -> CmdResult<FontInfoView> {
    let mut guard = lock(&state)?;
    Ok(info_view(doc_mut(&mut guard)?))
}

#[tauri::command]
pub fn set_font_info(
    state: State<AppState>,
    family_name: String,
    units_per_em: u16,
    version_major: u16,
    version_minor: u16,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        structure::set_info(f, &family_name, units_per_em, version_major, version_minor)
    })
}

#[tauri::command]
pub fn set_hinting(state: State<AppState>, hinting: Hinting) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_hinting(f, hinting))
}

#[tauri::command]
pub fn set_axes(state: State<AppState>, axes: Vec<Axis>) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_axes(f, axes))
}

#[tauri::command]
pub fn add_master(
    state: State<AppState>,
    copy_from: String,
    name: String,
    location: BTreeMap<String, f64>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        structure::add_master(f, &copy_from, &name, &location).map(|(f, _)| f)
    })
}

#[tauri::command]
pub fn update_master(
    state: State<AppState>,
    id: String,
    name: String,
    location: BTreeMap<String, f64>,
    metrics: Metrics,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        structure::update_master(f, &id, &name, &location, metrics)
    })
}

#[tauri::command]
pub fn remove_master(state: State<AppState>, id: String) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::remove_master(f, &id))
}

#[tauri::command]
pub fn set_instances(state: State<AppState>, instances: Vec<Instance>) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_instances(f, instances))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphIssues {
    glyph: String,
    issues: Vec<String>,
}

/// Glyphs whose masters can't be interpolated, with the reasons.
#[tauri::command]
pub fn compatibility(state: State<AppState>) -> CmdResult<Vec<GlyphIssues>> {
    let mut guard = lock(&state)?;
    let f = &doc_mut(&mut guard)?.font;
    Ok(f.glyphs
        .iter()
        .filter_map(|g| {
            let issues = check_glyph(f, &g.name);
            (!issues.is_empty()).then(|| GlyphIssues {
                glyph: g.name.clone(),
                issues,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;

    #[test]
    fn info_view_reports_warnings_and_compatibility() {
        let mut doc = Document::new(sample_font(), None);
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
        };
        let next = structure::set_axes(&doc.font, vec![wght]).unwrap();
        doc.history.apply_font(&mut doc.font, next).unwrap();
        let (next, _) = structure::add_master(
            &doc.font,
            "m01",
            "Bold",
            &BTreeMap::from([("wght".into(), 700.0)]),
        )
        .unwrap();
        doc.history.apply_font(&mut doc.font, next).unwrap();
        let v = info_view(&doc);
        assert_eq!(v.masters.len(), 2);
        assert_eq!(v.default_master_id, "m01");
        assert!(v.warnings.is_empty());
        assert_eq!(v.incompatible, 0);
    }
}
