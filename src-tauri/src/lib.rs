// Empty lib.rs - all code is in main.rs

use std::sync::atomic::{AtomicU32, Ordering};
use tauri::{AppHandle, Emitter};

static CURRENT_VOLUME: AtomicU32 = AtomicU32::new(u32::MAX);

pub fn get_volume() -> f32 {
    let vol = CURRENT_VOLUME.load(Ordering::Relaxed);
    if vol == u32::MAX {
        -1.0
    } else {
        vol as f32 / 100.0
    }
}

pub fn set_volume(app_handle: &AppHandle, volume: f32) {
    let vol_percent = (volume * 100.0).round() as u32;
    let old_vol = CURRENT_VOLUME.swap(vol_percent, Ordering::Relaxed);
    
    if old_vol != vol_percent {
        let _ = app_handle.emit("volume-change", VolumeChangeEvent {
            volume,
            is_muted: volume == 0.0,
        });
    }
}

#[derive(Clone, serde::Serialize)]
pub struct VolumeChangeEvent {
    pub volume: f32,
    pub is_muted: bool,
}
