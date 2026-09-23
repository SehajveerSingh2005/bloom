use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// Minimum time between background update checks. Manual checks bypass this.
const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
/// Network timeout for a single manifest request.
const CHECK_TIMEOUT_SECS: u64 = 10;
/// A release must be at least this old before auto-update installs it, so a
/// broken release cannot reach everyone within minutes of being published.
const MIN_AUTO_INSTALL_AGE_SECS: i64 = 24 * 60 * 60;
const STATE_FILE: &str = "update-state.json";

/// Serializes manifest requests so concurrent callers share a single network hit.
static CHECK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static UPDATE_BUSY: AtomicBool = AtomicBool::new(false);
static LAST_CHECK: Mutex<Option<UpdateCheckResult>> = Mutex::new(None);

#[derive(Clone, Default, Serialize)]
pub struct UpdateCheckResult {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct PersistedUpdateState {
    last_check: i64,
    app_version: String,
    version: String,
    date: String,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(STATE_FILE))
}

fn read_state(app: &AppHandle) -> PersistedUpdateState {
    state_path(app)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn write_state(app: &AppHandle, state: &PersistedUpdateState) {
    if let Some(path) = state_path(app) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(state) {
            let _ = std::fs::write(path, content);
        }
    }
}

fn result_from_state(state: &PersistedUpdateState) -> UpdateCheckResult {
    if state.version.is_empty() {
        return UpdateCheckResult::default();
    }
    UpdateCheckResult {
        available: true,
        version: Some(state.version.clone()),
        date: (!state.date.is_empty()).then(|| state.date.clone()),
        body: None,
    }
}

/// Returns a cached result only when the last check is recent and was made
/// against the version currently running (so updating invalidates the cache).
fn cached_result(app: &AppHandle) -> Option<UpdateCheckResult> {
    let state = read_state(app);
    if state.last_check == 0 || now_secs() - state.last_check >= CHECK_INTERVAL_SECS {
        return None;
    }
    if state.app_version != app.package_info().version.to_string() {
        return None;
    }
    Some(result_from_state(&state))
}

pub fn release_is_old_enough(result: &UpdateCheckResult) -> bool {
    match result.date.as_deref().and_then(parse_rfc3339_utc) {
        Some(published) => now_secs() - published >= MIN_AUTO_INSTALL_AGE_SECS,
        None => false,
    }
}

/// Checks for an update at most once per `CHECK_INTERVAL_SECS` unless `force`
/// is set. Development builds skip the automatic network check because their
/// version never tracks releases; manual checks still work.
pub async fn check(app: &AppHandle, force: bool) -> Result<UpdateCheckResult, String> {
    if !force {
        if let Some(cached) = cached_result(app) {
            return Ok(cached);
        }
        #[cfg(debug_assertions)]
        {
            return Ok(UpdateCheckResult::default());
        }
    }

    let _guard = CHECK_LOCK.lock().await;

    // Another caller may have completed a check while we waited for the lock.
    if !force {
        if let Some(cached) = cached_result(app) {
            return Ok(cached);
        }
    }

    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = tokio::time::timeout(Duration::from_secs(CHECK_TIMEOUT_SECS), updater.check())
        .await
        .map_err(|_| "update check timed out".to_string())?
        .map_err(|e| e.to_string())?;

    let result = match &update {
        Some(update) => UpdateCheckResult {
            available: true,
            version: Some(update.version.clone()),
            date: update
                .raw_json
                .get("pub_date")
                .and_then(|value| value.as_str())
                .map(str::to_string),
            body: update.body.clone(),
        },
        None => UpdateCheckResult::default(),
    };

    write_state(
        app,
        &PersistedUpdateState {
            last_check: now_secs(),
            app_version: app.package_info().version.to_string(),
            version: result.version.clone().unwrap_or_default(),
            date: result.date.clone().unwrap_or_default(),
        },
    );
    if let Ok(mut cached) = LAST_CHECK.lock() {
        *cached = Some(result.clone());
    }
    Ok(result)
}

/// Downloads and installs the available update. On Windows the updater plugin
/// launches the NSIS installer and terminates this process, which then
/// relaunches the app; on other platforms this returns after restarting.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if UPDATE_BUSY.swap(true, Ordering::SeqCst) {
        return Err("an update is already in progress".to_string());
    }
    let result = install_inner(app).await;
    UPDATE_BUSY.store(false, Ordering::SeqCst);
    result
}

async fn install_inner(app: &AppHandle) -> Result<(), String> {
    // Hold the check lock so a concurrent check cannot mutate state mid-install.
    let _guard = CHECK_LOCK.lock().await;

    let hook_handle = app.clone();
    let updater = app
        .updater_builder()
        .on_before_exit(move || {
            let _ = hook_handle.emit(
                "auto-update-status",
                serde_json::json!({ "status": "installing" }),
            );
            hook_handle.cleanup_before_exit();
        })
        .build()
        .map_err(|e| e.to_string())?;

    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;

    let _ = app.emit(
        "auto-update-status",
        serde_json::json!({ "status": "downloading", "progress": 0 }),
    );

    let progress_handle = app.clone();
    let downloaded = Arc::new(AtomicU64::new(0));
    let downloaded_cb = downloaded.clone();
    update
        .download_and_install(
            move |chunk_len, total| {
                let current =
                    downloaded_cb.fetch_add(chunk_len as u64, Ordering::Relaxed) + chunk_len as u64;
                if let Some(total) = total {
                    if total > 0 {
                        let progress = (current.saturating_mul(100) / total) as u32;
                        let _ = progress_handle.emit(
                            "auto-update-status",
                            serde_json::json!({ "status": "downloading", "progress": progress }),
                        );
                    }
                }
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    // Windows: `download_and_install` launches the installer and exits inside
    // the plugin, so this is only reached on other platforms.
    #[cfg(not(windows))]
    {
        let _ = app.emit(
            "auto-update-status",
            serde_json::json!({ "status": "done" }),
        );
        app.restart()
    }

    #[cfg(windows)]
    Ok(())
}

/// Startup entry point: always checks so the UI can show an update badge, and
/// auto-installs only when the user enabled it and the release has aged.
pub async fn run_startup_check(app: AppHandle) {
    let auto_update =
        crate::utils::get_setting_str(&app, "bloom-auto-update").as_deref() == Some("true");

    if auto_update {
        let _ = app.emit(
            "auto-update-status",
            serde_json::json!({ "status": "checking" }),
        );
    }

    let result = match check(&app, false).await {
        Ok(result) => result,
        Err(_) => {
            if auto_update {
                let _ = app.emit(
                    "auto-update-status",
                    serde_json::json!({ "status": "done" }),
                );
            }
            return;
        }
    };

    if !result.available {
        if auto_update {
            let _ = app.emit(
                "auto-update-status",
                serde_json::json!({ "status": "done" }),
            );
        }
        return;
    }

    let _ = app.emit("update-available", &result);

    if auto_update && release_is_old_enough(&result) {
        // On Windows this never returns: the installer exits the process.
        if install(&app).await.is_err() {
            let _ = app.emit(
                "auto-update-status",
                serde_json::json!({ "status": "done" }),
            );
        }
    } else if auto_update {
        let _ = app.emit(
            "auto-update-status",
            serde_json::json!({ "status": "done" }),
        );
    }
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle, force: bool) -> Result<UpdateCheckResult, String> {
    let result = check(&app, force).await?;
    if result.available {
        let _ = app.emit("update-available", &result);
    }
    Ok(result)
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    install(&app).await
}

#[tauri::command]
pub fn get_update_state(app: AppHandle) -> UpdateCheckResult {
    if let Ok(cached) = LAST_CHECK.lock() {
        if let Some(result) = cached.as_ref() {
            return result.clone();
        }
    }
    result_from_state(&read_state(&app))
}

fn parse_rfc3339_utc(value: &str) -> Option<i64> {
    if value.len() < 20 || !value.ends_with('Z') {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year: i64 = value.get(0..4)?.parse().ok()?;
    let month: i64 = value.get(5..7)?.parse().ok()?;
    let day: i64 = value.get(8..10)?.parse().ok()?;
    let hour: i64 = value.get(11..13)?.parse().ok()?;
    let minute: i64 = value.get(14..16)?.parse().ok()?;
    let second: i64 = value.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Howard Hinnant's days-from-civil algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::{days_from_civil, parse_rfc3339_utc};

    #[test]
    fn parses_tauri_action_pub_date() {
        assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_utc("2026-09-13T18:14:46Z"),
            Some(1_789_323_286)
        );
        assert_eq!(
            parse_rfc3339_utc("2026-09-13T18:14:46.123Z"),
            Some(1_789_323_286)
        );
    }

    #[test]
    fn rejects_non_utc_and_malformed_dates() {
        assert_eq!(parse_rfc3339_utc("2026-09-13T18:14:46+02:00"), None);
        assert_eq!(parse_rfc3339_utc("not-a-date"), None);
        assert_eq!(parse_rfc3339_utc("2026-13-40T99:99:99Z"), None);
    }

    #[test]
    fn computes_civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 9, 13), 20_709);
    }
}
