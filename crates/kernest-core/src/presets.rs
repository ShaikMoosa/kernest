use crate::Font;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Ttf,
    Otf,
    Woff2,
    Woff,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportSettings {
    pub variable: bool,
    pub statics: bool,
    pub remove_overlaps: bool,
    pub apply_metrics_keys: bool,
    pub autohint: bool,
    pub production_names: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            variable: false,
            statics: true,
            remove_overlaps: true,
            apply_metrics_keys: true,
            autohint: true,
            production_names: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPreset {
    pub name: String,
    pub formats: Vec<ExportFormat>,
    #[serde(default)]
    pub instances: Vec<String>,
    #[serde(default)]
    pub options: ExportSettings,
    #[serde(default)]
    pub folder: String,
}

pub fn validate(presets: &[ExportPreset]) -> Result<(), String> {
    let mut names = HashSet::new();
    for p in presets {
        let name = p.name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err("A preset needs a name of 1–64 characters.".into());
        }
        if !names.insert(name) {
            return Err(format!("Preset name \"{name}\" is used twice."));
        }
        if p.formats.is_empty() {
            return Err(format!("Preset \"{name}\" needs at least one file format."));
        }
        if !p.options.variable && !p.options.statics {
            return Err("Choose at least one kind of font to export.".into());
        }
        if p.folder.chars().any(char::is_control) {
            return Err("The export folder contains an invalid character.".into());
        }
    }
    Ok(())
}

pub fn set_presets(font: &Font, mut presets: Vec<ExportPreset>) -> Result<Font, String> {
    for p in &mut presets {
        p.name = p.name.trim().into();
        p.folder = p.folder.trim().into();
        let mut formats = HashSet::new();
        p.formats.retain(|f| formats.insert(*f));
        let mut instances = HashSet::new();
        p.instances.retain(|n| instances.insert(n.clone()));
        select_instances(font, &p.instances)?;
    }
    validate(&presets)?;
    let mut next = font.clone();
    next.export_presets = presets;
    Ok(next)
}

pub fn select_instances(font: &Font, names: &[String]) -> Result<Font, String> {
    let mut next = font.clone();
    if names.is_empty() {
        return Ok(next);
    }
    for name in names {
        if !font.instances.iter().any(|i| &i.name == name) {
            return Err(format!(
                "There is no instance named \"{name}\". Update the export preset."
            ));
        }
    }
    next.instances.retain(|i| names.contains(&i.name));
    for i in &mut next.instances {
        i.export = true;
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{Affected, History};

    #[test]
    fn preset_edit_is_one_undoable_step_and_normalizes_input() {
        let mut font = crate::sample::sample_font();
        let original = font.clone();
        let preset = ExportPreset {
            name: "  proof  ".into(),
            formats: vec![ExportFormat::Ttf, ExportFormat::Ttf],
            instances: vec![],
            options: ExportSettings::default(),
            folder: " build ".into(),
        };
        let next = set_presets(&font, vec![preset]).unwrap();
        let mut history = History::default();
        assert!(history.apply_font(&mut font, next).unwrap());
        assert_eq!(font.export_presets[0].name, "proof");
        assert_eq!(font.export_presets[0].formats.len(), 1);
        assert_eq!(font.export_presets[0].folder, "build");
        assert_eq!(history.undo(&mut font), Some(Affected::Font));
        assert_eq!(font, original);
        assert!(!history.can_undo());
        assert_eq!(history.redo(&mut font), Some(Affected::Font));
        let mut bad = font.export_presets.clone();
        bad[0].instances.push("Missing".into());
        assert!(set_presets(&font, bad).is_err());
        assert_eq!(font.export_presets[0].instances.len(), 0);
    }
}
