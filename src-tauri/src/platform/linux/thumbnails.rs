//! Window thumbnails captured from the X server.
//!
//! With a compositing window manager (Muffin on Cinnamon) every window's
//! contents are kept in offscreen storage, so `GetImage` on the window drawable
//! returns that window's own pixels rather than whatever happens to be on screen
//! above it. That keeps this a purely read-only capture: Bloom never redirects,
//! restyles or otherwise mutates another client's window, so a failure here can
//! never leave the desktop in a broken state.

use crate::{
    platform::linux::{now_ms, x11},
    state::{FOCUS_TIMESTAMPS, THUMBNAIL_CACHE},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{collections::HashMap, sync::Mutex};
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, Window};

/// Captures cost a round trip, a scale and a PNG encode, so a recent one is
/// reused while the pointer moves across the same dock item.
const CACHE_TTL_MS: i64 = 1500;

/// Refuse absurd captures before allocating for them.
const MAX_DIMENSION: u16 = 8_192;

struct Capture {
    width: u16,
    height: u16,
    depth: u8,
    pixels: Vec<u8>,
}

fn focus_time(window: Window) -> i64 {
    FOCUS_TIMESTAMPS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|times| times.get(&(window as isize)).copied())
        .unwrap_or(0)
}

fn cached(window: Window, now: i64) -> Option<String> {
    let cache = THUMBNAIL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().ok()?;
    match cache.get(&(window as isize)) {
        Some((image, stamp)) if now - stamp < CACHE_TTL_MS => Some(image.clone()),
        _ => {
            cache.remove(&(window as isize));
            None
        }
    }
}

fn store(window: Window, image: &str, now: i64) {
    let cache = THUMBNAIL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.insert(window as isize, (image.to_owned(), now));
    }
}

/// Drop cached thumbnails for windows that no longer exist.
pub fn forget_closed(windows: &[u32]) {
    let cache = THUMBNAIL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.retain(|id, _| windows.contains(&(*id as u32)));
    }
}

/// Read a window's geometry and pixels.
fn read_window(session: &x11::Session, window: Window) -> Result<Capture, String> {
    let connection = session.connection_guard()?;
    let geometry = connection
        .get_geometry(window)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| format!("Window geometry is unavailable: {e}"))?;
    if geometry.width == 0 || geometry.height == 0 {
        return Err("Window has no visible area".into());
    }
    if geometry.width > MAX_DIMENSION || geometry.height > MAX_DIMENSION {
        return Err("Window is too large to capture".into());
    }
    let reply = connection
        .get_image(
            ImageFormat::Z_PIXMAP,
            window,
            0,
            0,
            geometry.width,
            geometry.height,
            !0,
        )
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| format!("Window contents are unavailable: {e}"))?;
    Ok(Capture {
        width: geometry.width,
        height: geometry.height,
        depth: reply.depth,
        pixels: reply.data,
    })
}

/// Convert the X server's ZPixmap output into RGBA.
///
/// Scanlines are padded to four bytes, and the channel order is
/// little-endian BGRX for the depth used by ordinary desktop visuals.
fn to_rgba(capture: &Capture) -> Result<image::RgbaImage, String> {
    let bytes_per_pixel = match capture.depth {
        24 | 32 => 4usize,
        16 => 2usize,
        other => return Err(format!("Unsupported window depth {other}")),
    };
    let width = capture.width as usize;
    let height = capture.height as usize;
    let stride = (width * bytes_per_pixel + 3) & !3;
    let needed = stride
        .checked_mul(height)
        .ok_or("Window is too large to capture")?;
    if capture.pixels.len() < needed {
        return Err(format!(
            "Window pixel data is incomplete: {} of {needed} bytes",
            capture.pixels.len()
        ));
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    for row in 0..height {
        let row_start = row * stride;
        for column in 0..width {
            let offset = row_start + column * bytes_per_pixel;
            let pixel = &capture.pixels[offset..offset + bytes_per_pixel];
            let (red, green, blue, alpha) = if bytes_per_pixel == 4 {
                // Only depth 32 carries an alpha channel; at depth 24 the fourth
                // byte is unused padding and must not be read as transparency.
                let alpha = if capture.depth == 32 { pixel[3] } else { 255 };
                (pixel[2], pixel[1], pixel[0], alpha)
            } else {
                // 16-bit visuals are 5-6-5; expand by dropping the low bits.
                let value = u16::from_le_bytes([pixel[0], pixel[1]]);
                (
                    ((value >> 11) as u8 & 0x1f) << 3,
                    ((value >> 5) as u8 & 0x3f) << 2,
                    (value as u8 & 0x1f) << 3,
                    255,
                )
            };
            rgba.extend_from_slice(&[red, green, blue, alpha]);
        }
    }

    image::RgbaImage::from_raw(capture.width as u32, capture.height as u32, rgba)
        .ok_or_else(|| "Window image could not be assembled".to_string())
}

fn encode_png(image: &image::RgbaImage, max_width: u32, max_height: u32) -> Result<String, String> {
    let mut image = image.clone();
    if image.width() > max_width || image.height() > max_height {
        // `resize` preserves the aspect ratio and fits within the bounds.
        image = image::DynamicImage::ImageRgba8(image)
            .resize(
                max_width.max(1),
                max_height.max(1),
                image::imageops::FilterType::Triangle,
            )
            .into_rgba8();
    }
    let mut buffer = std::io::Cursor::new(Vec::new());
    image::write_buffer_with_format(
        &mut buffer,
        &image,
        image.width(),
        image.height(),
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| format!("Window image could not be encoded: {e}"))?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(buffer.into_inner())
    ))
}

/// Capture a window as a PNG data URI, paired with the time it was last focused
/// so the frontend can order previews most-recent-first.
pub fn capture(hwnd: isize, max_width: u32, max_height: u32) -> Result<Option<(String, i64)>, String> {
    let window = u32::try_from(hwnd).map_err(|_| "Invalid X11 window id".to_string())?;
    if window == 0 {
        return Err("Invalid X11 window id".into());
    }
    let focus = focus_time(window);
    let now = now_ms();
    if let Some(image) = cached(window, now) {
        return Ok(Some((image, focus)));
    }

    let session = x11::session()?;
    let capture = read_window(session, window)?;
    let rgba = to_rgba(&capture)?;
    let image = encode_png(&rgba, max_width, max_height)?;
    store(window, &image, now);
    Ok(Some((image, focus)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_with(depth: u8, width: u16, height: u16, pixels: Vec<u8>) -> Capture {
        Capture {
            width,
            height,
            depth,
            pixels,
        }
    }

    #[test]
    fn bgrx_pixels_become_rgba() {
        // Two pixels: blue-green-red ordering, with the padding byte ignored.
        let capture = capture_with(24, 2, 1, vec![0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00]);
        let image = to_rgba(&capture).expect("conversion should succeed");
        assert_eq!(image.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn scanline_padding_is_respected() {
        // A three pixel wide row is padded to 12 bytes, so the second row starts
        // at byte 12 and must not be read from byte 8.
        let mut pixels = vec![0u8; 24];
        pixels[0..4].copy_from_slice(&[0x10, 0x20, 0x30, 0x00]);
        pixels[12..16].copy_from_slice(&[0x40, 0x50, 0x60, 0x00]);
        let image = to_rgba(&capture_with(24, 3, 2, pixels)).expect("conversion should succeed");
        assert_eq!(image.get_pixel(0, 0).0, [0x30, 0x20, 0x10, 255]);
        assert_eq!(image.get_pixel(0, 1).0, [0x60, 0x50, 0x40, 255]);
    }

    #[test]
    fn rgb565_pixels_are_expanded() {
        // Pure green in 5-6-5 is 0x07E0. A single 16-bit pixel still occupies a
        // four byte scanline after the server's padding.
        let mut pixels = vec![0u8; 4];
        pixels[0..2].copy_from_slice(&0x07E0u16.to_le_bytes());
        let capture = capture_with(16, 1, 1, pixels);
        let image = to_rgba(&capture).expect("conversion should succeed");
        let [red, green, blue, _] = image.get_pixel(0, 0).0;
        assert_eq!((red, blue), (0, 0));
        assert_eq!(green, 0xfc);
    }

    #[test]
    fn depth_32_keeps_alpha_but_depth_24_stays_opaque() {
        let argb = capture_with(32, 1, 1, vec![0x00, 0x00, 0xff, 0x40]);
        assert_eq!(
            to_rgba(&argb).expect("conversion").get_pixel(0, 0).0,
            [255, 0, 0, 0x40]
        );
        // The same bytes at depth 24 must be treated as opaque: the fourth byte
        // is padding, not transparency.
        let rgb = capture_with(24, 1, 1, vec![0x00, 0x00, 0xff, 0x00]);
        assert_eq!(
            to_rgba(&rgb).expect("conversion").get_pixel(0, 0).0,
            [255, 0, 0, 255]
        );
    }

    #[test]
    fn unsupported_depths_and_short_buffers_are_rejected() {
        assert!(to_rgba(&capture_with(8, 1, 1, vec![0])).is_err());
        assert!(to_rgba(&capture_with(24, 2, 1, vec![0, 0, 0, 0])).is_err());
    }

    #[test]
    fn resizing_fits_within_the_requested_bounds() {
        let image = image::RgbaImage::from_pixel(400, 200, image::Rgba([1, 2, 3, 255]));
        let uri = encode_png(&image, 100, 100).expect("encode should succeed");
        assert!(uri.starts_with("data:image/png;base64,"));
        let bytes = STANDARD
            .decode(uri.trim_start_matches("data:image/png;base64,"))
            .expect("valid base64");
        let decoded = image::load_from_memory(&bytes).expect("valid png");
        assert!(decoded.width() <= 100 && decoded.height() <= 100);
        // Aspect ratio is preserved: 400x200 halves to 100x50.
        assert_eq!((decoded.width(), decoded.height()), (100, 50));
    }

    #[test]
    fn capturing_an_invalid_window_reports_an_error() {
        assert!(capture(-1, 320, 200).is_err());
        assert!(capture(0, 320, 200).is_err());
    }
}
