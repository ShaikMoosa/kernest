//! Commands for Font Info (family, axes, masters, instances) and compatibility.

use std::collections::BTreeMap;

use kernest_core::interpolate::{check_glyph, default_master};
use kernest_core::{structure, Axis, Font, Grid, Guide, Hinting, Instance, Metrics};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{doc_mut, lock, status, AppState, CmdResult, DocStatus, Document};

/// The naming fields edited on the General tab. Empty strings clear a field.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FontNames {
    pub designer: String,
    pub designer_url: String,
    pub manufacturer: String,
    pub manufacturer_url: String,
    pub copyright: String,
    pub vendor_id: String,
    pub license: String,
    pub license_url: String,
    pub description: String,
    pub trademark: String,
    pub sample_text: String,
}

fn opt(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

impl FontNames {
    fn from_info(i: &kernest_core::FontInfo) -> Self {
        let get = |v: &Option<String>| v.clone().unwrap_or_default();
        FontNames {
            designer: get(&i.designer),
            designer_url: get(&i.designer_url),
            manufacturer: get(&i.manufacturer),
            manufacturer_url: get(&i.manufacturer_url),
            copyright: get(&i.copyright),
            vendor_id: get(&i.vendor_id),
            license: get(&i.license),
            license_url: get(&i.license_url),
            description: get(&i.description),
            trademark: get(&i.trademark),
            sample_text: get(&i.sample_text),
        }
    }

    fn apply(&self, i: &mut kernest_core::FontInfo) {
        i.designer = opt(&self.designer);
        i.designer_url = opt(&self.designer_url);
        i.manufacturer = opt(&self.manufacturer);
        i.manufacturer_url = opt(&self.manufacturer_url);
        i.copyright = opt(&self.copyright);
        i.vendor_id = opt(&self.vendor_id);
        i.license = opt(&self.license);
        i.license_url = opt(&self.license_url);
        i.description = opt(&self.description);
        i.trademark = opt(&self.trademark);
        i.sample_text = opt(&self.sample_text);
    }
}

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
    names: FontNames,
    /// Designer notes, saved with the font but never exported.
    notes: String,
    axes: Vec<Axis>,
    masters: Vec<MasterView>,
    instances: Vec<Instance>,
    /// TrueType autohinting settings (the defaults if the font has none).
    hinting: Hinting,
    /// The edit view's snap grid, if set.
    grid: Option<Grid>,
    /// Colour palettes (CSS colours `#rrggbbaa`), all the same length.
    palettes: Vec<Vec<kernest_core::color::Color>>,
    default_master_id: String,
    /// The working glyph order; export follows it (`.notdef` always first).
    glyph_order: Vec<String>,
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
        names: FontNames::from_info(&f.info),
        notes: f.notes.clone(),
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
        grid: f.info.grid,
        palettes: f.palettes.clone(),
        default_master_id: default_master(f).id.clone(),
        glyph_order: f.glyphs.iter().map(|g| g.name.clone()).collect(),
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
pub fn set_font_names(state: State<AppState>, names: FontNames) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        let mut next = f.clone();
        names.apply(&mut next.info);
        Ok(next)
    })
}

#[tauri::command]
pub fn set_notes(state: State<AppState>, text: String) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        let mut next = f.clone();
        next.notes = text;
        Ok(next)
    })
}

#[tauri::command]
pub fn set_hinting(state: State<AppState>, hinting: Hinting) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_hinting(f, hinting))
}

/// Sets or clears the edit view's snap grid (Font Info › General).
#[tauri::command]
pub fn set_grid(state: State<AppState>, grid: Option<Grid>) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_grid(f, grid))
}

/// Replaces a master's global guides.
#[tauri::command]
pub fn set_master_guides(
    state: State<AppState>,
    master_id: String,
    guides: Vec<Guide>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        structure::set_master_guides(f, &master_id, guides)
    })
}

#[tauri::command]
pub fn set_palettes(
    state: State<AppState>,
    palettes: Vec<Vec<kernest_core::color::Color>>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| kernest_core::color::set_palettes(f, palettes))
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

/// Sets the working glyph order (Font View drag or the Font Info text); export follows it.
#[tauri::command]
pub fn set_glyph_order(state: State<AppState>, order: Vec<String>) -> CmdResult<FontInfoView> {
    edit(&state, |f| kernest_core::order::set_order(f, &order))
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
