// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, APPBARDATA,
    ABM_NEW, ABM_SETPOS,
    ABE_TOP,
};
use windows::Win32::UI::Shell::ShellExecuteA;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_VOLUME_MUTE, VK_VOLUME_UP, VK_VOLUME_DOWN,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Storage::Streams::DataReader;
use base64::{Engine as _, engine::general_purpose};
use std::sync::mpsc::{channel, Sender};
use wmi::{COMLibrary, WMIConnection};
use serde::Deserialize;
use std::os::windows::process::CommandExt;

#[derive(Deserialize, Debug)]
#[serde(rename = "WmiMonitorBrightness")]
#[serde(rename_all = "PascalCase")]
struct WmiMonitorBrightness {
    current_brightness: u8,
}

// Audio visualization event
#[derive(Clone, serde::Serialize)]
struct AudioVisualizationData {
    frequencies: Vec<f32>,
}

// Media info struct for Tauri commands
#[derive(serde::Serialize, Clone)]
pub struct MediaInfo {
    title: String,
    artist: String,
    is_playing: bool,
    has_media: bool,
    artwork: Option<Vec<String>>,
}

enum SystemCommand {
    VolumeMute,
    VolumeUp,
    VolumeDown,
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    ToggleVisibility(bool),
    BrightnessUp,
    BrightnessDown,
}

static mut COMMAND_SENDER: Option<Sender<SystemCommand>> = None;

#[tauri::command]
fn set_window_height(window: tauri::Window, height: f64) {
    if let Ok(scale_factor) = window.scale_factor() {
        if let Ok(physical_size) = window.inner_size() {
            let logical_width = physical_size.width as f64 / scale_factor;
            let _ = window.set_size(tauri::LogicalSize::new(logical_width, height));
        }
    }
}

#[tauri::command]
fn set_ignore_cursor_events(window: tauri::Window, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}

#[tauri::command]
fn toggle_corners_window(app_handle: tauri::AppHandle, mode: String) {
    if let Some(bc_win) = app_handle.get_webview_window("bottom-corners") {
        if mode == "all" {
            if let Ok(Some(monitor)) = bc_win.primary_monitor() {
                let size = monitor.size();
                let pos = monitor.position();
                let hwnd = bc_win.hwnd().unwrap();
                
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_SHOWWINDOW};
                    // Use 48px as requested/found for better alignment consistency with appbar
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        pos.x,
                        pos.y + (size.height as i32 - 48),
                        size.width as i32,
                        48,
                        SWP_NOZORDER | SWP_SHOWWINDOW
                    );
                }
            }
            let _ = bc_win.show();
            let _ = bc_win.set_always_on_top(true);
        } else {
            let _ = bc_win.hide();
        }
    }
}

#[tauri::command]
fn broadcast_setting(app: tauri::AppHandle, key: String, value: serde_json::Value) {
    let _ = app.emit("settings-changed", serde_json::json!({ "key": key, "value": value }));
}

#[tauri::command]
fn hide_native_osd() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
        let class1 = windows::core::PCSTR(b"NativeHWNDHost\0".as_ptr());
        if let Ok(hwnd) = FindWindowA(class1, windows::core::PCSTR::null()) {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

#[tauri::command]
fn open_settings_window(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn hide_volume_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("volume-overlay") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn hide_brightness_overlay(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("brightness-overlay") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn open_wifi_settings() {
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use std::ptr::null_mut;

        let _ = ShellExecuteA(
            Some(HWND(null_mut())),
            windows::core::PCSTR(b"open\0".as_ptr()),
            windows::core::PCSTR(b"ms-availablenetworks:\0".as_ptr()),
            windows::core::PCSTR::null(),
            windows::core::PCSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[tauri::command]
fn open_notification_center() {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use std::ptr::null_mut;

        let _ = ShellExecuteA(
            Some(HWND(null_mut())),
            windows::core::PCSTR(b"open\0".as_ptr()),
            windows::core::PCSTR(b"ms-actioncenter:\0".as_ptr()),
            windows::core::PCSTR::null(),
            windows::core::PCSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[tauri::command]
fn media_play_pause() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(SystemCommand::MediaPlayPause);
        }
    }
}

#[tauri::command]
fn media_next() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(SystemCommand::MediaNext);
        }
    }
}

#[tauri::command]
fn media_previous() {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let _ = sender.send(SystemCommand::MediaPrevious);
        }
    }
}



// WASAPI Audio capture for visualization
fn setup_audio_visualization(app_handle: AppHandle) {
    std::thread::spawn(move || {
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, IMMDevice, IAudioClient, IAudioCaptureClient,
            eRender, eConsole, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
        };
        use windows::Win32::System::Com::{
            CoInitializeEx, CoUninitialize, CoTaskMemFree,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::Foundation::S_OK;

        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let com_initialized = hr.is_ok() || hr == S_OK;

            let result: Result<(), String> = (|| {
                // Get default audio output device
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("CoCreateInstance failed: {:?}", e))?;

                let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                    .map_err(|e| format!("GetDefaultAudioEndpoint failed: {:?}", e))?;

                // Activate audio client
                let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)
                    .map_err(|e| format!("Activate failed: {:?}", e))?;

                // Get mix format for loopback
                let format_ptr = audio_client.GetMixFormat()
                    .map_err(|e| format!("GetMixFormat failed: {:?}", e))?;
                
                let channels = (*format_ptr).nChannels as usize;
                let bits_per_sample = (*format_ptr).wBitsPerSample;
                let bytes_per_sample = (bits_per_sample / 8) as usize;
                
                // Validate format
                if bytes_per_sample == 0 || channels == 0 || bytes_per_sample > 4 {
                    CoTaskMemFree(Some(format_ptr as *const _));
                    return Err(format!("Invalid audio format: channels={}, bits={}", channels, bits_per_sample));
                }
                
                // Initialize audio client for loopback capture
                let buffer_duration = 10_000_000i64; // 100ms in 100-ns units

                audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_LOOPBACK,
                    buffer_duration,
                    0,
                    format_ptr,
                    Some(std::ptr::null()),
                ).map_err(|e| format!("Initialize failed: {:?}", e))?;

                // Now free the format pointer after Initialize
                CoTaskMemFree(Some(format_ptr as *const _));

                // Get capture client
                let capture_client: IAudioCaptureClient = audio_client.GetService()
                    .map_err(|e| format!("GetService failed: {:?}", e))?;

                // Start capturing
                audio_client.Start()
                    .map_err(|e| format!("Start failed: {:?}", e))?;

                // FFT buffer (512 samples for frequency analysis)
                const FFT_SIZE: usize = 512;
                let mut fft_buffer = vec![0.0f32; FFT_SIZE];
                let mut buffer_pos = 0;

                // Simple frequency bands for visualization (5 bands)
                const NUM_BANDS: usize = 5;
                
                // Adaptive normalization state
                let mut max_band_energies = [0.01f32; NUM_BANDS];
                let mut prev_values = [0.1f32; NUM_BANDS];

                // Main visualization loop
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(16));

                    // Process all available packets
                    loop {
                        // Get available packet count
                        let packet_length = match capture_client.GetNextPacketSize() {
                            Ok(len) => len,
                            Err(_) => break,
                        };

                        if packet_length == 0 {
                            break;
                        }

                        // Get audio data
                        let mut data_ptr: *mut u8 = std::ptr::null_mut();
                        let mut num_frames = 0u32;
                        let mut flags = 0u32;

                        if capture_client.GetBuffer(
                            &mut data_ptr,
                            &mut num_frames,
                            &mut flags,
                            Some(std::ptr::null_mut()),
                            Some(std::ptr::null_mut()),
                        ).is_err() {
                            break;
                        }

                        if !data_ptr.is_null() && num_frames > 0 && bytes_per_sample > 0 && channels > 0 {
                            // Process audio data (convert to f32 samples)
                            let stride = channels * bytes_per_sample;
                            
                            for frame in 0..num_frames as usize {
                                let frame_offset = frame * stride;

                                // Read and mix channels
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

                                // Normalize to f32 [-1.0, 1.0]
                                let normalized = match bytes_per_sample {
                                    2 => (sample_val as f32) / 32768.0,
                                    4 => (sample_val as f32) / 2147483648.0,
                                    _ => 0.0,
                                };

                                // Add to FFT buffer with Hanning window
                                if buffer_pos < FFT_SIZE {
                                    let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * buffer_pos as f32 / FFT_SIZE as f32).cos());
                                    fft_buffer[buffer_pos] = normalized * window;
                                    buffer_pos += 1;
                                }

                                // Process FFT when buffer is full
                                if buffer_pos >= FFT_SIZE {
                                    // Balanced log-ish frequency bands
                                    let band_ranges = [
                                        (1, 2),    // Sub-bass (93Hz)
                                        (2, 6),    // Bass/Low-mids (187-562Hz)
                                        (6, 18),   // Mids (562-1687Hz)
                                        (18, 60),  // High-mids (1687-5625Hz)
                                        (60, 200), // Highs (5625-18750Hz)
                                    ];

                                    let mut output = [0.0f32; NUM_BANDS];
                                    
                                    for (band_idx, &(bin_start, bin_end)) in band_ranges.iter().enumerate() {
                                        let mut total_mag = 0.0f32;
                                        
                                        for bin in bin_start..bin_end {
                                            if bin >= FFT_SIZE / 2 { break; }
                                            
                                            // Single-bin DFT
                                            let freq = 2.0 * std::f32::consts::PI * bin as f32 / FFT_SIZE as f32;
                                            let mut real = 0.0f32;
                                            let mut imag = 0.0f32;
                                            
                                            for (sample_idx, &sample) in fft_buffer.iter().enumerate() {
                                                let phase = freq * sample_idx as f32;
                                                real += sample * phase.cos();
                                                imag -= sample * phase.sin();
                                            }
                                            
                                            let mag = (real * real + imag * imag).sqrt();
                                            total_mag += mag;
                                        }
                                        
                                        // Average magnitude in band
                                        let band_size = (bin_end - bin_start) as f32;
                                        let mut avg_mag = total_mag / band_size;

                                        // Apply sensitivity weighting (premium re-balance)
                                        let weighting = [1.2, 1.2, 1.5, 2.8, 5.0];
                                        avg_mag *= weighting[band_idx];

                                        // Adaptive Normalization (AGC) - Purely Independent
                                        if avg_mag > max_band_energies[band_idx] {
                                            max_band_energies[band_idx] = avg_mag;
                                        } else {
                                            // Stable decay (approx 0.7s to reset half sensitivity)
                                            max_band_energies[band_idx] *= 0.99;
                                        }
                                        
                                        // Ensure a sensible floor for max energy
                                        let active_max = max_band_energies[band_idx].max(0.12);
                                        let normalized_val = (avg_mag / active_max).min(1.0);
                                        
                                        // Map to dynamic range with restored variance (higher powf = more dynamic)
                                        let target = normalized_val.powf(0.75);
                                        
                                        // Premium smoothing: High-inertia rise, elegant fall
                                        let is_rising = target > prev_values[band_idx];
                                        let smooth_factor = if is_rising {
                                            0.10 // Snappy rise
                                        } else {
                                            0.20 // Fluid, graceful fall
                                        };
                                        
                                        output[band_idx] = prev_values[band_idx] * smooth_factor + target * (1.0 - smooth_factor);
                                        output[band_idx] = output[band_idx].min(1.0).max(0.18);
                                        prev_values[band_idx] = output[band_idx];
                                    }

                                    // Emit to frontend only if media is playing
                                    if ANY_MEDIA_PLAYING.load(std::sync::atomic::Ordering::Relaxed) {
                                        let _ = app_handle.emit("audio-visualization", AudioVisualizationData {
                                            frequencies: output.to_vec(),
                                        });
                                    }

                                    // Reset buffer for next batch
                                    buffer_pos = 0;
                                }
                            }

                            // Release buffer
                            let _ = capture_client.ReleaseBuffer(num_frames);
                        }
                    }
                }
            })();

            if com_initialized {
                CoUninitialize();
            }

            if let Err(_e) = result {
                // Silently handle error in release
            }
        }
    });
}


// Brightness event
#[derive(Clone, serde::Serialize)]
struct BrightnessChangeEvent {
    brightness: u32,
}

static BRIGHTNESS_SENDER: std::sync::OnceLock<std::sync::mpsc::Sender<u32>> = std::sync::OnceLock::new();
static CURRENT_BRIGHTNESS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(50);
static LAST_BRIGHTNESS_CHANGE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static ANY_MEDIA_PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn get_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// Refactored handle_volume_key to just send a command
fn handle_volume_key_event(vk_code: VIRTUAL_KEY) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let cmd = match vk_code {
                VK_VOLUME_MUTE => Some(SystemCommand::VolumeMute),
                VK_VOLUME_UP => Some(SystemCommand::VolumeUp),
                VK_VOLUME_DOWN => Some(SystemCommand::VolumeDown),
                _ => None,
            };
            if let Some(cmd) = cmd {
                // println!("Bloom Hook: Sending volume command: {:?}", vk_code);
                let _ = sender.send(cmd);
            }
        }
    }
}

fn handle_brightness_key_event(vk_code: VIRTUAL_KEY) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let cmd = if vk_code.0 == 0x216 || vk_code.0 == 0x7A { // 0x7A = VK_F11
                Some(SystemCommand::BrightnessDown)
            } else if vk_code.0 == 0x217 || vk_code.0 == 0x7B { // 0x7B = VK_F12
                Some(SystemCommand::BrightnessUp)
            } else {
                None
            };
            if let Some(cmd) = cmd {
                // println!("Bloom Hook: Sending brightness command: {:?}", vk_code);
                let _ = sender.send(cmd);
            }
        }
    }
}

fn setup_system_worker(app_handle: AppHandle) -> Sender<SystemCommand> {
    let (tx, rx) = channel::<SystemCommand>();
    
    let handle = app_handle.clone();
    std::thread::spawn(move || {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        use windows::Win32::Media::Audio::{IMMDeviceEnumerator, eRender, eConsole};
        use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus};

        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator = CoCreateInstance::<_, IMMDeviceEnumerator>(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL).ok();
            let device = enumerator.as_ref().and_then(|e| e.GetDefaultAudioEndpoint(eRender, eConsole).ok());
            let audio_endpoint_volume = device.as_ref().and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok());

            let mut manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()).ok();

            let mut last_processed_media = std::time::Instant::now();
            let mut last_emitted_info: Option<(String, String, bool, bool, Option<String>)> = None;
            let mut last_volume: f32 = -1.0;
            let mut last_muted: bool = false;

            // Helper to try hiding native Windows OSDs
            let hide_osd = || {
                use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
                let class1 = windows::core::PCSTR(b"NativeHWNDHost\0".as_ptr());
                if let Ok(hwnd1) = FindWindowA(class1, windows::core::PCSTR::null()) {
                    let _ = ShowWindow(hwnd1, SW_HIDE);
                }
            };

            loop {
                // Periodically try to get manager if it failed
                if manager.is_none() {
                    manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()).ok();
                }

                // Check for volume commands from hook
                while let Ok(cmd) = rx.try_recv() {
                    if let Some(ref aev) = audio_endpoint_volume {
                        match cmd {
                            SystemCommand::VolumeMute => {
                                if let Ok(muted) = aev.GetMute() {
                                    let _ = aev.SetMute(!muted.as_bool(), std::ptr::null());
                                    hide_osd();
                                }
                            }
                            SystemCommand::VolumeUp => {
                                if let Ok(vol) = aev.GetMasterVolumeLevelScalar() {
                                    let _ = aev.SetMasterVolumeLevelScalar((vol + 0.05).min(1.0), std::ptr::null());
                                    hide_osd();
                                }
                            }
                            SystemCommand::VolumeDown => {
                                if let Ok(vol) = aev.GetMasterVolumeLevelScalar() {
                                    let _ = aev.SetMasterVolumeLevelScalar((vol - 0.05).max(0.0), std::ptr::null());
                                    hide_osd();
                                }
                            }
                            SystemCommand::MediaPlayPause => {
                                if let Some(ref mgr) = manager {
                                    if let Ok(session) = mgr.GetCurrentSession() {
                                        let _ = session.TryTogglePlayPauseAsync();
                                    }
                                }
                            }
                            SystemCommand::MediaNext => {
                                if let Some(ref mgr) = manager {
                                    if let Ok(session) = mgr.GetCurrentSession() {
                                        let _ = session.TrySkipNextAsync();
                                    }
                                }
                            }
                            SystemCommand::MediaPrevious => {
                                if let Some(ref mgr) = manager {
                                    if let Ok(session) = mgr.GetCurrentSession() {
                                        let _ = session.TrySkipPreviousAsync();
                                    }
                                }
                            }
                            SystemCommand::ToggleVisibility(visible) => {
                                let _ = handle.emit("visibility-change", visible);
                                // For the bottom corners, we can just hide the window directly for 100% reliability
                                if let Some(w) = handle.get_webview_window("bottom-corners") {
                                    if visible { let _ = w.show(); } else { let _ = w.hide(); }
                                }
                            }
                            SystemCommand::BrightnessUp => {
                                let new_val = (CURRENT_BRIGHTNESS.load(std::sync::atomic::Ordering::Relaxed) + 10).min(100);
                                CURRENT_BRIGHTNESS.store(new_val, std::sync::atomic::Ordering::Relaxed);
                                LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), std::sync::atomic::Ordering::Relaxed);
                                let _ = handle.emit("brightness-change", BrightnessChangeEvent { brightness: new_val });
                                
                                if let Some(tx) = BRIGHTNESS_SENDER.get() {
                                    let _ = tx.send(new_val);
                                }
                                hide_osd();
                            }
                            SystemCommand::BrightnessDown => {
                                let current = CURRENT_BRIGHTNESS.load(std::sync::atomic::Ordering::Relaxed);
                                let new_val = if current > 10 { current - 10 } else { 0 };
                                CURRENT_BRIGHTNESS.store(new_val, std::sync::atomic::Ordering::Relaxed);
                                LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), std::sync::atomic::Ordering::Relaxed);
                                let _ = handle.emit("brightness-change", BrightnessChangeEvent { brightness: new_val });
                                
                                if let Some(tx) = BRIGHTNESS_SENDER.get() {
                                    let _ = tx.send(new_val);
                                }
                                hide_osd();
                            }
                        }
                    }
                }

                // Poll volume state and emit events (60fps)
                if let Some(ref aev) = audio_endpoint_volume {
                    if let (Ok(vol), Ok(muted)) = (aev.GetMasterVolumeLevelScalar(), aev.GetMute()) {
                        let is_muted: bool = muted.into();
                        if (vol - last_volume).abs() > 0.001 || is_muted != last_muted {
                            last_volume = vol;
                            last_muted = is_muted;
                            let _ = handle.emit("volume-change", VolumeChangeEvent { volume: vol, is_muted });
                            // Aggressively hide native Windows volume OSD
                            hide_osd();
                        }
                    }
                }

                // Poll Media Info (every 2 seconds)
                if last_processed_media.elapsed().as_millis() >= 2000 {
                    last_processed_media = std::time::Instant::now();
                    
                    let mut best_session_info: Option<MediaInfo> = None;
                    
                    if let Some(ref mgr) = manager {
                        if let Ok(sessions) = mgr.GetSessions() {
                            let count = sessions.Size().unwrap_or(0);
                            for i in 0..count {
                                if let Ok(session) = sessions.GetAt(i) {
                                    let playback_info = session.GetPlaybackInfo().ok();
                                    let is_playing = playback_info.and_then(|p| p.PlaybackStatus().ok()) == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
                                    
                                    if let Ok(media_props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
                                        let title = media_props.Title().unwrap_or_default().to_string();
                                        let artist = media_props.Artist().unwrap_or_default().to_string();
                                        let has_media = !title.is_empty();

                                        if has_media {
                                            // Extract artwork if possible
                                            let artwork = (|| -> Option<Vec<String>> {
                                                let thumb = media_props.Thumbnail().ok()?;
                                                let stream = thumb.OpenReadAsync().ok()?.get().ok()?;
                                                let mime = stream.ContentType().ok()?.to_string();
                                                let size = stream.Size().ok()? as u32;
                                                if size == 0 { return None; }
                                                let reader = DataReader::CreateDataReader(&stream).ok()?;
                                                reader.LoadAsync(size).ok()?.get().ok()?;
                                                let mut bytes = vec![0u8; size as usize];
                                                reader.ReadBytes(&mut bytes).ok()?;
                                                Some(vec![format!("data:{};base64,{}", mime, general_purpose::STANDARD.encode(bytes))])
                                            })();

                                            let info = MediaInfo { title, artist, is_playing, has_media, artwork };
                                            
                                            if is_playing {
                                                best_session_info = Some(info);
                                                break; // Prioritize playing session
                                            } else if best_session_info.is_none() {
                                                best_session_info = Some(info);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let current_info = best_session_info.unwrap_or(MediaInfo { 
                        title: "".into(), artist: "".into(), is_playing: false, has_media: false, artwork: None 
                    });

                    let artwork_str = current_info.artwork.as_ref().and_then(|a| a.first()).cloned();
                    let needs_emit = match &last_emitted_info {
                        Some((t, a, p, h, art)) => {
                            t != &current_info.title || 
                            a != &current_info.artist || 
                            p != &current_info.is_playing || 
                            h != &current_info.has_media || 
                            art != &artwork_str
                        },
                        None => true
                    };

                    if needs_emit {
                        let _ = handle.emit("media-update", current_info.clone());
                        ANY_MEDIA_PLAYING.store(current_info.is_playing, std::sync::atomic::Ordering::Relaxed);
                        last_emitted_info = Some((
                            current_info.title, 
                            current_info.artist, 
                            current_info.is_playing, 
                            current_info.has_media, 
                            artwork_str
                        ));
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        }
    });

    // Dedicated brightness polling thread to prevent blocking the system worker
    let handle_brightness = app_handle.clone();
    std::thread::spawn(move || {
        let com_lib = match COMLibrary::new() {
            Ok(lib) => lib,
            Err(_) => return,
        };

        let wmi_con = match WMIConnection::with_namespace_path("root\\WMI", com_lib) {
            Ok(con) => con,
            Err(_) => return,
        };

        // Initial fetch
        let mut last_brightness = match wmi_con.query::<WmiMonitorBrightness>() {
            Ok(results) => results.first().map(|b| b.current_brightness as u32).unwrap_or(50),
            Err(_) => 50,
        };
        CURRENT_BRIGHTNESS.store(last_brightness, std::sync::atomic::Ordering::Relaxed);

        loop {
            // Ignore poller if a manual change was made recently (2 second cooldown)
            let now = get_now_ms();
            let last_change = LAST_BRIGHTNESS_CHANGE.load(std::sync::atomic::Ordering::Relaxed);
            if now - last_change < 2000 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                continue;
            }

            if let Ok(results) = wmi_con.query::<WmiMonitorBrightness>() {
                if let Some(b) = results.first() {
                    let brightness = b.current_brightness as u32;
                    if brightness != last_brightness {
                        last_brightness = brightness;
                        CURRENT_BRIGHTNESS.store(brightness, std::sync::atomic::Ordering::Relaxed);
                        let _ = handle_brightness.emit("brightness-change", BrightnessChangeEvent { brightness });
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    });

    // Spawn a dedicated thread for fullscreen detection
    let tx_clone = tx.clone();
    std::thread::spawn(move || {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        use windows::Win32::Foundation::RECT;

        let mut last_visible = true;
        loop {
            unsafe {
                let hwnd = GetForegroundWindow();
                if !hwnd.is_invalid() {
                    // Check if focused window is desktop (Progman or WorkerW)
                    let mut class_name = [0u8; 256];
                    let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
                    let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");
                    let is_desktop = class_str == "Progman" || class_str == "WorkerW";

                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).is_ok() && !is_desktop {
                        let cx = GetSystemMetrics(SM_CXSCREEN);
                        let cy = GetSystemMetrics(SM_CYSCREEN);
                        // A window is fullscreen if it covers (or exceeds) the whole screen
                        let is_fs = rect.left <= 0 && rect.top <= 0 && rect.right >= cx && rect.bottom >= cy;
                        
                        if is_fs && last_visible {
                            let _ = tx_clone.send(SystemCommand::ToggleVisibility(false));
                            last_visible = false;
                        } else if !is_fs && !last_visible {
                            let _ = tx_clone.send(SystemCommand::ToggleVisibility(true));
                            last_visible = true;
                        }
                    } else if is_desktop && !last_visible {
                        // If we are on desktop, we should be visible
                        let _ = tx_clone.send(SystemCommand::ToggleVisibility(true));
                        last_visible = true;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    tx
}

// Low-level keyboard hook to intercept volume keys and hide native OSD

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: windows::Win32::Foundation::WPARAM, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT;
    use windows::Win32::UI::WindowsAndMessaging::{WM_KEYDOWN, WM_SYSKEYDOWN};

    if code >= 0 {
        let kbd_struct = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk_code = VIRTUAL_KEY(kbd_struct.vkCode as u16);

        if vk_code == VK_VOLUME_MUTE || vk_code == VK_VOLUME_UP || vk_code == VK_VOLUME_DOWN {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize {
                handle_volume_key_event(vk_code);
            }
            return windows::Win32::Foundation::LRESULT(1);
        }

        if vk_code.0 == 0x216 || vk_code.0 == 0x217 || vk_code.0 == 0x7A || vk_code.0 == 0x7B {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize {
                handle_brightness_key_event(vk_code);
            }
            return windows::Win32::Foundation::LRESULT(1);
        }
    }

    windows::Win32::UI::WindowsAndMessaging::CallNextHookEx(None, code, wparam, lparam)
}

fn setup_keyboard_hook() -> windows::Win32::UI::WindowsAndMessaging::HHOOK {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExA, WH_KEYBOARD_LL};

    unsafe {
        let hook = SetWindowsHookExA(WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0)
            .expect("Failed to set keyboard hook");
        hook
    }
}

#[derive(Clone, serde::Serialize)]
struct VolumeChangeEvent {
    volume: f32,
    is_muted: bool,
}

fn setup_brightness_worker() {
    let (tx, rx) = channel::<u32>();
    let _ = BRIGHTNESS_SENDER.set(tx);

    std::thread::spawn(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let child = Command::new("powershell")
            .args(&["-NoProfile", "-NoLogo", "-Command", "-"])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();

        if let Some(mut child) = child {
            if let Some(mut stdin) = child.stdin.take() {
                // Pre-cache the WMI monitor methods instance for instant future calls
                let init_cmd = "$m = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods\n";
                let _ = stdin.write_all(init_cmd.as_bytes());
                let _ = stdin.flush();

                while let Ok(brightness) = rx.recv() {
                    // Use the cached instance to change brightness instantly
                    let cmd = format!("$m | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Brightness={}; Timeout=0}}\n", brightness);
                    let _ = stdin.write_all(cmd.as_bytes());
                    let _ = stdin.flush();
                }
            }
            let _ = child.kill();
        }
    });
}

fn main() {
    // Initialize brightness worker before anything else
    setup_brightness_worker();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .invoke_handler(tauri::generate_handler![
            broadcast_setting,
            hide_native_osd,
            open_settings_window,
            toggle_corners_window,
            open_wifi_settings,
            open_notification_center,
            set_ignore_cursor_events,
            set_window_height,
            hide_volume_overlay,
            hide_brightness_overlay,
            media_play_pause,
            media_next,
            media_previous
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            let hwnd = window.hwnd().unwrap();
            register_appbar(hwnd);

            // Configure bottom corners window (it is already created by tauri.conf.json)
            if let Some(bc_win) = app.get_webview_window("bottom-corners") {
                let _ = bc_win.set_ignore_cursor_events(true);
                let hwnd = bc_win.hwnd().unwrap();
                register_bottom_appbar(hwnd);
            }

            if let Some(br_win) = app.get_webview_window("brightness-overlay") {
                if let Ok(Some(monitor)) = br_win.primary_monitor() {
                    let size = monitor.size();
                    let pos = monitor.position();
                    let scale = monitor.scale_factor();
                    let physical_width = (200.0 * scale) as i32;
                    let physical_height = (400.0 * scale) as i32;
                    let physical_y = pos.y + (size.height as i32 / 2) - (physical_height / 2);
                    
                    let _ = br_win.set_position(tauri::PhysicalPosition::new(
                        pos.x + (size.width as i32 - physical_width),
                        physical_y
                    ));
                }
            }

            let tx = setup_system_worker(app.handle().clone());
            unsafe { COMMAND_SENDER = Some(tx.clone()); }

            let _hook = setup_keyboard_hook();
            setup_audio_visualization(app.handle().clone());

            // Setup settings window effects and lifecycle
            if let Some(settings_win) = app.get_webview_window("settings") {
                // Apply Mica effect on Windows 11
                #[cfg(target_os = "windows")]
                {
                    let _ = window_vibrancy::apply_mica(&settings_win, None);
                }
                
                let win_clone = settings_win.clone();
                settings_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                });
            }

            // System tray icon with context menu
            {
                use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};
                use tauri::menu::{Menu, MenuItem};

                let quit_item = MenuItem::with_id(app, "quit", "Quit Bloom", true, None::<&str>)?;
                let restart_item = MenuItem::with_id(app, "restart", "Restart Bloom", true, None::<&str>)?;
                let settings_item = MenuItem::with_id(app, "settings", "Open Settings", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&settings_item, &restart_item, &quit_item])?;

                let app_handle_tray = app.handle().clone();
                TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("Bloom")
                    .menu(&menu)
                    .on_menu_event(move |_tray, event| {
                        match event.id().as_ref() {
                            "quit" => {
                                app_handle_tray.exit(0);
                            }
                            "restart" => {
                                app_handle_tray.restart();
                            }
                            "settings" => {
                                if let Some(win) = app_handle_tray.get_webview_window("settings") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                            let app = tray.app_handle();
                            if let Some(win) = app.get_webview_window("settings") {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn register_appbar(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, GetSystemMetrics, SM_CXSCREEN};
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
        use windows::Win32::Foundation::RECT;

        let screen_width = GetSystemMetrics(SM_CXSCREEN);

        let mut abd = APPBARDATA::default();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        abd.hWnd = hwnd;

        SHAppBarMessage(ABM_NEW, &mut abd);

        abd.uEdge = ABE_TOP;

        abd.rc = RECT {
            left: 0,
            top: 0,
            right: screen_width,
            bottom: 48,
        };

        SHAppBarMessage(ABM_SETPOS, &mut abd);

        let _ = SetWindowPos(
            hwnd,
            None,
            abd.rc.left,
            abd.rc.top,
            abd.rc.right - abd.rc.left,
            48, // Match reservation height
            SWP_NOZORDER,
        );
    }
}

fn register_bottom_appbar(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
        use windows::Win32::Foundation::RECT;

        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);

        let mut abd = APPBARDATA::default();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        abd.hWnd = hwnd;

        SHAppBarMessage(ABM_NEW, &mut abd);

        abd.uEdge = windows::Win32::UI::Shell::ABE_BOTTOM;

        // Register at the absolute bottom with 0 height reserved
        abd.rc = RECT {
            left: 0,
            top: screen_height - 1,
            right: screen_width,
            bottom: screen_height,
        };

        SHAppBarMessage(ABM_SETPOS, &mut abd);

        let _ = SetWindowPos(
            hwnd, 
            None, 
            0, 
            screen_height - 48, 
            screen_width, 
            48, 
            SWP_NOZORDER
        );
    }
}
