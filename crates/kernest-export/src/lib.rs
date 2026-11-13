//! Font export for Kernest: TrueType static fonts and variable fonts.
//!
//! Each master (or interpolated instance) is written as a UFO with norad, and
//! compiled with [fontc](https://github.com/googlefonts/fontc). Variable fonts use
//! a designspace of all masters. Static instances that sit between masters are
//! interpolated by Kernest first (same variation model as the editor's previews).

pub mod ufo;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kernest_core::designspace::{normalize, Location};
use kernest_core::interpolate::{check_glyph, Interpolator};
use kernest_core::{structure, Font, KernPair, Layer, Master, Metrics};
use serde::Serialize;

use ufo::{designspace, to_ufo, UfoSource};

/// What to build and where.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// One variable TrueType font with all masters.
    pub variable: bool,
    /// One static TrueType font per exported instance (or per master if there are none).
    pub statics: bool,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFile {
    pub path: String,
    /// "variable" | "static"
    pub kind: &'static str,
    pub style: String,
    pub bytes: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    pub files: Vec<ExportedFile>,
    /// Targets that could not be built, with the reason.
    pub problems: Vec<String>,
}

/// A file name part: letters, digits and hyphens only (e.g. "Kernest Sans" → "KernestSans").
pub fn file_stem(s: &str) -> String {
    let out: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if out.is_empty() {
        "Untitled".into()
    } else {
        out
    }
}

fn compile(input: &Path) -> Result<Vec<u8>, String> {
    let source = fontc::Input::new(input)
        .and_then(|i| i.create_source(&fontc::SourceOptions::default()))
        .map_err(|e| format!("could not read the generated sources: {e}"))?;
    fontc::generate_font(source, fontc::Options::default())
        .map_err(|e| format!("the font compiler failed: {e}"))
}

fn master_source<'a>(font: &'a Font, master: &Master, style: &'a str) -> UfoSource<'a> {
    UfoSource {
        font,
        style_name: style,
        metrics: master.metrics,
        layers: font
            .glyphs
            .iter()
            .filter_map(|g| g.layer(&master.id).map(|l| (g.name.as_str(), l.clone())))
            .collect(),
        kerning: font.kerning.get(&master.id).cloned().unwrap_or_default(),
    }
}

/// The font at a design-space location, as one master's worth of data.
fn instance_source<'a>(
    font: &'a Font,
    style: &'a str,
    location: &Location,
) -> Result<UfoSource<'a>, String> {
    // On a master: use it directly (works even if other masters are incompatible).
    let norm = normalize(font, location);
    if let Some(m) = font
        .masters
        .iter()
        .find(|m| normalize(font, &m.location) == norm)
    {
        return Ok(master_source(font, m, style));
    }
    let interp = Interpolator::new(font, location)?;
    let mut layers: Vec<(&str, Layer)> = Vec::with_capacity(font.glyphs.len());
    let mut failed: Vec<&str> = Vec::new();
    for g in &font.glyphs {
        match interp.layer(font, &g.name) {
            Ok(l) => layers.push((g.name.as_str(), l)),
            Err(_) => failed.push(&g.name),
        }
    }
    if !failed.is_empty() {
        let shown: Vec<&str> = failed.iter().take(8).copied().collect();
        return Err(format!(
            "{style}: {} glyph{} can't be interpolated ({}{})",
            failed.len(),
            if failed.len() == 1 { "" } else { "s" },
            shown.join(", "),
            if failed.len() > 8 { ", …" } else { "" }
        ));
    }
    let per_master: Vec<Vec<f64>> = font
        .masters
        .iter()
        .map(|m| {
            vec![
                m.metrics.ascender,
                m.metrics.cap_height,
                m.metrics.x_height,
                m.metrics.descender,
                m.metrics.italic_angle,
            ]
        })
        .collect();
    let v = interp.values(&per_master);
    let metrics = Metrics {
        ascender: v[0],
        cap_height: v[1],
        x_height: v[2],
        descender: v[3],
        italic_angle: v[4],
    };

    // Kerning: every pair that exists in any master, interpolated (missing = 0).
    let keys: BTreeSet<(String, String)> = font
        .kerning
        .values()
        .flatten()
        .map(|p| (p.left.clone(), p.right.clone()))
        .collect();
    let keys: Vec<(String, String)> = keys.into_iter().collect();
    let per_master: Vec<Vec<f64>> = font
        .masters
        .iter()
        .map(|m| {
            let pairs: BTreeMap<(&str, &str), f64> = font
                .kerning
                .get(&m.id)
                .map(|ps| {
                    ps.iter()
                        .map(|p| ((p.left.as_str(), p.right.as_str()), p.value))
                        .collect()
                })
                .unwrap_or_default();
            keys.iter()
                .map(|(l, r)| pairs.get(&(l.as_str(), r.as_str())).copied().unwrap_or(0.0))
                .collect()
        })
        .collect();
    let values = interp.values(&per_master);
    let kerning = keys
        .into_iter()
        .zip(values)
        .filter_map(|((left, right), v)| {
            let value = v.round();
            (value != 0.0).then_some(KernPair { left, right, value })
        })
        .collect();
    Ok(UfoSource {
        font,
        style_name: style,
        metrics,
        layers,
        kerning,
    })
}

fn write_out(dir: &Path, file: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = dir.join(file);
    std::fs::write(&path, bytes).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// Builds the requested fonts into `opts.out_dir`. Targets that fail are listed in
/// `problems`; the others are still written.
pub fn export(font: &Font, opts: &ExportOptions) -> Result<ExportReport, String> {
    std::fs::create_dir_all(&opts.out_dir)
        .map_err(|e| format!("could not create {}: {e}", opts.out_dir.display()))?;
    let tmp =
        tempfile::tempdir().map_err(|e| format!("could not create a temporary folder: {e}"))?;
    let family = file_stem(&font.info.family_name);
    let mut report = ExportReport::default();

    if opts.variable {
        match variable(font, tmp.path()) {
            Ok(bytes) => {
                let path = write_out(&opts.out_dir, &format!("{family}-VF.ttf"), &bytes)?;
                report.files.push(ExportedFile {
                    path: path.display().to_string(),
                    kind: "variable",
                    style: "Variable".into(),
                    bytes: bytes.len(),
                });
            }
            Err(e) => report.problems.push(format!("Variable font: {e}")),
        }
    }

    if opts.statics {
        let targets: Vec<(String, Location)> = if font.instances.iter().any(|i| i.export) {
            font.instances
                .iter()
                .filter(|i| i.export)
                .map(|i| (i.name.clone(), i.location.clone()))
                .collect()
        } else {
            font.masters
                .iter()
                .map(|m| (m.name.clone(), m.location.clone()))
                .collect()
        };
        for (i, (style, location)) in targets.iter().enumerate() {
            let built = instance_source(font, style, location).and_then(|src| {
                let path = tmp.path().join(format!("instance{i}.ufo"));
                to_ufo(&src)?
                    .save(&path)
                    .map_err(|e| format!("could not write a UFO: {e}"))?;
                compile(&path)
            });
            match built {
                Ok(bytes) => {
                    let path = write_out(
                        &opts.out_dir,
                        &format!("{family}-{}.ttf", file_stem(style)),
                        &bytes,
                    )?;
                    report.files.push(ExportedFile {
                        path: path.display().to_string(),
                        kind: "static",
                        style: style.clone(),
                        bytes: bytes.len(),
                    });
                }
                Err(e) => report.problems.push(if e.starts_with(style.as_str()) {
                    e
                } else {
                    format!("{style}: {e}")
                }),
            }
        }
    }
    Ok(report)
}

fn variable(font: &Font, tmp: &Path) -> Result<Vec<u8>, String> {
    if font.masters.len() < 2 || font.axes.is_empty() {
        return Err(
            "needs at least two masters and an axis (Font Info › Axes and Masters).".into(),
        );
    }
    if let Some(w) = structure::warnings(font).into_iter().next() {
        return Err(w);
    }
    let incompatible: Vec<&str> = font
        .glyphs
        .iter()
        .filter(|g| !check_glyph(font, &g.name).is_empty())
        .map(|g| g.name.as_str())
        .collect();
    if !incompatible.is_empty() {
        let shown: Vec<&str> = incompatible.iter().take(8).copied().collect();
        return Err(format!(
            "{} glyph{} can't be interpolated ({}{}). Fix them or export static fonts only.",
            incompatible.len(),
            if incompatible.len() == 1 { "" } else { "s" },
            shown.join(", "),
            if incompatible.len() > 8 { ", …" } else { "" }
        ));
    }
    let mut sources = Vec::new();
    for (i, m) in font.masters.iter().enumerate() {
        let file = format!("master{i}.ufo");
        to_ufo(&master_source(font, m, &m.name))?
            .save(tmp.join(&file))
            .map_err(|e| format!("could not write a UFO: {e}"))?;
        sources.push((file, m.name.clone(), m.location.clone()));
    }
    let ds = tmp.join("font.designspace");
    std::fs::write(&ds, designspace(font, &sources))
        .map_err(|e| format!("could not write the designspace: {e}"))?;
    compile(&ds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernest_core::sample::sample_font;
    use kernest_core::{structure, Axis, Instance, NodeKind, Shape};
    use write_fonts::read::{FontRef, TableProvider};

    fn advance(bytes: &[u8], ch: char) -> u16 {
        let f = FontRef::new(bytes).expect("a valid font");
        let gid = f.cmap().unwrap().map_codepoint(ch).expect("mapped");
        f.hmtx().unwrap().advance(gid).expect("advance")
    }

    fn two_masters() -> Font {
        let wght = Axis {
            name: "Weight".into(),
            tag: "wght".into(),
            min: 400.0,
            default: 400.0,
            max: 700.0,
            hidden: false,
        };
        let f = structure::set_axes(&sample_font(), vec![wght]).unwrap();
        let (mut f, bold) =
            structure::add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)]))
                .unwrap();
        let h = f.glyph_mut("H").unwrap().layer_mut(&bold).unwrap();
        h.width += 100.0;
        f
    }

    #[test]
    fn file_stems() {
        assert_eq!(file_stem("Kernest Sans"), "KernestSans");
        assert_eq!(file_stem("Semi Bold"), "SemiBold");
        assert_eq!(file_stem("   "), "Untitled");
    }

    #[test]
    fn static_from_a_single_master() {
        let out = tempfile::tempdir().unwrap();
        let r = export(
            &sample_font(),
            &ExportOptions {
                variable: false,
                statics: true,
                out_dir: out.path().into(),
            },
        )
        .unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.files.len(), 1);
        assert!(r.files[0].path.ends_with("KernestSample-Regular.ttf"));
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let f = FontRef::new(&bytes).unwrap();
        assert_eq!(f.maxp().unwrap().num_glyphs(), 96, "95 glyphs + .notdef");
        assert_eq!(advance(&bytes, 'H'), 680);
        assert!(f.fvar().is_err(), "static fonts have no fvar");
        assert!(f.gpos().is_ok(), "kerning is compiled into GPOS");
    }

    #[test]
    fn variable_and_interpolated_instance() {
        let mut f = two_masters();
        f.instances.push(Instance {
            name: "Medium".into(),
            location: BTreeMap::from([("wght".into(), 550.0)]),
            export: true,
        });
        let out = tempfile::tempdir().unwrap();
        let r = export(
            &f,
            &ExportOptions {
                variable: true,
                statics: true,
                out_dir: out.path().into(),
            },
        )
        .unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let vf = r.files.iter().find(|x| x.kind == "variable").unwrap();
        let bytes = std::fs::read(&vf.path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let axes = font.fvar().unwrap().axes().unwrap();
        assert_eq!(axes.len(), 1);
        assert_eq!(axes[0].axis_tag().to_string(), "wght");
        assert_eq!(
            (axes[0].min_value().to_f64(), axes[0].max_value().to_f64()),
            (400.0, 700.0)
        );
        assert!(font.gvar().is_ok());
        // The static Medium sits halfway: H is 680 at Regular and 780 at Bold.
        let medium = r.files.iter().find(|x| x.style == "Medium").unwrap();
        assert!(medium.path.ends_with("KernestSample-Medium.ttf"));
        assert_eq!(advance(&std::fs::read(&medium.path).unwrap(), 'H'), 730);
    }

    #[test]
    fn incompatible_glyphs_block_only_the_variable_font() {
        let mut f = two_masters();
        let bold = f.masters[1].id.clone();
        if let Shape::Path(p) = &mut f.glyph_mut("H").unwrap().layer_mut(&bold).unwrap().shapes[0] {
            p.nodes.pop();
            assert!(p.nodes.iter().all(|n| n.kind == NodeKind::Line));
        }
        let out = tempfile::tempdir().unwrap();
        let r = export(
            &f,
            &ExportOptions {
                variable: true,
                statics: true,
                out_dir: out.path().into(),
            },
        )
        .unwrap();
        assert_eq!(r.problems.len(), 1, "{:?}", r.problems);
        assert!(
            r.problems[0].contains("Variable font") && r.problems[0].contains("H"),
            "{:?}",
            r.problems
        );
        // Both masters still export as statics.
        assert_eq!(r.files.iter().filter(|x| x.kind == "static").count(), 2);
    }

    #[test]
    fn variable_needs_masters() {
        let out = tempfile::tempdir().unwrap();
        let r = export(
            &sample_font(),
            &ExportOptions {
                variable: true,
                statics: false,
                out_dir: out.path().into(),
            },
        )
        .unwrap();
        assert!(r.files.is_empty());
        assert!(r.problems[0].contains("two masters"));
    }
}
