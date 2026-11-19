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
        available,
        checked_at: settings.last_checked,
        error: settings.error.clone(),
    }
}

fn release_version(bytes: &[u8]) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
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
    Ok(version.to_string())
}

fn fetch_release() -> Result<String, String> {
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
    release_version(&bytes)
}

#[tauri::command]
pub async fn check_updates(state: State<'_, SettingsState>) -> Result<UpdateInfo, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let mut request = false;
    let settings = settings::change(&state, |s| {
        request = reserve_request(&mut s.updates, now);
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
            Ok(version) => {
                s.updates.latest_version = Some(version);
                s.updates.error = None;
            }
            Err(error) => s.updates.error = Some(error),
        }
        Ok(())
    })?;
    Ok(info(&settings.updates))
}

#[cfg(test)]
mod tests {
    use super::*;

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
