#[tauri::command]
pub async fn check_fonts(
    paths: Vec<String>,
    profile: Option<String>,
) -> Result<kernest_check::QualityReport, String> {
    let paths = paths
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    let profile = profile.unwrap_or_else(|| "universal".into());
    tauri::async_runtime::spawn_blocking(move || kernest_check::check_paths(&paths, &profile))
        .await
        .map_err(|e| format!("Font checks stopped unexpectedly: {e}"))?
}

#[tauri::command]
pub fn save_quality_report(
    path: String,
    report: kernest_check::QualityReport,
) -> Result<String, String> {
    let path = std::path::PathBuf::from(path);
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        return Err("Choose a full path ending in .json for the report.".into());
    }
    if path.ancestors().any(|p| {
        p.extension().is_some_and(|e| {
            e.eq_ignore_ascii_case("kernest")
                || e.eq_ignore_ascii_case("ufo")
                || e.eq_ignore_ascii_case("glyphspackage")
        })
    }) {
        return Err("Save the report outside the font source package.".into());
    }
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("Could not save the report: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_json_and_refuses_source_package_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checks.json");
        save_quality_report(
            path.to_string_lossy().into(),
            kernest_check::QualityReport {
                passed: 2,
                files: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["passed"], 2);
        let path = dir.path().join("Source.kernest/font.json");
        assert!(save_quality_report(path.to_string_lossy().into(), Default::default()).is_err());
        assert!(save_quality_report("relative.json".into(), Default::default()).is_err());
    }
}
