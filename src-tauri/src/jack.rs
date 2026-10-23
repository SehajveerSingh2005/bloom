//! Headphones and earphones plugged into the computer's 3.5 mm jack, for the
//! notch's device popups and its Connected devices panel.
//!
//! A jack has no device of its own to watch: the audio driver flips the
//! computer's "Headphones" output (and, for earphones with a mic, its
//! "Microphone" input) between unplugged and active. Windows reports those
//! state changes to an endpoint notification client; on each one the jack's
//! state is read again and a change is sent as `wired-audio-connection`.
//!
//! Windows files what's plugged into a jack under a device container of its
//! own, holding nothing but the jack's endpoints. Outputs of Bluetooth and
//! USB devices are in containers that also hold the device's hardware; those
//! are left to their own watchers.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use windows::core::{implement, Interface, GUID, HSTRING, PCWSTR};
use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationKind};
use windows::Win32::Devices::FunctionDiscovery::{
    PKEY_DeviceInterface_FriendlyName, PKEY_Device_ContainerId, PKEY_Device_DeviceDesc,
};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eCapture, eRender, EDataFlow, ERole, IMMDevice, IMMDeviceEnumerator, IMMEndpoint,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    PKEY_AudioEndpoint_FormFactor, DEVICE_STATE, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantToGUID, PropVariantToStringAlloc, PropVariantToUInt32,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows_collections::IIterable;

/// The container Windows puts the computer's own devices in.
const MACHINE_CONTAINER: GUID = GUID::from_u128(0x00000000_0000_0000_ffff_ffffffffffff);
/// How long state changes are gathered before the jack is read again: a
/// headset's output and mic flip a moment apart.
const GATHER: Duration = Duration::from_millis(700);

// Endpoint form factors (EndpointFormFactor).
const HEADPHONES: u32 = 3;
const MICROPHONE: u32 = 4;
const HEADSET: u32 = 5;

/// What's plugged into the jack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Jack {
    /// Headphones or earphones without a mic.
    Headphones,
    /// Earphones or a headset with a mic (a four-pole plug).
    Headset,
}

impl Jack {
    fn class(self) -> &'static str {
        match self {
            Jack::Headphones => "jack-headphones",
            Jack::Headset => "jack-headset",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Jack::Headphones => "Wired headphones",
            Jack::Headset => "Wired earphones",
        }
    }
}

/// A device plugged into or out of the jack, as sent in
/// `wired-audio-connection`; the same shape as `usb-connection`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WiredAudioConnection {
    pub id: &'static str,
    pub name: &'static str,
    /// "jack-headphones" or "jack-headset".
    pub class: &'static str,
    pub connected: bool,
}

/// One of the computer's own audio endpoints, as far as the jack goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub render: bool,
    pub form_factor: u32,
    /// The endpoint's name ("Headphones", "Microphone Array").
    pub name: String,
    /// The audio adapter it belongs to ("Realtek(R) Audio").
    pub adapter: String,
}

/// What the computer's active endpoints say is in the jack: an active
/// headphones or headset output means something's plugged in, and an active
/// mic input on the same adapter means it has a mic. Built-in microphone
/// arrays are always active, so they don't count.
pub fn jack_state(active: &[Endpoint]) -> Option<Jack> {
    let output = active
        .iter()
        .find(|e| e.render && matches!(e.form_factor, HEADPHONES | HEADSET))?;
    let mic = active.iter().any(|e| {
        !e.render
            && matches!(e.form_factor, MICROPHONE | HEADSET)
            && e.adapter == output.adapter
            && !e.name.to_ascii_lowercase().contains("array")
    });
    Some(if mic || output.form_factor == HEADSET {
        Jack::Headset
    } else {
        Jack::Headphones
    })
}

fn string_prop(device: &IMMDevice, key: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let value = device
            .OpenPropertyStore(STGM_READ)
            .ok()?
            .GetValue(key)
            .ok()?;
        let text = PropVariantToStringAlloc(&value).ok()?;
        let result = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const _));
        result
    }
}

/// Whether the devices in an endpoint's container are only audio endpoints
/// (`SWD\MMDEVAPI\...`): what's plugged into a jack has no hardware of its
/// own. A Bluetooth or USB device's container holds its hardware too.
pub fn is_plug_container(device_ids: &[String]) -> bool {
    device_ids
        .iter()
        .all(|id| id.to_ascii_uppercase().starts_with("SWD\\MMDEVAPI\\"))
}

/// Containers already checked, by id: whether they're the jack's.
static JACK_CONTAINERS: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

/// Whether an endpoint in this container is the computer's own: in its
/// container, or (for what's plugged into a jack, which Windows treats as a
/// device of its own) in a container with no hardware.
fn is_jack_container(container: &GUID) -> bool {
    if *container == MACHINE_CONTAINER {
        return true;
    }
    let key = format!("{container:?}");
    if let Some(known) = JACK_CONTAINERS
        .lock()
        .ok()
        .and_then(|c| c.as_ref()?.get(&key).copied())
    {
        return known;
    }
    let aqs = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{key}}}\""));
    let no_props: IIterable<HSTRING> = Vec::<HSTRING>::new().into();
    let ids: Option<Vec<String>> =
        DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(
            &aqs,
            &no_props,
            DeviceInformationKind::Device,
        )
        .and_then(|op| op.get())
        .ok()
        .map(|found| {
            (0..found.Size().unwrap_or(0))
                .filter_map(|i| found.GetAt(i).ok())
                .filter_map(|d| d.Id().ok())
                .map(|id| id.to_string())
                .collect()
        });
    // If the lookup fails, leave the endpoint out rather than guess.
    let jack = ids.is_some_and(|ids| is_plug_container(&ids));
    if let Ok(mut cache) = JACK_CONTAINERS.lock() {
        cache.get_or_insert_with(HashMap::new).insert(key, jack);
    }
    jack
}

/// The computer's own active audio endpoints, including what's plugged into
/// its jack. Bluetooth and USB outputs are left out.
fn active_endpoints() -> Vec<Endpoint> {
    unsafe {
        let Ok(enumerator) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return Vec::new();
        };
        let mut endpoints = Vec::new();
        for flow in [eRender, eCapture] {
            let Ok(devices) = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else {
                continue;
            };
            for i in 0..devices.GetCount().unwrap_or(0) {
                let Ok(device) = devices.Item(i) else {
                    continue;
                };
                let Ok(store) = device.OpenPropertyStore(STGM_READ) else {
                    continue;
                };
                let Ok(container) = store
                    .GetValue(&PKEY_Device_ContainerId)
                    .and_then(|v| PropVariantToGUID(&v))
                else {
                    continue;
                };
                if !is_jack_container(&container) {
                    continue;
                }
                let form_factor = store
                    .GetValue(&PKEY_AudioEndpoint_FormFactor)
                    .and_then(|v| PropVariantToUInt32(&v))
                    .unwrap_or(0);
                let render = device
                    .cast::<IMMEndpoint>()
                    .and_then(|e| e.GetDataFlow())
                    .map(|f| f == eRender)
                    .unwrap_or(flow == eRender);
                endpoints.push(Endpoint {
                    render,
                    form_factor,
                    name: string_prop(&device, &PKEY_Device_DeviceDesc).unwrap_or_default(),
                    adapter: string_prop(&device, &PKEY_DeviceInterface_FriendlyName)
                        .unwrap_or_default(),
                });
            }
        }
        endpoints
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();
/// The jack's last known state.
static STATE: Mutex<Option<Jack>> = Mutex::new(None);
/// What's plugged into the jack now, for the Connected devices panel.
#[tauri::command]
pub fn jack_list_devices() -> Vec<WiredAudioConnection> {
    STATE
        .lock()
        .ok()
        .and_then(|s| *s)
        .map(|jack| WiredAudioConnection {
            id: "jack",
            name: jack.name(),
            class: jack.class(),
            connected: true,
        })
        .into_iter()
        .collect()
}

#[implement(IMMNotificationClient)]
struct Client {
    tx: Mutex<Sender<()>>,
}

impl Client {
    fn changed(&self) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(());
        }
    }
}

impl IMMNotificationClient_Impl for Client_Impl {
    fn OnDeviceStateChanged(
        &self,
        _id: &PCWSTR,
        _state: DEVICE_STATE,
    ) -> windows::core::Result<()> {
        self.changed();
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        self.changed();
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        self.changed();
        Ok(())
    }
    fn OnDefaultDeviceChanged(
        &self,
        _flow: EDataFlow,
        _role: ERole,
        _id: &PCWSTR,
    ) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnPropertyValueChanged(
        &self,
        _id: &PCWSTR,
        _key: &PROPERTYKEY,
    ) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Watches the 3.5 mm jack and emits `wired-audio-connection` when headphones
/// or earphones are plugged in or out. What's plugged in at startup is only
/// recorded, for the Connected devices panel.
pub fn setup_jack_notifications(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        if let Ok(mut state) = STATE.lock() {
            *state = jack_state(&active_endpoints());
        }
        let (tx, rx) = channel::<()>();
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            while rx.recv().is_ok() {
                std::thread::sleep(GATHER);
                while rx.try_recv().is_ok() {}
                let now = jack_state(&active_endpoints());
                let before = STATE
                    .lock()
                    .ok()
                    .and_then(|mut s| std::mem::replace(&mut *s, now));
                for change in jack_changes(before, now) {
                    if let Some(app) = APP.get() {
                        let _ = app.emit("wired-audio-connection", change);
                    }
                }
            }
        });
        unsafe {
            let Ok(enumerator) =
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                return;
            };
            let client: IMMNotificationClient = Client { tx: Mutex::new(tx) }.into();
            if enumerator
                .RegisterEndpointNotificationCallback(&client)
                .is_err()
            {
                return;
            }
            // The client stays registered while the enumerator and client
            // live; COM objects can't be kept in a static, so this thread
            // holds them for the app's lifetime.
            let _kept = (enumerator, client);
            loop {
                std::thread::park();
            }
        }
    });
}

/// The events for the jack going from one state to another: a plug-in, a
/// removal, or both when headphones are swapped for a headset.
fn jack_changes(before: Option<Jack>, now: Option<Jack>) -> Vec<WiredAudioConnection> {
    if before == now {
        return Vec::new();
    }
    let event = |jack: Jack, connected| WiredAudioConnection {
        id: "jack",
        name: jack.name(),
        class: jack.class(),
        connected,
    };
    before
        .map(|jack| event(jack, false))
        .into_iter()
        .chain(now.map(|jack| event(jack, true)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(render: bool, form_factor: u32, adapter: &str) -> Endpoint {
        let name = match (render, form_factor) {
            (true, HEADPHONES) => "Headphones",
            (true, _) => "Speakers",
            (false, _) if adapter.contains("Intel") => "Microphone Array",
            (false, _) => "Microphone",
        };
        Endpoint {
            render,
            form_factor,
            name: name.into(),
            adapter: adapter.into(),
        }
    }

    #[test]
    fn reads_the_jack_from_the_active_endpoints() {
        let speakers = endpoint(true, 1, "Realtek(R) Audio");
        let mic_array = endpoint(false, MICROPHONE, "Intel Smart Sound Technology");
        // Nothing plugged in: only the speakers and the built-in mic array.
        assert_eq!(jack_state(&[speakers.clone(), mic_array.clone()]), None);
        // Headphones: the jack's output turns active.
        let headphones = endpoint(true, HEADPHONES, "Realtek(R) Audio");
        assert_eq!(
            jack_state(&[speakers.clone(), headphones.clone(), mic_array.clone()]),
            Some(Jack::Headphones)
        );
        // A headset output counts as having a mic.
        let headset = endpoint(true, HEADSET, "Realtek(R) Audio");
        assert_eq!(jack_state(&[headset]), Some(Jack::Headset));
    }

    #[test]
    fn tells_the_jacks_containers_from_devices() {
        // The jack's: no hardware, or only its endpoints.
        assert!(is_plug_container(&[]));
        assert!(is_plug_container(&[
            r"SWD\MMDEVAPI\{0.0.0.00000000}.{8cc2c256-3041-45ab-876a-323d70954d44}".into()
        ]));
        // A USB audio adapter and a Bluetooth headset bring their hardware.
        assert!(!is_plug_container(&[
            r"SWD\MMDEVAPI\{0.0.0.00000000}.{1f15b429-97d1-4558-aa07-f2c6e46d1b94}".into(),
            r"USB\VID_0020&PID_0B21\20210726905926".into()
        ]));
        assert!(!is_plug_container(&[
            r"BTHENUM\DEV_3E781D8557F0\7&1E98AE4D&0&BLUETOOTHDEVICE_3E781D8557F0".into()
        ]));
    }

    #[test]
    fn earphones_with_a_mic_turn_on_the_jacks_mic_too() {
        // Realtek's jack: a Headphones output and a Microphone input.
        let headphones = endpoint(true, HEADPHONES, "Realtek(R) Audio");
        let jack_mic = endpoint(false, MICROPHONE, "Realtek(R) Audio");
        assert_eq!(jack_state(&[headphones, jack_mic]), Some(Jack::Headset));
    }

    #[test]
    fn a_mic_on_another_adapter_is_not_the_jacks() {
        let headphones = endpoint(true, HEADPHONES, "Realtek(R) Audio");
        // The built-in mic array belongs to another adapter.
        let mic_array = endpoint(false, MICROPHONE, "Intel Smart Sound Technology");
        assert_eq!(
            jack_state(&[headphones.clone(), mic_array]),
            Some(Jack::Headphones)
        );
    }

    #[test]
    fn reports_plug_ins_removals_and_swaps() {
        assert_eq!(jack_changes(None, None), vec![]);
        assert_eq!(
            jack_changes(Some(Jack::Headset), Some(Jack::Headset)),
            vec![]
        );
        let plugged = jack_changes(None, Some(Jack::Headphones));
        assert_eq!(plugged.len(), 1);
        assert!(plugged[0].connected);
        assert_eq!(plugged[0].class, "jack-headphones");
        let removed = jack_changes(Some(Jack::Headset), None);
        assert_eq!(removed.len(), 1);
        assert!(!removed[0].connected);
        let swapped = jack_changes(Some(Jack::Headphones), Some(Jack::Headset));
        assert_eq!(swapped.len(), 2);
        assert!(!swapped[0].connected && swapped[1].connected);
    }
}
