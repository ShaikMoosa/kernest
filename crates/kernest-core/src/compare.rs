//! S42 · Compare two fonts. A read-only report of what differs: Font Info fields,
//! master metrics, per-glyph outline complexity and advance width, and kerning.
//! Pure like the rest of the core: it only reads the two fonts and returns rows
//! for the differences, so the desktop shell can show them in a report window.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::interpolate::default_master;
use crate::model::{Font, Layer, Shape};

/// One differing Font Info or metrics field, with both values as display strings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldDiff {
    /// A stable key ("familyName", "ascender", …) the UI turns into a label.
    pub field: String,
    pub current: String,
    pub other: String,
}

/// A master present in one font only, or whose metrics differ. Matched by name.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterDiff {
    pub name: String,
    /// "current" or "other" when the master exists on one side only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_in: Option<String>,
    pub metrics: Vec<FieldDiff>,
}

/// A glyph present in one font only, or whose default-master outline or width differs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphDiff {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only_in: Option<String>,
    /// (current, other) path counts on the default master.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<(usize, usize)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes: Option<(usize, usize)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<(f64, f64)>,
}

/// A kerning pair whose value differs, or that exists on one side only.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KernDiff {
    pub left: String,
    pub right: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub other: Option<f64>,
}

/// The whole comparison. Empty sections mean "no differences" there.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontComparison {
    pub info: Vec<FieldDiff>,
    pub masters: Vec<MasterDiff>,
    pub glyphs: Vec<GlyphDiff>,
    pub kerning: Vec<KernDiff>,
}

fn num(v: f64) -> String {
    format!("{v}")
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_default()
}

fn info_fields(f: &Font) -> Vec<(&'static str, String)> {
    let i = &f.info;
    vec![
        ("familyName", i.family_name.clone()),
        ("unitsPerEm", i.units_per_em.to_string()),
        (
            "version",
            format!("{}.{:03}", i.version_major, i.version_minor),
        ),
        ("designer", opt(&i.designer)),
        ("designerUrl", opt(&i.designer_url)),
        ("manufacturer", opt(&i.manufacturer)),
        ("manufacturerUrl", opt(&i.manufacturer_url)),
        ("copyright", opt(&i.copyright)),
        ("license", opt(&i.license)),
        ("licenseUrl", opt(&i.license_url)),
        ("vendorId", opt(&i.vendor_id)),
        ("description", opt(&i.description)),
        ("trademark", opt(&i.trademark)),
        ("sampleText", opt(&i.sample_text)),
    ]
}

fn metric_fields(m: &crate::model::Metrics) -> Vec<(&'static str, String)> {
    vec![
        ("ascender", num(m.ascender)),
        ("capHeight", num(m.cap_height)),
        ("xHeight", num(m.x_height)),
        ("descender", num(m.descender)),
        ("italicAngle", num(m.italic_angle)),
    ]
}

/// (path count, node count) of a layer's outline; components are not paths.
fn outline_counts(layer: &Layer) -> (usize, usize) {
    let mut paths = 0;
    let mut nodes = 0;
    for s in &layer.shapes {
        if let Shape::Path(p) = s {
            paths += 1;
            nodes += p.nodes.len();
        }
    }
    (paths, nodes)
}

fn default_layer<'a>(f: &'a Font, name: &str) -> Option<&'a Layer> {
    let id = &default_master(f).id;
    f.glyph(name).and_then(|g| g.layer(id))
}

fn kerning_map(f: &Font) -> BTreeMap<(String, String), f64> {
    let id = &default_master(f).id;
    f.kerning
        .get(id)
        .map(|pairs| {
            pairs
                .iter()
                .map(|p| ((p.left.clone(), p.right.clone()), p.value))
                .collect()
        })
        .unwrap_or_default()
}

/// Reports every difference between `current` (the open font) and `other`.
pub fn compare_fonts(current: &Font, other: &Font) -> FontComparison {
    let mut out = FontComparison::default();

    let cf = info_fields(current);
    let of = info_fields(other);
    for ((key, cv), (_, ov)) in cf.iter().zip(of.iter()) {
        if cv != ov {
            out.info.push(FieldDiff {
                field: (*key).to_string(),
                current: cv.clone(),
                other: ov.clone(),
            });
        }
    }

    // Masters matched by name; report metric differences and one-sided masters.
    let mut names: Vec<String> = current.masters.iter().map(|m| m.name.clone()).collect();
    for m in &other.masters {
        if !names.contains(&m.name) {
            names.push(m.name.clone());
        }
    }
    for name in names {
        let cm = current.masters.iter().find(|m| m.name == name);
        let om = other.masters.iter().find(|m| m.name == name);
        match (cm, om) {
            (Some(c), Some(o)) => {
                let cmf = metric_fields(&c.metrics);
                let omf = metric_fields(&o.metrics);
                let metrics: Vec<FieldDiff> = cmf
                    .iter()
                    .zip(omf.iter())
                    .filter(|((_, cv), (_, ov))| cv != ov)
                    .map(|((key, cv), (_, ov))| FieldDiff {
                        field: (*key).to_string(),
                        current: cv.clone(),
                        other: ov.clone(),
                    })
                    .collect();
                if !metrics.is_empty() {
                    out.masters.push(MasterDiff {
                        name,
                        only_in: None,
                        metrics,
                    });
                }
            }
            (Some(_), None) => out.masters.push(MasterDiff {
                name,
                only_in: Some("current".into()),
                metrics: Vec::new(),
            }),
            (None, Some(_)) => out.masters.push(MasterDiff {
                name,
                only_in: Some("other".into()),
                metrics: Vec::new(),
            }),
            (None, None) => {}
        }
    }

    // Glyphs: one-sided, or differing outline complexity / width on the default master.
    let mut gnames: Vec<String> = current.glyphs.iter().map(|g| g.name.clone()).collect();
    for g in &other.glyphs {
        if !gnames.contains(&g.name) {
            gnames.push(g.name.clone());
        }
    }
    for name in gnames {
        let cl = default_layer(current, &name);
        let ol = default_layer(other, &name);
        match (cl, ol) {
            (Some(c), Some(o)) => {
                let (cp, cn) = outline_counts(c);
                let (op, on) = outline_counts(o);
                if cp != op || cn != on || c.width != o.width {
                    out.glyphs.push(GlyphDiff {
                        name,
                        only_in: None,
                        paths: Some((cp, op)),
                        nodes: Some((cn, on)),
                        width: Some((c.width, o.width)),
                    });
                }
            }
            (Some(_), None) => out.glyphs.push(GlyphDiff {
                name,
                only_in: Some("current".into()),
                paths: None,
                nodes: None,
                width: None,
            }),
            (None, Some(_)) => out.glyphs.push(GlyphDiff {
                name,
                only_in: Some("other".into()),
                paths: None,
                nodes: None,
                width: None,
            }),
            (None, None) => {}
        }
    }

    // Kerning on the default master: differing values and one-sided pairs.
    let ck = kerning_map(current);
    let ok = kerning_map(other);
    let keys: BTreeSet<(String, String)> = ck.keys().chain(ok.keys()).cloned().collect();
    for (left, right) in keys {
        let cv = ck.get(&(left.clone(), right.clone())).copied();
        let ov = ok.get(&(left.clone(), right.clone())).copied();
        if cv != ov {
            out.kerning.push(KernDiff {
                left,
                right,
                current: cv,
                other: ov,
            });
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Glyph, KernPair, Layer, Path};
    use crate::sample::sample_font;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
        Shape::Path(Path {
            closed: true,
            locked: false,
            nodes: vec![
                crate::model::Node::new(x0, y0, crate::model::NodeKind::Line),
                crate::model::Node::new(x1, y0, crate::model::NodeKind::Line),
                crate::model::Node::new(x1, y1, crate::model::NodeKind::Line),
                crate::model::Node::new(x0, y1, crate::model::NodeKind::Line),
            ],
        })
    }

    #[test]
    fn identical_fonts_have_no_differences() {
        let a = sample_font();
        let r = compare_fonts(&a, &a);
        assert!(
            r.info.is_empty()
                && r.masters.is_empty()
                && r.glyphs.is_empty()
                && r.kerning.is_empty()
        );
    }

    #[test]
    fn compare_reports_info_metrics_glyphs_and_kerning() {
        let current = sample_font();
        let mut other = sample_font();
        other.info.family_name = "Other Family".into();
        other.info.units_per_em = 2048;
        other.masters[0].metrics.ascender = 850.0;
        // H: different node count and width on the default master.
        let h = other.glyph_mut("H").unwrap();
        let hl = h.layer_mut("m01").unwrap();
        hl.width = 700.0;
        hl.shapes = vec![rect(0.0, 0.0, 100.0, 700.0)];
        // I only in current; a new glyph only in other.
        other.glyphs.retain(|g| g.name != "I");
        let mut z = Glyph::new("foo", vec![]);
        z.layers.push(Layer::new("m01", 500.0));
        other.glyphs.push(z);
        // Kerning: change one value, add one pair.
        let k = other.kerning.get_mut("m01").unwrap();
        k[0].value -= 10.0;
        k.push(KernPair {
            left: "H".into(),
            right: "O".into(),
            value: -40.0,
        });

        let r = compare_fonts(&current, &other);
        assert!(r.info.iter().any(|d| d.field == "familyName"
            && d.current == "Kernest Sample"
            && d.other == "Other Family"));
        assert!(r
            .info
            .iter()
            .any(|d| d.field == "unitsPerEm" && d.current == "1000" && d.other == "2048"));
        let m = r.masters.iter().find(|m| m.name == "Regular").unwrap();
        assert!(m.only_in.is_none());
        assert!(m
            .metrics
            .iter()
            .any(|d| d.field == "ascender" && d.current == "800" && d.other == "850"));
        let h = r.glyphs.iter().find(|g| g.name == "H").unwrap();
        assert_eq!(h.paths, Some((3, 1)));
        assert_eq!(h.nodes, Some((12, 4)));
        assert_eq!(h.width, Some((680.0, 700.0)));
        assert!(r
            .glyphs
            .iter()
            .any(|g| g.name == "I" && g.only_in.as_deref() == Some("current")));
        assert!(r
            .glyphs
            .iter()
            .any(|g| g.name == "foo" && g.only_in.as_deref() == Some("other")));
        // Unchanged glyphs are not listed.
        assert!(!r.glyphs.iter().any(|g| g.name == "O"));
        assert!(r.kerning.iter().any(|d| d.left == "H"
            && d.right == "O"
            && d.current.is_none()
            && d.other == Some(-40.0)));
        assert!(r.kerning.iter().any(|d| d.left == "T"
            && d.right == "o"
            && d.current == Some(-60.0)
            && d.other == Some(-70.0)));
    }

    #[test]
    fn one_sided_masters_are_reported() {
        let current = sample_font();
        let mut other = sample_font();
        other.masters.push(crate::model::Master {
            id: "m02".into(),
            name: "Bold".into(),
            location: Default::default(),
            metrics: Default::default(),
            guides: Vec::new(),
        });
        let r = compare_fonts(&current, &other);
        assert!(r
            .masters
            .iter()
            .any(|m| m.name == "Bold" && m.only_in.as_deref() == Some("other")));
    }

    #[test]
    fn empty_layer_counts_as_zero_paths() {
        let current = sample_font();
        let mut other = sample_font();
        let h = other.glyph_mut("H").unwrap();
        h.layer_mut("m01").unwrap().shapes = Vec::new();
        let r = compare_fonts(&current, &other);
        let h = r.glyphs.iter().find(|g| g.name == "H").unwrap();
        assert_eq!(h.paths, Some((3, 0)));
        assert_eq!(h.nodes, Some((12, 0)));
    }

    #[test]
    fn layer_with_no_outline_on_one_side_is_one_sided() {
        let current = sample_font();
        let mut other = sample_font();
        // Remove H's default layer entirely from other.
        let h = other.glyph_mut("H").unwrap();
        h.layers.retain(|l| l.master_id != "m01" || l.id.is_some());
        let r = compare_fonts(&current, &other);
        assert!(r
            .glyphs
            .iter()
            .any(|g| g.name == "H" && g.only_in.as_deref() == Some("current")));
    }

    #[test]
    fn kerning_only_on_one_side_is_reported() {
        let current = sample_font();
        let mut other = sample_font();
        other.kerning.remove("m01");
        let r = compare_fonts(&current, &other);
        assert!(!r.kerning.is_empty());
        assert!(r
            .kerning
            .iter()
            .all(|d| d.current.is_some() && d.other.is_none()));
    }

    #[test]
    fn unused_layer_helper_keeps_counts_to_paths() {
        let l = Layer {
            master_id: "m01".into(),
            id: None,
            location: None,
            bracket: Vec::new(),
            smart: None,
            color: None,
            width: 500.0,
            shapes: vec![rect(0.0, 0.0, 10.0, 10.0)],
            anchors: Vec::new(),
            metrics_keys: Default::default(),
            guides: Vec::new(),
            background: None,
        };
        assert_eq!(outline_counts(&l), (1, 4));
    }
}
