//! The `.kernest` package format.
//!
//! A font is a folder:
//!
//! ```text
//! MyFont.kernest/
//!   font.json        format version, info, axes, masters, glyph order
//!   kerning.json     kerning per master (omitted when empty)
//!   features.fea     OpenType feature code (omitted when empty)
//!   glyphs/
//!     A_.json        one file per glyph
//!     a.json
//! ```
//!
//! One file per glyph keeps Git diffs small. Glyph file names are derived from
//! glyph names so they are unique on case-insensitive file systems (Windows):
//! every uppercase letter is followed by `_`, characters that Windows forbids
//! become `_`, and reserved device names (`con`, `nul`, …) are prefixed with `_`.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::model::{
    Axis, FeatureSettings, Font, FontInfo, Glyph, Instance, KernPair, Master, Shape,
};

/// Format version written by this build.
/// Version 2 adds colour palettes and colour layers; version 3 adds the component
/// align flag (older versions would silently re-align hand-placed components);
/// version 4 adds the naming fields, instance classes/style links, notes and
/// automatic-feature settings; version 5 adds guides, the background layer and the
/// snap grid; version 6 adds locks; version 7 adds saved filters (older versions
/// would drop them on save); version 8 adds axis mappings (older versions would
/// misplace masters, which live in design coordinates); version 9 adds per-master
/// palette overrides and per-master colour paints (older versions would drop the
/// variation and export every master with the default colour); version 10 adds
/// alignment zones, stems, manual hints and hinting settings (older versions
/// would drop them on save); version 11 adds saved export presets; version 12 adds
/// line spacing metrics (typo, hhea, win, underline and Use Typo Metrics); version 13
/// adds kept custom parameters, backup layers, strokes and corner/cap/segment
/// components; version 14 adds masks, annotations, placed images, interpolation
/// curves, virtual masters and glyph swaps; version 15 adds sbix bitmaps, SVG
/// drawings and number values for feature tokens; version 16 adds per-instance
/// family names and disabled masters; version 17 adds vertical metrics (vertical
/// widths and origins, vhea values) and glyph writing directions.
/// Fonts using none of these are still written as version 1, colour-only as 2,
/// manual-component-only as 3 and naming/notes-only as 4, so older Kernest versions
/// keep opening them.
pub const FORMAT_VERSION: u32 = 19;

pub const EXTENSION: &str = "kernest";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FontFile {
    format_version: u32,
    generator: String,
    info: FontInfo,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    axes: Vec<Axis>,
    masters: Vec<Master>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    instances: Vec<Instance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    palettes: Vec<Vec<crate::color::Color>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    master_palettes: BTreeMap<String, Vec<Vec<crate::color::Color>>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    #[serde(default, skip_serializing_if = "FeatureSettings::is_empty")]
    feature_settings: FeatureSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    filters: Vec<crate::smartfilter::Filter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    variable_fonts: Vec<crate::model::VariableFontSetting>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    export_presets: Vec<crate::presets::ExportPreset>,
    glyph_order: Vec<String>,
}

const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// File name (without folder) used for a glyph inside `glyphs/`.
pub fn glyph_file_name(glyph_name: &str) -> String {
    let mut out = String::with_capacity(glyph_name.len() + 8);
    for (i, c) in glyph_name.chars().enumerate() {
        match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push('_'),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => out.push('_'),
            '.' if i == 0 => out.push('_'),
            c if c.is_uppercase() => {
                out.push(c);
                out.push('_');
            }
            c => out.push(c),
        }
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    out.push_str(".json");
    out
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    // Skip unchanged files so saving a large font only touches what was edited.
    if fs::read(path).map(|old| old == bytes).unwrap_or(false) {
        return Ok(());
    }
    let tmp = path.with_extension("tmp~");
    fs::write(&tmp, bytes).at(&tmp)?;
    fs::rename(&tmp, path).at(path)
}

fn to_json<T: Serialize>(value: &T, path: &Path) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::Json {
        path: path.into(),
        source,
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).at(path)?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: path.into(),
        source,
    })
}

/// Writes `font` to the package folder at `dir`, creating it if needed.
/// Glyph files for glyphs that no longer exist are removed.
pub fn save(font: &Font, dir: &Path) -> Result<()> {
    font.validate()?;
    let glyph_dir = dir.join("glyphs");
    fs::create_dir_all(&glyph_dir).at(&glyph_dir)?;

    let mut written = HashSet::new();
    for g in &font.glyphs {
        let name = glyph_file_name(&g.name);
        if !written.insert(name.to_ascii_lowercase()) {
            return Err(Error::Invalid(format!(
                "glyph \"{}\" maps to a file name that is already used",
                g.name
            )));
        }
        let path = glyph_dir.join(&name);
        write_atomic(&path, &to_json(g, &path)?)?;
    }
    for entry in fs::read_dir(&glyph_dir).at(&glyph_dir)? {
        let entry = entry.at(&glyph_dir)?;
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.ends_with(".json") && !written.contains(&fname.to_ascii_lowercase()) {
            fs::remove_file(entry.path()).at(&entry.path())?;
        }
    }

    let kerning_path = dir.join("kerning.json");
    if font.kerning.values().all(Vec::is_empty) {
        let _ = fs::remove_file(&kerning_path);
    } else {
        write_atomic(&kerning_path, &to_json(&font.kerning, &kerning_path)?)?;
    }

    let fea_path = dir.join("features.fea");
    if font.features.is_empty() {
        let _ = fs::remove_file(&fea_path);
    } else {
        write_atomic(&fea_path, font.features.as_bytes())?;
    }

    // font.json last: a package with a font.json is a complete package.
    let font_path = dir.join("font.json");
    let has_color = !font.palettes.is_empty() || font.glyphs.iter().any(|g| g.is_color());
    let has_manual = font.glyphs.iter().any(|g| {
        g.layers.iter().any(|l| {
            l.shapes
                .iter()
                .any(|s| matches!(s, crate::model::Shape::Component { align: false, .. }))
        })
    });
    let i = &font.info;
    let has_naming = [
        &i.designer_url,
        &i.manufacturer,
        &i.manufacturer_url,
        &i.license,
        &i.license_url,
        &i.description,
        &i.trademark,
        &i.sample_text,
    ]
    .iter()
    .any(|v| v.is_some());
    let has_instance_overrides = font.instances.iter().any(|inst| {
        inst.weight_class.is_some() || inst.width_class.is_some() || inst.style_link.is_some()
    });
    let has_phase3 = !font.notes.is_empty()
        || !font.feature_settings.is_empty()
        || has_naming
        || has_instance_overrides;
    let has_phase4 = font.info.grid.as_ref().is_some_and(|g| g.step().is_some())
        || font.masters.iter().any(|m| !m.guides.is_empty())
        || font.glyphs.iter().any(|g| {
            g.layers.iter().any(|l| {
                !l.guides.is_empty() || l.background.as_ref().is_some_and(|b| !b.is_empty())
            })
        });
    let has_phase5 = font.glyphs.iter().any(|g| {
        g.locked
            || g.layers.iter().any(|l| {
                let locked_path = |s: &Shape| matches!(s, Shape::Path(p) if p.locked);
                l.shapes.iter().any(locked_path)
                    || l.background
                        .as_ref()
                        .is_some_and(|b| b.shapes.iter().any(locked_path))
            })
    });
    let has_phase8 = !font.filters.is_empty()
        || font
            .glyphs
            .iter()
            .any(|g| g.production_name.is_some() || !g.tags.is_empty());
    let has_phase9 = !font.variable_fonts.is_empty()
        || font
            .axes
            .iter()
            .any(|a| !a.map.is_empty() || !a.labels.is_empty());
    let has_var_color = crate::color::varies_between_masters(font);
    let has_phase10 = !font.info.control_instructions.is_empty()
        || font.info.ps_hinting.is_some()
        || font
            .masters
            .iter()
            .any(|m| !m.alignment_zones.is_empty() || !m.stems.is_empty())
        || font
            .glyphs
            .iter()
            .any(|g| g.layers.iter().any(|l| !l.hints.is_empty()));
    let has_v13 = !font.info.custom_parameters.is_empty()
        || font.glyphs.iter().any(|g| {
            g.layers.iter().any(|l| {
                l.backup.is_some()
                    || !l.parts.is_empty()
                    || l.shapes
                        .iter()
                        .any(|s| matches!(s, crate::model::Shape::Path(p) if p.stroke.is_some()))
            })
        });
    let has_v14 = !font.info.virtual_masters.is_empty()
        || font
            .instances
            .iter()
            .any(|i| !i.rename_glyphs.is_empty() || !i.remove_glyphs.is_empty())
        || font
            .variable_fonts
            .iter()
            .any(|v| !v.rename_glyphs.is_empty() || !v.remove_glyphs.is_empty())
        || font.glyphs.iter().any(|g| {
            g.layers.iter().any(|l| {
                !l.annotations.is_empty()
                    || l.image.is_some()
                    || !l.hoi.is_empty()
                    || l.shapes
                        .iter()
                        .any(|s| matches!(s, crate::model::Shape::Path(p) if p.mask))
            })
        });
    let has_v15 = !font.info.numbers.is_empty()
        || font
            .glyphs
            .iter()
            .any(|g| !g.bitmaps.is_empty() || g.svg.is_some());
    let has_v16 = font.instances.iter().any(|i| {
        i.family_name.is_some()
            || !i.localized_family_names.is_empty()
            || !i.disabled_masters.is_empty()
    }) || font
        .variable_fonts
        .iter()
        .any(|v| !v.disabled_masters.is_empty());
    let has_v17 = font.uses_vertical_metrics() || font.glyphs.iter().any(|g| g.direction.is_some());
    let has_v18 = font.instances.iter().any(|i| !i.names.is_empty())
        || font
            .variable_fonts
            .iter()
            .any(|v| v.family_name.is_some() || !v.name_records.is_empty());
    let has_v19 = font.glyphs.iter().any(|g| {
        g.layers.iter().any(|l| {
            l.shapes
                .iter()
                .any(|s| matches!(s, crate::model::Shape::Path(p) if p.nodes.iter().any(|n| n.g2)))
        })
    });
    let has_vertical =
        font.info.use_typo_metrics || font.masters.iter().any(|m| !m.vertical.is_empty());
    let file = FontFile {
        format_version: if has_v19 {
            FORMAT_VERSION
        } else if has_v18 {
            18
        } else if has_v17 {
            17
        } else if has_v16 {
            16
        } else if has_v15 {
            15
        } else if has_v14 {
            14
        } else if has_v13 {
            13
        } else if has_vertical {
            12
        } else if !font.export_presets.is_empty() {
            11
        } else if has_phase10 {
            10
        } else if has_var_color {
            9
        } else if has_phase9 {
            8
        } else if has_phase8 {
            7
        } else if has_phase5 {
            6
        } else if has_phase4 {
            5
        } else if has_phase3 {
            4
        } else if has_manual {
            3
        } else if has_color {
            2
        } else {
            1
        },
        generator: format!("Kernest {}", env!("CARGO_PKG_VERSION")),
        info: font.info.clone(),
        axes: font.axes.clone(),
        masters: font.masters.clone(),
        instances: font.instances.clone(),
        palettes: font.palettes.clone(),
        master_palettes: font.master_palettes.clone(),
        notes: font.notes.clone(),
        feature_settings: font.feature_settings.clone(),
        filters: font.filters.clone(),
        variable_fonts: font.variable_fonts.clone(),
        export_presets: font.export_presets.clone(),
        glyph_order: font.glyphs.iter().map(|g| g.name.clone()).collect(),
    };
    write_atomic(&font_path, &to_json(&file, &font_path)?)
}

/// Reads a package folder. Glyph files not listed in the glyph order are appended at the end.
pub fn open(dir: &Path) -> Result<Font> {
    let font_path = dir.join("font.json");
    let file: FontFile = read_json(&font_path)?;
    if file.format_version > FORMAT_VERSION {
        return Err(Error::NewerFormat(
            dir.to_path_buf(),
            file.format_version,
            FORMAT_VERSION,
        ));
    }

    let glyph_dir = dir.join("glyphs");
    let mut glyphs = Vec::with_capacity(file.glyph_order.len());
    let mut seen = HashSet::new();
    for name in &file.glyph_order {
        let path = glyph_dir.join(glyph_file_name(name));
        let g: Glyph = read_json(&path)?;
        if &g.name != name {
            return Err(Error::Invalid(format!(
                "{} contains glyph \"{}\", expected \"{name}\"",
                path.display(),
                g.name
            )));
        }
        seen.insert(glyph_file_name(name).to_ascii_lowercase());
        glyphs.push(g);
    }
    // Glyph files added outside Kernest (e.g. by a merge) are kept rather than lost.
    if let Ok(entries) = fs::read_dir(&glyph_dir) {
        let mut extra: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .filter(|p| {
                !seen.contains(
                    &p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_ascii_lowercase(),
                )
            })
            .collect();
        extra.sort();
        for path in extra {
            glyphs.push(read_json(&path)?);
        }
    }

    let kerning_path = dir.join("kerning.json");
    let kerning: BTreeMap<String, Vec<KernPair>> = if kerning_path.exists() {
        read_json(&kerning_path)?
    } else {
        BTreeMap::new()
    };
    let fea_path = dir.join("features.fea");
    let features = if fea_path.exists() {
        fs::read_to_string(&fea_path).at(&fea_path)?
    } else {
        String::new()
    };

    let font = Font {
        info: file.info,
        axes: file.axes,
        masters: file.masters,
        instances: file.instances,
        glyphs,
        kerning,
        features,
        palettes: file.palettes,
        master_palettes: file.master_palettes,
        notes: file.notes,
        feature_settings: file.feature_settings,
        filters: file.filters,
        variable_fonts: file.variable_fonts,
        export_presets: file.export_presets,
    };
    font.validate()?;
    Ok(font)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

    #[test]
    fn presets_round_trip_and_gate_only_fonts_that_use_them() {
        let original = sample_font();
        let mut json = serde_json::to_value(&original).unwrap();
        json["exportPresets"] = serde_json::json!([{
            "name": "web", "formats": ["woff2"], "instances": [],
            "options": {"variable": false, "statics": true, "autohint": true},
            "folder": "build/web"
        }]);
        let font: Font = serde_json::from_value(json).unwrap();
        let dir = tempfile::tempdir().unwrap();
        save(&font, dir.path()).unwrap();
        let saved: serde_json::Value = read_json(&dir.path().join("font.json")).unwrap();
        assert_eq!(saved["formatVersion"], 11);
        assert_eq!(saved["exportPresets"][0]["folder"], "build/web");
        assert_eq!(saved["exportPresets"][0]["formats"][0], "woff2");
        let back = open(dir.path()).unwrap();
        assert_eq!(
            serde_json::to_value(back).unwrap()["exportPresets"][0]["name"],
            "web"
        );
        save(&original, dir.path()).unwrap();
        let saved: serde_json::Value = read_json(&dir.path().join("font.json")).unwrap();
        assert_eq!(saved["formatVersion"], 1);
        assert!(saved.get("exportPresets").is_none());
    }

    #[test]
    fn invalid_preset_settings_cannot_be_saved() {
        let cases = [
            serde_json::json!([{"name":" ","formats":["ttf"],"options":{}}]),
            serde_json::json!([{"name":"web","formats":[],"options":{}}]),
            serde_json::json!([{"name":"web","formats":["ttf"],"options":{"variable":false,"statics":false}}]),
            serde_json::json!([{"name":"web","formats":["ttf"]},{"name":"web","formats":["otf"]}]),
        ];
        for presets in cases {
            let mut json = serde_json::to_value(sample_font()).unwrap();
            json["exportPresets"] = presets;
            let font: Font = serde_json::from_value(json).unwrap();
            assert!(font.validate().is_err());
        }
    }

    #[test]
    fn file_names_are_unique_on_windows() {
        assert_eq!(glyph_file_name("A"), "A_.json");
        assert_eq!(glyph_file_name("a"), "a.json");
        assert_eq!(glyph_file_name("a.sc"), "a.sc.json");
        assert_eq!(glyph_file_name("con"), "_con.json");
        assert_eq!(glyph_file_name("con.alt"), "_con.alt.json");
        assert_eq!(glyph_file_name("Con"), "C_on.json");
        assert_eq!(glyph_file_name(".notdef"), "_notdef.json");
        assert_eq!(glyph_file_name("a/b"), "a_b.json");
        assert_ne!(
            glyph_file_name("A").to_lowercase(),
            glyph_file_name("a").to_lowercase()
        );
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Sample.kernest");
        let font = sample_font();
        save(&font, &path).unwrap();
        let back = open(&path).unwrap();
        assert_eq!(font, back);
        // Saving again without changes rewrites nothing and stays identical.
        save(&back, &path).unwrap();
        assert_eq!(open(&path).unwrap(), font);
    }

    #[test]
    fn align_flag_versioning() {
        use crate::model::{Shape, IDENTITY};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        font.glyph_mut("bar").unwrap().layers[0].shapes = vec![Shape::component("l", IDENTITY)];
        save(&font, &path).unwrap();
        // Aligned components keep version 1 and leave the flag out of the JSON.
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        let gj = fs::read_to_string(path.join("glyphs/bar.json")).unwrap();
        assert!(!gj.contains("align"), "{gj}");
        assert_eq!(open(&path).unwrap(), font);
        // A hand-placed component needs version 3 and survives a round trip.
        let Shape::Component { align, .. } =
            &mut font.glyph_mut("bar").unwrap().layers[0].shapes[0]
        else {
            panic!("expected a component")
        };
        *align = false;
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 3"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);
    }

    #[test]
    fn brace_and_bracket_layers_round_trip() {
        use crate::model::{Axis, AxisRange};
        let mut font = sample_font();
        font.axes.push(Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        });
        let g = font.glyph_mut("H").unwrap();
        let mut brace = g.layers[0].clone();
        brace.id = Some("m01~1".into());
        brace.location = Some([("wght".to_string(), 550.0)].into());
        let mut bracket = g.layers[0].clone();
        bracket.id = Some("m01~2".into());
        bracket.bracket = vec![AxisRange {
            tag: "wght".into(),
            min: Some(600.0),
            max: None,
        }];
        g.layers.extend([brace, bracket]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Layers.kernest");
        save(&font, &path).unwrap();
        assert_eq!(open(&path).unwrap(), font);
        let json = std::fs::read_to_string(path.join("glyphs").join("H_.json")).unwrap();
        assert!(
            json.contains("\"location\"") && json.contains("\"bracket\""),
            "{json}"
        );
    }

    #[test]
    fn removed_glyphs_are_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        save(&font, &path).unwrap();
        assert!(path.join("glyphs/H_.json").exists());
        font.glyphs.retain(|g| g.name != "H");
        save(&font, &path).unwrap();
        assert!(!path.join("glyphs/H_.json").exists());
        assert!(open(&path).unwrap().glyph("H").is_none());
    }

    #[test]
    fn rejects_newer_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        save(&sample_font(), &path).unwrap();
        let fj = path.join("font.json");
        let text = fs::read_to_string(&fj)
            .unwrap()
            .replace("\"formatVersion\": 1", "\"formatVersion\": 99");
        fs::write(&fj, text).unwrap();
        assert!(matches!(
            open(&path),
            Err(Error::NewerFormat(_, 99, FORMAT_VERSION))
        ));
    }

    #[test]
    fn instance_family_names_round_trip_as_version_16() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        font.instances.push(crate::model::Instance {
            names: Default::default(),
            name: "Bold".into(),
            location: Default::default(),
            export: true,
            weight_class: None,
            width_class: None,
            style_link: None,
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
            family_name: None,
            localized_family_names: Default::default(),
            disabled_masters: Vec::new(),
        });
        // An instance without a family of its own doesn't need version 16.
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(!fj.contains("\"formatVersion\": 16"), "{fj}");
        font.instances[0].family_name = Some("Sample Deca".into());
        font.instances[0]
            .localized_family_names
            .insert("DEU".into(), "Probe Zehn".into());
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 16"), "{fj}");
        assert_eq!(open(&path).unwrap().instances, font.instances);
    }

    #[test]
    fn phase3_fields_round_trip_and_gate_version() {
        use crate::model::StyleLink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        // Without any phase-3 work the package stays at version 1.
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        assert!(!fj.contains("notes"), "{fj}");

        font.notes = "remember to check the ogonek".into();
        font.feature_settings.automatic.insert("ss01".into());
        font.info.designer_url = Some("https://example.com".into());
        font.info.license = Some("SIL OFL 1.1".into());
        font.instances.push(Instance {
            names: Default::default(),
            name: "Bold".into(),
            location: BTreeMap::new(),
            export: true,
            weight_class: Some(700),
            width_class: Some(5),
            style_link: Some(StyleLink {
                bold: true,
                italic: false,
                base: "Regular".into(),
            }),
            rename_glyphs: Vec::new(),
            remove_glyphs: Vec::new(),
            family_name: None,
            localized_family_names: Default::default(),
            disabled_masters: Vec::new(),
        });
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 4"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);
    }

    #[test]
    fn phase4_fields_round_trip_and_gate_version() {
        use crate::model::{Anchor, Background, Grid, Guide, Node, NodeKind, Path, Shape};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        // Without guides, background or grid the package stays at version 1.
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        assert!(!fj.contains("guides") && !fj.contains("grid"), "{fj}");
        let hj = fs::read_to_string(path.join("glyphs/H_.json")).unwrap();
        assert!(!hj.contains("guides") && !hj.contains("background"), "{hj}");

        font.info.grid = Some(Grid {
            spacing: 100,
            subdivision: 2,
        });
        font.masters[0].guides.push(Guide {
            x: 0.0,
            y: 250.0,
            angle: 0.0,
            name: Some("bar".into()),
            locked: false,
        });
        let layer = font.glyph_mut("H").unwrap().layer_mut("m01").unwrap();
        layer.guides.push(Guide {
            x: 412.0,
            y: 0.0,
            angle: 90.0,
            name: None,
            locked: true,
        });
        layer.background = Some(Background {
            shapes: vec![Shape::Path(Path {
                locked: false,
                stroke: None,
                mask: false,
                closed: true,
                nodes: vec![
                    Node::new(80.0, 0.0, NodeKind::Line),
                    Node::new(170.0, 0.0, NodeKind::Line),
                    Node::new(170.0, 700.0, NodeKind::Line),
                    Node::new(80.0, 700.0, NodeKind::Line),
                ],
            })],
            anchors: vec![Anchor {
                name: "top".into(),
                x: 125.0,
                y: 700.0,
            }],
        });
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 5"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);
    }

    #[test]
    fn phase5_locks_round_trip_and_gate_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        // Without locks the package stays at version 1 and writes no lock flags.
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        let hj = fs::read_to_string(path.join("glyphs/H_.json")).unwrap();
        assert!(!hj.contains("locked"), "{hj}");

        font.glyph_mut("H").unwrap().locked = true;
        let layer = font.glyph_mut("H").unwrap().layer_mut("m01").unwrap();
        if let Some(Shape::Path(p)) = layer.shapes.first_mut() {
            p.locked = true;
        }
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 6"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);

        // A glyph lock alone also gates the version; unlocking returns to 1.
        if let Some(Shape::Path(p)) = font
            .glyph_mut("H")
            .unwrap()
            .layer_mut("m01")
            .unwrap()
            .shapes
            .first_mut()
        {
            p.locked = false;
        }
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 6"), "{fj}");
        font.glyph_mut("H").unwrap().locked = false;
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
    }

    #[test]
    fn phase8_filters_round_trip_and_gate_version() {
        use crate::smartfilter::{Filter, FilterKind, Rule, RuleGroup};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        // Without filters the package stays at version 1 and writes no filters.
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        assert!(!fj.contains("filters"), "{fj}");

        font.filters = vec![
            Filter {
                name: "Capitals".into(),
                kind: FilterKind::Smart,
                rules: RuleGroup {
                    rules: vec![Rule::Category {
                        value: "Uppercase".into(),
                    }],
                    ..Default::default()
                },
                glyphs: Vec::new(),
            },
            Filter {
                name: "Picked".into(),
                kind: FilterKind::List,
                rules: RuleGroup::default(),
                glyphs: vec!["H".into(), "I".into()],
            },
        ];
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 7"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);

        // Removing the filters returns the package to version 1.
        font.filters.clear();
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        assert!(!fj.contains("filters"), "{fj}");
    }

    #[test]
    fn phase8_production_names_and_tags_gate_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        font.glyph_mut("H").unwrap().production_name = Some("H.prod".into());
        font.glyph_mut("o").unwrap().tags = vec!["round".into()];
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 7"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);
        // Glyphs without the fields don't write them.
        let hj = fs::read_to_string(path.join("glyphs").join("H_.json")).unwrap();
        assert!(hj.contains("\"productionName\": \"H.prod\""), "{hj}");
        let oj = fs::read_to_string(path.join("glyphs").join("o.json")).unwrap();
        assert!(!oj.contains("productionName"), "{oj}");

        for g in &mut font.glyphs {
            g.production_name = None;
            g.tags.clear();
        }
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);
    }

    #[test]
    fn phase9_axis_map_round_trip_and_gate_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        let mut font = sample_font();
        font.axes.push(Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 100.0,
            default: 400.0,
            max: 900.0,
            hidden: false,
            map: Vec::new(),
            labels: Vec::new(),
        });
        // An axis without a map writes no map and stays at version 1.
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        assert!(!fj.contains("\"map\""), "{fj}");

        font.axes[0].map = vec![(100.0, 100.0), (400.0, 300.0), (900.0, 900.0)];
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 8"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);

        // Removing the map returns the package to version 1.
        font.axes[0].map.clear();
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 1"), "{fj}");
        assert!(!fj.contains("\"map\""), "{fj}");
    }

    #[test]
    fn readable_glyph_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.kernest");
        save(&sample_font(), &path).unwrap();
        let h = fs::read_to_string(path.join("glyphs/H_.json")).unwrap();
        assert!(h.contains("\"unicodes\": [\n    \"0048\"\n  ]"), "{h}");
        assert!(h.contains("\"80 0 line\""), "{h}");
    }
}
