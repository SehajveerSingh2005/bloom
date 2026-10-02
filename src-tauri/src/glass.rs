//! Real glass behind Bloom's surfaces.
//!
//! A transparent WebView2 window can't blur what is behind it: CSS
//! `backdrop-filter` only sees the page itself, so a translucent dock is just
//! see-through. Frosting needs the compositor. Each glass surface (the dock,
//! the info panel, the notch) gets a small native "backdrop" window of its own,
//! placed exactly under it and directly beneath the Bloom window in z-order,
//! with DWM's blur-behind. The page above is untouched, so tooltips and menus
//! stay as they are; the CSS adds the tint and the light on top.
//!
//! What the compositor allows, found by testing on Windows 11:
//! - the window must have no redirection surface (WS_EX_NOREDIRECTIONBITMAP),
//!   or its own opaque black pixels cover the blur;
//! - the blur ignores window regions, so shapes can't be cut to measure; the
//!   window is instead rounded by DWM (8 px) and the page sends a box inset a
//!   little from its rounded edges, so those corners stay hidden inside the
//!   surface's larger curve;
//! - plain blur-behind (accent 3), not acrylic (4), which lags when moved.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;
use tauri::WebviewWindow;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SW_HIDE, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// One glass surface, in the page's CSS pixels, already inset from its
/// rounded edges (see useGlass).
#[derive(Deserialize)]
pub struct GlassRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Backdrop windows per Bloom window label (HWNDs as isize: `HWND` isn't Send).
static BACKDROPS: Mutex<Option<HashMap<String, Vec<isize>>>> = Mutex::new(None);

unsafe extern "system" fn backdrop_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    gradient: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionData {
    attrib: u32,
    data: *mut std::ffi::c_void,
    size: usize,
}

/// DWM blur-behind through the undocumented but long-stable
/// SetWindowCompositionAttribute (what TranslucentTB uses).
fn set_blur(hwnd: HWND) {
    type Swca = unsafe extern "system" fn(HWND, *mut CompositionData) -> i32;
    unsafe {
        let Ok(user32) = LoadLibraryW(w!("user32.dll")) else { return };
        let Some(f) = GetProcAddress(user32, windows::core::s!("SetWindowCompositionAttribute")) else {
            return;
        };
        let f: Swca = std::mem::transmute(f);
        let mut policy = AccentPolicy { state: 3, flags: 0, gradient: 0, animation: 0 };
        let mut data = CompositionData {
            attrib: 19, // WCA_ACCENT_POLICY
            data: &mut policy as *mut _ as *mut _,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        f(hwnd, &mut data);
    }
}

fn create_backdrop() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = w!("BloomGlassBackdrop");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(backdrop_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly once registered
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE
                | WS_EX_TRANSPARENT
                | WS_EX_TOPMOST,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round as *const _ as *const _,
            std::mem::size_of_val(&round) as u32,
        );
        let no_border: u32 = 0xFFFF_FFFE; // DWMWA_COLOR_NONE
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &no_border as *const _ as *const _,
            std::mem::size_of_val(&no_border) as u32,
        );
        set_blur(hwnd);
        Some(hwnd)
    }
}

/// Puts frosted glass under each of `rects` (none: no glass) of the calling
/// window.
#[tauri::command]
pub fn set_glass(window: WebviewWindow, rects: Vec<GlassRect>) {
    let Ok(owner) = window.hwnd() else { return };
    let mut guard = BACKDROPS.lock().unwrap();
    let backdrops = guard
        .get_or_insert_with(HashMap::new)
        .entry(window.label().to_string())
        .or_default();
    while backdrops.len() < rects.len() {
        let Some(h) = create_backdrop() else { return };
        backdrops.push(h.0 as isize);
    }

    let s = window.scale_factor().unwrap_or(1.0);
    let mut origin = POINT::default();
    unsafe {
        let _ = ClientToScreen(owner, &mut origin);
    }
    let px = |v: f64| (v * s).round() as i32;
    for (i, &h) in backdrops.iter().enumerate() {
        let hwnd = HWND(h as *mut _);
        unsafe {
            match rects.get(i) {
                // Directly beneath the Bloom window, exactly under its surface.
                Some(r) => {
                    let _ = SetWindowPos(
                        hwnd,
                        Some(owner),
                        origin.x + px(r.x),
                        origin.y + px(r.y),
                        px(r.w).max(1),
                        px(r.h).max(1),
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
                None => {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
    }
}
