use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MILESTONES: [&str; 6] = [
    "created", "drawn", "spaced", "kerned", "instance", "exported",
];
pub const COLOR_ELEMENTS: [&str; 9] = [
    "cornerNode",
    "smoothNode",
    "zones",
    "stroke",
    "background",
    "otherLayers",
    "canvas",
    "kerningNegative",
    "kerningPositive",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThemeColors {
    pub light: BTreeMap<String, String>,
    pub dark: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    pub theme: Option<String>,
    pub language: Option<String>,
    pub handle_size: String,
    pub text_width: u32,
    /// Text tabs switch to right to left when their text starts with right-to-left
    /// letters, and back (SMO-33).
    pub auto_direction: bool,
    /// How close (screen pixels) a dragged node must come to another node, a metric
    /// line or a guide to snap to it; 0 turns magnetic snapping off (SMO-43).
    pub snap_distance: u32,
    pub colors: ThemeColors,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: None,
            language: None,
            handle_size: "medium".into(),
            text_width: 16000,
            auto_direction: true,
            snap_distance: 6,
            colors: ThemeColors::default(),
        }
    }
}

impl Appearance {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .theme
            .as_ref()
            .is_some_and(|s| !["system", "light", "dark"].contains(&s.as_str()))
        {
            return Err("Choose System, Light or Dark for the theme.".into());
        }
        if self
            .language
            .as_ref()
            .is_some_and(|s| !["en", "es", "ar"].contains(&s.as_str()))
        {
            return Err("Choose English, Spanish or Arabic.".into());
        }
        if !["small", "medium", "large"].contains(&self.handle_size.as_str()) {
            return Err("Choose Small, Medium or Large for the handles.".into());
        }
        if self.snap_distance > 30 {
            return Err("Snap distance must be between 0 and 30 pixels.".into());
        }
        if !(1000..=100000).contains(&self.text_width) {
            return Err("Text View width must be between 1000 and 100000.".into());
        }
        for (key, value) in self.colors.light.iter().chain(self.colors.dark.iter()) {
            if !COLOR_ELEMENTS.contains(&key.as_str())
                || value.len() != 7
                || !value.starts_with('#')
                || !value[1..].chars().all(|c| c.is_ascii_hexdigit())
            {
                return Err("Use a six-digit hex colour, such as #5b4bff.".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Onboarding {
    pub welcome_done: bool,
    pub experience: Option<String>,
    pub tour_enabled: bool,
    pub tour_step: u8,
    pub completed: BTreeSet<String>,
}

impl Onboarding {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .experience
            .as_ref()
            .is_some_and(|s| !["beginner", "experienced"].contains(&s.as_str()))
            || self.tour_step > 5
            || self
                .completed
                .iter()
                .any(|s| !MILESTONES.contains(&s.as_str()))
        {
            return Err("The getting-started settings are invalid.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateSettings {
    pub enabled: bool,
    pub last_checked: Option<u64>,
    pub latest_version: Option<String>,
    /// The first lines of the latest release's notes.
    pub latest_notes: Option<String>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::Appearance;

    #[test]
    fn snap_distance_defaults_to_six_and_stays_in_range() {
        let a = Appearance::default();
        assert_eq!(a.snap_distance, 6);
        assert!(a.validate().is_ok());
        // Older settings files without the field get the default.
        let old: Appearance = serde_json::from_str(r#"{"handleSize":"small"}"#).unwrap();
        assert_eq!(old.snap_distance, 6);
        let off = Appearance {
            snap_distance: 0,
            ..Appearance::default()
        };
        assert!(off.validate().is_ok());
        let far = Appearance {
            snap_distance: 31,
            ..Appearance::default()
        };
        assert!(far.validate().is_err());
    }
}
