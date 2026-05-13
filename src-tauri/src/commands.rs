use tauri::{AppHandle, Emitter, Manager, Window};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
use std::sync::atomic::Ordering;

use crate::types::{IntRect, AppInfo};
use crate::state::*;
use crate::utils::*;
use crate::services::{register_appbar, register_dock_appbar, sync_overlays, unregister_appbar_native, enum_windows_proc};

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
pub fn set_ignore_cursor_events(window: Window, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}

#[tauri::command]
pub async fn toggle_dock(app: AppHandle, enable: bool) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        if enable {
            set_taskbar_visibility(false);
            NATIVE_TASKBAR_HIDDEN.store(true, Ordering::Relaxed);

            let _ = dock_win.set_always_on_top(true);
            let _ = dock_win.show();
        } else {
            let _ = dock_win.hide();
            if let Ok(hwnd) = dock_win.hwnd() {
                let hwnd_val = hwnd.0 as isize;
                tauri::async_runtime::spawn_blocking(move || {
                    unregister_appbar_native(HWND(hwnd_val as *mut _));
                });
            }
            DOCK_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
            set_taskbar_visibility(true);
            NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

            // Re-sync other appbars to ensure they stay in place
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
        }
    }
    if let Some(dock_win) = app.get_webview_window("dock") {
        if DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) && dock_win.is_visible().unwrap_or(false) {
            register_dock_appbar(dock_win);
        }
    }
    sync_overlays(&app);
}

#[tauri::command]
pub async fn change_dock_mode(app: AppHandle, mode: String) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        if mode == "fixed" {
            register_dock_appbar(dock_win.clone());
        } else if let Ok(hwnd) = dock_win.hwnd() {
            let hwnd_val = hwnd.0 as isize;
            tauri::async_runtime::spawn_blocking(move || {
                unregister_appbar_native(HWND(hwnd_val as *mut _));
            });
            DOCK_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
            
            // Force position even in auto-hide mode to ensure it's at the screen bottom
            if let Ok(Some(monitor)) = dock_win.primary_monitor() {
                let m_size = monitor.size();
                let m_pos = monitor.position();
                let scale = dock_win.scale_factor().unwrap_or(1.0);
                let ph = dock_win.outer_size().map(|s| s.height as i32).unwrap_or((600.0 * scale) as i32);
                let final_y = m_pos.y + m_size.height as i32 - ph;
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, SWP_FRAMECHANGED};
                    let _ = SetWindowPos(hwnd, None, m_pos.x, final_y, m_size.width as i32, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
                }
            }
        }
        
        // Ensure always on top and native taskbar stays hidden
        let _ = dock_win.set_always_on_top(true);
        set_taskbar_visibility(false);
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
        } else if let Ok(hwnd) = main_win.hwnd() {
            let hwnd_val = hwnd.0 as isize;
            tauri::async_runtime::spawn_blocking(move || {
                unregister_appbar_native(HWND(hwnd_val as *mut _));
            });
            MAIN_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
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
        let actual_path = if path.to_lowercase().ends_with(".lnk") {
            crate::utils::resolve_shortcut(&path).unwrap_or_else(|| path.clone())
        } else {
            path.clone()
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
            if file_name == "discord.exe" || file_name == "slack.exe" || file_name == "githubdesktop.exe" {
                if let Some(parent) = Path::new(&final_path).parent().and_then(|p| p.parent()) {
                    if parent.exists() {
                        if let Ok(entries) = std::fs::read_dir(parent) {
                            let mut app_dirs = Vec::new();
                            for entry in entries.flatten() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                if name.starts_with("app-") && entry.path().is_dir() {
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
        let mut apps = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut apps as *mut Vec<AppInfo> as isize));
        }
        // Deduplicate carefully: allow duplicates for host processes like msedge, chrome, explorer
        let mut seen = std::collections::HashSet::new();
        apps.into_iter().filter(|a: &AppInfo| {
            let p = a.path.to_lowercase();
            if p.contains("msedge.exe") || p.contains("chrome.exe") || p.contains("explorer.exe") || p.contains("applicationframehost.exe") {
                // For these, use path + name as unique key
                let key = format!("{}:{}", p, a.name);
                if seen.contains(&key) { return false; }
                seen.insert(key);
                true
            } else {
                if seen.contains(&p) { return false; }
                seen.insert(p);
                true
            }
        }).collect()
    }).await.unwrap_or_default()
}

#[tauri::command]
pub async fn focus_window(hwnd: isize) {
    tauri::async_runtime::spawn_blocking(move || unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW, SW_MINIMIZE, IsIconic, GetForegroundWindow, IsWindowVisible};
        let hwnd = HWND(hwnd as *mut _);
        let fg = GetForegroundWindow();
        
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        } else if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        } else if fg == hwnd {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
        } else {
            // Also check if the ancestor is the same (handles some UWP apps)
            use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};
            let fg_root = GetAncestor(fg, GA_ROOT);
            let hwnd_root = GetAncestor(hwnd, GA_ROOT);
            
            if fg_root == hwnd_root && !fg_root.is_invalid() {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            } else {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }).await.unwrap_or_default();
}

#[tauri::command]
pub async fn get_app_icon(app: AppHandle, path: String, hwnd: Option<isize>) -> Result<Option<String>, String> {
    let cache_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let cache_path = cache_dir.join("icons_cache.json");
    
    let cache = ICON_CACHE.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        if let Ok(content) = std::fs::read_to_string(&cache_path) {
            if let Ok(existing) = serde_json::from_str::<std::collections::HashMap<String, String>>(&content) {
                map = existing;
            }
        }
        std::sync::Mutex::new(map)
    });

    // 1. Check cache first
    if let Ok(c) = cache.lock() {
        if let Some(icon) = c.get(&path) {
            return Ok(Some(icon.clone()));
        }
    }

    // 2. Try to get icon from HWND if available (Best for PWAs/Netflix)
    if let Some(h) = hwnd {
        let result = tauri::async_runtime::spawn_blocking(move || unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED, CoUninitialize};
            use windows::Win32::UI::WindowsAndMessaging::{GetClassLongPtrW, GCLP_HICON, WM_GETICON, ICON_BIG, SendMessageTimeoutW, SMTO_ABORTIFHUNG};
            
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let h_hwnd = HWND(h as *mut _);
            
            // Try different ways to get the icon from HWND
            let mut h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(GetClassLongPtrW(h_hwnd, GCLP_HICON) as *mut _);
            if h_icon.is_invalid() {
                h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(GetClassLongPtrW(h_hwnd, windows::Win32::UI::WindowsAndMessaging::GCL_HICON) as *mut _);
            }

            if h_icon.is_invalid() {
                let mut res = 0usize;
                let _ = SendMessageTimeoutW(
                    h_hwnd, 
                    WM_GETICON, 
                    windows::Win32::Foundation::WPARAM(ICON_BIG as usize), 
                    windows::Win32::Foundation::LPARAM(0), 
                    SMTO_ABORTIFHUNG, 
                    250, // Increased timeout
                    Some(&mut res)
                );
                if res != 0 { h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(res as *mut _); }
            }
            
            if h_icon.is_invalid() {
                let mut res = 0usize;
                let _ = SendMessageTimeoutW(
                    h_hwnd, 
                    windows::Win32::UI::WindowsAndMessaging::WM_GETICON, 
                    windows::Win32::Foundation::WPARAM(windows::Win32::UI::WindowsAndMessaging::ICON_SMALL as usize), 
                    windows::Win32::Foundation::LPARAM(0), 
                    SMTO_ABORTIFHUNG, 
                    250, 
                    Some(&mut res)
                );
                if res != 0 { h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(res as *mut _); }
            }
            
            let res = if !h_icon.is_invalid() {
                icon_to_base64(h_icon)
            } else {
                None
            };
            CoUninitialize();
            res
        }).await.unwrap_or(None);

        if let Some(base64) = result {
            if let Ok(mut c) = cache.lock() {
                c.insert(path.clone(), base64.clone());
                let _ = std::fs::create_dir_all(&cache_dir);
                let _ = std::fs::write(&cache_path, serde_json::to_string(&*c).unwrap_or_default());
            }
            return Ok(Some(base64));
        }
    }

    // 3. Fallback to path-based extraction
    let path_clone = path.clone();
    tauri::async_runtime::spawn_blocking(move || unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED, CoUninitialize};
        use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
        use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;
        
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = {
            let (mut actual_path, is_lnk) = if path_clone.to_lowercase().ends_with(".lnk") {
                (resolve_shortcut(&path_clone).unwrap_or(path_clone.clone()), true)
            } else {
                (path_clone.clone(), false)
            };

            // Enhanced path resolution for common names
            if !std::path::Path::new(&actual_path).is_absolute() {
                if actual_path.to_lowercase() == "code" || actual_path.to_lowercase() == "code.exe" {
                    if let Ok(home) = std::env::var("USERPROFILE") {
                        let p1 = format!("{}\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe", home);
                        if std::path::Path::new(&p1).exists() { actual_path = p1; }
                    }
                } else if actual_path.to_lowercase() == "wt" || actual_path.to_lowercase() == "wt.exe" {
                   if let Ok(local) = std::env::var("LOCALAPPDATA") {
                        let p = format!("{}\\Microsoft\\WindowsApps\\wt.exe", local);
                        if std::path::Path::new(&p).exists() { actual_path = p; }
                   }
                } else if actual_path.to_lowercase() == "msedge" || actual_path.to_lowercase() == "msedge.exe" {
                    let p = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";
                    if std::path::Path::new(p).exists() { 
                        actual_path = p.to_string(); 
                    } else {
                        let p2 = r"C:\Program Files\Microsoft\Edge\Application\msedge.exe";
                        if std::path::Path::new(p2).exists() { actual_path = p2.to_string(); }
                    }
                } else {
                    let common_exes = vec![
                        r"C:\Windows\explorer.exe",
                        r"C:\Windows\System32\notepad.exe",
                        r"C:\Windows\System32\cmd.exe",
                    ];
                    for p in common_exes {
                        if p.to_lowercase().contains(&actual_path.to_lowercase()) {
                            actual_path = p.to_string();
                            break;
                        }
                    }
                }
            }

            let mut shfi: SHFILEINFOW = std::mem::zeroed();
            
            // If it's a .lnk, try getting the icon for the .lnk file first (contains custom icons for PWAs/Discord)
            let mut res = 0usize;
            if is_lnk {
                let lnk_u16: Vec<u16> = path_clone.encode_utf16().chain(std::iter::once(0)).collect();
                res = SHGetFileInfoW(
                    windows::core::PCWSTR(lnk_u16.as_ptr()),
                    Default::default(),
                    Some(&mut shfi),
                    std::mem::size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON
                );
            }

            // Fallback to actual_path if not a lnk or lnk icon fetch failed
            if res == 0 || shfi.hIcon.is_invalid() {
                let path_u16: Vec<u16> = actual_path.encode_utf16().chain(std::iter::once(0)).collect();
                res = SHGetFileInfoW(
                    windows::core::PCWSTR(path_u16.as_ptr()),
                    Default::default(),
                    Some(&mut shfi),
                    std::mem::size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON
                );
            }

            if res != 0 && !shfi.hIcon.is_invalid() {
                let base64_icon = icon_to_base64(shfi.hIcon);
                let _ = DestroyIcon(shfi.hIcon);
                if let Some(ref base64) = base64_icon {
                    if let Ok(mut lock) = ICON_CACHE.get().unwrap().lock() {
                        lock.insert(path_clone, base64.clone());
                        let _ = std::fs::write(&cache_path, serde_json::to_string(&*lock).unwrap_or_default());
                    }
                }
                Some(base64_icon)
            } else {
                None
            }
        };
        CoUninitialize();
        Ok(result.flatten())
    }).await.map_err(|e| e.to_string())?
}


#[tauri::command]
pub fn save_pinned_apps(app: AppHandle, apps: Vec<AppInfo>) -> Result<(), String> {
    let path = app.path().app_config_dir().map_err(|e| e.to_string())?
        .join("pinned_apps.json");
    
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    
    let content = serde_json::to_string(&apps).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn load_pinned_apps(app: AppHandle) -> Vec<AppInfo> {
    let path = app.path().app_config_dir().unwrap_or_default()
        .join("pinned_apps.json");
    
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(apps) = serde_json::from_str(&content) {
            return apps;
        }
    }
    
    // Default apps if none saved
    vec![
        AppInfo { name: "File Explorer".into(), path: "C:\\Windows\\explorer.exe".into(), icon: None, is_running: false, hwnd: None, executable: Some("explorer.exe".into()) },
        AppInfo { name: "Microsoft Edge".into(), path: "msedge".into(), icon: None, is_running: false, hwnd: None, executable: Some("msedge.exe".into()) },
        AppInfo { name: "Notepad".into(), path: "notepad.exe".into(), icon: None, is_running: false, hwnd: None, executable: Some("notepad.exe".into()) },
    ]
}

#[tauri::command]
pub async fn get_installed_apps() -> Vec<AppInfo> {
    let cache = INSTALLED_APPS_CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    
    // If empty and not scanning, trigger one
    let is_empty = if let Ok(lock) = cache.lock() { lock.is_empty() } else { true };
    if is_empty && !IS_SCANNING.load(Ordering::Relaxed) {
        crate::services::trigger_app_scan();
    }
    
    // Wait for scanning to finish if it's in progress (max 5 seconds to avoid hanging frontend)
    let start = std::time::Instant::now();
    while IS_SCANNING.load(Ordering::Relaxed) && start.elapsed() < std::time::Duration::from_secs(5) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    
    if let Ok(cache_lock) = cache.lock() {
        cache_lock.clone()
    } else {
        Vec::new()
    }
}

#[tauri::command]
pub fn broadcast_setting(app: AppHandle, key: String, value: serde_json::Value) {
    let _ = app.emit("settings-changed", serde_json::json!({ "key": key, "value": value }));
}

#[tauri::command]
pub fn hide_native_osd() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
        let class1 = windows::core::PCSTR(b"NativeHWNDHost\0".as_ptr());
        if let Ok(hwnd) = FindWindowA(class1, windows::core::PCSTR::null()) {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[tauri::command]
pub fn hide_volume_overlay(app: AppHandle) {
    if let Some(win) = app.get_webview_window("volume-overlay") {
        let _ = win.hide();
    }
}

#[tauri::command]
pub fn hide_brightness_overlay(app: AppHandle) {
    if let Some(win) = app.get_webview_window("brightness-overlay") {
        let _ = win.hide();
    }
}

#[tauri::command]
pub fn open_wifi_settings() {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::Win32::UI::Shell::ShellExecuteA;
        use std::ptr::null_mut;

        let _ = ShellExecuteA(
            Some(HWND(null_mut())),
            windows::core::PCSTR(b"open\0".as_ptr()),
            windows::core::PCSTR(b"ms-availablenetworks:\0".as_ptr()),
            windows::core::PCSTR::null(),
            windows::core::PCSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[tauri::command]
pub fn open_notification_center() {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::Win32::UI::Shell::ShellExecuteA;
        use std::ptr::null_mut;

        let _ = ShellExecuteA(
            Some(HWND(null_mut())),
            windows::core::PCSTR(b"open\0".as_ptr()),
            windows::core::PCSTR(b"ms-actioncenter:\0".as_ptr()),
            windows::core::PCSTR::null(),
            windows::core::PCSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[tauri::command]
pub fn media_play_pause() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(crate::types::SystemCommand::MediaPlayPause);
        }
    }
}

#[tauri::command]
pub fn media_next() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(crate::types::SystemCommand::MediaNext);
        }
    }
}

#[tauri::command]
pub fn media_previous() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(crate::types::SystemCommand::MediaPrevious);
        }
    }
}

#[tauri::command]
pub fn set_volume(volume: f32) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(crate::types::SystemCommand::SetVolume(volume));
        }
    }
}

#[tauri::command]
pub async fn quit_bloom(handle: AppHandle) {
    if let Some(w) = handle.get_webview_window("main") { let _ = unregister_appbar_native(w.hwnd().unwrap()); }
    if let Some(w) = handle.get_webview_window("dock") { let _ = unregister_appbar_native(w.hwnd().unwrap()); }
    set_taskbar_visibility(true);
    NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

    handle.exit(0);
}

#[tauri::command]
pub async fn restart_bloom(handle: AppHandle) {
    if let Some(w) = handle.get_webview_window("main") { let _ = unregister_appbar_native(w.hwnd().unwrap()); }
    if let Some(w) = handle.get_webview_window("dock") { let _ = unregister_appbar_native(w.hwnd().unwrap()); }
    set_taskbar_visibility(true);
    NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

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
