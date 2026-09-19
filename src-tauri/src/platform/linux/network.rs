//! Wi-Fi state through NetworkManager over the system bus.
//!
//! Only the software radio state the UI toggles is read, plus connection
//! settings launching. Toggling needs the caller to be authorized by polkit;
//! when that fails the error is surfaced rather than swallowed.

use crate::platform::linux::launch_first;
use zbus::blocking::{Connection, Proxy};

const NM_SERVICE: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_INTERFACE: &str = "org.freedesktop.NetworkManager";

fn system_bus() -> Result<Connection, String> {
    Connection::system().map_err(|e| format!("Cannot reach the system bus: {e}"))
}

fn manager<'a>(
    connection: &'a Connection,
    path: &'a str,
    interface: &'a str,
) -> Result<Proxy<'a>, String> {
    Proxy::new(connection, NM_SERVICE, path, interface).map_err(|e| e.to_string())
}

/// Whether Wi-Fi is actually usable.
///
/// Matches the Windows backend, whose radio API reports the combined state:
/// software Wi-Fi is meaningless while the hardware kill switch is off.
pub fn wifi_enabled() -> Result<bool, String> {
    let connection = system_bus()?;
    let proxy = manager(&connection, NM_PATH, NM_INTERFACE)?;
    let enabled = proxy
        .get_property::<bool>("WirelessEnabled")
        .map_err(|e| format!("NetworkManager Wi-Fi state is unavailable: {e}"))?;
    // A missing hardware-switch property should not hide a working radio.
    let hardware = proxy
        .get_property::<bool>("WirelessHardwareEnabled")
        .unwrap_or(true);
    Ok(enabled && hardware)
}

pub fn set_wifi_enabled(enabled: bool) -> Result<(), String> {
    let connection = system_bus()?;
    let proxy = manager(&connection, NM_PATH, NM_INTERFACE)?;
    proxy
        .set_property("WirelessEnabled", enabled)
        .map_err(|e| {
            format!("NetworkManager refused the Wi-Fi change (authorization may be required): {e}")
        })
}

/// Open Cinnamon's network settings, falling back to the connection editor.
pub fn open_settings() {
    launch_first(&[
        ("cinnamon-settings", &["network"]),
        ("nm-connection-editor", &[]),
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading Wi-Fi state must never panic; on a machine without
    /// NetworkManager this simply reports an error.
    #[test]
    fn reading_wifi_state_never_panics() {
        let _ = wifi_enabled();
    }
}
