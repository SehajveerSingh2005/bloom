//! Bluetooth state through BlueZ over the system bus.
//!
//! The adapter is discovered through the ObjectManager rather than assuming an
//! `hci0` path, and power changes report authorization failures instead of
//! failing silently. Bloom never asks for root.

use crate::platform::linux::launch_first;
use std::collections::HashMap;
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue},
};

const BLUEZ_SERVICE: &str = "org.bluez";
const OBJECT_MANAGER_PATH: &str = "/";
const OBJECT_MANAGER_INTERFACE: &str = "org.freedesktop.DBus.ObjectManager";
const ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";

fn system_bus() -> Result<Connection, String> {
    Connection::system().map_err(|e| format!("Cannot reach the system bus: {e}"))
}

/// Find BlueZ's adapter path, which is normally `/org/bluez/hciN`.
fn adapter_path(connection: &Connection) -> Result<String, String> {
    let manager = Proxy::new(
        connection,
        BLUEZ_SERVICE,
        OBJECT_MANAGER_PATH,
        OBJECT_MANAGER_INTERFACE,
    )
    .map_err(|e| format!("BlueZ is unavailable: {e}"))?;
    let objects: HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> = manager
        .call("GetManagedObjects", &())
        .map_err(|e| format!("BlueZ did not report its objects: {e}"))?;
    objects
        .into_iter()
        .find(|(_, interfaces)| interfaces.contains_key(ADAPTER_INTERFACE))
        .map(|(path, _)| path.as_str().to_owned())
        .ok_or_else(|| "No Bluetooth adapter was found".to_string())
}

/// Build a proxy for the adapter. The path is owned by the caller, so both must
/// be bound before the proxy is created.
fn adapter<'a>(
    connection: &'a Connection,
    path: &'a str,
) -> Result<Proxy<'a>, String> {
    Proxy::new(connection, BLUEZ_SERVICE, path, ADAPTER_INTERFACE).map_err(|e| e.to_string())
}

pub fn enabled() -> Result<bool, String> {
    let connection = system_bus()?;
    let path = adapter_path(&connection)?;
    let proxy = adapter(&connection, &path)?;
    proxy
        .get_property::<bool>("Powered")
        .map_err(|e| format!("Bluetooth power state is unavailable: {e}"))
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let connection = system_bus()?;
    let path = adapter_path(&connection)?;
    let proxy = adapter(&connection, &path)?;
    proxy.set_property("Powered", enabled).map_err(|e| {
        format!("BlueZ refused the Bluetooth change (authorization may be required): {e}")
    })
}

/// Open Blueman, falling back to Cinnamon's Bluetooth panel.
pub fn open_settings() {
    launch_first(&[
        ("blueman-manager", &[]),
        ("cinnamon-settings", &["bluetooth"]),
    ]);
}

/// Cinnamon has no airplane-mode panel, so this opens the network panel where
/// the radios are controlled. Bloom never blocks radios itself, because that
/// requires root.
pub fn open_airplane_mode_settings() {
    launch_first(&[("cinnamon-settings", &["network"])]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading Bluetooth state must never panic; without BlueZ this simply
    /// reports an error.
    #[test]
    fn reading_bluetooth_state_never_panics() {
        let _ = enabled();
    }
}
