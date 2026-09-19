//! Shared Linux utilities needed by platform-neutral backend code.

use tauri::{AppHandle, Manager};

/// The user's UI scale, matching the Windows backend's setting of the same name.
pub fn get_bloom_scale(app: &AppHandle) -> f64 {
    get_setting_str(app, "bloom-scale")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(1.0)
}

/// Read a string-like setting without coupling update logic to a desktop API.
pub fn get_setting_str(app: &AppHandle, key: &str) -> Option<String> {
    let path = app.path().app_config_dir().ok()?.join("settings.json");
    let settings: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    settings.get(key).and_then(|value| match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}
