//! Commands for Font Info (family, axes, masters, instances) and compatibility.

use std::collections::BTreeMap;
use std::path::Path;

use kernest_core::interpolate::{check_glyph, default_master};
use kernest_core::{
    structure, Axis, Font, Grid, Guide, Hinting, Instance, Metrics, PsHinting, Stem,
    VariableFontSetting, Zone,
};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::{
    doc_mut, lock, replace_dirty, status, AppState, CmdResult, DocStatus, Document, FontView,
};

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
    alignment_zones: Vec<Zone>,
    stems: Vec<Stem>,
    vertical: kernest_core::VerticalMetrics,
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
    /// Several variable fonts cut from this family; empty means one at full range.
    variable_fonts: Vec<VariableFontSetting>,
    export_presets: Vec<kernest_core::presets::ExportPreset>,
    /// TrueType autohinting settings (the defaults if the font has none).
    hinting: Hinting,
    /// TTFAutohint control instructions, applied to TrueType exports.
    control_instructions: String,
    /// PostScript blue zone controls (the defaults if the font has none).
    ps_hinting: PsHinting,
    /// The edit view's snap grid, if set.
    grid: Option<Grid>,
    /// OS/2 fsSelection bit 7: applications use the typo metrics for line spacing.
    use_typo_metrics: bool,
    /// Colour palettes (CSS colours `#rrggbbaa`), all the same length.
    palettes: Vec<Vec<kernest_core::color::Color>>,
    /// Per-master palette overrides, keyed by master id; empty means none.
    master_palettes: BTreeMap<String, Vec<Vec<kernest_core::color::Color>>>,
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
                alignment_zones: m.alignment_zones.clone(),
                stems: m.stems.clone(),
                vertical: m.vertical,
            })
            .collect(),
        instances: f.instances.clone(),
        variable_fonts: f.variable_fonts.clone(),
        export_presets: f.export_presets.clone(),
        hinting: f.info.hinting.unwrap_or_default(),
        control_instructions: f.info.control_instructions.clone(),
        ps_hinting: f.info.ps_hinting.unwrap_or_default(),
        grid: f.info.grid,
        use_typo_metrics: f.info.use_typo_metrics,
        palettes: f.palettes.clone(),
        master_palettes: f.master_palettes.clone(),
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

/// Replaces a master's alignment zones (Font Info › Masters).
#[tauri::command]
pub fn set_alignment_zones(
    state: State<AppState>,
    master_id: String,
    zones: Vec<Zone>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        kernest_core::hinting::set_alignment_zones(f, &master_id, zones)
    })
}

/// Replaces a master's standard stems (Font Info › Masters).
#[tauri::command]
pub fn set_stems(
    state: State<AppState>,
    master_id: String,
    stems: Vec<Stem>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        kernest_core::hinting::set_stems(f, &master_id, stems)
    })
}

/// Sets the TTFAutohint control instructions (empty clears them).
#[tauri::command]
pub fn set_control_instructions(state: State<AppState>, text: String) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        kernest_core::hinting::set_control_instructions(f, text)
    })
}

/// Sets or clears the PostScript blue zone controls (`None` restores defaults).
#[tauri::command]
pub fn set_ps_hinting(
    state: State<AppState>,
    ps_hinting: Option<PsHinting>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        kernest_core::hinting::set_ps_hinting(f, ps_hinting)
    })
}

/// Sets or clears the edit view's snap grid (Font Info › General).
#[tauri::command]
pub fn set_grid(state: State<AppState>, grid: Option<Grid>) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_grid(f, grid))
}

/// Sets one master's line spacing and underline values.
#[tauri::command]
pub fn set_vertical_metrics(
    state: State<AppState>,
    master_id: String,
    vertical: kernest_core::VerticalMetrics,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        structure::set_vertical_metrics(f, &master_id, vertical)
    })
}

/// Turns "Use Typo Metrics" on or off.
#[tauri::command]
pub fn set_use_typo_metrics(state: State<AppState>, on: bool) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_use_typo_metrics(f, on))
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

/// Sets or clears one master's palette override (`None` clears it).
#[tauri::command]
pub fn set_master_palettes(
    state: State<AppState>,
    master_id: String,
    palettes: Option<Vec<Vec<kernest_core::color::Color>>>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| {
        kernest_core::color::set_master_palettes(f, &master_id, palettes)
    })
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

/// Compatibility of every master in another font against this one, before adding any.
#[tauri::command]
pub fn preview_foreign_masters(
    state: State<AppState>,
    path: String,
) -> CmdResult<Vec<structure::ForeignMasterPreview>> {
    let source = crate::import::load_source_font(Path::new(path.trim()))?;
    let mut guard = lock(&state)?;
    let f = &doc_mut(&mut guard)?.font;
    source
        .masters
        .iter()
        .map(|m| structure::preview_foreign_master(f, &source, &m.id))
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMasterFromView {
    info: FontInfoView,
    /// Glyphs the source lacked, added as empty layers.
    missing: Vec<String>,
}

/// Adds a master copied from another font, returning the new Font Info and the glyphs
/// that had to be filled as empty layers.
#[tauri::command]
pub fn add_master_from(
    state: State<AppState>,
    path: String,
    master_id: String,
    name: String,
    location: BTreeMap<String, f64>,
) -> CmdResult<AddMasterFromView> {
    let source = crate::import::load_source_font(Path::new(path.trim()))?;
    let mut missing = Vec::new();
    let info = edit(&state, |f| {
        let (next, filled) = structure::add_master_from(f, &source, &master_id, &name, &location)?;
        missing = filled;
        Ok(next)
    })?;
    Ok(AddMasterFromView { info, missing })
}

/// S42 · The compare report plus the other font's identity for the window header.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareView {
    file_name: String,
    family_name: String,
    #[serde(flatten)]
    comparison: kernest_core::compare::FontComparison,
}

/// Reads another font and reports how it differs from the open one. Read-only:
/// neither font is changed, so there is nothing to undo.
#[tauri::command]
pub fn compare_fonts(state: State<AppState>, path: String) -> CmdResult<CompareView> {
    let trimmed = path.trim();
    let other = crate::import::load_source_font(Path::new(trimmed))?;
    let mut guard = lock(&state)?;
    let f = &doc_mut(&mut guard)?.font;
    let comparison = kernest_core::compare::compare_fonts(f, &other);
    let file_name = Path::new(trimmed)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| trimmed.to_string());
    Ok(CompareView {
        file_name,
        family_name: other.info.family_name.clone(),
        comparison,
    })
}

/// Generates one interpolated master per instance as a NEW unsaved document. The open
/// font is only read; the result replaces it in the session and stays dirty until Save As,
/// so generating writes nothing to disk.
#[tauri::command]
pub fn generate_instances(state: State<AppState>) -> CmdResult<FontView> {
    let generated = {
        let mut guard = lock(&state)?;
        let f = &doc_mut(&mut guard)?.font;
        kernest_core::interpolate::generate_instances(f)?
    };
    replace_dirty(&state, generated)
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

#[tauri::command]
pub fn set_variable_fonts(
    state: State<AppState>,
    variable_fonts: Vec<VariableFontSetting>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| structure::set_variable_fonts(f, variable_fonts))
}

#[tauri::command]
pub fn set_export_presets(
    state: State<AppState>,
    presets: Vec<kernest_core::presets::ExportPreset>,
) -> CmdResult<FontInfoView> {
    edit(&state, |f| kernest_core::presets::set_presets(f, presets))
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
            map: Vec::new(),
            labels: Vec::new(),
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

    #[test]
    fn hinting_edits_are_undoable_steps() {
        let mut doc = Document::new(sample_font(), None);
        let id = doc.font.masters[0].id.clone();
        let zones = vec![Zone {
            position: 700.0,
            size: 10.0,
        }];
        let f = kernest_core::hinting::set_alignment_zones(&doc.font, &id, zones.clone()).unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let stems = vec![Stem {
            name: "h".into(),
            horizontal: true,
            value: 80.0,
        }];
        let f = kernest_core::hinting::set_stems(&doc.font, &id, stems.clone()).unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let f = kernest_core::hinting::set_control_instructions(&doc.font, "H 10".into()).unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let v = info_view(&doc);
        assert_eq!(v.masters[0].alignment_zones, zones);
        assert_eq!(v.masters[0].stems, stems);
        assert_eq!(v.control_instructions, "H 10");
        assert_eq!(v.ps_hinting, PsHinting::default());
        assert!(v.status.can_undo);
        doc.history.undo(&mut doc.font).unwrap();
        assert!(info_view(&doc).control_instructions.is_empty());
        doc.history.undo(&mut doc.font).unwrap();
        let v = info_view(&doc);
        assert!(v.masters[0].stems.is_empty());
        assert_eq!(v.masters[0].alignment_zones, zones);
    }

    #[test]
    fn master_palette_override_is_one_undoable_step() {
        use kernest_core::color::{self, Color};
        let mut doc = Document::new(sample_font(), None);
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        };
        let f = structure::set_axes(&doc.font, vec![wght]).unwrap();
        let (f, bold) =
            structure::add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)]))
                .unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let f = color::set_palettes(
            &doc.font,
            vec![vec![
                Color::parse("#e63946").unwrap(),
                Color::parse("#1d3557").unwrap(),
            ]],
        )
        .unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        assert!(info_view(&doc).master_palettes.is_empty());

        let override_palettes = vec![vec![
            Color::parse("#00ff00").unwrap(),
            Color::parse("#1d3557").unwrap(),
        ]];
        let f =
            color::set_master_palettes(&doc.font, &bold, Some(override_palettes.clone())).unwrap();
        doc.history.apply_font(&mut doc.font, f).unwrap();
        let v = info_view(&doc);
        assert_eq!(v.master_palettes.get(&bold), Some(&override_palettes));
        assert!(v.status.can_undo);

        // One undo removes the override; the shared palettes stay.
        doc.history.undo(&mut doc.font).unwrap();
        let v = info_view(&doc);
        assert!(v.master_palettes.is_empty());
        assert_eq!(v.palettes.len(), 1);
    }
}
