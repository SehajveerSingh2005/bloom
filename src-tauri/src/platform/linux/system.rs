//! Low-overhead Linux system metrics backed by `sysinfo`.

use crate::types::BrightnessChangeEvent;
use std::{
    fs,
    path::PathBuf,
    sync::{atomic::Ordering, Mutex, OnceLock},
    time::Duration,
};
use sysinfo::{Disks, Networks, System};
use tauri::{AppHandle, Emitter};
use zbus::blocking::{Connection, Proxy};

struct Metrics {
    system: System,
    disks: Disks,
    networks: Networks,
}

impl Metrics {
    fn new() -> Self {
        let mut system = System::new();
        // CPU use is a delta. Prime it now so the first UI poll has a prior
        // sample rather than creating a process-wide busy wait.
        system.refresh_cpu_usage();
        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
        }
    }
}

fn metrics() -> &'static Mutex<Metrics> {
    static METRICS: OnceLock<Mutex<Metrics>> = OnceLock::new();
    METRICS.get_or_init(|| Mutex::new(Metrics::new()))
}

pub fn cpu_usage() -> Result<u32, String> {
    let mut metrics = metrics()
        .lock()
        .map_err(|_| "System metrics lock is unavailable")?;
    metrics.system.refresh_cpu_usage();
    Ok(metrics.system.global_cpu_usage().round().clamp(0.0, 100.0) as u32)
}

pub fn ram_usage() -> Result<f32, String> {
    let mut metrics = metrics()
        .lock()
        .map_err(|_| "System metrics lock is unavailable")?;
    metrics.system.refresh_memory();
    let total = metrics.system.total_memory();
    if total == 0 {
        return Err("System memory total is unavailable".into());
    }
    Ok((metrics.system.used_memory() as f64 / total as f64 * 100.0) as f32)
}

pub fn disk_free_space() -> Result<u64, String> {
    let mut metrics = metrics()
        .lock()
        .map_err(|_| "System metrics lock is unavailable")?;
    metrics.disks.refresh(false);
    Ok(metrics
        .disks
        .list()
        .iter()
        .map(|disk| disk.available_space())
        .sum())
}

/// Returns upload then download bytes observed since the previous refresh.
pub fn network_delta() -> Result<(u64, u64), String> {
    let mut metrics = metrics()
        .lock()
        .map_err(|_| "System metrics lock is unavailable")?;
    metrics.networks.refresh(true);
    let (received, transmitted) =
        metrics
            .networks
            .iter()
            .fold((0u64, 0u64), |(rx, tx), (_, data)| {
                (
                    rx.saturating_add(data.received()),
                    tx.saturating_add(data.transmitted()),
                )
            });
    Ok((transmitted, received))
}

fn backlight_device() -> Result<PathBuf, String> {
    let entries = fs::read_dir("/sys/class/backlight")
        .map_err(|_| "No kernel backlight device is available".to_string())?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .next()
        .ok_or_else(|| "No kernel backlight device is available".to_string())
}

fn backlight_values() -> Result<(PathBuf, u32, u32), String> {
    let device = backlight_device()?;
    let current: u32 = fs::read_to_string(device.join("brightness"))
        .map_err(|e| format!("Cannot read backlight brightness: {e}"))?
        .trim()
        .parse()
        .map_err(|_| "Kernel backlight brightness is invalid".to_string())?;
    let maximum: u32 = fs::read_to_string(device.join("max_brightness"))
        .map_err(|e| format!("Cannot read backlight maximum: {e}"))?
        .trim()
        .parse()
        .map_err(|_| "Kernel backlight maximum is invalid".to_string())?;
    if maximum == 0 {
        return Err("Kernel backlight maximum is zero".into());
    }
    Ok((device, current, maximum))
}

pub fn brightness() -> Result<u32, String> {
    let (_, current, maximum) = backlight_values()?;
    Ok((current.saturating_mul(100) / maximum).min(100))
}

/// logind grants a brightness change to the owner of the active session through
/// polkit, which is how the desktop's own brightness keys are applied.
const LOGIND_SERVICE: &str = "org.freedesktop.login1";
const LOGIND_SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const LOGIND_SESSION_INTERFACE: &str = "org.freedesktop.login1.Session";

/// The kernel's name for the backlight device, which is what logind is given.
fn backlight_name(device: &PathBuf) -> Result<String, String> {
    device
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "The kernel backlight device has no name".to_string())
}

fn set_brightness_through_logind(name: &str, raw: u32) -> Result<(), String> {
    let connection =
        Connection::system().map_err(|e| format!("Cannot reach the system bus: {e}"))?;
    let proxy = Proxy::new(
        &connection,
        LOGIND_SERVICE,
        LOGIND_SESSION_PATH,
        LOGIND_SESSION_INTERFACE,
    )
    .map_err(|e| e.to_string())?;
    proxy
        .call_method("SetBrightness", &("backlight", name, raw))
        .map_err(|e| format!("logind refused the brightness change: {e}"))?;
    Ok(())
}

/// Set the display brightness.
///
/// The device node is root-owned on a normal system, so writing it directly
/// fails for the user while the desktop's own brightness keys succeed. logind
/// performs the write for the active session instead, and the direct write is
/// kept as a fallback for systems that grant the user access through a group or
/// a udev rule.
pub fn set_brightness(percent: u32) -> Result<(), String> {
    let (device, _, maximum) = backlight_values()?;
    let raw = percent.min(100).saturating_mul(maximum) / 100;
    // Recorded so the watcher below does not announce Bloom's own change back to
    // Bloom, which would pop the brightness display while its slider is dragged.
    crate::state::LAST_BRIGHTNESS_CHANGE.store(crate::platform::linux::now_ms(), Ordering::Relaxed);
    let name = backlight_name(&device)?;
    match set_brightness_through_logind(&name, raw) {
        Ok(()) => Ok(()),
        Err(logind_error) => fs::write(device.join("brightness"), raw.to_string()).map_err(|e| {
            format!("{logind_error}, and the device node could not be written either: {e}")
        }),
    }
}

/// How often the kernel backlight is read. This is one small sysfs read with no
/// subprocess involved, so it can run often enough to feel immediate.
const BRIGHTNESS_POLL: Duration = Duration::from_millis(120);

/// Changes to the backlight are announced for this long after Bloom sets it.
const ECHO_WINDOW_MS: i64 = 2000;

/// Report the display brightness, wherever it was changed from.
///
/// The brightness keys belong to the desktop, which writes straight to the
/// kernel backlight, so this follows the hardware rather than pressing keys.
/// Nothing else announces brightness on Linux, so without this Bloom's
/// brightness display never appears at all.
pub fn start_brightness_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        // Seeded rather than announced, so Bloom does not pop its display on
        // startup before anything has been touched.
        let mut previous = brightness().ok();
        loop {
            if let Ok(percent) = brightness() {
                crate::state::CURRENT_BRIGHTNESS.store(percent, Ordering::Relaxed);
                if previous != Some(percent) {
                    // Remember it either way, so a suppressed change is not
                    // announced late once the echo window has passed.
                    previous = Some(percent);
                    let echo = crate::platform::linux::now_ms()
                        - crate::state::LAST_BRIGHTNESS_CHANGE.load(Ordering::Relaxed)
                        < ECHO_WINDOW_MS;
                    if !echo {
                        let _ = app.emit(
                            "brightness-change",
                            BrightnessChangeEvent { brightness: percent },
                        );
                    }
                }
            }
            std::thread::sleep(BRIGHTNESS_POLL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writing the backlight needs logind's help, which only exists on a real
    /// session, so this runs on demand. It moves the display for a moment and
    /// puts it back.
    ///
    /// `cargo test --release -- --ignored brightness_round_trip`
    #[test]
    #[ignore = "changes the display brightness for a moment"]
    fn brightness_round_trip() {
        let Ok(original) = brightness() else {
            eprintln!("skipping: no kernel backlight on this system");
            return;
        };
        // A visible but modest step, in either direction.
        let target = if original > 50 { original - 10 } else { original + 10 };
        let applied = set_brightness(target);
        std::thread::sleep(Duration::from_millis(250));
        let reached = brightness();
        // Restore before asserting, so a failure cannot leave the display dim.
        let restored = set_brightness(original);

        assert!(applied.is_ok(), "setting brightness failed: {applied:?}");
        let reached = reached.expect("brightness should still be readable");
        assert!(
            reached.abs_diff(target) <= 1,
            "expected about {target}%, read {reached}%"
        );
        assert!(restored.is_ok(), "restoring brightness failed: {restored:?}");
    }
}
