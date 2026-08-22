use tauri::{AppHandle, Emitter, Manager, Window};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use std::sync::atomic::Ordering;

use crate::types::{IntRect, AppInfo, AudioDevice, BrightnessChangeEvent};
use crate::state::*;
use crate::utils::*;
use crate::services::{register_appbar, register_dock_appbar, sync_overlays, unregister_appbar_native, enum_windows_proc};
use std::collections::HashMap;
use std::os::windows::process::CommandExt;

#[tauri::command]
pub async fn set_menu_open(open: bool, rect: Option<IntRect>) {
    MENU_IS_OPEN.store(open, Ordering::Relaxed);
    if let Ok(mut r) = MENU_RECT.lock() {
        *r = rect;
    }
}

#[tauri::command]
pub async fn set_dock_hovered(hovered: bool) {
    DOCK_IS_HOVERED.store(hovered, Ordering::Relaxed);
}

#[tauri::command]
pub async fn set_notch_hovered(hovered: bool) {
    NOTCH_IS_HOVERED.store(hovered, Ordering::Relaxed);
}

#[tauri::command]
pub async fn update_dock_rect(rect: IntRect) {
    if let Ok(mut r) = DOCK_RECT.lock() {
        *r = Some(rect);
    }
}

#[tauri::command]
pub async fn update_notch_rect(rect: IntRect) {
    if let Ok(mut r) = NOTCH_RECT.lock() {
        *r = Some(rect);
    }
}

#[tauri::command]
pub fn set_window_height(window: Window, height: f64) {
    if let Ok(scale_factor) = window.scale_factor() {
        if let Ok(physical_size) = window.inner_size() {
            let logical_width = physical_size.width as f64 / scale_factor;
            let _ = window.set_size(tauri::LogicalSize::new(logical_width, height));
        }
    }
}

#[tauri::command]
pub fn resize_settings_window(app: AppHandle, width: f64, height: f64) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.set_size(tauri::LogicalSize::new(width, height));
    }
}

#[tauri::command]
pub fn set_ignore_cursor_events(window: Window, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}

#[tauri::command]
pub async fn init_dock(app: AppHandle, mode: String) {
    // Backend guard: bail if dock is disabled in settings.
    // The frontend already checks this, but settings.json may have a stale
    // value if the write didn't complete before restart. Reading here too
    // makes the dock reliably stay hidden regardless of frontend timing.
    let enabled = get_setting_str(&app, "bloom-dock-enabled")
        .unwrap_or_else(|| "true".to_string());
    if enabled != "true" {
        if let Some(dock_win) = app.get_webview_window("dock") {
            let _ = dock_win.hide();
            DOCK_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
        }
        return;
    }

    if let Some(dock_win) = app.get_webview_window("dock") {
        // 1. Always show first — idempotent, required before any positioning
        let _ = dock_win.show();
        if let Ok(hwnd) = dock_win.hwnd() { re_assert_topmost(hwnd); }

        // 2. Register as appbar (fixed) or manually position (auto-hide)
        if mode == "fixed" {
            // register_dock_appbar calls show() internally too, and handles retries
            register_dock_appbar(dock_win.clone());
        } else {
            // Auto-hide mode: position at bottom of screen.
            // Retry until primary_monitor() is available (can fail on autostart before shell).
            let dock_clone = dock_win.clone();
            tauri::async_runtime::spawn(async move {
                for attempt in 0..20 {
                    // Wait for monitor and window dimensions to be available.
                    // Never use a hardcoded fallback — wrong values produce off-screen placement.
                    // Extract HWND as isize before any await (raw pointer is not Send).
                    let hwnd_val = dock_clone.hwnd().map(|h| h.0 as isize).unwrap_or(0);
                    let ph = dock_clone.outer_size().map(|s| s.height as i32).unwrap_or(0);
                    let monitor_info = dock_clone.primary_monitor().ok().flatten().map(|m| {
                        let s = m.size();
                        let p = m.position();
                        (tauri::PhysicalSize::new(s.width, s.height), tauri::PhysicalPosition::new(p.x, p.y))
                    });

                    if hwnd_val != 0 {
                        if let Some((m_size, m_pos)) = monitor_info {
                            if ph <= 10 {
                                // outer_size() not ready yet — retry next tick
                                if attempt < 19 {
                                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                                }
                                continue;
                            }
                            let final_y = m_pos.y + m_size.height as i32 - ph;
                            unsafe {
                                use windows::Win32::UI::WindowsAndMessaging::{
                                    SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, SWP_FRAMECHANGED
                                };
                                use windows::Win32::Foundation::HWND;
                                let _ = SetWindowPos(
                                    HWND(hwnd_val as *mut _), None,
                                    m_pos.x, final_y,
                                    m_size.width as i32, ph,
                                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED
                                );
                            }
                            // Re-assert topmost after repositioning
                            if let Ok(hwnd) = dock_clone.hwnd() { re_assert_topmost(hwnd); }
                            // Ensure visible after positioning
                            let _ = dock_clone.show();
                            break;
                        }
                    }
                    if attempt < 19 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    }
                }
            });
        }

        // 3. Hide taskbar after showing dock (not before, so user always has something)
        set_taskbar_visibility(false, false);
        NATIVE_TASKBAR_HIDDEN.store(true, Ordering::Relaxed);

        // 4. Reset overlap state so the overlap thread re-syncs cleanly
        CURRENT_DOCK_OVERLAP.store(0, Ordering::Relaxed);
        let _ = app.emit("dock-overlap", false);
    }
}

#[tauri::command]
pub async fn toggle_dock(app: AppHandle, enable: bool) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        if enable {
            // Load the saved dock mode rather than hardcoding "fixed"
            let saved_mode = crate::utils::get_setting_str(&app, "bloom-dock-mode")
                .unwrap_or_else(|| "auto-hide".to_string());
            init_dock(app, saved_mode).await;
        } else {
            let _ = dock_win.hide();
            if let Ok(hwnd) = dock_win.hwnd() {
                let hwnd_val = hwnd.0 as isize;
                tauri::async_runtime::spawn_blocking(move || {
                    unregister_appbar_native(HWND(hwnd_val as *mut _));
                });
            }
            DOCK_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
            set_taskbar_visibility(true, true);
            NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

            // Re-sync other appbars
            if let Some(main_win) = app.get_webview_window("main") {
                if MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                    register_appbar(main_win);
                }
            }
        }
    }
}

#[tauri::command]
pub async fn sync_appbar(app: AppHandle) {
    if let Some(main_win) = app.get_webview_window("main") {
        if MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
            register_appbar(main_win);
        } else {
            if let Ok(hwnd) = main_win.hwnd() { re_assert_topmost(hwnd); }
        }
    }
    if let Some(dock_win) = app.get_webview_window("dock") {
        // Skip dock re-registration if dock is disabled in settings.
        let dock_enabled = get_setting_str(&app, "bloom-dock-enabled")
            .unwrap_or_else(|| "true".to_string());
        if dock_enabled == "true" && DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) {
            register_dock_appbar(dock_win);
        } else {
            if let Ok(hwnd) = dock_win.hwnd() { re_assert_topmost(hwnd); }
        }
    }
    sync_overlays(&app);
}

#[tauri::command]
pub async fn change_dock_mode(app: AppHandle, mode: String) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        if mode == "fixed" {
            register_dock_appbar(dock_win.clone());
        } else {
            let _ = dock_win.show();
            if let Ok(hwnd) = dock_win.hwnd() {
                let hwnd_val = hwnd.0 as isize;
                tauri::async_runtime::spawn_blocking(move || {
                    unregister_appbar_native(HWND(hwnd_val as *mut _));
                });
                DOCK_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
                
                // Retry primary_monitor() — can fail on autostart before shell initializes
                let dock_clone = dock_win.clone();
                tauri::async_runtime::spawn(async move {
                    for attempt in 0..10 {
                        // Never fall back to a hardcoded pixel height.
                        // Extract HWND as isize before any await (raw pointer is not Send).
                        let hwnd_val = dock_clone.hwnd().map(|h| h.0 as isize).unwrap_or(0);
                        let ph = dock_clone.outer_size().map(|s| s.height as i32).unwrap_or(0);
                        let monitor_info = dock_clone.primary_monitor().ok().flatten().map(|m| {
                            let s = m.size();
                            let p = m.position();
                            (tauri::PhysicalSize::new(s.width, s.height), tauri::PhysicalPosition::new(p.x, p.y))
                        });

                        if hwnd_val != 0 {
                            if let Some((m_size, m_pos)) = monitor_info {
                                if ph <= 10 {
                                    if attempt < 9 {
                                        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                                    }
                                    continue;
                                }
                                let final_y = m_pos.y + m_size.height as i32 - ph;
                                unsafe {
                                    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, SWP_FRAMECHANGED};
                                    use windows::Win32::Foundation::HWND;
                                    let _ = SetWindowPos(HWND(hwnd_val as *mut _), None, m_pos.x, final_y, m_size.width as i32, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
                                }
                                if let Ok(hwnd) = dock_clone.hwnd() { re_assert_topmost(hwnd); }
                                break;
                            }
                        }
                        if attempt < 9 {
                            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                        }
                    }
                });
            }
        }
        
        // Ensure always on top and native taskbar stays hidden
        if let Ok(hwnd) = dock_win.hwnd() { re_assert_topmost(hwnd); }
        set_taskbar_visibility(false, false);
        NATIVE_TASKBAR_HIDDEN.store(true, Ordering::Relaxed);

        
        // Sync the current overlap state immediately to the frontend
        let current = CURRENT_DOCK_OVERLAP.load(Ordering::Relaxed);
        if current != -1 {
            let _ = app.emit("dock-overlap", current == 1);
        }

        // Double sync after a short delay to catch any layout changes
        let dock_clone = dock_win.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            if DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                register_dock_appbar(dock_clone);
            }
        });
    }
}

#[tauri::command]
pub async fn change_notch_mode(app: AppHandle, mode: String) {
    if let Some(main_win) = app.get_webview_window("main") {
        if mode == "fixed" {
            register_appbar(main_win.clone());
        } else {
            let _ = main_win.show();
            if let Ok(hwnd) = main_win.hwnd() {
                let hwnd_val = hwnd.0 as isize;
                tauri::async_runtime::spawn_blocking(move || {
                    unregister_appbar_native(HWND(hwnd_val as *mut _));
                });
                MAIN_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
                
                let main_clone = main_win.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    if !MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                        if let Ok(hwnd) = main_clone.hwnd() { re_assert_topmost(hwnd); }
                    }
                });
            }
        }
        // Reposition window to span the full primary monitor so CSS justify-content:center works
        if let Ok(Some(monitor)) = main_win.primary_monitor() {
            let m_pos = monitor.position();
            let m_size = monitor.size();
            let scale = monitor.scale_factor();
            let bloom_scale = crate::utils::get_bloom_scale(&app);
            let target_height = (420.0 * bloom_scale * scale) as u32;
            let _ = main_win.set_position(tauri::PhysicalPosition::new(m_pos.x, m_pos.y));
            let _ = main_win.set_size(tauri::PhysicalSize::new(m_size.width, target_height));
        }
        
        let current = CURRENT_NOTCH_OVERLAP.load(Ordering::Relaxed);
        if current != -1 {
            let _ = app.emit("notch-overlap", current == 1);
        }
    }
}

fn get_uwp_launch_cmd(exe_path: &str) -> Option<String> {
    let path = std::path::Path::new(exe_path);
    let mut is_windows_apps = false;
    let mut package_folder = String::new();
    
    for component in path.components() {
        if let std::path::Component::Normal(s) = component {
            let s_str = s.to_string_lossy();
            if is_windows_apps {
                package_folder = s_str.to_string();
                break;
            }
            if s_str.eq_ignore_ascii_case("WindowsApps") {
                is_windows_apps = true;
            }
        }
    }
    
    if !package_folder.is_empty() {
        // package_folder format: PackageName_Version_Architecture__PublisherId
        // We want: PackageName_PublisherId!App
        if let Some(publisher_idx) = package_folder.rfind("__") {
            let publisher_id = &package_folder[publisher_idx + 2..];
            if let Some(first_underscore) = package_folder.find('_') {
                let package_name = &package_folder[..first_underscore];
                return Some(format!("shell:AppsFolder\\{}_{}!App", package_name, publisher_id));
            }
        }
    }
    None
}

#[tauri::command]
pub async fn open_app(app_name: String) {
    if app_name == "start" {
        tauri::async_runtime::spawn_blocking(move || unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, KEYBDINPUT, VK_LWIN, KEYEVENTF_KEYUP};
            let inputs = [
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VK_LWIN,
                            wScan: 0,
                            dwFlags: Default::default(),
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: VK_LWIN,
                            wScan: 0,
                            dwFlags: KEYEVENTF_KEYUP,
                            time: 0,
                            dwExtraInfo: 0,
                        },
                    },
                },
            ];
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        });
        return;
    }
    
    let path = app_name;
    tauri::async_runtime::spawn_blocking(move || unsafe {
        let (actual_path, _args) = if path.to_lowercase().ends_with(".lnk") {
            resolve_shortcut(&path).unwrap_or((path.clone(), String::new()))
        } else {
            (path.clone(), String::new())
        };

        if let Some(uwp_cmd) = crate::commands::get_uwp_launch_cmd(&actual_path) {
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
            let wide_open: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
            let wide_cmd: Vec<u16> = uwp_cmd.encode_utf16().chain(std::iter::once(0)).collect();
            
            let res = ShellExecuteW(
                None,
                windows::core::PCWSTR(wide_open.as_ptr()),
                windows::core::PCWSTR(wide_cmd.as_ptr()),
                None,
                None,
                SW_SHOWNORMAL,
            );
            
            if res.0 as usize <= 32 {
                eprintln!("Failed to open UWP app {}: error code {}", uwp_cmd, res.0 as usize);
            }
            return;
        }

        use std::path::Path;
        let mut final_path = actual_path.clone();
        
        if !Path::new(&final_path).exists() {
            let file_name = Path::new(&final_path).file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
            
            // Handle Discord/Slack style auto-updaters (app-x.x.x folder structure)
            if file_name == "discord.exe" || file_name == "slack.exe" || file_name == "githubdesktop.exe" || file_name == "zentwilight.exe" {
                if let Some(parent) = Path::new(&final_path).parent().and_then(|p| p.parent()) {
                    if parent.exists() {
                        if let Ok(entries) = std::fs::read_dir(parent) {
                            let mut app_dirs = Vec::new();
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                if (name.starts_with("app-") || name.starts_with("current")) && entry.path().is_dir() {
                                    app_dirs.push(entry.path());
                                }
                            }
                            app_dirs.sort();
                            if let Some(latest) = app_dirs.last() {
                                let exe = latest.join(Path::new(&final_path).file_name().unwrap());
                                if exe.exists() {
                                    final_path = exe.to_string_lossy().to_string();
                                }
                            }
                        }
                    }
                }
            }
        }

        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        
        let wide_path: Vec<u16> = final_path.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_open: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        
        let res = ShellExecuteW(
            None,
            windows::core::PCWSTR(wide_open.as_ptr()),
            windows::core::PCWSTR(wide_path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        
        if res.0 as usize <= 32 {
            eprintln!("Failed to open app {}: error code {}", final_path, res.0 as usize);
        }
    });
}

#[tauri::command]
pub async fn get_active_windows() -> Vec<AppInfo> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut apps: Vec<AppInfo> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut apps as *mut Vec<AppInfo> as isize));
        }

        let mut grouped: HashMap<String, AppInfo> = HashMap::new();
        
        for app in apps {
            let path = app.path.to_lowercase();
            let name = app.name.to_lowercase();
            
            // For host processes (Edge, Chrome, ApplicationFrameHost), use path + name 
            // so that different PWAs/UWP apps are separate dock items.
            let key = if path.contains("msedge.exe") || path.contains("chrome.exe") || path.contains("applicationframehost.exe") {
                format!("{}:{}", path, name)
            } else if let Some(ref exe) = app.executable {
                format!("{}:{}", path, exe.to_lowercase())
            } else {
                path.clone()
            };

            if let Some(existing) = grouped.get_mut(&key) {
                if let Some(ref mut hwnds) = existing.all_hwnds {
                    hwnds.push((app.hwnd.unwrap_or(0), app.name.clone()));
                } else {
                    existing.all_hwnds = Some(vec![
                        (existing.hwnd.unwrap_or(0), existing.name.clone()),
                        (app.hwnd.unwrap_or(0), app.name.clone())
                    ]);
                }
            } else {
                let mut new_app = app.clone();
                new_app.all_hwnds = Some(vec![(app.hwnd.unwrap_or(0), app.name.clone())]);
                grouped.insert(key, new_app);
            }
        }

        let focus_guard = if let Some(map) = crate::state::FOCUS_TIMESTAMPS.get() {
            map.lock().ok()
        } else {
            None
        };

        let mut result_apps: Vec<AppInfo> = grouped.into_values().collect();

        for app in &mut result_apps {
            if let Some(ref mut hwnds) = app.all_hwnds {
                hwnds.sort_by(|a, b| {
                    let ts_a = focus_guard.as_ref()
                        .and_then(|g| g.get(&a.0))
                        .copied()
                        .unwrap_or(0);
                    let ts_b = focus_guard.as_ref()
                        .and_then(|g| g.get(&b.0))
                        .copied()
                        .unwrap_or(0);
                    ts_b.cmp(&ts_a)
                });
            }
        }

        result_apps
    }).await.unwrap_or_default()
}

#[tauri::command]
pub async fn focus_window(hwnd: isize) {
    tauri::async_runtime::spawn_blocking(move || unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW, SW_MINIMIZE, IsIconic, IsWindowVisible, GetForegroundWindow, GetWindowThreadProcessId};
        let hwnd = HWND(hwnd as *mut _);
        let my_pid = std::process::id();

        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        } else if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        } else {
            // Minimize if: window is foreground, OR same process as foreground (not Bloom), OR recently focused
            let fg = GetForegroundWindow();
            let mut should_minimize = false;

            if !fg.is_invalid() && fg == hwnd {
                should_minimize = true;
            } else if !fg.is_invalid() {
                let mut fg_pid = 0u32;
                let mut target_pid = 0u32;
                GetWindowThreadProcessId(fg, Some(&mut fg_pid));
                GetWindowThreadProcessId(hwnd, Some(&mut target_pid));
                if fg_pid == target_pid && fg_pid != 0 && fg_pid != my_pid {
                    should_minimize = true;
                }
            }

            if !should_minimize {
                let now = crate::utils::get_now_ms();
                should_minimize = if let Some(map) = crate::state::FOCUS_TIMESTAMPS.get() {
                    if let Ok(guard) = map.lock() {
                        guard.get(&(hwnd.0 as *mut () as isize))
                            .map(|ts| now - ts < 2000)
                            .unwrap_or(false)
                    } else { false }
                } else { false };
            }

            if should_minimize {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }).await.unwrap_or_default();
}

fn get_cache_key(path: &str, name: Option<&str>) -> String {
    let path_lc = path.to_lowercase();
    let name_lc = name.map(|n| n.to_lowercase()).unwrap_or_default();
    if path_lc.contains("msedge.exe") || path_lc.contains("chrome.exe") || path_lc.contains("applicationframehost.exe") {
        format!("{}:{}", path, name_lc)
    } else {
        path.to_string()
    }
}

fn get_custom_icons_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?.join("custom_icons");
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir)
}

fn sanitize_filename(key: &str) -> String {
    key.replace(|c: char| c == ':' || c == '\\' || c == '/' || c == '*' || c == '?' || c == '"' || c == '<' || c == '>' || c == '|', "_")
}

#[tauri::command]
pub async fn get_app_icon(app: AppHandle, path: String, name: Option<String>, hwnd: Option<isize>) -> Result<Option<String>, String> {
    let cache_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let cache_path = cache_dir.join("icons_cache.json");
    let cache_key = get_cache_key(&path, name.as_deref());

    // Strategy 0: Check for custom icon first
    let custom_icons_dir = cache_dir.join("custom_icons");
    let custom_file = custom_icons_dir.join(format!("{}.png", sanitize_filename(&cache_key)));
    if custom_file.exists() {
        if let Ok(data) = std::fs::read(&custom_file) {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            return Ok(Some(format!("data:image/png;base64,{}", b64)));
        }
    }

    let cache = ICON_CACHE.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        if let Ok(content) = std::fs::read_to_string(&cache_path) {
            if let Ok(existing) = serde_json::from_str::<std::collections::HashMap<String, String>>(&content) {
                map = existing;
            }
        }
        std::sync::Mutex::new(map)
    });

    // Strategy 1: Check persistent in-memory cache
    if let Ok(c) = cache.lock() {
        if let Some(icon) = c.get(&cache_key) {
            return Ok(Some(icon.clone()));
        }
    }

    // Strategy 2: Extract icon from live window HWND
    if let Some(h) = hwnd {
        let result = tauri::async_runtime::spawn_blocking(move || unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED, CoUninitialize};
            use windows::Win32::UI::WindowsAndMessaging::{GetClassLongPtrW, GCLP_HICON, WM_GETICON, ICON_BIG, SendMessageTimeoutW, SMTO_ABORTIFHUNG};
            
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let h_hwnd = HWND(h as *mut _);
            
            let mut h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(GetClassLongPtrW(h_hwnd, GCLP_HICON) as *mut _);
            if h_icon.is_invalid() {
                h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(GetClassLongPtrW(h_hwnd, windows::Win32::UI::WindowsAndMessaging::GCL_HICON) as *mut _);
            }

            if h_icon.is_invalid() {
                let mut res = 0usize;
                let _ = SendMessageTimeoutW(h_hwnd, WM_GETICON, windows::Win32::Foundation::WPARAM(ICON_BIG as usize), windows::Win32::Foundation::LPARAM(0), SMTO_ABORTIFHUNG, 250, Some(&mut res));
                if res != 0 { h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(res as *mut _); }
            }
            
            let res = if !h_icon.is_invalid() { icon_to_base64(h_icon) } else { None };
            
            CoUninitialize();
            res
        }).await.unwrap_or(None);

        if let Some(base64) = result {
            if let Ok(mut c) = cache.lock() {
                c.insert(cache_key.clone(), base64.clone());
                let _ = std::fs::write(&cache_path, serde_json::to_string(&*c).unwrap_or_default());
            }
            return Ok(Some(base64));
        }
    }

    // Strategy 3: Extract icon from file path
    let path_clone = path.clone();
    let ck_clone = cache_key.clone();
    tauri::async_runtime::spawn_blocking(move || unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED, CoUninitialize};
        use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
        use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
        
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = {
            let ((mut actual_path, args), is_lnk) = if path_clone.to_lowercase().ends_with(".lnk") {
                (resolve_shortcut(&path_clone).unwrap_or((path_clone.clone(), String::new())), true)
            } else {
                ((path_clone.clone(), String::new()), false)
            };

            if actual_path.to_lowercase().contains("chrome_proxy.exe") || actual_path.to_lowercase().contains("msedge_proxy.exe") || args.contains("--app-id=") {
                if let Some(app_id_start) = args.find("--app-id=") {
                    let app_id = &args[app_id_start + 9..].split_whitespace().next().unwrap_or("");
                    if !app_id.is_empty() {
                        if let Ok(local) = std::env::var("LOCALAPPDATA") {
                            let chrome_pwa = format!("{}\\Google\\Chrome\\User Data\\Default\\Web Applications\\_crx_{}\\icon_256.png", local, app_id);
                            if std::path::Path::new(&chrome_pwa).exists() { actual_path = chrome_pwa; }
                            else {
                                let edge_pwa = format!("{}\\Microsoft\\Edge\\User Data\\Default\\Web Applications\\_crx_{}\\icon_256.png", local, app_id);
                                if std::path::Path::new(&edge_pwa).exists() { actual_path = edge_pwa; }
                            }
                        }
                    }
                }
            }

            // Robust path resolution for common apps
            if !std::path::Path::new(&actual_path).is_absolute() {
                let lower = actual_path.to_lowercase();
                if lower == "code" || lower == "code.exe" {
                    if let Ok(home) = std::env::var("USERPROFILE") {
                        let p = format!("{}\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe", home);
                        if std::path::Path::new(&p).exists() { actual_path = p; }
                    }
                } else if lower == "msedge" || lower == "msedge.exe" {
                    let candidates = [
                        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
                    ];
                    for c in &candidates {
                        if std::path::Path::new(c).exists() { actual_path = c.to_string(); break; }
                    }
                } else if lower == "notepad" || lower == "notepad.exe" {
                    let candidates = [
                        r"C:\Windows\System32\notepad.exe",
                        r"C:\Windows\notepad.exe",
                    ];
                    for c in &candidates {
                        if std::path::Path::new(c).exists() { actual_path = c.to_string(); break; }
                    }
                } else if lower == "explorer" || lower == "explorer.exe" {
                    let p = r"C:\Windows\explorer.exe";
                    if std::path::Path::new(p).exists() { actual_path = p.to_string(); }
                }
            }

            let mut shfi: SHFILEINFOW = std::mem::zeroed();
            let icon_path = if is_lnk { &actual_path } else { &path_clone };
            let path_u16: Vec<u16> = icon_path.encode_utf16().chain(std::iter::once(0)).collect();
            let res = SHGetFileInfoW(windows::core::PCWSTR(path_u16.as_ptr()), Default::default(), Some(&mut shfi), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);

            if res != 0 && !shfi.hIcon.is_invalid() {
                let base64_icon = icon_to_base64(shfi.hIcon);
                let _ = DestroyIcon(shfi.hIcon);
                if let Some(ref base64) = base64_icon {
                    if let Ok(mut lock) = ICON_CACHE.get().unwrap().lock() { lock.insert(ck_clone, base64.clone()); }
                }
                Some(base64_icon)
            } else { None }
        };
        CoUninitialize();
        Ok(result.flatten())
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn save_pinned_apps(app: AppHandle, apps: Vec<AppInfo>) -> Result<(), String> {
    let path = app.path().app_config_dir().map_err(|e| e.to_string())?.join("pinned_apps.json");
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let content = serde_json::to_string(&apps).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn load_pinned_apps(app: AppHandle) -> Vec<AppInfo> {
    let path = app.path().app_config_dir().unwrap_or_default().join("pinned_apps.json");
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(apps) = serde_json::from_str(&content) { return apps; }
    }
    vec![
        AppInfo { name: "File Explorer".into(), path: "C:\\Windows\\explorer.exe".into(), icon: None, is_running: false, hwnd: None, executable: Some("explorer.exe".into()), all_hwnds: None },
        AppInfo { name: "Microsoft Edge".into(), path: "msedge".into(), icon: None, is_running: false, hwnd: None, executable: Some("msedge.exe".into()), all_hwnds: None },
        AppInfo { name: "Notepad".into(), path: "notepad.exe".into(), icon: None, is_running: false, hwnd: None, executable: Some("notepad.exe".into()), all_hwnds: None },
        AppInfo { name: "Settings".into(), path: "bloom-settings".into(), icon: None, is_running: false, hwnd: None, executable: None, all_hwnds: None },
    ]
}

#[tauri::command]
pub async fn clear_icon_cache(app: AppHandle) -> Result<(), String> {
    let cache_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let cache_path = cache_dir.join("icons_cache.json");
    let _ = std::fs::remove_file(&cache_path);
    if let Some(c) = ICON_CACHE.get() {
        if let Ok(mut lock) = c.lock() {
            lock.clear();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn set_custom_icon(app: AppHandle, cache_key: String, icon_data: String) -> Result<String, String> {
    use base64::Engine;
    use image::GenericImageView;

    // Validate and decode the base64 data URI
    let b64_str = if icon_data.starts_with("data:") {
        icon_data.split(',').nth(1).ok_or("Invalid data URI")?
    } else {
        &icon_data
    };

    let raw_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_str)
        .map_err(|e| format!("Invalid base64: {}", e))?;

    // Decode image to validate and get dimensions
    let img = image::load_from_memory(&raw_bytes)
        .map_err(|e| format!("Invalid image: {}", e))?;

    let (w, h) = img.dimensions();
    if w < 16 || h < 16 {
        return Err("Icon must be at least 16x16 pixels".into());
    }

    // Resize to fit within 256x256 preserving aspect ratio, then center on transparent canvas
    let target = 256u32;
    let final_img = if w > target || h > target {
        let scaled = img.resize(target, target, image::imageops::FilterType::Lanczos3);
        let (sw, sh) = scaled.dimensions();
        let mut canvas = image::RgbaImage::new(target, target);
        let ox = (target - sw) / 2;
        let oy = (target - sh) / 2;
        image::imageops::overlay(&mut canvas, &scaled, ox as i64, oy as i64);
        image::DynamicImage::ImageRgba8(canvas)
    } else {
        img
    };

    // Encode to PNG bytes
    let mut png_bytes: Vec<u8> = Vec::new();
    final_img.write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode PNG: {}", e))?;

    // Save to custom_icons directory
    let icons_dir = get_custom_icons_dir(&app)?;
    let filename = format!("{}.png", sanitize_filename(&cache_key));
    let file_path = icons_dir.join(&filename);
    std::fs::write(&file_path, &png_bytes).map_err(|e| e.to_string())?;

    // Return the data URI for immediate use
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}

#[tauri::command]
pub async fn remove_custom_icon(app: AppHandle, cache_key: String) -> Result<(), String> {
    let icons_dir = get_custom_icons_dir(&app)?;
    let filename = format!("{}.png", sanitize_filename(&cache_key));
    let file_path = icons_dir.join(&filename);
    if file_path.exists() {
        std::fs::remove_file(&file_path).map_err(|e| e.to_string())?;
    }
    // Also remove from persistent cache so the original icon is re-fetched
    if let Some(c) = ICON_CACHE.get() {
        if let Ok(mut lock) = c.lock() {
            lock.remove(&cache_key);
        }
    }
    let cache_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let cache_path = cache_dir.join("icons_cache.json");
    if let Some(c) = ICON_CACHE.get() {
        if let Ok(lock) = c.lock() {
            let _ = std::fs::write(&cache_path, serde_json::to_string(&*lock).unwrap_or_default());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_custom_icons(app: AppHandle) -> Result<HashMap<String, String>, String> {
    let icons_dir = get_custom_icons_dir(&app)?;
    let mut result = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(&icons_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("png") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(data) = std::fs::read(&path) {
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                        result.insert(stem.to_string(), format!("data:image/png;base64,{}", b64));
                    }
                }
            }
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_installed_apps() -> Vec<AppInfo> {
    let cache = INSTALLED_APPS_CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let is_empty = if let Ok(lock) = cache.lock() { lock.is_empty() } else { true };
    if is_empty && !IS_SCANNING.load(Ordering::Relaxed) { crate::services::trigger_app_scan(); }
    let start = std::time::Instant::now();
    while IS_SCANNING.load(Ordering::Relaxed) && start.elapsed() < std::time::Duration::from_secs(5) { tokio::time::sleep(std::time::Duration::from_millis(100)).await; }
    if let Ok(cache_lock) = cache.lock() { cache_lock.clone() } else { Vec::new() }
}

#[tauri::command]
pub fn broadcast_setting(app: AppHandle, key: String, value: serde_json::Value) {
    let _ = app.emit("settings-changed", serde_json::json!({ "key": key, "value": value }));
}

#[tauri::command]
pub fn hide_native_osd() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
        let class1 = windows::core::PCSTR(c"NativeHWNDHost".as_ptr() as *const u8);
        if let Ok(hwnd) = FindWindowA(class1, windows::core::PCSTR::null()) { let _ = ShowWindow(hwnd, SW_HIDE); }
    }
}

#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        if let Ok(hwnd) = win.hwnd() {
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

#[tauri::command]
pub fn hide_overlay(app: AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") { let _ = win.hide(); }
}

#[tauri::command]
pub fn set_splash_fullscreen(app: AppHandle, fullscreen: bool) {
    crate::state::OVERLAY_IN_SPLASH.store(fullscreen, Ordering::Relaxed);
    if let Some(win) = app.get_webview_window("overlay") {
        if fullscreen {
            let _ = win.hide();
            if let Ok(Some(monitor)) = win.primary_monitor() {
                let size = monitor.size();
                let pos = monitor.position();
                let _ = win.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
                let _ = win.set_size(tauri::PhysicalSize::new(size.width, size.height));
            }
            let _ = win.show();
        } else {
            let _ = win.hide();
            crate::services::sync_overlays(&app);
        }
    }
}

#[tauri::command]
pub fn sync_overlay_position(app: AppHandle) {
    crate::services::sync_overlays(&app);
}

#[tauri::command]
pub fn open_wifi_settings() {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::Win32::UI::Shell::ShellExecuteA;
        let _ = ShellExecuteA(Some(HWND(std::ptr::null_mut())), windows::core::PCSTR(c"open".as_ptr() as *const u8), windows::core::PCSTR(c"ms-availablenetworks:".as_ptr() as *const u8), windows::core::PCSTR::null(), windows::core::PCSTR::null(), SW_SHOWNORMAL);
    }
}

#[tauri::command]
pub fn open_sound_settings() {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::Win32::UI::Shell::ShellExecuteA;
        let _ = ShellExecuteA(Some(HWND(std::ptr::null_mut())), windows::core::PCSTR(c"open".as_ptr() as *const u8), windows::core::PCSTR(c"ms-settings:sound".as_ptr() as *const u8), windows::core::PCSTR::null(), windows::core::PCSTR::null(), SW_SHOWNORMAL);
    }
}

#[tauri::command]
pub fn open_notification_center() {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::Win32::UI::Shell::ShellExecuteA;
        let _ = ShellExecuteA(Some(HWND(std::ptr::null_mut())), windows::core::PCSTR(c"open".as_ptr() as *const u8), windows::core::PCSTR(c"ms-actioncenter:".as_ptr() as *const u8), windows::core::PCSTR::null(), windows::core::PCSTR::null(), SW_SHOWNORMAL);
    }
}

#[tauri::command]
pub fn open_system_tray() {
    tauri::async_runtime::spawn_blocking(move || unsafe {
        use std::sync::atomic::Ordering;
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, GetWindowLongA, SetWindowLongA, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_TRANSPARENT, SetLayeredWindowAttributes, LWA_ALPHA, IsWindowVisible, ShowWindow, SW_SHOW};
        use windows::core::PCSTR;

        let tray_class = PCSTR(c"Shell_TrayWnd".as_ptr() as *const u8);
        let hwnd = FindWindowA(tray_class, windows::core::PCSTR::null()).unwrap_or_default();
        if hwnd.0.is_null() { return; }

        let currently_hidden = crate::state::NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed);
        if currently_hidden {
            // 1. Make the taskbar completely invisible and click-through
            let exstyle = GetWindowLongA(hwnd, GWL_EXSTYLE);
            let _ = SetWindowLongA(hwnd, GWL_EXSTYLE, exstyle | WS_EX_LAYERED.0 as i32 | WS_EX_TRANSPARENT.0 as i32);
            let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 0, LWA_ALPHA);

            // 2. Show the taskbar window WITHOUT calling ABM_SETSTATE (prevents work-area
            //    recalculation which would displace the notch/dock appbars).
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE};
            use crate::utils::ORIGINAL_TRAY_RECT;
            if let Some(rect) = ORIGINAL_TRAY_RECT {
                let _ = SetWindowPos(hwnd, None, rect.left, rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
            let _ = ShowWindow(hwnd, SW_SHOW);
            crate::state::NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);
            
            // Give Windows a moment to realize the taskbar is "there"
            std::thread::sleep(std::time::Duration::from_millis(50));

            // 3. Send the Win+B and Space macro to open the tray chevron
            use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, KEYBDINPUT, VK_LWIN, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_SPACE};
            let b_key = VIRTUAL_KEY(0x42); // 'B' key
            
            let inputs = [
                // Win+B
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_LWIN, wScan: 0, dwFlags: Default::default(), time: 0, dwExtraInfo: 0 } },
                },
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: b_key, wScan: 0, dwFlags: Default::default(), time: 0, dwExtraInfo: 0 } },
                },
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: b_key, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } },
                },
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_LWIN, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } },
                },
            ];
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            
            std::thread::sleep(std::time::Duration::from_millis(150));
            
            // Space to open the popup
            let space_inputs = [
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_SPACE, wScan: 0, dwFlags: Default::default(), time: 0, dwExtraInfo: 0 } },
                },
                INPUT {
                    r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_SPACE, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } },
                },
            ];
            SendInput(&space_inputs, std::mem::size_of::<INPUT>() as i32);

            // 4. Start monitoring for the tray popup to close
            std::thread::spawn(move || {
                let overflow_class = PCSTR(c"TopLevelWindowForOverflowXamlIsland".as_ptr() as *const u8);
                let win10_overflow_class = PCSTR(c"NotifyIconOverflowWindow".as_ptr() as *const u8);
                
                // Wait for it to appear
                let mut found = false;
                for _ in 0..50 {
                    let h1 = FindWindowA(overflow_class, PCSTR::null()).unwrap_or_default();
                    let h2 = FindWindowA(win10_overflow_class, PCSTR::null()).unwrap_or_default();
                    if (!h1.0.is_null() && IsWindowVisible(h1).as_bool()) || (!h2.0.is_null() && IsWindowVisible(h2).as_bool()) {
                        found = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }

                if found {
                    // Wait for it to disappear
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        let h1 = FindWindowA(overflow_class, PCSTR::null()).unwrap_or_default();
                        let h2 = FindWindowA(win10_overflow_class, PCSTR::null()).unwrap_or_default();
                        let visible = (!h1.0.is_null() && IsWindowVisible(h1).as_bool()) || (!h2.0.is_null() && IsWindowVisible(h2).as_bool());
                        if !visible {
                            break;
                        }
                    }
                }
                
                // Once closed, hide taskbar again
                crate::utils::set_taskbar_visibility(false, false);
                crate::state::NATIVE_TASKBAR_HIDDEN.store(true, Ordering::Relaxed);
                
                // Revert transparency
                let tray_class = PCSTR(c"Shell_TrayWnd".as_ptr() as *const u8);
                let hwnd = FindWindowA(tray_class, PCSTR::null()).unwrap_or_default();
                if !hwnd.0.is_null() {
                    let exstyle = GetWindowLongA(hwnd, GWL_EXSTYLE);
                    let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 255, LWA_ALPHA);
                    let _ = SetWindowLongA(hwnd, GWL_EXSTYLE, exstyle & !(WS_EX_LAYERED.0 as i32) & !(WS_EX_TRANSPARENT.0 as i32));
                }
            });

        } else {
            // If already toggled on manually, toggle off
            crate::utils::set_taskbar_visibility(false, false);
            crate::state::NATIVE_TASKBAR_HIDDEN.store(true, Ordering::Relaxed);

            let exstyle = GetWindowLongA(hwnd, GWL_EXSTYLE);
            let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 255, LWA_ALPHA);
            let _ = SetWindowLongA(hwnd, GWL_EXSTYLE, exstyle & !(WS_EX_LAYERED.0 as i32) & !(WS_EX_TRANSPARENT.0 as i32));
        }
    });
}

#[tauri::command]
pub fn media_play_pause() { unsafe { if let Some(ref sender) = COMMAND_SENDER { let _ = sender.send(crate::types::SystemCommand::MediaPlayPause); } } }

#[tauri::command]
pub fn media_next() { unsafe { if let Some(ref sender) = COMMAND_SENDER { let _ = sender.send(crate::types::SystemCommand::MediaNext); } } }

#[tauri::command]
pub fn media_previous() { unsafe { if let Some(ref sender) = COMMAND_SENDER { let _ = sender.send(crate::types::SystemCommand::MediaPrevious); } } }

#[tauri::command]
pub fn media_seek(position_ms: f64) { unsafe { if let Some(ref sender) = COMMAND_SENDER { let _ = sender.send(crate::types::SystemCommand::MediaSeek(position_ms as i64)); } } }

#[tauri::command]
pub fn open_media_source_app() {
    unsafe {
        use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;
        use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, IsIconic, IsWindowVisible, FindWindowW, EnumWindows, GetWindowThreadProcessId};
        use windows::Win32::Foundation::{HWND, LPARAM};
        use windows::Win32::System::Threading::OpenProcess;

        let mgr = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()) {
            Ok(m) => m,
            Err(_) => return,
        };
        let session = match mgr.GetCurrentSession() {
            Ok(s) => s,
            Err(_) => return,
        };
        let app_id = match session.SourceAppUserModelId() {
            Ok(id) => id.to_string(),
            Err(_) => return,
        };
        if app_id.is_empty() { return; }

        // First try: FindWindow with the AppUserModelId directly (works for UWP)
        let h_app_id = windows::core::HSTRING::from(&app_id);
        if let Ok(hwnd) = FindWindowW(None, windows::core::PCWSTR(h_app_id.as_ptr())) {
            if !hwnd.is_invalid() {
                if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
                return;
            }
        }

        // Second try: find window by process name extracted from app_id
        let process_name = app_id.split('.').next().unwrap_or(&app_id).to_lowercase();

        struct EnumData { process_name: String, found_hwnd: Option<HWND> }

        unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let data = &mut *(lparam.0 as *mut EnumData);
            if !IsWindowVisible(hwnd).as_bool() {
                return windows::core::BOOL(1);
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 { return windows::core::BOOL(1); }

            if let Ok(proc) = OpenProcess(
                windows::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            ) {
                let mut buf = [0u16; 260];
                let mut size = buf.len() as u32;
                let ok = windows::Win32::System::Threading::QueryFullProcessImageNameW(
                    proc,
                    windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut size,
                );
                let _ = windows::Win32::Foundation::CloseHandle(proc);
                if ok.is_ok() {
                    let path = String::from_utf16_lossy(&buf[..size as usize]);
                    let file_name = path.rsplit('\\').next().unwrap_or("").to_lowercase().replace(".exe", "");
                    if file_name == data.process_name {
                        data.found_hwnd = Some(hwnd);
                        return windows::core::BOOL(0);
                    }
                }
            }
            windows::core::BOOL(1)
        }

        let mut data = EnumData { process_name, found_hwnd: None };
        let callback: unsafe extern "system" fn(HWND, LPARAM) -> windows::core::BOOL = enum_callback;
        let _ = EnumWindows(Some(callback), LPARAM(&mut data as *mut EnumData as isize));

        if let Some(hwnd) = data.found_hwnd {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[tauri::command]
pub fn get_audio_output_devices() -> Result<Vec<AudioDevice>, String> {
    unsafe {
        use windows::Win32::Media::Audio::{IMMDeviceEnumerator, eRender, eConsole, DEVICE_STATE_ACTIVE};
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
        use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(
            &windows::Win32::Media::Audio::MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ).map_err(|e| format!("Failed to create device enumerator: {}", e))?;

        // Get default device ID
        let default_device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("Failed to get default endpoint: {}", e))?;
        let default_id = default_device.GetId()
            .map(|id| {
                let s = windows::core::PCWSTR::from_raw(id.0).to_string().unwrap_or_default();
                windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
                s
            })
            .unwrap_or_default();

        // Enumerate all active render endpoints
        let collection = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("Failed to enumerate endpoints: {}", e))?;
        let count = collection.GetCount().map_err(|e| format!("Failed to get count: {}", e))?;

        let mut devices = Vec::new();
        for i in 0..count {
            if let Ok(device) = collection.Item(i) {
                let id = device.GetId().map(|id| {
                    let s = windows::core::PCWSTR::from_raw(id.0).to_string().unwrap_or_default();
                    windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
                    s
                }).unwrap_or_default();

                let name = device.OpenPropertyStore(STGM_READ).ok()
                    .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                    .map(|val| {
                        let pwstr = val.Anonymous.Anonymous.Anonymous.pwszVal;
                        windows::core::PCWSTR(pwstr.0 as *const _).to_string().unwrap_or_default()
                    })
                    .unwrap_or_else(|| "Unknown Device".to_string());

                let is_default = id == default_id;
                devices.push(AudioDevice { id, name, is_default });
            }
        }

        Ok(devices)
    }
}

#[tauri::command]
pub fn set_audio_output_device(device_id: String) -> Result<(), String> {
    // Uses the undocumented IPolicyConfig COM interface (stable since Vista/Win7).
    // Replaces the prior approach of generating + running C# via PowerShell Add-Type,
    // which AV engines classify as reflective code injection.
    //
    // PolicyConfigClient CLSID: {870af99c-e543-481c-8303-f0d7e7e06526}
    // IPolicyConfig      IID:   {f8679f50-84e7-43cd-b950-c298f2188e5c}
    // Vtable: [0]=QI [1]=AddRef [2]=Release [3–12]=stubs [13]=SetDefaultEndpoint
    unsafe {
        #[link(name = "ole32")]
        extern "system" {
            fn CoCreateInstance(
                rclsid: *const windows::core::GUID,
                punk_outer: *mut std::ffi::c_void,
                dwclscontext: u32,
                riid: *const windows::core::GUID,
                ppv: *mut *mut std::ffi::c_void,
            ) -> i32;
        }

        let clsid = windows::core::GUID {
            data1: 0x870a_f99c, data2: 0xe543, data3: 0x481c,
            data4: [0x83, 0x03, 0xf0, 0xd7, 0xe7, 0xe0, 0x65, 0x26],
        };
        let iid = windows::core::GUID {
            data1: 0xf867_9f50, data2: 0x84e7, data3: 0x43cd,
            data4: [0xb9, 0x50, 0xc2, 0x98, 0xf2, 0x18, 0x8e, 0x5c],
        };

        let mut ppv: *mut std::ffi::c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(&clsid, std::ptr::null_mut(), 0x17 /* CLSCTX_ALL */, &iid, &mut ppv);

        if hr < 0 || ppv.is_null() {
            // COM failed — open sound settings as fallback
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "ms-settings:sound"])
                .creation_flags(0x08000000)
                .spawn();
            return Ok(());
        }

        let vtable = *(ppv as *const *const usize);
        type SetDefaultEndpointFn = unsafe extern "system" fn(*mut std::ffi::c_void, *const u16, i32) -> i32;
        let set_default_endpoint: SetDefaultEndpointFn = std::mem::transmute(*vtable.add(13));

        let wide_id: Vec<u16> = device_id.encode_utf16().chain(std::iter::once(0)).collect();
        // Apply for all three audio roles: eConsole=0, eMultimedia=1, eCommunications=2
        for role in 0i32..=2 {
            let _ = set_default_endpoint(ppv, wide_id.as_ptr(), role);
        }

        // Release the COM object (IUnknown::Release = vtable[2])
        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release: ReleaseFn = std::mem::transmute(*vtable.add(2));
        release(ppv);
    }
    Ok(())
}

#[tauri::command]
pub fn set_volume(volume: f32) { unsafe { if let Some(ref sender) = COMMAND_SENDER { let _ = sender.send(crate::types::SystemCommand::SetVolume(volume)); } } }

/// Restore the native taskbar, unregister Bloom's appbars, and exit gracefully.
/// Shared by the tray menu, the in-app Quit button, and the window CloseRequested
/// handlers so that any shutdown path (including Task Manager's WM_CLOSE) behaves
/// identically.
pub fn restore_taskbar_and_exit(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("main") {
        if MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) { unregister_appbar_native(w.hwnd().unwrap()); }
    }
    if let Some(w) = handle.get_webview_window("dock") {
        if DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) { unregister_appbar_native(w.hwnd().unwrap()); }
    }
    set_taskbar_visibility(true, true);
    NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);
    // Destroy all webview windows before exiting so Chromium's UnregisterClass for
    // Chrome_WidgetWin_0 finds no open windows (avoids the harmless Error=1412 log).
    for (_, w) in handle.webview_windows() {
        let _ = w.destroy();
    }
    handle.exit(0);
}

#[tauri::command]
pub async fn quit_bloom(handle: AppHandle) {
    restore_taskbar_and_exit(&handle);
}

#[tauri::command]
pub async fn restart_bloom(handle: AppHandle) {
    if let Some(w) = handle.get_webview_window("main") { unregister_appbar_native(w.hwnd().unwrap()); }
    if let Some(w) = handle.get_webview_window("dock") { unregister_appbar_native(w.hwnd().unwrap()); }
    if let Some(w) = handle.get_webview_window("settings") { let _ = w.destroy(); }
    set_taskbar_visibility(true, true);
    NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);
    close_single_instance_handles();
    handle.restart();
}

#[tauri::command]
pub async fn close_window(hwnd: isize) {
    tauri::async_runtime::spawn_blocking(move || unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let hwnd = HWND(hwnd as *mut _);
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
    }).await.unwrap_or_default();
}

#[tauri::command]
pub fn save_setting(app: AppHandle, key: String, value: serde_json::Value) -> Result<(), String> {
    let path = app.path().app_config_dir().map_err(|e| e.to_string())?.join("settings.json");
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let mut settings = HashMap::new();
    if let Ok(content) = std::fs::read_to_string(&path) { if let Ok(existing) = serde_json::from_str::<HashMap<String, serde_json::Value>>(&content) { settings = existing; } }
    settings.insert(key.clone(), value);
    let content = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())?;
    
    if key == "bloom-scale" {
        if let Some(main_win) = app.get_webview_window("main") {
            let notch_fixed = settings.get("bloom-notch-mode").map(|v| v.as_str() == Some("fixed")).unwrap_or(true);
            if notch_fixed {
                crate::services::register_appbar(main_win);
            }
        }
        if let Some(dock_win) = app.get_webview_window("dock") {
            let is_fixed = settings.get("bloom-dock-mode").map(|v| v.as_str() == Some("fixed")).unwrap_or(false);
            if is_fixed {
                crate::services::register_dock_appbar(dock_win);
            }
        }
    }
    Ok(())
}


#[tauri::command]
pub fn load_settings(app: AppHandle) -> Result<HashMap<String, serde_json::Value>, String> {
    let path = match app.path().app_config_dir() {
        Ok(p) => p.join("settings.json"),
        Err(_) => return Ok(HashMap::new()),
    };
    if let Ok(content) = std::fs::read_to_string(path) { if let Ok(settings) = serde_json::from_str(&content) { return Ok(settings); } }
    Ok(HashMap::new())
}

#[tauri::command]
pub async fn capture_window_thumbnail(hwnd: isize, max_width: u32, max_height: u32) -> Result<Option<(String, i64)>, String> {
    tauri::async_runtime::spawn_blocking(move || unsafe {
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, CreateCompatibleBitmap, SelectObject, DeleteObject, DeleteDC, GetDC, ReleaseDC, GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC};
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT, IsWindow, IsIconic, ShowWindow, SW_SHOWNOACTIVATE, SW_MINIMIZE};
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
        #[link(name = "user32")]
        extern "system" { pub fn PrintWindow(hwnd: HWND, hdcBlt: HDC, nFlags: u32) -> i32; }

        let hwnd = HWND(hwnd as *mut _);
        if !IsWindow(Some(hwnd)).as_bool() { return None; }

        let hwnd_key = hwnd.0 as isize;
        let is_minimized = IsIconic(hwnd).as_bool();

        if is_minimized {
            if let Some(cache) = crate::state::THUMBNAIL_CACHE.get() {
                if let Ok(guard) = cache.lock() {
                    if let Some((cached, _)) = guard.get(&hwnd_key) {
                        let mut focus_time = 0i64;
                        if let Some(map) = crate::state::FOCUS_TIMESTAMPS.get() {
                            if let Ok(f_guard) = map.lock() {
                                focus_time = f_guard.get(&hwnd_key).copied().unwrap_or(0);
                            }
                        }
                        return Some((cached.clone(), focus_time));
                    }
                }
            }
        }

        let mut rect = RECT::default();
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32);
        if rect.right == 0 && rect.bottom == 0 && windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_err() { return None; }
        let mut width = rect.right - rect.left;
        let mut height = rect.bottom - rect.top;
        if width <= 10 || height <= 10 {
            let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            if GetWindowPlacement(hwnd, &mut wp).is_ok() {
                width = wp.rcNormalPosition.right - wp.rcNormalPosition.left;
                height = wp.rcNormalPosition.bottom - wp.rcNormalPosition.top;
            }
        }
        if width <= 100 || height <= 100 { return None; }

        let mut did_restore = false;
        if is_minimized {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            std::thread::sleep(std::time::Duration::from_millis(60));
            did_restore = true;
        }

        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let hbm_mem = CreateCompatibleBitmap(hdc_screen, width, height);
        let h_old = SelectObject(hdc_mem, hbm_mem.into());
        let success = PrintWindow(hwnd, hdc_mem, 2);

        let mut result = None;
        if success != 0 {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: width, biHeight: -height, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, biSizeImage: 0, biXPelsPerMeter: 0, biYPelsPerMeter: 0, biClrUsed: 0, biClrImportant: 0 },
                bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default(); 1],
            };
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            if GetDIBits(hdc_mem, hbm_mem, 0, height as u32, Some(pixels.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS) != 0 {
                for chunk in pixels.chunks_exact_mut(4) { let b = chunk[0]; let r = chunk[2]; chunk[0] = r; chunk[2] = b; chunk[3] = 255; }
                if let Ok(Some(png_base64)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Some(mut img) = image::RgbaImage::from_raw(width as u32, height as u32, pixels) {
                        if img.width() > max_width || img.height() > max_height {
                            let dyn_img = image::DynamicImage::ImageRgba8(img);
                            img = dyn_img.resize(max_width, max_height, image::imageops::FilterType::Triangle).into_rgba8();
                        }
                        let mut buf = std::io::Cursor::new(Vec::new());
                        if image::write_buffer_with_format(&mut buf, &img, img.width(), img.height(), image::ColorType::Rgba8, image::ImageFormat::Png).is_ok() {
                            use base64::Engine;
                            let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
                            return Some(format!("data:image/png;base64,{}", b64));
                        }
                    }
                    None
                })) { result = Some(png_base64); }
            }
        }
        SelectObject(hdc_mem, h_old);
        let _ = DeleteObject(hbm_mem.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        if did_restore {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        }

        if let Some(ref img) = result {
            if let Some(cache) = crate::state::THUMBNAIL_CACHE.get() {
                if let Ok(mut guard) = cache.lock() {
                    guard.insert(hwnd_key, (img.clone(), crate::utils::get_now_ms()));
                }
            }
        }

        let mut focus_time = 0i64;
        if let Some(map) = crate::state::FOCUS_TIMESTAMPS.get() {
            if let Ok(guard) = map.lock() {
                focus_time = guard.get(&hwnd_key).copied().unwrap_or(0);
            }
        }

        result.map(|img| (img, focus_time))
    }).await.map_err(|e| e.to_string())
}


// ── Radio helpers — Windows Runtime Windows.Devices.Radios ───────────────────
// Replaces PowerShell -ExecutionPolicy Bypass scripts for WiFi/Bluetooth state.

fn get_radio_state_sync(kind: windows::Devices::Radios::RadioKind) -> Result<bool, String> {
    use windows::Devices::Radios::{Radio, RadioState};
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None, windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
    let radios = Radio::GetRadiosAsync()
        .and_then(|op| op.get())
        .map_err(|e| e.to_string())?;
    for i in 0..radios.Size().unwrap_or(0) {
        if let Ok(radio) = radios.GetAt(i) {
            if radio.Kind().ok() == Some(kind) {
                return Ok(radio.State().ok() == Some(RadioState::On));
            }
        }
    }
    Ok(false)
}

fn set_radio_state_sync(kind: windows::Devices::Radios::RadioKind, enabled: bool) -> Result<(), String> {
    use windows::Devices::Radios::{Radio, RadioState};
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None, windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
    let radios = Radio::GetRadiosAsync()
        .and_then(|op| op.get())
        .map_err(|e| e.to_string())?;
    for i in 0..radios.Size().unwrap_or(0) {
        if let Ok(radio) = radios.GetAt(i) {
            if radio.Kind().ok() == Some(kind) {
                let target = if enabled { RadioState::On } else { RadioState::Off };
                let _ = radio.SetStateAsync(target).and_then(|op| op.get());
                return Ok(());
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_volume() -> f32 {
    crate::state::CURRENT_VOLUME.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.0
}

#[tauri::command]
pub fn get_brightness() -> u32 {
    crate::state::CURRENT_BRIGHTNESS.load(std::sync::atomic::Ordering::Relaxed)
}

#[tauri::command]
pub async fn get_wifi_state() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(|| {
        get_radio_state_sync(windows::Devices::Radios::RadioKind::WiFi)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_wifi_state(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        set_radio_state_sync(windows::Devices::Radios::RadioKind::WiFi, enabled)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_bluetooth_state() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(|| {
        get_radio_state_sync(windows::Devices::Radios::RadioKind::Bluetooth)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_bluetooth_state(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        set_radio_state_sync(windows::Devices::Radios::RadioKind::Bluetooth, enabled)
    }).await.map_err(|e| e.to_string())?
}

// Settings openers — use ShellExecuteA directly instead of spawning powershell

#[tauri::command]
pub fn open_bluetooth_settings() {
    unsafe {
        use windows::Win32::UI::Shell::ShellExecuteA;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let _ = ShellExecuteA(None, windows::core::PCSTR(c"open".as_ptr() as *const u8), windows::core::PCSTR(c"ms-settings:bluetooth".as_ptr() as *const u8), windows::core::PCSTR::null(), windows::core::PCSTR::null(), SW_SHOWNORMAL);
    }
}

#[tauri::command]
pub fn open_airplane_mode_settings() {
    unsafe {
        use windows::Win32::UI::Shell::ShellExecuteA;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let _ = ShellExecuteA(None, windows::core::PCSTR(c"open".as_ptr() as *const u8), windows::core::PCSTR(c"ms-settings:network-airplanemode".as_ptr() as *const u8), windows::core::PCSTR::null(), windows::core::PCSTR::null(), SW_SHOWNORMAL);
    }
}

#[tauri::command]
pub fn set_brightness(app: AppHandle, brightness: u32) {
    let val = brightness.min(100);
    crate::state::CURRENT_BRIGHTNESS.store(val, Ordering::Relaxed);
    crate::state::LAST_BRIGHTNESS_CHANGE.store(crate::utils::get_now_ms(), Ordering::Relaxed);
    let _ = app.emit("brightness-change", BrightnessChangeEvent { brightness: val });
    if let Some(tx) = crate::state::BRIGHTNESS_SENDER.get() {
        let _ = tx.send(val);
    }
}

#[tauri::command]
pub async fn get_battery_saver_state() -> Result<bool, String> {
    // Uses Windows Runtime PowerManager — no PowerShell required.
    tauri::async_runtime::spawn_blocking(|| {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None, windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        use windows::System::Power::{EnergySaverStatus, PowerManager};
        match PowerManager::EnergySaverStatus() {
            Ok(status) => Ok(status == EnergySaverStatus::On),
            Err(_) => Ok(false), // No battery / not supported on this device
        }
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn open_battery_saver_settings() {
    unsafe {
        use windows::Win32::UI::Shell::ShellExecuteA;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let _ = ShellExecuteA(None, windows::core::PCSTR(c"open".as_ptr() as *const u8), windows::core::PCSTR(c"ms-settings:batterysaver".as_ptr() as *const u8), windows::core::PCSTR::null(), windows::core::PCSTR::null(), SW_SHOWNORMAL);
    }
}


pub fn get_windows_accent_color() -> Option<String> {
    unsafe {
        use windows::Win32::System::Registry::{RegOpenKeyExW, RegQueryValueExW, RegCloseKey, HKEY_CURRENT_USER, KEY_READ, REG_BINARY, REG_DWORD};
        
        // 1. Try reading AccentPalette from Explorer\Accent for the true system accent color
        let subkey = windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent");
        let mut hkey = windows::Win32::System::Registry::HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_READ, &mut hkey).is_ok() {
            let mut value_type = REG_BINARY;
            let mut palette = [0u8; 32];
            let mut value_size = 32u32;
            let val_name = windows::core::w!("AccentPalette");
            if RegQueryValueExW(
                hkey,
                val_name,
                None,
                Some(&mut value_type),
                Some(palette.as_mut_ptr()),
                Some(&mut value_size)
            ).is_ok() {
                let _ = RegCloseKey(hkey);
                if value_size >= 15 {
                    // bytes 12, 13, 14 are R, G, B of the active accent color
                    let r = palette[12];
                    let g = palette[13];
                    let b = palette[14];
                    return Some(format!("#{:02x}{:02x}{:02x}", r, g, b));
                }
            } else {
                let _ = RegCloseKey(hkey);
            }
        }

        // 2. Fallback to DWM\AccentColor
        let subkey = windows::core::w!("Software\\Microsoft\\Windows\\DWM");
        let mut hkey = windows::Win32::System::Registry::HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey, Some(0), KEY_READ, &mut hkey).is_ok() {
            let mut value_type = REG_DWORD;
            let mut color_val = 0u32;
            let mut value_size = std::mem::size_of::<u32>() as u32;
            let val_name = windows::core::w!("AccentColor");
            if RegQueryValueExW(
                hkey,
                val_name,
                None,
                Some(&mut value_type),
                Some(&mut color_val as *mut u32 as *mut u8),
                Some(&mut value_size)
            ).is_ok() {
                let _ = RegCloseKey(hkey);
                // color_val is AABBGGRR (ABGR format)
                let r = (color_val & 0xff) as u8;
                let g = ((color_val >> 8) & 0xff) as u8;
                let b = ((color_val >> 16) & 0xff) as u8;
                return Some(format!("#{:02x}{:02x}{:02x}", r, g, b));
            }
            let _ = RegCloseKey(hkey);
        }
    }
    None
}

#[tauri::command]
pub fn get_system_accent_color() -> Result<String, String> {
    // Try reading registry first for exact Windows accent color
    if let Some(color) = get_windows_accent_color() {
        return Ok(color);
    }

    // Fallback to DwmGetColorizationColor
    unsafe {
        let mut color = 0u32;
        let mut opaque = windows::core::BOOL(0);
        if windows::Win32::Graphics::Dwm::DwmGetColorizationColor(&mut color, &mut opaque).is_ok() {
            let r = ((color >> 16) & 0xff) as u8;
            let g = ((color >> 8) & 0xff) as u8;
            let b = (color & 0xff) as u8;
            let hex = format!("#{:02x}{:02x}{:02x}", r, g, b);
            Ok(hex)
        } else {
            Err("Failed to query colorization color".into())
        }
    }
}
