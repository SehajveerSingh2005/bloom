use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightness")]
#[serde(rename_all = "PascalCase")]
pub struct WmiMonitorBrightness {
    pub current_brightness: u8,
}

#[derive(Clone, Serialize)]
pub struct AudioVisualizationData {
    pub frequencies: Vec<f32>,
}

#[derive(Serialize, Clone)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub is_playing: bool,
    pub has_media: bool,
    pub artwork: Option<Vec<String>>,
}

pub enum SystemCommand {
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

#[derive(Clone, Copy, Deserialize, Debug)]
pub struct IntRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
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

#[derive(Clone, Serialize)]
pub struct BrightnessChangeEvent {
    pub brightness: u32,
}

#[derive(Clone, Serialize)]
pub struct VolumeChangeEvent {
    pub volume: f32,
    pub is_muted: bool,
}
