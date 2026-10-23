//! USB device notifications for the notch's device popups: drives, webcams,
//! audio outputs (wired earbuds and headphones) and microphones that are
//! plugged in or removed.
//!
//! Each kind is watched through its device interfaces. Only devices in their
//! own USB container count: a laptop's built-in camera and drives are USB
//! too, but they belong to the computer's container. One physical device
//! often exposes several interfaces at once (a webcam has a microphone), so
//! arrivals are gathered for a moment and reported once per device, as its
//! most telling interface. Devices present at startup aren't reported.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows::core::{IInspectable, Interface, GUID, HSTRING};
use windows::Devices::Enumeration::{
    DeviceClass, DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DeviceWatcher,
};
use windows::Foundation::{IReference, TypedEventHandler};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows_collections::IIterable;

const PROP_CONTAINER: &str = "System.Devices.ContainerId";
/// The container Windows puts the computer's own devices in.
const MACHINE_CONTAINER: GUID = GUID::from_u128(0x00000000_0000_0000_ffff_ffffffffffff);
/// Enabled disk interfaces (GUID_DEVINTERFACE_DISK). An ejected drive's
/// interface is disabled, so it leaves this selector as it's ejected.
const DISK_SELECTOR: &str = "System.Devices.InterfaceClassGuid:=\"{53f56307-b6bf-11d0-94f2-00a0c91efb8b}\" AND System.Devices.InterfaceEnabled:=System.StructuredQueryType.Boolean#True";
/// How long arrivals are gathered before they're reported.
const GATHER: Duration = Duration::from_millis(700);

/// A USB device plugged in or removed, as sent in `usb-connection`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UsbConnection {
    /// The device's container, the same for all of its interfaces.
    pub id: String,
    pub name: String,
    /// "camera", "audio-out", "disk" or "audio-in".
    pub class: &'static str,
    pub connected: bool,
}

/// How telling an interface is about what the device is: a webcam's
/// microphone is a webcam, a headset's microphone is the headset.
fn rank(class: &str) -> u8 {
    match class {
        "camera" => 0,
        "audio-out" => 1,
        "disk" => 2,
        _ => 3,
    }
}

/// One report per device and direction, as its most telling interface, in
/// the order the devices first appeared.
fn pick(raw: Vec<UsbConnection>) -> Vec<UsbConnection> {
    let mut picked: Vec<UsbConnection> = Vec::new();
    for item in raw {
        match picked
            .iter_mut()
            .find(|p| p.id == item.id && p.connected == item.connected)
        {
            Some(existing) if rank(item.class) < rank(existing.class) => *existing = item,
            Some(_) => {}
            None => picked.push(item),
        }
    }
    picked
}

static APP: OnceLock<AppHandle> = OnceLock::new();
/// Keeps the watchers alive for the app's lifetime.
static WATCHERS: Mutex<Vec<DeviceWatcher>> = Mutex::new(Vec::new());
/// Each watcher's state, for listing what's plugged in now.
static WATCHES: Mutex<Vec<Arc<Watch>>> = Mutex::new(Vec::new());

/// The USB devices plugged in now, one per device as its most telling
/// interface, like the `usb-connection` events. Includes devices that were
/// already plugged in at startup.
#[tauri::command]
pub fn usb_list_devices() -> Vec<UsbConnection> {
    let Ok(watches) = WATCHES.lock() else {
        return Vec::new();
    };
    let present = watches
        .iter()
        .flat_map(|watch| {
            let known = watch.known.lock().map(|k| k.clone()).unwrap_or_default();
            known.into_values().map(|(id, name)| UsbConnection {
                id,
                name,
                class: watch.class,
                connected: true,
            })
        })
        .collect();
    pick(present)
}

fn props(keys: &[&str]) -> IIterable<HSTRING> {
    keys.iter()
        .map(|k| HSTRING::from(*k))
        .collect::<Vec<_>>()
        .into()
}

fn container_of(info: &DeviceInformation) -> Option<GUID> {
    info.Properties()
        .ok()?
        .Lookup(&HSTRING::from(PROP_CONTAINER))
        .ok()?
        .cast::<IReference<GUID>>()
        .ok()?
        .Value()
        .ok()
}

/// Whether a container holds a USB device: its devices' instance ids start
/// with USB\ (or USBSTOR\ for storage).
fn container_is_usb(container: &GUID) -> bool {
    let aqs = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container:?}}}\""));
    let Ok(found) = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(
        &aqs,
        &props(&[]),
        DeviceInformationKind::Device,
    )
    .and_then(|op| op.get()) else {
        return false;
    };
    (0..found.Size().unwrap_or(0))
        .filter_map(|i| found.GetAt(i).ok())
        .filter_map(|d| d.Id().ok())
        .any(|id| {
            let id = id.to_string().to_ascii_uppercase();
            id.starts_with("USB\\") || id.starts_with("USBSTOR\\")
        })
}

/// What one watcher shares with its callbacks.
struct Watch {
    class: &'static str,
    /// Interfaces seen, by id: their container and name, for removals.
    known: Mutex<HashMap<String, (String, String)>>,
    /// Set once the initial enumeration is done; arrivals before it are the
    /// devices already plugged in.
    ready: AtomicBool,
    usb_containers: Arc<Mutex<HashMap<String, bool>>>,
    tx: Mutex<Sender<UsbConnection>>,
}

impl Watch {
    fn added(&self, info: &DeviceInformation) {
        let Ok(id) = info.Id() else { return };
        let Some(container) = container_of(info) else {
            return;
        };
        if container == MACHINE_CONTAINER {
            return;
        }
        let key = format!("{container:?}");
        let usb = {
            let cached = self
                .usb_containers
                .lock()
                .ok()
                .and_then(|c| c.get(&key).copied());
            match cached {
                Some(usb) => usb,
                None => {
                    let usb = container_is_usb(&container);
                    if let Ok(mut c) = self.usb_containers.lock() {
                        c.insert(key.clone(), usb);
                    }
                    usb
                }
            }
        };
        if !usb {
            return;
        }
        let name = info.Name().map(|n| n.to_string()).unwrap_or_default();
        if let Ok(mut known) = self.known.lock() {
            known.insert(id.to_string(), (key.clone(), name.clone()));
        }
        if self.ready.load(Ordering::Acquire) {
            self.send(key, name, true);
        }
    }

    fn removed(&self, update: &DeviceInformationUpdate) {
        let Ok(id) = update.Id() else { return };
        let entry = self
            .known
            .lock()
            .ok()
            .and_then(|mut k| k.remove(&id.to_string()));
        if let Some((key, name)) = entry {
            if self.ready.load(Ordering::Acquire) {
                self.send(key, name, false);
            }
        }
    }

    fn send(&self, id: String, name: String, connected: bool) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(UsbConnection {
                id,
                name,
                class: self.class,
                connected,
            });
        }
    }
}

fn watch(selector: &HSTRING, watch: Arc<Watch>) -> windows::core::Result<DeviceWatcher> {
    let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
        selector,
        &props(&[PROP_CONTAINER]),
        DeviceInformationKind::DeviceInterface,
    )?;
    let on_added = watch.clone();
    watcher.Added(&TypedEventHandler::new(
        move |_, info: windows::core::Ref<DeviceInformation>| {
            if let Some(info) = info.as_ref() {
                on_added.added(info);
            }
            Ok(())
        },
    ))?;
    let on_removed = watch.clone();
    watcher.Removed(&TypedEventHandler::new(
        move |_, update: windows::core::Ref<DeviceInformationUpdate>| {
            if let Some(update) = update.as_ref() {
                on_removed.removed(update);
            }
            Ok(())
        },
    ))?;
    // Interface properties can change without an arrival or removal; the
    // watcher needs an Updated handler to report the others.
    watcher.Updated(&TypedEventHandler::new(
        |_, _: windows::core::Ref<DeviceInformationUpdate>| Ok(()),
    ))?;
    let on_done = watch;
    watcher.EnumerationCompleted(&TypedEventHandler::new(
        move |_, _: windows::core::Ref<IInspectable>| {
            on_done.ready.store(true, Ordering::Release);
            Ok(())
        },
    ))?;
    watcher.Start()?;
    Ok(watcher)
}

/// Watches USB drives, webcams, audio outputs and microphones and emits
/// `usb-connection` when one is plugged in or removed.
pub fn setup_usb_notifications(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let (tx, rx) = channel::<UsbConnection>();
        std::thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                std::thread::sleep(GATHER);
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                for change in pick(batch) {
                    if let Some(app) = APP.get() {
                        let _ = app.emit("usb-connection", change);
                    }
                }
            }
        });

        let usb_containers = Arc::new(Mutex::new(HashMap::new()));
        let selector = |class: DeviceClass| {
            DeviceInformation::GetAqsFilterFromDeviceClass(class).unwrap_or_default()
        };
        let kinds: [(&'static str, HSTRING); 4] = [
            ("disk", HSTRING::from(DISK_SELECTOR)),
            ("camera", selector(DeviceClass::VideoCapture)),
            ("audio-out", selector(DeviceClass::AudioRender)),
            ("audio-in", selector(DeviceClass::AudioCapture)),
        ];
        let mut watchers = Vec::new();
        for (class, aqs) in kinds {
            let state = Arc::new(Watch {
                class,
                known: Mutex::new(HashMap::new()),
                ready: AtomicBool::new(false),
                usb_containers: usb_containers.clone(),
                tx: Mutex::new(tx.clone()),
            });
            if let Ok(mut watches) = WATCHES.lock() {
                watches.push(state.clone());
            }
            if let Ok(watcher) = watch(&aqs, state) {
                watchers.push(watcher);
            }
        }
        if let Ok(mut kept) = WATCHERS.lock() {
            *kept = watchers;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: &str, class: &'static str, connected: bool) -> UsbConnection {
        UsbConnection {
            id: id.into(),
            name: id.into(),
            class,
            connected,
        }
    }

    #[test]
    fn reports_each_device_once_as_its_most_telling_interface() {
        let picked = pick(vec![
            conn("webcam", "audio-in", true),
            conn("drive-a", "disk", true),
            conn("webcam", "camera", true),
            conn("drive-b", "disk", true),
            conn("headset", "audio-in", true),
            conn("headset", "audio-out", true),
        ]);
        assert_eq!(
            picked,
            vec![
                conn("webcam", "camera", true),
                conn("drive-a", "disk", true),
                conn("drive-b", "disk", true),
                conn("headset", "audio-out", true),
            ]
        );
    }

    #[test]
    fn keeps_a_plug_and_an_unplug_of_the_same_device_apart() {
        let picked = pick(vec![
            conn("drive", "disk", false),
            conn("drive", "disk", true),
        ]);
        assert_eq!(picked.len(), 2);
    }
}
