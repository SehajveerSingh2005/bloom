use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindowVisible, GetWindowLongW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, GetClassNameW};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_NAME_WIN32};
use windows::Win32::Foundation::CloseHandle;
use std::path::Path;
use wmi::{COMLibrary, WMIConnection};

pub fn setup_keyboard_hook() -> windows::Win32::UI::WindowsAndMessaging::HHOOK {
    unsafe { windows::Win32::UI::WindowsAndMessaging::SetWindowsHookExA(windows::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0).expect("Failed") }
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: windows::Win32::Foundation::WPARAM, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN};
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_VOLUME_MUTE, VK_VOLUME_UP, VK_VOLUME_DOWN, VIRTUAL_KEY};
    if code >= 0 {
        let vk_code = VIRTUAL_KEY((*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode as u16);
        if vk_code == VK_VOLUME_MUTE || vk_code == VK_VOLUME_UP || vk_code == VK_VOLUME_DOWN {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize { handle_volume_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
        if vk_code.0 == 0x216 || vk_code.0 == 0x217 {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize { handle_brightness_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
    }
    windows::Win32::UI::WindowsAndMessaging::CallNextHookEx(None, code, wparam, lparam)
}

fn handle_volume_key_event(vk_code: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) {
    static mut LAST_TIME: u64 = 0;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    unsafe {
        if now - LAST_TIME < 50 { return; }
        LAST_TIME = now;
        if let Some(ref sender) = COMMAND_SENDER {
            use windows::Win32::UI::Input::KeyboardAndMouse::{VK_VOLUME_MUTE, VK_VOLUME_UP, VK_VOLUME_DOWN};
            let cmd = match vk_code {
                VK_VOLUME_MUTE => Some(SystemCommand::VolumeMute),
                VK_VOLUME_UP => Some(SystemCommand::VolumeUp),
                VK_VOLUME_DOWN => Some(SystemCommand::VolumeDown),
                _ => None,
            };
            if let Some(cmd) = cmd { let _ = sender.send(cmd); }
        }
    }
}

fn handle_brightness_key_event(vk_code: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let cmd = if vk_code.0 == 0x216 { Some(SystemCommand::BrightnessDown) }
            else if vk_code.0 == 0x217 { Some(SystemCommand::BrightnessUp) }
            else { None };
            if let Some(cmd) = cmd { let _ = sender.send(cmd); }
        }
    }
}
use crate::types::*;
use crate::state::*;
use crate::utils::*;

pub fn setup_taskbar_hook() {
    unsafe {
        use windows::Win32::UI::Accessibility::SetWinEventHook;
        use windows::Win32::UI::WindowsAndMessaging::{EVENT_OBJECT_SHOW, EVENT_OBJECT_LOCATIONCHANGE, WINEVENT_OUTOFCONTEXT};
        
        // Hook both "Show" and "Location Change" (happen when maximizing/switching apps)
        let _show_hook = SetWinEventHook(EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW, None, Some(taskbar_event_proc), 0, 0, WINEVENT_OUTOFCONTEXT);
        let _loc_hook = SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None, Some(taskbar_event_proc), 0, 0, WINEVENT_OUTOFCONTEXT);
    }
}

unsafe extern "system" fn taskbar_event_proc(
    _h_win_event_hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _dw_event_thread: u32,
    _dwms_event_time: u32,
) {
    if NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed) {
        let mut class_name = [0u8; 256];
        let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
        let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");
        
        if class_str == "Shell_TrayWnd" || class_str == "Shell_SecondaryTrayWnd" {
            // Taskbar is trying to show or move: slap it back down.
            set_taskbar_visibility(false);
        }
    }
}


pub fn setup_audio_visualization(app_handle: AppHandle) {
    std::thread::spawn(move || {
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, IMMDevice, IAudioClient, IAudioCaptureClient,
            eRender, eConsole, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
        };
        use windows::Win32::System::Com::{
            CoInitializeEx, CoTaskMemFree,
            COINIT_APARTMENTTHREADED, CoCreateInstance, CLSCTX_ALL
        };
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            const NUM_BANDS: usize = 5;
            let mut max_band_energies = [0.01f32; NUM_BANDS];
            let mut prev_values = [0.1f32; NUM_BANDS];

            loop {
                let _result: Result<(), String> = (|| {
                    let enumerator: IMMDeviceEnumerator =
                        CoCreateInstance(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL)
                            .map_err(|e| format!("CoCreateInstance failed: {:?}", e))?;

                    let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                        .map_err(|e| format!("GetDefaultAudioEndpoint failed: {:?}", e))?;

                    let current_id_str = if let Ok(id) = device.GetId() {
                        let id_str = windows::core::PCWSTR::from_raw(id.0).to_string().unwrap_or_default();
                        CoTaskMemFree(Some(id.0 as *const _));
                        id_str
                    } else { String::new() };

                    let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)
                        .map_err(|e| format!("Activate failed: {:?}", e))?;

                    let format_ptr = audio_client.GetMixFormat()
                        .map_err(|e| format!("GetMixFormat failed: {:?}", e))?;
                    
                    let channels = (*format_ptr).nChannels as usize;
                    let bits_per_sample = (*format_ptr).wBitsPerSample;
                    let bytes_per_sample = (bits_per_sample / 8) as usize;

                    
                    if bytes_per_sample == 0 || channels == 0 || bytes_per_sample > 4 {
                        CoTaskMemFree(Some(format_ptr as *const _));
                        return Err(format!("Invalid audio format: channels={}, bits={}", channels, bits_per_sample));
                    }
                    
                    let buffer_duration = 10_000_000i64;
                    audio_client.Initialize(
                        AUDCLNT_SHAREMODE_SHARED,
                        AUDCLNT_STREAMFLAGS_LOOPBACK,
                        buffer_duration,
                        0,
                        format_ptr,
                        Some(std::ptr::null()),
                    ).map_err(|e| format!("Initialize failed: {:?}", e))?;

                    CoTaskMemFree(Some(format_ptr as *const _));

                    let capture_client: IAudioCaptureClient = audio_client.GetService()
                        .map_err(|e| format!("GetService failed: {:?}", e))?;

                    audio_client.Start()
                        .map_err(|e| format!("Start failed: {:?}", e))?;

                    const FFT_SIZE: usize = 512;
                    let mut fft_buffer = vec![0.0f32; FFT_SIZE];
                    let mut buffer_pos = 0;

                    let mut last_device_check = std::time::Instant::now();
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(32));
                        
                        if last_device_check.elapsed().as_secs() >= 2 {
                            last_device_check = std::time::Instant::now();
                            if let Ok(new_device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
                                if let Ok(new_id) = new_device.GetId() {
                                    let new_str = windows::core::PCWSTR::from_raw(new_id.0).to_string().unwrap_or_default();
                                    CoTaskMemFree(Some(new_id.0 as *const _));
                                    if current_id_str != new_str {
                                        return Err("Default device changed".into());
                                    }
                                }
                            }
                        }
                        
                        // Skip all heavy processing if nothing is playing
                        if !ANY_MEDIA_PLAYING.load(Ordering::Relaxed) {
                            // Still gotta clear the buffer to avoid lag when it starts
                            loop {
                                let len = match capture_client.GetNextPacketSize() {
                                    Ok(l) => l,
                                    Err(_) => {
                                        return Err("Device invalidated".into());
                                    }
                                };
                                if len == 0 { break; }
                                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                                let mut num_frames = 0u32;
                                let mut flags = 0u32;
                                if capture_client.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None).is_err() {
                                    return Err("Device invalidated".into());
                                }
                                let _ = capture_client.ReleaseBuffer(num_frames);
                            }
                            continue;
                        }

                        let mut device_invalidated = false;
                        loop {
                            let packet_length = match capture_client.GetNextPacketSize() {
                                Ok(len) => len,
                                Err(_) => { 
                                    device_invalidated = true; 
                                    break; 
                                },
                            };
                            if packet_length == 0 { break; }
                            let mut data_ptr: *mut u8 = std::ptr::null_mut();
                            let mut num_frames = 0u32;
                            let mut flags = 0u32;

                            if capture_client.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None).is_err() {
                                device_invalidated = true;
                                break;
                            }

                            if !data_ptr.is_null() && num_frames > 0 && bytes_per_sample > 0 && channels > 0 {
                                let stride = channels * bytes_per_sample;
                                for frame in 0..num_frames as usize {
                                    let frame_offset = frame * stride;
                                    let mut sample_val: i64 = 0;
                                    for ch in 0..channels {
                                        let sample_offset = frame_offset + ch * bytes_per_sample;
                                        let sample: i64 = match bytes_per_sample {
                                            2 => *(data_ptr.add(sample_offset) as *const i16) as i64,
                                            4 => *(data_ptr.add(sample_offset) as *const i32) as i64,
                                            _ => 0,
                                        };
                                        sample_val = sample_val.wrapping_add(sample);
                                    }
                                    sample_val /= channels as i64;
                                    let normalized = match bytes_per_sample {
                                        2 => (sample_val as f32) / 32768.0,
                                        4 => (sample_val as f32) / 2147483648.0,
                                        _ => 0.0,
                                    };
                                    if buffer_pos < FFT_SIZE {
                                        let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * buffer_pos as f32 / FFT_SIZE as f32).cos());
                                        fft_buffer[buffer_pos] = normalized * window;
                                        buffer_pos += 1;
                                    }
                                    if buffer_pos >= FFT_SIZE {
                                        let band_ranges = [(1, 2), (2, 6), (6, 18), (18, 60), (60, 200)];
                                        let mut output = [0.0f32; NUM_BANDS];
                                        for (band_idx, &(bin_start, bin_end)) in band_ranges.iter().enumerate() {
                                            let mut total_mag = 0.0f32;
                                            // Optimization: Sample fewer bins in higher ranges where we have more of them
                                            // This drastically reduces CPU usage for DFT
                                            let step = if bin_end - bin_start > 20 { (bin_end - bin_start) / 10 } else { 1 };
                                            let mut count = 0;
                                            
                                            for bin in (bin_start..bin_end).step_by(step) {
                                                if bin >= FFT_SIZE / 2 { break; }
                                                let freq = 2.0 * std::f32::consts::PI * bin as f32 / FFT_SIZE as f32;
                                                let mut real = 0.0f32;
                                                let mut imag = 0.0f32;
                                                for (sample_idx, &sample) in fft_buffer.iter().enumerate() {
                                                    if sample == 0.0 { continue; } // Tiny optimization
                                                    let phase = freq * sample_idx as f32;
                                                    real += sample * phase.cos();
                                                    imag -= sample * phase.sin();
                                                }
                                                total_mag += (real * real + imag * imag).sqrt();
                                                count += 1;
                                            }
                                            let avg_mag = total_mag / count.max(1) as f32;
                                            let mut scaled_mag = avg_mag;
                                            let weighting = [1.2, 1.2, 1.5, 2.8, 5.0];
                                            scaled_mag *= weighting[band_idx];
                                            if scaled_mag > max_band_energies[band_idx] {
                                                max_band_energies[band_idx] = scaled_mag;
                                            } else {
                                                max_band_energies[band_idx] *= 0.99;
                                            }
                                            let target = (scaled_mag / max_band_energies[band_idx].max(0.12)).min(1.0).powf(0.75);
                                            let is_rising = target > prev_values[band_idx];
                                            let smooth_factor = if is_rising { 0.10 } else { 0.20 };
                                            output[band_idx] = prev_values[band_idx] * smooth_factor + target * (1.0 - smooth_factor);
                                            output[band_idx] = output[band_idx].min(1.0).max(0.18);
                                            prev_values[band_idx] = output[band_idx];
                                        }
                                        let _ = app_handle.emit("audio-visualization", AudioVisualizationData { frequencies: output.to_vec() });
                                        buffer_pos = 0;
                                    }
                                }
                                let _ = capture_client.ReleaseBuffer(num_frames);
                            }
                        }
                        
                        if device_invalidated {
                            return Err("Device invalidated".into());
                        }
                    }
                })();
                
                let _ = _result;
                
                // Wait before retrying (increased to 2500ms to allow Windows to fully update default endpoint)
                std::thread::sleep(std::time::Duration::from_millis(2500));
            }
            // if com_initialized { CoUninitialize(); }
        }
    });
}

pub fn setup_system_worker(app_handle: AppHandle) -> Sender<SystemCommand> {
    let (tx, rx) = channel::<SystemCommand>();
    let handle_system = app_handle.clone();
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED, CoCreateInstance, CLSCTX_ALL, CoTaskMemFree};
        use windows::Win32::Media::Audio::{IMMDeviceEnumerator, eRender, eConsole};
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus};
        use base64::{Engine as _, engine::general_purpose};
        use windows::Storage::Streams::DataReader;

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator = CoCreateInstance::<_, IMMDeviceEnumerator>(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL).ok();
            let mut device = enumerator.as_ref().and_then(|e| e.GetDefaultAudioEndpoint(eRender, eConsole).ok());
            let mut audio_endpoint_volume: Option<IAudioEndpointVolume> = device.as_ref().and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok());
            
            let mut current_device_id = String::new();
            if let Some(ref d) = device {
                if let Ok(id) = d.GetId() {
                    current_device_id = windows::core::PCWSTR::from_raw(id.0).to_string().unwrap_or_default();
                    CoTaskMemFree(Some(id.0 as *const _));
                }
            }

            let mut manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()).ok();
            let mut last_processed_media = std::time::Instant::now();
            let mut last_device_check = std::time::Instant::now();
            let mut last_emitted_info: Option<(String, String, bool, bool, Option<String>)> = None;
            let mut last_volume: f32 = -1.0;
            let mut last_muted: bool = false;

            let hide_osd = || {
                use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
                let class1 = windows::core::PCSTR(b"NativeHWNDHost\0".as_ptr());
                if let Ok(hwnd1) = FindWindowA(class1, windows::core::PCSTR::null()) { let _ = ShowWindow(hwnd1, SW_HIDE); }
            };

            loop {
                // Check for device change
                if last_device_check.elapsed().as_secs() >= 2 {
                    last_device_check = std::time::Instant::now();
                    if let Some(ref enum_ref) = enumerator {
                        if let Ok(new_device) = enum_ref.GetDefaultAudioEndpoint(eRender, eConsole) {
                            if let Ok(id) = new_device.GetId() {
                                let new_id = windows::core::PCWSTR::from_raw(id.0).to_string().unwrap_or_default();
                                CoTaskMemFree(Some(id.0 as *const _));
                                if new_id != current_device_id {
                                    current_device_id = new_id;
                                    device = Some(new_device);
                                    audio_endpoint_volume = device.as_ref().and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok());
                                    // Reset last_volume to force an update event
                                    last_volume = -1.0;
                                }
                            }
                        }
                    }
                }

                if manager.is_none() { manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()).ok(); }
                while let Ok(cmd) = rx.try_recv() {
                    if let Some(ref aev) = audio_endpoint_volume {
                        match cmd {
                            SystemCommand::VolumeMute => { if let Ok(muted) = aev.GetMute() { let _ = aev.SetMute(!muted.as_bool(), std::ptr::null()); hide_osd(); } }
                            SystemCommand::VolumeUp => { 
                                if let (Ok(vol), Ok(muted)) = (aev.GetMasterVolumeLevelScalar(), aev.GetMute()) { 
                                    let _ = aev.SetMasterVolumeLevelScalar((vol + 0.05).min(1.0), std::ptr::null()); 
                                    if muted.as_bool() { let _ = aev.SetMute(false, std::ptr::null()); }
                                    hide_osd(); 
                                } 
                            }
                            SystemCommand::VolumeDown => { if let Ok(vol) = aev.GetMasterVolumeLevelScalar() { let _ = aev.SetMasterVolumeLevelScalar((vol - 0.05).max(0.0), std::ptr::null()); hide_osd(); } }
                            SystemCommand::SetVolume(volume) => { 
                                let _ = aev.SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), std::ptr::null()); 
                                if volume > 0.0 { let _ = aev.SetMute(false, std::ptr::null()); }
                                hide_osd(); 
                            }
                            SystemCommand::MediaPlayPause => { if let Some(ref mgr) = manager { if let Ok(session) = mgr.GetCurrentSession() { let _ = session.TryTogglePlayPauseAsync(); } } }
                            SystemCommand::MediaNext => { if let Some(ref mgr) = manager { if let Ok(session) = mgr.GetCurrentSession() { let _ = session.TrySkipNextAsync(); } } }
                            SystemCommand::MediaPrevious => { if let Some(ref mgr) = manager { if let Ok(session) = mgr.GetCurrentSession() { let _ = session.TrySkipPreviousAsync(); } } }
                            SystemCommand::ToggleVisibility(visible) => {
                                let _ = handle_system.emit("visibility-change", visible);
                                if let Some(w) = handle_system.get_webview_window("bottom-corners") { if visible { let _ = w.show(); } else { let _ = w.hide(); } }
                            }
                            SystemCommand::BrightnessUp => {
                                let new_val = (CURRENT_BRIGHTNESS.load(Ordering::Relaxed) + 10).min(100);
                                CURRENT_BRIGHTNESS.store(new_val, Ordering::Relaxed);
                                LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), Ordering::Relaxed);
                                let _ = handle_system.emit("brightness-change", BrightnessChangeEvent { brightness: new_val });
                                if let Some(tx) = BRIGHTNESS_SENDER.get() { let _ = tx.send(new_val); }
                                hide_osd();
                            }
                            SystemCommand::BrightnessDown => {
                                let current = CURRENT_BRIGHTNESS.load(Ordering::Relaxed);
                                let new_val = if current > 10 { current - 10 } else { 0 };
                                CURRENT_BRIGHTNESS.store(new_val, Ordering::Relaxed);
                                LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), Ordering::Relaxed);
                                let _ = handle_system.emit("brightness-change", BrightnessChangeEvent { brightness: new_val });
                                if let Some(tx) = BRIGHTNESS_SENDER.get() { let _ = tx.send(new_val); }
                                hide_osd();
                            }
                        }
                    }
                }
                if let Some(ref aev) = audio_endpoint_volume {
                    if let (Ok(vol), Ok(muted)) = (aev.GetMasterVolumeLevelScalar(), aev.GetMute()) {
                        let is_muted: bool = muted.into();
                        if (vol - last_volume).abs() > 0.001 || is_muted != last_muted {
                            last_volume = vol; last_muted = is_muted;
                        let _ = handle_system.emit("volume-change", VolumeChangeEvent { volume: vol, is_muted });
                            hide_osd();
                        }
                    }
                }
                if last_processed_media.elapsed().as_millis() >= 2000 {
                    last_processed_media = std::time::Instant::now();
                    let mut best_info: Option<MediaInfo> = None;
                    if let Some(ref mgr) = manager {
                        if let Ok(sessions) = mgr.GetSessions() {
                            for i in 0..sessions.Size().unwrap_or(0) {
                                if let Ok(session) = sessions.GetAt(i) {
                                    let is_playing = session.GetPlaybackInfo().ok().and_then(|p| p.PlaybackStatus().ok()) == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
                                    if let Ok(props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
                                        let title = props.Title().unwrap_or_default().to_string();
                                        if !title.is_empty() {
                                            let artwork = (|| -> Option<Vec<String>> {
                                                let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
                                                let reader = DataReader::CreateDataReader(&stream).ok()?;
                                                reader.LoadAsync(stream.Size().ok()? as u32).ok()?.get().ok()?;
                                                let mut bytes = vec![0u8; stream.Size().ok()? as usize];
                                                reader.ReadBytes(&mut bytes).ok()?;
                                                Some(vec![format!("data:{};base64,{}", stream.ContentType().ok()?.to_string(), general_purpose::STANDARD.encode(bytes))])
                                            })();
                                            let info = MediaInfo { title, artist: props.Artist().unwrap_or_default().to_string(), is_playing, has_media: true, artwork };
                                            if is_playing { best_info = Some(info); break; } else if best_info.is_none() { best_info = Some(info); }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let current = best_info.unwrap_or(MediaInfo { title: "".into(), artist: "".into(), is_playing: false, has_media: false, artwork: None });
                    let art_str = current.artwork.as_ref().and_then(|a| a.first()).cloned();
                    if last_emitted_info.as_ref().map_or(true, |(t, a, p, h, art)| t != &current.title || a != &current.artist || p != &current.is_playing || h != &current.has_media || art != &art_str) {
                        let _ = handle_system.emit("media-update", current.clone());
                        ANY_MEDIA_PLAYING.store(current.is_playing, Ordering::Relaxed);
                        last_emitted_info = Some((current.title, current.artist, current.is_playing, current.has_media, art_str));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(64));
            }
        }
    });

    let handle_brightness = app_handle.clone();
    std::thread::spawn(move || {
        let com_lib = match COMLibrary::new() { Ok(lib) => lib, Err(_) => return };
        let wmi_con = match WMIConnection::with_namespace_path("root\\WMI", com_lib) { Ok(con) => con, Err(_) => return };
        let mut last_brightness = match wmi_con.query::<WmiMonitorBrightness>() { Ok(res) => res.first().map(|b| b.current_brightness as u32).unwrap_or(50), Err(_) => 50 };
        CURRENT_BRIGHTNESS.store(last_brightness, Ordering::Relaxed);
        loop {
            if get_now_ms() - LAST_BRIGHTNESS_CHANGE.load(Ordering::Relaxed) < 2000 { std::thread::sleep(std::time::Duration::from_millis(500)); continue; }
            if let Ok(results) = wmi_con.query::<WmiMonitorBrightness>() {
                if let Some(b) = results.first() {
                    let brightness = b.current_brightness as u32;
                    if brightness != last_brightness { last_brightness = brightness; CURRENT_BRIGHTNESS.store(brightness, Ordering::Relaxed); let _ = handle_brightness.emit("brightness-change", BrightnessChangeEvent { brightness }); }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    });

    let tx_clone = tx.clone();
    let handle_visibility = app_handle.clone();
    std::thread::spawn(move || {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, IsZoomed, IsIconic, GetWindowLongW, GWL_STYLE, WS_MAXIMIZE, GetClientRect};
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::Foundation::{RECT, POINT};
        let mut last_visible = true;
        let mut last_dock_overlap: Option<bool> = None;
        let mut last_notch_overlap: Option<bool> = None;
        let mut last_hwnd = HWND(std::ptr::null_mut());
        let mut last_emit = Instant::now();
        let mut is_known_shell = false;
        let my_process_id = std::process::id();
        
        loop {
            unsafe {
                use windows::Win32::Graphics::Gdi::{MonitorFromWindow, GetMonitorInfoA, MONITORINFO, MONITOR_DEFAULTTONEAREST};
                let mut hwnd = GetForegroundWindow();
                
                // Find the first meaningful window for overlap detection.
                // We skip Bloom windows, invisible windows, minimized windows, and 'cloaked' system ghosts.
                let mut check_count = 0;
                while !hwnd.is_invalid() && check_count < 15 {
                    let mut process_id = 0u32;
                    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
                    
                    let mut class_name = [0u8; 256];
                    let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
                    let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");
                    
                    let is_bloom = process_id == my_process_id || class_str.contains("Bloom");
                    let is_visible = IsWindowVisible(hwnd).as_bool();
                    let is_iconic = IsIconic(hwnd).as_bool();
                    
                    let mut cloaked = 0u32;
                    let is_cloaked = DwmGetWindowAttribute(hwnd, windows::Win32::Graphics::Dwm::DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4).is_ok() && cloaked != 0;

                    let mut rect = RECT::default();
                    let has_valid_rect = GetWindowRect(hwnd, &mut rect).is_ok() && (rect.right - rect.left) > 0 && (rect.bottom - rect.top) > 0;
                    
                    if is_bloom || !is_visible || is_iconic || is_cloaked || !has_valid_rect {
                        hwnd = windows::Win32::UI::WindowsAndMessaging::GetWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT).unwrap_or_default();
                        check_count += 1;
                    } else {
                        break;
                    }
                }

                let mut should_overlap = false;
                let mut should_notch_overlap = false;
                let mut current_is_fs = false;

                if !hwnd.is_invalid() && (hwnd != last_hwnd || last_emit.elapsed() >= Duration::from_secs(3)) {
                    last_hwnd = hwnd;
                    let mut class_name = [0u8; 256];
                    let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
                    let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");
                    let mut text = [0u16; 512];
                    let text_len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut text);
                    let title = String::from_utf16_lossy(&text[..text_len as usize]);

                    let is_desktop = class_str == "Progman" || class_str == "WorkerW";
                    let is_start = (class_str == "Windows.UI.Core.CoreWindow" || class_str == "SimpleWindow") && 
                                   (title == "Start" || title == "Search");
                    let is_shell = class_str == "Shell_TrayWnd" || class_str == "Shell_SecondaryTrayWnd" || is_start;
                    
                    is_known_shell = is_desktop || is_shell; 
                }

                if !hwnd.is_invalid() {
                    if is_known_shell {
                        should_overlap = false;
                        current_is_fs = false;
                    } else {
                        // Current hwnd is now guaranteed to be visible, non-iconic and non-cloaked
                        use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, WS_EX_TOOLWINDOW};
                        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                        
                        let is_transient = (ex_style & WS_EX_TOOLWINDOW.0) != 0;
                        
                        if !is_transient {
                            let mut rect = RECT::default();
                            let dwm_res = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32);
                            let has_rect = dwm_res.is_ok() || GetWindowRect(hwnd, &mut rect).is_ok();

                            if has_rect {
                                let h_monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                                let mut mi = MONITORINFO::default();
                                mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                                
                                if GetMonitorInfoA(h_monitor, &mut mi).as_bool() {
                                    let screen_rect = mi.rcMonitor;
                                    let is_maximized = IsZoomed(hwnd).as_bool() || (style & WS_MAXIMIZE.0) != 0;
                                    
                                    let mut is_client_fullscreen = false;
                                    let mut client_rect = RECT::default();
                                    if GetClientRect(hwnd, &mut client_rect).is_ok() {
                                        let mut top_left = POINT { x: client_rect.left, y: client_rect.top };
                                        let mut bottom_right = POINT { x: client_rect.right, y: client_rect.bottom };
                                        let _ = ClientToScreen(hwnd, &mut top_left);
                                        let _ = ClientToScreen(hwnd, &mut bottom_right);
                                        
                                        is_client_fullscreen = top_left.x <= screen_rect.left && top_left.y <= screen_rect.top &&
                                                               bottom_right.x >= screen_rect.right && bottom_right.y >= screen_rect.bottom;
                                    }

                                    let is_matches_screen = rect.left <= screen_rect.left && rect.top <= screen_rect.top && 
                                                            rect.right >= screen_rect.right && rect.bottom >= screen_rect.bottom;
                                    
                                    // Truly fullscreen means client covers screen, OR window matches screen but is not just a standard maximized window
                                    current_is_fs = is_client_fullscreen || (is_matches_screen && !is_maximized);

                                    if current_is_fs || is_maximized {
                                        should_overlap = true;
                                        should_notch_overlap = true;
                                    } else {
                                        should_overlap = false;
                                        if let Ok(dock_rect_lock) = DOCK_RECT.lock() {
                                            if let Some(dr) = *dock_rect_lock {
                                                let scale = if let Some(m) = handle_visibility.primary_monitor().ok().flatten() { m.scale_factor() } else { 1.0 };
                                                let d_left = (dr.x as f64 * scale) as i32;
                                                let d_right = d_left + (dr.width as f64 * scale) as i32;
                                                let res_h = (56.0 * scale) as i32;
                                                let trigger_y = screen_rect.bottom - res_h;

                                                if rect.left < d_right - 4 && rect.right > d_left + 4 && 
                                                   rect.bottom > trigger_y + 4 {
                                                    should_overlap = true;
                                                }
                                            }
                                        }

                                        if let Ok(notch_rect_lock) = NOTCH_RECT.lock() {
                                            if let Some(nr) = *notch_rect_lock {
                                                let scale = if let Some(m) = handle_visibility.primary_monitor().ok().flatten() { m.scale_factor() } else { 1.0 };
                                                let n_left = (nr.x as f64 * scale) as i32;
                                                let n_right = n_left + (nr.width as f64 * scale) as i32;
                                                let res_h = (36.0 * scale) as i32;
                                                let trigger_y = screen_rect.top + res_h;

                                                if rect.left < n_right - 4 && rect.right > n_left + 4 && 
                                                   rect.top < trigger_y - 4 {
                                                    should_notch_overlap = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            should_overlap = false;
                            current_is_fs = false;
                        }
                    }
                } else {
                    last_hwnd = hwnd;
                    should_overlap = false;
                    current_is_fs = false;
                    is_known_shell = false;
                }

                // Update overlap state
                CURRENT_DOCK_OVERLAP.store(if should_overlap { 1 } else { 0 }, Ordering::Relaxed);
                CURRENT_NOTCH_OVERLAP.store(if should_notch_overlap { 1 } else { 0 }, Ordering::Relaxed);
                
                if Some(should_overlap) != last_dock_overlap || last_emit.elapsed() >= Duration::from_secs(3) {
                    let _ = handle_visibility.emit("dock-overlap", should_overlap);
                    last_dock_overlap = Some(should_overlap);
                    last_emit = Instant::now();
                }

                if Some(should_notch_overlap) != last_notch_overlap || last_emit.elapsed() >= Duration::from_secs(3) {
                    let _ = handle_visibility.emit("notch-overlap", should_notch_overlap);
                    last_notch_overlap = Some(should_notch_overlap);
                }

                // Update full-screen visibility (hides TopBar/Corners)
                if current_is_fs && last_visible {
                    let _ = tx_clone.send(SystemCommand::ToggleVisibility(false));
                    last_visible = false;
                } else if !current_is_fs && !last_visible {
                    let _ = tx_clone.send(SystemCommand::ToggleVisibility(true));
                    last_visible = true;
                }

                // Enforce native taskbar hiding (periodic check)
                if NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed) {
                    use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, IsWindowVisible};
                    let tray_class = windows::core::PCSTR(b"Shell_TrayWnd\0".as_ptr());
                    let secondary_tray_class = windows::core::PCSTR(b"Shell_SecondaryTrayWnd\0".as_ptr());
                    
                    let mut should_rehide = false;
                    if let Ok(tray_hwnd) = FindWindowA(tray_class, windows::core::PCSTR::null()) {
                        if IsWindowVisible(tray_hwnd).as_bool() { should_rehide = true; }
                    }
                    if !should_rehide {
                        if let Ok(secondary_tray_hwnd) = FindWindowA(secondary_tray_class, windows::core::PCSTR::null()) {
                            if IsWindowVisible(secondary_tray_hwnd).as_bool() { should_rehide = true; }
                        }
                    }

                    if should_rehide {
                        set_taskbar_visibility(false);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    });

    tx
}



pub fn setup_brightness_worker() {
    let (tx, rx) = channel::<u32>();
    let _ = BRIGHTNESS_SENDER.set(tx);
    std::thread::spawn(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
        use std::os::windows::process::CommandExt;
        let child = Command::new("powershell").args(&["-NoProfile", "-NoLogo", "-Command", "-"]).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok();
        if let Some(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(b"$m = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods\n");
                let _ = stdin.flush();
                while let Ok(brightness) = rx.recv() {
                    let _ = stdin.write_all(format!("$m | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Brightness={}; Timeout=0}}\n", brightness).as_bytes());
                    let _ = stdin.flush();
                }
            }
            let _ = child.kill();
        }
    });
}

pub fn setup_cursor_monitor(app_handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        use windows::Win32::Foundation::POINT;
        
        let mut last_main_ignore = None;
        let mut last_dock_ignore = None;
        let mut last_edge_hover = None;
        let mut last_top_edge_hover = None;
        let mut dock_interaction_expiry = Instant::now() - Duration::from_secs(1);
        let mut topbar_interaction_expiry = Instant::now() - Duration::from_secs(1);
        
        loop {
            let now = Instant::now();
            let mut pt = POINT::default();
            unsafe {
                if GetCursorPos(&mut pt).is_ok() {
                    // --- Dock Interaction ---
                    if let Some(dock_win) = app_handle.get_webview_window("dock") {
                        if dock_win.is_visible().unwrap_or(false) {
                            let mut is_click_interactive = false;
                            let mut is_hovered = false;
                            
                            if let Ok(lock) = DOCK_WINDOW_RECT.lock() {
                                if let Some((win_pos, win_size)) = *lock {
                                    let in_window = pt.x >= win_pos.x && pt.x <= (win_pos.x + win_size.width as i32) &&
                                                    pt.y >= win_pos.y && pt.y <= (win_pos.y + win_size.height as i32);
                                    
                                    if in_window {
                                        // 1. Check if over actual dock items (DOCK_RECT)
                                        if let Ok(region) = DOCK_RECT.try_lock() {
                                            if let Some(r) = *region {
                                                let scale = dock_win.scale_factor().unwrap_or(1.0);
                                                let rx = win_pos.x + (r.x as f64 * scale) as i32 - 10;
                                                let ry = win_pos.y + (r.y as f64 * scale) as i32 - 10;
                                                let rw = (r.width as f64 * scale) as i32 + 20;
                                                let rh = (r.height as f64 * scale) as i32 + 20;
                                                if pt.x >= rx && pt.x <= (rx + rw) && pt.y >= ry && pt.y <= (ry + rh) {
                                                    is_click_interactive = true;
                                                }
                                            }
                                        }

                                        // 2. Check if over an open menu
                                        if !is_click_interactive && MENU_IS_OPEN.load(Ordering::Relaxed) {
                                            if let Ok(rect) = MENU_RECT.try_lock() {
                                                if let Some(r) = *rect {
                                                    let scale = dock_win.scale_factor().unwrap_or(1.0);
                                                    let rx = win_pos.x + (r.x as f64 * scale) as i32 - 5;
                                                    let ry = win_pos.y + (r.y as f64 * scale) as i32 - 5;
                                                    let rw = (r.width as f64 * scale) as i32 + 10;
                                                    let rh = (r.height as f64 * scale) as i32 + 10;
                                                    if pt.x >= rx && pt.x <= (rx + rw) && pt.y >= ry && pt.y <= (ry + rh) {
                                                        is_click_interactive = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // 3. Hot-edge detection (Bottom edge)
                            let in_dock_hover = DOCK_IS_HOVERED.load(Ordering::Relaxed);
                            if let Ok(Some(monitor)) = dock_win.primary_monitor() {
                                let m_pos = monitor.position();
                                let m_size = monitor.size();
                                let at_bottom_edge = pt.y >= (m_pos.y + m_size.height as i32 - 2) && 
                                                     pt.x >= m_pos.x && pt.x <= (m_pos.x + m_size.width as i32);
                                
                                if at_bottom_edge || in_dock_hover {
                                    is_hovered = true;
                                    dock_interaction_expiry = now + Duration::from_millis(500);
                                }
                            }

                            let final_dock_hover = is_hovered || now < dock_interaction_expiry;
                            if last_edge_hover != Some(final_dock_hover) {
                                let _ = app_handle.emit("dock-edge-hover", final_dock_hover);
                                last_edge_hover = Some(final_dock_hover);
                            }

                            let should_ignore = !is_click_interactive && !MENU_IS_OPEN.load(Ordering::Relaxed);
                            if last_dock_ignore != Some(should_ignore) {
                                let _ = dock_win.set_ignore_cursor_events(should_ignore);
                                last_dock_ignore = Some(should_ignore);
                            }
                        }
                    }

                    // --- Main (TopBar) Interaction ---
                    if let Some(main_win) = app_handle.get_webview_window("main") {
                        if main_win.is_visible().unwrap_or(false) {
                            // 3. Hot-edge detection (Top edge)
                            let in_notch_hover = NOTCH_IS_HOVERED.load(Ordering::Relaxed);
                            let mut is_notch_hovered = false;
                            if let Ok(Some(monitor)) = main_win.primary_monitor() {
                                let m_pos = monitor.position();
                                let m_size = monitor.size();
                                let at_top_edge = pt.y <= (m_pos.y + 2) && 
                                                  pt.x >= m_pos.x && pt.x <= (m_pos.x + m_size.width as i32);
                                
                                if at_top_edge || in_notch_hover {
                                    is_notch_hovered = true;
                                    topbar_interaction_expiry = now + Duration::from_millis(500);
                                }
                            }

                            let mut is_click_interactive = false;
                            if let Ok(lock) = MAIN_WINDOW_RECT.lock() {
                                if let Some((win_pos, win_size)) = *lock {
                                    let in_window = pt.x >= win_pos.x && pt.x <= (win_pos.x + win_size.width as i32) &&
                                                    pt.y >= win_pos.y && pt.y <= (win_pos.y + win_size.height as i32);
                                    
                                    if in_window {
                                        if let Ok(region) = NOTCH_RECT.try_lock() {
                                            if let Some(r) = *region {
                                                let scale = main_win.scale_factor().unwrap_or(1.0);
                                                let rx = win_pos.x + (r.x as f64 * scale) as i32 - 10;
                                                let ry = win_pos.y + (r.y as f64 * scale) as i32 - 10;
                                                let rw = (r.width as f64 * scale) as i32 + 20;
                                                let rh = (r.height as f64 * scale) as i32 + 20;
                                                if pt.x >= rx && pt.x <= (rx + rw) && pt.y >= ry && pt.y <= (ry + rh) {
                                                    is_click_interactive = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            let final_notch_hover = is_notch_hovered || now < topbar_interaction_expiry;
                            if last_top_edge_hover != Some(final_notch_hover) {
                                let _ = app_handle.emit("notch-edge-hover", final_notch_hover);
                                last_top_edge_hover = Some(final_notch_hover);
                            }

                            let final_ignore = !is_click_interactive && !MENU_IS_OPEN.load(Ordering::Relaxed);
                            if last_main_ignore != Some(final_ignore) {
                                let _ = main_win.set_ignore_cursor_events(final_ignore);
                                last_main_ignore = Some(final_ignore);
                            }
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(32));
        }
    });
}

pub fn trigger_app_scan() {
    if IS_SCANNING.load(Ordering::Relaxed) { return; }
    IS_SCANNING.store(true, Ordering::Relaxed);
    
    std::thread::spawn(|| {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED, CoTaskMemFree};
        use windows::Win32::UI::Shell::{SHGetKnownFolderIDList, FOLDERID_AppsFolder, SHGetDesktopFolder, IShellFolder, IEnumIDList, SHGetNameFromIDList, SIGDN_NORMALDISPLAY, SIGDN_FILESYSPATH, SIGDN_URL};
        let mut apps = Vec::new();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            // Use a scope to ensure COM objects are dropped before CoUninitialize
            {
                if let Ok(pidl_apps) = SHGetKnownFolderIDList(&FOLDERID_AppsFolder, 0, None) {
                    if let Ok(desktop) = SHGetDesktopFolder() {
                        if let Ok(apps_folder) = desktop.BindToObject::<_, IShellFolder>(pidl_apps, None) {
                            let mut enum_id: Option<IEnumIDList> = None;
                            let res = apps_folder.EnumObjects(HWND(std::ptr::null_mut()), (windows::Win32::UI::Shell::SHCONTF_FOLDERS.0 | windows::Win32::UI::Shell::SHCONTF_NONFOLDERS.0) as u32, &mut enum_id);
                            
                            if res.is_ok() {
                                if let Some(enum_id) = enum_id {
                                    let mut pidl_item = std::ptr::null_mut();
                                    let mut fetched = 0;
                                    while enum_id.Next(&mut [pidl_item], Some(&mut fetched)).is_ok() && fetched > 0 {
                                        
                                        let name = if let Ok(n_ptr) = SHGetNameFromIDList(pidl_item, SIGDN_NORMALDISPLAY) {
                                            let s = String::from_utf16_lossy(windows::core::PCWSTR(n_ptr.0).as_wide());
                                            CoTaskMemFree(Some(n_ptr.0 as *const _));
                                            s
                                        } else { "Unknown".to_string() };

                                        let path = if let Ok(p_ptr) = SHGetNameFromIDList(pidl_item, SIGDN_FILESYSPATH) {
                                            let s = String::from_utf16_lossy(windows::core::PCWSTR(p_ptr.0).as_wide());
                                            CoTaskMemFree(Some(p_ptr.0 as *const _));
                                            s
                                        } else if let Ok(p_ptr) = SHGetNameFromIDList(pidl_item, SIGDN_URL) {
                                            let s = String::from_utf16_lossy(windows::core::PCWSTR(p_ptr.0).as_wide());
                                            CoTaskMemFree(Some(p_ptr.0 as *const _));
                                            s
                                        } else { name.clone() };

                                        if !name.to_lowercase().contains("uninstall") && !name.is_empty() && name != "Unknown" {
                                            apps.push(AppInfo {
                                                name,
                                                path,
                                                icon: None,
                                                is_running: false,
                                                hwnd: None,
                                                executable: None,
                                                all_hwnds: None,
                                            });
                                        }
                                        CoTaskMemFree(Some(pidl_item as *const _));
                                        pidl_item = std::ptr::null_mut();
                                    }
                                }
                            }
                        }
                    }
                    CoTaskMemFree(Some(pidl_apps as *const _));
                }
            } // Close COM scope
            CoUninitialize();
        }

        // Fallback for classic FS apps
        let mut paths = vec![
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs".to_string(),
        ];
        if let Ok(appdata) = std::env::var("APPDATA") {
            paths.push(format!(r"{}\Microsoft\Windows\Start Menu\Programs", appdata));
            paths.push(format!(r"{}\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar", appdata));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            paths.push(format!(r"{}\Microsoft\Windows\Start Menu\Programs", local));
            paths.push(format!(r"{}\Microsoft\WindowsApps", local)); 
        }
        
        // Add Desktop folders as they often contain app shortcuts
        paths.push(r"C:\Users\Public\Desktop".to_string());
        if let Ok(home) = std::env::var("USERPROFILE") {
            paths.push(format!(r"{}\Desktop", home));
        }

        for root in paths {
            if Path::new(&root).exists() {
                scan_dir(Path::new(&root), &mut apps, 0);
            }
        }

        if let Some(c) = INSTALLED_APPS_CACHE.get() {
            if let Ok(mut lock) = c.lock() {
                *lock = apps;
            }
        }
        IS_SCANNING.store(false, Ordering::Relaxed);
    });
}

fn scan_dir(path: &Path, apps: &mut Vec<AppInfo>, depth: i32) {
    if depth > 10 { return; } // Increased depth for deeply nested Start Menu folders
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan_dir(&path, apps, depth + 1);
            } else if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                if ext_str == "lnk" || ext_str == "exe" {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    if name.to_lowercase().contains("uninstall") || name.starts_with("Install") { continue; }
                    
                    let path_str = path.to_string_lossy().to_string();
                    // Avoid duplicates
                    if !apps.iter().any(|a| a.path == path_str || a.name == name) {
                        apps.push(AppInfo {
                            name,
                            path: path_str,
                            icon: None,
                            is_running: false,
                            hwnd: None,
                            executable: None,
                            all_hwnds: None,
                        });
                    }
                }
            }
        }
    }
}

pub fn sync_overlays(app: &AppHandle) {
    if let Some(vol_win) = app.get_webview_window("volume-overlay") {
        if let Ok(Some(monitor)) = vol_win.primary_monitor() {
            let size = monitor.size();
            let pos = monitor.position();
            let scale = monitor.scale_factor();
            let pw = (200.0 * scale) as i32;
            let ph = (400.0 * scale) as i32;
            let _ = vol_win.set_position(tauri::PhysicalPosition::new(
                pos.x,
                pos.y + (size.height as i32 / 2) - (ph / 2),
            ));
            let _ = vol_win.set_size(tauri::PhysicalSize::new(pw as u32, ph as u32));
        }
    }
    if let Some(br_win) = app.get_webview_window("brightness-overlay") {
        if let Ok(Some(monitor)) = br_win.primary_monitor() {
            let size = monitor.size();
            let pos = monitor.position();
            let scale = monitor.scale_factor();
            let pw = (200.0 * scale) as i32;
            let ph = (400.0 * scale) as i32;
            let _ = br_win.set_position(tauri::PhysicalPosition::new(
                pos.x + (size.width as i32 - pw),
                pos.y + (size.height as i32 / 2) - (ph / 2),
            ));
            let _ = br_win.set_size(tauri::PhysicalSize::new(pw as u32, ph as u32));
        }
    }
}

pub fn register_appbar(window: tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.app_handle().primary_monitor() {
        let m_size = monitor.size();
        let m_pos = monitor.position();
        let hwnd = window.hwnd().unwrap();
        let scale = monitor.scale_factor();
        let ph = window.outer_size().map(|s| s.height as i32).unwrap_or((48.0 * scale) as i32);
        let pr = (40.0 * scale) as i32;  // But only reserve 40px of screen space
        
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE as WS_EX_NA, SWP_FRAMECHANGED};
            use windows::Win32::Foundation::RECT;
            use windows::Win32::UI::Shell::{SHAppBarMessage, APPBARDATA, ABM_NEW, ABM_QUERYPOS, ABM_SETPOS, ABE_TOP};
            
            // Set styles first
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
            ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);
            
            let mut abd = APPBARDATA::default();
            abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
            abd.hWnd = hwnd;
            
            if !MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                SHAppBarMessage(ABM_NEW, &mut abd);
                MAIN_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
            }

            abd.uEdge = ABE_TOP;
            abd.rc = RECT { 
                left: m_pos.x, 
                top: m_pos.y, 
                right: m_pos.x + m_size.width as i32, 
                bottom: m_pos.y + pr
            };
            
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            SHAppBarMessage(ABM_SETPOS, &mut abd);
            
            // Use the shell-approved rect for the final position, but keep our ph height for the window
            let final_width = abd.rc.right - abd.rc.left;
            let _ = SetWindowPos(hwnd, None, abd.rc.left, abd.rc.top, final_width, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            
            if !window.is_visible().unwrap_or(false) {
                let _ = window.show();
            }
        }
    }
}

pub fn register_dock_appbar(window: tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.app_handle().primary_monitor() {
        let m_size = monitor.size();
        let m_pos = monitor.position();
        let hwnd = window.hwnd().unwrap();
        let scale = monitor.scale_factor();
        
        // Ensure we have a valid height (fallback to 100 if 0)
        let mut ph = window.outer_size().map(|s| s.height as i32).unwrap_or(0);
        if ph <= 0 { ph = (100.0 * scale) as i32; }
        
        let pr = (56.0 * scale) as i32;
        
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE as WS_EX_NA, SWP_FRAMECHANGED};
            use windows::Win32::Foundation::RECT;
            use windows::Win32::UI::Shell::{SHAppBarMessage, APPBARDATA, ABM_NEW, ABM_QUERYPOS, ABM_SETPOS, ABE_BOTTOM};
            
            // Set extended styles (ToolWindow and NoActivate)
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
            ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);

            // Hide native taskbar first to free up space
            set_taskbar_visibility(false);

            let mut abd = APPBARDATA::default();
            abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
            abd.hWnd = hwnd;
            
            if !DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                SHAppBarMessage(ABM_NEW, &mut abd);
                DOCK_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
            }

            abd.uEdge = ABE_BOTTOM;
            abd.rc = RECT { 
                left: m_pos.x, 
                top: m_pos.y + m_size.height as i32 - pr, 
                right: m_pos.x + m_size.width as i32, 
                bottom: m_pos.y + m_size.height as i32 
            };
            
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            SHAppBarMessage(ABM_SETPOS, &mut abd);
            
            // Critical: Force the window to the actual bottom of the screen, 
            // ignoring what ABM_SETPOS might have tried to "correct" (like stacking on invisible taskbar)
            let final_y = m_pos.y + m_size.height as i32 - ph;
            let final_width = abd.rc.right - abd.rc.left;
            let _ = SetWindowPos(hwnd, None, abd.rc.left, final_y, final_width, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            
            if !window.is_visible().unwrap_or(false) {
                let _ = window.show();
            }
        }
    }
}

pub unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let apps = &mut *(lparam.0 as *mut Vec<AppInfo>);

    if IsWindowVisible(hwnd).as_bool() {
        let mut text = [0u16; 512];
        let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut text);
        if len > 0 {
            let title = String::from_utf16_lossy(&text[..len as usize]);
            
            // Filter out some common non-app windows
            let mut class_name = [0u16; 256];
            let class_len = GetClassNameW(hwnd, &mut class_name);
            let class_str = String::from_utf16_lossy(&class_name[..class_len as usize]);
            
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let _style = GetWindowLongW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE) as u32;
            
            // Basic filter for top-level app windows
            // We include windows without captions if they don't have the ToolWindow style,
            // as many games and modern apps (like Spotify/Valorant) lack WS_CAPTION.
            if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
                return true.into();
            }

            // Filter out system containers and background stuff
            if title == "Program Manager" || title == "Bloom" || title == "Bloom Dock" {
                return true.into();
            }

            if class_str == "Windows.UI.Core.CoreWindow" || class_str == "ApplicationFrameWindow" {
                // Ignore these or handle specially if needed
            }

            let mut process_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut process_id));
            
            if let Ok(process_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) {
                let mut path_buf = [0u16; 1024];
                let mut path_len = path_buf.len() as u32;
                if QueryFullProcessImageNameW(process_handle, PROCESS_NAME_WIN32, windows::core::PWSTR(path_buf.as_mut_ptr()), &mut path_len).is_ok() {
                    let path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
                    let lowercase_path = path.to_lowercase();
                    
                    // Filter out Bloom itself and some common background processes
                    if lowercase_path.contains("bloom.exe") || lowercase_path.contains("conhost.exe") || 
                       lowercase_path.contains("explorer.exe") || lowercase_path.contains("shellexperiencehost.exe") ||
                       lowercase_path.contains("searchhost.exe") || lowercase_path.contains("applicationframehost.exe") ||
                       lowercase_path.contains("textinputhost.exe") || lowercase_path.contains("systemsettings.exe") {
                        let _ = CloseHandle(process_handle);
                        return true.into();
                    }

                    let name = Path::new(&path).file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&title)
                        .replace(".exe", "");

                    let final_name = if (name == "msedge" || name == "chrome" || name == "ApplicationFrameHost") && !title.is_empty() {
                        // Extract a cleaner name from the window title for host processes (PWAs, UWP apps)
                        title.split(" - ").next().map(|s| s.trim()).unwrap_or(&title).to_string()
                    } else if name == "explorer" && title.is_empty() {
                        "File Explorer".to_string()
                    } else {
                        name.clone()
                    };

                    // Avoid duplicates, but allow host processes to have multiple entries if they have different names (PWAs)
                    let already_exists = if name == "msedge" || name == "chrome" || name == "ApplicationFrameHost" {
                        apps.iter().any(|a| a.path == path && a.name == final_name)
                    } else {
                        apps.iter().any(|a| a.path == path)
                    };

                    if !already_exists {
                        apps.push(AppInfo {
                            name: final_name,
                            path,
                            icon: None,
                            is_running: true,
                            hwnd: Some(hwnd.0 as isize),
                            executable: None,
                            all_hwnds: None,
                        });
                    }
                    let _ = CloseHandle(process_handle);
                }
            }
        }
    }
    true.into()
}

pub fn unregister_appbar_native(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::Shell::{SHAppBarMessage, APPBARDATA, ABM_REMOVE};
        let mut abd = APPBARDATA::default();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        abd.hWnd = hwnd;
        SHAppBarMessage(ABM_REMOVE, &mut abd);
    }
}
