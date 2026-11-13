//! Font export for Kernest: TrueType static fonts and variable fonts.
//!
//! Each master (or interpolated instance) is written as a UFO with norad, and
//! compiled with [fontc](https://github.com/googlefonts/fontc). Variable fonts use
//! a designspace of all masters. Static instances that sit between masters are
//! interpolated by Kernest first (same variation model as the editor's previews).

pub mod cff;
pub mod cff2;
pub mod ufo;
pub mod woff2;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kernest_core::designspace::{normalize, Location};
use kernest_core::interpolate::{check_glyph, Interpolator};
use kernest_core::{structure, Font, KernPair, Layer, Master, Metrics};
use serde::Serialize;

use ufo::{designspace, to_ufo, DsRule, DsSource, UfoSource};

/// What to build and where.
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// One variable font with all masters.
    pub variable: bool,
    /// One static font per exported instance (or per master if there are none).
    pub statics: bool,
    /// TrueType outlines (`.ttf`).
    pub ttf: bool,
    /// PostScript (CFF) outlines (`.otf`), for static fonts.
    pub otf: bool,
    /// Web fonts (`.woff2`, TrueType outlines inside).
    pub woff2: bool,
    /// Merge overlapping contours in static fonts (variable fonts keep them, as their
    /// masters must stay compatible).
    pub remove_overlaps: bool,
    /// Apply metrics keys (such as `=n`) to every glyph before building.
    pub apply_metrics_keys: bool,
    pub out_dir: PathBuf,
}

impl ExportOptions {
    /// TrueType files only, as in earlier versions.
    pub fn ttf(variable: bool, statics: bool, out_dir: impl Into<PathBuf>) -> Self {
        Self {
            variable,
            statics,
            ttf: true,
            otf: false,
            woff2: false,
            remove_overlaps: false,
            apply_metrics_keys: true,
            out_dir: out_dir.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedFile {
    pub path: String,
    /// "variable" | "static"
    pub kind: &'static str,
    /// "ttf" | "otf" | "woff2"
    pub format: &'static str,
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

/// One master as a UFO source. For a static font (`for_static`) each glyph uses the
/// layer that applies at the master's location (a bracket alternate if its ranges
/// include it); for the variable font, the master's own layers.
fn master_source<'a>(
    font: &'a Font,
    master: &Master,
    style: &'a str,
    for_static: bool,
) -> UfoSource<'a> {
    UfoSource {
        font,
        style_name: style,
        metrics: master.metrics,
        layers: font
            .glyphs
            .iter()
            .filter_map(|g| {
                let l = if for_static {
                    font.static_layer(&g.name, &master.id)
                } else {
                    g.layer(&master.id)
                };
                l.map(|l| (g.name.clone(), l.clone()))
            })
            .collect(),
        kerning: font.kerning.get(&master.id).cloned().unwrap_or_default(),
        alternates: Vec::new(),
        location: master.location.clone(),
    }
}

/// The generated glyph name for a glyph's `n`-th alternate (bracket layers).
pub fn alternate_name(glyph: &str, n: usize) -> String {
    format!("{glyph}.BRACKET.{}", n + 1)
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
        return Ok(master_source(font, m, style, true));
    }
    let interp = Interpolator::new(font, location)?;
    let mut layers: Vec<(String, Layer)> = Vec::with_capacity(font.glyphs.len());
    let mut failed: Vec<&str> = Vec::new();
    for g in &font.glyphs {
        match interp.layer(font, &g.name) {
            Ok(l) => layers.push((g.name.clone(), l)),
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
        alternates: Vec::new(),
        location: location.clone(),
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

    if !opts.ttf && !opts.otf && !opts.woff2 {
        return Err("Choose at least one format: TTF, OTF or WOFF2.".into());
    }
    let keyed;
    let font = if opts.apply_metrics_keys {
        let (f, problems) = kernest_core::metricskeys::apply_all(font);
        report
            .problems
            .extend(problems.into_iter().map(|p| format!("Metrics keys: {p}")));
        keyed = f;
        &keyed
    } else {
        font
    };
    let emit = |report: &mut ExportReport,
                kind: &'static str,
                format: &'static str,
                style: &str,
                file: String,
                bytes: &[u8]|
     -> Result<(), String> {
        let path = write_out(&opts.out_dir, &file, bytes)?;
        report.files.push(ExportedFile {
            path: path.display().to_string(),
            kind,
            format,
            style: style.to_string(),
            bytes: bytes.len(),
        });
        Ok(())
    };

    if opts.variable {
        match variable(font, tmp.path()) {
            Ok(bytes) => {
                if opts.ttf {
                    emit(
                        &mut report,
                        "variable",
                        "ttf",
                        "Variable",
                        format!("{family}-VF.ttf"),
                        &bytes,
                    )?;
                }
                if opts.woff2 {
                    match woff2::to_woff2(&bytes) {
                        Ok(w) => emit(
                            &mut report,
                            "variable",
                            "woff2",
                            "Variable",
                            format!("{family}-VF.woff2"),
                            &w,
                        )?,
                        Err(e) => report.problems.push(format!("Variable font (WOFF2): {e}")),
                    }
                }
                if opts.otf {
                    match variable_otf(font, &bytes) {
                        Ok(o) => emit(
                            &mut report,
                            "variable",
                            "otf",
                            "Variable",
                            format!("{family}-VF.otf"),
                            &o,
                        )?,
                        Err(e) => report.problems.push(format!("Variable font (OTF): {e}")),
                    }
                }
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
            let built = instance_source(font, style, location).and_then(|mut src| {
                if opts.remove_overlaps {
                    for (_, layer) in &mut src.layers {
                        kernest_core::overlap::remove_overlaps(layer);
                    }
                }
                let path = tmp.path().join(format!("instance{i}.ufo"));
                let ufo = to_ufo(&src)?;
                ufo.save(&path)
                    .map_err(|e| format!("could not write a UFO: {e}"))?;
                Ok((compile(&path)?, ufo))
            });
            match built {
                Ok((bytes, ufo)) => {
                    let stem = format!("{family}-{}", file_stem(style));
                    if opts.ttf {
                        emit(
                            &mut report,
                            "static",
                            "ttf",
                            style,
                            format!("{stem}.ttf"),
                            &bytes,
                        )?;
                    }
                    if opts.otf {
                        match cff::ttf_to_otf(&bytes, &ufo, &font.info.family_name, style) {
                            Ok(otf) => emit(
                                &mut report,
                                "static",
                                "otf",
                                style,
                                format!("{stem}.otf"),
                                &otf,
                            )?,
                            Err(e) => report.problems.push(format!("{style} (OTF): {e}")),
                        }
                    }
                    if opts.woff2 {
                        match woff2::to_woff2(&bytes) {
                            Ok(w) => emit(
                                &mut report,
                                "static",
                                "woff2",
                                style,
                                format!("{stem}.woff2"),
                                &w,
                            )?,
                            Err(e) => report.problems.push(format!("{style} (WOFF2): {e}")),
                        }
                    }
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

/// The variable font with CFF2 outlines, made from the variable TrueType font.
fn variable_otf(font: &Font, vf_ttf: &[u8]) -> Result<Vec<u8>, String> {
    let alternates: BTreeMap<String, (String, Vec<kernest_core::AxisRange>)> = font
        .glyphs
        .iter()
        .flat_map(|g| {
            g.bracket_groups()
                .into_iter()
                .enumerate()
                .map(|(n, r)| (alternate_name(&g.name, n), (g.name.clone(), r)))
        })
        .collect();
    let def = kernest_core::interpolate::default_master(font);
    let mut statics = norad::Font::new();
    statics
        .default_layer_mut()
        .insert_glyph(ufo::notdef(&def.metrics));
    cff2::vf_to_otf(vf_ttf, font, &alternates, &statics)
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
    compile(&write_sources(font, tmp)?)
}

/// Writes every master as a UFO plus a designspace into `dir`, and returns the
/// designspace's path. This is the source set the variable font is compiled from.
///
/// - Bracket layers become alternate glyphs (`A.BRACKET.1`) in every master, swapped
///   in by designspace rules inside their axis ranges.
/// - Brace layers become sparse sources: one extra layer in the default master's UFO
///   per distinct location.
pub fn write_sources(font: &Font, dir: &Path) -> Result<PathBuf, String> {
    // Alternates: (glyph, alternate name, ranges).
    let mut rules = Vec::new();
    for g in &font.glyphs {
        for (n, ranges) in g.bracket_groups().into_iter().enumerate() {
            rules.push(DsRule {
                glyph: g.name.clone(),
                with: alternate_name(&g.name, n),
                ranges,
            });
        }
    }
    // Brace locations, each with the glyphs that have a brace layer there.
    let mut braces: Vec<(Location, Vec<(String, Layer)>)> = Vec::new();
    for g in &font.glyphs {
        for l in g.brace_layers() {
            let loc = l.location.clone().unwrap_or_default();
            let mut plain = l.clone();
            plain.id = None;
            plain.location = None;
            match braces.iter_mut().find(|(b, _)| *b == loc) {
                Some((_, list)) => list.push((g.name.clone(), plain)),
                None => braces.push((loc, vec![(g.name.clone(), plain)])),
            }
        }
    }
    let default_id = kernest_core::interpolate::default_master(font).id.clone();
    let mut sources = Vec::new();
    for (i, m) in font.masters.iter().enumerate() {
        let file = format!("master{i}.ufo");
        let mut src = master_source(font, m, &m.name, false);
        for r in &rules {
            let g = font.glyph(&r.glyph).expect("rule glyphs exist");
            let layer = g
                .bracket_layers()
                .find(|l| l.master_id == m.id && l.bracket == r.ranges)
                .or_else(|| g.layer(&m.id))
                .cloned()
                .ok_or_else(|| format!("\"{}\" has no layer for {}", g.name, m.name))?;
            src.layers.push((r.with.clone(), layer));
            src.alternates.push((r.with.clone(), r.glyph.clone()));
        }
        let mut ufo = to_ufo(&src)?;
        if m.id == default_id {
            for (k, (loc, glyphs)) in braces.iter().enumerate() {
                let layer_name = format!("brace{}", k + 1);
                let layer = ufo
                    .layers
                    .new_layer(&layer_name)
                    .map_err(|e| format!("could not add a UFO layer: {e}"))?;
                for (gname, l) in glyphs {
                    layer.insert_glyph(ufo::glyph(font, gname, l)?);
                }
                sources.push(DsSource {
                    file: file.clone(),
                    style: format!("{} {{{}}}", m.name, k + 1),
                    location: loc.clone(),
                    layer: Some(layer_name),
                });
            }
        }
        ufo.save(dir.join(&file))
            .map_err(|e| format!("could not write a UFO: {e}"))?;
        sources.push(DsSource {
            file,
            style: m.name.clone(),
            location: m.location.clone(),
            layer: None,
        });
    }
    let ds = dir.join("font.designspace");
    std::fs::write(&ds, designspace(font, &sources, &rules))
        .map_err(|e| format!("could not write the designspace: {e}"))?;
    Ok(ds)
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
        let r = export(&sample_font(), &ExportOptions::ttf(false, true, out.path())).unwrap();
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
        let r = export(&f, &ExportOptions::ttf(true, true, out.path())).unwrap();
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
    fn brace_and_bracket_layers_export() {
        use kernest_core::{layers, AxisRange};
        let f = two_masters();
        let bold = f.masters[1].id.clone();
        // A brace layer for H at 550 and an alternate for o from 600 up, 50 units wider.
        let (f, brace) =
            layers::add_brace_layer(&f, "H", "m01", &BTreeMap::from([("wght".into(), 550.0)]))
                .unwrap();
        let ranges = [AxisRange {
            tag: "wght".into(),
            min: Some(600.0),
            max: None,
        }];
        let (mut f, _) = layers::add_bracket_layers(&f, "o", &bold, &ranges).unwrap();
        f.glyph_mut("H").unwrap().layer_mut(&brace).unwrap().width += 20.0;
        for l in f.glyph_mut("o").unwrap().layers.iter_mut() {
            if !l.bracket.is_empty() {
                l.width += 50.0;
            }
        }
        let o_regular = f.glyph("o").unwrap().layer("m01").unwrap().width as u16;
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);

        let vf =
            std::fs::read(&r.files.iter().find(|x| x.kind == "variable").unwrap().path).unwrap();
        let font = FontRef::new(&vf).unwrap();
        assert_eq!(
            font.maxp().unwrap().num_glyphs(),
            97,
            "95 glyphs, .notdef and o.BRACKET.1"
        );
        let gsub = font.gsub().expect("the alternate is swapped in with GSUB");
        assert!(
            gsub.feature_variations().is_some(),
            "the swap depends on the weight (feature variations)"
        );

        // Static Bold (wght 700) uses the alternate; Regular doesn't.
        let style =
            |s: &str| std::fs::read(&r.files.iter().find(|x| x.style == s).unwrap().path).unwrap();
        assert_eq!(advance(&style("Regular"), 'o'), o_regular);
        assert_eq!(advance(&style("Bold"), 'o'), o_regular + 50);
        // An instance at the brace location picks up the brace layer's extra width.
        let mut f2 = f.clone();
        f2.instances.push(Instance {
            name: "Medium".into(),
            location: BTreeMap::from([("wght".into(), 550.0)]),
            export: true,
        });
        let out2 = tempfile::tempdir().unwrap();
        let r2 = export(&f2, &ExportOptions::ttf(false, true, out2.path())).unwrap();
        let medium =
            std::fs::read(&r2.files.iter().find(|x| x.style == "Medium").unwrap().path).unwrap();
        assert_eq!(advance(&medium, 'H'), 730 + 20);
    }

    #[test]
    fn otf_and_woff2() {
        use write_fonts::read::ps::cff::CffFontRef;
        use write_fonts::types::{GlyphId, Tag};
        let out = tempfile::tempdir().unwrap();
        let mut opts = ExportOptions::ttf(true, true, out.path());
        opts.otf = true;
        opts.woff2 = true;
        let r = export(&two_masters(), &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let find = |style: &str, format: &str| {
            let f = r
                .files
                .iter()
                .find(|x| x.style == style && x.format == format)
                .unwrap_or_else(|| panic!("no {style} {format}"));
            assert!(f.path.ends_with(format));
            std::fs::read(&f.path).unwrap()
        };
        // 2 statics × (ttf, otf, woff2) + variable × (ttf, otf, woff2)
        assert_eq!(r.files.len(), 9);

        // OTF: CFF outlines, same glyphs, widths and bounds as the TrueType font.
        let ttf_bytes = find("Bold", "ttf");
        let otf_bytes = find("Bold", "otf");
        let ttf = FontRef::new(&ttf_bytes).unwrap();
        let otf = FontRef::new(&otf_bytes).unwrap();
        assert_eq!(otf.table_directory().sfnt_version(), 0x4F54_544F, "OTTO");
        assert!(otf.table_data(Tag::new(b"glyf")).is_none());
        assert!(otf.gpos().is_ok(), "kerning is kept");
        let n = ttf.maxp().unwrap().num_glyphs();
        assert_eq!(otf.maxp().unwrap().num_glyphs(), n);
        let cff_data = otf.table_data(Tag::new(b"CFF ")).unwrap();
        let cff = CffFontRef::new(cff_data.as_bytes(), 0, None).unwrap();
        assert_eq!(cff.num_glyphs(), u32::from(n));
        let sub = cff.subfont(0, &[]).unwrap();
        let gid = ttf.cmap().unwrap().map_codepoint('H').unwrap();
        let width = cff.evaluate_width(&sub, gid, &[]).unwrap().unwrap();
        assert_eq!(width.to_f64(), 780.0, "Bold H");
        let b = cff.evaluate_extents(&sub, gid, &[]).unwrap().unwrap();
        let glyf = ttf.glyf().unwrap();
        let loca = ttf.loca(None).unwrap();
        let tg = loca.get_glyf(gid, &glyf).unwrap().unwrap();
        assert_eq!(
            (
                b.x_min.to_f64(),
                b.y_min.to_f64(),
                b.x_max.to_f64(),
                b.y_max.to_f64()
            ),
            (
                f64::from(tg.x_min()),
                f64::from(tg.y_min()),
                f64::from(tg.x_max()),
                f64::from(tg.y_max())
            ),
            "same outline bounds as the TrueType glyph"
        );
        let _ = GlyphId::NOTDEF;

        // WOFF2: header, and the compressed stream holds exactly the font's tables.
        let w = find("Bold", "woff2");
        assert_eq!(&w[..4], b"wOF2");
        assert_eq!(
            u32::from_be_bytes(w[8..12].try_into().unwrap()) as usize,
            w.len()
        );
        let num_tables = u16::from_be_bytes(w[12..14].try_into().unwrap()) as usize;
        let compressed_len = u32::from_be_bytes(w[20..24].try_into().unwrap()) as usize;
        assert_eq!(num_tables, ttf.table_directory().table_records().len());
        let dir_len = w.len() - 48 - compressed_len;
        let start = 48
            + (0..=3)
                .find(|pad| {
                    (dir_len - pad) > 0 && {
                        let d = dir_len - pad;
                        let mut out = Vec::new();
                        brotli::BrotliDecompress(&mut &w[48 + d..48 + d + compressed_len], &mut out)
                            .is_ok()
                    }
                })
                .map(|pad| dir_len - pad)
                .unwrap();
        let mut stream = Vec::new();
        brotli::BrotliDecompress(&mut &w[start..start + compressed_len], &mut stream).unwrap();
        let mut tables: Vec<_> = ttf
            .table_directory()
            .table_records()
            .iter()
            .map(|r| {
                (
                    r.tag(),
                    ttf.table_data(r.tag()).unwrap().as_bytes().to_vec(),
                )
            })
            .collect();
        tables.sort_by_key(|(t, _)| *t);
        let expected: Vec<u8> = tables.into_iter().flat_map(|(_, d)| d).collect();
        assert_eq!(stream, expected);
        let vf = find("Variable", "woff2");
        assert_eq!(&vf[..4], b"wOF2");
    }

    #[test]
    fn overlaps_and_metrics_keys_on_export() {
        let mut f = sample_font();
        let m = f.masters[0].id.clone();
        // T is a crossbar and a stem that overlap; n's left sidebearing follows H.
        let t = f.glyph("T").unwrap().layer(&m).unwrap().shapes.len();
        assert_eq!(t, 2, "the sample T is two overlapping bars");
        f.glyph_mut("n")
            .unwrap()
            .layer_mut(&m)
            .unwrap()
            .metrics_keys
            .left = Some("=H".into());
        let h_lsb =
            kernest_core::geometry::sidebearings(&f, f.glyph("H").unwrap().layer(&m).unwrap())
                .unwrap()
                .0;
        let out = tempfile::tempdir().unwrap();
        let mut opts = ExportOptions::ttf(false, true, out.path());
        opts.remove_overlaps = true;
        let r = export(&f, &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let glyf = font.glyf().unwrap();
        let loca = font.loca(None).unwrap();
        let cmap = font.cmap().unwrap();
        let contours = |c: char| {
            let gid = cmap.map_codepoint(c).unwrap();
            match loca.get_glyf(gid, &glyf).unwrap().unwrap() {
                write_fonts::read::tables::glyf::Glyph::Simple(s) => s.number_of_contours(),
                _ => panic!("expected a simple glyph"),
            }
        };
        assert_eq!(contours('T'), 1, "the bars were merged");
        assert_eq!(contours('H'), 1, "H's bars were merged too");
        let gid = cmap.map_codepoint('n').unwrap();
        let g = loca.get_glyf(gid, &glyf).unwrap().unwrap();
        assert_eq!(
            f64::from(g.x_min()),
            h_lsb.round(),
            "n's left sidebearing follows H"
        );
        // Without the option, T keeps both contours.
        let out2 = tempfile::tempdir().unwrap();
        let r2 = export(&f, &ExportOptions::ttf(false, true, out2.path())).unwrap();
        let b2 = std::fs::read(&r2.files[0].path).unwrap();
        let f2 = FontRef::new(&b2).unwrap();
        let gid = f2.cmap().unwrap().map_codepoint('T').unwrap();
        let g2 = f2
            .loca(None)
            .unwrap()
            .get_glyf(gid, &f2.glyf().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(g2.number_of_contours(), 2);
    }

    #[test]
    fn os2_weight_and_width_class() {
        let os2_weight = |bytes: &[u8]| {
            FontRef::new(bytes)
                .unwrap()
                .os2()
                .unwrap()
                .us_weight_class()
        };
        // With a weight axis: the location.
        let mut f = two_masters();
        f.instances.push(Instance {
            name: "Medium".into(),
            location: BTreeMap::from([("wght".into(), 550.0)]),
            export: true,
        });
        f.instances.push(Instance {
            name: "Bold".into(),
            location: BTreeMap::from([("wght".into(), 700.0)]),
            export: true,
        });
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let read = |style: &str| {
            std::fs::read(&r.files.iter().find(|x| x.style == style).unwrap().path).unwrap()
        };
        assert_eq!(os2_weight(&read("Medium")), 550);
        assert_eq!(os2_weight(&read("Bold")), 700);
        // Without one: the style name.
        let mut g = sample_font();
        g.masters[0].name = "Semi Bold".into();
        let out2 = tempfile::tempdir().unwrap();
        let r2 = export(&g, &ExportOptions::ttf(false, true, out2.path())).unwrap();
        assert_eq!(os2_weight(&std::fs::read(&r2.files[0].path).unwrap()), 600);
        // Width classes from a wdth axis.
        let mut h = sample_font();
        h.axes.push(Axis {
            name: "Width".into(),
            tag: "wdth".into(),
            min: 75.0,
            default: 100.0,
            max: 100.0,
            hidden: false,
        });
        let loc = BTreeMap::from([("wdth".to_string(), 75.0)]);
        assert_eq!(ufo::width_class(&h, &loc), Some(3));
        assert_eq!(ufo::width_class(&h, &BTreeMap::new()), Some(5));
    }

    #[test]
    fn variable_otf_cff2() {
        use kernest_core::{edit::translate_x, layers, AxisRange};
        use write_fonts::read::ps::cff::CffFontRef;
        use write_fonts::types::{F2Dot14, Tag};
        // Bold's H outlines move 50 units right; a brace layer at 550 moves 20 more;
        // o gets an alternate from 600 up.
        let mut f = two_masters();
        let bold = f.masters[1].id.clone();
        translate_x(f.glyph_mut("H").unwrap().layer_mut(&bold).unwrap(), 50.0);
        let (f, brace) =
            layers::add_brace_layer(&f, "H", "m01", &BTreeMap::from([("wght".into(), 550.0)]))
                .unwrap();
        let mut f = f;
        translate_x(f.glyph_mut("H").unwrap().layer_mut(&brace).unwrap(), 20.0);
        let ranges = [AxisRange {
            tag: "wght".into(),
            min: Some(600.0),
            max: None,
        }];
        let (f, _) = layers::add_bracket_layers(&f, "o", &bold, &ranges).unwrap();

        let out = tempfile::tempdir().unwrap();
        let mut opts = ExportOptions::ttf(true, false, out.path());
        opts.ttf = false;
        opts.otf = true;
        let r = export(&f, &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.files.len(), 1);
        assert!(r.files[0].path.ends_with("KernestSample-VF.otf"));
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        assert_eq!(font.table_directory().sfnt_version(), 0x4F54_544F, "OTTO");
        assert!(font.table_data(Tag::new(b"glyf")).is_none());
        assert!(font.table_data(Tag::new(b"gvar")).is_none());
        assert!(font.fvar().is_ok() && font.gsub().is_ok());
        let n = font.maxp().unwrap().num_glyphs();
        let data = font.table_data(Tag::new(b"CFF2")).unwrap();
        let cff = CffFontRef::new_cff2(data.as_bytes(), None).unwrap();
        assert_eq!(cff.num_glyphs(), u32::from(n));
        let gid = font.cmap().unwrap().map_codepoint('H').unwrap();
        let x_min = |coord: f32| {
            let coords = [F2Dot14::from_f32(coord)];
            let sub = cff.subfont(0, &coords).unwrap();
            cff.evaluate_extents(&sub, gid, &coords)
                .unwrap()
                .unwrap()
                .x_min
                .to_f64()
        };
        let x0 = f.glyph("H").unwrap().layer("m01").unwrap();
        let x0 = kernest_core::geometry::bounds(&f, x0).unwrap().min_x;
        assert_eq!(x_min(0.0), x0, "Regular");
        assert_eq!(x_min(1.0), x0 + 50.0, "Bold");
        assert_eq!(
            x_min(0.5),
            x0 + 25.0 + 20.0,
            "the brace layer at 550 is a source"
        );
        // Between sources, deltas are whole units (as in TrueType variable fonts).
        assert!(
            (x_min(0.25) - (x0 + 12.5 + 10.0)).abs() <= 1.0,
            "halfway to the brace layer"
        );
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
        let r = export(&f, &ExportOptions::ttf(true, true, out.path())).unwrap();
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
        let r = export(&sample_font(), &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert!(r.files.is_empty());
        assert!(r.problems[0].contains("two masters"));
    }
}
