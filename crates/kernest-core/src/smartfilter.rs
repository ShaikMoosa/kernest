//! Smart filters and list filters for the Font View sidebar.
//!
//! A smart filter is a rule tree (All/Any, with nested groups) evaluated against
//! every glyph; a list filter is a fixed set of glyph names, e.g. saved from a
//! selection. Filters are saved with the font. The module is called `smartfilter`
//! because `filters` already holds the outline operations.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::glyphdata::{self, script_of};
use crate::interpolate::check_glyph;
use crate::model::{Font, Glyph, Shape};

type R<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterKind {
    #[default]
    Smart,
    List,
}

/// A saved filter: smart filters use `rules`, list filters use `glyphs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub name: String,
    #[serde(default)]
    pub kind: FilterKind,
    #[serde(default, skip_serializing_if = "RuleGroup::is_empty")]
    pub rules: RuleGroup,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub glyphs: Vec<String>,
}

/// All/Any over rules and nested groups. An empty group matches every glyph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleGroup {
    /// True means every rule must match (All); false means any (Any).
    #[serde(default = "yes")]
    pub all: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<RuleGroup>,
}

fn yes() -> bool {
    true
}

impl Default for RuleGroup {
    fn default() -> Self {
        Self {
            all: true,
            rules: Vec::new(),
            groups: Vec::new(),
        }
    }
}

impl RuleGroup {
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.groups.is_empty()
    }
}

/// One condition. Boolean rules match when the property equals `value`, so
/// "has components" and "has no components" are the same rule with opposite values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Rule {
    Category { value: String },
    Subcategory { value: String },
    Case { value: String },
    Script { value: String },
    HasComponents { value: bool },
    HasAnchors { value: bool },
    HasUnicode { value: bool },
    Compatible { value: bool },
    Export { value: bool },
    MetricsKeysOutOfSync { value: bool },
    ColorGlyph { value: bool },
    SmartGlyph { value: bool },
    NameContains { value: String },
    NameStarts { value: String },
    NameEnds { value: String },
    TagsContain { value: String },
}

/// Whether `glyph` matches `filter` in `font`.
pub fn matches(font: &Font, glyph: &Glyph, filter: &Filter) -> bool {
    Context::new(font, filter).matches(glyph, filter)
}

/// The names of the glyphs that match, in glyph order.
pub fn evaluate(font: &Font, filter: &Filter) -> Vec<String> {
    let ctx = Context::new(font, filter);
    font.glyphs
        .iter()
        .filter(|g| ctx.matches(g, filter))
        .map(|g| g.name.clone())
        .collect()
}

/// Replaces the font's filters. Names are trimmed and must be unique.
pub fn set_filters(font: &Font, filters: Vec<Filter>) -> R<Font> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(filters.len());
    for mut f in filters {
        f.name = f.name.trim().to_string();
        if f.name.is_empty() {
            return Err("Every filter needs a name.".into());
        }
        if !seen.insert(f.name.to_lowercase()) {
            return Err(format!("Two filters can't both be called \"{}\".", f.name));
        }
        out.push(f);
    }
    let mut font = font.clone();
    font.filters = out;
    Ok(font)
}

/// Evaluating "metrics keys out of sync" against many glyphs would clone the font
/// per glyph; instead the keys are applied once, in dependency order, and each
/// glyph is compared with its updated copy.
struct Context<'a> {
    font: &'a Font,
    applied: Option<Font>,
}

impl<'a> Context<'a> {
    fn new(font: &'a Font, filter: &Filter) -> Self {
        let needed = filter.kind == FilterKind::Smart && uses_sync(&filter.rules);
        Self {
            font,
            applied: needed.then(|| crate::metricskeys::apply_all(font).0),
        }
    }

    fn matches(&self, g: &Glyph, filter: &Filter) -> bool {
        match filter.kind {
            FilterKind::List => filter.glyphs.iter().any(|n| n == &g.name),
            FilterKind::Smart => self.group(g, &filter.rules),
        }
    }

    fn group(&self, g: &Glyph, group: &RuleGroup) -> bool {
        let mut results = group
            .rules
            .iter()
            .map(|r| self.rule(g, r))
            .chain(group.groups.iter().map(|sub| self.group(g, sub)));
        if group.all {
            results.all(|b| b)
        } else {
            results.any(|b| b)
        }
    }

    fn rule(&self, g: &Glyph, rule: &Rule) -> bool {
        match rule {
            Rule::Category { value } => g.category().label() == value,
            Rule::Subcategory { value } => {
                glyphdata::subcategory(g.unicodes.first().map(|c| c.0), &g.name) == value
            }
            Rule::Case { value } => g.category().label() == value,
            Rule::Script { value } => script_of(g) == value,
            Rule::HasComponents { value } => {
                g.layers.iter().any(|l| {
                    l.shapes
                        .iter()
                        .any(|s| matches!(s, Shape::Component { .. }))
                }) == *value
            }
            Rule::HasAnchors { value } => g.layers.iter().any(|l| !l.anchors.is_empty()) == *value,
            Rule::HasUnicode { value } => (!g.unicodes.is_empty()) == *value,
            Rule::Compatible { value } => check_glyph(self.font, &g.name).is_empty() == *value,
            Rule::Export { value } => g.export == *value,
            Rule::MetricsKeysOutOfSync { value } => self.out_of_sync(g) == *value,
            Rule::ColorGlyph { value } => g.is_color() == *value,
            Rule::SmartGlyph { value } => g.is_smart() == *value,
            Rule::NameContains { value } => contains(&g.name, value),
            Rule::NameStarts { value } => g.name.to_lowercase().starts_with(&value.to_lowercase()),
            Rule::NameEnds { value } => g.name.to_lowercase().ends_with(&value.to_lowercase()),
            Rule::TagsContain { value } => g.tags.iter().any(|t| contains(t, value)),
        }
    }

    fn out_of_sync(&self, g: &Glyph) -> bool {
        self.applied
            .as_ref()
            .is_some_and(|f| f.glyph(&g.name).is_some_and(|a| a.layers != g.layers))
    }
}

fn contains(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn uses_sync(group: &RuleGroup) -> bool {
    group
        .rules
        .iter()
        .any(|r| matches!(r, Rule::MetricsKeysOutOfSync { .. }))
        || group.groups.iter().any(uses_sync)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{MetricsKeys, IDENTITY};
    use crate::sample::sample_font;

    fn smart(name: &str, rules: Vec<Rule>) -> Filter {
        Filter {
            name: name.into(),
            kind: FilterKind::Smart,
            rules: RuleGroup {
                rules,
                ..Default::default()
            },
            glyphs: Vec::new(),
        }
    }

    #[test]
    fn category_and_name_rules() {
        let font = sample_font();
        let f = smart(
            "Uppercase A",
            vec![
                Rule::Category {
                    value: "Uppercase".into(),
                },
                Rule::NameStarts { value: "a".into() },
            ],
        );
        let names = evaluate(&font, &f);
        assert!(names.contains(&"A".to_string()));
        assert!(!names.contains(&"a".to_string()));
        assert!(names.iter().all(|n| n.starts_with('A')));
        // Any instead of All widens the match.
        let any = Filter {
            rules: RuleGroup {
                all: false,
                ..f.rules.clone()
            },
            ..f.clone()
        };
        assert!(evaluate(&font, &any).len() > names.len());
        // An empty group matches everything.
        let empty = smart("All", vec![]);
        assert_eq!(evaluate(&font, &empty).len(), font.glyphs.len());
    }

    #[test]
    fn nested_groups_and_boolean_rules() {
        let mut font = sample_font();
        let m = font.masters[0].id.clone();
        font.glyph_mut("A")
            .unwrap()
            .layer_mut(&m)
            .unwrap()
            .shapes
            .push(Shape::component("H", IDENTITY));
        font.glyph_mut("H").unwrap().export = false;
        font.glyph_mut("I").unwrap().tags = vec!["serif".into()];
        let f = smart("Mixed", vec![Rule::HasComponents { value: true }]);
        assert_eq!(evaluate(&font, &f), ["A"]);
        let nested = Filter {
            name: "Nested".into(),
            kind: FilterKind::Smart,
            rules: RuleGroup {
                all: false,
                rules: vec![Rule::Export { value: false }],
                groups: vec![RuleGroup {
                    rules: vec![Rule::TagsContain {
                        value: "SER".into(),
                    }],
                    ..Default::default()
                }],
            },
            glyphs: Vec::new(),
        };
        let mut names = evaluate(&font, &nested);
        names.sort();
        assert_eq!(names, ["H", "I"]);
        let no_components = smart("Plain", vec![Rule::HasComponents { value: false }]);
        assert!(!evaluate(&font, &no_components).contains(&"A".to_string()));
        let unencoded = smart("Unencoded", vec![Rule::HasUnicode { value: false }]);
        assert!(evaluate(&font, &unencoded).is_empty());
    }

    #[test]
    fn script_case_and_subcategory_rules() {
        let mut font = sample_font();
        let list = crate::glyphset::parse_glyph_list("Ж U+03B1").unwrap();
        font = crate::glyphset::add_glyphs(&font, &list).unwrap().font;
        let cyrillic = smart(
            "Cyrillic",
            vec![Rule::Script {
                value: "Cyrillic".into(),
            }],
        );
        assert_eq!(evaluate(&font, &cyrillic), ["uni0416"]);
        let greek = smart(
            "Greek",
            vec![Rule::Script {
                value: "Greek".into(),
            }],
        );
        assert_eq!(evaluate(&font, &greek), ["uni03B1"]);
        let lower = smart(
            "Lower",
            vec![Rule::Case {
                value: "Lowercase".into(),
            }],
        );
        let names = evaluate(&font, &lower);
        assert!(names.contains(&"a".to_string()));
        assert!(!names.contains(&"A".to_string()));
        let lig = smart(
            "Ligatures",
            vec![Rule::Subcategory {
                value: "Ligature".into(),
            }],
        );
        assert!(evaluate(&font, &lig).is_empty());
    }

    #[test]
    fn metrics_keys_out_of_sync() {
        let mut font = sample_font();
        let m = font.masters[0].id.clone();
        // H's width is 680; a key asking for 500 is out of sync, a key asking for
        // 680 is not. I has no keys at all.
        font.glyph_mut("H")
            .unwrap()
            .layer_mut(&m)
            .unwrap()
            .metrics_keys = MetricsKeys {
            width: Some("=500".into()),
            ..Default::default()
        };
        font.glyph_mut("I")
            .unwrap()
            .layer_mut(&m)
            .unwrap()
            .metrics_keys = MetricsKeys {
            width: Some("=250".into()),
            ..Default::default()
        };
        let f = smart(
            "Out of sync",
            vec![Rule::MetricsKeysOutOfSync { value: true }],
        );
        assert_eq!(evaluate(&font, &f), ["H"]);
        let synced = crate::metricskeys::apply_all(&font).0;
        assert!(evaluate(&synced, &f).is_empty());
        let in_sync = smart("In sync", vec![Rule::MetricsKeysOutOfSync { value: false }]);
        assert!(!evaluate(&font, &in_sync).contains(&"H".to_string()));
        assert!(evaluate(&font, &in_sync).contains(&"I".to_string()));
    }

    #[test]
    fn list_filters_and_set_filters_validation() {
        let font = sample_font();
        let list = Filter {
            name: "My list".into(),
            kind: FilterKind::List,
            rules: RuleGroup::default(),
            glyphs: vec!["H".into(), "I".into(), "gone".into()],
        };
        assert_eq!(evaluate(&font, &list), ["H", "I"]);
        let f = smart("Rules", vec![Rule::Compatible { value: true }]);
        assert_eq!(evaluate(&font, &f).len(), font.glyphs.len());
        let saved = set_filters(&font, vec![f.clone(), list.clone()]).unwrap();
        assert_eq!(saved.filters.len(), 2);
        assert!(set_filters(&font, vec![f.clone(), f]).is_err());
        let mut blank = list.clone();
        blank.name = "   ".into();
        assert!(set_filters(&font, vec![blank]).is_err());
        let trimmed = set_filters(
            &font,
            vec![Filter {
                name: " Padded ".into(),
                ..list
            }],
        )
        .unwrap();
        assert_eq!(trimmed.filters[0].name, "Padded");
    }

    #[test]
    fn serde_uses_camel_case_type_tags() {
        let f = smart(
            "Serif capitals",
            vec![
                Rule::Category {
                    value: "Uppercase".into(),
                },
                Rule::TagsContain {
                    value: "serif".into(),
                },
            ],
        );
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"type\":\"category\""), "{json}");
        assert!(json.contains("\"type\":\"tagsContain\""), "{json}");
        assert!(json.contains("\"all\":true"), "{json}");
        let back: Filter = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        // A list filter round-trips without rules.
        let list = Filter {
            name: "L".into(),
            kind: FilterKind::List,
            rules: RuleGroup::default(),
            glyphs: vec!["A".into()],
        };
        let json = serde_json::to_string(&list).unwrap();
        assert!(!json.contains("rules"), "{json}");
        assert_eq!(serde_json::from_str::<Filter>(&json).unwrap(), list);
        // Missing kind defaults to smart.
        let minimal: Filter = serde_json::from_str("{\"name\":\"M\"}").unwrap();
        assert_eq!(minimal.kind, FilterKind::Smart);
        assert!(minimal.rules.all);
    }
}
