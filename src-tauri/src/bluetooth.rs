//! Bluetooth device management for the notch's Bluetooth view: paired devices
//! with their type, connection and battery; connecting and disconnecting
//! headphones and speakers; pairing new devices, with PIN prompts answered in
//! Bloom's own UI; and removing devices.
//!
//! Windows has no public "connect this device" call. Audio devices are the
//! exception: their Bluetooth audio driver accepts a one-shot reconnect or
//! disconnect request, the same one the Settings app sends. Other devices
//! (mice, keyboards, phones) connect themselves when they're in use, so they
//! show their status and can be removed, but aren't connected from here.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use windows::core::{Interface, GUID, HSTRING};
use windows::Devices::Bluetooth::{BluetoothDevice, BluetoothLEDevice};
use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DevicePairingKinds,
    DevicePairingRequestedEventArgs, DevicePairingResultStatus, DeviceUnpairingResultStatus,
    DeviceWatcher,
};
use windows::Foundation::{IReference, TypedEventHandler};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_ContainerId;
use windows::Win32::Media::Audio::{
    eAll, IDeviceTopology, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE,
    DEVICE_STATE_ACTIVE, DEVICE_STATE_UNPLUGGED,
};
use windows::Win32::Media::KernelStreaming::{
    IKsControl, KSPROPSETID_BtAudio, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0,
    KSPROPERTY_ONESHOT_DISCONNECT, KSPROPERTY_ONESHOT_RECONNECT, KSPROPERTY_TYPE_GET,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToGUID;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows_collections::IIterable;

const PROP_CONNECTED: &str = "System.Devices.Aep.IsConnected";
const PROP_CONTAINER: &str = "System.Devices.Aep.ContainerId";
/// The battery level Windows shows in Settings, on the device's PnP nodes.
const PROP_BATTERY: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
/// How long Windows waits for the user to answer a pairing prompt.
const PAIRING_ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Serialize)]
pub struct BtDevice {
    pub id: String,
    pub name: String,
    /// "audio", "phone", "computer", "keyboard", "mouse", "gamepad",
    /// "wearable" or "other".
    pub kind: &'static str,
    /// For audio devices, the form their class or appearance reports:
    /// "earbuds", "headphones", "headset", "neckband" or "speaker".
    pub form: Option<&'static str>,
    pub connected: bool,
    /// Percent, when the device reports it.
    pub battery: Option<u8>,
    /// Bloom can connect and disconnect it (headphones and speakers).
    pub can_connect: bool,
}

#[derive(Clone, Serialize)]
pub struct BtConnectionEvent {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub form: Option<&'static str>,
    pub connected: bool,
    pub battery: Option<u8>,
}

fn connection_changes(before: &[BtDevice], after: &[BtDevice]) -> Vec<BtConnectionEvent> {
    let previous: HashMap<&str, &BtDevice> = before.iter().map(|d| (d.id.as_str(), d)).collect();
    after
        .iter()
        .filter_map(|device| {
            // The representative ID can switch between a device's classic
            // and LE endpoints as Windows updates their metadata.
            let old = previous
                .get(device.id.as_str())
                .copied()
                .or_else(|| before.iter().find(|old| old.name == device.name))?;
            let was_connected = old.connected;
            (was_connected != device.connected).then(|| BtConnectionEvent {
                id: device.id.clone(),
                name: device.name.clone(),
                kind: device.kind,
                form: device.form,
                connected: device.connected,
                battery: device.battery,
            })
        })
        .collect()
}

#[derive(Clone, Serialize)]
pub struct BtNearbyDevice {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
}

#[derive(Clone, Serialize)]
pub struct BtPairingRequest {
    pub id: String,
    /// "confirm" (does this PIN match the device?), "display" (type this PIN
    /// on the device) or "provide" (enter the PIN shown by the device).
    pub kind: &'static str,
    pub pin: Option<String>,
}

fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn props(keys: &[&str]) -> IIterable<HSTRING> {
    keys.iter()
        .map(|k| HSTRING::from(*k))
        .collect::<Vec<_>>()
        .into()
}

fn prop<T: windows::core::RuntimeType + 'static>(info: &DeviceInformation, key: &str) -> Option<T> {
    info.Properties()
        .ok()?
        .Lookup(&HSTRING::from(key))
        .ok()?
        .cast::<IReference<T>>()
        .ok()?
        .Value()
        .ok()
}

fn find_all(aqs: &HSTRING, keys: &[&str], kind: DeviceInformationKind) -> Vec<DeviceInformation> {
    let Ok(found) = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(
        aqs,
        &props(keys),
        kind,
    )
    .and_then(|op| op.get()) else {
        return Vec::new();
    };
    (0..found.Size().unwrap_or(0))
        .filter_map(|i| found.GetAt(i).ok())
        .collect()
}

/// Device type from a classic device's class of device: the major class, and
/// for peripherals the keyboard/pointer bits and joystick/gamepad minor class.
fn classic_kind(raw: u32) -> &'static str {
    let major = (raw >> 8) & 0x1F;
    let minor = (raw >> 2) & 0x3F;
    match major {
        1 => "computer",
        2 => "phone",
        4 => "audio",
        5 if minor & 0x10 != 0 => "keyboard",
        5 if minor & 0x20 != 0 => "mouse",
        5 if matches!(minor & 0x0F, 1 | 2) => "gamepad",
        7 => "wearable",
        _ => "other",
    }
}

/// Device type from a Bluetooth LE appearance category (and HID subcategory).
fn le_kind(category: u16, subcategory: u16) -> &'static str {
    match category {
        0x01 => "phone",
        0x02 => "computer",
        0x03 | 0x31 => "wearable",
        0x0F => match subcategory {
            0x01 => "keyboard",
            0x02 => "mouse",
            0x03 | 0x04 => "gamepad",
            _ => "other",
        },
        0x21 | 0x22 | 0x25 | 0x29 => "audio",
        _ => "other",
    }
}

/// The form of a classic audio device from its minor class: headsets and
/// hands-free kits, headphones, or speakers (loudspeaker, portable, hi-fi).
fn classic_form(raw: u32) -> Option<&'static str> {
    let major = (raw >> 8) & 0x1F;
    let minor = (raw >> 2) & 0x3F;
    match (major, minor) {
        (4, 1 | 2) => Some("headset"),
        (4, 6) => Some("headphones"),
        (4, 5 | 7 | 10) => Some("speaker"),
        _ => None,
    }
}

/// The form of an LE audio device from its appearance: a speaker, a
/// wearable audio device's subcategory, or a hearing aid (worn like earbuds).
fn le_form(category: u16, subcategory: u16) -> Option<&'static str> {
    match (category, subcategory) {
        (0x21, _) => Some("speaker"),
        (0x25, 0x01) => Some("earbuds"),
        (0x25, 0x02) => Some("headset"),
        (0x25, 0x03) => Some("headphones"),
        (0x25, 0x04) => Some("neckband"),
        (0x29, _) => Some("earbuds"),
        _ => None,
    }
}

/// A device's kind and, for audio devices, its form.
fn device_type(id: &HSTRING) -> (&'static str, Option<&'static str>) {
    if let Ok(device) = BluetoothDevice::FromIdAsync(id).and_then(|op| op.get()) {
        if let Ok(raw) = device.ClassOfDevice().and_then(|c| c.RawValue()) {
            return (classic_kind(raw), classic_form(raw));
        }
    }
    if let Ok(device) = BluetoothLEDevice::FromIdAsync(id).and_then(|op| op.get()) {
        if let Ok(appearance) = device.Appearance() {
            let category = appearance.Category().unwrap_or(0);
            let subcategory = appearance.SubCategory().unwrap_or(0);
            return (
                le_kind(category, subcategory),
                le_form(category, subcategory),
            );
        }
    }
    ("other", None)
}

fn device_kind(id: &HSTRING) -> &'static str {
    device_type(id).0
}

fn battery_level(container: &GUID) -> Option<u8> {
    let aqs = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container:?}}}\""));
    find_all(&aqs, &[PROP_BATTERY], DeviceInformationKind::Device)
        .iter()
        .find_map(|info| prop::<u8>(info, PROP_BATTERY))
        .filter(|level| *level <= 100)
}

fn selectors(paired: bool) -> Vec<HSTRING> {
    [
        BluetoothDevice::GetDeviceSelectorFromPairingState(paired),
        BluetoothLEDevice::GetDeviceSelectorFromPairingState(paired),
    ]
    .into_iter()
    .filter_map(Result::ok)
    .collect()
}

/// Bluetooth audio driver filters for each device container, so audio devices
/// can be connected and disconnected.
fn audio_filters() -> HashMap<GUID, Vec<String>> {
    let mut filters: HashMap<GUID, Vec<String>> = HashMap::new();
    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return filters;
        };
        let Ok(endpoints) = enumerator.EnumAudioEndpoints(
            eAll,
            DEVICE_STATE(DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_UNPLUGGED.0),
        ) else {
            return filters;
        };
        for i in 0..endpoints.GetCount().unwrap_or(0) {
            let Ok(endpoint) = endpoints.Item(i) else {
                continue;
            };
            let container = endpoint
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_ContainerId))
                .and_then(|value| PropVariantToGUID(&value));
            let Ok(container) = container else {
                continue;
            };
            // The endpoint's first connector leads to the driver filter that
            // owns the Bluetooth audio link.
            let filter_id = endpoint
                .Activate::<IDeviceTopology>(CLSCTX_ALL, None)
                .and_then(|topology| topology.GetConnector(0))
                .and_then(|connector| connector.GetDeviceIdConnectedTo());
            let Ok(filter_id) = filter_id else {
                continue;
            };
            let id = filter_id.to_string().unwrap_or_default();
            CoTaskMemFree(Some(filter_id.0 as *const _));
            let entry = filters.entry(container).or_default();
            if !id.is_empty() && !entry.contains(&id) {
                entry.push(id);
            }
        }
    }
    filters
}

/// Sends a one-shot reconnect or disconnect to a Bluetooth audio filter.
/// Fails for filters that aren't Bluetooth audio.
fn send_oneshot(filter_id: &str, connect: bool) -> windows::core::Result<()> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let filter = enumerator.GetDevice(&HSTRING::from(filter_id))?;
        let control: IKsControl = filter.Activate(CLSCTX_ALL, None)?;
        let request = if connect {
            KSPROPERTY_ONESHOT_RECONNECT
        } else {
            KSPROPERTY_ONESHOT_DISCONNECT
        };
        let property = KSIDENTIFIER {
            Anonymous: KSIDENTIFIER_0 {
                Anonymous: KSIDENTIFIER_0_0 {
                    Set: KSPROPSETID_BtAudio,
                    Id: request.0 as u32,
                    Flags: KSPROPERTY_TYPE_GET,
                },
            },
        };
        let mut returned = 0u32;
        control.KsProperty(
            &property,
            std::mem::size_of::<KSIDENTIFIER>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
        )
    }
}

/// Paired devices' containers, from the last listing, for connect requests.
static CONTAINERS: Mutex<Option<HashMap<String, GUID>>> = Mutex::new(None);

fn list_sync() -> Vec<BtDevice> {
    com_init();
    let audio = audio_filters();
    let mut by_container: HashMap<GUID, BtDevice> = HashMap::new();
    let mut unknown_container = Vec::new();
    let mut containers = HashMap::new();

    for selector in selectors(true) {
        for info in find_all(
            &selector,
            &[PROP_CONNECTED, PROP_CONTAINER],
            DeviceInformationKind::AssociationEndpoint,
        ) {
            let Ok(id) = info.Id() else { continue };
            let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
            let connected = prop::<bool>(&info, PROP_CONNECTED).unwrap_or(false);
            let container = prop::<GUID>(&info, PROP_CONTAINER);
            let (kind, form) = device_type(&id);
            let device = BtDevice {
                id: id.to_string(),
                name,
                kind,
                form,
                connected,
                battery: None,
                can_connect: container.is_some_and(|c| audio.contains_key(&c)),
            };
            match container {
                // A device paired over both classic and LE shows up twice;
                // keep one entry, connected if either link is.
                Some(container) => {
                    containers.insert(device.id.clone(), container);
                    by_container
                        .entry(container)
                        .and_modify(|existing| {
                            existing.connected |= device.connected;
                            existing.form = existing.form.or(device.form);
                            if existing.kind == "other" {
                                existing.kind = device.kind;
                            }
                        })
                        .or_insert(device);
                }
                None => unknown_container.push(device),
            }
        }
    }

    let devices: Vec<BtDevice> = by_container
        .into_iter()
        .map(|(container, mut device)| {
            device.battery = if device.connected {
                battery_level(&container)
            } else {
                None
            };
            device
        })
        .chain(unknown_container)
        .filter(|d| !d.name.is_empty())
        .collect();
    // Windows can file a device's classic and LE links under different
    // containers (phones do this), so entries with the same name are one device.
    let mut merged: Vec<BtDevice> = Vec::with_capacity(devices.len());
    for device in devices {
        match merged.iter_mut().find(|d| d.name == device.name) {
            Some(existing) => {
                existing.connected |= device.connected;
                existing.can_connect |= device.can_connect;
                existing.battery = existing.battery.or(device.battery);
                existing.form = existing.form.or(device.form);
                // Keep the entry whose type is known, as it's the one Windows
                // describes fully.
                if existing.kind == "other" && device.kind != "other" {
                    existing.id = device.id;
                    existing.kind = device.kind;
                }
            }
            None => merged.push(device),
        }
    }
    let mut devices = merged;
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    if let Ok(mut cache) = CONTAINERS.lock() {
        *cache = Some(containers);
    }
    devices
}

#[tauri::command]
pub async fn bluetooth_list_devices() -> Result<Vec<BtDevice>, String> {
    tauri::async_runtime::spawn_blocking(list_sync)
        .await
        .map_err(|e| e.to_string())
}

/// Connects or disconnects a paired audio device.
#[tauri::command]
pub async fn bluetooth_set_connected(id: String, connected: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        com_init();
        let container = CONTAINERS
            .lock()
            .ok()
            .and_then(|cache| cache.as_ref()?.get(&id).copied())
            .ok_or("not-found")?;
        let filters = audio_filters().remove(&container).ok_or("unsupported")?;
        // A headset has a filter per profile (stereo audio, hands-free); a
        // request on any of them moves the whole device.
        let mut last_error = None;
        let mut any_ok = false;
        for filter in &filters {
            match send_oneshot(filter, connected) {
                Ok(()) => {
                    any_ok = true;
                    if connected {
                        break;
                    }
                }
                Err(e) => last_error = Some(e),
            }
        }
        if any_ok {
            Ok(())
        } else {
            Err(last_error
                .map(|e| e.message())
                .unwrap_or_else(|| "unsupported".into()))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Watching ─────────────────────────────────────────────────────────────────

static APP: OnceLock<AppHandle> = OnceLock::new();
/// Keeps the paired-device watchers alive for the app's lifetime.
static PAIRED_WATCHERS: Mutex<Vec<DeviceWatcher>> = Mutex::new(Vec::new());

fn emit<S: Serialize + Clone>(event: &str, payload: S) {
    if let Some(app) = APP.get() {
        let _ = app.emit(event, payload);
    }
}

#[derive(Clone, Serialize)]
pub struct BtBatteryEvent {
    pub id: String,
    pub battery: u8,
}

/// The battery level of a connected device, found by id or, as its id can
/// switch between its classic and LE endpoints, by name.
fn connected_battery(devices: &[BtDevice], id: &str, name: &str) -> Option<u8> {
    devices
        .iter()
        .find(|d| d.id == id)
        .or_else(|| devices.iter().find(|d| d.name == name))
        .filter(|d| d.connected)
        .and_then(|d| d.battery)
}

/// Windows often reports a device's battery a moment after it connects, so
/// a connection reported without one is checked again shortly after, and
/// the level sent as `bluetooth-battery` once it's known.
fn follow_up_battery(id: String, name: String) {
    std::thread::spawn(move || {
        com_init();
        for wait in [1500, 2000] {
            std::thread::sleep(Duration::from_millis(wait));
            if let Some(battery) = connected_battery(&list_sync(), &id, &name) {
                emit("bluetooth-battery", BtBatteryEvent { id, battery });
                return;
            }
        }
    });
}

#[derive(Clone, Serialize)]
pub struct BtBudsEvent {
    pub id: String,
    #[serde(flatten)]
    pub battery: crate::buds::BudsBattery,
}

/// When each device's buds were last read, by name.
static BUDS_READ: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
/// How soon a device's buds can be read again for Battery hub.
const BUDS_REFRESH: Duration = Duration::from_secs(90);

/// Whether a device's buds were read too recently to read again.
fn buds_read_recently(name: &str) -> bool {
    BUDS_READ
        .lock()
        .ok()
        .and_then(|read| read.as_ref()?.get(name).copied())
        .is_some_and(|at| at.elapsed() < BUDS_REFRESH)
}

/// Reads each bud's and the case's battery again for the connected audio
/// devices, for Battery hub; results arrive as `bluetooth-buds`. A device
/// read in the last 90 seconds is skipped.
#[tauri::command]
pub async fn bluetooth_refresh_buds() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        com_init();
        for device in list_sync() {
            if device.connected && device.kind == "audio" && !buds_read_recently(&device.name) {
                follow_up_buds(device.id, device.name);
            }
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Reads each bud's and the case's battery for an audio device that just
/// connected, if it's earbuds that report them, and sends it as
/// `bluetooth-buds`.
fn follow_up_buds(id: String, name: String) {
    if let Ok(mut read) = BUDS_READ.lock() {
        read.get_or_insert_with(HashMap::new)
            .insert(name.clone(), Instant::now());
    }
    std::thread::spawn(move || {
        com_init();
        let Some(container) = CONTAINERS
            .lock()
            .ok()
            .and_then(|cache| cache.as_ref()?.get(&id).copied())
        else {
            return;
        };
        let aqs = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container:?}}}\""));
        let apple_model = find_all(&aqs, &[], DeviceInformationKind::Device)
            .iter()
            .filter_map(|info| info.Id().ok())
            .filter_map(|id| crate::buds::vendor_product(&id.to_string()))
            .find(|(vendor, _)| *vendor == crate::buds::APPLE_VENDOR)
            .map(|(_, product)| product);
        let battery = match apple_model {
            Some(model) => crate::buds::read_apple(model, Duration::from_secs(6)),
            None => {
                // RFCOMM runs over the classic link.
                let classic = CONTAINERS.lock().ok().and_then(|cache| {
                    cache
                        .as_ref()?
                        .iter()
                        .find(|(id, c)| **c == container && id.starts_with("Bluetooth#"))
                        .map(|(id, _)| id.clone())
                });
                classic.and_then(|classic| {
                    crate::buds::read_rfcomm_battery(&classic, &name, Duration::from_secs(4))
                })
            }
        };
        if let Some(battery) = battery.filter(crate::buds::BudsBattery::is_split) {
            emit("bluetooth-buds", BtBudsEvent { id, battery });
        }
    });
}

fn notify_paired_change(tx: &Sender<()>) {
    emit("bluetooth-changed", ());
    let _ = tx.send(());
}

fn watch(
    selector: &HSTRING,
    on_added: impl Fn(&DeviceInformation) + Send + Sync + 'static,
    on_updated: impl Fn(&DeviceInformationUpdate) + Send + Sync + 'static,
    on_removed: impl Fn(&DeviceInformationUpdate) + Send + Sync + 'static,
) -> windows::core::Result<DeviceWatcher> {
    let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
        selector,
        &props(&[PROP_CONNECTED, PROP_CONTAINER]),
        DeviceInformationKind::AssociationEndpoint,
    )?;
    watcher.Added(&TypedEventHandler::new(
        move |_, info: windows::core::Ref<DeviceInformation>| {
            if let Some(info) = info.as_ref() {
                on_added(info);
            }
            Ok(())
        },
    ))?;
    watcher.Updated(&TypedEventHandler::new(
        move |_, update: windows::core::Ref<DeviceInformationUpdate>| {
            if let Some(update) = update.as_ref() {
                on_updated(update);
            }
            Ok(())
        },
    ))?;
    watcher.Removed(&TypedEventHandler::new(
        move |_, update: windows::core::Ref<DeviceInformationUpdate>| {
            if let Some(update) = update.as_ref() {
                on_removed(update);
            }
            Ok(())
        },
    ))?;
    watcher.Start()?;
    Ok(watcher)
}

/// Watches paired devices so the view and the controls tile update when a
/// device connects, disconnects, pairs or is removed.
pub fn setup_bluetooth_notifications(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| {
        com_init();
        // Take a baseline before starting the watchers. Their initial Added
        // callbacks describe devices already connected at startup.
        let baseline = list_sync();
        let (changed_tx, changed_rx) = channel::<()>();
        std::thread::spawn(move || {
            com_init();
            let started = Instant::now();
            let mut previous = baseline;
            while changed_rx.recv().is_ok() {
                // Coalesce classic and LE updates for the same physical device.
                std::thread::sleep(Duration::from_millis(250));
                while changed_rx.try_recv().is_ok() {}
                let current = list_sync();
                if started.elapsed() >= Duration::from_secs(5) {
                    for change in connection_changes(&previous, &current) {
                        let needs_battery = change.connected && change.battery.is_none();
                        let earbuds = change.connected && change.kind == "audio";
                        let (id, name) = (change.id.clone(), change.name.clone());
                        emit("bluetooth-connection", change);
                        if needs_battery {
                            follow_up_battery(id.clone(), name.clone());
                        }
                        if earbuds {
                            follow_up_buds(id, name);
                        }
                    }
                }
                previous = current;
            }
        });
        let mut watchers = Vec::new();
        for selector in selectors(true) {
            let added_tx = changed_tx.clone();
            let updated_tx = changed_tx.clone();
            let removed_tx = changed_tx.clone();
            if let Ok(watcher) = watch(
                &selector,
                move |_| notify_paired_change(&added_tx),
                move |_| notify_paired_change(&updated_tx),
                move |_| notify_paired_change(&removed_tx),
            ) {
                watchers.push(watcher);
            }
        }
        if let Ok(mut kept) = PAIRED_WATCHERS.lock() {
            *kept = watchers;
        }
    });
}

static NEARBY: Mutex<Option<HashMap<String, BtNearbyDevice>>> = Mutex::new(None);
static NEARBY_WATCHERS: Mutex<Vec<DeviceWatcher>> = Mutex::new(Vec::new());

fn emit_nearby() {
    let list = NEARBY
        .lock()
        .ok()
        .and_then(|nearby| {
            let mut list: Vec<_> = nearby.as_ref()?.values().cloned().collect();
            list.sort_by_key(|d| d.name.to_lowercase());
            Some(list)
        })
        .unwrap_or_default();
    emit("bluetooth-nearby", list);
}

/// Starts or stops looking for devices that can be paired. Results arrive as
/// `bluetooth-nearby` events with the full list.
#[tauri::command]
pub async fn bluetooth_discovery(active: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        com_init();
        if let Ok(mut watchers) = NEARBY_WATCHERS.lock() {
            for watcher in watchers.drain(..) {
                let _ = watcher.Stop();
            }
        }
        if let Ok(mut nearby) = NEARBY.lock() {
            *nearby = active.then(HashMap::new);
        }
        if !active {
            return Ok(());
        }
        emit_nearby();
        let mut started = Vec::new();
        for selector in selectors(false) {
            let watcher = watch(
                &selector,
                |info| {
                    let (Ok(id), Ok(name)) = (info.Id(), info.Name()) else {
                        return;
                    };
                    // Unnamed advertisements (beacons, trackers) aren't pairable
                    // in any useful way; Windows' own list hides them too.
                    if name.is_empty() {
                        return;
                    }
                    let device = BtNearbyDevice {
                        id: id.to_string(),
                        name: name.to_string(),
                        kind: device_kind(&id),
                    };
                    if let Ok(mut nearby) = NEARBY.lock() {
                        if let Some(nearby) = nearby.as_mut() {
                            nearby.insert(device.id.clone(), device);
                        }
                    }
                    emit_nearby();
                },
                |_| {},
                |update| {
                    let Ok(id) = update.Id() else { return };
                    if let Ok(mut nearby) = NEARBY.lock() {
                        if let Some(nearby) = nearby.as_mut() {
                            nearby.remove(&id.to_string());
                        }
                    }
                    emit_nearby();
                },
            )
            .map_err(|e| e.message())?;
            started.push(watcher);
        }
        if let Ok(mut watchers) = NEARBY_WATCHERS.lock() {
            *watchers = started;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Pairing ──────────────────────────────────────────────────────────────────

/// Where the view's answer to the current pairing prompt goes: None rejects,
/// Some(pin) accepts (the PIN is only used when the device asked for one).
static PAIRING_ANSWER: Mutex<Option<Sender<Option<String>>>> = Mutex::new(None);

fn answer_request(id: &str, args: &DevicePairingRequestedEventArgs) -> windows::core::Result<()> {
    let kinds = args.PairingKind()?;
    let pin = args
        .Pin()
        .ok()
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty());
    let kind = if kinds == DevicePairingKinds::ConfirmOnly {
        // The user already chose to pair by clicking Pair.
        return args.Accept();
    } else if kinds == DevicePairingKinds::DisplayPin {
        "display"
    } else if kinds == DevicePairingKinds::ConfirmPinMatch {
        "confirm"
    } else if kinds == DevicePairingKinds::ProvidePin {
        "provide"
    } else {
        return Ok(());
    };

    // The handler may block: Windows waits for it (or its deferral) before
    // continuing, and this runs on a thread-pool thread.
    let (tx, rx) = channel();
    if let Ok(mut answer) = PAIRING_ANSWER.lock() {
        *answer = Some(tx);
    }
    emit(
        "bluetooth-pairing-request",
        BtPairingRequest {
            id: id.to_string(),
            kind,
            pin,
        },
    );
    if kind == "display" {
        // Nothing to answer: the user types the PIN on the device itself.
        return args.Accept();
    }
    match rx.recv_timeout(PAIRING_ANSWER_TIMEOUT) {
        Ok(Some(pin)) if kind == "provide" => args.AcceptWithPin(&HSTRING::from(pin)),
        Ok(Some(_)) => args.Accept(),
        _ => Ok(()),
    }
}

fn pairing_error(status: DevicePairingResultStatus) -> String {
    match status {
        DevicePairingResultStatus::AuthenticationTimeout => "timeout".into(),
        DevicePairingResultStatus::AuthenticationFailure
        | DevicePairingResultStatus::RejectedByHandler
        | DevicePairingResultStatus::PairingCanceled
        | DevicePairingResultStatus::OperationAlreadyInProgress => "cancelled".into(),
        DevicePairingResultStatus::ConnectionRejected
        | DevicePairingResultStatus::NoSupportedProfiles
        | DevicePairingResultStatus::Failed => "failed".into(),
        DevicePairingResultStatus::NotReadyToPair => "not-ready".into(),
        other => format!("Couldn't pair ({})", other.0),
    }
}

/// Pairs with a nearby device. PIN prompts arrive as
/// `bluetooth-pairing-request` events, answered with bluetooth_pairing_respond.
#[tauri::command]
pub async fn bluetooth_pair(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        com_init();
        let info = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id.as_str()))
            .and_then(|op| op.get())
            .map_err(|e| e.message())?;
        let custom = info
            .Pairing()
            .and_then(|p| p.Custom())
            .map_err(|e| e.message())?;
        let handler_id = id.clone();
        let token = custom
            .PairingRequested(&TypedEventHandler::new(
                move |_, args: windows::core::Ref<DevicePairingRequestedEventArgs>| {
                    if let Some(args) = args.as_ref() {
                        answer_request(&handler_id, args)?;
                    }
                    Ok(())
                },
            ))
            .map_err(|e| e.message())?;
        let kinds = DevicePairingKinds(
            DevicePairingKinds::ConfirmOnly.0
                | DevicePairingKinds::DisplayPin.0
                | DevicePairingKinds::ProvidePin.0
                | DevicePairingKinds::ConfirmPinMatch.0,
        );
        let result = custom.PairAsync(kinds).and_then(|op| op.get());
        let _ = custom.RemovePairingRequested(token);
        if let Ok(mut answer) = PAIRING_ANSWER.lock() {
            *answer = None;
        }
        let status = result.and_then(|r| r.Status()).map_err(|e| e.message())?;
        if status == DevicePairingResultStatus::Paired
            || status == DevicePairingResultStatus::AlreadyPaired
        {
            emit("bluetooth-changed", ());
            Ok(())
        } else {
            Err(pairing_error(status))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Answers the pending pairing prompt.
#[tauri::command]
pub fn bluetooth_pairing_respond(accept: bool, pin: Option<String>) {
    if let Ok(mut answer) = PAIRING_ANSWER.lock() {
        if let Some(tx) = answer.take() {
            let _ = tx.send(accept.then(|| pin.unwrap_or_default()));
        }
    }
}

/// Removes a paired device.
#[tauri::command]
pub async fn bluetooth_unpair(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        com_init();
        let status = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id.as_str()))
            .and_then(|op| op.get())
            .and_then(|info| info.Pairing())
            .and_then(|pairing| pairing.UnpairAsync())
            .and_then(|op| op.get())
            .and_then(|result| result.Status())
            .map_err(|e| e.message())?;
        if status == DeviceUnpairingResultStatus::Unpaired
            || status == DeviceUnpairingResultStatus::AlreadyUnpaired
        {
            emit("bluetooth-changed", ());
            Ok(())
        } else {
            Err(format!("Couldn't remove the device ({})", status.0))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_device(id: &str, connected: bool) -> BtDevice {
        BtDevice {
            id: id.into(),
            name: id.into(),
            kind: "audio",
            form: None,
            connected,
            battery: Some(72),
            can_connect: true,
        }
    }

    #[test]
    fn reports_only_existing_device_connection_transitions() {
        let before = [sample_device("a", false), sample_device("b", true)];
        let after = [
            sample_device("a", true),
            sample_device("b", true),
            sample_device("c", true),
        ];
        let changes = connection_changes(&before, &after);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id, "a");
        assert!(changes[0].connected);
        assert_eq!(changes[0].battery, Some(72));

        let changes = connection_changes(&after, &before);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id, "a");
        assert!(!changes[0].connected);

        let mut renamed_endpoint = sample_device("new-le-id", true);
        renamed_endpoint.name = "Headphones".into();
        let mut old_endpoint = sample_device("old-classic-id", true);
        old_endpoint.name = "Headphones".into();
        assert!(connection_changes(&[old_endpoint], &[renamed_endpoint]).is_empty());
    }

    #[test]
    fn maps_classic_classes() {
        // Major class in bits 8-12, minor in bits 2-7.
        assert_eq!(classic_kind(0x24_0404), "audio");
        assert_eq!(classic_kind(0x5A_020C), "phone");
        assert_eq!(classic_kind(0x00_0540), "keyboard");
        assert_eq!(classic_kind(0x00_0580), "mouse");
        assert_eq!(classic_kind(0x00_0508), "gamepad");
        assert_eq!(classic_kind(0x00_010C), "computer");
        assert_eq!(classic_kind(0x00_0000), "other");
    }

    #[test]
    fn maps_le_appearance() {
        assert_eq!(le_kind(0x0F, 0x01), "keyboard");
        assert_eq!(le_kind(0x0F, 0x02), "mouse");
        assert_eq!(le_kind(0x03, 0), "wearable");
        assert_eq!(le_kind(0x25, 0), "audio");
        assert_eq!(le_kind(0x00, 0), "other");
    }

    #[test]
    fn finds_a_connected_device_battery_by_id_or_name() {
        let mut le = sample_device("le-id", true);
        le.name = "Headphones".into();
        le.battery = Some(64);
        let mut off = sample_device("off", false);
        off.battery = Some(10);
        let devices = [le, off];
        assert_eq!(connected_battery(&devices, "le-id", "x"), Some(64));
        // The id moved to the other endpoint: found by name.
        assert_eq!(
            connected_battery(&devices, "classic-id", "Headphones"),
            Some(64)
        );
        // Disconnected or unknown devices report nothing.
        assert_eq!(connected_battery(&devices, "off", "off"), None);
        assert_eq!(connected_battery(&devices, "nope", "nope"), None);
    }

    #[test]
    fn maps_audio_forms() {
        // Audio/video major class (4) with the headset, headphones,
        // loudspeaker and car audio minor classes.
        assert_eq!(classic_form(0x24_0404), Some("headset"));
        assert_eq!(classic_form(0x24_0418), Some("headphones"));
        assert_eq!(classic_form(0x24_0414), Some("speaker"));
        assert_eq!(classic_form(0x24_0420), None);
        assert_eq!(classic_form(0x5A_020C), None);
        assert_eq!(le_form(0x25, 0x01), Some("earbuds"));
        assert_eq!(le_form(0x25, 0x04), Some("neckband"));
        assert_eq!(le_form(0x21, 0x00), Some("speaker"));
        assert_eq!(le_form(0x0F, 0x01), None);
    }
}
