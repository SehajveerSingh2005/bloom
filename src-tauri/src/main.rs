#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, APPBARDATA, ABM_NEW, ABM_SETPOS, ABM_REMOVE, ABM_QUERYPOS,
    ABE_TOP, ABE_BOTTOM, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
    IShellFolder, SHGetDesktopFolder, SHCONTF_FOLDERS, SHCONTF_NONFOLDERS,
    SHGetKnownFolderIDList, FOLDERID_AppsFolder, ShellExecuteA,
    IShellLinkW, ShellLink, IEnumIDList, SHGetNameFromIDList, SIGDN_NORMALDISPLAY, SIGDN_FILESYSPATH,
};
use windows::Win32::Foundation::{HWND, LPARAM, HGLOBAL, CloseHandle};
use windows::core::{BOOL, Interface};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_VOLUME_MUTE, VK_VOLUME_UP, VK_VOLUME_DOWN,
};
use windows::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_C_EVENT, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, CLSCTX_INPROC_SERVER, IPersistFile, CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Storage::Streams::DataReader;
use base64::{Engine as _, engine::general_purpose};
use std::sync::mpsc::{channel, Sender};
use wmi::{COMLibrary, WMIConnection};
use serde::Deserialize;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering, AtomicI32};
use std::time::{Instant, Duration};
use std::path::Path;
use serde::Serialize;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, GetWindowTextW,
    GetWindowLongW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, GetClassNameW, DestroyIcon,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_NAME_WIN32,
};
use windows::Win32::Graphics::Imaging::{
    IWICImagingFactory, CLSID_WICImagingFactory, GUID_ContainerFormatPng,
    WICBitmapEncoderNoCache, GUID_WICPixelFormat32bppPBGRA,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightness")]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorBrightness {
    current_brightness: u8,
}

// Audio visualization event
#[derive(Clone, serde::Serialize)]
struct AudioVisualizationData {
    frequencies: Vec<f32>,
}

// Media info struct for Tauri commands
#[derive(serde::Serialize, Clone)]
pub struct MediaInfo {
    title: String,
    artist: String,
    is_playing: bool,
    has_media: bool,
    artwork: Option<Vec<String>>,
}

enum SystemCommand {
    VolumeMute,
    VolumeUp,
    VolumeDown,
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    ToggleVisibility(bool),
    BrightnessUp,
    BrightnessDown,
}

static mut COMMAND_SENDER: Option<Sender<SystemCommand>> = None;
static MAIN_APPBAR_REGISTERED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, serde::Deserialize, Debug)]
pub struct IntRect { pub x: i32, pub y: i32, pub width: i32, pub height: i32 }
static DOCK_RECT: std::sync::Mutex<Option<IntRect>> = std::sync::Mutex::new(None);
static DOCK_IS_HOVERED: AtomicBool = AtomicBool::new(false);
static MENU_IS_OPEN: AtomicBool = AtomicBool::new(false);
static MENU_RECT: std::sync::Mutex<Option<IntRect>> = std::sync::Mutex::new(None);
static ICON_CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, String>>> = OnceLock::new();

#[tauri::command]
fn set_menu_open(open: bool, rect: Option<IntRect>) {
    MENU_IS_OPEN.store(open, Ordering::Relaxed);
    if let Ok(mut r) = MENU_RECT.lock() {
        *r = rect;
    }
}

// resolve_shortcut already added earlier

fn resolve_shortcut(path: &str) -> Option<String> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;
        
        let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        persist_file.Load(windows::core::PCWSTR(wide_path.as_ptr()), windows::Win32::System::Com::STGM(0)).ok()?;
        
        // Use non-interactive flags to avoid hangs or "Searching for..." dialogs
        let _ = shell_link.Resolve(HWND(std::ptr::null_mut()), 1 | 16 | 32); 
        
        let mut buffer = [0u16; 260];
        let mut data = windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW::default();
        shell_link.GetPath(&mut buffer, &mut data, 0).ok()?;
        
        let target = String::from_utf16_lossy(&buffer);
        let trimmed = target.trim_matches(char::from(0)).to_string();
        if trimmed.trim().is_empty() { None } else { Some(trimmed) }
    }
}

#[tauri::command]
fn set_dock_hovered(hovered: bool) {
    DOCK_IS_HOVERED.store(hovered, Ordering::Relaxed);
}

#[tauri::command]
fn update_dock_rect(rect: IntRect) {
    if let Ok(mut r) = DOCK_RECT.lock() {
        *r = Some(rect);
    }
}

#[tauri::command]
fn set_window_height(window: tauri::Window, height: f64) {
    if let Ok(scale_factor) = window.scale_factor() {
        if let Ok(physical_size) = window.inner_size() {
            let logical_width = physical_size.width as f64 / scale_factor;
            let _ = window.set_size(tauri::LogicalSize::new(logical_width, height));
        }
    }
}

#[tauri::command]
fn set_ignore_cursor_events(window: tauri::Window, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}


fn set_taskbar_visibility(visible: bool) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE, SW_SHOW};
        let tray_class = windows::core::PCSTR(b"Shell_TrayWnd\0".as_ptr());
        let secondary_tray_class = windows::core::PCSTR(b"Shell_SecondaryTrayWnd\0".as_ptr());

        if let Ok(tray_hwnd) = FindWindowA(tray_class, windows::core::PCSTR::null()) {
            let _ = ShowWindow(tray_hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
        if let Ok(secondary_tray_hwnd) = FindWindowA(secondary_tray_class, windows::core::PCSTR::null()) {
            let _ = ShowWindow(secondary_tray_hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }
}

#[tauri::command]
fn toggle_dock(app: tauri::AppHandle, enable: bool) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        let hwnd = dock_win.hwnd().unwrap();
        if enable {
            set_taskbar_visibility(false);
            let _ = dock_win.show();
            let _ = dock_win.set_always_on_top(true);
        } else {
            let _ = dock_win.hide();
            unregister_appbar_native(hwnd);
            DOCK_APPBAR_REGISTERED.store(false, Ordering::Relaxed);
            set_taskbar_visibility(true);
            // Re-sync other appbars to ensure they stay in place
            if let Some(main_win) = app.get_webview_window("main") {
                register_appbar(main_win);
            }
        }
    }
}

#[tauri::command]
fn sync_appbar(window: tauri::WebviewWindow) {
    let label = window.label().to_string();
    if label == "main" {
        register_appbar(window);
    }
}

#[tauri::command]
fn change_dock_mode(app: tauri::AppHandle, mode: String) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        if mode == "fixed" {
            register_dock_appbar(dock_win.clone());
        } else if let Ok(hwnd) = dock_win.hwnd() {
            unregister_appbar_native(hwnd);
        }
        
        // Ensure always on top and native taskbar stays hidden
        let _ = dock_win.set_always_on_top(true);
        set_taskbar_visibility(false);
        
        // Sync the current overlap state immediately to the frontend
        let current = CURRENT_DOCK_OVERLAP.load(Ordering::Relaxed);
        if current != -1 {
            let _ = app.emit("dock-overlap", current == 1);
        }
    }
}

#[tauri::command]
fn open_app(app_name: String) {
    if app_name == "start" {
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, VK_LWIN, KEYEVENTF_KEYUP};
            keybd_event(VK_LWIN.0 as u8, 0, Default::default(), 0);
            keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
        return;
    }
    
    let path = app_name;
    unsafe {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        
        let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_open: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        
        ShellExecuteW(
            None,
            windows::core::PCWSTR(wide_open.as_ptr()),
            windows::core::PCWSTR(wide_path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
    pub is_running: bool,
    pub hwnd: Option<isize>,
    pub executable: Option<String>,
}

#[tauri::command]
fn get_active_windows() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut apps as *mut Vec<AppInfo> as isize));
    }
    apps
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let apps = &mut *(lparam.0 as *mut Vec<AppInfo>);

    if IsWindowVisible(hwnd).as_bool() {
        let mut text = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut text);
        if len > 0 {
            let title = String::from_utf16_lossy(&text[..len as usize]);
            
            // Filter out some common non-app windows
            let mut class_name = [0u16; 256];
            let class_len = GetClassNameW(hwnd, &mut class_name);
            let class_str = String::from_utf16_lossy(&class_name[..class_len as usize]);
            
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let style = GetWindowLongW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE) as u32;
            
            // Basic filter for top-level app windows
            if (ex_style & WS_EX_TOOLWINDOW.0) != 0 || (style & windows::Win32::UI::WindowsAndMessaging::WS_CAPTION.0) == 0 {
                return true.into();
            }

            // Filter out system containers and background stuff
            if title == "Program Manager" || title == "Bloom" || title == "Bloom Dock" {
                return true.into();
            }

            if class_str == "Windows.UI.Core.CoreWindow" || class_str == "ApplicationFrameWindow" {
                // Ignore these or handle specially if needed
            }

            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));
            
            if let Ok(process_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) {
                let mut path_buf = [0u16; 1024];
                let mut path_len = path_buf.len() as u32;
                if QueryFullProcessImageNameW(process_handle, PROCESS_NAME_WIN32, windows::core::PWSTR(path_buf.as_mut_ptr()), &mut path_len).is_ok() {
                    let path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
                    let lowercase_path = path.to_lowercase();
                    
                    // Filter out Bloom itself and some common background processes
                    if lowercase_path.contains("bloom.exe") || lowercase_path.contains("conhost.exe") || 
                       lowercase_path.contains("explorer.exe") || lowercase_path.contains("shellexperiencehost.exe") ||
                       lowercase_path.contains("searchhost.exe") || lowercase_path.contains("applicationframehost.exe") ||
                       lowercase_path.contains("textinputhost.exe") || lowercase_path.contains("systemsettings.exe") {
                        return true.into();
                    }

                    let name = Path::new(&path).file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&title)
                        .replace(".exe", "");

                    let final_name = if name == "msedge" && title.contains("Netflix") {
                        "Netflix".to_string()
                    } else {
                        name
                    };

                    // Avoid duplicates
                    if !apps.iter().any(|a| a.path == path) {
                        apps.push(AppInfo {
                            name: final_name,
                            path,
                            icon: None,
                            is_running: true,
                            hwnd: Some(hwnd.0 as isize),
                            executable: None,
                        });
                    }
                    let _ = CloseHandle(process_handle);
                }
            }
        }
    }
    true.into()
}

#[tauri::command]
fn focus_window(hwnd: isize) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE, SW_MINIMIZE, IsIconic, GetForegroundWindow};
        let hwnd = HWND(hwnd as *mut _);
        let fg = GetForegroundWindow();
        
        if IsIconic(hwnd).as_bool() {
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
    }
}

#[tauri::command]
async fn get_app_icon(path: String, hwnd: Option<isize>) -> Result<Option<String>, String> {
    let cache = ICON_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    
    // 1. Check cache first
    if let Ok(c) = cache.lock() {
        if let Some(icon) = c.get(&path) {
            return Ok(Some(icon.clone()));
        }
    }

    // 2. Try to get icon from HWND if available (Best for PWAs/Netflix)
    if let Some(h) = hwnd {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GetClassLongPtrW, GCLP_HICON, WM_GETICON, ICON_BIG, SendMessageTimeoutW, SMTO_ABORTIFHUNG};
            let h_hwnd = HWND(h as *mut _);
            
            let mut h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(GetClassLongPtrW(h_hwnd, GCLP_HICON) as *mut _);
            if h_icon.is_invalid() {
                let mut res = 0usize;
                let _ = SendMessageTimeoutW(
                    h_hwnd, 
                    WM_GETICON, 
                    windows::Win32::Foundation::WPARAM(ICON_BIG as usize), 
                    windows::Win32::Foundation::LPARAM(0), 
                    SMTO_ABORTIFHUNG, 
                    100, 
                    Some(&mut res)
                );
                if res != 0 { h_icon = windows::Win32::UI::WindowsAndMessaging::HICON(res as *mut _); }
            }
            
            if !h_icon.is_invalid() {
                if let Some(base64) = icon_to_base64(h_icon) {
                    if let Ok(mut c) = cache.lock() {
                        c.insert(path.clone(), base64.clone());
                    }
                    return Ok(Some(base64));
                }
            }
        }
    }

    // 3. Fallback to path-based extraction (run heavy shell ops in blocking thread)
    let path_clone = path.clone();
    tauri::async_runtime::spawn_blocking(move || unsafe {
        // Ensure COM is initialized for this thread
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = {
            let target_path = if path_clone.to_lowercase().ends_with(".lnk") {
                resolve_shortcut(&path_clone).unwrap_or(path_clone.clone())
            } else {
                path_clone.clone()
            };

            let mut actual_path = target_path.clone();
            
            // Special case for code.exe
            if target_path.to_lowercase().contains("code.exe") || target_path.to_lowercase().ends_with("code") {
                if let Ok(home) = std::env::var("USERPROFILE") {
                    let p1 = format!("{}\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe", home);
                    if std::path::Path::new(&p1).exists() { actual_path = p1; }
                }
            }

            let mut shfi: SHFILEINFOW = std::mem::zeroed();
            let path_u16: Vec<u16> = actual_path.encode_utf16().chain(std::iter::once(0)).collect();
            let res = SHGetFileInfoW(
                windows::core::PCWSTR(path_u16.as_ptr()),
                Default::default(),
                Some(&mut shfi),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON
            );

            if res != 0 && !shfi.hIcon.is_invalid() {
                let base64_icon = icon_to_base64(shfi.hIcon);
                let _ = DestroyIcon(shfi.hIcon);
                if let Some(ref base64) = base64_icon {
                    if let Some(c) = ICON_CACHE.get() {
                        if let Ok(mut lock) = c.lock() {
                            lock.insert(path_clone, base64.clone());
                        }
                    }
                }
                Some(base64_icon)
            } else {
                None
            }
        };
        windows::Win32::System::Com::CoUninitialize();
        Ok(result.flatten())
    }).await.map_err(|e| e.to_string())?
}

unsafe fn icon_to_base64(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<String> {
    let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let bitmap = factory.CreateBitmapFromHICON(hicon).ok()?;
    
    let stream = CreateStreamOnHGlobal(HGLOBAL(std::ptr::null_mut()), true).ok()?;
    let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
    encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
    
    let mut frame = None;
    encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()).ok()?;
    let frame = frame?;
    frame.Initialize(None).ok()?;
    
    let (mut width, mut height) = (0u32, 0u32);
    bitmap.GetSize(&mut width, &mut height).ok()?;
    frame.SetSize(width, height).ok()?;
    
    let mut format = GUID_WICPixelFormat32bppPBGRA;
    frame.SetPixelFormat(&mut format).ok()?;
    
    frame.WriteSource(&bitmap, std::ptr::null()).ok()?;
    frame.Commit().ok()?;
    encoder.Commit().ok()?;
    
    let hglobal = GetHGlobalFromStream(&stream).ok()?;
    let ptr = windows::Win32::System::Memory::GlobalLock(hglobal);
    let size = windows::Win32::System::Memory::GlobalSize(hglobal);
    
    let data = std::slice::from_raw_parts(ptr as *const u8, size);
    let base64_str = general_purpose::STANDARD.encode(data);
    
    let _ = windows::Win32::System::Memory::GlobalUnlock(hglobal);
    
    Some(format!("data:image/png;base64,{}", base64_str))
}

#[tauri::command]
fn save_pinned_apps(app: tauri::AppHandle, apps: Vec<AppInfo>) -> Result<(), String> {
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
fn load_pinned_apps(app: tauri::AppHandle) -> Vec<AppInfo> {
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
        AppInfo { name: "Terminal".into(), path: "wt.exe".into(), icon: None, is_running: false, hwnd: None, executable: Some("wt.exe".into()) },
        AppInfo { name: "VS Code".into(), path: "code".into(), icon: None, is_running: false, hwnd: None, executable: Some("code.exe".into()) },
    ]
}

use std::sync::OnceLock;
static INSTALLED_APPS_CACHE: OnceLock<std::sync::Mutex<Vec<AppInfo>>> = OnceLock::new();
static IS_SCANNING: AtomicBool = AtomicBool::new(false);


#[tauri::command]
async fn get_installed_apps() -> Vec<AppInfo> {
    let cache = INSTALLED_APPS_CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    
    // If empty and not scanning, trigger one
    let is_empty = if let Ok(lock) = cache.lock() { lock.is_empty() } else { true };
    if is_empty && !IS_SCANNING.load(Ordering::Relaxed) {
        trigger_app_scan();
    }
    
    // Wait for scanning to finish if it's in progress (max 5 seconds to avoid hanging frontend)
    let start = Instant::now();
    while IS_SCANNING.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    if let Ok(cache_lock) = cache.lock() {
        cache_lock.clone()
    } else {
        Vec::new()
    }
}

fn trigger_app_scan() {
    if IS_SCANNING.load(Ordering::Relaxed) { return; }
    IS_SCANNING.store(true, Ordering::Relaxed);
    
    std::thread::spawn(|| {
        let mut apps = Vec::new();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            // Use a scope to ensure COM objects are dropped before CoUninitialize
            {
                use windows::Win32::System::Com::CoTaskMemFree;
            use windows::Win32::UI::Shell::SIGDN_URL;

            if let Ok(pidl_apps) = SHGetKnownFolderIDList(&FOLDERID_AppsFolder, 0, None) {
                if let Ok(desktop) = SHGetDesktopFolder() {
                    if let Ok(apps_folder) = desktop.BindToObject::<_, IShellFolder>(pidl_apps, None) {
                        let mut enum_id: Option<IEnumIDList> = None;
                        let res = apps_folder.EnumObjects(HWND(std::ptr::null_mut()), (SHCONTF_FOLDERS.0 | SHCONTF_NONFOLDERS.0) as u32, &mut enum_id);
                        
                        if res.is_ok() {
                            if let Some(enum_id) = enum_id {
                                let pidl_item = std::ptr::null_mut();
                                let mut fetched = 0;
                                while enum_id.Next(&mut [pidl_item], Some(&mut fetched)).is_ok() && fetched > 0 {
                                    
                                    let name = if let Ok(n_ptr) = SHGetNameFromIDList(pidl_item, SIGDN_NORMALDISPLAY) {
                                        let s = String::from_utf16_lossy(windows::core::PCWSTR(n_ptr.0).as_wide());
                                        CoTaskMemFree(Some(n_ptr.0 as *const _));
                                        s
                                    } else { "Unknown".to_string() };

                                    let path = if let Ok(p_ptr) = SHGetNameFromIDList(pidl_item, SIGDN_FILESYSPATH) {
                                        let s = String::from_utf16_lossy(windows::core::PCWSTR(p_ptr.0).as_wide());
                                        CoTaskMemFree(Some(p_ptr.0 as *const _));
                                        s
                                    } else if let Ok(p_ptr) = SHGetNameFromIDList(pidl_item, SIGDN_URL) {
                                        let s = String::from_utf16_lossy(windows::core::PCWSTR(p_ptr.0).as_wide());
                                        CoTaskMemFree(Some(p_ptr.0 as *const _));
                                        s
                                    } else { name.clone() };

                                    if !name.to_lowercase().contains("uninstall") && !name.is_empty() && name != "Unknown" {
                                        apps.push(AppInfo {
                                            name,
                                            path,
                                            icon: None,
                                            is_running: false,
                                            hwnd: None,
                                            executable: None,
                                        });
                                    }
                                    CoTaskMemFree(Some(pidl_item as *const _));
                                }
                            }
                        }
                    }
                }
                CoTaskMemFree(Some(pidl_apps as *const _));
            }
            } // Close COM scope
            windows::Win32::System::Com::CoUninitialize();
        }

        // Fallback for classic FS apps
        let mut paths = vec![
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs".to_string(),
        ];
        if let Ok(appdata) = std::env::var("APPDATA") {
            paths.push(format!(r"{}\Microsoft\Windows\Start Menu\Programs", appdata));
            paths.push(format!(r"{}\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar", appdata));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(format!(r"{}\Microsoft\Windows\Start Menu\Programs", local));
            paths.push(format!(r"{}\Microsoft\WindowsApps", local)); 
        }
        
        // Add Desktop folders as they often contain app shortcuts
        paths.push(r"C:\Users\Public\Desktop".to_string());
        if let Ok(home) = std::env::var("USERPROFILE") {
            paths.push(format!(r"{}\Desktop", home));
        }

        for root in paths {
            if Path::new(&root).exists() {
                scan_dir(Path::new(&root), &mut apps, 0);
            }
        }

        if let Some(c) = INSTALLED_APPS_CACHE.get() {
            if let Ok(mut lock) = c.lock() {
                *lock = apps;
            }
        }
        IS_SCANNING.store(false, Ordering::Relaxed);
    });
}

fn scan_dir(path: &Path, apps: &mut Vec<AppInfo>, depth: i32) {
    if depth > 10 { return; } // Increased depth for deeply nested Start Menu folders
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, apps, depth + 1);
            } else if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if ext_str == "lnk" || ext_str == "exe" {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    if name.to_lowercase().contains("uninstall") || name.starts_with("Install") { continue; }
                    
                    let path_str = path.to_string_lossy().to_string();
                    // Avoid duplicates
                    if !apps.iter().any(|a| a.path == path_str || a.name == name) {
                        apps.push(AppInfo {
                            name,
                            path: path_str,
                            icon: None,
                            is_running: false,
                            hwnd: None,
                            executable: None,
                        });
                    }
                }
            }
        }
    }
}

#[tauri::command]
fn broadcast_setting(app: tauri::AppHandle, key: String, value: serde_json::Value) {
    let _ = app.emit("settings-changed", serde_json::json!({ "key": key, "value": value }));
}

#[tauri::command]
fn hide_native_osd() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
        let class1 = windows::core::PCSTR(b"NativeHWNDHost\0".as_ptr());
        if let Ok(hwnd) = FindWindowA(class1, windows::core::PCSTR::null()) {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[tauri::command]
fn open_settings_window(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn hide_volume_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("volume-overlay") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn hide_brightness_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("brightness-overlay") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn open_wifi_settings() {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
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
fn open_notification_center() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
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
fn media_play_pause() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(SystemCommand::MediaPlayPause);
        }
    }
}

#[tauri::command]
fn media_next() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(SystemCommand::MediaNext);
        }
    }
}

#[tauri::command]
fn media_previous() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(SystemCommand::MediaPrevious);
        }
    }
}

// WASAPI Audio capture for visualization
fn setup_audio_visualization(app_handle: AppHandle) {
    std::thread::spawn(move || {
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, IMMDevice, IAudioClient, IAudioCaptureClient,
            eRender, eConsole, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
        };
        use windows::Win32::System::Com::{
            CoInitializeEx, CoUninitialize, CoTaskMemFree,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::Foundation::S_OK;

        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let com_initialized = hr.is_ok() || hr == S_OK;

            let _result: Result<(), String> = (|| {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("CoCreateInstance failed: {:?}", e))?;

                let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                    .map_err(|e| format!("GetDefaultAudioEndpoint failed: {:?}", e))?;

                let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)
                    .map_err(|e| format!("Activate failed: {:?}", e))?;

                let format_ptr = audio_client.GetMixFormat()
                    .map_err(|e| format!("GetMixFormat failed: {:?}", e))?;
                
                let channels = (*format_ptr).nChannels as usize;
                let bits_per_sample = (*format_ptr).wBitsPerSample;
                let bytes_per_sample = (bits_per_sample / 8) as usize;
                
                if bytes_per_sample == 0 || channels == 0 || bytes_per_sample > 4 {
                    CoTaskMemFree(Some(format_ptr as *const _));
                    return Err(format!("Invalid audio format: channels={}, bits={}", channels, bits_per_sample));
                }
                
                let buffer_duration = 10_000_000i64;
                audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    buffer_duration,
                    0,
                    format_ptr,
                    Some(std::ptr::null()),
                ).map_err(|e| format!("Initialize failed: {:?}", e))?;

                CoTaskMemFree(Some(format_ptr as *const _));

                let capture_client: IAudioCaptureClient = audio_client.GetService()
                    .map_err(|e| format!("GetService failed: {:?}", e))?;

                audio_client.Start()
                    .map_err(|e| format!("Start failed: {:?}", e))?;

                const FFT_SIZE: usize = 512;
                let mut fft_buffer = vec![0.0f32; FFT_SIZE];
                let mut buffer_pos = 0;
                const NUM_BANDS: usize = 5;
                let mut max_band_energies = [0.01f32; NUM_BANDS];
                let mut prev_values = [0.1f32; NUM_BANDS];

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(32));
                    loop {
                        let packet_length = match capture_client.GetNextPacketSize() {
                            Ok(len) => len,
                            Err(_) => break,
                        };
                        if packet_length == 0 { break; }
                        let mut data_ptr: *mut u8 = std::ptr::null_mut();
                        let mut num_frames = 0u32;
                        let mut flags = 0u32;

                        if capture_client.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None).is_err() {
                            break;
                        }

                        if !data_ptr.is_null() && num_frames > 0 && bytes_per_sample > 0 && channels > 0 {
                            let stride = channels * bytes_per_sample;
                            for frame in 0..num_frames as usize {
                                let frame_offset = frame * stride;
                                let mut sample_val: i64 = 0;
                                for ch in 0..channels {
                                    let sample_offset = frame_offset + ch * bytes_per_sample;
                                    let sample: i64 = match bytes_per_sample {
                                        2 => *(data_ptr.add(sample_offset) as *const i16) as i64,
                                        4 => *(data_ptr.add(sample_offset) as *const i32) as i64,
                                        _ => 0,
                                    };
                                    sample_val = sample_val.wrapping_add(sample);
                                }
                                sample_val /= channels as i64;
                                let normalized = match bytes_per_sample {
                                    2 => (sample_val as f32) / 32768.0,
                                    4 => (sample_val as f32) / 2147483648.0,
                                    _ => 0.0,
                                };
                                if buffer_pos < FFT_SIZE {
                                    let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * buffer_pos as f32 / FFT_SIZE as f32).cos());
                                    fft_buffer[buffer_pos] = normalized * window;
                                    buffer_pos += 1;
                                }
                                if buffer_pos >= FFT_SIZE {
                                    let band_ranges = [(1, 2), (2, 6), (6, 18), (18, 60), (60, 200)];
                                    let mut output = [0.0f32; NUM_BANDS];
                                    for (band_idx, &(bin_start, bin_end)) in band_ranges.iter().enumerate() {
                                        let mut total_mag = 0.0f32;
                                        for bin in bin_start..bin_end {
                                            if bin >= FFT_SIZE / 2 { break; }
                                            let freq = 2.0 * std::f32::consts::PI * bin as f32 / FFT_SIZE as f32;
                                            let mut real = 0.0f32;
                                            let mut imag = 0.0f32;
                                            for (sample_idx, &sample) in fft_buffer.iter().enumerate() {
                                                let phase = freq * sample_idx as f32;
                                                real += sample * phase.cos();
                                                imag -= sample * phase.sin();
                                            }
                                            total_mag += (real * real + imag * imag).sqrt();
                                        }
                                        let mut avg_mag = total_mag / (bin_end - bin_start) as f32;
                                        let weighting = [1.2, 1.2, 1.5, 2.8, 5.0];
                                        avg_mag *= weighting[band_idx];
                                        if avg_mag > max_band_energies[band_idx] {
                                            max_band_energies[band_idx] = avg_mag;
                                        } else {
                                            max_band_energies[band_idx] *= 0.99;
                                        }
                                        let target = (avg_mag / max_band_energies[band_idx].max(0.12)).min(1.0).powf(0.75);
                                        let is_rising = target > prev_values[band_idx];
                                        let smooth_factor = if is_rising { 0.10 } else { 0.20 };
                                        output[band_idx] = prev_values[band_idx] * smooth_factor + target * (1.0 - smooth_factor);
                                        output[band_idx] = output[band_idx].min(1.0).max(0.18);
                                        prev_values[band_idx] = output[band_idx];
                                    }
                                    if ANY_MEDIA_PLAYING.load(Ordering::Relaxed) {
                                        let _ = app_handle.emit("audio-visualization", AudioVisualizationData { frequencies: output.to_vec() });
                                    }
                                    buffer_pos = 0;
                                }
                            }
                            let _ = capture_client.ReleaseBuffer(num_frames);
                        }
                    }
                }
            })();
            if com_initialized { CoUninitialize(); }
        }
    });
}

#[derive(Clone, serde::Serialize)]
struct BrightnessChangeEvent { brightness: u32 }
static BRIGHTNESS_SENDER: std::sync::OnceLock<std::sync::mpsc::Sender<u32>> = std::sync::OnceLock::new();
static CURRENT_BRIGHTNESS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(50);
static LAST_BRIGHTNESS_CHANGE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static ANY_MEDIA_PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn get_now_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64 }

fn handle_volume_key_event(vk_code: VIRTUAL_KEY) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let cmd = match vk_code {
                VK_VOLUME_MUTE => Some(SystemCommand::VolumeMute),
                VK_VOLUME_UP => Some(SystemCommand::VolumeUp),
                VK_VOLUME_DOWN => Some(SystemCommand::VolumeDown),
                _ => None,
            };
            if let Some(cmd) = cmd { let _ = sender.send(cmd); }
        }
    }
}

fn handle_brightness_key_event(vk_code: VIRTUAL_KEY) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let cmd = if vk_code.0 == 0x216 || vk_code.0 == 0x7A { Some(SystemCommand::BrightnessDown) }
            else if vk_code.0 == 0x217 || vk_code.0 == 0x7B { Some(SystemCommand::BrightnessUp) }
            else { None };
            if let Some(cmd) = cmd { let _ = sender.send(cmd); }
        }
    }
}

fn setup_system_worker(app_handle: AppHandle) -> Sender<SystemCommand> {
    let (tx, rx) = channel::<SystemCommand>();
    let handle_system = app_handle.clone();
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        use windows::Win32::Media::Audio::{IMMDeviceEnumerator, eRender, eConsole};
        use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator = CoCreateInstance::<_, IMMDeviceEnumerator>(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL).ok();
            let device = enumerator.as_ref().and_then(|e| e.GetDefaultAudioEndpoint(eRender, eConsole).ok());
            let audio_endpoint_volume = device.as_ref().and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok());
            let mut manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()).ok();
            let mut last_processed_media = std::time::Instant::now();
            let mut last_emitted_info: Option<(String, String, bool, bool, Option<String>)> = None;
            let mut last_volume: f32 = -1.0;
            let mut last_muted: bool = false;

            let hide_osd = || {
                use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
                let class1 = windows::core::PCSTR(b"NativeHWNDHost\0".as_ptr());
                if let Ok(hwnd1) = FindWindowA(class1, windows::core::PCSTR::null()) { let _ = ShowWindow(hwnd1, SW_HIDE); }
            };

            loop {
                if manager.is_none() { manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()).ok(); }
                while let Ok(cmd) = rx.try_recv() {
                    if let Some(ref aev) = audio_endpoint_volume {
                        match cmd {
                            SystemCommand::VolumeMute => { if let Ok(muted) = aev.GetMute() { let _ = aev.SetMute(!muted.as_bool(), std::ptr::null()); hide_osd(); } }
                            SystemCommand::VolumeUp => { if let Ok(vol) = aev.GetMasterVolumeLevelScalar() { let _ = aev.SetMasterVolumeLevelScalar((vol + 0.05).min(1.0), std::ptr::null()); hide_osd(); } }
                            SystemCommand::VolumeDown => { if let Ok(vol) = aev.GetMasterVolumeLevelScalar() { let _ = aev.SetMasterVolumeLevelScalar((vol - 0.05).max(0.0), std::ptr::null()); hide_osd(); } }
                            SystemCommand::MediaPlayPause => { if let Some(ref mgr) = manager { if let Ok(session) = mgr.GetCurrentSession() { let _ = session.TryTogglePlayPauseAsync(); } } }
                            SystemCommand::MediaNext => { if let Some(ref mgr) = manager { if let Ok(session) = mgr.GetCurrentSession() { let _ = session.TrySkipNextAsync(); } } }
                            SystemCommand::MediaPrevious => { if let Some(ref mgr) = manager { if let Ok(session) = mgr.GetCurrentSession() { let _ = session.TrySkipPreviousAsync(); } } }
                            SystemCommand::ToggleVisibility(visible) => {
                                let _ = handle_system.emit("visibility-change", visible);
                                if let Some(w) = handle_system.get_webview_window("bottom-corners") { if visible { let _ = w.show(); } else { let _ = w.hide(); } }
                            }
                            SystemCommand::BrightnessUp => {
                                let new_val = (CURRENT_BRIGHTNESS.load(Ordering::Relaxed) + 10).min(100);
                                CURRENT_BRIGHTNESS.store(new_val, Ordering::Relaxed);
                                LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), Ordering::Relaxed);
                                let _ = handle_system.emit("brightness-change", BrightnessChangeEvent { brightness: new_val });
                                if let Some(tx) = BRIGHTNESS_SENDER.get() { let _ = tx.send(new_val); }
                                hide_osd();
                            }
                            SystemCommand::BrightnessDown => {
                                let current = CURRENT_BRIGHTNESS.load(Ordering::Relaxed);
                                let new_val = if current > 10 { current - 10 } else { 0 };
                                CURRENT_BRIGHTNESS.store(new_val, Ordering::Relaxed);
                                LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), Ordering::Relaxed);
                                let _ = handle_system.emit("brightness-change", BrightnessChangeEvent { brightness: new_val });
                                if let Some(tx) = BRIGHTNESS_SENDER.get() { let _ = tx.send(new_val); }
                                hide_osd();
                            }
                        }
                    }
                }
                if let Some(ref aev) = audio_endpoint_volume {
                    if let (Ok(vol), Ok(muted)) = (aev.GetMasterVolumeLevelScalar(), aev.GetMute()) {
                        let is_muted: bool = muted.into();
                        if (vol - last_volume).abs() > 0.001 || is_muted != last_muted {
                            last_volume = vol; last_muted = is_muted;
                        let _ = handle_system.emit("volume-change", VolumeChangeEvent { volume: vol, is_muted });
                            hide_osd();
                        }
                    }
                }
                if last_processed_media.elapsed().as_millis() >= 2000 {
                    last_processed_media = std::time::Instant::now();
                    let mut best_info: Option<MediaInfo> = None;
                    if let Some(ref mgr) = manager {
                        if let Ok(sessions) = mgr.GetSessions() {
                            for i in 0..sessions.Size().unwrap_or(0) {
                                if let Ok(session) = sessions.GetAt(i) {
                                    let is_playing = session.GetPlaybackInfo().ok().and_then(|p| p.PlaybackStatus().ok()) == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
                                    if let Ok(props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
                                        let title = props.Title().unwrap_or_default().to_string();
                                        if !title.is_empty() {
                                            let artwork = (|| -> Option<Vec<String>> {
                                                let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
                                                let reader = DataReader::CreateDataReader(&stream).ok()?;
                                                reader.LoadAsync(stream.Size().ok()? as u32).ok()?.get().ok()?;
                                                let mut bytes = vec![0u8; stream.Size().ok()? as usize];
                                                reader.ReadBytes(&mut bytes).ok()?;
                                                Some(vec![format!("data:{};base64,{}", stream.ContentType().ok()?.to_string(), general_purpose::STANDARD.encode(bytes))])
                                            })();
                                            let info = MediaInfo { title, artist: props.Artist().unwrap_or_default().to_string(), is_playing, has_media: true, artwork };
                                            if is_playing { best_info = Some(info); break; } else if best_info.is_none() { best_info = Some(info); }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let current = best_info.unwrap_or(MediaInfo { title: "".into(), artist: "".into(), is_playing: false, has_media: false, artwork: None });
                    let art_str = current.artwork.as_ref().and_then(|a| a.first()).cloned();
                    if last_emitted_info.as_ref().map_or(true, |(t, a, p, h, art)| t != &current.title || a != &current.artist || p != &current.is_playing || h != &current.has_media || art != &art_str) {
                        let _ = handle_system.emit("media-update", current.clone());
                        ANY_MEDIA_PLAYING.store(current.is_playing, Ordering::Relaxed);
                        last_emitted_info = Some((current.title, current.artist, current.is_playing, current.has_media, art_str));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(64));
            }
        }
    });
    let handle_brightness = app_handle.clone();
    std::thread::spawn(move || {
        let com_lib = match COMLibrary::new() { Ok(lib) => lib, Err(_) => return };
        let wmi_con = match WMIConnection::with_namespace_path("root\\WMI", com_lib) { Ok(con) => con, Err(_) => return };
        let mut last_brightness = match wmi_con.query::<WmiMonitorBrightness>() { Ok(res) => res.first().map(|b| b.current_brightness as u32).unwrap_or(50), Err(_) => 50 };
        CURRENT_BRIGHTNESS.store(last_brightness, Ordering::Relaxed);
        loop {
            if get_now_ms() - LAST_BRIGHTNESS_CHANGE.load(Ordering::Relaxed) < 2000 { std::thread::sleep(std::time::Duration::from_millis(500)); continue; }
            if let Ok(results) = wmi_con.query::<WmiMonitorBrightness>() {
                if let Some(b) = results.first() {
                    let brightness = b.current_brightness as u32;
                    if brightness != last_brightness { last_brightness = brightness; CURRENT_BRIGHTNESS.store(brightness, Ordering::Relaxed); let _ = handle_brightness.emit("brightness-change", BrightnessChangeEvent { brightness }); }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    });
    let tx_clone = tx.clone();
    let handle_visibility = app_handle.clone();
    std::thread::spawn(move || {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, IsZoomed};
        use windows::Win32::Foundation::RECT;
        let mut last_visible = true;
        let mut last_dock_overlap: Option<bool> = None;
        let mut last_emit = Instant::now();
        loop {
            unsafe {
                use windows::Win32::Graphics::Gdi::{MonitorFromWindow, GetMonitorInfoA, MONITORINFO, MONITOR_DEFAULTTONEAREST};
                let hwnd = GetForegroundWindow();
                if !hwnd.is_invalid() {
                    let mut class_name = [0u8; 256];
                    let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
                    let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");
                    
                    let mut rect = RECT::default();
                    let is_bloom = class_str.contains("Bloom") || class_str.contains("bloom"); 
                    let is_desktop = class_str == "Progman" || class_str == "WorkerW";
                    
                    let mut is_valid_window = false;
                    if !is_desktop && !is_bloom {
                        let dwm_res = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32);
                        if dwm_res.is_ok() {
                            is_valid_window = true;
                        } else if GetWindowRect(hwnd, &mut rect).is_ok() {
                            is_valid_window = true;
                        }
                    }

                    if is_valid_window {
                        let h_monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                        let mut mi = MONITORINFO::default();
                        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                        
                        if GetMonitorInfoA(h_monitor, &mut mi).as_bool() {
                            let screen_rect = mi.rcMonitor;
                            let is_fs = rect.left <= screen_rect.left && rect.top <= screen_rect.top && 
                                        rect.right >= screen_rect.right && rect.bottom >= screen_rect.bottom;
                            
                            let is_maximized = IsZoomed(hwnd).as_bool();
                            
                            // Tight threshold: only hide if truly overlapping the reservation area (56px logical)
                            // Use a slight offset to be less aggressive with shadows/borders
                            let scale = if let Some(monitor) = handle_visibility.primary_monitor().ok().flatten() { monitor.scale_factor() } else { 1.0 };
                            let dock_reserve_physical = (50.0 * scale) as i32; // Reduced from 56 to 50 for better feel
                            let overlaps_dock = rect.bottom > (screen_rect.bottom - dock_reserve_physical);
                            
                            let should_overlap = is_fs || is_maximized || overlaps_dock;

                            CURRENT_DOCK_OVERLAP.store(if should_overlap { 1 } else { 0 }, Ordering::Relaxed);

                            if Some(should_overlap) != last_dock_overlap || last_emit.elapsed() >= Duration::from_secs(3) {
                                let _ = handle_visibility.emit("dock-overlap", should_overlap);
                                last_dock_overlap = Some(should_overlap);
                                last_emit = Instant::now();
                            }

                            if is_fs && last_visible { let _ = tx_clone.send(SystemCommand::ToggleVisibility(false)); last_visible = false; }
                            else if !is_fs && !last_visible { let _ = tx_clone.send(SystemCommand::ToggleVisibility(true)); last_visible = true; }
                        }
                    } else if is_desktop || is_bloom {
                        CURRENT_DOCK_OVERLAP.store(0, Ordering::Relaxed);
                        if !last_visible { let _ = tx_clone.send(SystemCommand::ToggleVisibility(true)); last_visible = true; }
                        if last_dock_overlap != Some(false) || last_emit.elapsed() >= Duration::from_secs(3) { 
                            let _ = handle_visibility.emit("dock-overlap", false); 
                            last_dock_overlap = Some(false);
                            last_emit = Instant::now();
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(400)); // Lower frequency for visibility checks
        }
    });
    tx
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: windows::Win32::Foundation::WPARAM, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN};
    if code >= 0 {
        let vk_code = VIRTUAL_KEY((*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode as u16);
        if vk_code == VK_VOLUME_MUTE || vk_code == VK_VOLUME_UP || vk_code == VK_VOLUME_DOWN {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize { handle_volume_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
        if vk_code.0 == 0x216 || vk_code.0 == 0x217 || vk_code.0 == 0x7A || vk_code.0 == 0x7B {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize { handle_brightness_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
    }
    windows::Win32::UI::WindowsAndMessaging::CallNextHookEx(None, code, wparam, lparam)
}

fn setup_keyboard_hook() -> windows::Win32::UI::WindowsAndMessaging::HHOOK {
    unsafe { windows::Win32::UI::WindowsAndMessaging::SetWindowsHookExA(windows::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0).expect("Failed") }
}

#[derive(Clone, serde::Serialize)]
struct VolumeChangeEvent { volume: f32, is_muted: bool }

fn setup_brightness_worker() {
    let (tx, rx) = channel::<u32>();
    let _ = BRIGHTNESS_SENDER.set(tx);
    std::thread::spawn(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let child = Command::new("powershell").args(&["-NoProfile", "-NoLogo", "-Command", "-"]).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
        if let Some(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(b"$m = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods\n");
                let _ = stdin.flush();
                while let Ok(brightness) = rx.recv() {
                    let _ = stdin.write_all(format!("$m | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Brightness={}; Timeout=0}}\n", brightness).as_bytes());
                    let _ = stdin.flush();
                }
            }
            let _ = child.kill();
        }
    });
}

unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> BOOL {
    if ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT || ctrl_type == CTRL_CLOSE_EVENT {
        set_taskbar_visibility(true);
    }
    BOOL(0)
}

#[tauri::command]
fn close_window(hwnd: isize) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_CLOSE, windows::Win32::Foundation::WPARAM(0), windows::Win32::Foundation::LPARAM(0));
    }
}

fn main() {
    unsafe { let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true); }
    setup_brightness_worker();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![])))
        .invoke_handler(tauri::generate_handler![
            broadcast_setting, hide_native_osd, open_settings_window, open_wifi_settings,
            open_notification_center, set_ignore_cursor_events, set_window_height, hide_volume_overlay,
            hide_brightness_overlay, media_play_pause, media_next, media_previous, toggle_dock, change_dock_mode,
            sync_appbar, open_app, update_dock_rect, set_dock_hovered,
            get_active_windows, get_app_icon, get_installed_apps,
            save_pinned_apps, load_pinned_apps, set_menu_open, focus_window, close_window
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            register_appbar(window.clone());
            if let Some(br_win) = app.get_webview_window("brightness-overlay") {
                if let Ok(Some(monitor)) = br_win.primary_monitor() {
                    let size = monitor.size();
                    let pos = monitor.position();
                    let scale = monitor.scale_factor();
                    let pw = (200.0 * scale) as i32;
                    let ph = (400.0 * scale) as i32;
                    let _ = br_win.set_position(tauri::PhysicalPosition::new(pos.x + (size.width as i32 - pw), pos.y + (size.height as i32 / 2) - (ph / 2)));
                }
            }
            setup_cursor_monitor(app.handle().clone());
            trigger_app_scan();
            let tx = setup_system_worker(app.handle().clone());
            unsafe { COMMAND_SENDER = Some(tx.clone()); }
            let _hook = setup_keyboard_hook();
            setup_audio_visualization(app.handle().clone());
            if let Some(settings_win) = app.get_webview_window("settings") {
                #[cfg(target_os = "windows")] { let _ = window_vibrancy::apply_mica(&settings_win, None); }
                let win_clone = settings_win.clone();
                settings_win.on_window_event(move |event| { if let tauri::WindowEvent::CloseRequested { api, .. } = event { api.prevent_close(); let _ = win_clone.hide(); } });
            }
            {
                use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};
                use tauri::menu::{Menu, MenuItem};
                let quit_item = MenuItem::with_id(app, "quit", "Quit Bloom", true, None::<&str>)?;
                let restart_item = MenuItem::with_id(app, "restart", "Restart Bloom", true, None::<&str>)?;
                let settings_item = MenuItem::with_id(app, "settings", "Open Settings", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&settings_item, &restart_item, &quit_item])?;
                let ah = app.handle().clone();
                TrayIconBuilder::new().icon(app.default_window_icon().unwrap().clone()).tooltip("Bloom").menu(&menu)
                    .on_menu_event(move |_, event| {
                        match event.id().as_ref() {
                            "quit" => { if let Some(w) = ah.get_webview_window("main") { unregister_appbar_native(w.hwnd().unwrap()); } if let Some(w) = ah.get_webview_window("dock") { unregister_appbar_native(w.hwnd().unwrap()); } set_taskbar_visibility(true); ah.exit(0); }
                            "restart" => { if let Some(w) = ah.get_webview_window("main") { unregister_appbar_native(w.hwnd().unwrap()); } if let Some(w) = ah.get_webview_window("dock") { unregister_appbar_native(w.hwnd().unwrap()); } set_taskbar_visibility(true); ah.restart(); }
                            "settings" => { if let Some(w) = ah.get_webview_window("settings") { let _ = w.show(); let _ = w.set_focus(); } }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| { if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event { if let Some(w) = tray.app_handle().get_webview_window("settings") { let _ = w.show(); let _ = w.set_focus(); } } })
                    .build(app)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|_, event| { if let tauri::RunEvent::Exit = event { set_taskbar_visibility(true); } });
}

fn register_appbar(window: tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let m_size = monitor.size();
        let m_pos = monitor.position();
        let hwnd = window.hwnd().unwrap();
        let scale = window.scale_factor().unwrap_or(1.0);
        let ph = (300.0 * scale) as i32; // Window is tall enough for calendar
        let pr = (40.0 * scale) as i32;  // But only reserve 40px of screen space
        
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE as WS_EX_NA, SWP_FRAMECHANGED};
            use windows::Win32::Foundation::RECT;
            
            // Set styles first
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
            ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);
            
            // Set position before registering
            let _ = SetWindowPos(hwnd, None, m_pos.x, m_pos.y, m_size.width as i32, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);

            let mut abd = APPBARDATA::default();
            abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
            abd.hWnd = hwnd;
            
            // ONLY ABM_NEW if not already registered
            if !MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                SHAppBarMessage(ABM_NEW, &mut abd);
                MAIN_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
            }

            abd.uEdge = ABE_TOP;
            abd.rc = RECT { 
                left: m_pos.x, 
                top: m_pos.y, 
                right: m_pos.x + m_size.width as i32, 
                bottom: m_pos.y + pr
            };
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            SHAppBarMessage(ABM_SETPOS, &mut abd);
            
            let _ = window.show();
        }
    }
}

static DOCK_APPBAR_REGISTERED: AtomicBool = AtomicBool::new(false);
static CURRENT_DOCK_OVERLAP: AtomicI32 = AtomicI32::new(-1);

fn register_dock_appbar(window: tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let m_size = monitor.size();
        let m_pos = monitor.position();
        let hwnd = window.hwnd().unwrap();
        let scale = window.scale_factor().unwrap_or(1.0);
        let ph = (600.0 * scale) as i32;
        let pr = (56.0 * scale) as i32;
        
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE as WS_EX_NA, SWP_FRAMECHANGED, GWL_STYLE, WS_POPUP, WS_VISIBLE, WS_CAPTION};
            use windows::Win32::Foundation::RECT;
            
            // Force borderless popup style
            let mut style = GetWindowLongPtrW(hwnd, GWL_STYLE) as usize;
            style &= !WS_CAPTION.0 as usize;
            style |= WS_POPUP.0 as usize | WS_VISIBLE.0 as usize;
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);

            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
            ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);

            // Set position before showing
            let _ = SetWindowPos(hwnd, None, m_pos.x, m_pos.y + m_size.height as i32 - ph, m_size.width as i32, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            let _ = window.show();

            let mut abd = APPBARDATA::default();
            abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
            abd.hWnd = hwnd;
            
            // ONLY ABM_NEW if not already registered
            if !DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                SHAppBarMessage(ABM_NEW, &mut abd);
                DOCK_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
            }

            abd.uEdge = ABE_BOTTOM;
            abd.rc = RECT { 
                left: m_pos.x, 
                top: m_pos.y + m_size.height as i32 - pr, 
                right: m_pos.x + m_size.width as i32, 
                bottom: m_pos.y + m_size.height as i32 
            };
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            SHAppBarMessage(ABM_SETPOS, &mut abd);
        }
    }
}

fn unregister_appbar_native(hwnd: HWND) {
    unsafe {
        let mut abd = APPBARDATA::default();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        abd.hWnd = hwnd;
        SHAppBarMessage(ABM_REMOVE, &mut abd);
    }
}

fn setup_cursor_monitor(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        use windows::Win32::Foundation::POINT;
        
        let mut last_main_ignore = None;
        let mut last_dock_ignore = None;
        let mut dock_interaction_expiry = Instant::now();
        let mut topbar_interaction_expiry = Instant::now();
        let mut last_handle_fetch = Instant::now() - Duration::from_secs(10);
        let mut main_win_handle = None;
        let mut dock_win_handle = None;
        
        loop {
            let now = Instant::now();
            let mut pt = POINT::default();
            unsafe {
                if GetCursorPos(&mut pt).is_ok() {
                    // Periodic handle refresh to avoid IPC overhead and stale handles
                    if now.duration_since(last_handle_fetch).as_secs() >= 1 {
                        main_win_handle = app_handle.get_webview_window("main");
                        dock_win_handle = app_handle.get_webview_window("dock");
                        last_handle_fetch = now;
                    }

                    // --- Dock Interaction ---
                    if let Some(ref dock_win) = dock_win_handle {
                        if dock_win.is_visible().unwrap_or(false) {
                            let mut is_interactive = false;
                            
                            if let (Ok(win_pos), Ok(win_size)) = (dock_win.outer_position(), dock_win.outer_size()) {
                                let in_window = pt.x >= win_pos.x && pt.x <= (win_pos.x + win_size.width as i32) &&
                                                pt.y >= win_pos.y && pt.y <= (win_pos.y + win_size.height as i32);
                                
                                if in_window {
                                    // 1. Check if over actual dock items (DOCK_RECT)
                                    if let Ok(region) = DOCK_RECT.try_lock() {
                                        if let Some(r) = *region {
                                            let scale = dock_win.scale_factor().unwrap_or(1.0);
                                            let rx = win_pos.x + (r.x as f64 * scale) as i32 - 10;
                                            let ry = win_pos.y + (r.y as f64 * scale) as i32 - 10;
                                            let rw = (r.width as f64 * scale) as i32 + 20;
                                            let rh = (r.height as f64 * scale) as i32 + 20;
                                            if pt.x >= rx && pt.x <= (rx + rw) && pt.y >= ry && pt.y <= (ry + rh) {
                                                is_interactive = true;
                                            }
                                        }
                                    }

                                    // 2. Check if over an open menu
                                    if !is_interactive && MENU_IS_OPEN.load(Ordering::Relaxed) {
                                        if let Ok(rect) = MENU_RECT.try_lock() {
                                            if let Some(r) = *rect {
                                                let scale = dock_win.scale_factor().unwrap_or(1.0);
                                                let rx = (r.x as f64 * scale) as i32 - 5;
                                                let ry = (r.y as f64 * scale) as i32 - 5;
                                                let rw = (r.width as f64 * scale) as i32 + 10;
                                                let rh = (r.height as f64 * scale) as i32 + 10;
                                                if pt.x >= rx && pt.x <= (rx + rw) && pt.y >= ry && pt.y <= (ry + rh) {
                                                    is_interactive = true;
                                                }
                                            }
                                        }
                                    }

                                    // 3. Hot-edge trigger
                                    if !is_interactive {
                                        let monitor_bottom = win_pos.y + win_size.height as i32;
                                        if pt.y >= (monitor_bottom - 40) {
                                            is_interactive = true;
                                        }
                                    }
                                }
                            }
                            
                            if is_interactive {
                                dock_interaction_expiry = now + Duration::from_millis(150);
                            }
                            
                            let should_ignore = now > dock_interaction_expiry && !MENU_IS_OPEN.load(Ordering::Relaxed);
                            if Some(should_ignore) != last_dock_ignore {
                                let _ = dock_win.set_ignore_cursor_events(should_ignore);
                                last_dock_ignore = Some(should_ignore);
                            }
                        }
                    }

                    // --- TopBar Interaction ---
                    if let Some(ref main_win) = main_win_handle {
                        if main_win.is_visible().unwrap_or(false) {
                            if let (Ok(win_pos), Ok(win_size)) = (main_win.outer_position(), main_win.outer_size()) {
                                let in_bar = pt.x >= win_pos.x && pt.x <= (win_pos.x + win_size.width as i32) &&
                                             pt.y >= win_pos.y && pt.y <= (win_pos.y + win_size.height as i32);
                                
                                // Optimization: If mouse is very far from top, we can likely ignore
                                // But if it's currently interactive (e.g. calendar is open), keep it until mouse leaves
                                if in_bar {
                                    topbar_interaction_expiry = now + Duration::from_millis(200);
                                }
                                
                                let should_ignore = now > topbar_interaction_expiry;
                                
                                if Some(should_ignore) != last_main_ignore {
                                    let _ = main_win.set_ignore_cursor_events(should_ignore);
                                    last_main_ignore = Some(should_ignore);
                                }
                            }
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(64)); 
        }
    });
}
