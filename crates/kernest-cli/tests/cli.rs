use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kernest-cli"))
        .args(args)
        .output()
        .unwrap()
}
fn json(out: &Output) -> Value {
    assert!(
        !out.stdout.is_empty(),
        "CLI must produce a report: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn fixture(path: &Path) {
    kernest_core::package::save(&kernest_core::sample::sample_font(), path).unwrap();
}

#[test]
fn export_info_and_check_work_without_opening_a_window() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Source with spaces.kernest");
    fixture(&source);
    let info = run(&["info", source.to_str().unwrap(), "--json"]);
    assert!(info.status.success());
    assert_eq!(json(&info)["familyName"], "Kernest Sample");
    assert_eq!(json(&info)["glyphs"], 95);
    let outdir = dir.path().join("build");
    let export = run(&[
        "export",
        source.to_str().unwrap(),
        "--format",
        "ttf,otf,woff2",
        "--out",
        outdir.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        export.status.success(),
        "{}",
        String::from_utf8_lossy(&export.stderr)
    );
    assert_eq!(json(&export)["files"].as_array().unwrap().len(), 3);
    let check = run(&[
        "check",
        outdir.to_str().unwrap(),
        "--profile",
        "opentype",
        "--json",
    ]);
    let report = json(&check);
    assert_eq!(report["files"], 3);
    assert!(report["passed"].as_u64().unwrap() > 0);
    assert!(matches!(check.status.code(), Some(0 | 1)));
}

#[test]
fn convert_to_ufo_and_back_preserves_outline_and_refuses_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Source.kernest");
    fixture(&source);
    let output = dir.path().join("ufo-sources");
    let converted = run(&[
        "convert",
        source.to_str().unwrap(),
        "--to",
        "ufo",
        "--out",
        output.to_str().unwrap(),
        "--json",
    ]);
    assert!(converted.status.success());
    assert!(!json(&converted)["files"].as_array().unwrap().is_empty());
    let designspace = output.join("font.designspace");
    assert!(designspace.is_file());
    let back = dir.path().join("Back.kernest");
    let converted = run(&[
        "convert",
        designspace.to_str().unwrap(),
        "--to",
        "kernest",
        "--out",
        back.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        converted.status.success(),
        "{}",
        String::from_utf8_lossy(&converted.stderr)
    );
    let roundtrip = kernest_core::package::open(&back).unwrap();
    assert_eq!(roundtrip.info.family_name, "Kernest Sample");
    assert_eq!(roundtrip.glyph("H").unwrap().layers[0].width, 680.0);
    assert_eq!(roundtrip.glyph("H").unwrap().layers[0].shapes.len(), 3);
    let original = std::fs::read(back.join("font.json")).unwrap();
    let rejected = run(&[
        "convert",
        designspace.to_str().unwrap(),
        "--to",
        "kernest",
        "--out",
        back.to_str().unwrap(),
    ]);
    assert!(!rejected.status.success());
    assert_eq!(std::fs::read(back.join("font.json")).unwrap(), original);
}

#[test]
fn bad_fonts_fail_checks_and_bad_arguments_have_distinct_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.ttf");
    std::fs::write(&bad, b"broken").unwrap();
    let checked = run(&["check", bad.to_str().unwrap(), "--json"]);
    assert_eq!(checked.status.code(), Some(1));
    assert_eq!(json(&checked)["failed"], 1);
    assert_eq!(run(&["no-such-command"]).status.code(), Some(2));
    assert_eq!(run(&["export", "missing.kernest"]).status.code(), Some(2));
    assert_eq!(
        run(&["check", dir.path().join("empty").to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn named_preset_is_shared_with_gui_and_out_overrides_folder() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("Preset.kernest");
    let mut font = kernest_core::sample::sample_font();
    font.export_presets = vec![kernest_core::presets::ExportPreset {
        name: "web".into(),
        formats: vec![kernest_core::presets::ExportFormat::Woff2],
        instances: vec![],
        options: Default::default(),
        folder: "web-build".into(),
    }];
    kernest_core::package::save(&font, &source).unwrap();
    let report = run(&[
        "export",
        source.to_str().unwrap(),
        "--preset",
        "web",
        "--json",
    ]);
    assert!(report.status.success());
    assert_eq!(json(&report)["files"][0]["format"], "woff2");
    assert!(dir
        .path()
        .join("web-build/KernestSample-Regular.woff2")
        .is_file());
    let override_dir = dir.path().join("override");
    let report = run(&[
        "export",
        source.to_str().unwrap(),
        "--preset",
        "web",
        "--out",
        override_dir.to_str().unwrap(),
        "--json",
    ]);
    assert!(report.status.success());
    assert!(override_dir.join("KernestSample-Regular.woff2").is_file());
}
