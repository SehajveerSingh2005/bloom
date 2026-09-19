//! Linux command implementations for the first Mint/Cinnamon/X11 milestone.
//!
//! Commands whose native backend has not been implemented yet deliberately
//! return a structured error where their IPC contract permits one.  They are
//! not emulated with shell snippets or Windows semantics.

use crate::{
    platform::linux::{
        apps, audio, bluetooth, icons, launch_first, media, network, power, system, theme,
        thumbnails, windows_manager,
    },
    types::{AppInfo, IntRect},
};
use std::{collections::HashMap, fs, path::PathBuf};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Window};

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("settings.json"))
}
fn unsupported(feature: &str) -> String {
    format!("{feature} is not available yet on Linux; Bloom remains usable without it")
}

#[tauri::command]
pub fn get_platform() -> String {
    "linux".into()
}

#[tauri::command]
pub async fn set_menu_open(open: bool, rect: Option<IntRect>) {
    crate::state::MENU_IS_OPEN.store(open, Ordering::Relaxed);
    if let Ok(mut menu_rect) = crate::state::MENU_RECT.lock() {
        *menu_rect = rect;
    }
}
/// The frontend reports the pointer entering and leaving the dock and notch.
/// The X11 pointer watcher folds this into the edge-hover events, the way the
/// Windows mouse hook does.
#[tauri::command]
pub async fn set_dock_hovered(hovered: bool) {
    crate::state::DOCK_IS_HOVERED.store(hovered, Ordering::Relaxed);
}
#[tauri::command]
pub async fn set_notch_hovered(hovered: bool) {
    crate::state::NOTCH_IS_HOVERED.store(hovered, Ordering::Relaxed);
}
#[tauri::command]
pub async fn update_dock_rect(rect: IntRect) {
    if let Ok(mut dock_rect) = crate::state::DOCK_RECT.lock() {
        *dock_rect = Some(rect);
    }
}
#[tauri::command]
pub async fn update_notch_rect(rect: IntRect) {
    if let Ok(mut notch_rect) = crate::state::NOTCH_RECT.lock() {
        *notch_rect = Some(rect);
    }
}
#[tauri::command]
pub fn set_window_height(window: Window, height: f64) {
    if let Ok(size) = window.inner_size() {
        let _ = window.set_size(tauri::PhysicalSize::new(size.width, height.max(1.0) as u32));
    }
}
#[tauri::command]
pub fn resize_settings_window(app: AppHandle, width: f64, height: f64) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.set_size(tauri::LogicalSize::new(width, height));
    }
}
#[tauri::command]
pub fn set_ignore_cursor_events(window: Window, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}
#[tauri::command]
pub async fn init_dock(app: AppHandle, _mode: String) {
    if let Some(window) = app.get_webview_window("dock") {
        let _ = window.show();
    }
    apply_dock_layout(&app);
}
#[tauri::command]
pub async fn toggle_dock(app: AppHandle, enable: bool) {
    if let Some(window) = app.get_webview_window("dock") {
        let _ = if enable { window.show() } else { window.hide() };
    }
    if enable {
        apply_dock_layout(&app);
    }
}
#[tauri::command]
pub async fn sync_appbar(app: AppHandle) {
    apply_notch_layout(&app, false);
    apply_dock_layout(&app);
}
/// The dock is not an AppBar on Linux and its fixed/auto-hide behaviour is
/// computed in the frontend from the overlap events, so there is nothing to
/// re-register here beyond re-anchoring the window.
#[tauri::command]
pub async fn change_dock_mode(app: AppHandle, _mode: String) {
    apply_dock_layout(&app);
}

/// Height of the notch webview at 100% scale, matching the Windows backend.
const NOTCH_BASE_HEIGHT: f64 = 420.0;
/// Height of the dock webview at 100% scale, matching its configured box.
const DOCK_BASE_HEIGHT: f64 = 600.0;

/// Anchor the dock across the bottom of the primary monitor.
///
/// The dock's configured box is a fixed 1920x600 at y=480, which only happens to
/// land on the bottom edge at 1080p. Anchoring it to the monitor keeps it correct
/// at any resolution, and is the Linux counterpart to the Windows backend's
/// auto-hide placement. Nothing is reserved from the desktop: Bloom simply sits
/// over the panel's own area.
fn apply_dock_layout(app: &AppHandle) {
    let Some(window) = app.get_webview_window("dock") else {
        return;
    };
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let monitor_position = *monitor.position();
    let monitor_size = *monitor.size();
    // The dock's CSS places the pill at the bottom of its own window, so the
    // window keeps its configured height and moves to the screen's bottom edge.
    let height = (DOCK_BASE_HEIGHT * scale) as i32;
    let _ = window.set_position(PhysicalPosition::new(
        monitor_position.x,
        monitor_position.y + monitor_size.height as i32 - height,
    ));
    let _ = window.set_size(PhysicalSize::new(monitor_size.width, height as u32));
}

/// Lay the notch out across the primary monitor so its CSS can centre itself,
/// applying the user's UI scale the way the Windows backend does. On Linux there
/// is no AppBar to reserve screen edges, so Bloom simply coexists with the
/// desktop's own panel.
fn apply_notch_layout(app: &AppHandle, show: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if show {
        let _ = window.show();
    }
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let height = (NOTCH_BASE_HEIGHT * crate::utils::get_bloom_scale(app) * scale) as u32;
    let _ = window.set_position(tauri::PhysicalPosition::new(
        monitor.position().x,
        monitor.position().y,
    ));
    let _ = window.set_size(tauri::PhysicalSize::new(
        monitor.size().width,
        height.max(1),
    ));
}

#[tauri::command]
pub async fn change_notch_mode(app: AppHandle, _mode: String) {
    apply_notch_layout(&app, true);
}

#[tauri::command]
pub async fn open_app(app_name: String) -> Result<(), String> {
    if app_name == "start" {
        return Err(unsupported("The Cinnamon application-menu integration"));
    }
    apps::launch(&app_name)
}
#[tauri::command]
pub async fn get_active_windows() -> Vec<AppInfo> {
    // X11 property reads are blocking round trips; keep them off the async
    // runtime just like the Windows backend does.
    tauri::async_runtime::spawn_blocking(windows_manager::list)
        .await
        .unwrap_or_default()
}
#[tauri::command]
pub async fn focus_window(hwnd: isize) -> Result<(), String> {
    windows_manager::focus(hwnd)
}
#[tauri::command]
pub async fn close_window(hwnd: isize) -> Result<(), String> {
    windows_manager::close(hwnd)
}
/// Resolve an application's icon.
///
/// A user-chosen icon always wins over the theme's. Theme lookups are pure
/// functions of the desktop entry, so they are memoized per path, and a failed
/// lookup returns `None` so the frontend's own fallback keeps working.
fn resolve_icon(
    app: &AppHandle,
    path: &str,
    name: Option<&str>,
    hwnd: Option<isize>,
) -> Option<String> {
    if let Ok(dir) = custom_icons_dir(app) {
        for key in icon_keys(path, name, hwnd) {
            if let Some(icon) = read_icon(&dir, &key) {
                return Some(icon);
            }
        }
    }

    let cache = crate::state::ICON_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Ok(cache) = cache.lock() {
        if let Some(icon) = cache.get(path) {
            return Some(icon.clone());
        }
    }
    let icon_name = apps::icon_name_for(path, name)?;
    let icon = icons::data_uri_for_name(&icon_name)?;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(path.to_owned(), icon.clone());
    }
    Some(icon)
}

#[tauri::command]
pub async fn get_app_icon(
    app: AppHandle,
    path: String,
    name: Option<String>,
    hwnd: Option<isize>,
) -> Result<Option<String>, String> {
    // Icon theme lookup stats hundreds of paths, so run it off the runtime.
    tauri::async_runtime::spawn_blocking(move || resolve_icon(&app, &path, name.as_deref(), hwnd))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_pinned_apps(app: AppHandle, apps: Vec<AppInfo>) -> Result<(), String> {
    let path = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("pinned_apps.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, serde_json::to_vec(&apps).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn load_pinned_apps(app: AppHandle) -> Vec<AppInfo> {
    let path = app
        .path()
        .app_config_dir()
        .unwrap_or_default()
        .join("pinned_apps.json");
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
#[tauri::command]
pub async fn clear_icon_cache(_app: AppHandle) -> Result<(), String> {
    if let Some(cache) = crate::state::ICON_CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.clear();
        }
    }
    Ok(())
}
/// Where user-chosen application icons live. These are Bloom's own files in its
/// config directory; no desktop or theme file is ever modified.
fn custom_icons_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("custom_icons");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Icon keys arrive from the frontend and can contain path separators, so they
/// are flattened before being used as a filename.
fn sanitize_filename(key: &str) -> String {
    key.replace(
        |c: char| c == ':' || c == '\\' || c == '/' || c == '*' || c == '?' || c == '"' || c == '<'
            || c == '>' || c == '|',
        "_",
    )
}

fn custom_icon_file(dir: &PathBuf, key: &str) -> PathBuf {
    dir.join(format!("{}.png", sanitize_filename(key)))
}

/// The frontend identifies an application's icon with one of a few key shapes,
/// most specific first, so a custom icon is looked up under all of them.
fn icon_keys(path: &str, name: Option<&str>, hwnd: Option<isize>) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(name) = name {
        keys.push(format!("{}:{}", path, name.to_lowercase()));
    }
    if let Some(hwnd) = hwnd {
        keys.push(format!("{path}-{hwnd}"));
    }
    keys.push(path.to_owned());
    keys
}

fn read_icon(dir: &PathBuf, key: &str) -> Option<String> {
    let bytes = fs::read(custom_icon_file(dir, key)).ok()?;
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:image/png;base64,{encoded}"))
}

/// Keys the frontend has given icons to, so `get_custom_icons` can report them
/// under the original key rather than the flattened filename.
fn custom_icon_manifest(dir: &PathBuf) -> HashMap<String, String> {
    fs::read(dir.join("custom_icons.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Store a user-chosen icon, returning it as a data URI for immediate use.
///
/// Separated from the command so the round trip can be tested without a running
/// application.
fn write_custom_icon(dir: &PathBuf, cache_key: &str, icon_data: &str) -> Result<String, String> {
    use base64::Engine;
    use image::GenericImageView;

    let encoded = if icon_data.starts_with("data:") {
        icon_data
            .split(',')
            .nth(1)
            .ok_or("Invalid data URI")?
            .to_owned()
    } else {
        icon_data.to_owned()
    };
    let raw = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .map_err(|e| format!("Invalid base64: {e}"))?;
    let image = image::load_from_memory(&raw).map_err(|e| format!("Invalid image: {e}"))?;

    let (width, height) = image.dimensions();
    if width < 16 || height < 16 {
        return Err("Icon must be at least 16x16 pixels".into());
    }

    // Oversized icons are fitted onto a square transparent canvas so the dock
    // row keeps a consistent size, matching the Windows backend.
    const TARGET: u32 = 256;
    let image = if width > TARGET || height > TARGET {
        let scaled = image.resize(TARGET, TARGET, image::imageops::FilterType::Lanczos3);
        let (scaled_width, scaled_height) = scaled.dimensions();
        let mut canvas = image::RgbaImage::new(TARGET, TARGET);
        let x = ((TARGET - scaled_width) / 2) as i64;
        let y = ((TARGET - scaled_height) / 2) as i64;
        image::imageops::overlay(&mut canvas, &scaled, x, y);
        image::DynamicImage::ImageRgba8(canvas)
    } else {
        image
    };

    let mut png: Vec<u8> = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode PNG: {e}"))?;

    fs::write(custom_icon_file(dir, cache_key), &png).map_err(|e| e.to_string())?;

    let mut manifest = custom_icon_manifest(dir);
    manifest.insert(sanitize_filename(cache_key), cache_key.to_owned());
    fs::write(
        dir.join("custom_icons.json"),
        serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    Ok(format!("data:image/png;base64,{encoded}"))
}

/// Remove every icon a given application could have been given. The frontend
/// removes by application, so all key shapes are cleared.
fn clear_custom_icon(dir: &PathBuf, path: &str, name: Option<&str>) -> Result<(), String> {
    let mut manifest = custom_icon_manifest(dir);
    for key in icon_keys(path, name, None) {
        let _ = fs::remove_file(custom_icon_file(dir, &key));
        manifest.remove(&sanitize_filename(&key));
    }
    fs::write(
        dir.join("custom_icons.json"),
        serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Every stored icon, keyed as the frontend supplied it.
fn read_custom_icons(dir: &PathBuf) -> HashMap<String, String> {
    use base64::Engine;
    let manifest = custom_icon_manifest(dir);
    let mut icons = HashMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return icons;
    };
    for entry in entries.flatten() {
        let file = entry.path();
        if file.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let key = manifest.get(stem).cloned().unwrap_or_else(|| stem.to_owned());
        if let Ok(bytes) = fs::read(&file) {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            icons.insert(key, format!("data:image/png;base64,{encoded}"));
        }
    }
    icons
}

#[tauri::command]
pub async fn set_custom_icon(
    app: AppHandle,
    cache_key: String,
    icon_data: String,
) -> Result<String, String> {
    let dir = custom_icons_dir(&app)?;
    write_custom_icon(&dir, &cache_key, &icon_data)
}

#[tauri::command]
pub async fn remove_custom_icon(
    app: AppHandle,
    path: String,
    name: Option<String>,
) -> Result<(), String> {
    clear_custom_icon(&custom_icons_dir(&app)?, &path, name.as_deref())
}

#[tauri::command]
pub async fn get_custom_icons(app: AppHandle) -> Result<HashMap<String, String>, String> {
    Ok(read_custom_icons(&custom_icons_dir(&app)?))
}
#[tauri::command]
pub async fn get_installed_apps() -> Vec<AppInfo> {
    tauri::async_runtime::spawn_blocking(apps::discover)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub fn hide_native_osd() {}
#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
#[tauri::command]
pub fn hide_overlay(app: AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
}
/// The overlay window doubles as Bloom's splash screen and as the volume and
/// brightness on-screen displays. Fullscreen mode covers the primary monitor for
/// the splash; leaving it hides the overlay and restores the OSD geometry.
#[tauri::command]
pub fn set_splash_fullscreen(app: AppHandle, fullscreen: bool) {
    crate::state::OVERLAY_IN_SPLASH.store(fullscreen, Ordering::Relaxed);
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    if fullscreen {
        let _ = window.hide();
        size_overlay_to_primary_monitor(&window);
        let _ = window.show();
    } else {
        let _ = window.hide();
        sync_overlay_position(app);
    }
}

#[tauri::command]
pub fn sync_overlay_position(app: AppHandle) {
    if crate::state::OVERLAY_IN_SPLASH.load(Ordering::Relaxed) {
        return;
    }
    if let Some(window) = app.get_webview_window("overlay") {
        size_overlay_to_primary_monitor(&window);
    }
}

/// The overlay spans the whole primary monitor so its left and right notches sit
/// against the real screen edges without further positioning.
fn size_overlay_to_primary_monitor(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.primary_monitor() else {
        return;
    };
    let position = PhysicalPosition::new(monitor.position().x, monitor.position().y);
    let size = PhysicalSize::new(monitor.size().width, monitor.size().height);
    // This runs immediately before the overlay is shown, so skip the two X
    // requests when it already covers the monitor: they would only delay the
    // display appearing.
    let already_placed = window.outer_position().ok() == Some(position)
        && window.outer_size().ok() == Some(size);
    if !already_placed {
        let _ = window.set_position(position);
        let _ = window.set_size(size);
    }
}
#[tauri::command]
pub fn open_wifi_settings() {
    network::open_settings();
}
#[tauri::command]
pub fn open_sound_settings() {
    launch_first(&[("cinnamon-settings", &["sound"])]);
}
/// Cinnamon has no system notification centre of its own, so this opens the
/// desktop's notification settings. Bloom's own notification history stays in
/// Bloom.
#[tauri::command]
pub fn open_notification_center() {
    launch_first(&[("cinnamon-settings", &["notifications"])]);
}
/// Likewise there is no separate overflow tray to reveal: the equivalent surface
/// is the panel settings, where the tray applet lives.
#[tauri::command]
pub fn open_system_tray() {
    launch_first(&[("cinnamon-settings", &["panel"])]);
}
#[tauri::command]
pub fn media_play_pause() {
    let _ = media::send(media::MediaCommand::PlayPause);
}
#[tauri::command]
pub fn media_next() {
    let _ = media::send(media::MediaCommand::Next);
}
#[tauri::command]
pub fn media_previous() {
    let _ = media::send(media::MediaCommand::Previous);
}
#[tauri::command]
pub fn media_seek(position_ms: f64) {
    let _ = media::send(media::MediaCommand::Seek(position_ms as i64));
}
/// Raising the player's own window is player-specific on Linux, so this stays a
/// no-op rather than guessing which window belongs to the active player.
#[tauri::command]
pub fn open_media_source_app() {}

#[tauri::command]
pub async fn set_volume(volume: f32) -> Result<(), String> {
    // Talking to PipeWire/PulseAudio spawns a short-lived helper, so keep it off
    // the async runtime.
    tauri::async_runtime::spawn_blocking(move || audio::set_volume(volume))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn quit_bloom(handle: AppHandle) {
    handle.exit(0);
}
#[tauri::command]
pub async fn restart_bloom(handle: AppHandle) {
    // Unlike Windows, Bloom registers no AppBars and hides no panel on Linux, so
    // there is no desktop state to unwind before re-executing.
    handle.restart();
}

#[tauri::command]
pub fn save_setting(app: AppHandle, key: String, value: serde_json::Value) -> Result<(), String> {
    let path = settings_path(&app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut settings: HashMap<String, serde_json::Value> = fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    settings.insert(key, value);
    fs::write(
        path,
        serde_json::to_vec(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<HashMap<String, serde_json::Value>, String> {
    Ok(fs::read(settings_path(&app)?)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default())
}
#[tauri::command]
pub async fn capture_window_thumbnail(
    hwnd: isize,
    max_width: u32,
    max_height: u32,
) -> Result<Option<(String, i64)>, String> {
    // A capture is an X round trip plus a scale and a PNG encode.
    tauri::async_runtime::spawn_blocking(move || {
        thumbnails::capture(hwnd, max_width, max_height)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn get_volume() -> f32 {
    // The sink's real level, which on Linux may exceed 1.0 because both PipeWire
    // and PulseAudio allow amplification above unity gain.
    tauri::async_runtime::spawn_blocking(|| {
        audio::state().map(|state| state.volume).unwrap_or(0.0)
    })
    .await
    .unwrap_or(0.0)
}
#[tauri::command]
pub fn get_brightness() -> Result<u32, String> {
    system::brightness()
}
#[tauri::command]
pub async fn get_wifi_state() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(network::wifi_enabled)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn set_wifi_state(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || network::set_wifi_enabled(enabled))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn get_bluetooth_state() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(bluetooth::enabled)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn set_bluetooth_state(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || bluetooth::set_enabled(enabled))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn open_bluetooth_settings() {
    bluetooth::open_settings();
}
#[tauri::command]
pub fn open_airplane_mode_settings() {
    bluetooth::open_airplane_mode_settings();
}
#[tauri::command]
pub fn set_brightness(_app: AppHandle, brightness: u32) -> Result<(), String> {
    system::set_brightness(brightness)
}
#[tauri::command]
pub async fn get_battery_state() -> Result<power::BatteryState, String> {
    tauri::async_runtime::spawn_blocking(power::state)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_battery_saver_state() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(power::power_saver_enabled)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn open_battery_saver_settings() {
    // Power modes live in the desktop's power panel.
    launch_first(&[("cinnamon-settings", &["power"])]);
}
#[tauri::command]
pub async fn get_system_accent_color() -> Result<String, String> {
    // Each lookup may start a short-lived gsettings helper, so keep it off the
    // async runtime.
    tauri::async_runtime::spawn_blocking(theme::accent_color)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn get_cpu_usage() -> Result<u32, String> {
    system::cpu_usage()
}
#[tauri::command]
pub fn get_ram_usage() -> Result<f32, String> {
    system::ram_usage()
}
#[tauri::command]
pub fn get_disk_space() -> Result<u64, String> {
    system::disk_free_space()
}
#[tauri::command]
pub fn get_network_speed() -> Result<(u64, u64), String> {
    system::network_delta()
}
#[tauri::command]
pub fn export_settings(app: AppHandle) -> Result<String, String> {
    fs::read_to_string(settings_path(&app)?).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn read_settings_from_path(path: String) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn write_settings_to_path(path: String, content: String) -> Result<(), String> {
    fs::write(path, content).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn import_settings(app: AppHandle, settings: String) -> Result<(), String> {
    let _: HashMap<String, serde_json::Value> =
        serde_json::from_str(&settings).map_err(|e| e.to_string())?;
    fs::write(settings_path(&app)?, settings).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique scratch directory per test run.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bloom-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A real PNG, encoded the same way the frontend sends one.
    fn icon_data_uri(size: u32) -> String {
        use base64::Engine;
        let image = image::RgbaImage::from_pixel(size, size, image::Rgba([220, 40, 60, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&png))
    }

    #[test]
    fn custom_icons_survive_a_save_and_load_round_trip() {
        let dir = scratch("round-trip");
        // A key with path separators must still come back unchanged.
        let key = "/usr/share/applications/code.desktop-1234";
        let stored = write_custom_icon(&dir, key, &icon_data_uri(32)).unwrap();
        assert!(stored.starts_with("data:image/png;base64,"));
        assert!(custom_icon_file(&dir, key).exists());

        let icons = read_custom_icons(&dir);
        assert_eq!(icons.get(key).map(String::as_str), Some(stored.as_str()));

        // And the frontend can find it under the plain application path too.
        assert!(read_custom_icons(&dir).contains_key(key));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn removing_an_icon_clears_every_key_shape_for_that_application() {
        let dir = scratch("remove");
        let path = "/usr/share/applications/firefox.desktop";
        let hwnd_key = format!("{path}-99");
        write_custom_icon(&dir, path, &icon_data_uri(32)).unwrap();
        write_custom_icon(&dir, &hwnd_key, &icon_data_uri(32)).unwrap();
        assert_eq!(read_custom_icons(&dir).len(), 2);

        // The frontend removes by application, without the window id, so the
        // path key is the one it names.
        clear_custom_icon(&dir, path, None).unwrap();
        let remaining = read_custom_icons(&dir);
        assert!(!remaining.contains_key(path));
        // The window-specific icon is a different key and is left alone.
        assert!(remaining.contains_key(&hwnd_key));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tiny_or_unreadable_icons_are_rejected() {
        let dir = scratch("reject");
        assert!(write_custom_icon(&dir, "small", &icon_data_uri(8)).is_err());
        assert!(write_custom_icon(&dir, "junk", "data:image/png;base64,bm90LWltYWdl").is_err());
        // Nothing was written for either attempt.
        assert!(read_custom_icons(&dir).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn icon_keys_are_ordered_most_specific_first() {
        let keys = icon_keys("/apps/x.desktop", Some("X App"), Some(7));
        assert_eq!(
            keys,
            vec![
                "/apps/x.desktop:x app".to_owned(),
                "/apps/x.desktop-7".to_owned(),
                "/apps/x.desktop".to_owned(),
            ]
        );
    }

    #[test]
    fn filenames_are_flattened_so_keys_cannot_escape_the_directory() {
        assert_eq!(sanitize_filename("a/b:c"), "a_b_c");
        assert!(!sanitize_filename("../../etc/passwd").contains('/'));
    }
}
