use clap::{Parser, Subcommand, ValueEnum};
use kernest_core::Font;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "kernest-cli",
    version,
    about = "Build, check and convert fonts without opening the editor"
)]
struct Args {
    #[arg(long, global = true, help = "Write a machine-readable JSON report")]
    json: bool,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    #[command(about = "Export a font using the same compiler as the editor")]
    Export {
        font: PathBuf,
        #[arg(long, value_enum, value_delimiter = ',', conflicts_with = "preset")]
        format: Vec<Format>,
        #[arg(long, conflicts_with = "preset", help = "Also build a variable font")]
        variable: bool,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        preset: Option<String>,
    },
    #[command(about = "Check exported fonts offline; exit 1 on FAIL, 2 on a check error")]
    Check {
        path: PathBuf,
        #[arg(long, default_value = "universal", value_parser = ["opentype", "universal"])]
        profile: String,
    },
    #[command(about = "Convert a font source without overwriting existing files")]
    Convert {
        input: PathBuf,
        #[arg(long, value_enum)]
        to: SourceFormat,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    #[command(about = "Show a font's family, glyphs, masters, axes and instances")]
    Info { font: PathBuf },
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Ttf,
    Otf,
    Woff2,
    Variable,
}
#[derive(Clone, Copy, ValueEnum)]
enum SourceFormat {
    Kernest,
    Glyphs,
    Ufo,
}

fn full_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path))
    }
}

fn load(path: &Path) -> Result<(Font, PathBuf, Vec<String>), String> {
    let path = full_path(path)?;
    let recognized = |p: &Path| {
        p.extension().and_then(|s| s.to_str()).is_some_and(|e| {
            [
                "kernest",
                "glyphs",
                "glyphspackage",
                "ufo",
                "designspace",
                "ttf",
                "otf",
                "woff2",
            ]
            .contains(&e.to_ascii_lowercase().as_str())
        })
    };
    let source = path
        .ancestors()
        .find(|p| recognized(p))
        .unwrap_or(&path)
        .to_path_buf();
    if source
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("kernest"))
    {
        let font = kernest_core::package::open(&source).map_err(|e| e.to_string())?;
        Ok((font, source, vec![]))
    } else {
        let imported = kernest_import::import(&source)?;
        Ok((imported.font, source, imported.warnings))
    }
}

fn print_json(value: &impl serde::Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn run(args: &Args) -> Result<u8, String> {
    match &args.command {
        Action::Info { font } => {
            let (font, source, warnings) = load(font)?;
            if args.json {
                print_json(
                    &json!({"path":source,"familyName":font.info.family_name,"unitsPerEm":font.info.units_per_em,"glyphs":font.glyphs.len(),"axes":font.axes,"masters":font.masters,"instances":font.instances,"presets":font.export_presets,"warnings":warnings}),
                )?;
            } else {
                println!(
                    "{}\n{} glyphs · {} masters · {} UPM",
                    font.info.family_name,
                    font.glyphs.len(),
                    font.masters.len(),
                    font.info.units_per_em
                );
                for axis in &font.axes {
                    println!(
                        "{}: {}–{} (default {})",
                        axis.tag, axis.min, axis.max, axis.default
                    );
                }
                for instance in &font.instances {
                    println!("{}: {:?}", instance.name, instance.location);
                }
                for warning in warnings {
                    eprintln!("WARN: {warning}");
                }
            }
            Ok(0)
        }
        Action::Export {
            font,
            format,
            variable,
            out,
            preset,
        } => {
            let (font, source, warnings) = load(font)?;
            let out = out.as_deref().map(full_path).transpose()?;
            let report = if let Some(name) = preset {
                kernest_export::export_preset(
                    &font,
                    name,
                    source.parent().unwrap_or(Path::new(".")),
                    out.as_deref(),
                )?
            } else {
                let formats = if format.is_empty() {
                    vec![Format::Ttf]
                } else {
                    format.clone()
                };
                let just_variable = formats.iter().all(|f| *f == Format::Variable);
                kernest_export::export(
                    &font,
                    &kernest_export::ExportOptions {
                        variable: *variable || formats.contains(&Format::Variable),
                        statics: !just_variable,
                        ttf: just_variable || formats.contains(&Format::Ttf),
                        otf: formats.contains(&Format::Otf),
                        woff2: formats.contains(&Format::Woff2),
                        remove_overlaps: true,
                        apply_metrics_keys: true,
                        autohint: true,
                        production_names: false,
                        out_dir: out.unwrap_or(full_path(Path::new("build"))?),
                    },
                )?
            };
            for warning in warnings {
                eprintln!("WARN: {warning}");
            }
            if args.json {
                print_json(&report)?;
            } else {
                for file in &report.files {
                    println!("{} ({} bytes)", file.path, file.bytes);
                }
                for problem in &report.problems {
                    eprintln!("FAIL: {problem}");
                }
                println!("Exported {} files", report.files.len());
            }
            Ok(u8::from(
                !report.problems.is_empty() || report.files.is_empty(),
            ))
        }
        Action::Check { path, profile } => {
            let report = kernest_check::check_paths(&[full_path(path)?], profile)?;
            if args.json {
                print_json(&report)?;
            } else {
                for check in &report.checks {
                    println!("{:?} {} — {}", check.status, check.id, check.title);
                    for message in &check.messages {
                        println!("  {}", message.message);
                    }
                }
                println!(
                    "{} FAIL · {} WARN · {} PASS · {} SKIP · {} ERROR",
                    report.failed, report.warnings, report.passed, report.skipped, report.errors
                );
            }
            Ok(report.exit_code())
        }
        Action::Convert { input, to, out } => {
            let (font, source, mut warnings) = load(input)?;
            let target = if let Some(path) = out {
                full_path(path)?
            } else {
                match to {
                    SourceFormat::Kernest => source.with_extension("kernest"),
                    SourceFormat::Glyphs => source.with_extension("glyphs"),
                    SourceFormat::Ufo => source.with_file_name(format!(
                        "{}-ufo",
                        source.file_stem().unwrap_or_default().to_string_lossy()
                    )),
                }
            };
            if target.symlink_metadata().is_ok() {
                return Err(format!(
                    "{} already exists. Choose a different output path.",
                    target.display()
                ));
            }
            if source.is_dir() && target.starts_with(&source) {
                return Err("Choose an output path outside the source font.".into());
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let files = match to {
                SourceFormat::Kernest => {
                    kernest_core::package::save(&font, &target).map_err(|e| e.to_string())?;
                    vec![target.to_string_lossy().to_string()]
                }
                SourceFormat::Glyphs => {
                    kernest_import::write_glyphs(&font, &target)?;
                    vec![target.to_string_lossy().to_string()]
                }
                SourceFormat::Ufo => {
                    let report = kernest_export::export_ufo(
                        &font,
                        &target,
                        &kernest_export::UfoExportOptions {
                            masters: vec![],
                            decompose_smart: false,
                            color_layers: true,
                        },
                    )?;
                    warnings.extend(report.problems);
                    report.files
                }
            };
            if args.json {
                print_json(&json!({"files":files,"warnings":warnings}))?;
            } else {
                for file in files {
                    println!("{file}");
                }
                for warning in warnings {
                    eprintln!("WARN: {warning}");
                }
            }
            Ok(0)
        }
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if args.json {
                let _ = print_json(&json!({"error":error}));
            } else {
                eprintln!("{error}");
            }
            ExitCode::from(2)
        }
    }
}
