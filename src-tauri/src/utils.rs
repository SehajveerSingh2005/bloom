use base64::{engine::general_purpose, Engine as _};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::Manager;
use windows::core::Interface;
use windows::Win32::Foundation::{HGLOBAL, HWND};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppPBGRA,
    IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{
    CoCreateInstance, IPersistFile, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::HICON;

pub fn resolve_shortcut(path: &str) -> Option<(String, String)> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;

        let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        persist_file
            .Load(
                windows::core::PCWSTR(wide_path.as_ptr()),
                windows::Win32::System::Com::STGM(0),
            )
            .ok()?;

        let _ = shell_link.Resolve(HWND(std::ptr::null_mut()), 1 | 16 | 32);

        let mut buffer = [0u16; 260];
        let mut data = windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW::default();
        shell_link.GetPath(&mut buffer, &mut data, 0).ok()?;

        let mut arg_buffer = [0u16; 1024];
        let _ = shell_link.GetArguments(&mut arg_buffer);

        let target = String::from_utf16_lossy(&buffer)
            .trim_matches(char::from(0))
            .to_string();
        let args = String::from_utf16_lossy(&arg_buffer)
            .trim_matches(char::from(0))
            .to_string();

        if target.trim().is_empty() {
            None
        } else {
            Some((target, args))
        }
    }
}

pub static ORIGINAL_TRAY_RECT: std::sync::Mutex<Option<windows::Win32::Foundation::RECT>> =
    std::sync::Mutex::new(None);
static ORIGINAL_SEC_TRAY_RECTS: std::sync::Mutex<
    Option<std::collections::HashMap<isize, windows::Win32::Foundation::RECT>>,
> = std::sync::Mutex::new(None);
static ORIGINAL_TASKBAR_STATE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
static TASKBAR_OWNER_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static TASKBAR_OWNER_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

static TASKBAR_MARKER: OnceLock<PathBuf> = OnceLock::new();
static TASKBAR_WATCHDOG_STARTED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

/// Point the crash-recovery marker at this user's app config dir (called from setup).
pub fn init_taskbar_marker(app: &tauri::AppHandle) {
    if let Ok(dir) = app.path().app_config_dir() {
        let _ = TASKBAR_MARKER.set(dir.join("taskbar_hidden.flag"));
    }
}

/// A separate Bloom process restores Explorer's taskbar when this process is
/// terminated without running Tauri's exit handlers.
pub fn start_taskbar_watchdog() -> std::io::Result<()> {
    let mut started = TASKBAR_WATCHDOG_STARTED
        .lock()
        .map_err(|_| std::io::Error::other("taskbar watchdog lock poisoned"))?;
    if *started {
        return Ok(());
    }
    let marker = TASKBAR_MARKER.get().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "taskbar marker path unavailable",
        )
    })?;
    if let Some(dir) = marker.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::process::Command::new(std::env::current_exe()?)
        .arg("--taskbar-watchdog")
        .arg(std::process::id().to_string())
        .arg(marker)
        .spawn()?;
    *started = true;
    Ok(())
}

pub fn run_taskbar_watchdog(parent_pid: u32, marker: PathBuf) {
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
    };
    // Only treat a missing PID as an already-exited parent. Access denied or
    // another wait failure must not restore the taskbar over a live Bloom.
    unsafe {
        match OpenProcess(PROCESS_SYNCHRONIZE, false, parent_pid) {
            Ok(parent) => {
                let result = WaitForSingleObject(parent, INFINITE);
                let _ = CloseHandle(parent);
                if result != WAIT_OBJECT_0 {
                    return;
                }
            }
            Err(_) if GetLastError() == ERROR_INVALID_PARAMETER => {}
            Err(_) => {
                return;
            }
        }
    }
    let _ = TASKBAR_MARKER.set(marker.clone());
    // Explorer can be restarting at the same time. Retry briefly, but never
    // act on a marker owned by a replacement Bloom process.
    for _ in 0..120 {
        if std::fs::read_to_string(&marker)
            .ok()
            .and_then(|contents| parse_marker_pid(&contents))
            != Some(parent_pid)
        {
            return;
        }
        restore_taskbar_after_crash();
        if !marker.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// True if a previous Bloom session died without restoring the native taskbar.
pub fn taskbar_marker_exists() -> bool {
    TASKBAR_MARKER.get().is_some_and(|p| p.exists())
}

/// The marker records the user's own taskbar state (`ABM_GETSTATE`) from
/// before Bloom forced auto-hide, so a crash can't lose it. Markers written by
/// older versions only contain "1" and carry no state.
fn marker_contents(
    original_state: i32,
    pid: u32,
    primary: Option<windows::Win32::Foundation::RECT>,
    secondaries: &std::collections::HashMap<isize, windows::Win32::Foundation::RECT>,
) -> String {
    let mut contents = format!("abstate={original_state}\npid={pid}\n");
    if let Some(rect) = primary {
        contents.push_str(&format!(
            "primary={},{},{},{}\n",
            rect.left, rect.top, rect.right, rect.bottom
        ));
    }
    // One line per secondary taskbar, keyed by its window, for every monitor.
    let mut secondaries: Vec<_> = secondaries.iter().collect();
    secondaries.sort_by_key(|(hwnd, _)| **hwnd);
    for (hwnd, rect) in secondaries {
        contents.push_str(&format!(
            "secondary={hwnd},{},{},{},{}\n",
            rect.left, rect.top, rect.right, rect.bottom
        ));
    }
    contents
}

fn parse_marker_state(contents: &str) -> Option<i32> {
    contents
        .lines()
        .next()?
        .strip_prefix("abstate=")?
        .parse()
        .ok()
}

fn parse_marker_pid(contents: &str) -> Option<u32> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix("pid=")?.parse().ok())
}

fn parse_rect<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<windows::Win32::Foundation::RECT> {
    let mut next = || parts.next()?.parse::<i32>().ok();
    let rect = windows::Win32::Foundation::RECT {
        left: next()?,
        top: next()?,
        right: next()?,
        bottom: next()?,
    };
    parts.next().is_none().then_some(rect)
}

fn parse_marker_rect(contents: &str, key: &str) -> Option<windows::Win32::Foundation::RECT> {
    parse_rect(
        contents
            .lines()
            .find_map(|line| line.strip_prefix(key))?
            .split(','),
    )
}

fn parse_marker_secondaries(
    contents: &str,
) -> std::collections::HashMap<isize, windows::Win32::Foundation::RECT> {
    contents
        .lines()
        .filter_map(|line| {
            let mut parts = line.strip_prefix("secondary=")?.split(',');
            let hwnd = parts.next()?.parse::<isize>().ok()?;
            Some((hwnd, parse_rect(parts)?))
        })
        .collect()
}

/// Restores the native taskbar after a previous session died with it hidden.
/// The state saved in the marker becomes the original to restore; without it,
/// the current state would be captured instead, and that is Bloom's own
/// auto-hide.
pub fn restore_taskbar_after_crash() {
    let saved = TASKBAR_MARKER
        .get()
        .and_then(|p| std::fs::read_to_string(p).ok());
    if let Some(contents) = saved {
        if let Some(state) = parse_marker_state(&contents) {
            ORIGINAL_TASKBAR_STATE.store(state, std::sync::atomic::Ordering::Relaxed);
        }
        // An unplaced rectangle would park the taskbar in the screen's corner.
        if let Some(rect) = parse_marker_rect(&contents, "primary=").filter(is_placed_rect) {
            if let Ok(mut guard) = ORIGINAL_TRAY_RECT.lock() {
                *guard = Some(rect);
            }
        }
        let mut secondaries = parse_marker_secondaries(&contents);
        secondaries.retain(|_, rect| is_placed_rect(rect));
        if !secondaries.is_empty() {
            if let Ok(mut guard) = ORIGINAL_SEC_TRAY_RECTS.lock() {
                guard
                    .get_or_insert_with(std::collections::HashMap::new)
                    .extend(secondaries);
            }
        }
    }
    set_taskbar_visibility(true, true);
}

pub fn secondary_taskbar_windows() -> Vec<windows::Win32::Foundation::HWND> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameA};

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let windows = &mut *(lparam.0 as *mut Vec<HWND>);
        let mut class = [0u8; 64];
        let length = GetClassNameA(hwnd, &mut class);
        if &class[..length as usize] == b"Shell_SecondaryTrayWnd" {
            windows.push(hwnd);
        }
        BOOL(1)
    }

    let mut windows = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(collect),
            LPARAM(&mut windows as *mut Vec<HWND> as isize),
        );
    }
    windows
}

pub fn any_native_taskbar_visible() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, IsWindowVisible};
    unsafe {
        let primary = windows::core::PCSTR(c"Shell_TrayWnd".as_ptr() as *const u8);
        FindWindowA(primary, windows::core::PCSTR::null())
            .is_ok_and(|hwnd| IsWindowVisible(hwnd).as_bool())
            || secondary_taskbar_windows()
                .into_iter()
                .any(|hwnd| IsWindowVisible(hwnd).as_bool())
    }
}

fn reset_taskbar_rects_after_shell_restart() {
    if let Ok(mut guard) = ORIGINAL_TRAY_RECT.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = ORIGINAL_SEC_TRAY_RECTS.lock() {
        *guard = None;
    }
}

/// Explorer can recreate the tray windows while Bloom remains active. A new
/// owner or primary HWND invalidates the saved tray rectangles; a DPI broadcast
/// with the same HWND must not discard them while the taskbar is parked offscreen.
pub fn refresh_taskbar_owner() -> bool {
    use std::sync::atomic::Ordering;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, GetWindowThreadProcessId};
    unsafe {
        let class = windows::core::PCSTR(c"Shell_TrayWnd".as_ptr() as *const u8);
        let Ok(hwnd) = FindWindowA(class, windows::core::PCSTR::null()) else {
            return false;
        };
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let previous = TASKBAR_OWNER_PID.swap(pid, Ordering::Relaxed);
        let previous_hwnd = TASKBAR_OWNER_HWND.swap(hwnd.0 as isize, Ordering::Relaxed);
        if (previous != 0 && previous != pid)
            || (previous_hwnd != 0 && previous_hwnd != hwnd.0 as isize)
        {
            reset_taskbar_rects_after_shell_restart();
            return true;
        }
        false
    }
}

/// A taskbar window's rectangle, if Explorer has placed it on screen. Right
/// after Explorer restarts, a new taskbar can still report an empty rectangle;
/// saving that would restore the taskbar to the screen's corner later, so it
/// is left unsaved and restoring asks Explorer for the position instead.
fn placed_taskbar_rect(
    hwnd: windows::Win32::Foundation::HWND,
) -> Option<windows::Win32::Foundation::RECT> {
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect) }.ok()?;
    is_placed_rect(&rect).then_some(rect)
}

fn is_placed_rect(rect: &windows::Win32::Foundation::RECT) -> bool {
    rect.right > rect.left && rect.bottom > rect.top && rect.left > -9000 && rect.top > -9000
}

pub fn set_taskbar_visibility(visible: bool, always_on_top: bool) -> bool {
    // Delayed AppBar and tray callbacks cannot hide Windows' taskbar after the
    // dock has been disabled or while its tray is in use.
    if !visible
        && (!dock_enabled()
            || crate::state::TRAY_INTERACTION_ACTIVE.load(std::sync::atomic::Ordering::Relaxed))
    {
        return false;
    }
    refresh_taskbar_owner();
    unsafe {
        use windows::Win32::UI::Shell::{
            SHAppBarMessage, ABM_GETSTATE, ABM_GETTASKBARPOS, ABM_SETSTATE, APPBARDATA,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowA, GetWindowRect, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOW,
        };

        // Startup retries and display/AppBar handlers can still try to hide the
        // taskbar after shutdown restored it, leaving the user without one.
        if !visible && crate::state::SHUTTING_DOWN.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }

        let tray_class = windows::core::PCSTR(c"Shell_TrayWnd".as_ptr() as *const u8);
        let Ok(tray_hwnd) = FindWindowA(tray_class, windows::core::PCSTR::null()) else {
            return false;
        };
        // Every secondary taskbar, including third and later monitors.
        let secondary_trays = secondary_taskbar_windows();

        // Legacy crash markers did not save the original window rectangle.
        // Explorer's AppBar position can still recover it after Bloom moved the
        // taskbar window off-screen.
        if visible {
            if let Ok(mut guard) = ORIGINAL_TRAY_RECT.lock() {
                if guard.is_none() {
                    let mut position = APPBARDATA {
                        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                        hWnd: tray_hwnd,
                        ..Default::default()
                    };
                    if SHAppBarMessage(ABM_GETTASKBARPOS, &mut position) != 0
                        && is_placed_rect(&position.rc)
                    {
                        *guard = Some(position.rc);
                    }
                }
            }
        }

        if !visible {
            if let Ok(mut guard) = ORIGINAL_TRAY_RECT.lock() {
                if guard.is_none() {
                    *guard = placed_taskbar_rect(tray_hwnd);
                }
            }
            if let Ok(mut guard) = ORIGINAL_SEC_TRAY_RECTS.lock() {
                let rects = guard.get_or_insert_with(std::collections::HashMap::new);
                for hwnd in &secondary_trays {
                    if let Some(rect) = placed_taskbar_rect(*hwnd) {
                        rects.entry(hwnd.0 as isize).or_insert(rect);
                    }
                }
            }
        }

        // Save original taskbar state before modifying
        if ORIGINAL_TASKBAR_STATE.load(std::sync::atomic::Ordering::Relaxed) == -1 {
            let mut get_abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: tray_hwnd,
                ..Default::default()
            };
            let original_state = SHAppBarMessage(ABM_GETSTATE, &mut get_abd);
            ORIGINAL_TASKBAR_STATE
                .store(original_state as i32, std::sync::atomic::Ordering::Relaxed);
        }

        // Persist the original state and placement before hiding. The watchdog
        // restores it if this process is force-killed, and startup is a fallback.
        // Written after the original state is known, so the marker can carry it.
        if !visible {
            if start_taskbar_watchdog().is_err() {
                return false;
            }
            let Some(p) = TASKBAR_MARKER.get() else {
                return false;
            };
            let original = ORIGINAL_TASKBAR_STATE.load(std::sync::atomic::Ordering::Relaxed);
            let primary = ORIGINAL_TRAY_RECT.lock().ok().and_then(|guard| *guard);
            let secondaries = ORIGINAL_SEC_TRAY_RECTS
                .lock()
                .ok()
                .and_then(|guard| guard.clone())
                .unwrap_or_default();
            let contents = marker_contents(original, std::process::id(), primary, &secondaries);
            let existing = std::fs::read_to_string(p).ok();
            // Never take over a marker that another Bloom process owns.
            if (existing.is_some() || p.exists())
                && existing.as_deref().and_then(parse_marker_pid) != Some(std::process::id())
            {
                return false;
            }
            // Rewritten when a monitor added since the last hide brings another
            // taskbar, so the watchdog can restore it as well.
            if existing.as_deref() != Some(contents.as_str()) {
                use std::io::Write;
                let written = std::fs::File::create(p).and_then(|mut file| {
                    file.write_all(contents.as_bytes())?;
                    file.sync_all()
                });
                if written.is_err() {
                    return false;
                }
            }
        }

        let state_val = if visible {
            let orig = ORIGINAL_TASKBAR_STATE.load(std::sync::atomic::Ordering::Relaxed);
            if orig != -1 {
                orig as isize
            } else {
                if always_on_top {
                    2
                } else {
                    1
                }
            }
        } else {
            1 // Force Auto-hide when hiding
        };

        // 1. Set the taskbar state (Auto-hide or Always-on-top)
        let mut abd = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: tray_hwnd,
            lParam: windows::Win32::Foundation::LPARAM(state_val),
            ..Default::default()
        };
        SHAppBarMessage(ABM_SETSTATE, &mut abd);

        // 2. Control visibility of the primary taskbar
        let mut primary_positioned = true;
        {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowLongA, SetLayeredWindowAttributes, SetWindowLongA, SetWindowPos,
                GWL_EXSTYLE, LWA_ALPHA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_LAYERED,
                WS_EX_TRANSPARENT,
            };
            if visible {
                // Revert any lingering WS_EX_LAYERED / WS_EX_TRANSPARENT left by
                // open_system_tray if the user quit before the tray thread cleaned up.
                let ex = GetWindowLongA(tray_hwnd, GWL_EXSTYLE);
                let cleaned = ex & !(WS_EX_LAYERED.0 as i32) & !(WS_EX_TRANSPARENT.0 as i32);
                if cleaned != ex {
                    let _ = SetWindowLongA(tray_hwnd, GWL_EXSTYLE, cleaned);
                    let _ = SetLayeredWindowAttributes(
                        tray_hwnd,
                        windows::Win32::Foundation::COLORREF(0),
                        255,
                        LWA_ALPHA,
                    );
                }
                if let Ok(guard) = ORIGINAL_TRAY_RECT.lock() {
                    if let Some(rect) = *guard {
                        primary_positioned = SetWindowPos(
                            tray_hwnd,
                            None,
                            rect.left,
                            rect.top,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                        )
                        .is_ok();
                    }
                }
                let _ = ShowWindow(tray_hwnd, SW_SHOW);
            } else {
                if let Ok(mut guard) = ORIGINAL_TRAY_RECT.lock() {
                    if guard.is_none() {
                        *guard = placed_taskbar_rect(tray_hwnd);
                    }
                }
                let _ = ShowWindow(tray_hwnd, SW_HIDE);
                // Move it far off-screen to prevent any "thin line" artifacts or flashes
                let _ = SetWindowPos(
                    tray_hwnd,
                    None,
                    -10000,
                    -10000,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }

        // 3. Control every secondary taskbar (multi-monitor)
        let mut secondary_positioned = true;
        for &secondary_tray_hwnd in &secondary_trays {
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
            };
            if visible {
                let saved = ORIGINAL_SEC_TRAY_RECTS.lock().ok().and_then(|guard| {
                    guard
                        .as_ref()
                        .and_then(|rects| rects.get(&(secondary_tray_hwnd.0 as isize)).copied())
                });
                if let Some(rect) = saved {
                    secondary_positioned &= SetWindowPos(
                        secondary_tray_hwnd,
                        None,
                        rect.left,
                        rect.top,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    )
                    .is_ok();
                }
                let _ = ShowWindow(secondary_tray_hwnd, SW_SHOW);
            } else {
                let _ = ShowWindow(secondary_tray_hwnd, SW_HIDE);
                let _ = SetWindowPos(
                    secondary_tray_hwnd,
                    None,
                    -10000,
                    -10000,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        let on_screen = |hwnd| {
            let mut rect = windows::Win32::Foundation::RECT::default();
            GetWindowRect(hwnd, &mut rect).is_ok() && rect.left > -9000 && rect.top > -9000
        };
        let primary_matches = primary_positioned
            && IsWindowVisible(tray_hwnd).as_bool() == visible
            && (!visible || on_screen(tray_hwnd));
        let secondary_matches = secondary_positioned
            && secondary_trays.iter().all(|&hwnd| {
                IsWindowVisible(hwnd).as_bool() == visible && (!visible || on_screen(hwnd))
            });
        if visible && primary_matches && secondary_matches {
            if let Some(p) = TASKBAR_MARKER.get() {
                if p.exists() && std::fs::remove_file(p).is_err() {
                    return false;
                }
            }
            // Restored taskbars no longer need their saved placement.
            if let Ok(mut guard) = ORIGINAL_SEC_TRAY_RECTS.lock() {
                if let Some(rects) = guard.as_mut() {
                    for hwnd in &secondary_trays {
                        rects.remove(&(hwnd.0 as isize));
                    }
                }
            }
        }
        primary_matches && secondary_matches
    }
}

/// The `IconFile=` an Internet shortcut's text names, if any.
fn url_shortcut_icon_line(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("IconFile="))
        .map(str::trim)
        .filter(|file| !file.is_empty())
}

/// The icon file an Internet shortcut (`.url`) names. The shell shows every
/// `.url` with the generic web shortcut icon, so web apps installed as one
/// (Google Messages) need it read from the file.
pub fn url_shortcut_icon_file(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let file = expand_env_vars(url_shortcut_icon_line(&text)?);
    std::path::Path::new(&file).is_file().then_some(file)
}

/// Extracts the icon a shortcut declares for itself (`IShellLinkW::GetIconLocation`).
///
/// This is how Firefox web apps and user-customized shortcuts store their icon.
/// Returns only real image files (`.ico`, `.png`, ...). Icon references into
/// executables/DLLs are ignored so the regular target-based extraction is used
/// instead (it yields a better icon than the file's first resource).
pub fn get_shortcut_icon_location(path: &str) -> Option<String> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;
        let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        persist_file
            .Load(
                windows::core::PCWSTR(wide_path.as_ptr()),
                windows::Win32::System::Com::STGM(0),
            )
            .ok()?;

        let mut buffer = [0u16; 512];
        let mut index = 0i32;
        shell_link.GetIconLocation(&mut buffer, &mut index).ok()?;

        let raw = String::from_utf16_lossy(&buffer)
            .trim_matches(char::from(0))
            .trim()
            .to_string();
        if raw.is_empty() {
            return None;
        }

        let location = expand_env_vars(&raw);
        let ext = std::path::Path::new(&location)
            .extension()?
            .to_str()?
            .to_lowercase();
        if !matches!(
            ext.as_str(),
            "ico" | "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif"
        ) {
            return None;
        }
        if !std::path::Path::new(&location).exists() {
            return None;
        }
        Some(location)
    }
}

/// Expands `%VAR%` references using the current process environment.
fn expand_env_vars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let name = &after[..end];
            if !name.is_empty() && !name.contains(' ') {
                match std::env::var(name) {
                    Ok(value) => result.push_str(&value),
                    Err(_) => {
                        result.push('%');
                        result.push_str(name);
                        result.push('%');
                    }
                }
            } else {
                result.push('%');
                result.push_str(name);
                result.push('%');
            }
            rest = &after[end + 1..];
        } else {
            result.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    result.push_str(rest);
    result
}

/// Reads an image file (PNG/ICO/etc.) and returns it as a base64 PNG data URI.
pub fn image_file_to_base64(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }

    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        use base64::Engine;
        return Some(format!(
            "data:image/png;base64,{}",
            general_purpose::STANDARD.encode(&bytes)
        ));
    }

    let img = image::load_from_memory(&bytes).ok()?;
    let mut png: Vec<u8> = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    use base64::Engine;
    Some(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(&png)
    ))
}

/// True when the window's client area covers its monitor and it is not a
/// standard (captioned) maximized window — i.e. a real fullscreen window.
pub fn is_window_fullscreen(hwnd: HWND) -> bool {
    unsafe {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::Graphics::Gdi::{
            ClientToScreen, GetMonitorInfoA, MonitorFromWindow, MONITORINFO,
            MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClientRect, GetWindowLongW, IsZoomed, GWL_STYLE, WS_CAPTION, WS_MAXIMIZE,
        };

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return false;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoA(monitor, &mut info).as_bool() {
            return false;
        }
        let screen = info.rcMonitor;

        let mut client = RECT::default();
        if GetClientRect(hwnd, &mut client).is_err() {
            return false;
        }
        let mut top_left = POINT {
            x: client.left,
            y: client.top,
        };
        let mut bottom_right = POINT {
            x: client.right,
            y: client.bottom,
        };
        let _ = ClientToScreen(hwnd, &mut top_left);
        let _ = ClientToScreen(hwnd, &mut bottom_right);

        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let is_maximized_standard = (IsZoomed(hwnd).as_bool() || (style & WS_MAXIMIZE.0) != 0)
            && (style & WS_CAPTION.0) != 0;

        let client_fullscreen = top_left.x <= screen.left
            && top_left.y <= screen.top
            && bottom_right.x >= screen.right
            && bottom_right.y >= screen.bottom;
        client_fullscreen && !is_maximized_standard
    }
}

/// Resolves a bare executable name (e.g. `notepad.exe`, `msedge`, `wt.exe`) to a
/// full path, using the same mechanisms Windows uses: the registry `App Paths`
/// registration, then the standard executable search path, then a few well-known
/// install locations for apps that register in neither.
///
/// This matters for default pins: on Windows 11 `notepad.exe` resolves through
/// App Paths to the Store Notepad package, whose icon comes from the package
/// manifest rather than the stale stub in System32.
pub fn resolve_executable_path(name: &str) -> Option<String> {
    let trimmed = name.trim().trim_matches('"');
    if trimmed.is_empty() || trimmed.contains('\\') || trimmed.contains('/') {
        return None;
    }

    let file = if trimmed.to_lowercase().ends_with(".exe") {
        trimmed.to_string()
    } else {
        format!("{}.exe", trimmed)
    };

    if let Some(path) = app_paths_lookup(&file) {
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }
    if let Some(path) = search_system_path(&file) {
        return Some(path);
    }
    known_install_location(&file)
}

/// The version in a Squirrel version folder's name: `app-1.0.9264` is
/// `[1, 0, 9264]`. Compared as numbers, so `app-1.0.10000` is newer.
fn squirrel_folder_version(name: &str) -> Option<Vec<u64>> {
    let version = name
        .get(..4)?
        .eq_ignore_ascii_case("app-")
        .then(|| &name[4..])?;
    version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

/// The newest of a Squirrel app's version folders, by name.
fn newest_squirrel_folder<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names
        .into_iter()
        .filter_map(|name| squirrel_folder_version(name).map(|v| (v, name)))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, name)| name)
}

/// Where an app installed by Squirrel (Discord, Slack, GitHub Desktop, Teams
/// classic, Postman, Figma) lives now. Squirrel keeps each version in its own
/// `app-<version>` folder next to `Update.exe` and deletes the old one after an
/// update, so a path saved earlier, such as a dock pin, can point at a folder
/// that's gone. Returns the same executable in the newest version folder (or in
/// `current`, which some apps use), or None when the path still exists or isn't
/// in a version folder.
pub fn moved_squirrel_exe(path: &str) -> Option<String> {
    let path = std::path::Path::new(path.trim().trim_matches('"'));
    if path.exists() {
        return None;
    }
    let folder = path.parent()?;
    let folder_name = folder.file_name()?.to_str()?;
    if squirrel_folder_version(folder_name).is_none()
        && !folder_name.eq_ignore_ascii_case("current")
    {
        return None;
    }
    let root = folder.parent()?;
    let exe = path.file_name()?;
    let names: Vec<String> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().join(exe).is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let newest = newest_squirrel_folder(names.iter().map(String::as_str)).or_else(|| {
        names
            .iter()
            .find(|n| n.eq_ignore_ascii_case("current"))
            .map(String::as_str)
    })?;
    Some(root.join(newest).join(exe).to_string_lossy().into_owned())
}

/// Reads `HKCU`/`HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\<file>`.
fn app_paths_lookup(file: &str) -> Option<String> {
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ,
    };

    unsafe {
        let subkey: Vec<u16> = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{}",
            file
        )
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let mut key = HKEY::default();
            if RegOpenKeyExW(
                root,
                windows::core::PCWSTR(subkey.as_ptr()),
                None,
                KEY_READ,
                &mut key,
            )
            .0 != 0
            {
                continue;
            }

            let mut buffer = [0u16; 1024];
            let mut size = (buffer.len() * 2) as u32;
            let status = RegQueryValueExW(
                key,
                windows::core::PCWSTR::null(),
                None,
                None,
                Some(buffer.as_mut_ptr() as *mut u8),
                Some(&mut size),
            );
            let _ = RegCloseKey(key);
            if status.0 != 0 {
                continue;
            }

            let value = String::from_utf16_lossy(&buffer[..(size as usize / 2).min(buffer.len())]);
            let value = value
                .trim_matches(char::from(0))
                .trim()
                .trim_matches('"')
                .to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
        None
    }
}

/// Standard executable search (PATH, System32, Windows, ...).
fn search_system_path(file: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::SearchPathW;

    unsafe {
        let wide: Vec<u16> = file.encode_utf16().chain(std::iter::once(0)).collect();
        let mut buffer = vec![0u16; 32768];
        let len = SearchPathW(
            None,
            windows::core::PCWSTR(wide.as_ptr()),
            windows::core::PCWSTR::null(),
            Some(&mut buffer),
            None,
        );
        if len == 0 || len as usize >= buffer.len() {
            return None;
        }

        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        if std::path::Path::new(&path).exists() {
            Some(path)
        } else {
            None
        }
    }
}

/// Last resort for apps that are neither registered in App Paths nor on PATH.
fn known_install_location(file: &str) -> Option<String> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let home = std::env::var("USERPROFILE").unwrap_or_default();

    let candidates: Vec<String> = match file.to_lowercase().as_str() {
        "code.exe" | "code-insiders.exe" => vec![
            format!(
                "{}\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe",
                home
            ),
            format!("{}\\Programs\\Microsoft VS Code\\Code.exe", local),
            "C:\\Program Files\\Microsoft VS Code\\Code.exe".into(),
            "C:\\Program Files (x86)\\Microsoft VS Code\\Code.exe".into(),
        ],
        "msedge.exe" => vec![
            "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe".into(),
            "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe".into(),
            format!("{}\\Microsoft\\Edge\\Application\\msedge.exe", local),
        ],
        "notepad.exe" => vec![
            "C:\\Windows\\System32\\notepad.exe".into(),
            "C:\\Windows\\notepad.exe".into(),
        ],
        "explorer.exe" => vec!["C:\\Windows\\explorer.exe".into()],
        _ => Vec::new(),
    };

    candidates
        .into_iter()
        .find(|c| std::path::Path::new(c).exists())
}

/// Reads the `System.AppUserModel.ID` shell property of a window.
///
/// This is the same identity the taskbar uses to group windows: for Store/UWP
/// apps it is the package AppUserModelID, for installed browser web apps it is
/// the browser's web app id. Unlike the process command line it is always
/// available on the window itself.
pub fn get_window_app_user_model_id(hwnd: HWND) -> Option<String> {
    get_window_string_property(hwnd, 5)
}

/// Reads the `System.AppUserModel.RelaunchCommand` shell property of a window:
/// the command the taskbar pins and relaunches for it. Apps whose UI window is
/// owned by a helper process set it to their real executable (Steam's window
/// belongs to steamwebhelper.exe but relaunches Steam.exe).
pub fn get_window_relaunch_command(hwnd: HWND) -> Option<String> {
    get_window_string_property(hwnd, 2)
}

/// The executable named by a relaunch command, when the command is a bare,
/// existing `.exe` path without arguments. Commands with arguments (browser
/// profiles, launchers) are not an app identity and are ignored.
pub fn relaunch_executable(command: &str) -> Option<String> {
    let command = command.trim();
    let exe = match command.strip_prefix('"') {
        Some(rest) => {
            let end = rest.find('"')?;
            if !rest[end + 1..].trim().is_empty() {
                return None;
            }
            &rest[..end]
        }
        None => command,
    };
    if !exe.to_lowercase().ends_with(".exe") || !std::path::Path::new(exe).is_file() {
        return None;
    }
    Some(exe.to_string())
}

/// Package AppUserModelID of a packaged process, which the taskbar uses when
/// the window itself carries none (Snipping Tool's `XamlWindow`).
pub unsafe fn process_app_user_model_id(
    process: windows::Win32::Foundation::HANDLE,
) -> Option<String> {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    let status = GetApplicationUserModelId(
        process,
        &mut len,
        Some(windows::core::PWSTR(buf.as_mut_ptr())),
    );
    if status.0 != 0 || len == 0 {
        return None;
    }
    let text = String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]);
    Some(text.trim().to_string()).filter(|s| !s.is_empty())
}

fn get_window_string_property(hwnd: HWND, pid: u32) -> Option<String> {
    unsafe {
        use windows::Win32::Foundation::PROPERTYKEY;
        use windows::Win32::System::Com::StructuredStorage::{
            PropVariantClear, PropVariantToStringAlloc,
        };
        use windows::Win32::UI::Shell::PropertiesSystem::{
            IPropertyStore, SHGetPropertyStoreForWindow,
        };

        const PKEY_APP_USER_MODEL: PROPERTYKEY = PROPERTYKEY {
            fmtid: windows::core::GUID {
                data1: 0x9F4C2855,
                data2: 0x9F79,
                data3: 0x4B39,
                data4: [0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3],
            },
            pid: 0,
        };
        let key = PROPERTYKEY {
            pid,
            ..PKEY_APP_USER_MODEL
        };

        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let mut prop = store.GetValue(&key).ok()?;
        let result = PropVariantToStringAlloc(&prop).ok().map(|pwstr| {
            let value = String::from_utf16_lossy(pwstr.as_wide()).trim().to_string();
            windows::Win32::System::Com::CoTaskMemFree(Some(pwstr.0 as _));
            value
        });
        let _ = PropVariantClear(&mut prop);
        result.filter(|s| !s.is_empty())
    }
}

pub unsafe fn icon_to_base64(hicon: HICON) -> Option<String> {
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let bitmap = factory.CreateBitmapFromHICON(hicon).ok()?;
    wic_bitmap_to_base64(&factory, &bitmap)
}

/// Encodes an `HBITMAP` (as returned by `IShellItemImageFactory::GetImage`) to a base64 PNG.
pub unsafe fn hbitmap_to_base64(hbitmap: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<String> {
    let factory: IWICImagingFactory =
        CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let bitmap = factory
        .CreateBitmapFromHBITMAP(
            hbitmap,
            windows::Win32::Graphics::Gdi::HPALETTE::default(),
            windows::Win32::Graphics::Imaging::WICBitmapUsePremultipliedAlpha,
        )
        .ok()?;
    wic_bitmap_to_base64(&factory, &bitmap)
}

unsafe fn wic_bitmap_to_base64(
    factory: &IWICImagingFactory,
    bitmap: &windows::Win32::Graphics::Imaging::IWICBitmapSource,
) -> Option<String> {
    let stream = CreateStreamOnHGlobal(HGLOBAL(std::ptr::null_mut()), true).ok()?;
    let encoder = factory
        .CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())
        .ok()?;
    encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;

    let mut frame = None;
    encoder
        .CreateNewFrame(&mut frame, std::ptr::null_mut())
        .ok()?;
    let frame = frame?;
    frame.Initialize(None).ok()?;

    let (mut width, mut height) = (0u32, 0u32);
    bitmap.GetSize(&mut width, &mut height).ok()?;
    frame.SetSize(width, height).ok()?;

    let mut format = GUID_WICPixelFormat32bppPBGRA;
    frame.SetPixelFormat(&mut format).ok()?;

    frame.WriteSource(bitmap, std::ptr::null()).ok()?;
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

pub fn get_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Parse the editable settings file without discarding a UTF-8 BOM. A parse
/// failure must reach the caller rather than turning into an empty map: the
/// next save would otherwise overwrite all of the user's preferences.
pub fn read_settings_file(
    path: &std::path::Path,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(content.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("Invalid settings file: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(e) => Err(format!("Could not read settings file: {e}")),
    }
}

/// Write a complete file in the same directory and replace the old file only
/// after the new contents are ready. A crash or write failure leaves the old file.
pub fn atomic_write_file(path: &std::path::Path, content: &[u8]) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let temp = path.with_file_name(format!(
        "{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("bloom"),
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(e) = std::fs::write(&temp, content) {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("Could not write {}: {e}", temp.display()));
    }
    // Antivirus, indexers and editors can briefly open the destination without
    // FILE_SHARE_DELETE. Keep the original intact and retry only these Windows
    // sharing/access errors; a permanent failure is still returned to Settings.
    let mut replaced = std::fs::rename(&temp, path);
    for delay_ms in [25, 50, 100, 200] {
        if !replaced
            .as_ref()
            .is_err_and(|e| matches!(e.raw_os_error(), Some(5 | 32 | 33)))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        replaced = std::fs::rename(&temp, path);
    }
    if let Err(e) = replaced {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("Could not replace {}: {e}", path.display()));
    }
    Ok(())
}

pub fn write_settings_file(
    path: &std::path::Path,
    settings: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    let content = serde_json::to_vec(settings).map_err(|e| e.to_string())?;
    atomic_write_file(path, &content)
}

/// The four webviews may save at once. Keep each read/modify/write transaction
/// together so one window cannot replace another window's new setting.
pub fn save_setting_file(
    path: &std::path::Path,
    key: String,
    value: serde_json::Value,
) -> Result<std::collections::HashMap<String, serde_json::Value>, String> {
    let _guard = crate::state::SETTINGS_FILE_LOCK
        .lock()
        .map_err(|e| e.to_string())?;
    let mut settings = read_settings_file(path)?;
    settings.insert(key, value);
    write_settings_file(path, &settings)?;
    replace_settings_cache(settings.clone());
    Ok(settings)
}

/// Load settings.json into the in-memory cache. Call once at startup.
pub fn init_settings_cache(app: &tauri::AppHandle) {
    use crate::state::SETTINGS_CACHE;
    use tauri::Manager;
    let cache =
        SETTINGS_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Some(path) = app
        .path()
        .app_config_dir()
        .ok()
        .map(|p| p.join("settings.json"))
    {
        if let Ok(settings) = read_settings_file(&path) {
            if let Ok(mut cache) = cache.lock() {
                *cache = settings;
            }
        }
    }
}

/// Replace the entire settings cache (used by save_setting and the file watcher).
///
/// Uses `get_or_init` rather than `get().unwrap()`: a webview can invoke
/// `save_setting` before the setup hook has initialized the cache, and a panic
/// on the main thread inside a WebView2 callback cannot unwind, which aborts
/// the process.
pub fn replace_settings_cache(new_settings: std::collections::HashMap<String, serde_json::Value>) {
    let cache = crate::state::SETTINGS_CACHE
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        *cache = new_settings;
    }
}

pub fn get_bloom_scale(_app: &tauri::AppHandle) -> f64 {
    get_setting_str(_app, "bloom-scale")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0)
}

/// Read any string value from the settings cache. Returns None if the key is absent.
pub fn get_setting_str(_app: &tauri::AppHandle, key: &str) -> Option<String> {
    let cache = crate::state::SETTINGS_CACHE.get()?;
    let guard = cache.lock().ok()?;
    guard.get(key)?.as_str().map(|s| s.to_string())
}

fn dock_setting_enabled(value: Option<&serde_json::Value>) -> bool {
    value.is_none_or(|value| value.as_str() == Some("true"))
}

/// An absent setting uses the fresh-install default; any explicit value other
/// than the string "true" leaves Windows' taskbar in charge.
pub fn dock_enabled() -> bool {
    crate::state::SETTINGS_CACHE.get().is_none_or(|cache| {
        cache
            .lock()
            .map(|settings| dock_setting_enabled(settings.get("bloom-dock-enabled")))
            .unwrap_or(false)
    })
}

/// Makes a window click-through or not by its WS_EX_TRANSPARENT bit alone,
/// keeping WS_EX_LAYERED set in both states.
///
/// tao's `set_ignore_cursor_events` sets both bits when ignoring and clears
/// both otherwise. A window with a region (window_region.rs) that isn't
/// layered loses its transparency: its clear pixels inside the region paint
/// opaque. Writes only when the style differs, so it can be called on every
/// cursor update, which also restores the bits if tao rewrites the extended
/// style (it does on its own flag changes, such as show and hide). Returns
/// whether it changed anything.
pub fn set_click_through(hwnd: HWND, ignore: bool) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE,
    };
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let wanted = click_through_style(current, ignore);
        if wanted == current {
            return false;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);
        true
    }
}

/// The extended style with WS_EX_LAYERED set and WS_EX_TRANSPARENT set only
/// when ignoring input, every other bit kept.
fn click_through_style(style: u32, ignore: bool) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{WS_EX_LAYERED, WS_EX_TRANSPARENT};
    let layered = style | WS_EX_LAYERED.0;
    if ignore {
        layered | WS_EX_TRANSPARENT.0
    } else {
        layered & !WS_EX_TRANSPARENT.0
    }
}

/// Re-assert HWND_TOPMOST without activating the window.
///
/// Tauri's `set_always_on_top(true)` calls `SetWindowPos(HWND_TOPMOST)` without
/// `SWP_NOACTIVATE`, which causes Windows to send `WM_ACTIVATE` to the WebView2 window.
/// This activation message makes the WebView compositor briefly blank/hide the window,
/// and can also strip the `WS_EX_NOACTIVATE` extended style.
///
/// This helper uses the raw Win32 call with the correct flags and re-stamps
/// `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` to prevent both problems.
pub fn re_assert_topmost(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSENDCHANGING, SWP_NOSIZE, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
        };
        // Set topmost without activating or notifying the window
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
        );
        // Re-stamp NOACTIVATE + TOOLWINDOW — HWND_TOPMOST can cause these to be reset
        // by the shell on some Windows builds
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
        let new_ex = ex | WS_EX_NOACTIVATE.0 as usize | WS_EX_TOOLWINDOW.0 as usize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex as isize);
    }
}

/// Captures an HWND to a base64-encoded PNG thumbnail, scaling it down if it exceeds max_width x max_height.
pub fn capture_hwnd_to_base64(hwnd: HWND, max_width: u32, max_height: u32) -> Option<String> {
    unsafe {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
            ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowPlacement, IsWindow, WINDOWPLACEMENT,
        };
        #[link(name = "user32")]
        extern "system" {
            pub fn PrintWindow(hwnd: HWND, hdcBlt: HDC, nFlags: u32) -> i32;
        }

        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }

        let mut rect = RECT::default();
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
        if rect.right == 0
            && rect.bottom == 0
            && windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_err()
        {
            return None;
        }
        let mut width = rect.right - rect.left;
        let mut height = rect.bottom - rect.top;
        if width <= 10 || height <= 10 {
            let mut wp = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(hwnd, &mut wp).is_ok() {
                width = wp.rcNormalPosition.right - wp.rcNormalPosition.left;
                height = wp.rcNormalPosition.bottom - wp.rcNormalPosition.top;
            }
        }
        if width <= 100 || height <= 100 || width > 7680 || height > 4320 {
            return None;
        }

        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        let hbm_mem = CreateCompatibleBitmap(hdc_screen, width, height);
        let h_old = SelectObject(hdc_mem, hbm_mem.into());
        let success = PrintWindow(hwnd, hdc_mem, 2);

        let mut result = None;
        if success != 0 {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default(); 1],
            };
            let mut pixels = vec![0u8; (width * height * 4) as usize];
            if GetDIBits(
                hdc_mem,
                hbm_mem,
                0,
                height as u32,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            ) != 0
            {
                for chunk in pixels.chunks_exact_mut(4) {
                    let b = chunk[0];
                    let r = chunk[2];
                    chunk[0] = r;
                    chunk[2] = b;
                    chunk[3] = 255;
                }
                if let Ok(Some(png_base64)) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if let Some(mut img) =
                            image::RgbaImage::from_raw(width as u32, height as u32, pixels)
                        {
                            if img.width() > max_width || img.height() > max_height {
                                let dyn_img = image::DynamicImage::ImageRgba8(img);
                                img = dyn_img
                                    .resize(
                                        max_width,
                                        max_height,
                                        image::imageops::FilterType::Triangle,
                                    )
                                    .into_rgba8();
                            }
                            let mut buf = std::io::Cursor::new(Vec::new());
                            if image::write_buffer_with_format(
                                &mut buf,
                                &img,
                                img.width(),
                                img.height(),
                                image::ColorType::Rgba8,
                                image::ImageFormat::Png,
                            )
                            .is_ok()
                            {
                                use base64::Engine;
                                let b64 = base64::engine::general_purpose::STANDARD
                                    .encode(buf.into_inner());
                                return Some(format!("data:image/png;base64,{}", b64));
                            }
                        }
                        None
                    }))
                {
                    result = Some(png_base64);
                }
            }
        }
        SelectObject(hdc_mem, h_old);
        let _ = DeleteObject(hbm_mem.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::{
        click_through_style, expand_env_vars, is_placed_rect, marker_contents, moved_squirrel_exe,
        newest_squirrel_folder, parse_marker_pid, parse_marker_rect, parse_marker_secondaries,
        parse_marker_state, relaunch_executable, url_shortcut_icon_line,
    };

    #[test]
    fn picks_the_newest_squirrel_version_by_number() {
        assert_eq!(
            newest_squirrel_folder(["app-1.0.9264", "app-1.0.10000", "app-1.0.999", "packages"]),
            Some("app-1.0.10000")
        );
        assert_eq!(
            newest_squirrel_folder(["App-3.2", "app-3.10"]),
            Some("app-3.10")
        );
        assert_eq!(
            newest_squirrel_folder(["app-beta", "current", "packages"]),
            None
        );
    }

    #[test]
    fn finds_a_squirrel_app_after_an_update() {
        let root = std::env::temp_dir().join(format!("bloom-squirrel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for version in ["app-1.0.9264", "app-1.0.10000"] {
            std::fs::create_dir_all(root.join(version)).unwrap();
            std::fs::write(root.join(version).join("Discord.exe"), b"").unwrap();
        }
        // A newer folder without the exe (an update still unpacking) is skipped.
        std::fs::create_dir_all(root.join("app-1.0.20000")).unwrap();
        let stale = root.join("app-1.0.9262").join("Discord.exe");
        let moved = moved_squirrel_exe(&stale.to_string_lossy());
        assert_eq!(
            moved.as_deref(),
            Some(
                root.join("app-1.0.10000")
                    .join("Discord.exe")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        // A path that still exists, or that isn't in a version folder, stays put.
        let current = root.join("app-1.0.9264").join("Discord.exe");
        assert_eq!(moved_squirrel_exe(&current.to_string_lossy()), None);
        assert_eq!(
            moved_squirrel_exe(&root.join("Discord.exe").to_string_lossy()),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relaunch_identity_requires_an_existing_executable_without_arguments() {
        let exe = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(relaunch_executable(&exe), Some(exe.clone()));
        assert_eq!(
            relaunch_executable(&format!("\"{exe}\"")),
            Some(exe.clone())
        );
        assert_eq!(
            relaunch_executable(&format!("\"{exe}\" --profile-directory=Default")),
            None
        );
        assert_eq!(relaunch_executable(&format!("{exe} --argument")), None);
        assert_eq!(relaunch_executable("C:\\does\\not\\exist.exe"), None);
        assert_eq!(relaunch_executable(""), None);
    }

    #[test]
    fn reads_the_icon_an_internet_shortcut_names() {
        let messages = "[{000214A0-0000-0000-C000-000000000046}]\r\nProp3=19,11\r\n[InternetShortcut]\r\nIDList=\r\nURL=https://messages.google.com/web\r\nIconFile=c:\\windows\\Google Messages.ico\r\nIconIndex=0\r\n";
        assert_eq!(
            url_shortcut_icon_line(messages),
            Some("c:\\windows\\Google Messages.ico")
        );
        // A documentation link without its own icon.
        let docs = "[InternetShortcut]\nIconIndex=0\nURL=https://nodejs.org/\nIconFile=\n";
        assert_eq!(url_shortcut_icon_line(docs), None);
    }

    #[test]
    fn click_through_keeps_the_window_layered() {
        const LAYERED: u32 = 0x0008_0000;
        const TRANSPARENT: u32 = 0x0000_0020;
        const NOACTIVATE: u32 = 0x0800_0000;
        // tao's interactive style: neither bit. Taking clicks adds layered.
        assert_eq!(click_through_style(NOACTIVATE, false), NOACTIVATE | LAYERED);
        // Click-through adds both; other bits are kept.
        assert_eq!(
            click_through_style(NOACTIVATE, true),
            NOACTIVATE | LAYERED | TRANSPARENT
        );
        // Back to taking clicks drops only the transparent bit.
        assert_eq!(
            click_through_style(NOACTIVATE | LAYERED | TRANSPARENT, false),
            NOACTIVATE | LAYERED
        );
    }
    use std::collections::HashMap;
    static TEST_SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn test_settings_path() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "bloom-settings-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).expect("create test directory");
        dir.join("settings.json")
    }

    #[test]
    fn settings_bom_is_read_and_a_damaged_file_is_never_overwritten() {
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        let path = test_settings_path();
        std::fs::write(&path, "\u{feff}{\"bloom-existing\":\"true\"}").unwrap();
        let settings = super::read_settings_file(&path).unwrap();
        assert_eq!(settings["bloom-existing"], "true");

        super::save_setting_file(&path, "bloom-new".into(), "false".into()).unwrap();
        let settings = super::read_settings_file(&path).unwrap();
        assert_eq!(settings.len(), 2);

        std::fs::write(&path, "{invalid").unwrap();
        assert!(super::save_setting_file(&path, "bloom-another".into(), "true".into()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{invalid");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn simultaneous_setting_saves_keep_every_key() {
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        let path = test_settings_path();
        let workers: Vec<_> = (0..12)
            .map(|index| {
                let path = path.clone();
                std::thread::spawn(move || {
                    super::save_setting_file(
                        &path,
                        format!("bloom-key-{index}"),
                        serde_json::json!(index.to_string()),
                    )
                    .unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(super::read_settings_file(&path).unwrap().len(), 12);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn atomic_settings_save_retries_a_temporary_windows_file_lock() {
        use std::os::windows::fs::OpenOptionsExt;
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        let path = test_settings_path();
        std::fs::write(&path, "{\"bloom-existing\":\"true\"}").unwrap();
        // Allow reads and writes, but withhold FILE_SHARE_DELETE as an editor
        // or scanner can do. Replacing this file fails until the handle closes.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            drop(held);
        });
        super::save_setting_file(&path, "bloom-new".into(), "false".into()).unwrap();
        release.join().unwrap();
        let settings = super::read_settings_file(&path).unwrap();
        assert_eq!(settings["bloom-existing"], "true");
        assert_eq!(settings["bloom-new"], "false");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn taskbar_marker_round_trips_the_original_state() {
        for state in [0, 1, 2, 3] {
            let rect = windows::Win32::Foundation::RECT {
                left: 12,
                top: 34,
                right: 1234,
                bottom: 56,
            };
            let contents = marker_contents(state, 1234, Some(rect), &HashMap::new());
            assert_eq!(parse_marker_state(&contents), Some(state));
            assert_eq!(parse_marker_pid(&contents), Some(1234));
            assert_eq!(parse_marker_rect(&contents, "primary="), Some(rect));
            assert!(parse_marker_secondaries(&contents).is_empty());
        }
    }

    #[test]
    fn a_failed_setting_write_keeps_the_file_and_cache_then_allows_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        let path = test_settings_path();
        super::save_setting_file(&path, "bloom-theme-mode".into(), "dark".into()).unwrap();
        let original = std::fs::read(&path).unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        let result = super::save_setting_file(&path, "bloom-theme-mode".into(), "light".into());
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(
            crate::state::SETTINGS_CACHE.get().unwrap().lock().unwrap()["bloom-theme-mode"],
            "dark"
        );
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        drop(held);
        super::save_setting_file(&path, "bloom-theme-mode".into(), "light".into()).unwrap();
        assert_eq!(
            super::read_settings_file(&path).unwrap()["bloom-theme-mode"],
            "light"
        );
        assert_eq!(
            crate::state::SETTINGS_CACHE.get().unwrap().lock().unwrap()["bloom-theme-mode"],
            "light"
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn taskbar_marker_round_trips_every_secondary_taskbar() {
        let rect = |left, top| windows::Win32::Foundation::RECT {
            left,
            top,
            right: left + 1920,
            bottom: top + 48,
        };
        let secondaries = HashMap::from([(0x1010, rect(1920, 1032)), (0x2020, rect(-1920, 1032))]);
        let contents = marker_contents(1, 7, None, &secondaries);
        assert_eq!(parse_marker_rect(&contents, "primary="), None);
        assert_eq!(parse_marker_secondaries(&contents), secondaries);
        // A damaged line is skipped without losing the others.
        let damaged = format!("{contents}secondary=99,1,2,3\nsecondary=x,1,2,3,4\n");
        assert_eq!(parse_marker_secondaries(&damaged), secondaries);
    }

    #[test]
    fn legacy_and_damaged_markers_carry_no_state() {
        // Older versions wrote a bare "1": it must not be read as auto-hide.
        assert_eq!(parse_marker_state("1"), None);
        assert_eq!(parse_marker_state(""), None);
        assert_eq!(parse_marker_state("abstate="), None);
        assert_eq!(parse_marker_state("abstate=x"), None);
        assert_eq!(parse_marker_state("abstate=2\r\n"), Some(2));
        assert_eq!(parse_marker_pid("abstate=2\r\n"), None);
        assert_eq!(parse_marker_rect("primary=1,2,3", "primary="), None);
    }

    #[test]
    fn unplaced_taskbar_rects_are_not_restored() {
        let rect = |left, top, right, bottom| windows::Win32::Foundation::RECT {
            left,
            top,
            right,
            bottom,
        };
        assert!(is_placed_rect(&rect(0, 1020, 1920, 1080)));
        assert!(is_placed_rect(&rect(-1920, 1032, 0, 1080)));
        // A taskbar Explorer hasn't placed yet, and one Bloom parked off-screen.
        assert!(!is_placed_rect(&rect(0, 0, 0, 0)));
        assert!(!is_placed_rect(&rect(-10000, -10000, -8080, -9952)));
    }

    #[test]
    fn env_expansion() {
        let out = expand_env_vars("%SystemRoot%\\System32");
        assert!(out.to_lowercase().ends_with("\\system32"), "got {out}");
        assert!(!out.contains('%'), "got {out}");

        // Unknown variables stay verbatim
        assert_eq!(
            expand_env_vars("%BLOOM_NOT_A_REAL_VAR%\\x"),
            "%BLOOM_NOT_A_REAL_VAR%\\x"
        );

        // Unclosed percent is left alone
        assert_eq!(expand_env_vars("50% done"), "50% done");

        // Plain paths are untouched
        assert_eq!(expand_env_vars("C:\\plain\\path"), "C:\\plain\\path");
    }

    #[test]
    fn bare_executable_resolution() {
        use super::resolve_executable_path;

        for name in ["notepad", "notepad.exe", "msedge"] {
            let path =
                resolve_executable_path(name).unwrap_or_else(|| panic!("{name} did not resolve"));
            assert!(
                std::path::Path::new(&path).exists(),
                "{name} -> {path} does not exist"
            );
            assert!(path.to_lowercase().ends_with(".exe"), "{name} -> {path}");
        }

        // Already-qualified paths and nonsense are not resolved
        assert_eq!(resolve_executable_path("C:\\Windows\\notepad.exe"), None);
        assert_eq!(
            resolve_executable_path("bloom-definitely-not-installed"),
            None
        );
        assert_eq!(resolve_executable_path(""), None);
    }

    #[test]
    fn replace_settings_cache_before_init_does_not_panic() {
        let _guard = TEST_SETTINGS_LOCK.lock().unwrap();
        use super::replace_settings_cache;
        use crate::state::SETTINGS_CACHE;

        // Mirrors a webview invoking save_setting before the setup hook has
        // initialized the cache.
        let mut settings = std::collections::HashMap::new();
        settings.insert("bloom-test".to_string(), serde_json::json!("true"));
        replace_settings_cache(settings);

        let cache = SETTINGS_CACHE.get().expect("cache initialized");
        let guard = cache.lock().expect("cache lock");
        assert_eq!(
            guard.get("bloom-test").and_then(|value| value.as_str()),
            Some("true")
        );
    }

    #[test]
    fn dock_setting_defaults_on_but_rejects_explicit_invalid_values() {
        use super::dock_setting_enabled;

        assert!(dock_setting_enabled(None));
        assert!(dock_setting_enabled(Some(&serde_json::json!("true"))));
        assert!(!dock_setting_enabled(Some(&serde_json::json!("false"))));
        assert!(!dock_setting_enabled(Some(&serde_json::json!("invalid"))));
        assert!(!dock_setting_enabled(Some(&serde_json::json!(true))));
    }
}
