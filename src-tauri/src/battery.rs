//! Battery details for the notch's battery tooltip: charge, time left or until
//! full, the power mode and the battery's health, as the taskbar's battery
//! icon and Windows' battery settings report them.

use serde::Serialize;
use windows::core::w;
use windows::Devices::Power::Battery;
use windows::System::Power::BatteryStatus;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

/// `GetSystemPowerStatus` reports this when a value is unknown.
const UNKNOWN_U8: u8 = 255;
const NO_SYSTEM_BATTERY: u8 = 128;
const CHARGING_FLAG: u8 = 8;

/// Estimates longer than this come from a momentary trickle of power and are
/// not worth showing.
const MAX_ESTIMATE_SECS: u64 = 24 * 60 * 60;

#[derive(Clone, Serialize, Default, Debug, PartialEq)]
pub struct BatteryDetails {
    pub present: bool,
    pub percent: Option<u8>,
    pub plugged_in: bool,
    pub charging: bool,
    /// Windows' estimate of the time left on battery.
    pub seconds_left: Option<u64>,
    /// Estimated from the charge rate, so it runs short as charging tapers.
    pub seconds_to_full: Option<u64>,
    pub health_percent: Option<u8>,
    pub power_mode: Option<&'static str>,
}

/// Names of the power mode overlays from Settings > System > Power & battery.
fn power_mode_name(guid: &str) -> Option<&'static str> {
    match guid.trim().to_ascii_lowercase().as_str() {
        "961cc777-2547-4f9d-8174-7d86181b8a7a" => Some("Best power efficiency"),
        "00000000-0000-0000-0000-000000000000" => Some("Balanced"),
        "3af9b8d9-7c97-431d-ad78-34a8bfea439f" => Some("Better performance"),
        "ded574b5-45a0-4f42-8737-46345c09c238" => Some("Best performance"),
        _ => None,
    }
}

fn seconds_to_full(remaining_mwh: i32, full_mwh: i32, rate_mw: i32) -> Option<u64> {
    if rate_mw <= 0 || full_mwh <= remaining_mwh {
        return None;
    }
    let secs = (full_mwh - remaining_mwh) as u64 * 3600 / rate_mw as u64;
    (secs > 0 && secs <= MAX_ESTIMATE_SECS).then_some(secs)
}

/// Full-charge capacity as a share of design capacity. New batteries can
/// exceed their design capacity slightly, which reads as 100%.
fn health_percent(full_mwh: i32, design_mwh: i32) -> Option<u8> {
    if full_mwh <= 0 || design_mwh <= 0 {
        return None;
    }
    Some((full_mwh as u64 * 100 / design_mwh as u64).min(100) as u8)
}

fn from_power_status(status: &SYSTEM_POWER_STATUS) -> BatteryDetails {
    let present = status.BatteryFlag != UNKNOWN_U8 && status.BatteryFlag & NO_SYSTEM_BATTERY == 0;
    let plugged_in = status.ACLineStatus == 1;
    BatteryDetails {
        present,
        percent: (present && status.BatteryLifePercent <= 100).then_some(status.BatteryLifePercent),
        plugged_in,
        charging: present && status.BatteryFlag & CHARGING_FLAG != 0,
        seconds_left: (present && !plugged_in && status.BatteryLifeTime != u32::MAX)
            .then_some(status.BatteryLifeTime as u64)
            .filter(|secs| *secs > 0 && *secs <= MAX_ESTIMATE_SECS),
        ..Default::default()
    }
}

fn read_power_mode(plugged_in: bool) -> Option<&'static str> {
    let value = if plugged_in {
        w!("ActiveOverlayAcPowerScheme")
    } else {
        w!("ActiveOverlayDcPowerScheme")
    };
    let mut buffer = [0u16; 64];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SYSTEM\\CurrentControlSet\\Control\\Power\\User\\PowerSchemes"),
            value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    power_mode_name(&String::from_utf16_lossy(&buffer[..len]))
}

fn details_sync() -> BatteryDetails {
    let mut status = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return BatteryDetails::default();
    }
    let mut details = from_power_status(&status);
    details.power_mode = read_power_mode(details.plugged_in);
    if !details.present {
        return details;
    }

    // The aggregate report covers every battery and adds what
    // GetSystemPowerStatus lacks: the charge rate and the capacities.
    let Ok(report) = Battery::AggregateBattery().and_then(|battery| battery.GetReport()) else {
        return details;
    };
    let read = |value: windows::core::Result<windows::Foundation::IReference<i32>>| {
        value.and_then(|value| value.Value()).ok()
    };
    let remaining = read(report.RemainingCapacityInMilliwattHours());
    let full = read(report.FullChargeCapacityInMilliwattHours());
    let design = read(report.DesignCapacityInMilliwattHours());
    let rate = read(report.ChargeRateInMilliwatts());

    if let Ok(battery_status) = report.Status() {
        details.charging = battery_status == BatteryStatus::Charging;
    }
    if details.charging {
        if let (Some(remaining), Some(full), Some(rate)) = (remaining, full, rate) {
            details.seconds_to_full = seconds_to_full(remaining, full, rate);
        }
    }
    if let (Some(full), Some(design)) = (full, design) {
        details.health_percent = health_percent(full, design);
    }
    details
}

#[tauri::command]
pub async fn get_battery_details() -> Result<BatteryDetails, String> {
    tauri::async_runtime::spawn_blocking(|| {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
        details_sync()
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(ac: u8, flag: u8, percent: u8, life: u32) -> SYSTEM_POWER_STATUS {
        SYSTEM_POWER_STATUS {
            ACLineStatus: ac,
            BatteryFlag: flag,
            BatteryLifePercent: percent,
            SystemStatusFlag: 0,
            BatteryLifeTime: life,
            BatteryFullLifeTime: u32::MAX,
        }
    }

    #[test]
    fn names_power_modes() {
        assert_eq!(
            power_mode_name("961CC777-2547-4F9D-8174-7D86181B8A7A"),
            Some("Best power efficiency")
        );
        assert_eq!(
            power_mode_name("00000000-0000-0000-0000-000000000000"),
            Some("Balanced")
        );
        assert_eq!(
            power_mode_name(" ded574b5-45a0-4f42-8737-46345c09c238 "),
            Some("Best performance")
        );
        assert_eq!(
            power_mode_name("ab6534a3-bc02-4c44-94d1-a8535b2eb070"),
            None
        );
    }

    #[test]
    fn estimates_time_to_full() {
        // 10 Wh to go at 20 W is half an hour.
        assert_eq!(seconds_to_full(40_000, 50_000, 20_000), Some(1800));
        assert_eq!(seconds_to_full(50_000, 50_000, 20_000), None);
        assert_eq!(seconds_to_full(40_000, 50_000, 0), None);
        assert_eq!(seconds_to_full(40_000, 50_000, -5_000), None);
        // A trickle of power gives no useful estimate.
        assert_eq!(seconds_to_full(0, 50_000, 1), None);
    }

    #[test]
    fn computes_health() {
        assert_eq!(health_percent(45_000, 50_000), Some(90));
        assert_eq!(health_percent(51_000, 50_000), Some(100));
        assert_eq!(health_percent(45_000, 0), None);
        assert_eq!(health_percent(0, 50_000), None);
    }

    #[test]
    fn reads_power_status() {
        let on_battery = from_power_status(&status(0, 1, 72, 3 * 3600));
        assert!(on_battery.present && !on_battery.plugged_in && !on_battery.charging);
        assert_eq!(on_battery.percent, Some(72));
        assert_eq!(on_battery.seconds_left, Some(3 * 3600));

        let charging = from_power_status(&status(1, CHARGING_FLAG, 87, u32::MAX));
        assert!(charging.plugged_in && charging.charging);
        assert_eq!(charging.seconds_left, None);

        let desktop = from_power_status(&status(1, NO_SYSTEM_BATTERY, UNKNOWN_U8, u32::MAX));
        assert!(!desktop.present);
        assert_eq!(desktop.percent, None);

        let unknown = from_power_status(&status(UNKNOWN_U8, UNKNOWN_U8, UNKNOWN_U8, u32::MAX));
        assert!(!unknown.present && !unknown.plugged_in);
    }
}
