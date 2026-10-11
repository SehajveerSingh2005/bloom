//! Linux backends for the native calls the Windows build makes through Win32.
//!
//! Windows are read and arranged through X11/EWMH: Bloom asks GTK for its X11
//! backend (XWayland on Wayland sessions) because Wayland lets no client place
//! its own windows or see other apps' windows. Without an X server the window
//! calls do nothing. Installed apps come from freedesktop `.desktop` files,
//! media from MPRIS, and system toggles from the usual tools (wpctl/pactl,
//! nmcli, bluetoothctl, brightnessctl, powerprofilesctl).

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose, Engine as _};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _, EventMask,
    PropMode, Window,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use zbus::zvariant::{OwnedValue, Value};

use crate::services::{dock_span_px, window_reaches_dock, Cursor};
use crate::state::*;
use crate::types::{AppInfo, AudioSessionInfo, BrightnessChangeEvent, MediaInfo, SystemCommand};
use crate::utils::{get_bloom_scale, get_now_ms, image_file_to_base64};

// --- Window handles -------------------------------------------------------

/// Stand-in for Win32's `HWND` in the window code shared with Windows: the
/// X11 window id, null when the window has none (no X server).
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HWND(pub *mut std::ffi::c_void);

/// `hwnd()` for Bloom's own windows, like tauri's Windows-only method.
pub trait WindowHandleExt {
    fn hwnd(&self) -> tauri::Result<HWND>;
}

impl<R: Runtime> WindowHandleExt for WebviewWindow<R> {
    fn hwnd(&self) -> tauri::Result<HWND> {
        Ok(HWND(xid(self) as usize as *mut _))
    }
}

/// X11 id of one of Bloom's windows, 0 when it has none.
fn xid<R: Runtime>(window: &WebviewWindow<R>) -> Window {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    static XIDS: Mutex<Option<HashMap<String, Window>>> = Mutex::new(None);
    let mut guard = XIDS.lock().unwrap_or_else(|e| e.into_inner());
    let ids = guard.get_or_insert_with(HashMap::new);
    if let Some(&id) = ids.get(window.label()) {
        return id;
    }
    let id = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xlib(h)) => h.window as Window,
        _ => 0,
    };
    if id != 0 {
        ids.insert(window.label().to_string(), id);
    }
    id
}

/// `SetWindowPos` stand-in for Bloom's own windows.
pub fn place_window(window: &WebviewWindow, x: i32, y: i32, width: i32, height: i32) {
    let _ = window.set_position(PhysicalPosition::new(x, y));
    let _ = window.set_size(PhysicalSize::new(width.max(1) as u32, height.max(1) as u32));
}

/// ponytail: `alwaysOnTop` already sets `_NET_WM_STATE_ABOVE` and X11 has no
/// activation side effects to undo, so there is nothing to re-assert.
pub fn re_assert_topmost(_hwnd: HWND) {}

/// ponytail: desktop panels (GNOME Shell, Plasma, XFCE) have no common hide
/// API, so the native panel stays; Bloom's dock reserves its own strip.
pub fn set_taskbar_visibility(_visible: bool, _always_on_top: bool) {}

/// The panel is never hidden (see `set_taskbar_visibility`), so a crash leaves
/// nothing to restore.
pub fn restore_taskbar_after_crash() {}

/// Dock-type hints, set before the windows are first shown, keep Bloom's
/// surfaces out of the window switcher, make the WM honour their struts, and
/// stop it treating the screen-sized overlay as a fullscreen app.
pub fn mark_dock_windows(app: &AppHandle) {
    use gtk::prelude::GtkWindowExt;
    for label in ["main", "dock", "overlay"] {
        if let Some(window) = app.get_webview_window(label).and_then(|w| w.gtk_window().ok()) {
            window.set_type_hint(gtk::gdk::WindowTypeHint::Dock);
        }
    }
}

// --- X11 ------------------------------------------------------------------

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_CLIENT_LIST,
        _NET_ACTIVE_WINDOW,
        _NET_CLOSE_WINDOW,
        _NET_WM_PID,
        _NET_WM_NAME,
        _NET_WM_ICON,
        _NET_WM_STATE,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_FRAME_EXTENTS,
        WM_CHANGE_STATE,
    }
}

struct X11 {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
}

fn connect_x11() -> Option<X11> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let atoms = Atoms::new(&conn).ok()?.reply().ok()?;
    Some(X11 { conn, root, atoms })
}

/// The shared X connection; `None` without an X server (pure Wayland).
fn x11() -> Option<&'static X11> {
    static CONN: OnceLock<Option<X11>> = OnceLock::new();
    CONN.get_or_init(connect_x11).as_ref()
}

impl X11 {
    fn property(&self, win: Window, prop: impl Into<u32>) -> Option<xproto::GetPropertyReply> {
        let cookie = self
            .conn
            .get_property(false, win, prop, AtomEnum::ANY, 0, u32::MAX)
            .ok()?;
        cookie.reply().ok()
    }

    fn prop32(&self, win: Window, prop: u32) -> Vec<u32> {
        self.property(win, prop)
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn text(&self, win: Window, prop: impl Into<u32>) -> String {
        self.property(win, prop)
            .map(|r| String::from_utf8_lossy(&r.value).into_owned())
            .unwrap_or_default()
    }

    fn title(&self, win: Window) -> String {
        let title = self.text(win, self.atoms._NET_WM_NAME);
        if title.is_empty() {
            self.text(win, AtomEnum::WM_NAME)
        } else {
            title
        }
    }

    /// `WM_CLASS` as (instance, class).
    fn wm_class(&self, win: Window) -> (String, String) {
        let raw = self.text(win, AtomEnum::WM_CLASS);
        let mut parts = raw.split('\0');
        let instance = parts.next().unwrap_or_default().to_string();
        let class = parts.next().unwrap_or_default().to_string();
        (instance, class)
    }

    fn pid(&self, win: Window) -> u32 {
        self.prop32(win, self.atoms._NET_WM_PID)
            .first()
            .copied()
            .unwrap_or(0)
    }

    fn states(&self, win: Window) -> Vec<u32> {
        self.prop32(win, self.atoms._NET_WM_STATE)
    }

    fn types(&self, win: Window) -> Vec<u32> {
        self.prop32(win, self.atoms._NET_WM_WINDOW_TYPE)
    }

    fn client_list(&self) -> Vec<Window> {
        self.prop32(self.root, self.atoms._NET_CLIENT_LIST)
    }

    fn active(&self) -> Window {
        self.prop32(self.root, self.atoms._NET_ACTIVE_WINDOW)
            .first()
            .copied()
            .unwrap_or(0)
    }

    /// EWMH request to the window manager about `win`.
    fn message(&self, win: Window, kind: u32, data: [u32; 5]) {
        let event = ClientMessageEvent::new(32, win, kind, data);
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        let _ = self.conn.send_event(false, self.root, mask, event);
        let _ = self.conn.flush();
    }

    /// Frame rectangle (x, y, width, height) in root coordinates.
    fn rect(&self, win: Window) -> Option<(i32, i32, i32, i32)> {
        let geometry = self.conn.get_geometry(win).ok()?.reply().ok()?;
        let origin = self
            .conn
            .translate_coordinates(win, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        // left, right, top, bottom decorations drawn by the WM
        let frame = self.prop32(win, self.atoms._NET_FRAME_EXTENTS);
        let f = |i: usize| frame.get(i).copied().unwrap_or(0) as i32;
        Some((
            origin.dst_x as i32 - f(0),
            origin.dst_y as i32 - f(2),
            geometry.width as i32 + f(0) + f(1),
            geometry.height as i32 + f(2) + f(3),
        ))
    }

    fn root_height(&self) -> Option<i32> {
        let geometry = self.conn.get_geometry(self.root).ok()?.reply().ok()?;
        Some(geometry.height as i32)
    }
}

/// Last active window that isn't Bloom's: clicking the dock can activate it.
static LAST_FOREIGN_ACTIVE: AtomicU32 = AtomicU32::new(0);

/// The window the user is working in, looking past Bloom's own windows.
fn effective_active(x: &X11) -> Window {
    let win = x.active();
    if win != 0 && x.pid(win) != std::process::id() {
        LAST_FOREIGN_ACTIVE.store(win, Ordering::Relaxed);
        return win;
    }
    LAST_FOREIGN_ACTIVE.load(Ordering::Relaxed)
}

pub fn cursor_pos() -> Option<Cursor> {
    let x = x11()?;
    let pointer = x.conn.query_pointer(x.root).ok()?.reply().ok()?;
    Some(Cursor {
        x: pointer.root_x as i32,
        y: pointer.root_y as i32,
    })
}

/// X11 clients get no global mouse hook, so the pointer is polled at the
/// Windows hook's 32 ms throttle and fed to the same hit-testing.
pub fn poll_pointer(app: AppHandle) {
    if x11().is_none() {
        return;
    }
    std::thread::spawn(move || {
        let mut last = None;
        loop {
            std::thread::sleep(Duration::from_millis(32));
            // Stands down during shutdown, like the Windows mouse hook.
            if SHUTTING_DOWN.load(Ordering::Relaxed) {
                continue;
            }
            let Some(cursor) = cursor_pos() else { continue };
            if last != Some(cursor) {
                last = Some(cursor);
                crate::services::handle_mouse_move(&app, cursor, get_now_ms());
            }
        }
    });
}

// --- Dock struts (AppBar) --------------------------------------------------

/// Reserves `size` px along the top or bottom of the monitor for a Bloom
/// window: the X11 counterpart of `ABM_SETPOS`. Struts count from the edges of
/// the whole X screen, so a monitor that doesn't reach them adds the gap.
fn reserve_edge(
    window: &WebviewWindow,
    top: bool,
    size: u32,
    m_pos: PhysicalPosition<i32>,
    m_size: PhysicalSize<u32>,
) {
    let (Some(x), win) = (x11(), xid(window)) else { return };
    let Some(root_h) = x.root_height() else { return };
    if win == 0 {
        return;
    }
    let x0 = m_pos.x.max(0) as u32;
    let x1 = (m_pos.x + m_size.width as i32 - 1).max(0) as u32;
    let mut strut = [0u32; 12];
    if top {
        strut[2] = m_pos.y.max(0) as u32 + size;
        strut[8] = x0;
        strut[9] = x1;
    } else {
        strut[3] = (root_h - m_pos.y - m_size.height as i32).max(0) as u32 + size;
        strut[10] = x0;
        strut[11] = x1;
    }
    let (partial, legacy) = (x.atoms._NET_WM_STRUT_PARTIAL, x.atoms._NET_WM_STRUT);
    let _ = x.conn.change_property32(PropMode::REPLACE, win, partial, AtomEnum::CARDINAL, &strut);
    let _ = x.conn.change_property32(PropMode::REPLACE, win, legacy, AtomEnum::CARDINAL, &strut[..4]);
    let _ = x.conn.flush();
}

pub fn unregister_appbar_native(hwnd: HWND) {
    let win = hwnd.0 as usize as Window;
    let Some(x) = x11() else { return };
    if win == 0 {
        return;
    }
    let _ = x.conn.delete_property(win, x.atoms._NET_WM_STRUT_PARTIAL);
    let _ = x.conn.delete_property(win, x.atoms._NET_WM_STRUT);
    let _ = x.conn.flush();
}

/// Fixed notch mode: the window spans the top of the primary monitor and
/// reserves the 40 px strip under the notch.
pub fn register_appbar(window: WebviewWindow) {
    let Ok(Some(monitor)) = window.app_handle().primary_monitor() else {
        tauri::async_runtime::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if let Ok(Some(_)) = window.app_handle().primary_monitor() {
                    crate::services::reconcile_main_appbar(window.app_handle());
                    break;
                }
            }
        });
        return;
    };
    let (pos, size) = (*monitor.position(), *monitor.size());
    let scale = monitor.scale_factor() * get_bloom_scale(window.app_handle());
    let ph = (420.0 * scale) as i32;
    place_window(&window, pos.x, pos.y, size.width as i32, ph);
    reserve_edge(&window, true, (40.0 * scale) as u32, pos, size);
    MAIN_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
    }
}

/// Fixed dock mode: the window sits on the bottom of the primary monitor and
/// reserves the 56 px strip under the dock.
pub fn register_dock_appbar_inner(window: WebviewWindow, attempt: i32) {
    let Ok(Some(monitor)) = window.app_handle().primary_monitor() else {
        tauri::async_runtime::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if let Ok(Some(_)) = window.app_handle().primary_monitor() {
                    crate::services::register_dock_appbar(window);
                    break;
                }
            }
        });
        return;
    };
    // outer_size() is 0 until the window has rendered: retry, never guess.
    let ph = window.outer_size().map(|s| s.height as i32).unwrap_or(0);
    if ph <= 0 {
        if attempt < 10 {
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                register_dock_appbar_inner(window, attempt + 1);
            });
        }
        return;
    }
    let (pos, size) = (*monitor.position(), *monitor.size());
    let scale = monitor.scale_factor() * get_bloom_scale(window.app_handle());
    let bottom = pos.y + size.height as i32;
    place_window(&window, pos.x, bottom - ph, size.width as i32, ph);
    reserve_edge(&window, false, (56.0 * scale) as u32, pos, size);
    DOCK_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
    }
}

/// Polls the primary monitor (X11 has no `WM_DISPLAYCHANGE` for plain
/// clients) and re-lays-out Bloom's windows when it changes.
pub fn setup_display_change_monitor(app: AppHandle) {
    std::thread::spawn(move || {
        let snapshot = |app: &AppHandle| {
            app.primary_monitor()
                .ok()
                .flatten()
                .map(|m| (*m.position(), *m.size(), m.scale_factor().to_bits()))
        };
        let mut last = snapshot(&app);
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let current = snapshot(&app);
            if current.is_some() && current != last {
                last = current;
                crate::services::reposition_all_windows(&app);
            }
        }
    });
}

// --- Running windows --------------------------------------------------------

fn is_taskbar_window(x: &X11, win: Window) -> bool {
    let a = &x.atoms;
    let types = x.types(win);
    !types.contains(&a._NET_WM_WINDOW_TYPE_DOCK)
        && !types.contains(&a._NET_WM_WINDOW_TYPE_DESKTOP)
        && !x.states(win).contains(&a._NET_WM_STATE_SKIP_TASKBAR)
}

/// One `AppInfo` per taskbar window, like `enum_windows_proc`. Windows that
/// belong to an installed app carry its desktop file as `path`, so they match
/// the app's pin; others fall back to their executable.
pub fn list_windows() -> Vec<AppInfo> {
    let Some(x) = x11() else { return Vec::new() };
    let own = std::process::id();
    x.client_list()
        .into_iter()
        .filter_map(|win| {
            let title = x.title(win);
            let pid = x.pid(win);
            if title.is_empty() || !is_taskbar_window(x, win) || (pid == own && title != "Settings")
            {
                return None;
            }
            let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            let (instance, class) = x.wm_class(win);
            let (name, path, executable) = match entry_for_window(&instance, &class, exe.as_deref())
            {
                Some(entry) => (entry.name, entry.path, Some(entry.id)),
                None => {
                    let path = exe.unwrap_or(class);
                    let exe_name = file_name(&path);
                    (exe_name.clone().unwrap_or(title), path, exe_name)
                }
            };
            Some(AppInfo {
                name,
                path,
                icon: None,
                is_running: true,
                hwnd: Some(win as isize),
                executable,
                all_hwnds: None,
            })
        })
        .collect()
}

fn file_name(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// Launchers whose name says nothing about the app they start.
const GENERIC_LAUNCHERS: [&str; 7] = ["flatpak", "snap", "env", "sh", "bash", "python3", "java"];

/// The installed app a window belongs to: by `StartupWMClass` or desktop id
/// against `WM_CLASS`, else by executable name.
fn entry_for_window(instance: &str, class: &str, exe: Option<&str>) -> Option<DesktopEntry> {
    let entries = desktop_entries();
    let wm_classes: Vec<String> = [instance, class]
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| c.to_lowercase())
        .collect();
    let by_class = entries.iter().find(|e| {
        let id = e.id.to_lowercase();
        let short_id = id.rsplit('.').next().unwrap_or(&id).to_string();
        let wm = e.wm_class.to_lowercase();
        wm_classes
            .iter()
            .any(|c| *c == wm || *c == id || *c == short_id)
    });
    let by_exe = || {
        let exe_name = file_name(exe?)?.to_lowercase();
        entries.iter().find(|e| {
            e.exec_bin == exe_name && !GENERIC_LAUNCHERS.contains(&e.exec_bin.as_str())
        })
    };
    by_class.or_else(by_exe).cloned()
}

/// Activates `win`, or minimizes it when it already has focus (the dock's
/// click toggle, same rules as the Windows `focus_window`).
fn toggle_window(win: Window) {
    let Some(x) = x11() else { return };
    let a = &x.atoms;
    let hidden = x.states(win).contains(&a._NET_WM_STATE_HIDDEN);
    let fg = effective_active(x);
    let same_app = fg != 0 && x.pid(fg) != 0 && x.pid(fg) == x.pid(win);
    let recently_focused = FOCUS_TIMESTAMPS
        .get()
        .and_then(|m| m.lock().ok()?.get(&(win as isize)).copied())
        .is_some_and(|ts| get_now_ms() - ts < 2000);
    if !hidden && (fg == win || same_app || recently_focused) {
        // WM_CHANGE_STATE to IconicState
        x.message(win, a.WM_CHANGE_STATE, [3, 0, 0, 0, 0]);
    } else {
        // Source 2: a pager, which WMs obey without focus-stealing checks.
        x.message(win, a._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0]);
    }
}

static LAST_WINDOW_CHANGE_MS: AtomicI64 = AtomicI64::new(0);

fn note_active(x: &X11) {
    let win = x.active();
    if win == 0 || x.pid(win) == std::process::id() {
        return;
    }
    LAST_FOREIGN_ACTIVE.store(win, Ordering::Relaxed);
    if let Some(map) = FOCUS_TIMESTAMPS.get() {
        if let Ok(mut guard) = map.lock() {
            guard.insert(win as isize, get_now_ms());
        }
    }
}

/// `windows-changed` when the client list changes and focus timestamps when
/// the active window does, from root property events.
pub fn setup_window_change_hook(app: AppHandle) {
    std::thread::spawn(move || {
        // Own connection: this thread blocks in wait_for_event.
        let Some(x) = connect_x11() else { return };
        let events = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
        if x.conn.change_window_attributes(x.root, &events).is_err() || x.conn.flush().is_err() {
            return;
        }
        note_active(&x);
        while let Ok(event) = x.conn.wait_for_event() {
            let Event::PropertyNotify(e) = event else { continue };
            if e.atom == x.atoms._NET_ACTIVE_WINDOW {
                note_active(&x);
            } else if e.atom == x.atoms._NET_CLIENT_LIST {
                // Debounced like the Windows hook: only the last of a burst emits.
                let now = get_now_ms();
                LAST_WINDOW_CHANGE_MS.store(now, Ordering::Relaxed);
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    if LAST_WINDOW_CHANGE_MS.load(Ordering::Relaxed) == now {
                        let _ = app.emit("windows-changed", ());
                    }
                });
            }
        }
    });
}

/// ponytail: no global key hook for X11 clients short of grabbing keys from
/// the desktop, so media/volume keys stay with the DE (Bloom's HUD still
/// follows the volume) and Win+Number isn't replaced.
pub fn setup_keyboard_hook(_app: AppHandle) {}

/// ponytail: the desktop's own volume/brightness OSD can't be suppressed (see
/// `hide_native_osd`), so there is nothing to hide or restore.
pub fn set_native_osd_suppressed(_suppress: bool) {}

/// ponytail: no native panel to keep hidden (see `set_taskbar_visibility`).
pub fn setup_taskbar_hook() {}

/// ponytail: window previews need XComposite pixmaps; thumbnails stay empty.
pub fn setup_thumbnail_capture(_app: AppHandle) {}

/// ponytail: the visualizer needs a PipeWire/Pulse monitor capture; it stays idle.
pub fn setup_audio_visualization(_app: AppHandle) {}

/// The dock's Start button: a Super tap opens the desktop's launcher or
/// overview (GNOME Activities, Plasma's menu) as Win opens Start.
pub fn tap_super_key() {
    use x11rb::protocol::xtest::ConnectionExt as _;
    const SUPER_L: u32 = 0xffeb;
    let Some(x) = x11() else { return };
    let (min, max) = (x.conn.setup().min_keycode, x.conn.setup().max_keycode);
    let Some(map) = x
        .conn
        .get_keyboard_mapping(min, max - min + 1)
        .ok()
        .and_then(|c| c.reply().ok())
    else {
        return;
    };
    let per_key = map.keysyms_per_keycode.max(1) as usize;
    let Some(index) = map.keysyms.iter().position(|&k| k == SUPER_L) else { return };
    let keycode = min + (index / per_key) as u8;
    for (kind, hold) in [(xproto::KEY_PRESS_EVENT, 40), (xproto::KEY_RELEASE_EVENT, 0)] {
        let _ = x.conn.xtest_fake_input(kind, keycode, 0, x.root, 0, 0, 0);
        let _ = x.conn.flush();
        std::thread::sleep(Duration::from_millis(hold));
    }
}

// --- Overlap worker ---------------------------------------------------------

/// `dock-overlap`, `notch-overlap` and `dock-maximized` from the active
/// window's EWMH state, mirroring the Windows foreground-window worker.
fn overlap_loop(app: AppHandle, tx: Sender<SystemCommand>) {
    let Some(x) = x11() else { return };
    let a = &x.atoms;
    let mut last_visible = true;
    let mut last_dock_overlap: Option<bool> = None;
    let mut last_notch_overlap: Option<bool> = None;
    let mut last_dock_maximized: Option<bool> = None;
    let mut held_dock_span: Option<(i32, i32)> = None;
    let mut last_emit = Instant::now();
    let mut monitor: Option<(PhysicalPosition<i32>, PhysicalSize<u32>, f64)> = None;
    let mut monitor_checked: Option<Instant> = None;

    loop {
        std::thread::sleep(Duration::from_millis(150));
        if SHUTTING_DOWN.load(Ordering::Relaxed) {
            continue;
        }
        if monitor_checked.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
            if let Some(m) = app.primary_monitor().ok().flatten() {
                monitor = Some((*m.position(), *m.size(), m.scale_factor()));
                monitor_checked = Some(Instant::now());
            }
        }
        let Some((m_pos, m_size, scale)) = monitor else { continue };
        let (s_left, s_top) = (m_pos.x, m_pos.y);
        let (s_right, s_bottom) = (s_left + m_size.width as i32, s_top + m_size.height as i32);

        let dock_span = DOCK_RECT
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(|d| dock_span_px(d, scale));
        // Hold the span the dock had when the overlap began (see the Windows worker).
        let overlap_span = held_dock_span.or(dock_span);

        let mut should_overlap = false;
        let mut should_notch_overlap = false;
        let mut should_maximized = false;
        let mut is_fs = false;

        let win = effective_active(x);
        let states = if win != 0 { x.states(win) } else { Vec::new() };
        let types = if win != 0 { x.types(win) } else { Vec::new() };
        let is_shell = win == 0
            || states.contains(&a._NET_WM_STATE_HIDDEN)
            || types.contains(&a._NET_WM_WINDOW_TYPE_DESKTOP)
            || types.contains(&a._NET_WM_WINDOW_TYPE_DOCK);
        let rect = if is_shell { None } else { x.rect(win) };
        if let Some((left, top, width, height)) = rect {
            let (right, bottom) = (left + width, top + height);
            let maximized = states.contains(&a._NET_WM_STATE_MAXIMIZED_VERT)
                && states.contains(&a._NET_WM_STATE_MAXIMIZED_HORZ);
            let covers = left <= s_left && top <= s_top && right >= s_right && bottom >= s_bottom;
            is_fs = states.contains(&a._NET_WM_STATE_FULLSCREEN) || (covers && !maximized);
            if is_fs || maximized {
                should_overlap = true;
                should_notch_overlap = true;
                should_maximized = maximized && !is_fs;
            } else {
                if let Some(span) = overlap_span {
                    let trigger_y = s_bottom - (56.0 * scale) as i32;
                    should_overlap = window_reaches_dock(span, (left, right, bottom), trigger_y);
                }
                if let Some(nr) = NOTCH_RECT.lock().ok().and_then(|g| *g) {
                    let n_left = (nr.x as f64 * scale) as i32;
                    let n_right = n_left + (nr.width as f64 * scale) as i32;
                    let trigger_y = s_top + (36.0 * scale) as i32;
                    should_notch_overlap =
                        left < n_right - 4 && right > n_left + 4 && top < trigger_y - 4;
                }
            }
        }

        // Only report dock overlap while the dock is shown (see the Windows worker).
        let dock_visible = app
            .get_webview_window("dock")
            .is_some_and(|w| w.is_visible().unwrap_or(false));
        let effective_dock_overlap = should_overlap && dock_visible;
        held_dock_span = if effective_dock_overlap { overlap_span } else { None };

        CURRENT_DOCK_OVERLAP.store(effective_dock_overlap as i32, Ordering::Relaxed);
        CURRENT_NOTCH_OVERLAP.store(should_notch_overlap as i32, Ordering::Relaxed);
        CURRENT_FOREGROUND_FULLSCREEN.store(is_fs, Ordering::Relaxed);
        CURRENT_FOREGROUND_MAXIMIZED.store(should_maximized, Ordering::Relaxed);

        let refresh = last_emit.elapsed() >= Duration::from_secs(3);
        if refresh {
            last_emit = Instant::now();
        }
        if refresh || Some(effective_dock_overlap) != last_dock_overlap {
            let _ = app.emit("dock-overlap", effective_dock_overlap);
            last_dock_overlap = Some(effective_dock_overlap);
        }
        if refresh || Some(should_notch_overlap) != last_notch_overlap {
            let _ = app.emit("notch-overlap", should_notch_overlap);
            last_notch_overlap = Some(should_notch_overlap);
        }
        if refresh || Some(should_maximized) != last_dock_maximized {
            let _ = app.emit("dock-maximized", should_maximized);
            last_dock_maximized = Some(should_maximized);
        }
        if is_fs == last_visible {
            last_visible = !is_fs;
            let _ = tx.send(SystemCommand::ToggleVisibility(last_visible));
        }
    }
}

// --- Desktop entries --------------------------------------------------------

#[derive(Clone)]
struct DesktopEntry {
    /// Desktop file id without `.desktop` (`org.gnome.Nautilus`).
    id: String,
    path: String,
    name: String,
    exec: String,
    icon: String,
    wm_class: String,
    /// Lowercase file name of the program `Exec` runs.
    exec_bin: String,
}

fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| format!("{home}/.local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    std::iter::once(data_home.as_str())
        .chain(data_dirs.split(':'))
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Key/values of a desktop file's `[Desktop Entry]` group (unlocalized).
fn desktop_keys(path: &Path) -> Option<HashMap<String, String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut keys = HashMap::new();
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if let Some((key, value)) = line.split_once('=').filter(|_| in_entry) {
            keys.entry(key.trim().to_string())
                .or_insert_with(|| value.trim().to_string());
        }
    }
    Some(keys)
}

fn parse_desktop_file(path: &Path, id: String) -> Option<DesktopEntry> {
    let keys = desktop_keys(path)?;
    let get = |k: &str| keys.get(k).cloned().unwrap_or_default();
    if get("Type") != "Application" || get("NoDisplay") == "true" || get("Hidden") == "true" {
        return None;
    }
    let (name, exec) = (get("Name"), get("Exec"));
    if name.is_empty() || exec.is_empty() {
        return None;
    }
    let exec_bin = exec_args(&exec)
        .first()
        .and_then(|p| file_name(p))
        .unwrap_or_default()
        .to_lowercase();
    Some(DesktopEntry {
        id,
        path: path.to_string_lossy().into_owned(),
        name,
        exec,
        icon: get("Icon"),
        wm_class: get("StartupWMClass"),
        exec_bin,
    })
}

fn collect_desktop_files(
    dir: &Path,
    prefix: &str,
    seen: &mut HashSet<String>,
    out: &mut Vec<DesktopEntry>,
    depth: u32,
) {
    let Ok(items) = std::fs::read_dir(dir) else { return };
    for item in items.flatten() {
        let path = item.path();
        let file = item.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if depth < 3 {
                collect_desktop_files(&path, &format!("{prefix}{file}-"), seen, out, depth + 1);
            }
        } else if let Some(stem) = file.strip_suffix(".desktop") {
            let id = format!("{prefix}{stem}");
            // Earlier dirs win, so a user's (even hidden) entry overrides the system one.
            if seen.insert(id.clone()) {
                out.extend(parse_desktop_file(&path, id));
            }
        }
    }
}

fn scan_desktop_entries() -> Vec<DesktopEntry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for dir in data_dirs() {
        collect_desktop_files(&dir.join("applications"), "", &mut seen, &mut entries, 0);
    }
    entries.sort_by_key(|e| e.name.to_lowercase());
    entries
}

static DESKTOP_ENTRIES: OnceLock<Mutex<Vec<DesktopEntry>>> = OnceLock::new();

fn desktop_entries() -> MutexGuard<'static, Vec<DesktopEntry>> {
    DESKTOP_ENTRIES
        .get_or_init(|| Mutex::new(scan_desktop_entries()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn entry_app(entry: &DesktopEntry) -> AppInfo {
    AppInfo {
        name: entry.name.clone(),
        path: entry.path.clone(),
        icon: None,
        is_running: false,
        hwnd: None,
        executable: Some(entry.id.clone()),
        all_hwnds: None,
    }
}

pub fn trigger_app_scan() {
    if IS_SCANNING.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        let entries = scan_desktop_entries();
        let apps: Vec<AppInfo> = entries.iter().map(entry_app).collect();
        match DESKTOP_ENTRIES.get() {
            Some(cache) => *cache.lock().unwrap_or_else(|e| e.into_inner()) = entries,
            None => {
                let _ = DESKTOP_ENTRIES.set(Mutex::new(entries));
            }
        }
        if let Some(cache) = INSTALLED_APPS_CACHE.get() {
            if let Ok(mut lock) = cache.lock() {
                *lock = apps;
            }
        }
        IS_SCANNING.store(false, Ordering::Relaxed);
    });
}

/// First-run pins: the file manager, browser and terminal that are installed,
/// then Bloom's settings.
pub fn default_pinned_apps() -> Vec<AppInfo> {
    const CHOICES: [&[&str]; 3] = [
        &["org.gnome.Nautilus", "org.kde.dolphin", "thunar", "nemo"],
        &["firefox", "org.mozilla.firefox", "firefox_firefox", "chromium", "google-chrome"],
        &["org.gnome.Console", "org.gnome.Terminal", "org.kde.konsole", "xfce4-terminal"],
    ];
    let entries = desktop_entries();
    let mut apps: Vec<AppInfo> = CHOICES
        .iter()
        .filter_map(|ids| ids.iter().find_map(|id| entries.iter().find(|e| e.id == *id)))
        .map(entry_app)
        .collect();
    apps.push(AppInfo {
        name: "Settings".into(),
        path: "bloom-settings".into(),
        icon: None,
        is_running: false,
        hwnd: None,
        executable: None,
        all_hwnds: None,
    });
    apps
}

/// Splits a desktop entry `Exec` line into argv, dropping field codes (`%U`...)
/// and a leading `env VAR=value` wrapper.
fn exec_args(exec: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut started) = (false, false);
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => current.extend(chars.next()),
            c if c.is_whitespace() && !quoted => {
                if started || !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => current.push(c),
        }
    }
    if started || !current.is_empty() {
        args.push(current);
    }
    let mut args: Vec<String> = args
        .into_iter()
        .filter(|a| !(a.len() == 2 && a.starts_with('%') && a != "%%"))
        .map(|a| a.replace("%%", "%"))
        .collect();
    if args.first().is_some_and(|a| a == "env") {
        args.remove(0);
        while args.first().is_some_and(|a| a.contains('=')) {
            args.remove(0);
        }
    }
    args
}

/// Starts a program detached from Bloom: own process group, no inherited
/// stdio, reaped by a thread so it never lingers as a zombie.
fn spawn_detached<S: AsRef<OsStr>>(argv: &[S]) -> bool {
    let Some((program, args)) = argv.split_first() else { return false };
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    // GDK_BACKEND=x11 is for Bloom only (see prepare_process).
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        cmd.env_remove("GDK_BACKEND");
    }
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
            true
        }
        Err(_) => false,
    }
}

/// Runs a tool and returns its stdout when it exits successfully.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Opens a pinned path: a desktop file, a bare command or desktop id, an
/// executable, or any other file through `xdg-open`.
pub fn launch_path(path: &str) {
    let argv = if path.ends_with(".desktop") {
        desktop_keys(Path::new(path))
            .and_then(|keys| keys.get("Exec").map(|e| exec_args(e)))
            .unwrap_or_default()
    } else if !path.contains('/') {
        let lower = path.to_lowercase();
        let entry = desktop_entries()
            .iter()
            .find(|e| e.id.to_lowercase() == lower || e.exec_bin == lower)
            .map(|e| exec_args(&e.exec));
        entry.unwrap_or_else(|| vec![path.to_string()])
    } else if std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    {
        vec![path.to_string()]
    } else {
        vec!["xdg-open".to_string(), path.to_string()]
    };
    if !spawn_detached(argv.as_slice()) {
        eprintln!("Failed to open {path}");
    }
}

// --- Icons ----------------------------------------------------------------

/// Icon for a dock item: its desktop entry's themed icon, else the window's
/// own `_NET_WM_ICON`.
pub fn app_icon(path: &str, hwnd: Option<isize>) -> Option<String> {
    let icon_name = if path.ends_with(".desktop") {
        desktop_keys(Path::new(path)).and_then(|keys| keys.get("Icon").cloned())
    } else {
        let file = file_name(path).unwrap_or_default().to_lowercase();
        desktop_entries()
            .iter()
            .find(|e| e.id.to_lowercase() == file || e.exec_bin == file)
            .map(|e| e.icon.clone())
    };
    icon_name
        .filter(|n| !n.is_empty())
        .and_then(|n| themed_icon(&n))
        .or_else(|| window_icon(hwnd? as Window))
}

/// Resolves an icon name in the hicolor theme and pixmaps, largest PNG first,
/// then SVG.
/// ponytail: hicolor and pixmaps only; app icons install there, but themed
/// variants (Adwaita, breeze) and their index.theme layouts aren't searched.
fn themed_icon(name: &str) -> Option<String> {
    if name.starts_with('/') {
        return icon_file_to_data_uri(Path::new(name));
    }
    const SIZES: [&str; 8] = [
        "256x256", "512x512", "192x192", "128x128", "96x96", "64x64", "48x48", "scalable",
    ];
    let home = std::env::var("HOME").unwrap_or_default();
    let mut bases = vec![PathBuf::from(format!("{home}/.icons"))];
    bases.extend(data_dirs().into_iter().map(|d| d.join("icons")));
    for size in SIZES {
        for base in &bases {
            for ext in ["png", "svg"] {
                let file = base.join("hicolor").join(size).join("apps").join(format!("{name}.{ext}"));
                if file.is_file() {
                    return icon_file_to_data_uri(&file);
                }
            }
        }
    }
    ["png", "svg"]
        .iter()
        .map(|ext| PathBuf::from(format!("/usr/share/pixmaps/{name}.{ext}")))
        .find(|p| p.is_file())
        .and_then(|p| icon_file_to_data_uri(&p))
}

fn icon_file_to_data_uri(path: &Path) -> Option<String> {
    if path.extension().is_some_and(|e| e == "svg") {
        let bytes = std::fs::read(path).ok()?;
        return Some(format!(
            "data:image/svg+xml;base64,{}",
            general_purpose::STANDARD.encode(bytes)
        ));
    }
    image_file_to_base64(path.to_str()?)
}

/// The largest `_NET_WM_ICON` image up to 256 px, as a PNG data URI.
fn window_icon(win: Window) -> Option<String> {
    let x = x11()?;
    let data = x.prop32(win, x.atoms._NET_WM_ICON);
    // Entries are width, height, then width * height ARGB pixels.
    let mut best: Option<(usize, usize, usize)> = None;
    let mut i = 0;
    while i + 2 <= data.len() {
        let (w, h) = (data[i] as usize, data[i + 1] as usize);
        let Some(end) = w.checked_mul(h).map(|n| i + 2 + n) else { break };
        if w == 0 || h == 0 || end > data.len() {
            break;
        }
        if w <= 256 && best.is_none_or(|(bw, _, _)| w > bw) {
            best = Some((w, h, i + 2));
        }
        i = end;
    }
    let (w, h, start) = best?;
    let rgba: Vec<u8> = data[start..start + w * h]
        .iter()
        .flat_map(|p| {
            let [b, g, r, a] = p.to_le_bytes();
            [r, g, b, a]
        })
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        general_purpose::STANDARD.encode(png)
    ))
}

// --- Audio ------------------------------------------------------------------

/// Default output volume (0..1) and mute, from PipeWire's `wpctl` or
/// PulseAudio's `pactl`.
fn read_volume() -> Option<(f32, bool)> {
    if let Some(out) = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]) {
        return parse_wpctl_volume(&out);
    }
    let volume = run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?;
    let mute = run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"]).unwrap_or_default();
    Some((parse_percent(&volume)? / 100.0, mute.contains("yes")))
}

/// `Volume: 0.45` or `Volume: 0.45 [MUTED]`.
fn parse_wpctl_volume(out: &str) -> Option<(f32, bool)> {
    let volume = out.split_whitespace().nth(1)?.parse::<f32>().ok()?;
    Some((volume.min(1.0), out.contains("[MUTED]")))
}

/// First `NN%` in pactl output.
fn parse_percent(out: &str) -> Option<f32> {
    out.split_whitespace()
        .find_map(|t| t.strip_suffix('%')?.parse::<f32>().ok())
}

fn write_volume(volume: f32) {
    let volume = volume.clamp(0.0, 1.0);
    let wp = format!("{volume:.2}");
    if run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &wp]).is_none() {
        let pct = format!("{}%", (volume * 100.0).round());
        let _ = run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &pct]);
    }
    if volume > 0.0 && run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"]).is_none() {
        let _ = run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"]);
    }
}

/// Per-app streams from `pactl -f json list sink-inputs` (PulseAudio or
/// pipewire-pulse).
fn sink_inputs() -> Vec<serde_json::Value> {
    run("pactl", &["-f", "json", "list", "sink-inputs"])
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn sink_input_pid(input: &serde_json::Value) -> Option<u32> {
    input["properties"]["application.process.id"]
        .as_str()?
        .parse()
        .ok()
}

fn audio_sessions() -> Vec<AudioSessionInfo> {
    let own = std::process::id();
    let mut seen = HashSet::new();
    let mut found: Vec<(AudioSessionInfo, bool)> = sink_inputs()
        .iter()
        .filter_map(|input| {
            let pid = sink_input_pid(input)?;
            if pid == own || !seen.insert(pid) {
                return None;
            }
            let process_path = std::fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            let name = input["properties"]["application.name"]
                .as_str()
                .map(str::to_string)
                .or_else(|| process_path.as_deref().and_then(file_name))
                .unwrap_or_else(|| format!("Process {pid}"));
            let volume = input["volume"]
                .as_object()
                .and_then(|channels| channels.values().next())
                .and_then(|c| c["value_percent"].as_str())
                .and_then(parse_percent)
                .map_or(1.0, |p| p / 100.0);
            let info = AudioSessionInfo {
                pid,
                name,
                process_path,
                volume,
                is_muted: input["mute"].as_bool().unwrap_or(false),
            };
            Some((info, !input["corked"].as_bool().unwrap_or(false)))
        })
        .collect();
    found.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
    });
    found.into_iter().map(|(info, _)| info).collect()
}

/// Runs `pactl <command> <index> <value>` for every stream of `pid`.
fn for_app_streams(pid: u32, command: &str, value: &str) {
    for input in sink_inputs().iter().filter(|i| sink_input_pid(i) == Some(pid)) {
        if let Some(index) = input["index"].as_u64() {
            let _ = run("pactl", &[command, &index.to_string(), value]);
        }
    }
}

// --- Media (MPRIS) ----------------------------------------------------------

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

type Props = HashMap<String, OwnedValue>;

fn session_bus() -> Option<zbus::blocking::Connection> {
    static BUS: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
    BUS.get_or_init(|| zbus::blocking::Connection::session().ok())
        .clone()
}

fn mpris_players(bus: &zbus::blocking::Connection) -> Vec<String> {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(bus) else { return Vec::new() };
    dbus.list_names()
        .map(|names| {
            names
                .into_iter()
                .map(|n| n.to_string())
                .filter(|n| n.starts_with(MPRIS_PREFIX))
                .collect()
        })
        .unwrap_or_default()
}

fn player_props(bus: &zbus::blocking::Connection, name: &str) -> Option<Props> {
    let reply = bus
        .call_method(
            Some(name),
            MPRIS_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(PLAYER_IFACE,),
        )
        .ok()?;
    reply.body().deserialize::<Props>().ok()
}

fn prop<'a>(props: &'a Props, key: &str) -> Option<&'a Value<'static>> {
    props.get(key).map(|v| &**v)
}

/// Variant contents (`v` values in dicts arrive boxed).
fn unboxed<'a, 'b>(value: &'a Value<'b>) -> &'a Value<'b> {
    match value {
        Value::Value(inner) => inner,
        other => other,
    }
}

fn v_str<'a>(value: &'a Value<'_>) -> Option<&'a str> {
    match unboxed(value) {
        Value::Str(s) => Some(s.as_str()),
        Value::ObjectPath(p) => Some(p.as_str()),
        _ => None,
    }
}

fn v_i64(value: &Value<'_>) -> Option<i64> {
    match unboxed(value) {
        Value::I64(n) => Some(*n),
        Value::U64(n) => Some(*n as i64),
        Value::I32(n) => Some(*n as i64),
        Value::U32(n) => Some(*n as i64),
        _ => None,
    }
}

#[derive(Default)]
struct Track {
    title: String,
    artist: String,
    art_url: String,
    playing: bool,
    position_ms: i64,
    duration_ms: i64,
    seek: bool,
}

fn read_track(bus: &zbus::blocking::Connection, name: &str) -> Option<Track> {
    let props = player_props(bus, name)?;
    let mut track = Track {
        playing: prop(&props, "PlaybackStatus").and_then(v_str) == Some("Playing"),
        position_ms: prop(&props, "Position").and_then(v_i64).unwrap_or(0) / 1000,
        seek: matches!(prop(&props, "CanSeek").map(unboxed), Some(Value::Bool(true))),
        ..Default::default()
    };
    if let Some(Value::Dict(metadata)) = prop(&props, "Metadata").map(unboxed) {
        for (key, value) in metadata.iter() {
            match v_str(key) {
                Some("xesam:title") => track.title = v_str(value).unwrap_or_default().into(),
                Some("xesam:artist") => {
                    track.artist = match unboxed(value) {
                        Value::Array(names) => names
                            .iter()
                            .filter_map(v_str)
                            .collect::<Vec<_>>()
                            .join(", "),
                        other => v_str(other).unwrap_or_default().into(),
                    }
                }
                Some("mpris:length") => track.duration_ms = v_i64(value).unwrap_or(0) / 1000,
                Some("mpris:artUrl") => track.art_url = v_str(value).unwrap_or_default().into(),
                _ => {}
            }
        }
    }
    (!track.title.is_empty()).then_some(track)
}

/// The player media keys act on: the playing one, else the first.
fn active_player(bus: &zbus::blocking::Connection) -> Option<String> {
    let players = mpris_players(bus);
    let playing = players.iter().find(|name| {
        player_props(bus, name)
            .is_some_and(|p| prop(&p, "PlaybackStatus").and_then(v_str) == Some("Playing"))
    });
    playing.or(players.first()).cloned()
}

fn player_call<B>(bus: &zbus::blocking::Connection, method: &str, body: &B)
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    if let Some(name) = active_player(bus) {
        let _ = bus.call_method(Some(name.as_str()), MPRIS_PATH, Some(PLAYER_IFACE), method, body);
    }
}

/// Local (`file://`) cover art as a data URI.
/// ponytail: http(s) art (Spotify, some browsers) isn't downloaded.
fn artwork(url: &str) -> Option<String> {
    let path = percent_decode(url.strip_prefix("file://")?);
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some(format!(
        "data:{mime};base64,{}",
        general_purpose::STANDARD.encode(bytes)
    ))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read_media(bus: &zbus::blocking::Connection, last: Option<&MediaInfo>) -> MediaInfo {
    let tracks: Vec<Track> = mpris_players(bus)
        .iter()
        .filter_map(|name| read_track(bus, name))
        .collect();
    let Some(track) = tracks.iter().find(|t| t.playing).or(tracks.first()) else {
        return MediaInfo {
            title: String::new(),
            artist: String::new(),
            is_playing: false,
            has_media: false,
            artwork: None,
            position_ms: 0,
            duration_ms: 0,
            seek_enabled: false,
            position_updated_at: 0,
        };
    };
    let artwork = match last {
        Some(l) if l.title == track.title && l.artist == track.artist => l.artwork.clone(),
        _ => artwork(&track.art_url).map(|art| vec![art]),
    };
    MediaInfo {
        title: track.title.clone(),
        artist: track.artist.clone(),
        is_playing: track.playing,
        has_media: true,
        artwork,
        position_ms: track.position_ms,
        duration_ms: track.duration_ms,
        seek_enabled: track.seek,
        position_updated_at: get_now_ms() as u64,
    }
}

/// Focuses the window of the active player's process, else asks it to raise.
fn open_media_source() {
    let Some(bus) = session_bus() else { return };
    let Some(name) = active_player(&bus) else { return };
    let pid = zbus::names::BusName::try_from(name.as_str())
        .ok()
        .and_then(|bus_name| {
            let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).ok()?;
            dbus.get_connection_unix_process_id(bus_name).ok()
        });
    let window = x11().zip(pid).and_then(|(x, pid)| {
        x.client_list()
            .into_iter()
            .find(|&w| x.pid(w) == pid && is_taskbar_window(x, w))
            .map(|w| (x, w))
    });
    match window {
        Some((x, win)) => x.message(win, x.atoms._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0]),
        None => {
            let root = "org.mpris.MediaPlayer2";
            let _ = bus.call_method(Some(name.as_str()), MPRIS_PATH, Some(root), "Raise", &());
        }
    }
}

// --- System worker ----------------------------------------------------------

pub fn setup_system_worker(app: AppHandle) -> Sender<SystemCommand> {
    let (tx, rx) = channel::<SystemCommand>();
    let handle = app.clone();
    std::thread::spawn(move || media_volume_loop(handle, rx));
    let handle = app.clone();
    std::thread::spawn(move || brightness_loop(handle));
    let overlap_tx = tx.clone();
    std::thread::spawn(move || overlap_loop(app, overlap_tx));
    tx
}

/// Commands from the frontend plus volume (0.5 s) and media (2 s) polling.
/// ponytail: polled, not subscribed (`pactl subscribe`, MPRIS signals), so
/// external changes show up within those intervals.
fn media_volume_loop(app: AppHandle, rx: Receiver<SystemCommand>) {
    let bus = session_bus();
    let mut last_volume: Option<(f32, bool)> = None;
    let mut last_media: Option<MediaInfo> = None;
    let mut next_volume = Instant::now();
    let mut next_media = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(64)) {
            Ok(SystemCommand::SetVolume(volume)) => {
                write_volume(volume);
                next_volume = Instant::now();
            }
            Ok(SystemCommand::ToggleVisibility(visible)) => {
                let _ = app.emit("visibility-change", visible);
            }
            Ok(cmd) => {
                if let Some(bus) = &bus {
                    match cmd {
                        SystemCommand::MediaPlayPause => player_call(bus, "PlayPause", &()),
                        SystemCommand::MediaNext => player_call(bus, "Next", &()),
                        SystemCommand::MediaPrevious => player_call(bus, "Previous", &()),
                        SystemCommand::MediaSeek(position_ms) => {
                            // Seek is relative; SetPosition would need the track id.
                            let current = active_player(bus)
                                .and_then(|n| player_props(bus, &n))
                                .and_then(|p| prop(&p, "Position").and_then(v_i64))
                                .unwrap_or(0);
                            player_call(bus, "Seek", &(position_ms * 1000 - current,));
                        }
                        _ => {}
                    }
                    next_media = Instant::now() + Duration::from_millis(300);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }

        if Instant::now() >= next_volume {
            next_volume = Instant::now() + Duration::from_millis(500);
            if let Some((volume, is_muted)) = read_volume() {
                if last_volume.is_none_or(|(v, m)| (v - volume).abs() > 0.001 || m != is_muted) {
                    last_volume = Some((volume, is_muted));
                    CURRENT_VOLUME.store((volume * 100.0) as u32, Ordering::Relaxed);
                    let event = crate::types::VolumeChangeEvent { volume, is_muted };
                    let _ = app.emit("volume-change", event);
                }
            }
        }

        if let Some(bus) = bus.as_ref().filter(|_| Instant::now() >= next_media) {
            next_media = Instant::now() + Duration::from_secs(2);
            let info = read_media(bus, last_media.as_ref());
            let changed = last_media.as_ref().is_none_or(|l| {
                l.title != info.title
                    || l.artist != info.artist
                    || l.is_playing != info.is_playing
                    || l.has_media != info.has_media
                    || l.artwork != info.artwork
                    || (l.position_ms - info.position_ms).abs() > 1000
            });
            if changed {
                let _ = app.emit("media-update", info.clone());
                ANY_MEDIA_PLAYING.store(info.is_playing, Ordering::Relaxed);
                last_media = Some(info);
            }
        }
    }
}

// --- Brightness -------------------------------------------------------------

/// First `/sys/class/backlight` device (the laptop panel).
/// ponytail: external monitors (DDC/CI through ddcutil) aren't driven.
fn backlight_dir() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

fn read_sys_u32(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn read_brightness() -> Option<u32> {
    let dir = backlight_dir()?;
    let max = read_sys_u32(&dir.join("max_brightness"))?.max(1);
    let value = read_sys_u32(&dir.join("brightness"))?;
    Some(((value as u64 * 100 + max as u64 / 2) / max as u64) as u32)
}

/// `brightnessctl` (works through logind for the active user), else a direct
/// sysfs write, which needs a udev rule or the video group.
fn write_brightness(percent: u32) {
    if run("brightnessctl", &["-q", "set", &format!("{percent}%")]).is_some() {
        return;
    }
    if let Some(dir) = backlight_dir() {
        if let Some(max) = read_sys_u32(&dir.join("max_brightness")) {
            let value = (max as u64 * percent as u64 / 100).to_string();
            let _ = std::fs::write(dir.join("brightness"), value);
        }
    }
}

pub fn setup_brightness_worker() {
    let (tx, rx) = channel::<u32>();
    let _ = BRIGHTNESS_SENDER.set(tx);
    std::thread::spawn(move || {
        while let Ok(percent) = rx.recv() {
            // Slider drags queue many values; only the newest matters.
            let percent = rx.try_iter().last().unwrap_or(percent);
            write_brightness(percent.min(100));
        }
    });
}

fn brightness_loop(app: AppHandle) {
    let Some(mut last) = read_brightness() else { return };
    CURRENT_BRIGHTNESS.store(last, Ordering::Relaxed);
    loop {
        if get_now_ms() - LAST_BRIGHTNESS_CHANGE.load(Ordering::Relaxed) < 2000 {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        if let Some(brightness) = read_brightness().filter(|b| *b != last) {
            last = brightness;
            CURRENT_BRIGHTNESS.store(brightness, Ordering::Relaxed);
            let _ = app.emit("brightness-change", BrightnessChangeEvent { brightness });
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
}

// --- Power, radios, metrics -------------------------------------------------

/// Battery for webviews without `navigator.getBattery` (WebKitGTK).
#[derive(Clone, Serialize)]
pub struct BatteryInfo {
    /// 0.0 to 1.0, like `BatteryManager.level`.
    level: f64,
    charging: bool,
}

fn battery() -> Option<BatteryInfo> {
    for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
        let dir = entry.path();
        let read = |f: &str| {
            std::fs::read_to_string(dir.join(f))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        // Mice and headsets report scope=Device; the laptop battery doesn't.
        if read("type") != "Battery" || read("scope") == "Device" {
            continue;
        }
        let Ok(capacity) = read("capacity").parse::<f64>() else { continue };
        return Some(BatteryInfo {
            level: capacity / 100.0,
            charging: read("status") != "Discharging",
        });
    }
    None
}

/// power-profiles-daemon profile under the names the frontend uses.
fn power_mode() -> Option<&'static str> {
    match run("powerprofilesctl", &["get"])?.trim() {
        "performance" => Some("performance"),
        "balanced" => Some("balanced"),
        "power-saver" => Some("efficiency"),
        _ => None,
    }
}

/// Same cycle as Windows: performance, balanced, efficiency. Opens the power
/// settings when the daemon is missing or refuses the switch.
fn cycle_power() -> Option<&'static str> {
    let next = match power_mode() {
        Some("performance") => Some("balanced"),
        Some("balanced") => Some("power-saver"),
        Some(_) => Some("performance"),
        None => None,
    };
    if next.is_some_and(|n| run("powerprofilesctl", &["set", n]).is_some()) {
        return power_mode();
    }
    open_settings_panel("power", "kcm_powerdevilprofilesconfig");
    power_mode()
}

/// GNOME Settings or Plasma System Settings at a panel.
/// ponytail: other desktops have no common settings app; nothing opens.
fn open_settings_panel(gnome: &str, kde: &str) {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if desktop.contains("KDE") {
        spawn_detached(&["systemsettings", kde]);
    } else {
        spawn_detached(&["gnome-control-center", gnome]);
    }
}

fn cpu_usage() -> u32 {
    static LAST: Mutex<(u64, u64)> = Mutex::new((0, 0));
    let stat = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    // cpu user nice system idle iowait irq softirq steal
    let fields: Vec<u64> = stat
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .skip(1)
        .take(8)
        .filter_map(|n| n.parse().ok())
        .collect();
    let idle = fields.get(3).copied().unwrap_or(0) + fields.get(4).copied().unwrap_or(0);
    let total: u64 = fields.iter().sum();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let (total_delta, idle_delta) = (total.saturating_sub(last.0), idle.saturating_sub(last.1));
    *last = (total, idle);
    if total_delta == 0 {
        return 0;
    }
    ((total_delta - idle_delta.min(total_delta)) * 100 / total_delta) as u32
}

fn ram_usage() -> Option<f32> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |key: &str| -> Option<f64> {
        info.lines()
            .find(|l| l.starts_with(key))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    let (total, available) = (field("MemTotal:")?, field("MemAvailable:")?);
    Some(((1.0 - available / total) * 100.0).round() as f32)
}

/// Free GiB on the home directory's filesystem (Windows reports C:).
#[allow(clippy::unnecessary_cast)]
fn disk_free_gib() -> Option<u64> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let path = std::ffi::CString::new(home).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64) / (1024 * 1024 * 1024))
}

/// (sent, received) bytes per second across all interfaces but loopback.
fn network_speed() -> (u64, u64) {
    static LAST: Mutex<Option<(Instant, u64, u64)>> = Mutex::new(None);
    let text = std::fs::read_to_string("/proc/net/dev").unwrap_or_default();
    let (rx, tx) = text
        .lines()
        .skip(2)
        .filter_map(|l| l.split_once(':'))
        .filter(|(name, _)| name.trim() != "lo")
        .fold((0u64, 0u64), |(rx, tx), (_, counters)| {
            let n: Vec<u64> = counters.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            (
                rx + n.first().copied().unwrap_or(0),
                tx + n.get(8).copied().unwrap_or(0),
            )
        });
    let now = Instant::now();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let rate = match *last {
        Some((then, last_rx, last_tx)) => {
            let ms = (now.duration_since(then).as_millis() as u64).max(1);
            (
                tx.saturating_sub(last_tx) * 1000 / ms,
                rx.saturating_sub(last_rx) * 1000 / ms,
            )
        }
        None => (0, 0),
    };
    *last = Some((now, rx, tx));
    rate
}

/// The desktop wallpaper from gsettings (GNOME, Cinnamon, Budgie), the dark
/// variant first when the desktop prefers dark.
/// ponytail: Plasma keeps it in its own config, which isn't read.
pub fn wallpaper_path() -> Option<String> {
    let scheme = run("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"]);
    let keys: &[&str] = if scheme.is_some_and(|s| s.contains("dark")) {
        &["picture-uri-dark", "picture-uri"]
    } else {
        &["picture-uri"]
    };
    keys.iter().find_map(|key| {
        let uri = run("gsettings", &["get", "org.gnome.desktop.background", *key])?;
        let uri = uri.trim().trim_matches('\'');
        let path = percent_decode(uri.strip_prefix("file://").unwrap_or(uri));
        (!path.is_empty()).then_some(path)
    })
}

// --- Single instance ------------------------------------------------------

static INSTANCE_LISTENER: OnceLock<UnixListener> = OnceLock::new();

fn instance_socket() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("bloom-{}.sock", unsafe { libc::getuid() }))
}

/// Process setup before Tauri starts. False when Bloom is already running:
/// connecting asks it to open its settings (the Windows named event).
pub fn prepare_process() -> bool {
    // Wayland lets no client place its own windows or read others', so Bloom
    // runs under XWayland whenever an X server is there.
    if std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
    let socket = instance_socket();
    if UnixStream::connect(&socket).is_ok() {
        return false;
    }
    let _ = std::fs::remove_file(&socket);
    if let Ok(listener) = UnixListener::bind(&socket) {
        let _ = INSTANCE_LISTENER.set(listener);
    }
    true
}

pub fn listen_second_instance(app: AppHandle) {
    let Some(listener) = INSTANCE_LISTENER.get() else { return };
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if stream.is_ok() {
                crate::commands::open_settings_window(app.clone());
            }
        }
    });
}

/// Frees the socket so a restarted Bloom can claim it while this one exits.
pub fn close_single_instance_handles() {
    let _ = std::fs::remove_file(instance_socket());
}

/// Linux bodies of the Tauri commands whose Windows versions are Win32-only:
/// same names and arguments, re-exported by `commands`.
pub mod cmds {
    use super::*;
    use crate::types::{VolumeChangeEvent, WifiStatus};

    #[tauri::command]
    pub async fn get_active_windows() -> Vec<AppInfo> {
        let apps = tauri::async_runtime::spawn_blocking(list_windows)
            .await
            .unwrap_or_default();
        crate::commands::group_windows(apps)
    }

    #[tauri::command]
    pub async fn focus_window(hwnd: isize) {
        let _ = tauri::async_runtime::spawn_blocking(move || toggle_window(hwnd as Window)).await;
    }

    #[tauri::command]
    pub async fn focus_app_windows(hwnds: Vec<isize>) {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let Some(x) = x11() else { return };
            let fg = effective_active(x) as isize;
            if let Some(win) = crate::commands::next_app_window(&hwnds, fg) {
                // Source 2, as in toggle_window; it also restores a minimized window.
                x.message(win as Window, x.atoms._NET_ACTIVE_WINDOW, [2, 0, 0, 0, 0]);
            }
        })
        .await;
    }

    #[tauri::command]
    pub async fn close_window(hwnd: isize) {
        if let Some(x) = x11() {
            x.message(hwnd as Window, x.atoms._NET_CLOSE_WINDOW, [0, 2, 0, 0, 0]);
        }
    }

    /// ponytail: previews need XComposite pixmaps; none are captured.
    #[tauri::command]
    #[allow(unused_variables)]
    pub async fn capture_window_thumbnail(
        hwnd: isize,
        max_width: u32,
        max_height: u32,
    ) -> Result<Option<(String, i64)>, String> {
        Ok(None)
    }

    /// ponytail: the desktop's own volume OSD can't be suppressed.
    #[tauri::command]
    pub fn hide_native_osd() {}

    #[tauri::command]
    pub fn open_wifi_settings() {
        open_settings_panel("wifi", "kcm_networkmanagement");
    }

    #[tauri::command]
    pub fn open_sound_settings() {
        open_settings_panel("sound", "kcm_pulseaudio");
    }

    #[tauri::command]
    pub fn open_bluetooth_settings() {
        open_settings_panel("bluetooth", "kcm_bluetooth");
    }

    #[tauri::command]
    pub fn open_airplane_mode_settings() {
        open_settings_panel("wifi", "kcm_networkmanagement");
    }

    /// ponytail: no common notification centre to open.
    #[tauri::command]
    pub fn open_notification_center() {}

    /// ponytail: tray icons live in the desktop's own panel.
    #[tauri::command]
    pub fn open_system_tray() {}

    #[tauri::command]
    pub async fn open_media_source_app() {
        let _ = tauri::async_runtime::spawn_blocking(open_media_source).await;
    }

    #[tauri::command]
    pub async fn get_volume_state() -> Result<VolumeChangeEvent, String> {
        let (volume, is_muted) = tauri::async_runtime::spawn_blocking(read_volume)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("No wpctl or pactl output")?;
        CURRENT_VOLUME.store((volume * 100.0) as u32, Ordering::Relaxed);
        Ok(VolumeChangeEvent { volume, is_muted })
    }

    #[tauri::command]
    pub async fn get_audio_sessions() -> Result<Vec<AudioSessionInfo>, String> {
        tauri::async_runtime::spawn_blocking(audio_sessions)
            .await
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn set_app_volume(pid: u32, volume: f32) -> Result<(), String> {
        let volume = volume.clamp(0.0, 1.0);
        tauri::async_runtime::spawn_blocking(move || {
            let percent = format!("{}%", (volume * 100.0).round());
            for_app_streams(pid, "set-sink-input-volume", &percent);
            if volume > 0.0 {
                for_app_streams(pid, "set-sink-input-mute", "0");
            }
        })
        .await
        .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn set_app_mute(pid: u32, muted: bool) -> Result<(), String> {
        let value = if muted { "1" } else { "0" };
        tauri::async_runtime::spawn_blocking(move || {
            for_app_streams(pid, "set-sink-input-mute", value)
        })
        .await
        .map_err(|e| e.to_string())
    }

    /// Radio on, and whether a Wi-Fi device is associated (NetworkManager).
    #[tauri::command]
    pub async fn get_wifi_status() -> Result<WifiStatus, String> {
        tauri::async_runtime::spawn_blocking(|| {
            let enabled =
                run("nmcli", &["radio", "wifi"]).is_some_and(|s| s.trim() == "enabled");
            let connected = enabled
                && run("nmcli", &["-t", "-f", "TYPE,STATE", "device"])
                    .is_some_and(|s| s.lines().any(|l| l.starts_with("wifi:connected")));
            WifiStatus { enabled, connected }
        })
        .await
        .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn set_wifi_state(enabled: bool) -> Result<(), String> {
        let state = if enabled { "on" } else { "off" };
        tauri::async_runtime::spawn_blocking(move || run("nmcli", &["radio", "wifi", state]))
            .await
            .map_err(|e| e.to_string())?
            .map(|_| ())
            .ok_or_else(|| "nmcli failed".into())
    }

    #[tauri::command]
    pub async fn get_bluetooth_state() -> Result<bool, String> {
        tauri::async_runtime::spawn_blocking(|| {
            run("bluetoothctl", &["show"])
                .is_some_and(|s| s.lines().any(|l| l.trim() == "Powered: yes"))
        })
        .await
        .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn set_bluetooth_state(enabled: bool) -> Result<(), String> {
        tauri::async_runtime::spawn_blocking(move || {
            if enabled {
                // A soft-blocked adapter refuses to power on.
                let _ = run("rfkill", &["unblock", "bluetooth"]);
            }
            run("bluetoothctl", &["power", if enabled { "on" } else { "off" }])
        })
        .await
        .map_err(|e| e.to_string())?
        .map(|_| ())
        .ok_or_else(|| "bluetoothctl failed".into())
    }

    #[tauri::command]
    pub async fn get_power_mode() -> Result<Option<&'static str>, String> {
        tauri::async_runtime::spawn_blocking(power_mode)
            .await
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn cycle_power_mode() -> Result<Option<&'static str>, String> {
        tauri::async_runtime::spawn_blocking(cycle_power)
            .await
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn get_cpu_usage() -> Result<u32, String> {
        Ok(cpu_usage())
    }

    #[tauri::command]
    pub fn get_ram_usage() -> Result<f32, String> {
        ram_usage().ok_or_else(|| "Unreadable /proc/meminfo".into())
    }

    #[tauri::command]
    pub fn get_disk_space() -> Result<u64, String> {
        disk_free_gib().ok_or_else(|| "statvfs failed".into())
    }

    #[tauri::command]
    pub fn get_network_speed() -> Result<(u64, u64), String> {
        Ok(network_speed())
    }

    #[tauri::command]
    pub fn get_system_accent_color() -> Result<String, String> {
        crate::commands::adaptive_color().ok_or_else(|| "No wallpaper colour".into())
    }

    #[tauri::command]
    pub fn get_battery() -> Option<BatteryInfo> {
        battery()
    }

    /// Polls settings.json's mtime (the Windows build uses ReadDirectoryChangesW).
    pub fn setup_settings_watcher(app: AppHandle) {
        let Ok(dir) = app.path().app_config_dir() else { return };
        let path = dir.join("settings.json");
        let _ = SETTINGS_CACHE.set(Mutex::new(HashMap::new()));
        std::thread::spawn(move || {
            let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
            let mut last = modified(&path);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let current = modified(&path);
                if current != last {
                    last = current;
                    std::thread::sleep(Duration::from_millis(200));
                    crate::commands::reload_settings(&app, &path);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_lines_become_argv() {
        assert_eq!(exec_args("firefox %u"), ["firefox"]);
        assert_eq!(
            exec_args("env BAMF_DESKTOP_FILE_HINT=/x.desktop /snap/bin/code --new-window %F"),
            ["/snap/bin/code", "--new-window"]
        );
        assert_eq!(
            exec_args(r#""/opt/My App/app" --name "a \"b\"" 100%%"#),
            ["/opt/My App/app", "--name", "a \"b\"", "100%"]
        );
        assert_eq!(exec_args(r#"sh -c "" x"#), ["sh", "-c", "", "x"]);
    }

    #[test]
    fn tool_output_parsing() {
        assert_eq!(parse_wpctl_volume("Volume: 0.45\n"), Some((0.45, false)));
        assert_eq!(parse_wpctl_volume("Volume: 1.20 [MUTED]\n"), Some((1.0, true)));
        assert_eq!(
            parse_percent("Volume: front-left: 29491 /  45% / -20.81 dB,"),
            Some(45.0)
        );
        assert_eq!(percent_decode("/home/a/My%20Song.jpg"), "/home/a/My Song.jpg");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
