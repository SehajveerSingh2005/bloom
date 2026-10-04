//! Bloom's side of the optional AI agent (bloom-ai.exe; see
//! docs/superpowers/plans/2026-10-04-bloom-ai-sidecar.md). Bloom starts the
//! agent on first use, relays its events to the webviews as `ai-event`, does
//! the Bloom actions it asks for, and removes it for good on "Delete AI".

use crate::types::AppInfo;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// VK_RMENU: Right Alt.
pub const DEFAULT_HOTKEY_VK: u32 = 0xA5;

/// Virtual-key code of the push-to-talk key, read by the keyboard hook on
/// every key event. 0 while AI is off or deleted.
pub static HOTKEY_VK: AtomicU32 = AtomicU32::new(0);

struct Sidecar {
    child: Child,
    stdin: ChildStdin,
}

static SIDECAR: Mutex<Option<Sidecar>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();
/// Hotkey presses, handled in order on one thread: a quick tap must never
/// deliver its release before its press.
static HOTKEY_TX: OnceLock<Sender<bool>> = OnceLock::new();
static NEXT_TASK: AtomicU64 = AtomicU64::new(0);
/// "Hello Janice" is on: the agent was told `wake_on`, and is told again
/// whenever it is restarted.
static WAKE: AtomicBool = AtomicBool::new(false);

fn ai_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join("ai"))
}

fn exe_path(app: &AppHandle) -> Option<PathBuf> {
    ai_dir(app).map(|d| d.join("bloom-ai.exe"))
}

/// Written by the agent once the user has taught it "Hello Janice".
fn wake_model(app: &AppHandle) -> Option<PathBuf> {
    ai_dir(app).map(|d| d.join("wake").join("hello-janice.rpw"))
}

fn deleted_flag(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("ai_deleted.flag"))
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

fn is_deleted(app: &AppHandle) -> bool {
    deleted_flag(app).is_some_and(|p| p.exists())
}

fn enabled(app: &AppHandle) -> bool {
    !is_deleted(app) && crate::utils::get_setting_str(app, "bloom-ai-enabled").as_deref() == Some("true")
}

/// The window that shows the panel: the dock when the notch is merged into it.
fn surface(app: &AppHandle) -> &'static str {
    if crate::utils::info_centre_enabled(app) {
        "dock"
    } else {
        "main"
    }
}

/// Once from setup, after the settings cache is loaded.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    // An install after "Delete AI" may have put the agent back: remove it again.
    if is_deleted(app) {
        if let Some(dir) = ai_dir(app).filter(|d| d.exists()) {
            // Finish a delete that failed halfway, off the setup thread: wipe the
            // keys, then the files. A failed wipe leaves everything for the next start.
            let exe = exe_path(app).filter(|p| p.exists());
            std::thread::spawn(move || {
                if exe.is_none_or(|exe| wipe(&exe).is_ok()) {
                    let _ = std::fs::remove_dir_all(dir);
                }
            });
        }
    }
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let handle = app.clone();
    std::thread::spawn(move || {
        for down in rx {
            hotkey(&handle, down);
        }
    });
    let _ = HOTKEY_TX.set(tx);
    sync_from_settings();
}

/// After any settings change: arms or disarms the hotkey, turns "Hello Janice"
/// on or off, and stops the agent when AI is turned off. Must be called
/// without the settings lock held.
pub fn sync_from_settings() {
    let Some(app) = APP.get() else { return };
    let on = enabled(app);
    let vk = if on {
        crate::utils::get_setting_str(app, "bloom-ai-hotkey")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| (1..=254).contains(v))
            .unwrap_or(DEFAULT_HOTKEY_VK)
    } else {
        0
    };
    HOTKEY_VK.store(vk, Ordering::Relaxed);
    let wake = on && crate::utils::get_setting_str(app, "bloom-ai-wake").as_deref() == Some("true");
    if wake != WAKE.load(Ordering::Relaxed) {
        // Sent before the flag flips, so a fresh agent isn't told twice.
        let _ = if wake {
            send(app, json!({ "type": "wake_on" }))
        } else {
            send_if_running(json!({ "type": "wake_off" }))
        };
        WAKE.store(wake, Ordering::Relaxed);
    }
    if !on {
        stop();
    }
}

/// Kills the agent. Nothing of it keeps running or holds memory afterwards.
pub fn stop() {
    let taken = SIDECAR.lock().ok().and_then(|mut slot| slot.take());
    if let Some(mut sidecar) = taken {
        let _ = sidecar.child.kill();
        let _ = sidecar.child.wait();
    }
}

/// The agent removes its own Credential Manager entries.
fn wipe(exe: &std::path::Path) -> Result<(), String> {
    match Command::new(exe).arg("--wipe").creation_flags(CREATE_NO_WINDOW).status() {
        Ok(status) if status.success() => Ok(()),
        _ => Err("Couldn't remove the AI's saved keys; nothing was deleted. Try again.".into()),
    }
}

fn spawn(app: &AppHandle) -> Result<Sidecar, String> {
    let exe = exe_path(app).filter(|p| p.exists()).ok_or("The AI agent isn't installed.")?;
    let mut child = Command::new(&exe)
        .arg("--settings")
        .arg(settings_path(app).unwrap_or_default())
        .current_dir(exe.parent().unwrap_or(exe.as_path()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Couldn't start the AI agent: {e}"))?;
    let stdin = child.stdin.take().ok_or("The AI agent has no input pipe.")?;
    let stdout = child.stdout.take().ok_or("The AI agent has no output pipe.")?;
    let handle = app.clone();
    std::thread::spawn(move || relay(handle, stdout));
    Ok(Sidecar { child, stdin })
}

/// The agent's output: `bloom` requests are carried out here, everything else
/// goes to the webviews.
fn relay(app: AppHandle, stdout: ChildStdout) {
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message["type"] == "bloom" {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let action = message["action"].as_str().unwrap_or_default();
                let (ok, detail) = match bloom_action(&app, action, &message["value"]).await {
                    Ok(detail) => (true, detail),
                    Err(detail) => (false, detail),
                };
                let _ = send_if_running(json!({ "type": "bloom_result", "id": message["id"], "ok": ok, "detail": detail }));
            });
        } else {
            // "Hello Janice": open the panel the way the hotkey does.
            if message["type"] == "wake" {
                let _ = app.emit_to(surface(&app), "ai-open", json!({ "recording": true }));
            }
            let _ = app.emit("ai-event", message);
        }
    }
    let _ = app.emit("ai-event", json!({ "type": "exited" }));
}

/// Sends one message, starting the agent first if needed.
fn send(app: &AppHandle, message: Value) -> Result<(), String> {
    if !enabled(app) {
        return Err("Bloom AI is off. Turn it on in Settings > AI.".into());
    }
    let mut slot = SIDECAR.lock().map_err(|_| "AI state is unavailable.")?;
    // An agent that exited on its own is replaced.
    if slot.as_mut().is_some_and(|s| !matches!(s.child.try_wait(), Ok(None))) {
        *slot = None;
    }
    if slot.is_none() {
        *slot = Some(spawn(app)?);
        if WAKE.load(Ordering::Relaxed) {
            write_line(&mut slot, json!({ "type": "wake_on" }))?;
        }
    }
    write_line(&mut slot, message)
}

/// For answers and cancels: never starts the agent just to deliver them.
fn send_if_running(message: Value) -> Result<(), String> {
    let mut slot = SIDECAR.lock().map_err(|_| "AI state is unavailable.")?;
    write_line(&mut slot, message)
}

fn write_line(slot: &mut Option<Sidecar>, message: Value) -> Result<(), String> {
    let Some(sidecar) = slot.as_mut() else {
        return Err("The AI agent isn't running.".into());
    };
    let mut line = message.to_string();
    line.push('\n');
    let written = sidecar.stdin.write_all(line.as_bytes()).and_then(|()| sidecar.stdin.flush());
    if let Err(e) = written {
        *slot = None;
        return Err(format!("The AI agent stopped: {e}"));
    }
    Ok(())
}

/// From the keyboard hook (services.rs). Never blocks the hook.
pub fn hotkey_event(down: bool) {
    if let Some(tx) = HOTKEY_TX.get() {
        let _ = tx.send(down);
    }
}

fn hotkey(app: &AppHandle, down: bool) {
    if down {
        let _ = app.emit_to(surface(app), "ai-open", json!({ "recording": true }));
        if let Err(message) = send(app, json!({ "type": "record_start" })) {
            let _ = app.emit("ai-event", json!({ "type": "error", "task": 0, "message": message }));
        }
    } else {
        let task = NEXT_TASK.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = send_if_running(json!({ "type": "record_stop", "task": task }));
    }
}

/// Best installed-app match: exact name, then prefix, then anywhere.
fn find_app<'a>(apps: &'a [AppInfo], query: &str) -> Option<&'a AppInfo> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let name = |a: &AppInfo| a.name.to_lowercase();
    apps.iter()
        .find(|a| name(a) == q)
        .or_else(|| apps.iter().find(|a| name(a).starts_with(&q)))
        .or_else(|| apps.iter().find(|a| name(a).contains(&q)))
}

/// 40, 40.5 or "40%".
fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().trim_end_matches('%').trim().parse().ok())
        .filter(|v| v.is_finite())
}

/// true/false or "on"/"off".
fn flag(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| match value.as_str()?.trim().to_ascii_lowercase().as_str() {
        "on" | "true" => Some(true),
        "off" | "false" => Some(false),
        _ => None,
    })
}

/// What the agent's `bloom_control` and `open` tools ask Bloom to do. Reuses
/// the same commands the notch and dock call.
async fn bloom_action(app: &AppHandle, action: &str, value: &Value) -> Result<String, String> {
    use crate::commands;
    match action {
        "volume" => {
            let v = number(value).ok_or("volume needs a number from 0 to 100")?.clamp(0.0, 100.0);
            commands::set_volume((v / 100.0) as f32);
            Ok(format!("Volume is {v:.0}%."))
        }
        "brightness" => {
            let v = number(value).ok_or("brightness needs a number from 0 to 100")?.clamp(0.0, 100.0);
            commands::set_brightness(app.clone(), v as u32);
            Ok(format!("Brightness is {v:.0}%."))
        }
        "media" => {
            match value.as_str() {
                Some("play_pause") => commands::media_play_pause(),
                Some("next") => commands::media_next(),
                Some("previous") => commands::media_previous(),
                _ => return Err("media needs play_pause, next or previous".into()),
            }
            Ok("Done.".into())
        }
        "wifi" => {
            let on = flag(value).ok_or("wifi needs on or off")?;
            commands::set_wifi_state(on).await?;
            Ok(format!("Wi-Fi is {}.", if on { "on" } else { "off" }))
        }
        "bluetooth" => {
            let on = flag(value).ok_or("bluetooth needs on or off")?;
            commands::set_bluetooth_state(on).await?;
            Ok(format!("Bluetooth is {}.", if on { "on" } else { "off" }))
        }
        "open_app" => {
            let query = value.as_str().unwrap_or_default();
            let found = {
                let apps = crate::state::INSTALLED_APPS_CACHE
                    .get()
                    .and_then(|cache| cache.lock().ok())
                    .ok_or("The app list isn't ready yet.")?;
                find_app(&apps, query).map(|a| (a.name.clone(), a.path.clone()))
            };
            let (name, path) = found.ok_or_else(|| format!("No installed app called {query}."))?;
            commands::open_app(app.clone(), path).await;
            Ok(format!("Opened {name}."))
        }
        _ => Err(format!("unknown action {action}")),
    }
}

fn strip_ai_keys(settings: &mut serde_json::Map<String, Value>) {
    settings.retain(|key, _| !key.starts_with("bloom-ai-"));
}

fn remove_ai_settings(app: &AppHandle) -> Result<(), String> {
    let path = settings_path(app).ok_or("Can't find settings.json.")?;
    let Ok(content) = std::fs::read_to_string(&path) else { return Ok(()) };
    let mut settings: serde_json::Map<String, Value> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    strip_ai_keys(&mut settings);
    std::fs::write(&path, Value::Object(settings).to_string()).map_err(|e| e.to_string())?;
    // Updates the cache and tells every window the keys are gone.
    crate::commands::reload_settings(app, &path);
    Ok(())
}

#[tauri::command]
pub fn ai_status(app: AppHandle) -> Value {
    json!({
        "installed": exe_path(&app).is_some_and(|p| p.exists()),
        "deleted": is_deleted(&app),
        "enabled": enabled(&app),
        "running": SIDECAR
            .lock()
            .map(|mut slot| slot.as_mut().is_some_and(|s| matches!(s.child.try_wait(), Ok(None))))
            .unwrap_or(false),
        "wake_trained": wake_model(&app).is_some_and(|p| p.exists()),
    })
}

#[tauri::command]
pub fn ai_prompt(app: AppHandle, text: String) -> Result<u64, String> {
    let task = NEXT_TASK.fetch_add(1, Ordering::Relaxed) + 1;
    send(&app, json!({ "type": "prompt", "task": task, "text": text }))?;
    Ok(task)
}

#[tauri::command]
pub fn ai_cancel() {
    let _ = send_if_running(json!({ "type": "cancel" }));
}

#[tauri::command]
pub fn ai_confirm(id: u64, approved: bool) -> Result<(), String> {
    send_if_running(json!({ "type": "confirm_reply", "id": id, "approved": approved }))
}

/// Secrets go straight to the agent, which keeps them in Credential Manager.
#[tauri::command]
pub fn ai_set_secret(app: AppHandle, name: String, value: String) -> Result<(), String> {
    send(&app, json!({ "type": "set_secret", "name": name, "value": value }))
}

#[tauri::command]
pub fn ai_outlook_login(app: AppHandle) -> Result<(), String> {
    send(&app, json!({ "type": "outlook_login" }))
}

/// Records "Hello Janice" sample `index` (1 starts over); `enroll_saved` follows.
#[tauri::command]
pub fn ai_enroll_sample(app: AppHandle, index: u32) -> Result<(), String> {
    send(&app, json!({ "type": "enroll_sample", "index": index }))
}

/// Builds the wake word from the samples; `enroll_done` follows.
#[tauri::command]
pub fn ai_enroll_build(app: AppHandle) -> Result<(), String> {
    send(&app, json!({ "type": "enroll_build" }))
}

/// The dock's AI button: show the panel with its text box.
#[tauri::command]
pub fn ai_open(app: AppHandle) {
    if enabled(&app) {
        let _ = app.emit_to(surface(&app), "ai-open", json!({ "recording": false }));
    }
}

/// "Delete AI altogether". Blocking work runs off the main thread.
#[tauri::command]
pub async fn ai_delete(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete_blocking(&app))
        .await
        .map_err(|e| e.to_string())?
}

fn delete_blocking(app: &AppHandle) -> Result<(), String> {
    // The marker first: AI now counts as off, so nothing can respawn the agent,
    // and if removing files fails halfway init() finishes the job on next start.
    let flag_path = deleted_flag(app).ok_or("Can't find Bloom's settings folder.")?;
    std::fs::write(&flag_path, "Bloom AI was deleted in Settings. Delete this file to allow installing it again.
")
        .map_err(|e| e.to_string())?;
    stop();
    HOTKEY_VK.store(0, Ordering::Relaxed);
    if let Some(exe) = exe_path(app).filter(|p| p.exists()) {
        wipe(&exe)?;
    }
    if let Some(dir) = ai_dir(app).filter(|d| d.exists()) {
        // The exe can stay locked for a moment after its process exits.
        let mut removed = std::fs::remove_dir_all(&dir);
        for _ in 0..5 {
            if removed.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
            removed = std::fs::remove_dir_all(&dir);
        }
        removed.map_err(|e| format!("Couldn't remove {}: {e}", dir.display()))?;
    }
    remove_ai_settings(app)?;
    let _ = app.emit("ai-event", json!({ "type": "deleted" }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> AppInfo {
        AppInfo {
            name: name.into(),
            path: format!("C:\\{name}.lnk"),
            icon: None,
            is_running: false,
            hwnd: None,
            executable: None,
            all_hwnds: None,
        }
    }

    #[test]
    fn finds_apps_exact_then_prefix_then_anywhere() {
        let apps = vec![app("Spotify Helper"), app("Spotify"), app("Microsoft Edge")];
        assert_eq!(find_app(&apps, "spotify").unwrap().name, "Spotify");
        assert_eq!(find_app(&apps, "micro").unwrap().name, "Microsoft Edge");
        assert_eq!(find_app(&apps, "edge").unwrap().name, "Microsoft Edge");
        assert!(find_app(&apps, "zoom").is_none());
        assert!(find_app(&apps, " ").is_none());
    }

    #[test]
    fn numbers_and_flags_from_loose_model_output() {
        assert_eq!(number(&json!(40)), Some(40.0));
        assert_eq!(number(&json!("40%")), Some(40.0));
        assert_eq!(number(&json!("loud")), None);
        assert_eq!(flag(&json!(true)), Some(true));
        assert_eq!(flag(&json!("off")), Some(false));
        assert_eq!(flag(&json!("maybe")), None);
        assert_eq!(number(&json!("NaN")), None);
        assert_eq!(number(&json!("inf")), None);
        assert_eq!(flag(&json!("On")), Some(true));
        assert_eq!(flag(&json!("OFF")), Some(false));
    }

    #[test]
    fn strips_only_ai_keys() {
        let mut settings: serde_json::Map<String, Value> = serde_json::from_value(json!({
            "bloom-ai-enabled": "true",
            "bloom-ai-model": "m",
            "bloom-dock-enabled": "true"
        }))
        .unwrap();
        strip_ai_keys(&mut settings);
        assert_eq!(settings.keys().collect::<Vec<_>>(), vec!["bloom-dock-enabled"]);
    }
}
