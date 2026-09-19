//! X11 input handling for Bloom's oversized transparent webview windows.
//!
//! One pointer poll drives two jobs, so the X server is queried at a single
//! modest rate rather than by two independent loops:
//!
//! * pointer-region transparency, so the invisible parts of Bloom's webviews do
//!   not swallow clicks aimed at the desktop underneath
//! * screen-edge hover, which the React layer turns into "reveal the dock", "pop
//!   the notch down", and the volume/brightness on-screen displays
//!
//! The Windows backend gets both from a low-level mouse hook. X11 has no
//! equivalent that is not a keylogger-shaped global hook, so this polls the
//! pointer position instead. Only the pointer coordinate is read; no keystrokes,
//! no clicks, and no window contents.

use crate::{
    platform::linux::x11,
    state::{
        CURRENT_FOREGROUND_FULLSCREEN, DOCK_IS_HOVERED, DOCK_RECT, MENU_IS_OPEN, MENU_RECT,
        NOTCH_IS_HOVERED, NOTCH_RECT,
    },
    types::IntRect,
};
use std::{
    collections::HashMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};
use x11rb::protocol::xproto::ConnectionExt;

/// A GTK window only becomes safe to control once it has been realized, which
/// happens when it is first shown. Sending an input-shape request before that
/// point panics inside tao, so wait briefly after a window first appears.
const REALIZE_GRACE: Duration = Duration::from_millis(500);

/// Thickness of the hot border at each screen edge, matching the Windows hook.
const EDGE: i32 = 8;

/// How long an edge stays hot after the pointer leaves it. Keep an OSD or a
/// revealed dock from snapping shut while the pointer travels between the edge
/// and the panel it revealed.
const EDGE_LINGER: Duration = Duration::from_millis(500);

#[derive(Default)]
struct ClickThrough {
    /// When each window was first observed visible.
    realized: HashMap<&'static str, Instant>,
    /// Last applied value, so identical requests are not repeated 30x/second.
    applied: HashMap<&'static str, bool>,
}

/// A hot edge with a short linger, plus the last value reported to the frontend.
#[derive(Default)]
struct Latch {
    until: Option<Instant>,
    reported: Option<bool>,
}

impl Latch {
    /// `hot` is the instantaneous edge test. The result stays true for
    /// `EDGE_LINGER` after it drops, so an approach is not cancelled partway.
    fn value(&mut self, hot: bool, now: Instant) -> bool {
        if hot {
            self.until = Some(now + EDGE_LINGER);
        }
        match self.until {
            Some(until) if now < until => true,
            _ => {
                self.until = None;
                false
            }
        }
    }

    /// True only when the value differs from the last one reported.
    fn changed(&mut self, value: bool) -> bool {
        if self.reported == Some(value) {
            return false;
        }
        self.reported = Some(value);
        true
    }
}

#[derive(Default)]
struct Edges {
    dock: Latch,
    notch: Latch,
    volume: Latch,
    brightness: Latch,
}

fn visible(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label)
        .is_some_and(|window| window.is_visible().unwrap_or(false))
}

/// Pointer coordinates are reported in the same space as the monitor geometry,
/// so the edge tests compare directly.
fn on_monitor(pointer: (i32, i32), monitor: (i32, i32, i32, i32)) -> bool {
    let (x, y) = pointer;
    let (mx, my, mw, mh) = monitor;
    x >= mx && x <= mx + mw && y >= my && y <= my + mh
}

/// The primary monitor's bounds. X11 reports the pointer over the whole root
/// window, so this is what keeps the hot borders on the monitor Bloom lives on
/// rather than on the outer edge of a multi-monitor desktop.
fn monitor_bounds(app: &AppHandle) -> Option<(i32, i32, i32, i32)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let position = *monitor.position();
    let size = *monitor.size();
    Some((
        position.x,
        position.y,
        size.width as i32,
        size.height as i32,
    ))
}

/// Report each edge transition the frontend listens for.
fn apply_edges(app: &AppHandle, edges: &mut Edges, pointer: (i32, i32), monitor: (i32, i32, i32, i32)) {
    let (mx, my, mw, mh) = monitor;
    let (x, y) = pointer;
    let on_screen = on_monitor(pointer, monitor);
    let fullscreen = CURRENT_FOREGROUND_FULLSCREEN.load(Ordering::Relaxed);
    let now = Instant::now();

    // Dock: the bottom edge, or the React layer telling us the pointer is over
    // the dock itself (which keeps it revealed once the pointer moves onto it).
    if visible(app, "dock") {
        let hot = on_screen
            && (y >= my + mh - EDGE || DOCK_IS_HOVERED.load(Ordering::Relaxed));
        let value = edges.dock.value(hot, now);
        if edges.dock.changed(value) {
            let _ = app.emit("dock-edge-hover", value);
        }
    }

    // Notch: the top edge. Suppressed entirely while a window is fullscreen, so
    // the notch does not intrude over a fullscreen video.
    if visible(app, "main") {
        let hot = !fullscreen
            && on_screen
            && (y <= my + EDGE || NOTCH_IS_HOVERED.load(Ordering::Relaxed));
        let value = edges.notch.value(hot, now);
        if edges.notch.changed(value) {
            let _ = app.emit("notch-edge-hover", value);
        }
    }

    // Left edge reveals the volume display, right edge the brightness display.
    // The dock needs the full coordinate, so `on_screen` covers the y bounds.
    let volume_hot = !fullscreen
        && on_screen
        && y >= my
        && y <= my + mh
        && x <= mx + EDGE;
    let volume = edges.volume.value(volume_hot, now);
    if edges.volume.changed(volume) {
        let _ = app.emit("volume-edge-hover", volume);
    }

    let brightness_hot = !fullscreen
        && on_screen
        && y >= my
        && y <= my + mh
        && x >= mx + mw - EDGE;
    let brightness = edges.brightness.value(brightness_hot, now);
    if edges.brightness.changed(brightness) {
        let _ = app.emit("brightness-edge-hover", brightness);
    }
}

fn contains(window: &WebviewWindow, rect: IntRect, cursor: (i32, i32)) -> bool {
    let Ok(position) = window.outer_position() else {
        return false;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    // `outer_position` and the pointer are both in physical pixels, while the
    // rect arrives from the frontend in CSS pixels.
    let position: PhysicalPosition<i32> = position;
    let x = position.x + (rect.x as f64 * scale) as i32 - 8;
    let y = position.y + (rect.y as f64 * scale) as i32 - 8;
    let width = (rect.width as f64 * scale) as i32 + 16;
    let height = (rect.height as f64 * scale) as i32 + 16;
    cursor.0 >= x && cursor.0 <= x + width && cursor.1 >= y && cursor.1 <= y + height
}

/// Apply the pointer-region rule for one window. `control` is the only region
/// that accepts input while `edge` keeps the window interactive when the pointer
/// is on the screen edge those controls sit against; `None` for `control` means
/// the window is always click-through.
fn apply_region(
    state: &mut ClickThrough,
    label: &'static str,
    window: &WebviewWindow,
    control: Option<IntRect>,
    edge: bool,
    cursor: (i32, i32),
) {
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let first_seen = *state.realized.entry(label).or_insert_with(Instant::now);
    if first_seen.elapsed() < REALIZE_GRACE {
        return;
    }

    let interactive = edge
        || control.is_some_and(|rect| contains(window, rect, cursor))
        // A menu or popup reported by the frontend keeps the window accepting
        // input across the whole panel, not just the dock pill's rect.
        || (MENU_IS_OPEN.load(Ordering::Relaxed)
            && MENU_RECT
                .lock()
                .ok()
                .and_then(|rect| *rect)
                .is_some_and(|rect| contains(window, rect, cursor)));
    let ignore = !interactive;
    if state.applied.get(label) == Some(&ignore) {
        return;
    }
    if window.set_ignore_cursor_events(ignore).is_ok() {
        state.applied.insert(label, ignore);
    }
}

/// Starts the Linux/X11 equivalent of Bloom's Windows mouse-region hook and its
/// screen-edge hover reporting.
pub fn setup_pointer(app: AppHandle) {
    std::thread::spawn(move || {
        let Ok(session) = x11::session() else {
            eprintln!("Bloom pointer handling is unavailable: no X11 session");
            return;
        };
        let mut click = ClickThrough::default();
        let mut edges = Edges::default();
        loop {
            let pointer = {
                let Ok(connection) = session.connection_guard() else {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                };
                connection
                    .query_pointer(session.root)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .map(|reply| (i32::from(reply.root_x), i32::from(reply.root_y)))
            };

            if let Some(cursor) = pointer {
                let monitor = monitor_bounds(&app);
                if let Some(monitor) = monitor {
                    apply_edges(&app, &mut edges, cursor, monitor);
                }

                // An approach from a hot edge keeps that window interactive, so
                // the dock or notch does not flicker click-through at the
                // boundary on the way in.
                let at_bottom = monitor.is_some_and(|(_, my, _, mh)| cursor.1 >= my + mh - EDGE);
                let at_top = monitor.is_some_and(|(_, my, _, _)| cursor.1 <= my + EDGE);

                let dock_rect = DOCK_RECT.lock().ok().and_then(|rect| *rect);
                let notch_rect = NOTCH_RECT.lock().ok().and_then(|rect| *rect);
                if let Some(window) = app.get_webview_window("dock") {
                    apply_region(&mut click, "dock", &window, dock_rect, at_bottom, cursor);
                }
                if let Some(window) = app.get_webview_window("main") {
                    apply_region(&mut click, "main", &window, notch_rect, at_top, cursor);
                }
                if let Some(window) = app.get_webview_window("overlay") {
                    // The overlay is a pure on-screen display, so it never
                    // accepts pointer input once it is realized.
                    apply_region(&mut click, "overlay", &window, None, false, cursor);
                }
            }

            std::thread::sleep(Duration::from_millis(33));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latch_lingers_after_the_pointer_leaves() {
        let mut latch = Latch::default();
        let start = Instant::now();
        assert!(latch.value(true, start));
        // Still hot just after the pointer leaves, so an OSD does not snap shut.
        assert!(latch.value(false, start + Duration::from_millis(100)));
        // Cold once the linger has expired.
        assert!(!latch.value(false, start + Duration::from_millis(600)));
    }

    #[test]
    fn latch_reports_only_transitions() {
        let mut latch = Latch::default();
        assert!(latch.changed(true));
        assert!(!latch.changed(true));
        assert!(latch.changed(false));
        assert!(!latch.changed(false));
        assert!(latch.changed(true));
    }

    #[test]
    fn edge_tests_stay_on_the_primary_monitor() {
        let monitor = (0, 0, 1920, 1080);
        assert!(on_monitor((0, 1079), monitor));
        // A monitor to the left of the primary must not trip the edge tests.
        assert!(!on_monitor((-200, 500), monitor));
        assert!(!on_monitor((2000, 500), monitor));
        assert!(!on_monitor((500, 1200), monitor));
    }
}
