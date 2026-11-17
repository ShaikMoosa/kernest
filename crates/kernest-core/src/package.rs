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
use crate::model::{Axis, FeatureSettings, Font, FontInfo, Glyph, Instance, KernPair, Master};

/// Format version written by this build.
/// Version 2 adds colour palettes and colour layers; version 3 adds the component
/// align flag (older versions would silently re-align hand-placed components);
/// version 4 adds the naming fields, instance classes/style links, notes and
/// automatic-feature settings (older versions would drop them on save).
/// Fonts using none of these are still written as version 1, colour-only as 2 and
/// manual-component-only as 3, so older Kernest versions keep opening them.
pub const FORMAT_VERSION: u32 = 4;

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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    #[serde(default, skip_serializing_if = "FeatureSettings::is_empty")]
    feature_settings: FeatureSettings,
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
    let file = FontFile {
        format_version: if has_phase3 {
            FORMAT_VERSION
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
        notes: font.notes.clone(),
        feature_settings: font.feature_settings.clone(),
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
        notes: file.notes,
        feature_settings: file.feature_settings,
    };
    font.validate()?;
    Ok(font)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sample::sample_font;

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
        });
        save(&font, &path).unwrap();
        let fj = fs::read_to_string(path.join("font.json")).unwrap();
        assert!(fj.contains("\"formatVersion\": 4"), "{fj}");
        assert_eq!(open(&path).unwrap(), font);
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
