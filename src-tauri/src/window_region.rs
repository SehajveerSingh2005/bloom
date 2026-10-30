//! Clips a Bloom window to the parts of the screen its UI actually draws on.
//!
//! Bloom's windows are large transparent rectangles (the notch window spans
//! the whole top band of the screen). Wherever such a window catches input,
//! it swallows clicks meant for the apps underneath, even where it draws
//! nothing. A window region (`SetWindowRgn`) limits both drawing and hit
//! testing to the given rectangles, so outside them the window doesn't exist
//! for input at all. Each webview reports the rectangles it needs, in its own
//! CSS pixels, whenever they change.

use tauri::Window;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
};

use crate::types::IntRect;

/// The rectangles in physical window pixels: scaled by the window's DPI scale,
/// rounded outwards so nothing drawn at a fractional edge is clipped, and with
/// empty ones dropped.
fn physical_rects(rects: &[IntRect], scale: f64) -> Vec<RECT> {
    rects
        .iter()
        .filter(|r| r.width > 0 && r.height > 0)
        .map(|r| RECT {
            left: (r.x as f64 * scale).floor() as i32,
            top: (r.y as f64 * scale).floor() as i32,
            right: ((r.x + r.width) as f64 * scale).ceil() as i32,
            bottom: ((r.y + r.height) as f64 * scale).ceil() as i32,
        })
        .collect()
}

/// Snaps physical rectangle edges that align with the window's viewport bounds to
/// the exact HWND client area edges.
///
/// Under fractional DPI scaling (e.g. 125%, 150%, 175%), WebView2's internal
/// viewport dimension (`Math.floor(client / scale)`) scaled back to physical pixels
/// can fall short of the true HWND client bounds by up to `ceil(scale)` pixels.
/// Snapping edges that lie within this subpixel tolerance ensures GDI region clipping
/// does not truncate borders or bottom-aligned dock bands.
pub(crate) fn snap_rect_to_client(mut rect: RECT, client: RECT, scale: f64) -> RECT {
    if client.right <= 0 || client.bottom <= 0 {
        return rect;
    }
    let tol = (scale.ceil() as i32 + 1).max(2);
    if rect.left <= tol {
        rect.left = 0;
    }
    if rect.top <= tol {
        rect.top = 0;
    }
    if rect.right >= client.right - tol {
        rect.right = client.right;
    }
    if rect.bottom >= client.bottom - tol {
        rect.bottom = client.bottom;
    }
    rect
}

/// Sets the calling window's region to the union of `rects` (CSS pixels of
/// the window). An empty list leaves the window with an empty region: it draws
/// nothing and takes no input until the next call.
#[tauri::command]
pub fn set_window_region(window: Window, rects: Vec<IntRect>) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    // A region on a window that isn't layered turns its transparent pixels
    // opaque (see set_click_through). Make sure it's layered before the region
    // lands, keeping whatever click-through state it has.
    let click_through = unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TRANSPARENT,
        };
        GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TRANSPARENT.0 != 0
    };
    crate::utils::set_click_through(hwnd, click_through);
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
        let mut client = RECT::default();
        let has_client =
            GetClientRect(hwnd, &mut client).is_ok() && client.right > 0 && client.bottom > 0;

        let region = CreateRectRgn(0, 0, 0, 0);
        for mut rect in physical_rects(&rects, scale) {
            if has_client {
                rect = snap_rect_to_client(rect, client, scale);
            }
            let part = CreateRectRgn(rect.left, rect.top, rect.right, rect.bottom);
            CombineRgn(Some(region), Some(region), Some(part), RGN_OR);
            let _ = DeleteObject(part.into());
        }
        // The system owns the region from here on, so it isn't deleted on
        // success. Redraw so the newly uncovered parts paint.
        if SetWindowRgn(hwnd, Some(region), true) == 0 {
            let _ = DeleteObject(region.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> IntRect {
        IntRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn scales_and_rounds_outwards() {
        let scaled = physical_rects(&[rect(10, 0, 101, 45)], 1.25);
        assert_eq!(
            scaled,
            vec![RECT {
                left: 12,
                top: 0,
                right: 139,
                bottom: 57
            }]
        );
    }

    #[test]
    fn drops_empty_rectangles() {
        let scaled = physical_rects(&[rect(0, -100, 140, 0), rect(5, 5, 0, 10)], 2.0);
        assert!(scaled.is_empty());
    }

    #[test]
    fn snaps_near_boundary_edges_to_client() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        // Scale 1.5 -> tol = ceil(1.5) + 1 = 3.
        // A bottom-aligned dock with bottom = 1078 (2px gap from subpixel rounding)
        let rect = RECT {
            left: 1,
            top: 1000,
            right: 1918,
            bottom: 1078,
        };
        let snapped = snap_rect_to_client(rect, client, 1.5);
        assert_eq!(
            snapped,
            RECT {
                left: 0,
                top: 1000,
                right: 1920,
                bottom: 1080,
            }
        );
    }

    #[test]
    fn leaves_interior_rectangles_unmodified() {
        let client = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        // A context menu or element 10px inside edges should not snap
        let rect = RECT {
            left: 10,
            top: 50,
            right: 500,
            bottom: 400,
        };
        let snapped = snap_rect_to_client(rect, client, 1.5);
        assert_eq!(snapped, rect);
    }
}
