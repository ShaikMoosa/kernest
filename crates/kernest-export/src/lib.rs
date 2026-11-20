//! Font export for Kernest: TrueType static fonts and variable fonts.
//!
//! Each master (or interpolated instance) is written as a UFO with norad, and
//! compiled with [fontc](https://github.com/googlefonts/fontc). Variable fonts use
//! a designspace of all masters. Static instances that sit between masters are
//! interpolated by Kernest first (same variation model as the editor's previews).

pub mod cff;
pub mod cff2;
mod cffhint;
mod charstring;
mod colr;
mod hint;
mod stat;
pub mod ufo;
pub mod woff2;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kernest_core::designspace::{normalize, Location};
use kernest_core::interpolate::{check_glyph, Interpolator};
use kernest_core::{structure, Font, Instance, KernPair, Layer, Master, Metrics};
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
    /// Add TrueType hinting instructions (TTF and WOFF2; OTF is unaffected), with the
    /// font's hinting settings.
    pub autohint: bool,
    /// Name glyphs in the compiled fonts by their production names.
    pub production_names: bool,
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
            autohint: false,
            production_names: false,
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
    /// Things worth a look in fonts that were built (metrics keys that couldn't be
    /// applied, glyphs that may not interpolate cleanly).
    pub warnings: Vec<String>,
}

pub fn export_preset(
    font: &Font,
    name: &str,
    source_dir: &Path,
    out_dir: Option<&Path>,
) -> Result<ExportReport, String> {
    use kernest_core::presets::{self, ExportFormat};
    presets::validate(&font.export_presets)?;
    let preset = font
        .export_presets
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| format!("There is no export preset named \"{name}\"."))?;
    let selected = presets::select_instances(font, &preset.instances)?;
    let destination = out_dir.map(Path::to_path_buf).unwrap_or_else(|| {
        source_dir.join(if preset.folder.is_empty() {
            "build"
        } else {
            &preset.folder
        })
    });
    let options = &preset.options;
    export(
        &selected,
        &ExportOptions {
            variable: options.variable,
            statics: options.statics,
            ttf: preset.formats.contains(&ExportFormat::Ttf),
            otf: preset.formats.contains(&ExportFormat::Otf),
            woff2: preset.formats.contains(&ExportFormat::Woff2),
            remove_overlaps: options.remove_overlaps,
            apply_metrics_keys: options.apply_metrics_keys,
            autohint: options.autohint,
            production_names: options.production_names,
            out_dir: destination,
        },
    )
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
    fontc::generate_font(source, fontc::Options::default()).map_err(|e| {
        // fontc's own message only counts FEA errors; the verbose compiler error
        // names the offending lines. Transparent wrappers hide it from source(),
        // so match the variants directly.
        if let fontc::Error::Backend(fontbe::error::Error::FeaCompileError(fea)) = &e {
            return format!("the font compiler failed: {}", fea.display_verbose());
        }
        format!("the font compiler failed: {}", e.display_with_causes())
    })
}

/// A quick TrueType compile of the default master (or an interpolated location) for
/// the text view's feature preview, with the generated and hand-written features and
/// the kerning baked in. Returns the bytes and the glyph order they were built with,
/// so a shaper's glyph ids can be mapped back to names.
pub fn preview_ttf(
    font: &Font,
    master_id: &str,
    location: Option<&Location>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let src = match location {
        Some(l) => instance_source(font, "Preview", l)?,
        None => {
            let m = font
                .master(master_id)
                .ok_or_else(|| format!("No master with id \"{master_id}\""))?;
            master_source(font, m, &m.name, true)
        }
    };
    // Mirrors to_ufo: .notdef is glyph id 0 even if the source order lists it later.
    let mut order: Vec<String> = vec![".notdef".into()];
    order.extend(
        src.layers
            .iter()
            .filter(|(n, _)| n != ".notdef")
            .map(|(n, _)| n.clone()),
    );
    let dir =
        tempfile::tempdir().map_err(|e| format!("could not create a temporary folder: {e}"))?;
    let path = dir.path().join("preview.ufo");
    let ufo = to_ufo(&src)?;
    ufo.save(&path)
        .map_err(|e| format!("could not write a UFO: {e}"))?;
    Ok((compile(&path)?, order))
}

/// The preview compile with the font's TrueType hinting applied, for the edit
/// view's pixel preview. Uses the same settings and mappings as a TTF export.
pub fn hint_preview(
    ttf: &[u8],
    font: &Font,
    location: Option<&Location>,
) -> Result<Vec<u8>, String> {
    let tmp =
        tempfile::tempdir().map_err(|e| format!("could not create a temporary folder: {e}"))?;
    hint::autohint(
        ttf,
        &font.info.hinting.unwrap_or_default(),
        font,
        location,
        tmp.path(),
        "preview",
    )
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
        instance: None,
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
            // A zero exception (a glyph on either side) still cancels group kerning.
            let exception = !left.starts_with('@') || !right.starts_with('@');
            (value != 0.0 || exception).then_some(KernPair { left, right, value })
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
        instance: None,
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
    // Smart components become outlines; the compiler only sees plain shapes.
    let flat = kernest_core::smart::flatten_font(font);
    // Components sit where their anchors say, so exported positions are current.
    let mut aligned = kernest_core::anchors::align_all(&flat);
    if !opts.production_names {
        for g in &mut aligned.glyphs {
            g.production_name = None;
        }
    }
    let font = &aligned;
    let keyed;
    let font = if opts.apply_metrics_keys {
        let (f, problems) = kernest_core::metricskeys::apply_all(font);
        report
            .warnings
            .extend(problems.into_iter().map(|p| format!("Metrics keys: {p}")));
        keyed = f;
        &keyed
    } else {
        font
    };
    // Stroked paths are drawn as the outlines they make.
    let stroked = kernest_core::stroke::expand_font(font);
    let font = &stroked;
    // Colour layers become ordinary glyphs; COLR and CPAL are added to each file.
    let (split, colored) = kernest_core::color::split_color_layers(font);
    let font = &split;
    let colorize = |report: &mut ExportReport,
                    label: &str,
                    ids_from: &[u8],
                    bytes: Vec<u8>,
                    variable: bool,
                    location: Option<&kernest_core::designspace::Location>|
     -> Vec<u8> {
        if colored.is_empty() {
            return bytes;
        }
        let with = colr::glyph_ids(ids_from)
            .map(|mut ids| {
                // Colour glyphs are looked up by source name; with production
                // names the compiled font carries those instead.
                for g in &font.glyphs {
                    if let Some(p) = &g.production_name {
                        if let Some(&id) = ids.get(p) {
                            ids.insert(g.name.clone(), id);
                        }
                    }
                }
                ids
            })
            .and_then(|ids| {
                if variable {
                    colr::variable_tables(font, &colored, &ids)
                } else if let Some(m) =
                    location.and_then(|loc| font.masters.iter().find(|m| &m.location == loc))
                {
                    colr::tables_at(font, &colored, &ids, &m.id)
                } else {
                    colr::tables(&colored, &font.palettes, &ids)
                }
            })
            .and_then(|(c, p)| colr::add_tables(&bytes, &c, &p));
        match with {
            Ok(b) => b,
            Err(e) => {
                report
                    .problems
                    .push(format!("{label}: {e}; exported without colour."));
                bytes
            }
        }
    };
    let hinting = font.info.hinting.unwrap_or_default();
    // The TrueType bytes for TTF and WOFF2: hinted if asked (a failure is reported and
    // the unhinted font is used).
    let truetype = |report: &mut ExportReport,
                    label: &str,
                    name: &str,
                    bytes: &[u8],
                    location: Option<&Location>|
     -> Vec<u8> {
        if !opts.autohint || !(opts.ttf || opts.woff2) {
            return bytes.to_vec();
        }
        match hint::autohint(bytes, &hinting, font, location, tmp.path(), name) {
            Ok(h) => h,
            Err(e) => {
                report
                    .problems
                    .push(format!("{label}: {e}; exported without hinting."));
                bytes.to_vec()
            }
        }
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

    if opts.variable && font.masters.len() > 1 {
        let warned: Vec<&str> = font
            .glyphs
            .iter()
            .filter(|g| {
                g.export && kernest_core::interpolation_quality::has_warnings(font, &g.name)
            })
            .map(|g| g.name.as_str())
            .collect();
        if !warned.is_empty() {
            let shown: Vec<&str> = warned.iter().take(8).copied().collect();
            report.warnings.push(format!(
                "{} glyph{} may not interpolate cleanly ({}{}). See Interpolation warnings in Font View.",
                warned.len(),
                if warned.len() == 1 { "" } else { "s" },
                shown.join(", "),
                if warned.len() > 8 { ", \u{2026}" } else { "" }
            ));
        }
    }
    if opts.variable {
        let emit_variable = |report: &mut ExportReport,
                             stat_font: &Font,
                             label: &str,
                             file_base: &str,
                             bytes: Vec<u8>|
         -> Result<(), String> {
            let bytes = match stat::add_stat(stat_font, &bytes) {
                Ok(b) => b,
                Err(e) => {
                    report.problems.push(format!("{label} (STAT): {e}"));
                    bytes
                }
            };
            let tt = truetype(report, label, file_base, &bytes, None);
            let tt = colorize(report, label, &bytes, tt, true, None);
            if opts.ttf {
                emit(
                    report,
                    "variable",
                    "ttf",
                    label,
                    format!("{file_base}.ttf"),
                    &tt,
                )?;
            }
            if opts.woff2 {
                match woff2::to_woff2(&tt) {
                    Ok(w) => emit(
                        report,
                        "variable",
                        "woff2",
                        label,
                        format!("{file_base}.woff2"),
                        &w,
                    )?,
                    Err(e) => report.problems.push(format!("{label} (WOFF2): {e}")),
                }
            }
            if opts.otf {
                match variable_otf(stat_font, &bytes).map(|o| {
                    let o = colorize(report, &format!("{label} (OTF)"), &bytes, o, true, None);
                    match stat::add_stat(stat_font, &o) {
                        Ok(s) => s,
                        Err(e) => {
                            report.problems.push(format!("{label} (OTF, STAT): {e}"));
                            o
                        }
                    }
                }) {
                    Ok(o) => emit(
                        report,
                        "variable",
                        "otf",
                        label,
                        format!("{file_base}.otf"),
                        &o,
                    )?,
                    Err(e) => report.problems.push(format!("{label} (OTF): {e}")),
                }
            }
            Ok(())
        };
        if font.variable_fonts.is_empty() {
            match variable(font, tmp.path()) {
                Ok(bytes) => emit_variable(
                    &mut report,
                    font,
                    "Variable",
                    &format!("{family}-VF"),
                    bytes,
                )?,
                Err(e) => report.problems.push(format!("Variable font: {e}")),
            }
        } else {
            for (i, vf) in font.variable_fonts.iter().enumerate() {
                let dir = tmp.path().join(format!("vf{i}"));
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
                let reduced = match kernest_core::designspace::reduce_for_variable(font, vf) {
                    Ok(r) => r,
                    Err(e) => {
                        report.problems.push(format!("{}: {e}", vf.name));
                        continue;
                    }
                };
                match variable(&reduced, &dir) {
                    Ok(bytes) => {
                        emit_variable(&mut report, &reduced, &vf.name, &vf.file_name, bytes)?
                    }
                    Err(e) => report.problems.push(format!("{}: {e}", vf.name)),
                }
            }
        }
    }

    if opts.statics {
        let targets: Vec<(String, Location, Option<&Instance>)> =
            if font.instances.iter().any(|i| i.export) {
                font.instances
                    .iter()
                    .filter(|i| i.export)
                    .map(|i| (i.name.clone(), i.location.clone(), Some(i)))
                    .collect()
            } else {
                font.masters
                    .iter()
                    .map(|m| (m.name.clone(), m.location.clone(), None))
                    .collect()
            };
        for (i, (style, location, inst)) in targets.iter().enumerate() {
            let built = instance_source(font, style, location).and_then(|mut src| {
                src.instance = *inst;
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
                    let tt = truetype(
                        &mut report,
                        style,
                        &format!("instance{i}"),
                        &bytes,
                        Some(location),
                    );
                    let tt = colorize(&mut report, style, &bytes, tt, false, Some(location));
                    if opts.ttf {
                        emit(
                            &mut report,
                            "static",
                            "ttf",
                            style,
                            format!("{stem}.ttf"),
                            &tt,
                        )?;
                    }
                    if opts.otf {
                        match cff::ttf_to_otf(&bytes, &ufo, &font.info.family_name, style).map(
                            |o| {
                                colorize(
                                    &mut report,
                                    &format!("{style} (OTF)"),
                                    &bytes,
                                    o,
                                    false,
                                    Some(location),
                                )
                            },
                        ) {
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
                        match woff2::to_woff2(&tt) {
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

/// Options for exporting editable UFO sources (File › Export › UFO…).
#[derive(Debug, Clone)]
pub struct UfoExportOptions {
    /// Master ids to include; empty means every master.
    pub masters: Vec<String>,
    /// Flatten smart components to outlines; otherwise they stay plain components.
    pub decompose_smart: bool,
    /// Write colour layers as named UFO layers instead of leaving them out.
    pub color_layers: bool,
}

/// The UFO folders and designspace written, with anything the format can't carry.
#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UfoExportReport {
    pub files: Vec<String>,
    pub problems: Vec<String>,
}

/// Writes the chosen masters as UFOs plus a designspace, for editing in other tools.
/// Unlike [`write_sources`] this honours the user's master selection and can keep
/// colour layers as UFO layers; smart components either stay components or flatten.
pub fn export_ufo(
    font: &Font,
    dir: &Path,
    opts: &UfoExportOptions,
) -> Result<UfoExportReport, String> {
    use std::collections::HashSet;
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    // UFO has no strokes: stroked paths are written as the outlines they draw.
    let mut work = kernest_core::stroke::expand_font(&if opts.decompose_smart {
        kernest_core::smart::flatten_font(font)
    } else {
        font.clone()
    });
    let selected: Vec<Master> = if opts.masters.is_empty() {
        work.masters.clone()
    } else {
        work.masters
            .iter()
            .filter(|m| opts.masters.contains(&m.id))
            .cloned()
            .collect()
    };
    if selected.is_empty() {
        return Err("Choose at least one master to export.".into());
    }
    let ids: HashSet<String> = selected.iter().map(|m| m.id.clone()).collect();
    work.masters = selected;
    for g in &mut work.glyphs {
        g.layers.retain(|l| ids.contains(l.master_id.as_str()));
    }
    work.kerning.retain(|k, _| ids.contains(k.as_str()));

    let mut report = UfoExportReport::default();
    if !opts.decompose_smart && work.glyphs.iter().any(|g| g.is_smart()) {
        report.problems.push(
            "Smart components export as plain components; UFO doesn't store smart interpolation."
                .into(),
        );
    }
    if !opts.color_layers
        && work
            .glyphs
            .iter()
            .any(|g| g.layers.iter().any(|l| l.color.is_some()))
    {
        report
            .problems
            .push("Colour layers were left out. Turn on \"Colour layers\" to keep them.".into());
    }

    let mut rules = Vec::new();
    for g in &work.glyphs {
        for (n, ranges) in g.bracket_groups().into_iter().enumerate() {
            rules.push(DsRule {
                glyph: g.name.clone(),
                with: alternate_name(&g.name, n),
                ranges,
            });
        }
    }
    let mut braces: Vec<(Location, Vec<(String, Layer)>)> = Vec::new();
    for g in &work.glyphs {
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
    let default_id = kernest_core::interpolate::default_master(&work).id.clone();
    let mut sources = Vec::new();
    for (i, m) in work.masters.iter().enumerate() {
        let file = format!("master{i}.ufo");
        let mut src = master_source(&work, m, &m.name, false);
        for r in &rules {
            let g = work.glyph(&r.glyph).expect("rule glyphs exist");
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
        if opts.color_layers {
            let colored: Vec<(&kernest_core::Glyph, Vec<&Layer>)> = work
                .glyphs
                .iter()
                .map(|g| {
                    (
                        g,
                        g.layers
                            .iter()
                            .filter(|l| l.master_id == m.id && l.color.is_some())
                            .collect::<Vec<&Layer>>(),
                    )
                })
                .filter(|(_, cls)| !cls.is_empty())
                .collect();
            let max = colored.iter().map(|(_, cls)| cls.len()).max().unwrap_or(0);
            for k in 0..max {
                let layer_name = format!("color{}", k + 1);
                let layer = ufo
                    .layers
                    .new_layer(&layer_name)
                    .map_err(|e| format!("could not add a UFO layer: {e}"))?;
                for (g, cls) in &colored {
                    if let Some(l) = cls.get(k) {
                        layer.insert_glyph(ufo::glyph(&work, &g.name, l)?);
                    }
                }
            }
        }
        if m.id == default_id {
            for (k, (loc, glyphs)) in braces.iter().enumerate() {
                let layer_name = format!("brace{}", k + 1);
                let layer = ufo
                    .layers
                    .new_layer(&layer_name)
                    .map_err(|e| format!("could not add a UFO layer: {e}"))?;
                for (gname, l) in glyphs {
                    layer.insert_glyph(ufo::glyph(&work, gname, l)?);
                }
                sources.push(DsSource {
                    file: file.clone(),
                    style: format!("{} {{{}}}", m.name, k + 1),
                    location: loc.clone(),
                    layer: Some(layer_name),
                });
            }
        }
        let path = dir.join(&file);
        ufo.save(&path)
            .map_err(|e| format!("could not write a UFO: {e}"))?;
        report.files.push(path.display().to_string());
        sources.push(DsSource {
            file,
            style: m.name.clone(),
            location: m.location.clone(),
            layer: None,
        });
    }
    let ds = dir.join("font.designspace");
    std::fs::write(&ds, designspace(&work, &sources, &rules))
        .map_err(|e| format!("could not write the designspace: {e}"))?;
    report.files.push(ds.display().to_string());
    Ok(report)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use kernest_core::sample::sample_font;
    use kernest_core::{structure, Axis, Instance, NodeKind, Shape};
    use write_fonts::read::{FontRef, TableProvider};

    fn advance(bytes: &[u8], ch: char) -> u16 {
        let f = FontRef::new(bytes).expect("a valid font");
        let gid = f.cmap().unwrap().map_codepoint(ch).expect("mapped");
        f.hmtx().unwrap().advance(gid).expect("advance")
    }

    #[test]
    fn stroked_paths_export_in_static_and_variable_fonts() {
        use kernest_core::stroke::{PathStroke, StrokeCap, StrokeJoin};
        let mut f = two_masters();
        // Stroke H's first path, thin in one master and heavy in the other.
        for (key, w) in [("m01", 20.0), ("m02", 90.0)] {
            if let Some(Shape::Path(p)) = f
                .glyph_mut("H")
                .unwrap()
                .layer_mut(key)
                .unwrap()
                .shapes
                .first_mut()
            {
                p.stroke = Some(PathStroke {
                    width: w,
                    join: StrokeJoin::Miter,
                    end_cap: StrokeCap::Round,
                    ..Default::default()
                });
            }
        }
        assert!(kernest_core::interpolate::check_glyph(&f, "H").is_empty());
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert!(r.files.iter().any(|x| x.kind == "variable"));
    }

    fn two_masters() -> Font {
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
        let f = structure::set_axes(&sample_font(), vec![wght]).unwrap();
        let (mut f, bold) =
            structure::add_master(&f, "m01", "Bold", &BTreeMap::from([("wght".into(), 700.0)]))
                .unwrap();
        let h = f.glyph_mut("H").unwrap().layer_mut(&bold).unwrap();
        h.width += 100.0;
        f
    }

    #[test]
    fn preset_exports_only_selected_instances_and_honours_destination() {
        let mut f = two_masters();
        f.instances = [400.0, 700.0]
            .into_iter()
            .map(|weight| Instance {
                name: if weight == 400.0 { "Regular" } else { "Bold" }.into(),
                location: BTreeMap::from([("wght".into(), weight)]),
                export: false,
                weight_class: None,
                width_class: None,
                style_link: None,
            })
            .collect();
        f.export_presets = vec![kernest_core::presets::ExportPreset {
            name: "proof".into(),
            formats: vec![kernest_core::presets::ExportFormat::Ttf],
            instances: vec!["Bold".into()],
            options: kernest_core::presets::ExportSettings {
                autohint: false,
                ..Default::default()
            },
            folder: "build/proof".into(),
        }];
        let dir = tempfile::tempdir().unwrap();
        let report = export_preset(&f, "proof", dir.path(), None).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.files.len(), 1);
        assert_eq!(
            Path::new(&report.files[0].path).parent().unwrap(),
            dir.path().join("build/proof")
        );
        assert_eq!(report.files[0].style, "Bold");
        assert_eq!(
            advance(&std::fs::read(&report.files[0].path).unwrap(), 'H'),
            780
        );
        let override_dir = dir.path().join("override");
        let report = export_preset(&f, "proof", dir.path(), Some(&override_dir)).unwrap();
        assert_eq!(
            Path::new(&report.files[0].path).parent().unwrap(),
            override_dir
        );
        assert!(!f.instances[1].export);
        f.instances.clear();
        assert!(export_preset(&f, "proof", dir.path(), None).is_err());
        assert!(export_preset(&f, "missing", dir.path(), None).is_err());
    }

    #[test]
    fn ufo_export_writes_masters_and_designspace() {
        let f = two_masters();
        let dir = tempfile::tempdir().unwrap();
        let opts = UfoExportOptions {
            masters: vec![],
            decompose_smart: false,
            color_layers: false,
        };
        let all = export_ufo(&f, dir.path(), &opts).unwrap();
        assert_eq!(all.files.len(), 3);
        assert!(dir.path().join("master0.ufo").is_dir());
        assert!(dir.path().join("master1.ufo").is_dir());
        assert!(dir.path().join("font.designspace").is_file());
        let one = export_ufo(
            &f,
            dir.path(),
            &UfoExportOptions {
                masters: vec!["m01".into()],
                ..opts.clone()
            },
        )
        .unwrap();
        assert_eq!(one.files.len(), 2);
        let ufo = norad::Font::load(dir.path().join("master0.ufo")).unwrap();
        assert!(ufo.get_glyph("H").is_some());
    }

    #[test]
    fn ufo_export_keeps_colour_layers() {
        use kernest_core::color::{ColorInfo, Paint};
        let mut f = sample_font();
        let h = f.glyph_mut("H").unwrap();
        let mut cl = Layer::new("m01", 600.0);
        cl.id = Some("m01~1".into());
        cl.color = Some(ColorInfo {
            paint: Paint::Solid {
                color: 0,
                alpha: 1.0,
            },
        });
        h.layers.push(cl);
        let dir = tempfile::tempdir().unwrap();
        let with = export_ufo(
            &f,
            dir.path(),
            &UfoExportOptions {
                masters: vec![],
                decompose_smart: false,
                color_layers: true,
            },
        )
        .unwrap();
        assert!(with.problems.is_empty());
        let ufo = norad::Font::load(dir.path().join("master0.ufo")).unwrap();
        let color1 = ufo.layers.get("color1").expect("colour layer written");
        assert!(color1.get_glyph("H").is_some());
        let without = export_ufo(
            &f,
            dir.path(),
            &UfoExportOptions {
                masters: vec![],
                decompose_smart: false,
                color_layers: false,
            },
        )
        .unwrap();
        assert!(!without.problems.is_empty());
    }

    #[test]
    fn production_names_reach_ufo_and_compiled_fonts() {
        let mut f = two_masters();
        f.glyph_mut("H").unwrap().production_name = Some("Hprod".into());
        // UFO sources always carry the map, under the source names.
        let dir = tempfile::tempdir().unwrap();
        export_ufo(
            &f,
            dir.path(),
            &UfoExportOptions {
                masters: vec![],
                decompose_smart: false,
                color_layers: false,
            },
        )
        .unwrap();
        let ufo = norad::Font::load(dir.path().join("master0.ufo")).unwrap();
        let map = ufo
            .lib
            .get("public.postscriptNames")
            .and_then(|v| v.as_dictionary())
            .expect("postscriptNames map");
        assert_eq!(map.get("H").and_then(|v| v.as_string()), Some("Hprod"));
        assert!(ufo.get_glyph("H").is_some());
        assert!(ufo.get_glyph("Hprod").is_none());

        let post_names = |bytes: &[u8]| -> Vec<String> {
            let font = FontRef::new(bytes).unwrap();
            let post = font.post().unwrap();
            (0..font.maxp().unwrap().num_glyphs())
                .map(|g| {
                    post.glyph_name(write_fonts::types::GlyphId16::new(g))
                        .unwrap()
                        .to_string()
                })
                .collect()
        };
        // Compiled fonts (variable TTF/OTF plus one static TTF/OTF per master):
        // the option decides which names the post table carries.
        let out = tempfile::tempdir().unwrap();
        let mut opts = ExportOptions::ttf(true, true, out.path());
        opts.otf = true;
        let plain = export(&f, &opts).unwrap();
        assert!(plain.problems.is_empty(), "{:?}", plain.problems);
        assert_eq!(plain.files.len(), 6);
        for file in &plain.files {
            let names = post_names(&std::fs::read(&file.path).unwrap());
            assert!(names.contains(&"H".to_string()), "{}: {names:?}", file.path);
            assert!(!names.contains(&"Hprod".to_string()), "{}", file.path);
        }
        opts.production_names = true;
        let prod = export(&f, &opts).unwrap();
        assert!(prod.problems.is_empty(), "{:?}", prod.problems);
        assert_eq!(prod.files.len(), plain.files.len());
        for file in &prod.files {
            let names = post_names(&std::fs::read(&file.path).unwrap());
            assert!(
                names.contains(&"Hprod".to_string()),
                "{}: {names:?}",
                file.path
            );
            assert!(!names.contains(&"H".to_string()), "{}", file.path);
        }
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

    fn ufo_glyph_order(ufo: &norad::Font) -> Vec<String> {
        ufo.lib
            .get("public.glyphOrder")
            .and_then(|v| v.as_array())
            .expect("glyphOrder array")
            .iter()
            .map(|v| v.as_string().unwrap().to_string())
            .collect()
    }

    #[test]
    fn custom_glyph_order_reaches_ufo_and_preview() {
        let f = sample_font();
        let order: Vec<String> = f.glyphs.iter().map(|g| g.name.clone()).rev().collect();
        let f = kernest_core::order::set_order(&f, &order).unwrap();
        let m = &f.masters[0];
        let ufo = to_ufo(&master_source(&f, m, &m.name, true)).unwrap();
        let want: Vec<String> = std::iter::once(".notdef".to_string())
            .chain(order.iter().cloned())
            .collect();
        assert_eq!(ufo_glyph_order(&ufo), want);
        let (bytes, prev) = preview_ttf(&f, &m.id, None).unwrap();
        assert_eq!(prev, want);
        let font = FontRef::new(&bytes).unwrap();
        let post = font.post().unwrap();
        let names: Vec<String> = (0..font.maxp().unwrap().num_glyphs())
            .map(|g| {
                post.glyph_name(write_fonts::types::GlyphId16::new(g))
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names, want,
            "the compiled font's glyph ids follow the order"
        );
    }

    #[test]
    fn notdef_mid_list_still_exports_first() {
        let mut f = sample_font();
        let mut nd = f.glyph("H").unwrap().clone();
        nd.name = ".notdef".into();
        nd.unicodes.clear();
        f.glyphs.push(nd);
        let last = f.glyphs.len() - 1;
        f.glyphs.swap(3, last);
        let m = &f.masters[0];
        let ufo = to_ufo(&master_source(&f, m, &m.name, true)).unwrap();
        let got = ufo_glyph_order(&ufo);
        assert_eq!(got[0], ".notdef");
        assert_eq!(got.len(), f.glyphs.len());
        let (_, prev) = preview_ttf(&f, &m.id, None).unwrap();
        assert_eq!(prev, got);
    }

    #[test]
    fn variable_and_interpolated_instance() {
        let mut f = two_masters();
        f.instances.push(Instance {
            name: "Medium".into(),
            location: BTreeMap::from([("wght".into(), 550.0)]),
            export: true,
            weight_class: None,
            width_class: None,
            style_link: None,
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
    fn stat_axis_value_labels_reach_the_font() {
        use kernest_core::AxisValue;
        use write_fonts::read::tables::stat::{AxisValue as StatValue, AxisValueTableFlags};
        use write_fonts::types::NameId;
        let mut f = two_masters();
        f.axes[0].labels = vec![
            AxisValue {
                name: "Regular".into(),
                value: 400.0,
                min: None,
                max: None,
                elidable: true,
                linked_value: Some(700.0),
            },
            AxisValue {
                name: "Bold".into(),
                value: 700.0,
                min: None,
                max: None,
                elidable: false,
                linked_value: None,
            },
            AxisValue {
                name: "Semibold".into(),
                value: 600.0,
                min: Some(550.0),
                max: Some(650.0),
                elidable: false,
                linked_value: None,
            },
        ];
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let vf = r.files.iter().find(|x| x.kind == "variable").unwrap();
        let bytes = std::fs::read(&vf.path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let stat = font.stat().expect("a STAT table");
        let axes = stat.design_axes().unwrap();
        assert_eq!(axes.len(), 1);
        assert_eq!(axes[0].axis_tag().to_string(), "wght");
        assert_eq!(stat.elided_fallback_name_id(), Some(NameId::SUBFAMILY_NAME));
        let values = stat.offset_to_axis_values().unwrap().unwrap();
        let names = name_strings(&bytes);
        let name_of = |id: NameId| names[&u16::from_be_bytes(id.to_be_bytes())].clone();
        let collected: Vec<_> = values.axis_values().iter().filter_map(|v| v.ok()).collect();
        assert_eq!(collected.len(), 3);
        match &collected[0] {
            StatValue::Format3(t) => {
                assert_eq!(name_of(t.value_name_id()), "Regular");
                assert_eq!(t.value().to_f64(), 400.0);
                assert_eq!(t.linked_value().to_f64(), 700.0);
                assert!(t
                    .flags()
                    .contains(AxisValueTableFlags::ELIDABLE_AXIS_VALUE_NAME));
            }
            _ => panic!("expected a format 3 axis value for Regular"),
        }
        match &collected[1] {
            StatValue::Format1(t) => {
                assert_eq!(name_of(t.value_name_id()), "Bold");
                assert_eq!(t.value().to_f64(), 700.0);
                assert!(!t
                    .flags()
                    .contains(AxisValueTableFlags::ELIDABLE_AXIS_VALUE_NAME));
            }
            _ => panic!("expected a format 1 axis value for Bold"),
        }
        match &collected[2] {
            StatValue::Format2(t) => {
                assert_eq!(name_of(t.value_name_id()), "Semibold");
                assert_eq!(t.nominal_value().to_f64(), 600.0);
                assert_eq!(t.range_min_value().to_f64(), 550.0);
                assert_eq!(t.range_max_value().to_f64(), 650.0);
            }
            _ => panic!("expected a format 2 axis value for Semibold"),
        }
    }

    #[test]
    fn two_variable_fonts_from_one_source() {
        use kernest_core::{VariableAxisSubset, VariableFontSetting};
        let f = two_masters();
        let f = structure::set_variable_fonts(
            &f,
            vec![
                VariableFontSetting {
                    name: "Light".into(),
                    file_name: "Sample-Light".into(),
                    axes: vec![VariableAxisSubset {
                        tag: "wght".into(),
                        min: Some(400.0),
                        max: Some(500.0),
                    }],
                    glyphs: Vec::new(),
                },
                VariableFontSetting {
                    name: "Bold".into(),
                    file_name: "Sample-Bold".into(),
                    axes: vec![VariableAxisSubset {
                        tag: "wght".into(),
                        min: Some(500.0),
                        max: Some(700.0),
                    }],
                    glyphs: Vec::new(),
                },
            ],
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let vfs: Vec<_> = r.files.iter().filter(|x| x.kind == "variable").collect();
        assert_eq!(vfs.len(), 2, "one variable font per setting");
        let range_of = |file: &str| {
            let vf = vfs.iter().find(|x| x.path.ends_with(file)).unwrap();
            let bytes = std::fs::read(&vf.path).unwrap();
            let font = FontRef::new(&bytes).unwrap();
            assert!(font.gvar().is_ok());
            let axes = font.fvar().unwrap().axes().unwrap();
            assert_eq!(axes.len(), 1);
            assert_eq!(axes[0].axis_tag().to_string(), "wght");
            (
                axes[0].min_value().to_f64(),
                axes[0].max_value().to_f64(),
                advance(&bytes, 'H'),
            )
        };
        // The Light cut keeps the original default (400); H is 680 there.
        assert_eq!(range_of("Sample-Light.ttf"), (400.0, 500.0, 680));
        // The Bold cut moves the default to 500 and interpolates a master there:
        // H is 680 at 400 and 780 at 700, so 713 at 500.
        assert_eq!(range_of("Sample-Bold.ttf"), (500.0, 700.0, 713));
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
            weight_class: None,
            width_class: None,
            style_link: None,
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
        // The WOFF2 directory places loca immediately after glyf, so the compressed
        // stream follows that order rather than pure tag order.
        if let Some(g) = tables.iter().position(|(t, _)| *t == Tag::new(b"glyf")) {
            if let Some(l) = tables.iter().position(|(t, _)| *t == Tag::new(b"loca")) {
                if l != g + 1 {
                    let loca = tables.remove(l);
                    tables.insert(g + 1, loca);
                }
            }
        }
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
            weight_class: None,
            width_class: None,
            style_link: None,
        });
        f.instances.push(Instance {
            name: "Bold".into(),
            location: BTreeMap::from([("wght".into(), 700.0)]),
            export: true,
            weight_class: None,
            width_class: None,
            style_link: None,
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
            map: Vec::new(),
            labels: Vec::new(),
        });
        let loc = BTreeMap::from([("wdth".to_string(), 75.0)]);
        assert_eq!(ufo::width_class(&h, &loc), Some(3));
        assert_eq!(ufo::width_class(&h, &BTreeMap::new()), Some(5));
    }

    fn name_strings(bytes: &[u8]) -> BTreeMap<u16, String> {
        let f = FontRef::new(bytes).unwrap();
        let name = f.name().unwrap();
        let mut out = BTreeMap::new();
        for rec in name.name_record() {
            let s = rec
                .string(name.string_data())
                .map(|s| s.to_string())
                .unwrap_or_default();
            out.entry(u16::from_be_bytes(rec.name_id().to_be_bytes()))
                .or_insert(s);
        }
        out
    }

    #[test]
    fn naming_fields_reach_the_name_table() {
        let mut f = sample_font();
        f.info.copyright = Some("(c) 2026 Ada".into());
        f.info.designer = Some("Ada Lovelace".into());
        f.info.designer_url = Some("https://ada.example".into());
        f.info.manufacturer = Some("Analytical Foundry".into());
        f.info.manufacturer_url = Some("https://foundry.example".into());
        f.info.license = Some("SIL Open Font License 1.1".into());
        f.info.license_url = Some("https://scripts.sil.org/OFL".into());
        f.info.description = Some("A face for testing names".into());
        f.info.trademark = Some("Analytical is a trademark".into());
        f.info.sample_text = Some("Hamburgefonstiv".into());
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let names = name_strings(&std::fs::read(&r.files[0].path).unwrap());
        for (id, expected) in [
            (0, "(c) 2026 Ada"),
            (7, "Analytical is a trademark"),
            (8, "Analytical Foundry"),
            (9, "Ada Lovelace"),
            (10, "A face for testing names"),
            (11, "https://foundry.example"),
            (12, "https://ada.example"),
            (13, "SIL Open Font License 1.1"),
            (14, "https://scripts.sil.org/OFL"),
            (19, "Hamburgefonstiv"),
        ] {
            assert_eq!(
                names.get(&id).map(String::as_str),
                Some(expected),
                "name id {id}"
            );
        }
    }

    #[test]
    fn instance_class_overrides_and_style_map() {
        let mut f = two_masters();
        f.instances.push(Instance {
            name: "Medium".into(),
            location: BTreeMap::from([("wght".into(), 550.0)]),
            export: true,
            weight_class: Some(560),
            width_class: Some(4),
            style_link: None,
        });
        f.instances.push(Instance {
            name: "Bold".into(),
            location: BTreeMap::from([("wght".into(), 700.0)]),
            export: true,
            weight_class: None,
            width_class: None,
            style_link: Some(kernest_core::StyleLink {
                bold: true,
                italic: false,
                base: "Regular".into(),
            }),
        });
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let read = |style: &str| {
            std::fs::read(&r.files.iter().find(|x| x.style == style).unwrap().path).unwrap()
        };
        let medium_bytes = read("Medium");
        let medium = FontRef::new(&medium_bytes).unwrap();
        assert_eq!(medium.os2().unwrap().us_weight_class(), 560);
        assert_eq!(medium.os2().unwrap().us_width_class(), 4);
        let bold = read("Bold");
        let names = name_strings(&bold);
        let family = f.info.family_name.clone();
        assert_eq!(names.get(&1).map(String::as_str), Some(family.as_str()));
        assert_eq!(names.get(&2).map(String::as_str), Some("Bold"));
        // fontc omits 16/17 when they would repeat 1/2; when present they must agree.
        if let Some(n16) = names.get(&16) {
            assert_eq!(n16, &family);
        }
        if let Some(n17) = names.get(&17) {
            assert_eq!(n17, "Bold");
        }
        // Linking to a non-Regular base groups the styles under that base's family.
        f.instances[1].style_link = Some(kernest_core::StyleLink {
            bold: true,
            italic: false,
            base: "Book".into(),
        });
        let out2 = tempfile::tempdir().unwrap();
        let r2 = export(&f, &ExportOptions::ttf(false, true, out2.path())).unwrap();
        let bold2 =
            std::fs::read(&r2.files.iter().find(|x| x.style == "Bold").unwrap().path).unwrap();
        let names2 = name_strings(&bold2);
        assert_eq!(
            names2.get(&1).map(String::as_str),
            Some(format!("{family} Book").as_str())
        );
        assert_eq!(names2.get(&2).map(String::as_str), Some("Bold"));
    }

    fn feature_font() -> Font {
        let mut f = Font::new("Feat", [0x61, 0x66, 0x69]);
        let mid = f.masters[0].id.clone();
        for n in ["a.ss01", "a.alt", "f_i"] {
            let mut g = kernest_core::Glyph::new(n, vec![]);
            g.layers.push(Layer::new(&mid, 600.0));
            f.glyphs.push(g);
        }
        f
    }

    fn gsub_tags(bytes: &[u8]) -> Vec<String> {
        let f = FontRef::new(bytes).unwrap();
        let gsub = f.gsub().unwrap();
        let mut tags: Vec<String> = gsub
            .feature_list()
            .unwrap()
            .feature_records()
            .iter()
            .map(|r| String::from_utf8_lossy(&r.feature_tag().to_be_bytes()).into_owned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    #[test]
    fn generated_features_compile_into_gsub() {
        let mut f = feature_font();
        f.feature_settings.automatic = kernest_core::features::available(&f);
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let tags = gsub_tags(&std::fs::read(&r.files[0].path).unwrap());
        for t in ["ss01", "liga", "salt", "aalt"] {
            assert!(tags.contains(&t.to_string()), "{tags:?}");
        }
    }

    #[test]
    fn hand_written_and_generated_features_compile_together() {
        let mut f = feature_font();
        f.feature_settings.automatic = kernest_core::features::available(&f);
        // ss01 is written by hand, so generation must leave it out; the rest still
        // compile alongside it without a duplicate-feature error.
        f.features = "feature ss01 {\n  sub a by a.alt;\n} ss01;\n".into();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let tags = gsub_tags(&std::fs::read(&r.files[0].path).unwrap());
        assert!(
            tags.contains(&"ss01".to_string()) && tags.contains(&"liga".to_string()),
            "{tags:?}"
        );
    }

    #[test]
    fn notes_are_saved_but_never_exported() {
        let mut f = sample_font();
        f.notes = "private remark about the ogonek".into();
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("x.kernest");
        kernest_core::package::save(&f, &pkg).unwrap();
        assert_eq!(kernest_core::package::open(&pkg).unwrap().notes, f.notes);
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        assert!(
            !bytes
                .windows(f.notes.len())
                .any(|w| w == f.notes.as_bytes()),
            "notes leaked into the exported font"
        );
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

    /// The sample font with a drawn acute and accented O / o made of components, as
    /// imported fonts usually have.
    pub(crate) fn with_composites(mut f: Font) -> Font {
        use kernest_core::glyphset::{add_glyphs, parse_glyph_list};
        let list =
            parse_glyph_list("acutecomb gravecomb Oacute Ograve oacute ograve Oslash.alt").unwrap();
        f = add_glyphs(&f, &list).unwrap().font;
        let ids: Vec<String> = f.masters.iter().map(|m| m.id.clone()).collect();
        for id in &ids {
            let mark = |x0: f64, x1: f64| {
                Shape::Path(kernest_core::Path {
                    locked: false,
                    stroke: None,
                    closed: true,
                    nodes: [
                        (x0, 720.0),
                        (x0 + 40.0, 720.0),
                        (x1, 800.0),
                        (x1 - 40.0, 800.0),
                    ]
                    .into_iter()
                    .map(|(x, y)| kernest_core::Node::new(x, y, NodeKind::Line))
                    .collect(),
                })
            };
            f.glyph_mut("acutecomb")
                .unwrap()
                .layer_mut(id)
                .unwrap()
                .shapes = vec![mark(250.0, 330.0)];
            f.glyph_mut("gravecomb")
                .unwrap()
                .layer_mut(id)
                .unwrap()
                .shapes = vec![mark(330.0, 250.0)];
            for (g, base, mark) in [
                ("Oacute", "O", "acutecomb"),
                ("Ograve", "O", "gravecomb"),
                ("oacute", "o", "acutecomb"),
                ("ograve", "o", "gravecomb"),
                ("Oslash.alt", "O", "acutecomb"),
            ] {
                let width = f.glyph(base).unwrap().layer(id).unwrap().width;
                let l = f.glyph_mut(g).unwrap().layer_mut(id).unwrap();
                l.width = width;
                l.shapes = vec![
                    Shape::component(base, kernest_core::IDENTITY),
                    Shape::component(mark, [1.0, 0.0, 0.0, 1.0, 40.0, 0.0]),
                ];
            }
        }
        f
    }

    /// (width, bounds) of every glyph of a CFF or CFF2 table at `coord` on the first axis.
    fn cff_glyphs(
        table: &[u8],
        cff2: bool,
        coord: f32,
        count: u16,
    ) -> Vec<(f64, Option<[f64; 4]>)> {
        use write_fonts::read::ps::cff::CffFontRef;
        use write_fonts::types::{F2Dot14, GlyphId};
        let coords = [F2Dot14::from_f32(coord)];
        let coords: &[F2Dot14] = if cff2 { &coords } else { &[] };
        let cff = if cff2 {
            CffFontRef::new_cff2(table, None).unwrap()
        } else {
            CffFontRef::new_cff(table, 0, None).unwrap()
        };
        let sub = cff.subfont(0, coords).unwrap();
        (0..count)
            .map(|g| {
                let gid = GlyphId::new(u32::from(g));
                let w = if cff2 {
                    0.0
                } else {
                    cff.evaluate_width(&sub, gid, coords)
                        .unwrap()
                        .map(|w| w.to_f64())
                        .unwrap_or(0.0)
                };
                let b = cff.evaluate_extents(&sub, gid, coords).unwrap().map(|b| {
                    [
                        b.x_min.to_f64(),
                        b.y_min.to_f64(),
                        b.x_max.to_f64(),
                        b.y_max.to_f64(),
                    ]
                });
                (w, b)
            })
            .collect()
    }

    #[test]
    fn optimised_cff_draws_the_same_and_is_smaller() {
        use write_fonts::types::Tag;
        // Static CFF.
        let f = with_composites(sample_font());
        let m = f.masters[0].clone();
        let ufo = to_ufo(&master_source(&f, &m, "Regular", true)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.ufo");
        ufo.save(&path).unwrap();
        let ttf = compile(&path).unwrap();
        let font = FontRef::new(&ttf).unwrap();
        let n = font.maxp().unwrap().num_glyphs();
        let post = font.post().unwrap();
        let order: Vec<String> = (0..n)
            .map(|g| {
                post.glyph_name(write_fonts::types::GlyphId16::new(g))
                    .unwrap()
                    .to_string()
            })
            .collect();
        let hmtx = font.hmtx().unwrap();
        let widths: Vec<u16> = (0..n)
            .map(|g| {
                hmtx.advance(write_fonts::types::GlyphId::new(u32::from(g)))
                    .unwrap()
            })
            .collect();
        let build =
            |opt| cff::build_cff_with(&ufo, &order, &widths, "T-R", "T R", "T", opt).unwrap();
        let (small, plain) = (build(true), build(false));
        assert_eq!(
            cff_glyphs(&small, false, 0.0, n),
            cff_glyphs(&plain, false, 0.0, n)
        );
        let w: Vec<f64> = cff_glyphs(&small, false, 0.0, n)
            .iter()
            .map(|g| g.0)
            .collect();
        assert_eq!(
            w,
            widths.iter().map(|w| f64::from(*w)).collect::<Vec<_>>(),
            "widths"
        );
        assert!(
            (small.len() as f64) < plain.len() as f64 * 0.85,
            "static CFF: {} vs {} bytes",
            small.len(),
            plain.len()
        );
        println!(
            "static CFF: {} bytes optimised, {} plain",
            small.len(),
            plain.len()
        );

        // Variable CFF2, at three weights.
        let mut f = with_composites(two_masters());
        let bold = f.masters[1].id.clone();
        // Bold's H and O outlines differ, so their charstrings blend.
        for g in ["H", "O"] {
            kernest_core::edit::translate_x(
                f.glyph_mut(g).unwrap().layer_mut(&bold).unwrap(),
                50.0,
            );
        }
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        let vf = std::fs::read(&r.files[0].path).unwrap();
        let vfont = FontRef::new(&vf).unwrap();
        let n = vfont.maxp().unwrap().num_glyphs();
        let small = FontRef::new(&variable_otf(&f, &vf).unwrap())
            .unwrap()
            .table_data(Tag::new(b"CFF2"))
            .unwrap()
            .as_bytes()
            .to_vec();
        // The same font, encoded plainly.
        let post = vfont.post().unwrap();
        let names: Vec<String> = (0..n)
            .map(|g| {
                post.glyph_name(write_fonts::types::GlyphId16::new(g))
                    .unwrap()
                    .to_string()
            })
            .collect();
        let mut statics = norad::Font::new();
        statics
            .default_layer_mut()
            .insert_glyph(ufo::notdef(&f.masters[0].metrics));
        let order: Vec<cff2::GlyphSource> = names
            .iter()
            .map(|n| match f.glyph(n) {
                Some(_) => cff2::GlyphSource::Glyph(n.as_str()),
                None => cff2::GlyphSource::Static(statics.get_glyph(n.as_str()).unwrap()),
            })
            .collect();
        let plain = cff2::build_cff2_with(&f, &order, false).unwrap();
        for coord in [0.0, 0.5, 1.0] {
            assert_eq!(
                cff_glyphs(&small, true, coord, n),
                cff_glyphs(&plain, true, coord, n),
                "at {coord}"
            );
        }
        assert!(
            (small.len() as f64) < plain.len() as f64 * 0.85,
            "CFF2: {} vs {} bytes",
            small.len(),
            plain.len()
        );
        println!(
            "CFF2: {} bytes optimised, {} plain",
            small.len(),
            plain.len()
        );
    }

    /// A font whose H and O have known stems (as in the core hinting tests), zones,
    /// blue controls, and a manual ghost hint on I.
    fn hinted_font() -> Font {
        use kernest_core::{Hint, Node, Path, PsHinting, Stem, Zone};
        let mut f = sample_font();
        let line = |points: &[(f64, f64)]| {
            Shape::Path(Path {
                closed: true,
                locked: false,
                stroke: None,
                nodes: points
                    .iter()
                    .map(|&(x, y)| Node::new(x, y, NodeKind::Line))
                    .collect(),
            })
        };
        f.glyph_mut("H").unwrap().layer_mut("m01").unwrap().shapes = vec![line(&[
            (80.0, 0.0),
            (180.0, 0.0),
            (180.0, 330.0),
            (420.0, 330.0),
            (420.0, 0.0),
            (520.0, 0.0),
            (520.0, 700.0),
            (420.0, 700.0),
            (420.0, 380.0),
            (180.0, 380.0),
            (180.0, 700.0),
            (80.0, 700.0),
        ])];
        let ring = |rx: f64, ry: f64| {
            let (cy, k) = (350.0, 0.55);
            let nodes = [
                (300.0, cy - ry, NodeKind::Curve),
                (300.0 + k * rx, cy - ry, NodeKind::OffCurve),
                (300.0 + rx, cy - k * ry, NodeKind::OffCurve),
                (300.0 + rx, cy, NodeKind::Curve),
                (300.0 + rx, cy + k * ry, NodeKind::OffCurve),
                (300.0 + k * rx, cy + ry, NodeKind::OffCurve),
                (300.0, cy + ry, NodeKind::Curve),
                (300.0 - k * rx, cy + ry, NodeKind::OffCurve),
                (300.0 - rx, cy + k * ry, NodeKind::OffCurve),
                (300.0 - rx, cy, NodeKind::Curve),
                (300.0 - rx, cy - k * ry, NodeKind::OffCurve),
                (300.0 - k * rx, cy - ry, NodeKind::OffCurve),
            ]
            .into_iter()
            .map(|(x, y, k)| Node::new(x, y, k))
            .collect();
            Shape::Path(Path {
                closed: true,
                locked: false,
                stroke: None,
                nodes,
            })
        };
        f.glyph_mut("O").unwrap().layer_mut("m01").unwrap().shapes =
            vec![ring(220.0, 360.0), ring(120.0, 260.0)];
        let m = &mut f.masters[0];
        m.alignment_zones = vec![
            Zone {
                position: 700.0,
                size: 10.0,
            },
            Zone {
                position: 0.0,
                size: -10.0,
            },
            Zone {
                position: -210.0,
                size: -10.0,
            },
        ];
        m.stems = vec![
            Stem {
                name: "h0".into(),
                horizontal: true,
                value: 50.0,
            },
            Stem {
                name: "h1".into(),
                horizontal: true,
                value: 100.0,
            },
            Stem {
                name: "v0".into(),
                horizontal: false,
                value: 100.0,
            },
        ];
        f.info.ps_hinting = Some(PsHinting {
            blue_scale: 0.05,
            blue_shift: 7.0,
            blue_fuzz: 2.0,
        });
        // Manual hints win over the automatic ones: a top ghost and one bar.
        f.glyph_mut("I").unwrap().layer_mut("m01").unwrap().hints = vec![
            Hint {
                horizontal: true,
                position: 700.0,
                width: 0.0,
                ghost_top: true,
            },
            Hint {
                horizontal: false,
                position: 80.0,
                width: 100.0,
                ghost_top: false,
            },
        ];
        f
    }

    #[test]
    fn cff_export_carries_zones_stems_and_hints() {
        use write_fonts::read::ps::cff::dict::{self, Entry};
        use write_fonts::read::ps::cff::index::Index;
        use write_fonts::read::ps::cff::CffFontRef;
        use write_fonts::types::Fixed;
        let f = hinted_font();
        let m = f.masters[0].clone();
        let ufo = to_ufo(&master_source(&f, &m, "Regular", true)).unwrap();
        assert_eq!(
            ufo.font_info.postscript_blue_values,
            Some(vec![-10.0, 0.0, 700.0, 710.0])
        );
        assert_eq!(
            ufo.font_info.postscript_other_blues,
            Some(vec![-220.0, -210.0])
        );
        assert_eq!(
            ufo.font_info.postscript_stem_snap_h,
            Some(vec![50.0, 100.0])
        );
        assert_eq!(ufo.font_info.postscript_stem_snap_v, Some(vec![100.0]));
        let lib_hints = |name: &str| -> Vec<(bool, f64, f64, bool)> {
            let mut v: Vec<(bool, f64, f64, bool)> = ufo
                .lib
                .get("com.kernest.hints")
                .and_then(|v| v.as_dictionary())
                .and_then(|d| d.get(name))
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .map(|h| {
                            let d = h.as_dictionary().unwrap();
                            let num = |k: &str| {
                                d.get(k)
                                    .and_then(|v| v.as_real())
                                    .or_else(|| {
                                        d.get(k)
                                            .and_then(|v| v.as_signed_integer())
                                            .map(|i| i as f64)
                                    })
                                    .unwrap()
                            };
                            (
                                d.get("horizontal").and_then(|v| v.as_boolean()).unwrap(),
                                num("position"),
                                num("width"),
                                d.get("ghostTop").and_then(|v| v.as_boolean()).unwrap(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            v.sort_by_key(|(h, p, _, _)| (!*h, *p as i64));
            v
        };
        // Automatic stems: H's crossbar and bars, O's ring; I keeps its manual hints.
        assert_eq!(
            lib_hints("H"),
            vec![
                (true, 330.0, 50.0, false),
                (false, 80.0, 100.0, false),
                (false, 420.0, 100.0, false)
            ]
        );
        assert_eq!(
            lib_hints("O"),
            vec![
                (true, -10.0, 100.0, false),
                (true, 610.0, 100.0, false),
                (false, 80.0, 100.0, false),
                (false, 420.0, 100.0, false)
            ]
        );
        assert_eq!(
            lib_hints("I"),
            vec![(true, 700.0, 0.0, true), (false, 80.0, 100.0, false)]
        );

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.ufo");
        ufo.save(&path).unwrap();
        let ttf = compile(&path).unwrap();
        let font = FontRef::new(&ttf).unwrap();
        let n = font.maxp().unwrap().num_glyphs();
        let post = font.post().unwrap();
        let order: Vec<String> = (0..n)
            .map(|g| {
                post.glyph_name(write_fonts::types::GlyphId16::new(g))
                    .unwrap()
                    .to_string()
            })
            .collect();
        let hmtx = font.hmtx().unwrap();
        let widths: Vec<u16> = (0..n)
            .map(|g| {
                hmtx.advance(write_fonts::types::GlyphId::new(u32::from(g)))
                    .unwrap()
            })
            .collect();
        let table = cff::build_cff(&ufo, &order, &widths, "T-R", "T R", "T").unwrap();

        // The Private DICT carries the zones, stems and blue controls.
        let hdr = usize::from(table[2]);
        let names = Index::new(&table[hdr..], false).unwrap();
        let top_at = hdr + names.size_in_bytes().unwrap();
        let top_index = Index::new(&table[top_at..], false).unwrap();
        let top = top_index.get(0).unwrap();
        let range = dict::entries(top, None)
            .filter_map(|e| e.ok())
            .find_map(|e| match e {
                Entry::PrivateDictRange(r) => Some(r),
                _ => None,
            })
            .expect("a Private DICT");
        let entries: Vec<Entry> = dict::entries(&table[range.clone()], None)
            .filter_map(|e| e.ok())
            .collect();
        assert!(entries.contains(&Entry::StdHw(Fixed::from_i32(50))));
        assert!(entries.contains(&Entry::StdVw(Fixed::from_i32(100))));
        assert!(entries.contains(&Entry::BlueShift(Fixed::from_i32(7))));
        assert!(entries.contains(&Entry::BlueFuzz(Fixed::from_i32(2))));
        let cff = CffFontRef::new_cff(&table, 0, None).unwrap();
        let (_, params) = cff.subfont_hinted(0, &[]).unwrap();
        assert_eq!(
            params.blues.values(),
            [
                (Fixed::from_i32(-10), Fixed::ZERO),
                (Fixed::from_i32(700), Fixed::from_i32(710))
            ]
        );
        assert_eq!(
            params.other_blues.values(),
            [(Fixed::from_i32(-220), Fixed::from_i32(-210))]
        );
        // Readers hold blue values at 1/65536 precision.
        assert!((params.blue_scale.to_f64() - 0.05).abs() < 1.0 / 65536.0);
        assert_eq!(params.blue_shift, Fixed::from_i32(7));
        assert_eq!(params.blue_fuzz, Fixed::from_i32(2));

        // The charstrings carry the stem hints (exact bytes, before the outlines).
        let has_bytes = |name: &str, bytes: &[u8]| {
            let gid = order.iter().position(|g| g == name).unwrap();
            let cs = cff.charstrings().get(gid).unwrap();
            assert!(
                cs.windows(bytes.len()).any(|w| w == bytes),
                "{name} charstring lacks its hints"
            );
        };
        // hstem 330 50; vstem 80 100 240 100.
        has_bytes("H", &[247, 222, 189, 1, 219, 239, 247, 132, 239, 3]);
        // hstem -10 100 520 100; the same vstem.
        has_bytes(
            "O",
            &[129, 239, 248, 156, 239, 1, 219, 239, 247, 132, 239, 3],
        );
        // I's manual hints: hstem 700 -20 (ghost); vstem 80 100.
        has_bytes("I", &[249, 80, 119, 1, 219, 239, 3]);

        // Hints don't change the drawing: every glyph decodes to the same widths
        // and bounds as a hint-free build of the same source.
        let mut plain_ufo = ufo.clone();
        plain_ufo.lib.remove("com.kernest.hints");
        plain_ufo.font_info.postscript_blue_values = None;
        plain_ufo.font_info.postscript_other_blues = None;
        plain_ufo.font_info.postscript_stem_snap_h = None;
        plain_ufo.font_info.postscript_stem_snap_v = None;
        plain_ufo.font_info.postscript_blue_scale = None;
        plain_ufo.font_info.postscript_blue_shift = None;
        plain_ufo.font_info.postscript_blue_fuzz = None;
        let plain = cff::build_cff(&plain_ufo, &order, &widths, "T-R", "T R", "T").unwrap();
        assert_ne!(table, plain);
        assert_eq!(
            cff_glyphs(&table, false, 0.0, n),
            cff_glyphs(&plain, false, 0.0, n)
        );
        let w: Vec<f64> = cff_glyphs(&table, false, 0.0, n)
            .iter()
            .map(|g| g.0)
            .collect();
        assert_eq!(w, widths.iter().map(|w| f64::from(*w)).collect::<Vec<_>>());
    }

    #[test]
    fn cff2_export_carries_zones_and_stems() {
        use kernest_core::{PsHinting, Stem, Zone};
        use write_fonts::read::ps::cff::dict::{self, Entry};
        use write_fonts::read::ps::cff::index::Index;
        use write_fonts::read::ps::cff::CffFontRef;
        use write_fonts::types::{F2Dot14, Fixed, Tag};
        let mut f = two_masters();
        let m = &mut f.masters[0];
        m.alignment_zones = vec![
            Zone {
                position: 700.0,
                size: 10.0,
            },
            Zone {
                position: 0.0,
                size: -10.0,
            },
            Zone {
                position: -210.0,
                size: -10.0,
            },
        ];
        m.stems = vec![
            Stem {
                name: "h0".into(),
                horizontal: true,
                value: 50.0,
            },
            Stem {
                name: "h1".into(),
                horizontal: true,
                value: 100.0,
            },
            Stem {
                name: "v0".into(),
                horizontal: false,
                value: 100.0,
            },
        ];
        f.info.ps_hinting = Some(PsHinting {
            blue_scale: 0.05,
            blue_shift: 7.0,
            blue_fuzz: 2.0,
        });
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        let vf = std::fs::read(&r.files[0].path).unwrap();
        let otf = variable_otf(&f, &vf).unwrap();
        let font = FontRef::new(&otf).unwrap();
        let n = font.maxp().unwrap().num_glyphs();
        let table = font
            .table_data(Tag::new(b"CFF2"))
            .unwrap()
            .as_bytes()
            .to_vec();
        let cff = CffFontRef::new_cff2(&table, None).unwrap();
        let (_, params) = cff.subfont_hinted(0, &[F2Dot14::ZERO]).unwrap();
        assert_eq!(
            params.blues.values(),
            [
                (Fixed::from_i32(-10), Fixed::ZERO),
                (Fixed::from_i32(700), Fixed::from_i32(710))
            ]
        );
        assert_eq!(
            params.other_blues.values(),
            [(Fixed::from_i32(-220), Fixed::from_i32(-210))]
        );
        assert!((params.blue_scale.to_f64() - 0.05).abs() < 1.0 / 65536.0);
        assert_eq!(params.blue_shift, Fixed::from_i32(7));
        assert_eq!(params.blue_fuzz, Fixed::from_i32(2));
        // StdHW and the stem snaps live in the Private DICT under the FDArray.
        let top_len = u16::from_be_bytes([table[3], table[4]]) as usize;
        let top = &table[5..5 + top_len];
        let fd_off = dict::entries(top, None)
            .filter_map(|e| e.ok())
            .find_map(|e| match e {
                Entry::FdArrayOffset(o) => Some(o),
                _ => None,
            })
            .expect("an FDArray");
        let fd = Index::new(&table[fd_off..], true).unwrap();
        let font_dict = fd.get(0).unwrap();
        let range = dict::entries(font_dict, None)
            .filter_map(|e| e.ok())
            .find_map(|e| match e {
                Entry::PrivateDictRange(r) => Some(r),
                _ => None,
            })
            .expect("a Private DICT");
        let entries: Vec<Entry> = dict::entries(&table[range], None)
            .filter_map(|e| e.ok())
            .collect();
        assert!(entries.contains(&Entry::StdHw(Fixed::from_i32(50))));
        assert!(entries.contains(&Entry::StdVw(Fixed::from_i32(100))));
        let snaps = |pick: fn(&Entry) -> bool| -> Vec<Fixed> {
            entries
                .iter()
                .find(|e| pick(e))
                .map(|e| match e {
                    Entry::StemSnapH(s) | Entry::StemSnapV(s) => s.values().to_vec(),
                    _ => unreachable!(),
                })
                .unwrap()
        };
        assert_eq!(
            snaps(|e| matches!(e, Entry::StemSnapH(_))),
            [Fixed::from_i32(50), Fixed::from_i32(100)]
        );
        assert_eq!(
            snaps(|e| matches!(e, Entry::StemSnapV(_))),
            [Fixed::from_i32(100)]
        );
        // The charstrings still decode at every weight (CFF2 carries no stem hints).
        for coord in [0.0, 0.5, 1.0] {
            assert_eq!(cff_glyphs(&table, true, coord, n).len(), usize::from(n));
        }
    }

    #[test]
    fn smart_components_export_as_outlines() {
        use kernest_core::glyphset::{add_glyphs, NewGlyph};
        use kernest_core::{smart, SmartAxis};
        let f = sample_font();
        let f = add_glyphs(
            &f,
            &[NewGlyph {
                name: "_stem".into(),
                codepoint: None,
            }],
        )
        .unwrap()
        .font;
        let axes = vec![SmartAxis {
            name: "Height".into(),
            min: 0.0,
            max: 100.0,
        }];
        let mut f = smart::set_smart_axes(&f, "_stem", axes).unwrap();
        let rect = |h: f64| {
            Shape::Path(kernest_core::Path {
                locked: false,
                stroke: None,
                closed: true,
                nodes: [(0.0, 0.0), (80.0, 0.0), (80.0, h), (0.0, h)]
                    .into_iter()
                    .map(|(x, y)| kernest_core::Node::new(x, y, NodeKind::Line))
                    .collect(),
            })
        };
        f.glyph_mut("_stem")
            .unwrap()
            .layer_mut("m01")
            .unwrap()
            .shapes = vec![rect(300.0)];
        let (mut f, pole) = smart::add_pole(
            &f,
            "_stem",
            "m01",
            &BTreeMap::from([("Height".into(), 100.0)]),
        )
        .unwrap();
        f.glyph_mut("_stem")
            .unwrap()
            .layer_mut(&pole)
            .unwrap()
            .shapes = vec![rect(700.0)];
        f.glyph_mut("_stem").unwrap().export = false;
        for (g, h) in [("I", 100.0), ("l", 50.0)] {
            f.glyph_mut(g).unwrap().layer_mut("m01").unwrap().shapes = vec![Shape::Component {
                base: "_stem".into(),
                transform: [1.0, 0.0, 0.0, 1.0, 80.0, 0.0],
                smart: BTreeMap::from([("Height".into(), h)]),
                align: true,
            }];
        }
        let out = tempfile::tempdir().unwrap();
        let mut opts = ExportOptions::ttf(false, true, out.path());
        opts.otf = true;
        let r = export(&f, &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let ttf = std::fs::read(&r.files.iter().find(|x| x.format == "ttf").unwrap().path).unwrap();
        let font = FontRef::new(&ttf).unwrap();
        let (glyf, loca, cmap) = (
            font.glyf().unwrap(),
            font.loca(None).unwrap(),
            font.cmap().unwrap(),
        );
        let top = |c: char| {
            loca.get_glyf(cmap.map_codepoint(c).unwrap(), &glyf)
                .unwrap()
                .unwrap()
                .y_max()
        };
        assert_eq!(top('I'), 700, "Height 100: the tall pole");
        assert_eq!(top('l'), 500, "Height 50: halfway");
        assert!(r.files.iter().any(|x| x.format == "otf"));
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
    fn autohint_on_export() {
        use write_fonts::types::Tag;
        let mut f = two_masters();
        f.info.hinting = Some(kernest_core::Hinting {
            range_min: 9,
            range_max: 40,
            ..Default::default()
        });
        let out = tempfile::tempdir().unwrap();
        let opts = ExportOptions {
            otf: true,
            autohint: true,
            ..ExportOptions::ttf(true, true, out.path())
        };
        let r = export(&f, &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let fpgm = |path: &str| {
            let b = std::fs::read(path).unwrap();
            FontRef::new(&b)
                .unwrap()
                .table_data(Tag::new(b"fpgm"))
                .is_some()
        };
        for file in &r.files {
            assert_eq!(fpgm(&file.path), file.format == "ttf", "{}", file.path);
        }
        assert_eq!(r.files.iter().filter(|x| x.format == "ttf").count(), 3);
        // Without the option nothing is hinted.
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, true, out.path())).unwrap();
        assert!(r.files.iter().all(|x| !fpgm(&x.path)));
    }

    #[test]
    fn colour_fonts_export_colr_and_cpal() {
        use kernest_core::color::{self, Color, ColorStop, Paint};
        use write_fonts::read::tables::colr::Paint as RPaint;
        use write_fonts::types::Tag;
        let f = color::set_palettes(
            &two_masters(),
            vec![vec![
                Color::parse("#e63946").unwrap(),
                Color::parse("#1d3557").unwrap(),
            ]],
        )
        .unwrap();
        let (f, _) = color::add_color_layer(&f, "H", "m01").unwrap();
        let (f, top) = color::add_color_layer(&f, "H", "m01").unwrap();
        // The top layer: a bar with a gradient, in both masters.
        let mut f = f;
        for l in f
            .glyph_mut("H")
            .unwrap()
            .layers
            .iter_mut()
            .filter(|l| l.color.is_some())
        {
            if l.shapes.is_empty() {
                l.shapes = vec![Shape::Path(kernest_core::Path {
                    locked: false,
                    stroke: None,
                    closed: true,
                    nodes: [(0.0, 300.0), (600.0, 300.0), (600.0, 400.0), (0.0, 400.0)]
                        .into_iter()
                        .map(|(x, y)| kernest_core::Node::new(x, y, NodeKind::Line))
                        .collect(),
                })];
            }
        }
        let grad = Paint::Linear {
            start: [0.0, 0.0],
            end: [600.0, 0.0],
            stops: vec![
                ColorStop {
                    offset: 0.0,
                    color: 0,
                    alpha: 1.0,
                },
                ColorStop {
                    offset: 1.0,
                    color: 1,
                    alpha: 0.5,
                },
            ],
        };
        let mut f = color::set_paint(&f, "H", &top, grad).unwrap();
        // Production names rename H in the compiled font; COLR must still find it.
        f.glyph_mut("H").unwrap().production_name = Some("Hprod".into());
        let out = tempfile::tempdir().unwrap();
        let opts = ExportOptions {
            otf: true,
            production_names: true,
            ..ExportOptions::ttf(true, true, out.path())
        };
        let r = export(&f, &opts).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        for file in &r.files {
            let bytes = std::fs::read(&file.path).unwrap();
            let font = FontRef::new(&bytes).unwrap();
            let colr = font
                .colr()
                .unwrap_or_else(|e| panic!("{}: no COLR: {e}", file.path));
            assert_eq!(colr.version(), 1);
            let cpal = font.cpal().unwrap();
            assert_eq!((cpal.num_palettes(), cpal.num_palette_entries()), (1, 2));
            let rec = cpal.color_records_array().unwrap().unwrap()[0];
            assert_eq!((rec.red(), rec.green(), rec.blue()), (0xe6, 0x39, 0x46));
            assert!(font.table_data(Tag::new(b"COLR")).is_some());
            // H has a v1 paint: two layers, the second a linear gradient on H.color1.
            let gid = font.cmap().unwrap().map_codepoint('H').unwrap();
            let list = colr.base_glyph_list().unwrap().unwrap();
            let rec = list
                .base_glyph_paint_records()
                .iter()
                .find(|r| r.glyph_id().to_u32() == gid.to_u32())
                .expect("H is a colour glyph");
            let RPaint::ColrLayers(layers) = rec.paint(list.offset_data()).unwrap() else {
                panic!("layers")
            };
            assert_eq!(layers.num_layers(), 2);
            let ll = colr.layer_list().unwrap().unwrap();
            let RPaint::Glyph(top) = ll
                .paints()
                .get(layers.first_layer_index() as usize + 1)
                .unwrap()
            else {
                panic!("glyph paint")
            };
            assert!(
                matches!(top.paint().unwrap(), RPaint::LinearGradient(_)),
                "{}",
                file.path
            );
            // A gradient layer means no v0 fallback record for H.
            assert_eq!(colr.num_base_glyph_records(), 0);
        }
        // Variable fonts vary the colour layers too (the layer glyphs are in gvar).
        let vf = r
            .files
            .iter()
            .find(|x| x.kind == "variable" && x.format == "ttf")
            .unwrap();
        let bytes = std::fs::read(&vf.path).unwrap();
        assert!(FontRef::new(&bytes).unwrap().gvar().is_ok());
    }

    #[test]
    fn variable_colour_gradient_deltas_at_extremes() {
        use kernest_core::color::{self, Color, ColorStop, Paint};
        use write_fonts::read::tables::colr::Paint as RPaint;
        use write_fonts::read::tables::variations::DeltaSetIndex;
        use write_fonts::types::F2Dot14;

        let f = color::set_palettes(
            &two_masters(),
            vec![vec![
                Color::parse("#e63946").unwrap(),
                Color::parse("#1d3557").unwrap(),
            ]],
        )
        .unwrap();
        let bold = f.masters[1].id.clone();
        let (f, key) = color::add_color_layer(&f, "H", "m01").unwrap();
        let stops = |a: f64| {
            vec![
                ColorStop {
                    offset: 0.0,
                    color: 0,
                    alpha: 1.0,
                },
                ColorStop {
                    offset: 1.0,
                    color: 1,
                    alpha: a,
                },
            ]
        };
        let f = color::set_paint(
            &f,
            "H",
            &key,
            Paint::Linear {
                start: [0.0, 0.0],
                end: [600.0, 0.0],
                stops: stops(1.0),
            },
        )
        .unwrap();
        // The bold master's gradient reaches further and its last stop is half opaque.
        let f = color::set_master_paint(
            &f,
            "H",
            &key,
            &bold,
            Paint::Linear {
                start: [0.0, 0.0],
                end: [900.0, 0.0],
                stops: stops(0.5),
            },
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let vf = r
            .files
            .iter()
            .find(|x| x.kind == "variable" && x.format == "ttf")
            .unwrap();
        let bytes = std::fs::read(&vf.path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        assert!(font.gvar().is_ok());
        let colr = font.colr().unwrap();
        assert_eq!(colr.version(), 1);
        let ivs = colr.item_variation_store().unwrap().unwrap();
        let map = colr.var_index_map().unwrap().unwrap();
        // CPAL is unchanged: the colours are the same in both masters.
        assert_eq!(font.cpal().unwrap().num_palette_entries(), 2);

        let gid = font.cmap().unwrap().map_codepoint('H').unwrap();
        let list = colr.base_glyph_list().unwrap().unwrap();
        let rec = list
            .base_glyph_paint_records()
            .iter()
            .find(|r| r.glyph_id().to_u32() == gid.to_u32())
            .unwrap();
        let RPaint::ColrLayers(layers) = rec.paint(list.offset_data()).unwrap() else {
            panic!("layers")
        };
        assert_eq!(layers.num_layers(), 1, "uniform colours need no cross-fade");
        let ll = colr.layer_list().unwrap().unwrap();
        let RPaint::Glyph(gp) = ll
            .paints()
            .get(layers.first_layer_index() as usize)
            .unwrap()
        else {
            panic!("glyph paint")
        };
        let RPaint::VarLinearGradient(grad) = gp.paint().unwrap() else {
            panic!("expected a variable linear gradient");
        };
        let base = grad.var_index_base();
        let default = [F2Dot14::ZERO];
        let bold_coords = [F2Dot14::from_f32(1.0)];
        let delta = |index: u32, coords: &[F2Dot14]| {
            let ix: DeltaSetIndex = map.get(index).unwrap();
            ivs.compute_delta(ix, coords).unwrap()
        };
        // x1 (field 2): 600 at the default master, 900 at the bold master.
        assert_eq!(grad.x1().to_i16(), 600);
        assert_eq!(grad.x1().apply_delta(delta(base + 2, &default)), 600.0);
        assert_eq!(grad.x1().apply_delta(delta(base + 2, &bold_coords)), 900.0);
        // x0 stays put; the gradient still starts at the origin in both masters.
        assert_eq!(grad.x0().apply_delta(delta(base, &bold_coords)), 0.0);
        // The last stop's opacity (its var block + 1): 1.0 → 0.5.
        let cl = grad.color_line().unwrap();
        let last = cl.color_stops().get(1).unwrap();
        let sb = last.var_index_base();
        assert_eq!(last.alpha().to_f32(), 1.0);
        assert_eq!(last.alpha().apply_delta(delta(sb + 1, &default)), 1.0);
        assert!((last.alpha().apply_delta(delta(sb + 1, &bold_coords)) - 0.5).abs() < 1e-3);
        // Stop positions do not move.
        assert_eq!(last.stop_offset().apply_delta(delta(sb, &bold_coords)), 1.0);
    }

    #[test]
    fn variable_colour_palette_override_cross_fades() {
        use kernest_core::color::{self, Color, Paint};
        use write_fonts::read::tables::colr::Paint as RPaint;
        use write_fonts::types::F2Dot14;

        let f = color::set_palettes(
            &two_masters(),
            vec![vec![
                Color::parse("#e63946").unwrap(),
                Color::parse("#1d3557").unwrap(),
            ]],
        )
        .unwrap();
        let bold = f.masters[1].id.clone();
        let (f, key) = color::add_color_layer(&f, "H", "m01").unwrap();
        let f = color::set_paint(
            &f,
            "H",
            &key,
            Paint::Solid {
                color: 0,
                alpha: 1.0,
            },
        )
        .unwrap();
        // The bold master paints colour 1 green instead of red.
        let f = color::set_master_palettes(
            &f,
            &bold,
            Some(vec![vec![
                Color::parse("#00ff00").unwrap(),
                Color::parse("#1d3557").unwrap(),
            ]]),
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let vf = r
            .files
            .iter()
            .find(|x| x.kind == "variable" && x.format == "ttf")
            .unwrap();
        let bytes = std::fs::read(&vf.path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let colr = font.colr().unwrap();
        let ivs = colr.item_variation_store().unwrap().unwrap();
        let map = colr.var_index_map().unwrap().unwrap();
        // CPAL grew by the override colour (red, blue, green).
        let cpal = font.cpal().unwrap();
        assert_eq!(cpal.num_palette_entries(), 3);
        let recs = cpal.color_records_array().unwrap().unwrap();
        assert_eq!(
            (recs[2].red(), recs[2].green(), recs[2].blue()),
            (0, 0xff, 0)
        );

        let gid = font.cmap().unwrap().map_codepoint('H').unwrap();
        let list = colr.base_glyph_list().unwrap().unwrap();
        let rec = list
            .base_glyph_paint_records()
            .iter()
            .find(|r| r.glyph_id().to_u32() == gid.to_u32())
            .unwrap();
        let RPaint::ColrLayers(layers) = rec.paint(list.offset_data()).unwrap() else {
            panic!("layers")
        };
        assert_eq!(layers.num_layers(), 2, "two colours cross-fade");
        let ll = colr.layer_list().unwrap().unwrap();
        let default = [F2Dot14::ZERO];
        let bold_coords = [F2Dot14::from_f32(1.0)];
        let mut alphas = Vec::new();
        for i in 0..2 {
            let RPaint::Glyph(gp) = ll
                .paints()
                .get(layers.first_layer_index() as usize + i)
                .unwrap()
            else {
                panic!("glyph paint")
            };
            let RPaint::VarSolid(solid) = gp.paint().unwrap() else {
                panic!("expected a variable solid")
            };
            let ix = map.get(solid.var_index_base()).unwrap();
            let at_default = solid
                .alpha()
                .apply_delta(ivs.compute_delta(ix, &default).unwrap());
            let at_bold = solid
                .alpha()
                .apply_delta(ivs.compute_delta(ix, &bold_coords).unwrap());
            alphas.push((solid.palette_index(), at_default, at_bold));
        }
        // Exactly one branch is opaque at each master: red at the default, green at bold.
        let red = alphas.iter().find(|(p, _, _)| *p == 0).unwrap();
        let green = alphas.iter().find(|(p, _, _)| *p == 2).unwrap();
        assert!(
            (red.1 - 1.0).abs() < 1e-3 && red.2.abs() < 1e-3,
            "{alphas:?}"
        );
        assert!(
            green.1.abs() < 1e-3 && (green.2 - 1.0).abs() < 1e-3,
            "{alphas:?}"
        );
    }

    #[test]
    fn solid_colour_glyphs_also_get_colr_v0() {
        use kernest_core::color::{self, Color};
        let f = color::set_palettes(&sample_font(), vec![vec![Color::parse("#2a9d8f").unwrap()]])
            .unwrap();
        let (f, _) = color::add_color_layer(&f, "O", "m01").unwrap();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let colr = font.colr().unwrap();
        assert_eq!(
            colr.num_base_glyph_records(),
            1,
            "O has a v0 record for older apps"
        );
        let gid = font.cmap().unwrap().map_codepoint('O').unwrap();
        let rec = colr.base_glyph_records().unwrap().unwrap()[0];
        assert_eq!(rec.glyph_id().to_u32(), gid.to_u32());
        assert_eq!(rec.num_layers(), 1);
    }

    #[test]
    fn variable_export_notes_interpolation_warnings() {
        let mut f = two_masters();
        let bold = f.masters[1].id.clone();
        // Start Bold's O one segment later: still compatible, but it would morph badly.
        let l = f.glyph_mut("O").unwrap().layer_mut(&bold).unwrap();
        let Shape::Path(p) = &mut l.shapes[0] else {
            panic!("O starts with a path")
        };
        let on: Vec<usize> = (0..p.nodes.len())
            .filter(|&i| p.nodes[i].on_curve())
            .collect();
        p.nodes.rotate_left(on[1] - on[0]);
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert_eq!(r.files.len(), 1, "the variable font is still built");
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(
            r.warnings,
            ["1 glyph may not interpolate cleanly (O). See Interpolation warnings in Font View."]
        );
    }

    #[test]
    fn variable_needs_masters() {
        let out = tempfile::tempdir().unwrap();
        let r = export(&sample_font(), &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert!(r.files.is_empty());
        assert!(r.problems[0].contains("two masters"));
    }

    /// A font ready for mark attachment: a drawn `a`, zero-width marks with default
    /// anchors, and the composites `aacute` and `acircumflexacute` built from them.
    fn with_anchor_marks(mut f: Font) -> Font {
        use kernest_core::anchors::{create_composites, set_default_anchors};
        use kernest_core::glyphset::{add_glyphs, NewGlyph};
        f = add_glyphs(
            &f,
            &[
                NewGlyph {
                    name: "a".into(),
                    codepoint: Some(0x61),
                },
                NewGlyph {
                    name: "circumflexcomb".into(),
                    codepoint: Some(0x302),
                },
                NewGlyph {
                    name: "acutecomb".into(),
                    codepoint: Some(0x301),
                },
            ],
        )
        .unwrap()
        .font;
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
            Shape::Path(kernest_core::Path {
                locked: false,
                stroke: None,
                closed: true,
                nodes: [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                    .into_iter()
                    .map(|(x, y)| kernest_core::Node::new(x, y, NodeKind::Line))
                    .collect(),
            })
        };
        let ids: Vec<String> = f.masters.iter().map(|m| m.id.clone()).collect();
        for id in &ids {
            let l = f.glyph_mut("a").unwrap().layer_mut(id).unwrap();
            l.width = 580.0;
            l.shapes = vec![rect(0.0, 0.0, 580.0, 500.0)];
            let l = f
                .glyph_mut("circumflexcomb")
                .unwrap()
                .layer_mut(id)
                .unwrap();
            l.width = 0.0;
            l.shapes = vec![rect(-100.0, 500.0, 100.0, 650.0)];
            let l = f.glyph_mut("acutecomb").unwrap().layer_mut(id).unwrap();
            l.width = 0.0;
            l.shapes = vec![rect(-80.0, 700.0, 80.0, 780.0)];
        }
        let f = set_default_anchors(
            &f,
            &["a".into(), "circumflexcomb".into(), "acutecomb".into()],
        )
        .unwrap();
        create_composites(&f, &["aacute".into(), "acircumflexacute".into()])
            .unwrap()
            .font
    }

    #[test]
    fn anchors_export_mark_and_mkmk_positioning() {
        use write_fonts::read::tables::gpos::PositionLookup;
        let f = with_anchor_marks(sample_font());
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let gpos = font.gpos().expect("anchors compile into GPOS");
        let (mut mark_base, mut mark_mark) = (0, 0);
        for l in gpos.lookup_list().unwrap().lookups().iter() {
            match l.unwrap() {
                PositionLookup::MarkToBase(_) => mark_base += 1,
                PositionLookup::MarkToMark(_) => mark_mark += 1,
                _ => {}
            }
        }
        assert!(mark_base > 0, "mark lookup");
        assert!(mark_mark > 0, "mkmk lookup for stacked marks");
        let gdef = font.gdef().expect("categories compile into GDEF");
        let classes = gdef.glyph_class_def().unwrap().unwrap();
        let cmap = font.cmap().unwrap();
        assert_eq!(classes.get(cmap.map_codepoint('a').unwrap()), 1);
        assert_eq!(classes.get(cmap.map_codepoint(0x301u32).unwrap()), 3);
        assert_eq!(
            classes.get(cmap.map_codepoint(0x1EA5u32).unwrap()),
            1,
            "the composite is a base, so it can take further marks"
        );
    }

    #[test]
    fn caret_anchors_export_lig_caret_list() {
        use kernest_core::glyphset::{add_glyphs, parse_glyph_list};
        use kernest_core::Anchor;
        use write_fonts::read::tables::gdef::CaretValue;
        use write_fonts::types::GlyphId16;
        let f = sample_font();
        let mut f = add_glyphs(&f, &parse_glyph_list("f_i").unwrap())
            .unwrap()
            .font;
        let l = f.glyph_mut("f_i").unwrap().layer_mut("m01").unwrap();
        l.width = 600.0;
        let stem = |x: f64| {
            Shape::Path(kernest_core::Path {
                locked: false,
                stroke: None,
                closed: true,
                nodes: [(x, 0.0), (x + 100.0, 0.0), (x + 100.0, 700.0), (x, 700.0)]
                    .into_iter()
                    .map(|(x, y)| kernest_core::Node::new(x, y, NodeKind::Line))
                    .collect(),
            })
        };
        l.shapes = vec![stem(50.0), stem(400.0)];
        l.anchors = vec![Anchor {
            name: "caret_1".into(),
            x: 300.0,
            y: 0.0,
        }];
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let gdef = font.gdef().unwrap();
        let carets = gdef
            .lig_caret_list()
            .expect("caret_1 compiles into LigCaretList")
            .unwrap();
        assert_eq!(carets.lig_glyph_count(), 1);
        let lig = carets.lig_glyphs().iter().next().unwrap().unwrap();
        assert_eq!(lig.caret_count(), 1);
        let CaretValue::Format1(c) = lig.caret_values().iter().next().unwrap().unwrap() else {
            panic!("expected a coordinate caret")
        };
        assert_eq!(c.coordinate(), 300);
        let post = font.post().unwrap();
        let gid = (0..font.maxp().unwrap().num_glyphs())
            .map(GlyphId16::new)
            .find(|g| post.glyph_name(*g).unwrap() == "f_i")
            .unwrap();
        let classes = gdef.glyph_class_def().unwrap().unwrap();
        assert_eq!(classes.get(gid), 2, "f_i is a ligature");
    }

    #[test]
    fn entry_exit_anchors_export_cursive() {
        use kernest_core::anchors::set_default_anchors;
        use kernest_core::glyphset::{add_glyphs, NewGlyph};
        use write_fonts::read::tables::gpos::PositionLookup;
        let f = sample_font();
        let mut f = add_glyphs(
            &f,
            &[
                NewGlyph {
                    name: "uni0628".into(),
                    codepoint: Some(0x628),
                },
                NewGlyph {
                    name: "uni0627".into(),
                    codepoint: Some(0x627),
                },
            ],
        )
        .unwrap()
        .font;
        for (n, x0, x1) in [("uni0628", 50.0, 550.0), ("uni0627", 200.0, 300.0)] {
            let l = f.glyph_mut(n).unwrap().layer_mut("m01").unwrap();
            l.shapes = vec![Shape::Path(kernest_core::Path {
                locked: false,
                stroke: None,
                closed: true,
                nodes: [(x0, 0.0), (x1, 0.0), (x1, 500.0), (x0, 500.0)]
                    .into_iter()
                    .map(|(x, y)| kernest_core::Node::new(x, y, NodeKind::Line))
                    .collect(),
            })];
        }
        let f = set_default_anchors(&f, &["uni0628".into(), "uni0627".into()]).unwrap();
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(false, true, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let gpos = font.gpos().expect("entry/exit compile into GPOS");
        let cursive = gpos
            .lookup_list()
            .unwrap()
            .lookups()
            .iter()
            .filter(|l| matches!(l, Ok(PositionLookup::Cursive(_))))
            .count();
        assert!(cursive > 0, "cursive lookup");
    }

    #[test]
    fn variable_font_keeps_mark_positioning() {
        use write_fonts::read::tables::gpos::PositionLookup;
        let f = with_anchor_marks(two_masters());
        let out = tempfile::tempdir().unwrap();
        let r = export(&f, &ExportOptions::ttf(true, false, out.path())).unwrap();
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        let bytes = std::fs::read(&r.files[0].path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        let gpos = font.gpos().expect("variable font keeps GPOS");
        let marks = gpos
            .lookup_list()
            .unwrap()
            .lookups()
            .iter()
            .filter(|l| matches!(l, Ok(PositionLookup::MarkToBase(_))))
            .count();
        assert!(marks > 0);
        let gdef = font.gdef().unwrap();
        let classes = gdef.glyph_class_def().unwrap().unwrap();
        let cmap = font.cmap().unwrap();
        assert_eq!(classes.get(cmap.map_codepoint(0x301u32).unwrap()), 3);
    }
}
