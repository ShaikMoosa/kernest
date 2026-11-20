use crate::preferences::UpdateSettings;
use crate::settings::{self, SettingsState};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;

fn reserve_request(settings: &mut UpdateSettings, now: u64) -> bool {
    if !settings.enabled
        || settings
            .last_checked
            .is_some_and(|last| now.saturating_sub(last) < 86400)
    {
        return false;
    }
    settings.last_checked = Some(now);
    true
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current_version: String,
    latest_version: Option<String>,
    /// What's new, in a line or two.
    notes: Option<String>,
    available: bool,
    checked_at: Option<u64>,
    error: Option<String>,
}

fn info(settings: &UpdateSettings) -> UpdateInfo {
    let current = env!("CARGO_PKG_VERSION");
    let available = settings.enabled
        && settings
            .latest_version
            .as_deref()
            .and_then(|v| semver::Version::parse(v).ok())
            .zip(semver::Version::parse(current).ok())
            .is_some_and(|(latest, current)| latest > current);
    UpdateInfo {
        current_version: current.into(),
        latest_version: settings.latest_version.clone(),
        notes: settings.latest_notes.clone(),
        available,
        checked_at: settings.last_checked,
        error: settings.error.clone(),
    }
}

/// A short summary of release notes: the first plain lines.
fn notes_summary(body: &str) -> Option<String> {
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('#')
                && !l.starts_with("Download")
                && !l.starts_with("- `")
        })
        .take(2)
        .collect();
    let text = lines
        .join(" ")
        .replace("**", "")
        .trim_start_matches("- ")
        .to_string();
    if text.is_empty() {
        return None;
    }
    let mut out: String = text.chars().take(160).collect();
    if text.chars().count() > 160 {
        out.push('\u{2026}');
    }
    Some(out)
}

#[cfg(test)]
fn release_version(bytes: &[u8]) -> Result<String, String> {
    release_info(bytes).map(|(v, _)| v)
}

fn release_info(bytes: &[u8]) -> Result<(String, Option<String>), String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        body: Option<String>,
    }
    let release: Release = serde_json::from_slice(bytes)
        .map_err(|_| "The release service returned an invalid response.")?;
    let version = semver::Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )
    .map_err(|_| "The release service returned an invalid version.")?;
    if release.draft || release.prerelease || !version.pre.is_empty() {
        return Err("The release service did not return a stable release.".into());
    }
    // "Kernest 2.10 \u{2014} sbix bitmaps and feature tokens": after the dash is what's new.
    let title = release
        .name
        .as_deref()
        .and_then(|n| {
            n.split_once('\u{2014}')
                .map(|(_, rest)| rest.trim().to_string())
        })
        .filter(|t| !t.is_empty());
    let notes = title.or_else(|| release.body.as_deref().and_then(notes_summary));
    Ok((version.to_string(), notes))
}

fn fetch_release() -> Result<(String, Option<String>), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("Kernest/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("Could not check for updates: {e}"))?;
    let response = client
        .get("https://api.github.com/repos/ShaikMoosa/kernest/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Could not check for updates: {e}"))?;
    let mut bytes = Vec::new();
    response
        .take(1048577)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not check for updates: {e}"))?;
    if bytes.len() > 1048576 {
        return Err("The release service returned an oversized response.".into());
    }
    release_info(&bytes)
}

/// Checks for a newer release at most once a day while automatic checks are on;
/// `force` (Help › Check for Updates…) checks now, even when they are off.
#[tauri::command]
pub async fn check_updates(
    state: State<'_, SettingsState>,
    force: Option<bool>,
) -> Result<UpdateInfo, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let force = force.unwrap_or(false);
    let mut request = false;
    let settings = settings::change(&state, |s| {
        request = reserve_request(&mut s.updates, now);
        if force && !request {
            s.updates.last_checked = Some(now);
            request = true;
        }
        Ok(())
    })?;
    if !request {
        return Ok(info(&settings.updates));
    }
    let result = tauri::async_runtime::spawn_blocking(fetch_release)
        .await
        .map_err(|e| format!("Could not check for updates: {e}"))?;
    let settings = settings::change(&state, |s| {
        match result {
            Ok((version, notes)) => {
                s.updates.latest_version = Some(version);
                s.updates.latest_notes = notes;
                s.updates.error = None;
            }
            Err(error) => s.updates.error = Some(error),
        }
        Ok(())
    })?;
    let mut out = info(&settings.updates);
    if force {
        // Asked for now: say whether there is a newer version, checks on or off.
        out.available = settings
            .updates
            .latest_version
            .as_deref()
            .and_then(|v| semver::Version::parse(v).ok())
            .zip(semver::Version::parse(env!("CARGO_PKG_VERSION")).ok())
            .is_some_and(|(latest, current)| latest > current);
    }
    Ok(out)
}

// --- Download & install ------------------------------------------------------------

/// The setup program for this PC in a release.
fn setup_name(version: &str) -> String {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    format!("Kernest_{version}_{arch}-setup.exe")
}

/// The SHA-256 a `SHA256SUMS.txt` lists for `name`.
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        let sum = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && sum.len() == 64).then(|| sum.to_ascii_lowercase())
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn download(url: &str, limit: u64) -> Result<Vec<u8>, String> {
    // Release files redirect to GitHub's download host; only https is followed.
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.url().scheme() != "https" || a.previous().len() > 5 {
                a.stop()
            } else {
                a.follow()
            }
        }))
        .user_agent(concat!("Kernest/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("Could not download the update: {e}"))?;
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Could not download the update: {e}"))?;
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Could not download the update: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err("The update is larger than expected, so it wasn't installed.".into());
    }
    Ok(bytes)
}

/// Downloads the setup program for `version`, checks it against the release's
/// SHA256SUMS.txt, starts it and closes Kernest.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle, version: String) -> Result<(), String> {
    let parsed = semver::Version::parse(&version).map_err(|_| "That isn't a version number.")?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
    if parsed <= current || !parsed.pre.is_empty() {
        return Err("This version is not newer than the one you have.".into());
    }
    let path =
        tauri::async_runtime::spawn_blocking(move || -> Result<std::path::PathBuf, String> {
            use sha2::{Digest, Sha256};
            let base = format!("https://github.com/ShaikMoosa/kernest/releases/download/v{parsed}");
            let name = setup_name(&parsed.to_string());
            let sums = download(&format!("{base}/SHA256SUMS.txt"), 64 * 1024)?;
            let sums = String::from_utf8_lossy(&sums);
            let want = expected_sum(&sums, &name)
                .ok_or("The release doesn't list a checksum for this PC's setup program.")?;
            let setup = download(&format!("{base}/{name}"), 400 * 1024 * 1024)?;
            if hex(&Sha256::digest(&setup)) != want {
                return Err(
                    "The download didn't match the release's checksum, so it wasn't installed."
                        .into(),
                );
            }
            let dir = std::env::temp_dir().join("Kernest Update");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(&name);
            std::fs::write(&path, &setup).map_err(|e| format!("Could not save the update: {e}"))?;
            Ok(path)
        })
        .await
        .map_err(|e| format!("The update stopped unexpectedly: {e}"))??;
    std::process::Command::new(&path)
        .spawn()
        .map_err(|e| format!("Could not start the setup program: {e}"))?;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_and_notes() {
        let sums = "abc  other.exe\n0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF  Kernest_2.10.0_x64-setup.exe\n";
        assert_eq!(
            expected_sum(sums, "Kernest_2.10.0_x64-setup.exe").as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert_eq!(expected_sum(sums, "missing.exe"), None);
        let (v, notes) = release_info(
            "{\"tag_name\":\"v2.10.0\",\"draft\":false,\"prerelease\":false,\"name\":\"Kernest 2.10 \u{2014} sbix bitmaps and feature tokens\",\"body\":\"x\"}".as_bytes(),
        )
        .unwrap();
        assert_eq!(v, "2.10.0");
        assert_eq!(notes.as_deref(), Some("sbix bitmaps and feature tokens"));
        assert_eq!(
            notes_summary("## Masks\n\n- **Masks:** tick Mask on a path.\n").as_deref(),
            Some("Masks: tick Mask on a path.")
        );
    }

    #[test]
    fn only_stable_release_responses_are_accepted() {
        assert_eq!(
            release_version(br#"{"tag_name":"v2.5.0","draft":false,"prerelease":false}"#).unwrap(),
            "2.5.0"
        );
        for bytes in [
            br#"{"tag_name":"v2.5.0-beta.1","draft":false,"prerelease":true}"#.as_slice(),
            br#"{"tag_name":"v2.5.0","draft":true,"prerelease":false}"#.as_slice(),
            br#"{"tag_name":"not-a-version","draft":false,"prerelease":false}"#.as_slice(),
            b"not json".as_slice(),
        ] {
            assert!(release_version(bytes).is_err());
        }
        let s = UpdateSettings {
            enabled: true,
            latest_version: Some("999.0.0".into()),
            ..Default::default()
        };
        assert!(info(&s).available);
        assert!(
            !info(&UpdateSettings {
                enabled: false,
                ..s
            })
            .available
        );
    }

    #[test]
    fn disabled_checks_never_reserve_a_request() {
        let mut settings = UpdateSettings::default();
        for now in [0, 100, 1000000] {
            assert!(!reserve_request(&mut settings, now));
            assert_eq!(settings.last_checked, None);
        }
    }

    #[test]
    fn opt_in_checks_reserve_at_most_once_per_day() {
        let mut settings = UpdateSettings {
            enabled: true,
            ..Default::default()
        };
        assert!(reserve_request(&mut settings, 100));
        assert_eq!(settings.last_checked, Some(100));
        assert!(!reserve_request(&mut settings, 100));
        assert!(!reserve_request(&mut settings, 86499));
        assert!(reserve_request(&mut settings, 86500));
        settings.enabled = false;
        assert!(!reserve_request(&mut settings, 200000));
        settings.enabled = true;
        assert!(!reserve_request(&mut settings, 10));
    }
}
