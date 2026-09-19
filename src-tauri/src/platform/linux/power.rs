//! Battery state via UPower over D-Bus.
//!
//! WebKitGTK does not implement the browser Battery Status API that the
//! frontend uses on Windows, so the Linux build reads UPower instead and the
//! frontend subscribes to `battery-change`.

use serde::Serialize;
use std::{sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedObjectPath,
};

const UPOWER_SERVICE: &str = "org.freedesktop.UPower";
const UPOWER_ROOT: &str = "/org/freedesktop/UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
const PROFILE_SERVICE: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILE_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// UPower's `Device.Type` value for a battery.
const DEVICE_TYPE_BATTERY: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BatteryState {
    /// Charge percentage, 0-100.
    pub level: u32,
    pub charging: bool,
    pub present: bool,
}

/// Map UPower's charge state to the "is plugged in and gaining charge" flag the
/// UI expects from the browser API.
fn is_charging(state: u32, on_battery: bool) -> bool {
    match state {
        1 | 4 | 5 => true,  // charging, fully charged, pending charge
        2 | 3 | 6 => false, // discharging, empty, pending discharge
        _ => !on_battery,
    }
}

fn percentage_level(percentage: f64) -> u32 {
    if !percentage.is_finite() {
        return 0;
    }
    percentage.round().clamp(0.0, 100.0) as u32
}

fn system_bus() -> Result<Connection, String> {
    Connection::system().map_err(|e| format!("Cannot reach the system bus: {e}"))
}

fn proxied<'a>(
    connection: &'a Connection,
    service: &'a str,
    path: &'a str,
    interface: &'a str,
) -> Result<Proxy<'a>, String> {
    Proxy::new(connection, service, path, interface).map_err(|e| e.to_string())
}

/// The composite battery UPower exposes for the whole system.
fn display_device_state(connection: &Connection, on_battery: bool) -> Result<BatteryState, String> {
    let device = proxied(connection, UPOWER_SERVICE, DISPLAY_DEVICE, DEVICE_INTERFACE)?;
    let present: bool = device
        .get_property("IsPresent")
        .map_err(|e| e.to_string())?;
    if !present {
        return Err("No battery is present".into());
    }
    let percentage: f64 = device
        .get_property("Percentage")
        .map_err(|e| e.to_string())?;
    let state: u32 = device.get_property("State").unwrap_or(0);
    Ok(BatteryState {
        level: percentage_level(percentage),
        charging: is_charging(state, on_battery),
        present: true,
    })
}

/// Fall back to enumerating real battery devices when the composite device does
/// not report a battery (some systems have no `DisplayDevice`).
fn enumerated_battery(connection: &Connection) -> Result<BatteryState, String> {
    let root = proxied(connection, UPOWER_SERVICE, UPOWER_ROOT, UPOWER_SERVICE)?;
    let devices: Vec<OwnedObjectPath> = root
        .call("EnumerateDevices", &())
        .map_err(|e| e.to_string())?;
    for device_path in devices {
        let Ok(device) = proxied(
            connection,
            UPOWER_SERVICE,
            device_path.as_str(),
            DEVICE_INTERFACE,
        ) else {
            continue;
        };
        let device_type: u32 = device.get_property("Type").unwrap_or(0);
        let present: bool = device.get_property("IsPresent").unwrap_or(false);
        if device_type != DEVICE_TYPE_BATTERY || !present {
            continue;
        }
        let percentage: f64 = device.get_property("Percentage").unwrap_or(0.0);
        let state: u32 = device.get_property("State").unwrap_or(0);
        return Ok(BatteryState {
            level: percentage_level(percentage),
            charging: is_charging(state, false),
            present: true,
        });
    }
    Err("No battery device is available".into())
}

fn on_battery(connection: &Connection) -> bool {
    proxied(connection, UPOWER_SERVICE, UPOWER_ROOT, UPOWER_SERVICE)
        .and_then(|root| root.get_property::<bool>("OnBattery").map_err(|e| e.to_string()))
        .unwrap_or(false)
}

pub fn state() -> Result<BatteryState, String> {
    let connection = system_bus()?;
    let on_battery = on_battery(&connection);
    display_device_state(&connection, on_battery).or_else(|_| enumerated_battery(&connection))
}

/// True when the desktop is in its power-saving profile.
pub fn power_saver_enabled() -> Result<bool, String> {
    let connection = system_bus()?;
    let proxy = proxied(
        &connection,
        PROFILE_SERVICE,
        PROFILE_PATH,
        PROFILE_SERVICE,
    )?;
    let profile: String = proxy
        .get_property("ActiveProfile")
        .map_err(|e| e.to_string())?;
    Ok(profile == "power-saver")
}

/// Battery charge changes slowly, so a low frequency poll is plenty.
pub fn start_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let last: Mutex<Option<BatteryState>> = Mutex::new(None);
        loop {
            if let Ok(current) = state() {
                let changed = last
                    .lock()
                    .map(|previous| *previous != Some(current))
                    .unwrap_or(false);
                if changed {
                    if let Ok(mut previous) = last.lock() {
                        *previous = Some(current);
                    }
                    let _ = app.emit("battery-change", current);
                }
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_state_prefers_the_reported_upower_state() {
        assert!(is_charging(1, true)); // charging
        assert!(is_charging(4, false)); // fully charged
        assert!(!is_charging(2, false)); // discharging
        assert!(!is_charging(3, false)); // empty
        // Unknown state falls back to whether the system is on battery.
        assert!(is_charging(0, false));
        assert!(!is_charging(0, true));
    }

    #[test]
    fn percentages_are_clamped_and_survive_bad_input() {
        assert_eq!(percentage_level(57.4), 57);
        assert_eq!(percentage_level(100.0), 100);
        assert_eq!(percentage_level(-5.0), 0);
        assert_eq!(percentage_level(f64::NAN), 0);
    }

    #[test]
    fn reading_battery_never_panics_without_upower() {
        // On a machine with no UPower this returns an error; either way it must
        // not panic and must not claim a battery exists.
        if let Ok(state) = state() {
            assert!(state.level <= 100);
            assert!(state.present);
        }
    }
}
