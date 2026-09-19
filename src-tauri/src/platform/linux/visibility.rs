//! Foreground-window awareness, so Bloom's dock and notch know when they are
//! covered.
//!
//! The dock tucks itself away when a window covers it, and the notch hides while
//! a fullscreen window is in front. Windows derives both from
//! `GetForegroundWindow` plus DWM frame bounds; on X11 the equivalent signals are
//! `_NET_ACTIVE_WINDOW`, `_NET_WM_STATE` and the window's root-relative
//! geometry.
//!
//! This reads window geometry and state only. No window contents are read and no
//! other application's state is modified.

use crate::{
    platform::linux::{windows_manager, x11::{self, Session}},
    state::{
        CURRENT_DOCK_OVERLAP, CURRENT_FOREGROUND_FULLSCREEN, CURRENT_NOTCH_OVERLAP, DOCK_RECT,
        NOTCH_RECT,
    },
    types::IntRect,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use x11rb::protocol::xproto::Window;

/// How tall the strip at the bottom of the screen is that a window has to reach
/// into before it counts as covering the dock, at 100% scale. The dock's own
/// on-screen rect supplies the horizontal span, so only the height is fixed here.
const DOCK_BAND: f64 = 56.0;
/// The same for the notch, at the top of the screen.
const NOTCH_BAND: f64 = 36.0;
/// Slack in pixels so a window edge resting exactly on a band does not flip the
/// dock in and out.
const PAD: i32 = 4;
/// Overlap flags are re-sent this often even when unchanged, matching the
/// Windows backend, so a frontend that missed an event still converges.
const RESYNC: Duration = Duration::from_secs(3);
/// Two small property reads per tick, so this stays cheap.
const TICK: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Rect {
    fn from_xywh(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + width as i32,
            bottom: y + height as i32,
        }
    }
}

/// The screen and the two regions Bloom places on it.
struct Regions {
    screen: Rect,
    dock: Option<Rect>,
    notch: Option<Rect>,
    dock_band: i32,
    notch_band: i32,
}

/// Whether `window` reaches into the dock's band at the bottom of the screen.
fn covers_dock(window: Rect, dock: Rect, screen: Rect, band: i32) -> bool {
    window.left < dock.right - PAD
        && window.right > dock.left + PAD
        && window.bottom > screen.bottom - band + PAD
}

/// Whether `window` reaches into the notch's band at the top of the screen.
fn covers_notch(window: Rect, notch: Rect, screen: Rect, band: i32) -> bool {
    window.left < notch.right - PAD
        && window.right > notch.left + PAD
        && window.top < screen.top + band - PAD
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Flags {
    dock: bool,
    notch: bool,
    fullscreen: bool,
}

fn has_window_type(session: &Session, window: Window, name: &'static str) -> bool {
    let (Ok(types_atom), Ok(wanted)) = (
        session.atom("_NET_WM_WINDOW_TYPE"),
        session.atom(name),
    ) else {
        return false;
    };
    session.cardinals(window, types_atom).contains(&wanted)
}

/// Whether a window is really occupying screen space.
///
/// A minimized window stays `_NET_ACTIVE_WINDOW` and keeps the geometry it had
/// before it was hidden, so geometry alone would report it as still covering the
/// dock and Bloom would never come back on an empty desktop. EWMH also marks
/// minimized windows with `_NET_WM_STATE_HIDDEN`, so both are checked.
fn on_screen(session: &Session, window: Window) -> bool {
    session.is_viewable(window)
        && !windows_manager::has_atom_state(session, window, "_NET_WM_STATE_HIDDEN")
}

/// The window the dock and notch should react to.
///
/// Bloom's own windows are excluded, as are the desktop, panels and splash
/// screens: those are the shell, not something that should make Bloom retreat.
fn foreground(session: &Session) -> Option<Window> {
    let window = windows_manager::active_window(session);
    if window == 0 || window == session.root {
        return None;
    }
    if !on_screen(session, window) {
        return None;
    }
    if windows_manager::pid_of(session, window) == Some(std::process::id()) {
        return None;
    }
    if windows_manager::has_atom_state(session, window, "_NET_WM_STATE_SKIP_TASKBAR") {
        return None;
    }
    for kind in [
        "_NET_WM_WINDOW_TYPE_DESKTOP",
        "_NET_WM_WINDOW_TYPE_DOCK",
        "_NET_WM_WINDOW_TYPE_SPLASH",
    ] {
        if has_window_type(session, window, kind) {
            return None;
        }
    }
    Some(window)
}

fn compute(session: &Session, regions: &Regions) -> Flags {
    let Some(window) = foreground(session) else {
        return Flags::default();
    };

    // EWMH states are authoritative, so a fullscreen window is recognised
    // directly rather than inferred from geometry the way the Windows backend
    // has to.
    let fullscreen = windows_manager::has_atom_state(session, window, "_NET_WM_STATE_FULLSCREEN");
    let maximized = windows_manager::has_atom_state(session, window, "_NET_WM_STATE_MAXIMIZED_HORZ")
        && windows_manager::has_atom_state(session, window, "_NET_WM_STATE_MAXIMIZED_VERT");
    if fullscreen || maximized {
        return Flags {
            dock: true,
            notch: true,
            fullscreen,
        };
    }

    let Some((x, y, width, height)) = session.root_geometry(window) else {
        return Flags::default();
    };
    let window = Rect::from_xywh(x, y, width, height);
    Flags {
        dock: regions
            .dock
            .is_some_and(|dock| covers_dock(window, dock, regions.screen, regions.dock_band)),
        notch: regions
            .notch
            .is_some_and(|notch| covers_notch(window, notch, regions.screen, regions.notch_band)),
        fullscreen: false,
    }
}

fn visible(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label)
        .is_some_and(|window| window.is_visible().unwrap_or(false))
}

/// A window that is on screen, in the physical pixel coordinates the X server
/// reports window geometry in.
fn monitor_rect(app: &AppHandle) -> Option<(Rect, f64)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let position = *monitor.position();
    let size = *monitor.size();
    Some((
        Rect::from_xywh(position.x, position.y, size.width, size.height),
        monitor.scale_factor(),
    ))
}

/// Where Bloom's dock or notch actually is on screen.
///
/// The frontend reports the visible pill's rect in CSS pixels relative to its
/// own window, so the window position and scale are applied to get screen
/// coordinates. Hidden windows have no meaningful region.
fn region_on_screen(
    app: &AppHandle,
    label: &str,
    reported: &Mutex<Option<IntRect>>,
) -> Option<Rect> {
    if !visible(app, label) {
        return None;
    }
    let window = app.get_webview_window(label)?;
    let reported = reported.lock().ok().and_then(|rect| *rect)?;
    let position = window.outer_position().ok()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    Some(Rect {
        left: position.x + (reported.x as f64 * scale) as i32,
        top: position.y + (reported.y as f64 * scale) as i32,
        right: position.x + ((reported.x + reported.width) as f64 * scale) as i32,
        bottom: position.y + ((reported.y + reported.height) as f64 * scale) as i32,
    })
}

/// Watch the foreground window and report when the dock or notch is covered.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let Ok(session) = x11::session() else {
            eprintln!("Bloom overlap tracking is unavailable: no X11 session");
            return;
        };
        let mut last_dock: Option<bool> = None;
        let mut last_notch: Option<bool> = None;
        let mut last_chrome: Option<bool> = None;
        let mut last_emit = Instant::now();
        let mut first_tick = true;

        loop {
            if let Some((screen, scale)) = monitor_rect(&app) {
                let regions = Regions {
                    screen,
                    dock: region_on_screen(&app, "dock", &DOCK_RECT),
                    notch: region_on_screen(&app, "main", &NOTCH_RECT),
                    dock_band: (DOCK_BAND * scale) as i32,
                    notch_band: (NOTCH_BAND * scale) as i32,
                };
                let flags = compute(&session, &regions);

                // On autostart the dock window is still hidden here, and
                // reporting an overlap then would immediately tuck away a dock
                // that has not been shown yet.
                let dock_overlap = flags.dock && visible(&app, "dock");

                CURRENT_DOCK_OVERLAP.store(i32::from(dock_overlap), std::sync::atomic::Ordering::Relaxed);
                CURRENT_NOTCH_OVERLAP.store(i32::from(flags.notch), std::sync::atomic::Ordering::Relaxed);
                CURRENT_FOREGROUND_FULLSCREEN.store(flags.fullscreen, std::sync::atomic::Ordering::Relaxed);

                let resync = last_emit.elapsed() >= RESYNC;
                // Always emit on first tick to ensure frontend gets initial state,
                // even if React listeners weren't registered when the thread started.
                if first_tick || Some(dock_overlap) != last_dock || resync {
                    let _ = app.emit("dock-overlap", dock_overlap);
                    last_dock = Some(dock_overlap);
                    last_emit = Instant::now();
                }
                if first_tick || Some(flags.notch) != last_notch || resync {
                    let _ = app.emit("notch-overlap", flags.notch);
                    last_notch = Some(flags.notch);
                }
                // Bloom's own chrome steps aside for fullscreen windows.
                let chrome = !flags.fullscreen;
                if Some(chrome) != last_chrome {
                    let _ = app.emit("visibility-change", chrome);
                    last_chrome = Some(chrome);
                }
            }

            std::thread::sleep(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    /// A centred dock pill such as the frontend reports.
    const DOCK: Rect = Rect {
        left: 700,
        top: 1020,
        right: 1220,
        bottom: 1060,
    };
    const NOTCH: Rect = Rect {
        left: 800,
        top: 0,
        right: 1120,
        bottom: 36,
    };

    #[test]
    fn a_window_reaching_the_bottom_hides_the_dock() {
        // Extends to the bottom of the screen and overlaps the dock's span.
        assert!(covers_dock(
            Rect::from_xywh(0, 100, 1920, 980),
            DOCK,
            SCREEN,
            DOCK_BAND as i32
        ));
    }

    #[test]
    fn a_window_stopping_above_the_band_leaves_the_dock_alone() {
        // Ends 200px short of the bottom edge.
        assert!(!covers_dock(
            Rect::from_xywh(0, 100, 1920, 780),
            DOCK,
            SCREEN,
            DOCK_BAND as i32
        ));
    }

    #[test]
    fn a_window_beside_the_dock_leaves_it_alone() {
        // Full height, but on the left of the dock's span.
        assert!(!covers_dock(
            Rect::from_xywh(0, 0, 400, 1080),
            DOCK,
            SCREEN,
            DOCK_BAND as i32
        ));
    }

    #[test]
    fn a_window_touching_the_top_hides_the_notch() {
        assert!(covers_notch(
            Rect::from_xywh(0, 0, 1920, 200),
            NOTCH,
            SCREEN,
            NOTCH_BAND as i32
        ));
    }

    #[test]
    fn a_window_below_the_notch_band_leaves_it_alone() {
        assert!(!covers_notch(
            Rect::from_xywh(0, 100, 1920, 200),
            NOTCH,
            SCREEN,
            NOTCH_BAND as i32
        ));
    }

    #[test]
    fn a_small_window_in_the_middle_affects_nothing() {
        let window = Rect::from_xywh(800, 400, 320, 240);
        assert!(!covers_dock(window, DOCK, SCREEN, DOCK_BAND as i32));
        assert!(!covers_notch(window, NOTCH, SCREEN, NOTCH_BAND as i32));
    }

    #[test]
    fn a_window_still_counting_as_covered_is_the_dock_hiding() {
        // Guards the combination the watcher actually applies: a covering
        // window on the primary monitor means both surfaces retreat.
        let regions = Regions {
            screen: SCREEN,
            dock: Some(DOCK),
            notch: Some(NOTCH),
            dock_band: DOCK_BAND as i32,
            notch_band: NOTCH_BAND as i32,
        };
        let covering = Rect::from_xywh(0, 0, 1920, 1080);
        assert!(covers_dock(covering, regions.dock.unwrap(), regions.screen, regions.dock_band));
        assert!(covers_notch(covering, regions.notch.unwrap(), regions.screen, regions.notch_band));
    }

    /// Live check against the running desktop. A minimized window keeps
    /// `_NET_ACTIVE_WINDOW` and its last geometry, so without the on-screen test
    /// the dock would stay hidden on an empty desktop.
    ///
    /// Runs only on demand because it minimizes whichever window is in front:
    /// `cargo test --release -- --ignored minimized_window_stops_covering`
    #[test]
    #[ignore = "needs a live X11 session and minimizes the active window"]
    fn minimized_window_stops_covering() {
        const SOURCE_PAGER: u32 = 2;
        const ICONIC_STATE: u32 = 3;

        let Ok(session) = x11::session() else {
            eprintln!("skipping: no X11 session");
            return;
        };
        let window = windows_manager::active_window(session);
        if window == 0 || foreground(session) != Some(window) {
            eprintln!("skipping: no usable active window");
            return;
        }

        // Minimize through the window manager, then put it back the same way.
        let state = session.atom("WM_CHANGE_STATE").expect("WM_CHANGE_STATE atom");
        session
            .send_client_message_to_root(window, state, [ICONIC_STATE, 0, 0, 0, 0])
            .expect("iconify request");
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            foreground(session).is_none(),
            "a minimized window must not count as covering the dock"
        );

        let activate = session.atom("_NET_ACTIVE_WINDOW").expect("_NET_ACTIVE_WINDOW atom");
        session
            .send_client_message_to_root(window, activate, [SOURCE_PAGER, 0, 0, 0, 0])
            .expect("activate request");
        std::thread::sleep(Duration::from_millis(400));
    }
}
