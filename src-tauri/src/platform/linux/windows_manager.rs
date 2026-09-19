//! X11/EWMH window management.
//!
//! Linux Mint runs Muffin, an EWMH-compliant window manager, so a taskbar only
//! needs the standard root properties and client messages. Bloom reads exactly
//! the properties a taskbar legitimately needs and never synthesizes input.
//! Wayland is not covered by this module.

use crate::{
    platform::linux::{
        apps::EntryIndex,
        now_ms,
        thumbnails,
        x11::{self, Session},
    },
    state::FOCUS_TIMESTAMPS,
    types::AppInfo,
};
use std::{collections::HashMap, fs, sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter};
use x11rb::protocol::xproto::Window;

/// EWMH source indication meaning "pager/taskbar", which is what Bloom is.
const SOURCE_PAGER: u32 = 2;
/// ICCCM `IconicState`, used to request that a window is minimized.
const ICONIC_STATE: u32 = 3;

pub(super) fn pid_of(session: &Session, window: Window) -> Option<u32> {
    let atom = session.atom("_NET_WM_PID").ok()?;
    session.cardinals(window, atom).first().copied()
}

fn executable_of_pid(pid: u32) -> Option<String> {
    let path = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    // Deleted binaries are reported with a " (deleted)" suffix.
    let name = path.file_name()?.to_str()?.trim_end_matches(" (deleted)");
    (!name.is_empty()).then(|| name.to_owned())
}

/// `WM_CLASS` carries the instance and class names as two NUL-terminated
/// strings.
fn window_class(session: &Session, window: Window) -> (String, String) {
    let Ok(atom) = session.atom("WM_CLASS") else {
        return (String::new(), String::new());
    };
    let Some(value) = session.text(window, atom) else {
        return (String::new(), String::new());
    };
    let mut parts = value.split('\0');
    let instance = parts.next().unwrap_or_default().to_owned();
    let class = parts.next().unwrap_or_default().to_owned();
    if class.is_empty() {
        (String::new(), instance)
    } else {
        (instance, class)
    }
}

fn window_title(session: &Session, window: Window) -> String {
    if let Ok(atom) = session.atom("_NET_WM_NAME") {
        if let Some(title) = session.text(window, atom) {
            return title;
        }
    }
    session
        .atom("WM_NAME")
        .ok()
        .and_then(|atom| session.text(window, atom))
        .unwrap_or_default()
}

pub(super) fn has_atom_state(session: &Session, window: Window, name: &'static str) -> bool {
    let Ok(atom) = session.atom(name) else {
        return false;
    };
    let Ok(state_atom) = session.atom("_NET_WM_STATE") else {
        return false;
    };
    session.cardinals(window, state_atom).contains(&atom)
}

/// Only ordinary top-level application windows belong in a taskbar.
fn is_taskbar_window(session: &Session, window: Window, own_pid: u32) -> bool {
    if pid_of(session, window) == Some(own_pid) {
        return false;
    }
    if has_atom_state(session, window, "_NET_WM_STATE_SKIP_TASKBAR") {
        return false;
    }
    let (Ok(type_atom), Ok(window_type_atom)) = (
        session.atom("_NET_WM_WINDOW_TYPE"),
        session.atom("_NET_WM_WINDOW_TYPE_NORMAL"),
    ) else {
        return false;
    };
    let types = session.cardinals(window, type_atom);
    if types.is_empty() {
        // Windows that do not declare a type are treated as normal, matching
        // what other X11 taskbars do.
        return true;
    }
    let dialog = session
        .atom("_NET_WM_WINDOW_TYPE_DIALOG")
        .unwrap_or_default();
    types
        .iter()
        .any(|value| *value == window_type_atom || *value == dialog)
}

fn prettify(value: &str) -> String {
    let cleaned = value.replace(['-', '_', '.'], " ");
    let mut characters = cleaned.trim().chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

/// Enumerate the windows a taskbar should show, grouped per application.
pub fn list() -> Vec<AppInfo> {
    let Ok(session) = x11::session() else {
        return Vec::new();
    };
    let Ok(client_list) = session.atom("_NET_CLIENT_LIST") else {
        return Vec::new();
    };
    let index = EntryIndex::build();
    let own_pid = std::process::id();

    // Grouping mirrors the Windows backend: one dock item per application, with
    // every window of that application collected in `all_hwnds`.
    let mut grouped: HashMap<String, AppInfo> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for id in session.cardinals(session.root, client_list) {
        let window: Window = id;
        if !is_taskbar_window(session, window, own_pid) {
            continue;
        }
        let title = window_title(session, window);
        let (instance, class) = window_class(session, window);
        if title.is_empty() && class.is_empty() {
            continue;
        }
        let executable = pid_of(session, window).and_then(executable_of_pid);

        let (name, path) = match index.match_window(&class, &instance, executable.as_deref()) {
            Some(entry) => (entry.name.clone(), entry.path.clone()),
            None => {
                let display = executable
                    .as_deref()
                    .or((!class.is_empty()).then_some(class.as_str()))
                    .unwrap_or("Window");
                let path = if class.is_empty() {
                    format!("x11:{display}")
                } else {
                    format!("x11:{class}")
                };
                (prettify(display), path)
            }
        };

        let key = path.to_lowercase();
        let hwnd = window as isize;
        match grouped.get_mut(&key) {
            Some(existing) => {
                if let Some(hwnds) = existing.all_hwnds.as_mut() {
                    hwnds.push((hwnd, title));
                }
            }
            None => {
                order.push(key.clone());
                grouped.insert(
                    key,
                    AppInfo {
                        name,
                        path,
                        icon: None,
                        is_running: true,
                        hwnd: Some(hwnd),
                        executable,
                        all_hwnds: Some(vec![(hwnd, title)]),
                    },
                );
            }
        }
    }

    order
        .into_iter()
        .filter_map(|key| grouped.remove(&key))
        .collect()
}

pub(super) fn active_window(session: &Session) -> Window {
    session
        .atom("_NET_ACTIVE_WINDOW")
        .ok()
        .and_then(|atom| session.cardinals(session.root, atom).first().copied())
        .unwrap_or(0)
}

/// Focus a window, or minimize it when it is already the active one, matching
/// the toggle behaviour of Bloom's Windows taskbar.
pub fn focus(hwnd: isize) -> Result<(), String> {
    let session = x11::session()?;
    let window = u32::try_from(hwnd).map_err(|_| "Invalid X11 window id".to_string())?;
    if window == 0 {
        return Err("Invalid X11 window id".into());
    }

    if active_window(session) == window {
        let state_message = session.atom("WM_CHANGE_STATE")?;
        return session.send_client_message_to_root(window, state_message, [ICONIC_STATE, 0, 0, 0, 0]);
    }

    let activate_message = session.atom("_NET_ACTIVE_WINDOW")?;
    session.send_client_message_to_root(
        window,
        activate_message,
        [SOURCE_PAGER, 0, 0, 0, 0],
    )
}

/// Ask the window manager to close a window politely.
pub fn close(hwnd: isize) -> Result<(), String> {
    let session = x11::session()?;
    let window = u32::try_from(hwnd).map_err(|_| "Invalid X11 window id".to_string())?;
    if window == 0 {
        return Err("Invalid X11 window id".into());
    }
    let close_message = session.atom("_NET_CLOSE_WINDOW")?;
    session.send_client_message_to_root(window, close_message, [0, SOURCE_PAGER, 0, 0, 0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_names_are_prettified_for_display() {
        assert_eq!(prettify("google-chrome"), "Google chrome");
        assert_eq!(prettify("jetbrains_idea"), "Jetbrains idea");
        assert_eq!(prettify(""), "");
    }

    #[test]
    fn invalid_window_ids_are_rejected() {
        assert!(focus(-1).is_err());
        assert!(close(0).is_err());
    }

    /// Smoke test: enumeration must degrade to an empty list, not panic, when
    /// the environment has no usable X11 session.
    #[test]
    fn enumeration_never_panics() {
        for app in list() {
            assert!(!app.path.is_empty());
        }
    }
}

/// Current window ids, used for capability checks and diagnostics.
pub fn window_ids(session: &Session) -> Vec<u32> {
    session
        .atom("_NET_CLIENT_LIST")
        .ok()
        .map(|atom| session.cardinals(session.root, atom))
        .unwrap_or_default()
}

/// Remember when each window was last activated so dock previews can be ordered
/// most-recently-used first, and drop entries for windows that have closed.
fn record_focus(windows: &[u32], active: u32) {
    let times = FOCUS_TIMESTAMPS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut times) = times.lock() else {
        return;
    };
    times.retain(|window, _| windows.contains(&(*window as u32)));
    if active != 0 {
        times.insert(active as isize, now_ms());
    }
}

/// Watch the window list and active window, telling the frontend to refresh
/// only when something actually changed. Two property reads per tick keep this
/// far cheaper than re-enumerating windows on a timer.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let Ok(session) = x11::session() else {
            eprintln!("Bloom window tracking is unavailable: no X11 session");
            return;
        };
        let mut previous: Option<Vec<u32>> = None;
        loop {
            let windows = window_ids(session);
            let active = active_window(session);
            let mut signature = windows.clone();
            signature.push(active);
            if previous.as_deref() != Some(signature.as_slice()) {
                record_focus(&windows, active);
                // Closed windows must not keep their cached previews alive.
                thumbnails::forget_closed(&windows);
                previous = Some(signature);
                let _ = app.emit("windows-changed", ());
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    });
}

/// Activating and iconifying another client's window is the one part of window
/// management that cannot be checked without a live desktop, so it runs on
/// demand rather than in the normal suite: it briefly takes over focus.
///
/// `cargo test --release -- --ignored focus_and_minimize_toggle`
#[cfg(test)]
mod live_focus {
    use super::*;

    #[test]
    #[ignore = "takes over the desktop focus for a moment"]
    fn focus_and_minimize_toggle() {
        let Ok(session) = x11::session() else {
            eprintln!("skipping: no X11 session");
            return;
        };
        let original = active_window(session);
        // Only windows a taskbar shows can be activated: the window manager
        // refuses docks and the desktop however it is asked.
        let target = list()
            .iter()
            .filter_map(|app| app.hwnd)
            .map(|hwnd| hwnd as u32)
            .find(|id| *id != original)
            .unwrap_or(0);
        if target == 0 {
            eprintln!("skipping: no second activatable window on this desktop");
            return;
        }

        // Sending the message is not enough: the window has to actually become
        // active, which only happens when the message names the window.
        focus(target as isize).expect("activation should be sent");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            active_window(session),
            target,
            "focus should move to the window"
        );

        // Focusing the window that is already active is the dock's minimise
        // toggle, which is how a dock item removes a window from the screen.
        focus(target as isize).expect("state change should be sent");
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            has_atom_state(session, target, "_NET_WM_STATE_HIDDEN"),
            "the already-active window should be iconified"
        );

        if original != 0 {
            let _ = focus(original as isize);
        }
    }
}
