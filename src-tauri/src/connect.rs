// Wi-Fi networks and Bluetooth devices, for the Wi-Fi and Bluetooth pages of
// the controls (src/ConnectPage.tsx): scan, join (with a password), pair
// (with PIN prompts), connect, disconnect and forget, without Windows
// Settings.
//
// Windows: the WinRT Wi-Fi and device-pairing APIs; saved networks come from
// the WLAN API. Audio devices connect the way Settings does it, with the
// Bluetooth audio driver's one-shot reconnect. Linux: nmcli and bluetoothctl.

use serde::Serialize;
use tauri::{AppHandle, WebviewWindow};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0…4.
    pub bars: u8,
    pub secure: bool,
    /// Needs a username (WPA-Enterprise): not joinable from here.
    pub enterprise: bool,
    pub connected: bool,
    /// A saved network: joins without asking for the password.
    pub saved: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BtDevice {
    pub id: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    /// "audio", "input", "phone", "computer" or "other": picks the icon, and
    /// only audio devices can be connected from here (the rest connect
    /// themselves when they wake).
    pub kind: String,
}

/// How a join went: "connected", or "password" when the network wants a
/// (new) password. Anything else is an error.
pub type JoinResult = Result<String, String>;

/// Joining keeps the strongest of duplicate SSIDs (several access points).
fn dedupe(mut list: Vec<WifiNetwork>) -> Vec<WifiNetwork> {
    list.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.bars.cmp(&a.bars)));
    let mut seen = std::collections::HashSet::new();
    list.retain(|n| !n.ssid.is_empty() && seen.insert(n.ssid.clone()));
    list
}

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;
    use std::sync::{mpsc, Mutex};
    use std::time::Duration;

    use tauri::{AppHandle, Emitter};
    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Devices::Enumeration::{
        DeviceInformation, DeviceInformationCustomPairing, DeviceInformationKind,
        DeviceInformationUpdate, DevicePairingKinds, DevicePairingRequestedEventArgs,
        DevicePairingResultStatus, DeviceUnpairingResultStatus, DeviceWatcher,
    };
    use windows::Devices::WiFi::{
        WiFiAccessStatus, WiFiAdapter, WiFiAvailableNetwork, WiFiConnectionStatus,
        WiFiReconnectionKind,
    };
    use windows::Foundation::{IPropertyValue, TypedEventHandler};
    use windows::Networking::Connectivity::NetworkAuthenticationType as Auth;
    use windows::Security::Credentials::PasswordCredential;

    use super::{dedupe, BtDevice, JoinResult, WifiNetwork};

    fn com() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            );
        }
    }

    fn err(e: windows::core::Error) -> String {
        e.message().to_string()
    }

    // ── Wi-Fi ────────────────────────────────────────────────────────────────

    fn adapter() -> Result<WiFiAdapter, String> {
        com();
        let access = WiFiAdapter::RequestAccessAsync()
            .and_then(|o| o.get())
            .map_err(err)?;
        if access != WiFiAccessStatus::Allowed {
            return Err(
                "Windows didn't allow Bloom to see Wi-Fi networks. Turn on Location \
                        and \"Let desktop apps access your location\" in Windows privacy settings."
                    .into(),
            );
        }
        let adapters = WiFiAdapter::FindAllAdaptersAsync()
            .and_then(|o| o.get())
            .map_err(err)?;
        adapters
            .GetAt(0)
            .map_err(|_| "No Wi-Fi adapter.".to_string())
    }

    fn connected_ssid(a: &WiFiAdapter) -> Option<String> {
        let profile = a
            .NetworkAdapter()
            .ok()?
            .GetConnectedProfileAsync()
            .ok()?
            .get()
            .ok()?;
        let ssid = profile
            .WlanConnectionProfileDetails()
            .ok()?
            .GetConnectedSsid()
            .ok()?;
        Some(ssid.to_string())
    }

    /// The names of the saved Wi-Fi profiles (each is named after its SSID).
    fn saved_profiles() -> Vec<String> {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::NetworkManagement::WiFi::{
            WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetProfileList,
            WlanOpenHandle, WLAN_INTERFACE_INFO_LIST, WLAN_PROFILE_INFO_LIST,
        };
        let mut names = Vec::new();
        unsafe {
            let mut version = 0u32;
            let mut handle = HANDLE::default();
            if WlanOpenHandle(2, None, &mut version, &mut handle) != 0 {
                return names;
            }
            let mut ifaces: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(handle, None, &mut ifaces) == 0 && !ifaces.is_null() {
                let list = std::slice::from_raw_parts(
                    (*ifaces).InterfaceInfo.as_ptr(),
                    (*ifaces).dwNumberOfItems as usize,
                );
                for iface in list {
                    let mut profiles: *mut WLAN_PROFILE_INFO_LIST = std::ptr::null_mut();
                    if WlanGetProfileList(handle, &iface.InterfaceGuid, None, &mut profiles) == 0
                        && !profiles.is_null()
                    {
                        let items = std::slice::from_raw_parts(
                            (*profiles).ProfileInfo.as_ptr(),
                            (*profiles).dwNumberOfItems as usize,
                        );
                        for p in items {
                            let end = p.strProfileName.iter().position(|&c| c == 0).unwrap_or(256);
                            names.push(String::from_utf16_lossy(&p.strProfileName[..end]));
                        }
                        WlanFreeMemory(profiles as *const _);
                    }
                }
                WlanFreeMemory(ifaces as *const _);
            }
            WlanCloseHandle(handle, None);
        }
        names
    }

    fn report(a: &WiFiAdapter) -> Vec<WiFiAvailableNetwork> {
        a.NetworkReport()
            .and_then(|r| r.AvailableNetworks())
            .map(|v| v.into_iter().collect())
            .unwrap_or_default()
    }

    pub fn wifi_networks() -> Result<Vec<WifiNetwork>, String> {
        let a = adapter()?;
        a.ScanAsync().and_then(|o| o.get()).map_err(err)?;
        let connected = connected_ssid(&a);
        let saved = saved_profiles();
        let list = report(&a)
            .into_iter()
            .filter_map(|n| {
                let ssid = n.Ssid().ok()?.to_string();
                let auth = n
                    .SecuritySettings()
                    .and_then(|s| s.NetworkAuthenticationType())
                    .unwrap_or(Auth::None);
                Some(WifiNetwork {
                    connected: connected.as_deref() == Some(ssid.as_str()),
                    saved: saved.contains(&ssid),
                    bars: n.SignalBars().unwrap_or(0).min(4),
                    secure: !matches!(auth, Auth::None | Auth::Open80211 | Auth::Unknown),
                    enterprise: matches!(
                        auth,
                        Auth::Rsna | Auth::Wpa | Auth::Wpa3Enterprise | Auth::Wpa3Enterprise192Bits
                    ),
                    ssid,
                })
            })
            .collect();
        Ok(dedupe(list))
    }

    pub fn wifi_connect(ssid: &str, password: Option<&str>) -> JoinResult {
        let a = adapter()?;
        let find = |a: &WiFiAdapter| {
            report(a)
                .into_iter()
                .find(|n| n.Ssid().map(|s| s == ssid).unwrap_or(false))
        };
        let network = match find(&a) {
            Some(n) => n,
            None => {
                a.ScanAsync().and_then(|o| o.get()).map_err(err)?;
                find(&a).ok_or("That network is out of range.")?
            }
        };
        let op = match password {
            Some(pw) => {
                let cred = PasswordCredential::new().map_err(err)?;
                cred.SetPassword(&HSTRING::from(pw)).map_err(err)?;
                a.ConnectWithPasswordCredentialAsync(
                    &network,
                    WiFiReconnectionKind::Automatic,
                    &cred,
                )
            }
            None => a.ConnectAsync(&network, WiFiReconnectionKind::Automatic),
        };
        let status = op
            .and_then(|o| o.get())
            .and_then(|r| r.ConnectionStatus())
            .map_err(err)?;
        match status {
            WiFiConnectionStatus::Success => Ok("connected".into()),
            WiFiConnectionStatus::InvalidCredential => Ok("password".into()),
            WiFiConnectionStatus::NetworkNotAvailable => {
                Err("That network is out of range.".into())
            }
            WiFiConnectionStatus::Timeout => Err("The network didn't answer in time.".into()),
            WiFiConnectionStatus::UnsupportedAuthenticationProtocol => {
                Err("This network's sign-in type isn't supported.".into())
            }
            WiFiConnectionStatus::AccessRevoked => Err("Windows revoked Wi-Fi access.".into()),
            // A secured network without a saved profile ends here when joined
            // without a password.
            _ if password.is_none() => Ok("password".into()),
            _ => Err("Couldn't connect.".into()),
        }
    }

    pub fn wifi_disconnect() -> Result<(), String> {
        adapter()?.Disconnect().map_err(err)
    }

    // ── Bluetooth ────────────────────────────────────────────────────────────

    /// Bluetooth classic and Bluetooth LE association endpoints.
    const AQS: &str =
        "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR \
                       System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\")";
    const PROPS: [&str; 5] = [
        "System.Devices.Aep.IsPaired",
        "System.Devices.Aep.IsConnected",
        "System.Devices.Aep.DeviceAddress",
        "System.Devices.Aep.Bluetooth.Cod.Major",
        "System.Devices.Aep.Bluetooth.Le.Appearance.Category",
    ];

    fn props() -> windows::core::Result<windows_collections::IIterable<HSTRING>> {
        let v: Vec<HSTRING> = PROPS.iter().map(|p| HSTRING::from(*p)).collect();
        Ok(windows_collections::IIterable::<HSTRING>::from(v))
    }

    fn prop(d: &DeviceInformation, key: &str) -> Option<IPropertyValue> {
        d.Properties()
            .ok()?
            .Lookup(&HSTRING::from(key))
            .ok()?
            .cast()
            .ok()
    }

    fn kind_of(d: &DeviceInformation) -> String {
        // Classic: the major device class. LE: the appearance category.
        if let Some(major) = prop(d, PROPS[3]).and_then(|v| v.GetUInt32().ok()) {
            return match major {
                4 => "audio",
                5 => "input",
                2 => "phone",
                1 => "computer",
                _ => "other",
            }
            .into();
        }
        match prop(d, PROPS[4]).and_then(|v| v.GetUInt16().ok()) {
            Some(15) => "input".into(), // HID: keyboards, mice, gamepads
            Some(1) => "phone".into(),
            Some(2) => "computer".into(),
            Some(65) | Some(66) | Some(67) => "audio".into(),
            _ => "other".into(),
        }
    }

    fn device(d: &DeviceInformation) -> Option<BtDevice> {
        let name = d.Name().ok()?.to_string();
        if name.trim().is_empty() {
            return None;
        }
        let flag = |k: &str| {
            prop(d, k)
                .and_then(|v| v.GetBoolean().ok())
                .unwrap_or(false)
        };
        Some(BtDevice {
            id: d.Id().ok()?.to_string(),
            paired: flag(PROPS[0]),
            connected: flag(PROPS[1]),
            kind: kind_of(d),
            name,
        })
    }

    /// The page's live list: an association-endpoint watcher that keeps
    /// inquiring while it runs and streams "bt-device" (a BtDevice, again on
    /// every change) and "bt-device-gone" (an id) to the page's window. Paired
    /// devices come first, at once; nearby ones as they answer.
    static WATCHER: Mutex<Option<DeviceWatcher>> = Mutex::new(None);

    pub fn bt_watch(app: AppHandle, label: String) -> Result<(), String> {
        com();
        bt_unwatch();
        let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
            &HSTRING::from(AQS),
            &props().map_err(err)?,
            DeviceInformationKind::AssociationEndpoint,
        )
        .map_err(err)?;
        let seen: std::sync::Arc<Mutex<HashMap<String, DeviceInformation>>> = Default::default();
        let send = {
            let (app, label) = (app.clone(), label.clone());
            move |d: &DeviceInformation| {
                if let Some(device) = device(d) {
                    let _ = app.emit_to(label.as_str(), "bt-device", device);
                }
            }
        };
        let (added, on_add) = (seen.clone(), send.clone());
        watcher
            .Added(&TypedEventHandler::new(
                move |_, d: windows::core::Ref<DeviceInformation>| {
                    let d = d.ok()?;
                    added.lock().unwrap().insert(d.Id()?.to_string(), d.clone());
                    on_add(d);
                    Ok(())
                },
            ))
            .map_err(err)?;
        let updated = seen.clone();
        watcher
            .Updated(&TypedEventHandler::new(
                move |_, u: windows::core::Ref<DeviceInformationUpdate>| {
                    let u = u.ok()?;
                    if let Some(d) = updated.lock().unwrap().get(&u.Id()?.to_string()) {
                        d.Update(u)?;
                        send(d);
                    }
                    Ok(())
                },
            ))
            .map_err(err)?;
        watcher
            .Removed(&TypedEventHandler::new(
                move |_, u: windows::core::Ref<DeviceInformationUpdate>| {
                    let id = u.ok()?.Id()?.to_string();
                    seen.lock().unwrap().remove(&id);
                    let _ = app.emit_to(label.as_str(), "bt-device-gone", id);
                    Ok(())
                },
            ))
            .map_err(err)?;
        watcher.Start().map_err(err)?;
        *WATCHER.lock().unwrap() = Some(watcher);
        Ok(())
    }

    pub fn bt_unwatch() {
        if let Some(w) = WATCHER.lock().unwrap().take() {
            let _ = w.Stop();
        }
    }

    /// The pairing in progress waits here for the user's answer to its prompt
    /// (`None`: declined).
    static PAIR_REPLY: Mutex<Option<mpsc::Sender<Option<String>>>> = Mutex::new(None);

    pub fn bt_pair_answer(accept: bool, pin: Option<String>) {
        if let Some(tx) = PAIR_REPLY.lock().unwrap().as_ref() {
            let _ = tx.send(accept.then(|| pin.unwrap_or_default()));
        }
    }

    /// Pairs, asking the page (event "bt-pair-prompt") when the device wants
    /// a PIN confirmed or typed. Returns once paired, or why it wasn't.
    pub fn bt_pair(app: AppHandle, label: String, id: &str) -> Result<(), String> {
        com();
        let info = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id))
            .and_then(|o| o.get())
            .map_err(err)?;
        let custom: DeviceInformationCustomPairing =
            info.Pairing().and_then(|p| p.Custom()).map_err(err)?;
        let (tx, rx) = mpsc::channel::<Option<String>>();
        *PAIR_REPLY.lock().unwrap() = Some(tx);
        let rx = Mutex::new(rx);
        let token = custom
            .PairingRequested(&TypedEventHandler::new(
                move |_, args: windows::core::Ref<DevicePairingRequestedEventArgs>| {
                    let args = args.ok()?;
                    let kind = args.PairingKind()?;
                    let pin = args.Pin().map(|p| p.to_string()).unwrap_or_default();
                    let ask = |what: &str| {
                        let _ = app.emit_to(
                            label.as_str(),
                            "bt-pair-prompt",
                            serde_json::json!({ "kind": what, "pin": pin }),
                        );
                    };
                    if kind == DevicePairingKinds::ConfirmOnly {
                        return args.Accept();
                    }
                    if kind == DevicePairingKinds::DisplayPin {
                        // Typed on the device itself: show it, nothing to answer.
                        ask("display");
                        return args.Accept();
                    }
                    let deferral = args.GetDeferral()?;
                    ask(if kind == DevicePairingKinds::ProvidePin {
                        "provide"
                    } else {
                        "confirm"
                    });
                    let reply = rx
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(60))
                        .ok()
                        .flatten();
                    match reply {
                        Some(typed) if kind == DevicePairingKinds::ProvidePin => {
                            args.AcceptWithPin(&HSTRING::from(typed))?
                        }
                        Some(_) => args.Accept()?,
                        None => {}
                    }
                    deferral.Complete()
                },
            ))
            .map_err(err)?;
        let kinds = DevicePairingKinds::ConfirmOnly
            | DevicePairingKinds::DisplayPin
            | DevicePairingKinds::ProvidePin
            | DevicePairingKinds::ConfirmPinMatch;
        let result = custom
            .PairAsync(kinds)
            .and_then(|o| o.get())
            .and_then(|r| r.Status());
        let _ = custom.RemovePairingRequested(token);
        *PAIR_REPLY.lock().unwrap() = None;
        match result.map_err(err)? {
            DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired => Ok(()),
            DevicePairingResultStatus::AuthenticationFailure => Err("The PIN didn't match.".into()),
            DevicePairingResultStatus::AuthenticationTimeout => {
                Err("The device took too long to answer.".into())
            }
            DevicePairingResultStatus::PairingCanceled
            | DevicePairingResultStatus::RejectedByHandler => Err("Pairing was cancelled.".into()),
            DevicePairingResultStatus::NoSupportedProfiles => {
                Err("Windows has no driver for this device.".into())
            }
            DevicePairingResultStatus::NotReadyToPair
            | DevicePairingResultStatus::ConnectionRejected => {
                Err("The device isn't ready: put it in pairing mode and try again.".into())
            }
            _ => Err("Couldn't pair with the device.".into()),
        }
    }

    pub fn bt_forget(id: &str) -> Result<(), String> {
        com();
        let info = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id))
            .and_then(|o| o.get())
            .map_err(err)?;
        let status = info
            .Pairing()
            .and_then(|p| p.UnpairAsync())
            .and_then(|o| o.get())
            .and_then(|r| r.Status())
            .map_err(err)?;
        match status {
            DeviceUnpairingResultStatus::Unpaired
            | DeviceUnpairingResultStatus::AlreadyUnpaired => Ok(()),
            _ => Err("Couldn't remove the device.".into()),
        }
    }

    /// Connects or disconnects a paired audio device, as Settings does: the
    /// Bluetooth audio driver's one-shot reconnect / disconnect, sent to each
    /// of its audio filters (found through its sound endpoints).
    pub fn bt_connect(id: &str, connect: bool) -> Result<(), String> {
        use windows::Win32::Media::Audio::{
            eAll, IConnector, IDeviceTopology, IMMDeviceEnumerator, IPart, MMDeviceEnumerator,
            DEVICE_STATE, DEVICE_STATEMASK_ALL,
        };
        use windows::Win32::Media::KernelStreaming::{
            IKsControl, KSIDENTIFIER, KSPROPERTY_TYPE_GET,
        };
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
        // KSPROPSETID_BtAudio, with KSPROPERTY_ONESHOT_RECONNECT (0) / DISCONNECT (1).
        const BT_AUDIO: windows::core::GUID =
            windows::core::GUID::from_u128(0x7fa06c40_b8f6_4c7e_8556_e8c33a12e54d);

        com();
        // "Bluetooth#Bluetoothxx:xx:..-aa:bb:cc:dd:ee:ff": the device's address ends the id.
        let address: String = id
            .rsplit('-')
            .next()
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_hexdigit())
            .collect::<String>()
            .to_lowercase();
        if address.len() != 12 {
            return Err("Unknown device address.".into());
        }
        let mut sent = 0;
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(err)?;
            let endpoints = enumerator
                .EnumAudioEndpoints(eAll, DEVICE_STATE(DEVICE_STATEMASK_ALL))
                .map_err(err)?;
            let mut filters = std::collections::HashSet::new();
            for i in 0..endpoints.GetCount().unwrap_or(0) {
                let Ok(endpoint) = endpoints.Item(i) else {
                    continue;
                };
                let filter = (|| -> windows::core::Result<String> {
                    let topology: IDeviceTopology = endpoint.Activate(CLSCTX_ALL, None)?;
                    let connector: IConnector = topology.GetConnector(0)?;
                    let other: IConnector = connector.GetConnectedTo()?;
                    let part: IPart = other.cast()?;
                    let filter_topology = part.GetTopologyObject()?;
                    let id = filter_topology.GetDeviceId()?;
                    let s = id.to_string()?;
                    windows::Win32::System::Com::CoTaskMemFree(Some(id.0 as *const _));
                    Ok(s)
                })();
                let Ok(filter) = filter else { continue };
                if !filter.to_lowercase().contains(&address) || !filters.insert(filter.clone()) {
                    continue;
                }
                let wide: Vec<u16> = filter.encode_utf16().chain(Some(0)).collect();
                let Ok(device) = enumerator.GetDevice(PCWSTR(wide.as_ptr())) else {
                    continue;
                };
                let Ok(ks) = device.Activate::<IKsControl>(CLSCTX_ALL, None) else {
                    continue;
                };
                let mut property = KSIDENTIFIER::default();
                property.Anonymous.Anonymous.Set = BT_AUDIO;
                property.Anonymous.Anonymous.Id = if connect { 0 } else { 1 };
                property.Anonymous.Anonymous.Flags = KSPROPERTY_TYPE_GET;
                let mut returned = 0u32;
                if ks
                    .KsProperty(
                        &property,
                        std::mem::size_of::<KSIDENTIFIER>() as u32,
                        std::ptr::null_mut(),
                        0,
                        &mut returned,
                    )
                    .is_ok()
                {
                    sent += 1;
                }
            }
        }
        if sent == 0 {
            return Err(if connect {
                "Couldn't reach the device: is it on and nearby?".into()
            } else {
                "Couldn't disconnect the device.".into()
            });
        }
        Ok(())
    }
}

// ponytail: Linux pairs through bluetoothctl's default agent, so devices that
// need a PIN typed or confirmed don't pair from here yet; a D-Bus BlueZ agent
// would add the prompts.
#[cfg(target_os = "linux")]
mod imp {
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};

    use tauri::{AppHandle, Emitter};

    use super::{dedupe, BtDevice, JoinResult, WifiNetwork};

    fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
        let out = Command::new(cmd)
            .args(args)
            .output()
            .map_err(|e| format!("{cmd}: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            Ok(text)
        } else {
            let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if e.is_empty() {
                text.trim().to_string()
            } else {
                e
            })
        }
    }

    /// nmcli's terse output: fields split on ':', with '\:' an escaped colon.
    pub(super) fn fields(line: &str) -> Vec<String> {
        let mut out = vec![String::new()];
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.last_mut().unwrap().extend(chars.next()),
                ':' => out.push(String::new()),
                c => out.last_mut().unwrap().push(c),
            }
        }
        out
    }

    pub fn wifi_networks() -> Result<Vec<WifiNetwork>, String> {
        let saved: Vec<String> = run("nmcli", &["-t", "-f", "NAME,TYPE", "connection", "show"])?
            .lines()
            .map(fields)
            .filter(|f| f.get(1).is_some_and(|t| t.contains("wireless")))
            .map(|f| f[0].clone())
            .collect();
        let list = run(
            "nmcli",
            &[
                "-t",
                "-f",
                "IN-USE,SSID,SIGNAL,SECURITY",
                "device",
                "wifi",
                "list",
                "--rescan",
                "auto",
            ],
        )?
        .lines()
        .map(fields)
        .filter(|f| f.len() >= 4)
        .map(|f| {
            let signal: u8 = f[2].parse().unwrap_or(0);
            WifiNetwork {
                connected: f[0] == "*",
                saved: saved.contains(&f[1]),
                bars: (signal / 25).min(4),
                secure: !f[3].is_empty() && f[3] != "--",
                enterprise: f[3].contains("802.1X"),
                ssid: f[1].clone(),
            }
        })
        .collect();
        Ok(dedupe(list))
    }

    pub fn wifi_connect(ssid: &str, password: Option<&str>) -> JoinResult {
        let result = match password {
            Some(pw) => run(
                "nmcli",
                &["device", "wifi", "connect", ssid, "password", pw],
            ),
            None => run("nmcli", &["connection", "up", "id", ssid])
                .or_else(|_| run("nmcli", &["device", "wifi", "connect", ssid])),
        };
        match result {
            Ok(_) => Ok("connected".into()),
            Err(e)
                if e.contains("Secrets were required")
                    || e.contains("802-11-wireless-security") =>
            {
                Ok("password".into())
            }
            Err(e) => Err(e),
        }
    }

    pub fn wifi_disconnect() -> Result<(), String> {
        let iface = run("nmcli", &["-t", "-f", "DEVICE,TYPE", "device"])?
            .lines()
            .map(fields)
            .find(|f| f.get(1).is_some_and(|t| t == "wifi"))
            .map(|f| f[0].clone())
            .ok_or("No Wi-Fi adapter.")?;
        run("nmcli", &["device", "disconnect", &iface]).map(|_| ())
    }

    /// "Device AA:BB:… Name" lines → (address, name).
    fn listed(args: &[&str]) -> Vec<(String, String)> {
        run("bluetoothctl", args)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let rest = l.strip_prefix("Device ")?;
                let (mac, name) = rest.split_once(' ')?;
                Some((mac.to_string(), name.to_string()))
            })
            .collect()
    }

    fn list(scan: bool) -> Result<Vec<BtDevice>, String> {
        if scan {
            let _ = run("bluetoothctl", &["--timeout", "5", "scan", "on"]);
        }
        let paired: Vec<String> = listed(&["devices", "Paired"])
            .into_iter()
            .map(|d| d.0)
            .collect();
        let connected: Vec<String> = listed(&["devices", "Connected"])
            .into_iter()
            .map(|d| d.0)
            .collect();
        let mut list: Vec<BtDevice> = listed(&["devices"])
            .into_iter()
            // Unnamed devices list their address as the name.
            .filter(|(mac, name)| (scan || paired.contains(mac)) && name.replace('-', ":") != *mac)
            .map(|(mac, name)| {
                let icon = run("bluetoothctl", &["info", &mac])
                    .unwrap_or_default()
                    .lines()
                    .find_map(|l| l.trim().strip_prefix("Icon: ").map(str::to_string))
                    .unwrap_or_default();
                let kind = if icon.starts_with("audio") {
                    "audio"
                } else if icon.starts_with("input") {
                    "input"
                } else if icon.starts_with("phone") {
                    "phone"
                } else if icon.starts_with("computer") {
                    "computer"
                } else {
                    "other"
                };
                BtDevice {
                    paired: paired.contains(&mac),
                    connected: connected.contains(&mac),
                    kind: kind.into(),
                    id: mac,
                    name,
                }
            })
            .collect();
        list.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(list)
    }

    static WATCHING: AtomicBool = AtomicBool::new(false);

    /// Paired devices at once, then a scan every few seconds while the page
    /// is open, streamed like the Windows watcher.
    pub fn bt_watch(app: AppHandle, label: String) -> Result<(), String> {
        if WATCHING.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        std::thread::spawn(move || {
            let mut scan = false;
            while WATCHING.load(Ordering::SeqCst) {
                for d in list(scan).unwrap_or_default() {
                    let _ = app.emit_to(label.as_str(), "bt-device", d);
                }
                scan = true;
            }
        });
        Ok(())
    }

    pub fn bt_unwatch() {
        WATCHING.store(false, Ordering::SeqCst);
    }

    pub fn bt_pair(_app: AppHandle, _label: String, id: &str) -> Result<(), String> {
        run("bluetoothctl", &["--timeout", "20", "pair", id])?;
        let _ = run("bluetoothctl", &["trust", id]);
        let _ = run("bluetoothctl", &["connect", id]);
        Ok(())
    }

    pub fn bt_pair_answer(_accept: bool, _pin: Option<String>) {}

    pub fn bt_forget(id: &str) -> Result<(), String> {
        run("bluetoothctl", &["remove", id]).map(|_| ())
    }

    pub fn bt_connect(id: &str, connect: bool) -> Result<(), String> {
        run(
            "bluetoothctl",
            &[if connect { "connect" } else { "disconnect" }, id],
        )
        .map(|_| ())
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn wifi_networks() -> Result<Vec<WifiNetwork>, String> {
    blocking(imp::wifi_networks).await
}

#[tauri::command]
pub async fn wifi_connect(ssid: String, password: Option<String>) -> JoinResult {
    blocking(move || imp::wifi_connect(&ssid, password.as_deref())).await
}

#[tauri::command]
pub async fn wifi_disconnect() -> Result<(), String> {
    blocking(imp::wifi_disconnect).await
}

/// Starts streaming the device list to the calling window (see imp::bt_watch).
#[tauri::command]
pub fn bt_watch(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    imp::bt_watch(app, window.label().to_string())
}

#[tauri::command]
pub fn bt_unwatch() {
    imp::bt_unwatch();
}

#[tauri::command]
pub async fn bt_pair(app: AppHandle, window: WebviewWindow, id: String) -> Result<(), String> {
    let label = window.label().to_string();
    blocking(move || imp::bt_pair(app, label, &id)).await
}

/// The answer to a "bt-pair-prompt": confirm (with the typed PIN when asked) or decline.
#[tauri::command]
pub fn bt_pair_answer(accept: bool, pin: Option<String>) {
    imp::bt_pair_answer(accept, pin);
}

#[tauri::command]
pub async fn bt_connect(id: String, connect: bool) -> Result<(), String> {
    blocking(move || imp::bt_connect(&id, connect)).await
}

#[tauri::command]
pub async fn bt_forget(id: String) -> Result<(), String> {
    blocking(move || imp::bt_forget(&id)).await
}

/// Lets the calling panel take the keyboard (it's no-activate otherwise), for
/// a password or PIN field.
#[tauri::command]
pub fn take_keyboard(window: WebviewWindow) {
    let _ = window.set_focus();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, bars: u8, connected: bool) -> WifiNetwork {
        WifiNetwork {
            ssid: ssid.into(),
            bars,
            secure: true,
            enterprise: false,
            connected,
            saved: false,
        }
    }

    #[test]
    fn duplicate_networks_keep_the_strongest() {
        let list = dedupe(vec![
            net("Home", 2, false),
            net("", 4, false),
            net("Home", 4, false),
            net("Cafe", 1, true),
        ]);
        let got: Vec<_> = list.iter().map(|n| (n.ssid.as_str(), n.bars)).collect();
        assert_eq!(got, [("Cafe", 1), ("Home", 4)]);
    }

    /// Lists real networks and devices; run with `--ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn probe() {
        println!("{:#?}", imp::wifi_networks());
    }
}
