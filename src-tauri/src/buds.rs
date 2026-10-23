//! Battery levels for each earbud and the case, for the notch's connection
//! popup. Windows only reports one level per device, so true wireless
//! earbuds are read the way their own apps read them:
//!
//! - **Apple and Beats** broadcast a proximity pairing message with both
//!   buds' and the case's levels. Its model id matches the product id in the
//!   connected device's Windows id, and the strongest broadcast of that
//!   model is taken, so another person's AirPods nearby aren't read.
//! - **Google Fast Pair** earbuds (Pixel Buds and most Android earbuds from
//!   Sony, JBL, Nothing, OnePlus, Soundcore and others) send a battery update
//!   on their Fast Pair message stream, an RFCOMM channel, when it opens.
//! - **Samsung Galaxy Buds** send a status update on their own RFCOMM
//!   channel when it opens.
//!
//! Each is read once, just after the device connects, and closed again.
//! Earbuds that support none of these keep the single level Windows reports.

use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{GUID, HSTRING};
use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
    BluetoothLEScanningMode,
};
use windows::Devices::Bluetooth::Rfcomm::RfcommServiceId;
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothDevice};
use windows::Foundation::TypedEventHandler;
use windows::Networking::Sockets::{SocketProtectionLevel, StreamSocket};
use windows::Storage::Streams::{DataReader, InputStreamOptions};

/// One part's battery.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BudLevel {
    /// Percent.
    pub level: u8,
    pub charging: bool,
}

/// Each bud's and the case's battery, where known.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BudsBattery {
    pub left: Option<BudLevel>,
    pub right: Option<BudLevel>,
    pub case: Option<BudLevel>,
}

impl BudsBattery {
    /// Worth showing as separate levels: at least two parts are known.
    /// (Headphones that use the same messages report one level.)
    pub fn is_split(&self) -> bool {
        [self.left, self.right, self.case]
            .iter()
            .filter(|l| l.is_some())
            .count()
            >= 2
    }
}

// ---- Apple proximity pairing ----------------------------------------------

pub const APPLE_COMPANY_ID: u16 = 0x004C;
const PROXIMITY_PAIRING: u8 = 0x07;
const PROXIMITY_PAIRING_LEN: usize = 25;

/// A level sent in tens of percent: 0 to 10, anything else is unknown.
fn tens(value: u8, charging: bool) -> Option<BudLevel> {
    (value <= 10).then_some(BudLevel {
        level: value * 10,
        charging,
    })
}

/// The model and battery levels in Apple's manufacturer data (the bytes
/// after the company id), which holds one or more type-length-value
/// messages.
///
/// A proximity pairing message's body is: a prefix byte, the model id
/// (little-endian), a status byte, the two buds' levels (one per nibble), the
/// case's level and the charging flags, then the lid. The buds' levels are
/// sent as "this bud" (low nibble) and "the other bud": status bit 5 is set
/// when the left bud is the one broadcasting. The body is 25 bytes, ending in
/// an encrypted block.
///
/// Some earbuds send a shorter message of the same type, 15 bytes holding a
/// model id and their Bluetooth address but no levels where these are. It
/// isn't read.
pub fn apple_battery(data: &[u8]) -> Option<(u16, BudsBattery)> {
    let mut i = 0;
    while i + 2 <= data.len() {
        let (kind, len) = (data[i], data[i + 1] as usize);
        let body = data.get(i + 2..i + 2 + len)?;
        if kind == PROXIMITY_PAIRING && body.len() >= PROXIMITY_PAIRING_LEN {
            let model = u16::from_le_bytes([body[1], body[2]]);
            let left_broadcasting = body[3] & 0x20 != 0;
            let (this, other) = (body[4] & 0x0F, body[4] >> 4);
            let flags = body[5] >> 4;
            let this = tens(this, flags & 0b001 != 0);
            let other = tens(other, flags & 0b010 != 0);
            let case = tens(body[5] & 0x0F, flags & 0b100 != 0);
            let (left, right) = if left_broadcasting {
                (this, other)
            } else {
                (other, this)
            };
            return Some((model, BudsBattery { left, right, case }));
        }
        i += 2 + len;
    }
    None
}

/// A device's Bluetooth vendor and product id, from one of its Windows
/// instance ids, such as `BTHENUM\{0000110B-...}_VID&0001004C_PID&200E\...`.
/// The vendor keeps its id source (0001 is the Bluetooth SIG list).
pub fn vendor_product(instance_id: &str) -> Option<(u32, u16)> {
    let upper = instance_id.to_ascii_uppercase();
    let vid_at = upper.find("VID&")? + 4;
    let pid_at = upper.find("_PID&")? + 5;
    let vid = u32::from_str_radix(upper.get(vid_at..vid_at + 8)?, 16).ok()?;
    let pid = u16::from_str_radix(upper.get(pid_at..pid_at + 4)?, 16).ok()?;
    Some((vid, pid))
}

/// The Bluetooth SIG id source with Apple's company id.
pub const APPLE_VENDOR: u32 = 0x0001_0000 | APPLE_COMPANY_ID as u32;

/// The weakest broadcast taken for the connected AirPods: a device in use
/// is near the computer.
const APPLE_MIN_RSSI: i16 = -72;

/// Listens for the connected Apple device's broadcasts for up to `wait` and
/// returns the levels from the strongest one of its model.
pub fn read_apple(model: u16, wait: Duration) -> Option<BudsBattery> {
    let watcher = BluetoothLEAdvertisementWatcher::new().ok()?;
    // The proximity pairing message is in the broadcast itself; there's no
    // need to ask the buds for more.
    let _ = watcher.SetScanningMode(BluetoothLEScanningMode::Passive);
    let best: Arc<Mutex<Option<(i16, BudsBattery)>>> = Arc::new(Mutex::new(None));
    let seen = best.clone();
    watcher
        .Received(&TypedEventHandler::new(
            move |_, args: windows::core::Ref<BluetoothLEAdvertisementReceivedEventArgs>| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let rssi = args.RawSignalStrengthInDBm().unwrap_or(i16::MIN);
                if rssi < APPLE_MIN_RSSI {
                    return Ok(());
                }
                let Ok(entries) = args.Advertisement().and_then(|a| a.ManufacturerData()) else {
                    return Ok(());
                };
                for entry in entries {
                    if entry.CompanyId().ok() != Some(APPLE_COMPANY_ID) {
                        continue;
                    }
                    let Some(bytes) = entry.Data().ok().and_then(|b| buffer_bytes(&b)) else {
                        continue;
                    };
                    if let Some((found, battery)) = apple_battery(&bytes) {
                        if found != model || !battery.is_split() {
                            continue;
                        }
                        if let Ok(mut best) = seen.lock() {
                            if best.as_ref().is_none_or(|(r, _)| rssi > *r) {
                                *best = Some((rssi, battery));
                            }
                        }
                    }
                }
                Ok(())
            },
        ))
        .ok()?;
    watcher.Start().ok()?;
    // Gather for a moment once something is heard, so the strongest of a
    // few broadcasts (each bud sends its own) is taken.
    let started = Instant::now();
    let mut heard_at = None;
    while started.elapsed() < wait {
        std::thread::sleep(Duration::from_millis(100));
        let heard = best.lock().ok().is_some_and(|b| b.is_some());
        match heard_at {
            None if heard => heard_at = Some(Instant::now()),
            Some(at) if at.elapsed() >= Duration::from_millis(1200) => break,
            _ => {}
        }
    }
    let _ = watcher.Stop();
    let found = best.lock().ok()?.take();
    found.map(|(_, battery)| battery)
}

fn buffer_bytes(buffer: &windows::Storage::Streams::IBuffer) -> Option<Vec<u8>> {
    let reader = DataReader::FromBuffer(buffer).ok()?;
    let mut bytes = vec![0u8; buffer.Length().ok()? as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

// ---- Google Fast Pair message stream --------------------------------------

/// The Fast Pair message stream's RFCOMM service.
pub const FAST_PAIR_STREAM: GUID = GUID::from_u128(0xdf21fe2c_2515_4fdb_8886_f12c4d67927c);

/// A Fast Pair level: the low seven bits are percent (0x7F is unknown), the
/// top bit is set while charging.
fn fast_pair_level(byte: u8) -> Option<BudLevel> {
    let level = byte & 0x7F;
    (level <= 100).then_some(BudLevel {
        level,
        charging: byte & 0x80 != 0,
    })
}

/// The latest battery update in bytes read from the message stream. Each
/// message is a group, a code, a big-endian length and its data; a battery
/// update is group 3 (device information), code 3, with the left bud's,
/// right bud's and case's levels.
pub fn fast_pair_battery(stream: &[u8]) -> Option<BudsBattery> {
    let mut found = None;
    let mut i = 0;
    while i + 4 <= stream.len() {
        let (group, code) = (stream[i], stream[i + 1]);
        let len = u16::from_be_bytes([stream[i + 2], stream[i + 3]]) as usize;
        let Some(data) = stream.get(i + 4..i + 4 + len) else {
            break;
        };
        if group == 0x03 && code == 0x03 && len >= 3 {
            found = Some(BudsBattery {
                left: fast_pair_level(data[0]),
                right: fast_pair_level(data[1]),
                case: fast_pair_level(data[2]),
            });
        }
        i += 4 + len;
    }
    found
}

// ---- Samsung Galaxy Buds --------------------------------------------------

/// The Galaxy Buds' RFCOMM services: Buds2 and later, then Buds+, Live and
/// Pro (the standard serial port id), then the first Galaxy Buds.
pub const GALAXY_NEW: GUID = GUID::from_u128(0x2e73a4ad_332d_41fc_90e2_16bef06523f2);
pub const GALAXY_STANDARD: GUID = GUID::from_u128(0x00001101_0000_1000_8000_00805f9b34fb);
pub const GALAXY_LEGACY: GUID = GUID::from_u128(0x00001102_0000_1000_8000_00805f9b34fd);

const STATUS_UPDATED: u8 = 0x60;
const EXTENDED_STATUS_UPDATED: u8 = 0x61;

/// A bud's placement, from a nibble: 0 means it isn't connected (out of
/// range or flat), 3 and 4 that it's in the case (open or closed).
fn placed(placement: u8) -> bool {
    placement != 0
}
fn in_case(placement: u8) -> bool {
    matches!(placement, 3 | 4)
}

fn percent(value: u8) -> Option<BudLevel> {
    (value <= 100).then_some(BudLevel {
        level: value,
        charging: false,
    })
}

/// The battery in one status message's payload.
fn galaxy_status(id: u8, payload: &[u8], legacy: bool) -> Option<BudsBattery> {
    // The extended status has an extra byte (the ear tip type) before the
    // levels. Its placement and case level come after the connection state.
    let (at, placement_at, case_at) = match id {
        STATUS_UPDATED => (1, 5, 6),
        EXTENDED_STATUS_UPDATED => (2, 6, 7),
        _ => return None,
    };
    let left = percent(*payload.get(at)?);
    let right = percent(*payload.get(at + 1)?);
    if legacy {
        // The first Galaxy Buds report no case level or placement.
        return Some(BudsBattery {
            left,
            right,
            case: None,
        });
    }
    let placement = *payload.get(placement_at)?;
    let (left_at, right_at) = (placement >> 4, placement & 0x0F);
    // The case only reports a level while a bud is in it.
    let case = payload
        .get(case_at)
        .copied()
        .filter(|_| in_case(left_at) || in_case(right_at))
        .and_then(percent)
        .filter(|l| l.level > 0);
    Some(BudsBattery {
        left: left.filter(|_| placed(left_at)),
        right: right.filter(|_| placed(right_at)),
        case,
    })
}

/// The latest battery in bytes read from a Galaxy Buds channel. Messages
/// are framed by a start byte (0xFD, or 0xFE on the first Galaxy Buds), a
/// header holding the size of the id, payload and checksum, then the id,
/// the payload, a two-byte checksum and an end byte (0xDD, or 0xEE).
pub fn galaxy_battery(stream: &[u8]) -> Option<BudsBattery> {
    let mut found = None;
    let mut i = 0;
    while i + 4 <= stream.len() {
        let legacy = match stream[i] {
            0xFD => false,
            0xFE => true,
            _ => {
                i += 1;
                continue;
            }
        };
        let size = if legacy {
            stream[i + 2] as usize
        } else {
            (u16::from_le_bytes([stream[i + 1], stream[i + 2]]) & 0x3FF) as usize
        };
        let end = i + 3 + size;
        let end_byte = if legacy { 0xEE } else { 0xDD };
        if size < 3 || stream.get(end) != Some(&end_byte) {
            i += 1;
            continue;
        }
        let id = stream[i + 3];
        let payload = &stream[i + 4..end - 2];
        if let Some(battery) = galaxy_status(id, payload, legacy) {
            found = Some(battery);
        }
        i = end + 1;
    }
    found
}

// ---- Reading over RFCOMM --------------------------------------------------

/// Opens a device's RFCOMM service, if it has one, and returns what it sends
/// until `parse` finds a battery reading or `wait` runs out.
fn read_rfcomm(
    device: &BluetoothDevice,
    service: GUID,
    wait: Duration,
    parse: fn(&[u8]) -> Option<BudsBattery>,
) -> Option<BudsBattery> {
    let id = RfcommServiceId::FromUuid(service).ok()?;
    // The services Windows recorded when the device was paired: asking the
    // device again can block for a long time.
    let services = device
        .GetRfcommServicesForIdWithCacheModeAsync(&id, BluetoothCacheMode::Cached)
        .and_then(|op| op.get())
        .and_then(|r| r.Services())
        .ok()?;
    let service = services.GetAt(0).ok()?;
    let socket = StreamSocket::new().ok()?;
    // Connecting and reading block until the device answers; closing the
    // socket ends either, so it's closed once `wait` runs out.
    let closer = socket.clone();
    let done = Arc::new(Mutex::new(false));
    let timer_done = done.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        if !timer_done.lock().map(|d| *d).unwrap_or(true) {
            let _ = closer.Close();
        }
    });
    socket
        .ConnectWithProtectionLevelAsync(
            &service.ConnectionHostName().ok()?,
            &service.ConnectionServiceName().ok()?,
            SocketProtectionLevel::BluetoothEncryptionAllowNullAuthentication,
        )
        .and_then(|op| op.get())
        .ok()?;
    let reader = DataReader::CreateDataReader(&socket.InputStream().ok()?).ok()?;
    let _ = reader.SetInputStreamOptions(InputStreamOptions::Partial);
    let mut received = Vec::new();
    let mut found = None;
    while let Ok(count) = reader.LoadAsync(512).and_then(|op| op.get()) {
        if count == 0 {
            break;
        }
        let mut chunk = vec![0u8; count as usize];
        if reader.ReadBytes(&mut chunk).is_err() {
            break;
        }
        received.extend_from_slice(&chunk);
        found = parse(&received).filter(BudsBattery::is_split);
        if found.is_some() {
            break;
        }
    }
    if let Ok(mut done) = done.lock() {
        *done = true;
    }
    let _ = socket.Close();
    found
}

/// Reads a connected device's levels over Fast Pair or, for Galaxy Buds,
/// Samsung's channel. `classic_id` is the device's classic Bluetooth id.
pub fn read_rfcomm_battery(classic_id: &str, name: &str, wait: Duration) -> Option<BudsBattery> {
    let device = BluetoothDevice::FromIdAsync(&HSTRING::from(classic_id))
        .and_then(|op| op.get())
        .ok()?;
    if let Some(battery) = read_rfcomm(&device, FAST_PAIR_STREAM, wait, fast_pair_battery) {
        return Some(battery);
    }
    // The serial port ids are generic, so only Galaxy Buds are read on them.
    if !name.to_ascii_lowercase().contains("galaxy buds") {
        return read_rfcomm(&device, GALAXY_NEW, wait, galaxy_battery);
    }
    [GALAXY_NEW, GALAXY_STANDARD, GALAXY_LEGACY]
        .into_iter()
        .find_map(|service| read_rfcomm(&device, service, wait, galaxy_battery))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn lvl(level: u8, charging: bool) -> Option<BudLevel> {
        Some(BudLevel { level, charging })
    }

    #[test]
    fn reads_airpods_with_either_bud_broadcasting() {
        // AirPods Pro (0x200E), lid open, both in the case: the right bud
        // broadcasts (status bit 5 clear), so the low nibble is the right
        // bud. Right 80%, left 100%, case 50% and charging.
        let right_sends = [
            0x07, 0x19, 0x01, 0x0E, 0x20, 0x55, 0xA8, 0x45, 0x31, 0x00, 0x05, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let (model, battery) = apple_battery(&right_sends).unwrap();
        assert_eq!(model, 0x200E);
        assert_eq!(
            battery,
            BudsBattery {
                left: lvl(100, false),
                right: lvl(80, false),
                case: lvl(50, true),
            }
        );
        // The same state from the left bud: status bit 5 set, nibbles swapped,
        // and "this bud is charging" now means the left one.
        let mut left_sends = right_sends;
        left_sends[5] = 0x75;
        left_sends[6] = 0x8A;
        left_sends[7] = 0x15;
        let (_, battery) = apple_battery(&left_sends).unwrap();
        assert_eq!(battery.left, lvl(100, true));
        assert_eq!(battery.right, lvl(80, false));
        assert_eq!(battery.case, lvl(50, false));
    }

    /// A full proximity pairing message (type, length, 25-byte body).
    fn proximity(model: u16, status: u8, buds: u8, case: u8) -> Vec<u8> {
        let mut data = vec![0x07, 0x19, 0x01];
        data.extend(model.to_le_bytes());
        data.extend([status, buds, case, 0x31, 0x00, 0x00]);
        data.extend([0u8; 16]);
        data
    }

    #[test]
    fn ignores_the_short_message_with_an_address() {
        // Heard from AirPods-compatible earbuds: model 0x2014, then their
        // Bluetooth address, and no levels.
        let short = [
            0x07, 0x0F, 0x00, 0x14, 0x20, 0x3E, 0x78, 0x1D, 0x85, 0x57, 0xF0, 0xF5, 0xFF, 0xE4,
            0x33, 0x0A, 0x00,
        ];
        assert_eq!(apple_battery(&short), None);
    }

    #[test]
    fn treats_apple_levels_over_ten_as_unknown() {
        // A bud out of range (0xF) and the case closed away (0xF).
        let data = proximity(0x2014, 0x55, 0xF6, 0x0F);
        let (model, battery) = apple_battery(&data).unwrap();
        assert_eq!(model, 0x2014);
        assert_eq!(battery.right, lvl(60, false));
        assert_eq!(battery.left, None);
        assert_eq!(battery.case, None);
        assert!(!battery.is_split());
    }

    #[test]
    fn finds_proximity_pairing_among_other_apple_messages() {
        // A nearby-info message (0x10) first, then proximity pairing.
        let mut data = vec![0x10, 0x02, 0x0B, 0x1C];
        data.extend(proximity(0x2024, 0x75, 0x99, 0x09));
        let (model, battery) = apple_battery(&data).unwrap();
        assert_eq!(model, 0x2024);
        assert_eq!(battery.left, lvl(90, false));
        assert_eq!(battery.right, lvl(90, false));
        assert_eq!(battery.case, lvl(90, false));
        // Not proximity pairing, or cut short.
        assert!(apple_battery(&[0x10, 0x02, 0x0B, 0x1C]).is_none());
        assert!(apple_battery(&[0x07, 0x19, 0x01, 0x0E]).is_none());
        assert!(apple_battery(&[]).is_none());
    }

    #[test]
    fn reads_vendor_and_product_from_windows_ids() {
        assert_eq!(
            vendor_product(
                r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001004C_PID&200E\7&1E98AE4D&0&3E781D8557F0_C00000000"
            ),
            Some((APPLE_VENDOR, 0x200E))
        );
        assert_eq!(
            vendor_product(r"BTHENUM\{0000110b-...}_vid&000105d6_pid&000a\x"),
            Some((0x0001_05D6, 0x000A))
        );
        assert_eq!(vendor_product(r"BTHENUM\DEV_E80F6884BD6E\7&1E98"), None);
    }

    #[test]
    fn reads_fast_pair_battery_updates() {
        // Model id, then a battery update (left 87%, right 65% charging,
        // case unknown), then another (case now 40%).
        let stream = [
            0x03, 0x01, 0x00, 0x03, 0xAA, 0xBB, 0xCC, // model id
            0x03, 0x03, 0x00, 0x03, 0x57, 0xC1, 0x7F, // battery
        ];
        assert_eq!(
            fast_pair_battery(&stream),
            Some(BudsBattery {
                left: lvl(87, false),
                right: lvl(65, true),
                case: None,
            })
        );
        let mut later = stream.to_vec();
        later.extend([0x03, 0x03, 0x00, 0x03, 0x57, 0x41, 0x28]);
        assert_eq!(fast_pair_battery(&later).unwrap().case, lvl(40, false));
        // A message cut off mid-way, and nothing about the battery.
        assert_eq!(fast_pair_battery(&[0x03, 0x03, 0x00, 0x03, 0x57]), None);
        assert_eq!(fast_pair_battery(&stream[..7]), None);
    }

    fn galaxy_frame(id: u8, payload: &[u8]) -> Vec<u8> {
        let size = (payload.len() + 3) as u16;
        let mut frame = vec![0xFD];
        frame.extend(size.to_le_bytes());
        frame.push(id);
        frame.extend(payload);
        frame.extend([0x12, 0x34, 0xDD]);
        frame
    }

    #[test]
    fn reads_galaxy_buds_status_updates() {
        // Extended status: revision, ear tip, left 76, right 81, coupled,
        // main connection, placement (left wearing, right in the case),
        // case 55.
        let extended = galaxy_frame(0x61, &[0x0D, 0x00, 76, 81, 1, 0, 0x13, 55, 0, 0]);
        assert_eq!(
            galaxy_battery(&extended),
            Some(BudsBattery {
                left: lvl(76, false),
                right: lvl(81, false),
                case: lvl(55, false),
            })
        );
        // A later status update: both worn, so the case level isn't current;
        // and noise before the frame.
        let mut stream = vec![0x00, 0xDD];
        stream.extend(&extended);
        stream.extend(galaxy_frame(0x60, &[0x0D, 70, 80, 1, 0, 0x11, 55, 0]));
        assert_eq!(
            galaxy_battery(&stream),
            Some(BudsBattery {
                left: lvl(70, false),
                right: lvl(80, false),
                case: None,
            })
        );
        // A bud that isn't connected has no level.
        let one = galaxy_frame(0x60, &[0x0D, 70, 0, 1, 0, 0x30, 40, 0]);
        let battery = galaxy_battery(&one).unwrap();
        assert_eq!(battery.right, None);
        assert_eq!(battery.case, lvl(40, false));
    }

    #[test]
    fn reads_the_first_galaxy_buds() {
        let mut frame = vec![0xFE, 0x00, 9, 0x60, 0x00, 64, 72, 1, 0, 0x11];
        frame.extend([0x00, 0x00, 0xEE]);
        assert_eq!(
            galaxy_battery(&frame),
            Some(BudsBattery {
                left: lvl(64, false),
                right: lvl(72, false),
                case: None,
            })
        );
        // Other messages, and a frame without its end byte.
        assert_eq!(galaxy_battery(&galaxy_frame(0x40, &[1, 2, 3])), None);
        let mut cut = galaxy_frame(0x60, &[0x0D, 70, 80, 1, 0, 0x11, 55, 0]);
        cut.pop();
        assert_eq!(galaxy_battery(&cut), None);
    }
}
