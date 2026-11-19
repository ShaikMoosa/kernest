use fontspector_checkapi::{
    Context, ProfileProvider, Registry, StatusCode, TestFont, Testable, TestableCollection,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Pass,
    Warn,
    Fail,
    Skip,
    Info,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityCheck {
    pub id: String,
    pub title: String,
    pub status: Severity,
    pub file: Option<String>,
    #[serde(default)]
    pub glyphs: Vec<String>,
    pub messages: Vec<CheckMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckMessage {
    pub code: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityReport {
    pub profile: String,
    pub files: usize,
    pub checks: Vec<QualityCheck>,
    pub failed: usize,
    pub warnings: usize,
    pub passed: usize,
    pub skipped: usize,
    pub errors: usize,
}

impl QualityReport {
    pub fn exit_code(&self) -> u8 {
        if self.errors > 0 {
            2
        } else if self.failed > 0 {
            1
        } else {
            0
        }
    }

    fn push(&mut self, check: QualityCheck) {
        match check.status {
            Severity::Fail => self.failed += 1,
            Severity::Warn => self.warnings += 1,
            Severity::Pass => self.passed += 1,
            Severity::Skip => self.skipped += 1,
            Severity::Error => self.errors += 1,
            Severity::Info => {}
        }
        self.checks.push(check);
    }
}

fn severity(status: StatusCode) -> Severity {
    match status {
        StatusCode::Pass => Severity::Pass,
        StatusCode::Warn => Severity::Warn,
        StatusCode::Fail | StatusCode::Fatal => Severity::Fail,
        StatusCode::Skip => Severity::Skip,
        StatusCode::Info => Severity::Info,
        StatusCode::Error => Severity::Error,
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn font_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut paths = BTreeSet::new();
    let mut pending = inputs.to_vec();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in std::fs::read_dir(&path)
                .map_err(|e| format!("Could not read {}: {e}", path.display()))?
            {
                let entry = entry.map_err(|e| e.to_string())?;
                let kind = entry.file_type().map_err(|e| e.to_string())?;
                if kind.is_dir()
                    || (kind.is_file()
                        && matches!(extension(&entry.path()).as_str(), "ttf" | "otf" | "woff2"))
                {
                    pending.push(entry.path());
                }
            }
        } else {
            if !matches!(extension(&path).as_str(), "ttf" | "otf" | "woff2") {
                return Err(
                    "Choose a TTF, OTF or WOFF2 font, or a folder of exported fonts.".into(),
                );
            }
            paths.insert(
                path.canonicalize()
                    .map_err(|e| format!("Could not read {}: {e}", path.display()))?,
            );
        }
    }
    if paths.is_empty() {
        return Err("No exported fonts were found to check.".into());
    }
    Ok(paths.into_iter().collect())
}

pub fn check_paths(inputs: &[PathBuf], profile_name: &str) -> Result<QualityReport, String> {
    if !matches!(profile_name, "opentype" | "universal") {
        return Err(format!("Unknown font-check profile \"{profile_name}\"."));
    }
    let paths = font_files(inputs)?;
    let mut report = QualityReport {
        profile: profile_name.into(),
        files: paths.len(),
        ..Default::default()
    };
    let mut registry = Registry::new();
    fontspector_profile_opentype::OpenType
        .register(&mut registry)
        .map_err(|e| e.to_string())?;
    fontspector_profile_universal::Universal
        .register(&mut registry)
        .map_err(|e| e.to_string())?;
    let profile = registry
        .get_profile(profile_name)
        .ok_or("The font-check profile is unavailable.")?;
    let mut groups: BTreeMap<(PathBuf, String), Vec<(Testable, String)>> = BTreeMap::new();
    for path in paths {
        let original = path.to_string_lossy().to_string();
        let bytes = std::fs::read(&path).map_err(|e| format!("Could not read {original}: {e}"))?;
        let ext = extension(&path);
        let decoded = if ext == "woff2" {
            kernest_import::decode_woff2(&bytes)
        } else {
            Ok(bytes)
        };
        let bytes = match decoded {
            Ok(bytes) if TestFont::new_from_data(&path, &bytes).is_ok() => bytes,
            _ => {
                report.push(QualityCheck {
                    id: "kernest/readable_font".into(),
                    title: "The exported font can be read".into(),
                    status: Severity::Fail,
                    file: Some(original),
                    glyphs: vec![],
                    messages: vec![CheckMessage {
                        code: Some("invalid-font".into()),
                        message: "The file is not a readable OpenType font.".into(),
                    }],
                });
                continue;
            }
        };
        // Fontspector matches sfnt extensions; WOFF2 is decoded in memory, never rewritten.
        let sfnt_path = path.with_extension(if bytes.starts_with(b"OTTO") {
            "otf"
        } else {
            "ttf"
        });
        groups
            .entry((path.parent().unwrap_or(Path::new(".")).to_path_buf(), ext))
            .or_default()
            .push((Testable::new_with_contents(sfnt_path, bytes), original));
    }
    // Each output format is its own family, so equivalent TTF/OTF exports aren't duplicate styles.
    for ((parent, _), files) in groups {
        let originals: HashMap<_, _> = files
            .iter()
            .map(|(f, original)| (f.filename.to_string_lossy().to_string(), original.clone()))
            .collect();
        let collection = TestableCollection::from_testables(
            files.into_iter().map(|(f, _)| f).collect(),
            Some(parent.to_string_lossy().to_string()),
        );
        let testables: Vec<_> = collection.collection_and_files().collect();
        let context = Context {
            skip_network: true,
            network_timeout: Some(1),
            ..Default::default()
        };
        for (section, testable, check, context) in
            profile.check_order(&[], &[], &registry, context, &HashMap::new(), &testables)
        {
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                check.run(testable, &context, Some(&section))
            }));
            match run {
                Ok(Some(result)) => {
                    let file = result
                        .filename
                        .as_ref()
                        .and_then(|f| originals.get(f).cloned())
                        .or(result.filename.clone());
                    report.push(QualityCheck {
                        id: result.check_id.clone(),
                        title: result.check_name.clone(),
                        status: severity(result.worst_status()),
                        file,
                        glyphs: result
                            .subresults
                            .iter()
                            .flat_map(|s| &s.metadata)
                            .filter_map(|m| match m {
                                fontspector_checkapi::Metadata::GlyphProblem {
                                    glyph_name, ..
                                } => Some(glyph_name.clone()),
                                _ => None,
                            })
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect(),
                        messages: result
                            .subresults
                            .into_iter()
                            .filter_map(|s| {
                                s.message.map(|message| CheckMessage {
                                    code: s.code,
                                    message,
                                })
                            })
                            .collect(),
                    });
                }
                Ok(None) => {}
                Err(_) => report.push(QualityCheck {
                    id: check.id.into(),
                    title: check.title.into(),
                    status: Severity::Error,
                    file: match testable {
                        fontspector_checkapi::TestableType::Single(f) => originals
                            .get(&f.filename.to_string_lossy().to_string())
                            .cloned(),
                        _ => None,
                    },
                    glyphs: vec![],
                    messages: vec![CheckMessage {
                        code: Some("checker-error".into()),
                        message: "This check stopped unexpectedly; its result is unknown.".into(),
                    }],
                }),
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let font = kernest_core::sample::sample_font();
        let mut opts = kernest_export::ExportOptions::ttf(false, true, dir);
        opts.otf = true;
        opts.woff2 = true;
        let report = kernest_export::export(&font, &opts).unwrap();
        assert!(report.problems.is_empty());
        report.files.into_iter().map(|f| f.path.into()).collect()
    }

    #[test]
    fn runs_real_checks_on_each_output_without_changing_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = fixture(dir.path());
        let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let report = check_paths(&paths, "universal").unwrap();
        assert_eq!(report.files, 3);
        assert!(report.passed > 0);
        assert!(report
            .checks
            .iter()
            .any(|c| c.id == "opentype/font_version"));
        let network: Vec<_> = report
            .checks
            .iter()
            .filter(|c| c.id == "fontdata_namecheck")
            .collect();
        assert_eq!(network.len(), 3);
        assert!(network.iter().all(|c| c.status == Severity::Skip));
        assert!(!report
            .checks
            .iter()
            .any(|c| c.id == "opentype/family/unique_names" && c.status == Severity::Fail));
        assert!(report
            .checks
            .iter()
            .any(|c| c.file.as_deref().is_some_and(|f| f.ends_with(".woff2"))));
        for (path, bytes) in paths.iter().zip(before) {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn malformed_fonts_fail_and_empty_inputs_are_not_a_pass() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_paths(&[], "opentype").is_err());
        assert!(check_paths(&[dir.path().to_path_buf()], "opentype").is_err());
        let path = dir.path().join("bad.ttf");
        std::fs::write(&path, b"not a font").unwrap();
        let report = check_paths(&[path], "opentype").unwrap();
        assert_eq!(report.failed, 1);
        assert_eq!(report.passed, 0);
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn directory_checks_find_fonts_in_nested_export_folders() {
        let dir = tempfile::tempdir().unwrap();
        fixture(&dir.path().join("web"));
        std::fs::write(dir.path().join("notes.txt"), "not a font").unwrap();
        let report = check_paths(&[dir.path().into()], "opentype").unwrap();
        assert_eq!(report.files, 3);
        assert!(check_paths(&[dir.path().into()], "unknown-profile").is_err());
    }

    #[test]
    fn warnings_do_not_fail_a_build_but_errors_do() {
        let mut report = QualityReport {
            warnings: 3,
            passed: 1,
            ..Default::default()
        };
        assert_eq!(report.exit_code(), 0);
        report.failed = 1;
        assert_eq!(report.exit_code(), 1);
        report.failed = 0;
        report.errors = 1;
        assert_eq!(report.exit_code(), 2);
    }
}
