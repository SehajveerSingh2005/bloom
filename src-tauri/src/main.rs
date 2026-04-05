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
fn toggle_corners_window(app: tauri::AppHandle, show: bool) {
    if let Some(win) = app.get_webview_window("bottom-corners") {
        if show {
            // Position at bottom of primary monitor
            if let Ok(Some(monitor)) = win.primary_monitor() {
                let size = monitor.size();
                let _ = win.set_position(tauri::PhysicalPosition::new(0, (size.height - 40) as i32));
                let _ = win.set_size(tauri::PhysicalSize::new(size.width, 40));
            }
            let _ = win.show();
            let _ = win.set_always_on_top(true);
        } else {
            let _ = win.hide();
        }
    }
}

#[tauri::command]
fn broadcast_setting(app: tauri::AppHandle, key: String, value: bool) {
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
                
                // Per-band gain to balance visual output
                
                // History for smoothing
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

                                // Read and mix channels - use wrapping to prevent overflow
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
                                    // Simple DFT for frequency analysis (5 bands)
                                    let mut bands = [0.0f32; NUM_BANDS];
                                    
                                    // Shifted bands lower to better capture music frequencies (93Hz per bin at 48kHz)
                                    let band_ranges = [
                                        (1, 3),    // Bass / Beat
                                        (3, 10),   // Low-mids / Vocals
                                        (10, 30),  // Mids / Snare
                                        (30, 80),  // High-mids / Perk
                                        (80, 250), // Highs / Cymbals
                                    ];

                                    for (band_idx, (bin_start, bin_end)) in band_ranges.iter().enumerate() {
                                        let mut peak_mag = 0.0f32;
                                        
                                        for bin in *bin_start..*bin_end {
                                            if bin >= FFT_SIZE / 2 { break; }
                                            
                                            let freq = 2.0 * std::f32::consts::PI * bin as f32 / FFT_SIZE as f32;
                                            let mut real = 0.0f32;
                                            let mut imag = 0.0f32;
                                            
                                            for (sample_idx, &sample) in fft_buffer.iter().enumerate() {
                                                let phase = freq * sample_idx as f32;
                                                real += sample * phase.cos();
                                                imag -= sample * phase.sin();
                                            }
                                            
                                            let mag = (real * real + imag * imag).sqrt();
                                            if mag > peak_mag {
                                                peak_mag = mag;
                                            }
                                        }
                                        
                                        // Peak normalization and band gains
                                        let normalization = 110.0; 
                                        let gain_multiplier = [2.0, 1.8, 2.2, 3.5, 5.5];
                                        bands[band_idx] = (peak_mag / normalization) * gain_multiplier[band_idx];
                                    }

                                    // Apply response curve and smoothing
                                    let mut output = [0.0f32; NUM_BANDS];
                                    for i in 0..NUM_BANDS {
                                        // More aggressive response to keep bars dancing
                                        let target = (bands[i] * 1.5).powf(0.5);
                                        // Snappy 20/80 smoothing for high reactivity
                                        output[i] = prev_values[i] * 0.2 + target * 0.8;
                                        output[i] = output[i].min(1.0).max(0.18);
                                        prev_values[i] = output[i];
                                    }

                                    // Emit to frontend (one event per full FFT buffer)
                                    let _ = app_handle.emit("audio-visualization", AudioVisualizationData {
                                        frequencies: output.to_vec(),
                                    });

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

            if let Err(e) = result {
                eprintln!("Audio visualization error: {}", e);
            }
        }
    });
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
                    
                    let mut media_emitted = false;
                    if let Some(ref mgr) = manager {
                        let sessions = mgr.GetSessions().ok();
                        if let Some(sessions) = sessions {
                            let count = sessions.Size().unwrap_or(0);
                             for i in 0..count {
                                if let Ok(session) = sessions.GetAt(i) {
                                    let playback_info = session.GetPlaybackInfo().ok();
                                    let is_playing = playback_info.and_then(|p| p.PlaybackStatus().ok()) == Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
                                    
                                    if let Ok(media_props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
                                        let title = media_props.Title().unwrap_or_default().to_string();
                                        let artist = media_props.Artist().unwrap_or_default().to_string();
                                        let has_media = !title.is_empty();

                                        if has_media && (is_playing || !media_emitted) {
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

                                            let _ = handle.emit("media-update", MediaInfo { title, artist, is_playing, has_media, artwork });
                                            media_emitted = true;
                                            if is_playing { break; } // Prioritize playing session
                                        }
                                    }
                                }
                             }
                        }
                    }

                    if !media_emitted {
                        // println!("Bloom Media: No media active");
                        let _ = handle.emit("media-update", MediaInfo { title: "".into(), artist: "".into(), is_playing: false, has_media: false, artwork: None });
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(16));
            }
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

fn main() {
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
                if let Ok(Some(monitor)) = bc_win.primary_monitor() {
                   let size = monitor.size();
                   let _ = bc_win.set_position(tauri::PhysicalPosition::new(0, (size.height - 40) as i32));
                   let _ = bc_win.set_size(tauri::PhysicalSize::new(size.width, 40));
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

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn register_appbar(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::SetWindowPos;
        use windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER;
        use windows::Win32::Foundation::RECT;

        let mut abd = APPBARDATA::default();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        abd.hWnd = hwnd;

        SHAppBarMessage(ABM_NEW, &mut abd);

        abd.uEdge = ABE_TOP;

        abd.rc = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 48, // Slightly more headroom
        };

        SHAppBarMessage(ABM_SETPOS, &mut abd);

        let _ = SetWindowPos(
            hwnd,
            None,
            abd.rc.left,
            abd.rc.top,
            abd.rc.right - abd.rc.left,
            40, // Match reserved height
            SWP_NOZORDER,
        );
    }
}
