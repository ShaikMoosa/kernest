use crate::{Axis, Font, Instance, Metrics};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupMaster {
    pub name: String,
    pub location: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewFontSetup {
    pub family_name: String,
    pub units_per_em: u16,
    pub metrics: Metrics,
    pub glyph_sets: Vec<String>,
    pub axes: Vec<Axis>,
    pub masters: Vec<SetupMaster>,
    pub instances: Vec<Instance>,
}

impl Default for NewFontSetup {
    fn default() -> Self {
        Self {
            family_name: "Untitled Sans".into(),
            units_per_em: 1000,
            metrics: Metrics::default(),
            glyph_sets: vec!["latin-basic".into()],
            axes: vec![],
            masters: vec![SetupMaster {
                name: "Regular".into(),
                location: BTreeMap::new(),
            }],
            instances: vec![],
        }
    }
}

pub fn create_font(setup: &NewFontSetup) -> Result<Font, String> {
    use crate::{designspace::design_bounds, glyphdata, structure};
    let family = structure::check_family(&setup.family_name)?;
    for id in &setup.glyph_sets {
        if !glyphdata::GLYPH_SETS.iter().any(|s| s.id == id) {
            return Err(format!("Unknown glyph set \"{id}\"."));
        }
    }
    let first = setup
        .masters
        .first()
        .ok_or("A font needs at least one master.")?;
    let ids: Vec<&str> = setup.glyph_sets.iter().map(String::as_str).collect();
    let mut font = Font::new(family, glyphdata::codepoints_for(&ids));
    font = structure::set_info(&font, family, setup.units_per_em, 1, 0)?;
    let m = setup.metrics;
    if [
        m.ascender,
        m.cap_height,
        m.x_height,
        m.descender,
        m.italic_angle,
    ]
    .iter()
    .any(|v| !v.is_finite())
    {
        return Err("Metrics must be numbers.".into());
    }
    if m.descender > 0.0
        || m.x_height <= 0.0
        || m.x_height > m.cap_height
        || m.cap_height > m.ascender
    {
        return Err("Use descender ≤ 0 < x-height ≤ cap height ≤ ascender.".into());
    }
    font = structure::set_axes(&font, setup.axes.clone())?;
    for loc in setup
        .masters
        .iter()
        .map(|m| &m.location)
        .chain(setup.instances.iter().map(|i| &i.location))
    {
        for tag in loc.keys() {
            if !font.axes.iter().any(|a| &a.tag == tag) {
                return Err(format!("No axis with tag \"{tag}\"."));
            }
        }
    }
    font = structure::update_master(&font, "m01", &first.name, &first.location, m)?;
    let mut names = std::collections::HashSet::from([first.name.trim()]);
    for master in setup.masters.iter().skip(1) {
        if !names.insert(master.name.trim()) {
            return Err(format!(
                "Master name \"{}\" is used twice.",
                master.name.trim()
            ));
        }
        font = structure::add_master(&font, "m01", &master.name, &master.location)?.0;
        let added = font.masters.last().expect("a master was added");
        if font.masters[..font.masters.len() - 1]
            .iter()
            .any(|m| m.location == added.location)
        {
            return Err("Each master needs a different axis location.".into());
        }
    }
    if !font.masters.iter().any(|m| {
        font.axes
            .iter()
            .all(|a| m.location.get(&a.tag) == Some(&design_bounds(a).1))
    }) {
        return Err("Place a master at the default of every axis.".into());
    }
    for g in &mut font.glyphs {
        for l in &mut g.layers {
            l.width *= f64::from(setup.units_per_em) / 1000.0;
        }
    }
    font = structure::set_instances(&font, setup.instances.clone())?;
    font.validate().map_err(|e| e.to_string())?;
    Ok(font)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{glyphdata, package};
    use std::collections::BTreeMap;

    fn axis(tag: &str, min: f64, max: f64) -> Axis {
        Axis {
            name: tag.into(),
            tag: tag.into(),
            min,
            default: min,
            max,
            hidden: false,
            map: vec![],
            labels: vec![],
        }
    }

    fn master(name: &str, location: &[(&str, f64)]) -> SetupMaster {
        SetupMaster {
            name: name.into(),
            location: location.iter().map(|(k, v)| ((*k).into(), *v)).collect(),
        }
    }

    #[test]
    fn single_style_uses_metrics_and_deduplicates_sets() {
        let setup = NewFontSetup {
            family_name: "  First Family  ".into(),
            units_per_em: 2000,
            metrics: Metrics {
                ascender: 1600.0,
                cap_height: 1400.0,
                x_height: 1000.0,
                descender: -400.0,
                italic_angle: 0.0,
            },
            glyph_sets: vec!["latin-basic".into(), "latin-1".into(), "latin-basic".into()],
            ..Default::default()
        };
        let f = create_font(&setup).unwrap();
        assert_eq!(f.info.family_name, "First Family");
        assert_eq!(f.info.units_per_em, 2000);
        assert_eq!(f.masters.len(), 1);
        assert_eq!(f.masters[0].metrics.cap_height, 1400.0);
        assert_eq!(
            f.glyphs.len(),
            glyphdata::codepoints_for(&["latin-basic", "latin-1"]).len()
        );
        assert_eq!(f.glyph("space").unwrap().layers[0].width, 500.0);
        assert_eq!(f.glyph("A").unwrap().layers[0].width, 1200.0);
        f.validate().unwrap();
    }

    #[test]
    fn weight_family_has_all_layers_and_selected_instances() {
        let setup = NewFontSetup {
            axes: vec![axis("wght", 100.0, 900.0)],
            masters: vec![
                master("Light", &[("wght", 100.0)]),
                master("Bold", &[("wght", 900.0)]),
            ],
            instances: vec![Instance {
                name: "Regular".into(),
                location: BTreeMap::from([("wght".into(), 400.0)]),
                export: true,
                weight_class: None,
                width_class: None,
                style_link: None,
            }],
            ..Default::default()
        };
        let f = create_font(&setup).unwrap();
        assert_eq!(f.masters.len(), 2);
        assert_eq!(f.instances[0].location["wght"], 400.0);
        assert!(f.glyphs.iter().all(|g| g.layers.len() == 2));
        assert_eq!(f.masters[1].location["wght"], 900.0);
        f.validate().unwrap();
        let dir = tempfile::tempdir().unwrap();
        package::save(&f, dir.path()).unwrap();
        assert_eq!(package::open(dir.path()).unwrap(), f);
    }

    #[test]
    fn weight_width_family_has_four_distinct_corners() {
        let setup = NewFontSetup {
            axes: vec![axis("wght", 100.0, 900.0), axis("wdth", 75.0, 125.0)],
            masters: vec![
                master("Light Condensed", &[("wght", 100.0), ("wdth", 75.0)]),
                master("Bold Condensed", &[("wght", 900.0), ("wdth", 75.0)]),
                master("Light Wide", &[("wght", 100.0), ("wdth", 125.0)]),
                master("Bold Wide", &[("wght", 900.0), ("wdth", 125.0)]),
            ],
            ..Default::default()
        };
        let f = create_font(&setup).unwrap();
        assert_eq!(f.masters.len(), 4);
        assert_eq!(f.masters[3].location["wdth"], 125.0);
        assert!(f.glyphs.iter().all(|g| g.layers.len() == 4));
        f.validate().unwrap();
    }

    #[test]
    fn invalid_setup_never_produces_a_partial_font() {
        let cases: Vec<NewFontSetup> = vec![
            NewFontSetup {
                family_name: " ".into(),
                ..Default::default()
            },
            NewFontSetup {
                units_per_em: 0,
                ..Default::default()
            },
            NewFontSetup {
                glyph_sets: vec!["missing-set".into()],
                ..Default::default()
            },
            NewFontSetup {
                masters: vec![],
                ..Default::default()
            },
            NewFontSetup {
                metrics: Metrics {
                    x_height: f64::NAN,
                    ..Default::default()
                },
                ..Default::default()
            },
            NewFontSetup {
                metrics: Metrics {
                    x_height: 900.0,
                    ..Default::default()
                },
                ..Default::default()
            },
            NewFontSetup {
                axes: vec![axis("wght", 100.0, 900.0)],
                masters: vec![master("Outside", &[("wght", 1000.0)])],
                ..Default::default()
            },
            NewFontSetup {
                masters: vec![master("One", &[]), master("Two", &[])],
                ..Default::default()
            },
            NewFontSetup {
                masters: vec![master("Regular", &[("oops", 4.0)])],
                ..Default::default()
            },
            NewFontSetup {
                axes: vec![axis("wght", 100.0, 900.0)],
                masters: vec![
                    master("One", &[("wght", 400.0)]),
                    master("Two", &[("wght", 900.0)]),
                ],
                ..Default::default()
            },
        ];
        for (i, setup) in cases.iter().enumerate() {
            assert!(create_font(setup).is_err(), "case {i}");
        }
    }
}
