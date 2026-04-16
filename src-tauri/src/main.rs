// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::UI::Shell::{
    SHAppBarMessage, APPBARDATA,
    ABM_NEW, ABM_SETPOS, ABM_REMOVE, ABM_QUERYPOS,
    ABE_TOP, ABE_BOTTOM,
};
use windows::Win32::UI::Shell::ShellExecuteA;
use windows::Win32::Foundation::HWND;
use windows::core::BOOL;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_VOLUME_MUTE, VK_VOLUME_UP, VK_VOLUME_DOWN,
};
use windows::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_C_EVENT, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Storage::Streams::DataReader;
use base64::{Engine as _, engine::general_purpose};
use std::sync::mpsc::{channel, Sender};
use wmi::{COMLibrary, WMIConnection};
use serde::Deserialize;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering};

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
static MAIN_APPBAR_REGISTERED: AtomicBool = AtomicBool::new(false);

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


fn set_taskbar_visibility(visible: bool) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE, SW_SHOW};
        let tray_class = windows::core::PCSTR(b"Shell_TrayWnd\0".as_ptr());
        let secondary_tray_class = windows::core::PCSTR(b"Shell_SecondaryTrayWnd\0".as_ptr());

        if let Ok(tray_hwnd) = FindWindowA(tray_class, windows::core::PCSTR::null()) {
            let _ = ShowWindow(tray_hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
        if let Ok(secondary_tray_hwnd) = FindWindowA(secondary_tray_class, windows::core::PCSTR::null()) {
            let _ = ShowWindow(secondary_tray_hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }
}

#[tauri::command]
fn toggle_dock(app: tauri::AppHandle, enable: bool) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        let hwnd = dock_win.hwnd().unwrap();
        if enable {
            set_taskbar_visibility(false);
            let _ = dock_win.show();
            let _ = dock_win.set_always_on_top(true);
        } else {
            let _ = dock_win.hide();
            unregister_appbar_native(hwnd);
            set_taskbar_visibility(true);
            // Re-sync other appbars to ensure they stay in place
            if let Some(main_win) = app.get_webview_window("main") {
                register_appbar(main_win);
            }
        }
    }
}

#[tauri::command]
fn sync_appbar(window: tauri::WebviewWindow) {
    let label = window.label().to_string();
    if label == "main" {
        register_appbar(window);
    }
}

#[tauri::command]
fn change_dock_mode(app: tauri::AppHandle, mode: String) {
    if let Some(dock_win) = app.get_webview_window("dock") {
        let hwnd = dock_win.hwnd().unwrap();
        let scale = dock_win.scale_factor().unwrap_or(1.0);
        
        if mode == "fixed" {
            register_dock_appbar(dock_win.clone());
        } else {
            unregister_appbar_native(hwnd);
            if let Ok(Some(monitor)) = dock_win.primary_monitor() {
                let m_size = monitor.size();
                let m_pos = monitor.position();
                let ph = (100.0 * scale) as i32;
                
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE as WS_EX_NA};
                    // Apply tool window styles to ignore work area pushes
                    let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
                    ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
                    let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);

                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        m_pos.x,
                        m_pos.y + m_size.height as i32 - ph,
                        m_size.width as i32,
                        ph,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    let _ = dock_win.show();
                }
            }
        }
        let _ = dock_win.set_always_on_top(true);
    }
}

#[tauri::command]
fn open_app(app_name: String) {
    use std::process::Command;
    match app_name.as_str() {
        "start" => {
            unsafe {
                use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, VK_LWIN, KEYEVENTF_KEYUP};
                keybd_event(VK_LWIN.0 as u8, 0, Default::default(), 0);
                keybd_event(VK_LWIN.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            }
        }
        "explorer" => { let _ = Command::new("explorer").spawn(); }
        "terminal" => { let _ = Command::new("wt.exe").spawn(); }
        "vscode" => { let _ = Command::new("cmd").args(["/C", "code"]).spawn(); }
        "browser" => { let _ = Command::new("cmd").args(["/C", "start", "https://google.com"]).spawn(); }
        _ => {}
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

            let _result: Result<(), String> = (|| {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("CoCreateInstance failed: {:?}", e))?;

                let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                    .map_err(|e| format!("GetDefaultAudioEndpoint failed: {:?}", e))?;

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
                const NUM_BANDS: usize = 5;
                let mut max_band_energies = [0.01f32; NUM_BANDS];
                let mut prev_values = [0.1f32; NUM_BANDS];

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                    loop {
                        let packet_length = match capture_client.GetNextPacketSize() {
                            Ok(len) => len,
                            Err(_) => break,
                        };
                        if packet_length == 0 { break; }
                        let mut data_ptr: *mut u8 = std::ptr::null_mut();
                        let mut num_frames = 0u32;
                        let mut flags = 0u32;

                        if capture_client.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None).is_err() {
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
                                        for bin in bin_start..bin_end {
                                            if bin >= FFT_SIZE / 2 { break; }
                                            let freq = 2.0 * std::f32::consts::PI * bin as f32 / FFT_SIZE as f32;
                                            let mut real = 0.0f32;
                                            let mut imag = 0.0f32;
                                            for (sample_idx, &sample) in fft_buffer.iter().enumerate() {
                                                let phase = freq * sample_idx as f32;
                                                real += sample * phase.cos();
                                                imag -= sample * phase.sin();
                                            }
                                            total_mag += (real * real + imag * imag).sqrt();
                                        }
                                        let mut avg_mag = total_mag / (bin_end - bin_start) as f32;
                                        let weighting = [1.2, 1.2, 1.5, 2.8, 5.0];
                                        avg_mag *= weighting[band_idx];
                                        if avg_mag > max_band_energies[band_idx] {
                                            max_band_energies[band_idx] = avg_mag;
                                        } else {
                                            max_band_energies[band_idx] *= 0.99;
                                        }
                                        let target = (avg_mag / max_band_energies[band_idx].max(0.12)).min(1.0).powf(0.75);
                                        let is_rising = target > prev_values[band_idx];
                                        let smooth_factor = if is_rising { 0.10 } else { 0.20 };
                                        output[band_idx] = prev_values[band_idx] * smooth_factor + target * (1.0 - smooth_factor);
                                        output[band_idx] = output[band_idx].min(1.0).max(0.18);
                                        prev_values[band_idx] = output[band_idx];
                                    }
                                    if ANY_MEDIA_PLAYING.load(Ordering::Relaxed) {
                                        let _ = app_handle.emit("audio-visualization", AudioVisualizationData { frequencies: output.to_vec() });
                                    }
                                    buffer_pos = 0;
                                }
                            }
                            let _ = capture_client.ReleaseBuffer(num_frames);
                        }
                    }
                }
            })();
            if com_initialized { CoUninitialize(); }
        }
    });
}

#[derive(Clone, serde::Serialize)]
struct BrightnessChangeEvent { brightness: u32 }
static BRIGHTNESS_SENDER: std::sync::OnceLock<std::sync::mpsc::Sender<u32>> = std::sync::OnceLock::new();
static CURRENT_BRIGHTNESS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(50);
static LAST_BRIGHTNESS_CHANGE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static ANY_MEDIA_PLAYING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn get_now_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64 }

fn handle_volume_key_event(vk_code: VIRTUAL_KEY) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
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

fn handle_brightness_key_event(vk_code: VIRTUAL_KEY) {
    unsafe {
        if let Some(ref sender) = COMMAND_SENDER {
            let cmd = if vk_code.0 == 0x216 || vk_code.0 == 0x7A { Some(SystemCommand::BrightnessDown) }
            else if vk_code.0 == 0x217 || vk_code.0 == 0x7B { Some(SystemCommand::BrightnessUp) }
            else { None };
            if let Some(cmd) = cmd { let _ = sender.send(cmd); }
        }
    }
}

fn setup_system_worker(app_handle: AppHandle) -> Sender<SystemCommand> {
    let (tx, rx) = channel::<SystemCommand>();
    let handle_system = app_handle.clone();
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

            let hide_osd = || {
                use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
                let class1 = windows::core::PCSTR(b"NativeHWNDHost\0".as_ptr());
                if let Ok(hwnd1) = FindWindowA(class1, windows::core::PCSTR::null()) { let _ = ShowWindow(hwnd1, SW_HIDE); }
            };

            loop {
                if manager.is_none() { manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()).ok(); }
                while let Ok(cmd) = rx.try_recv() {
                    if let Some(ref aev) = audio_endpoint_volume {
                        match cmd {
                            SystemCommand::VolumeMute => { if let Ok(muted) = aev.GetMute() { let _ = aev.SetMute(!muted.as_bool(), std::ptr::null()); hide_osd(); } }
                            SystemCommand::VolumeUp => { if let Ok(vol) = aev.GetMasterVolumeLevelScalar() { let _ = aev.SetMasterVolumeLevelScalar((vol + 0.05).min(1.0), std::ptr::null()); hide_osd(); } }
                            SystemCommand::VolumeDown => { if let Ok(vol) = aev.GetMasterVolumeLevelScalar() { let _ = aev.SetMasterVolumeLevelScalar((vol - 0.05).max(0.0), std::ptr::null()); hide_osd(); } }
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
                std::thread::sleep(std::time::Duration::from_millis(16));
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
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, IsZoomed};
        use windows::Win32::Foundation::RECT;
        let mut last_visible = true;
        let mut last_dock_overlap = false;
        loop {
            unsafe {
                let hwnd = GetForegroundWindow();
                if !hwnd.is_invalid() {
                    let mut class_name = [0u8; 256];
                    let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
                    let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");
                    let is_desktop = class_str == "Progman" || class_str == "WorkerW";
                    let mut rect = RECT::default();
                    if GetWindowRect(hwnd, &mut rect).is_ok() && !is_desktop {
                        let is_fs = rect.left <= 0 && rect.top <= 0 && rect.right >= GetSystemMetrics(SM_CXSCREEN) && rect.bottom >= GetSystemMetrics(SM_CYSCREEN);
                        
                        // Dock overlap detection: FS, Maximized, or physically overlapping the bottom 100px
                        let is_maximized = IsZoomed(hwnd).as_bool();
                        let overlaps_dock = rect.bottom > (GetSystemMetrics(SM_CYSCREEN) - 100);
                        let should_overlap = is_fs || is_maximized || overlaps_dock;

                        if should_overlap != last_dock_overlap {
                            let _ = handle_visibility.emit("dock-overlap", should_overlap);
                            last_dock_overlap = should_overlap;
                        }

                        if is_fs && last_visible { let _ = tx_clone.send(SystemCommand::ToggleVisibility(false)); last_visible = false; }
                        else if !is_fs && !last_visible { let _ = tx_clone.send(SystemCommand::ToggleVisibility(true)); last_visible = true; }
                    } else if is_desktop {
                        if !last_visible { let _ = tx_clone.send(SystemCommand::ToggleVisibility(true)); last_visible = true; }
                        if last_dock_overlap { let _ = handle_visibility.emit("dock-overlap", false); last_dock_overlap = false; }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
    tx
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: windows::Win32::Foundation::WPARAM, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN};
    if code >= 0 {
        let vk_code = VIRTUAL_KEY((*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode as u16);
        if vk_code == VK_VOLUME_MUTE || vk_code == VK_VOLUME_UP || vk_code == VK_VOLUME_DOWN {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize { handle_volume_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
        if vk_code.0 == 0x216 || vk_code.0 == 0x217 || vk_code.0 == 0x7A || vk_code.0 == 0x7B {
            if wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize { handle_brightness_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
    }
    windows::Win32::UI::WindowsAndMessaging::CallNextHookEx(None, code, wparam, lparam)
}

fn setup_keyboard_hook() -> windows::Win32::UI::WindowsAndMessaging::HHOOK {
    unsafe { windows::Win32::UI::WindowsAndMessaging::SetWindowsHookExA(windows::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0).expect("Failed") }
}

#[derive(Clone, serde::Serialize)]
struct VolumeChangeEvent { volume: f32, is_muted: bool }

fn setup_brightness_worker() {
    let (tx, rx) = channel::<u32>();
    let _ = BRIGHTNESS_SENDER.set(tx);
    std::thread::spawn(move || {
        use std::io::Write;
        use std::process::{Command, Stdio};
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

unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> BOOL {
    if ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT || ctrl_type == CTRL_CLOSE_EVENT {
        set_taskbar_visibility(true);
    }
    BOOL(0)
}

fn main() {
    unsafe { let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true); }
    setup_brightness_worker();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![])))
        .invoke_handler(tauri::generate_handler![
            broadcast_setting, hide_native_osd, open_settings_window, open_wifi_settings,
            open_notification_center, set_ignore_cursor_events, set_window_height, hide_volume_overlay,
            hide_brightness_overlay, media_play_pause, media_next, media_previous, toggle_dock, change_dock_mode,
            sync_appbar, open_app
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            register_appbar(window.clone());
            if let Some(br_win) = app.get_webview_window("brightness-overlay") {
                if let Ok(Some(monitor)) = br_win.primary_monitor() {
                    let size = monitor.size();
                    let pos = monitor.position();
                    let scale = monitor.scale_factor();
                    let pw = (200.0 * scale) as i32;
                    let ph = (400.0 * scale) as i32;
                    let _ = br_win.set_position(tauri::PhysicalPosition::new(pos.x + (size.width as i32 - pw), pos.y + (size.height as i32 / 2) - (ph / 2)));
                }
            }
            let tx = setup_system_worker(app.handle().clone());
            unsafe { COMMAND_SENDER = Some(tx.clone()); }
            let _hook = setup_keyboard_hook();
            setup_audio_visualization(app.handle().clone());
            if let Some(settings_win) = app.get_webview_window("settings") {
                #[cfg(target_os = "windows")] { let _ = window_vibrancy::apply_mica(&settings_win, None); }
                let win_clone = settings_win.clone();
                settings_win.on_window_event(move |event| { if let tauri::WindowEvent::CloseRequested { api, .. } = event { api.prevent_close(); let _ = win_clone.hide(); } });
            }
            {
                use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};
                use tauri::menu::{Menu, MenuItem};
                let quit_item = MenuItem::with_id(app, "quit", "Quit Bloom", true, None::<&str>)?;
                let restart_item = MenuItem::with_id(app, "restart", "Restart Bloom", true, None::<&str>)?;
                let settings_item = MenuItem::with_id(app, "settings", "Open Settings", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&settings_item, &restart_item, &quit_item])?;
                let ah = app.handle().clone();
                TrayIconBuilder::new().icon(app.default_window_icon().unwrap().clone()).tooltip("Bloom").menu(&menu)
                    .on_menu_event(move |_, event| {
                        match event.id().as_ref() {
                            "quit" => { if let Some(w) = ah.get_webview_window("main") { unregister_appbar_native(w.hwnd().unwrap()); } if let Some(w) = ah.get_webview_window("dock") { unregister_appbar_native(w.hwnd().unwrap()); } set_taskbar_visibility(true); ah.exit(0); }
                            "restart" => { if let Some(w) = ah.get_webview_window("main") { unregister_appbar_native(w.hwnd().unwrap()); } if let Some(w) = ah.get_webview_window("dock") { unregister_appbar_native(w.hwnd().unwrap()); } set_taskbar_visibility(true); ah.restart(); }
                            "settings" => { if let Some(w) = ah.get_webview_window("settings") { let _ = w.show(); let _ = w.set_focus(); } }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| { if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event { if let Some(w) = tray.app_handle().get_webview_window("settings") { let _ = w.show(); let _ = w.set_focus(); } } })
                    .build(app)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|_, event| { if let tauri::RunEvent::Exit = event { set_taskbar_visibility(true); } });
}

fn register_appbar(window: tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let m_size = monitor.size();
        let m_pos = monitor.position();
        let hwnd = window.hwnd().unwrap();
        let scale = window.scale_factor().unwrap_or(1.0);
        let ph = (48.0 * scale) as i32;
        let pr = (40.0 * scale) as i32;
        
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE as WS_EX_NA, SWP_FRAMECHANGED};
            use windows::Win32::Foundation::RECT;
            
            // Set styles first
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
            ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);
            
            // Set position before registering
            let _ = SetWindowPos(hwnd, None, m_pos.x, m_pos.y, m_size.width as i32, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);

            let mut abd = APPBARDATA::default();
            abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
            abd.hWnd = hwnd;
            
            // Clean slate registration
            SHAppBarMessage(ABM_REMOVE, &mut abd);
            SHAppBarMessage(ABM_NEW, &mut abd);
            abd.uEdge = ABE_TOP;
            abd.rc = RECT { 
                left: m_pos.x, 
                top: m_pos.y, 
                right: m_pos.x + m_size.width as i32, 
                bottom: m_pos.y + pr
            };
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            SHAppBarMessage(ABM_SETPOS, &mut abd);
            
            let _ = window.show();
            MAIN_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
        }
    }
}

fn register_dock_appbar(window: tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let m_size = monitor.size();
        let m_pos = monitor.position();
        let hwnd = window.hwnd().unwrap();
        let scale = window.scale_factor().unwrap_or(1.0);
        let ph = (100.0 * scale) as i32;
        let pr = (56.0 * scale) as i32;
        
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE as WS_EX_NA, SWP_FRAMECHANGED};
            use windows::Win32::Foundation::RECT;

            // Set styles first
            let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
            ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);

            // Set position before showing
            let _ = SetWindowPos(hwnd, None, m_pos.x, m_pos.y + m_size.height as i32 - ph, m_size.width as i32, ph, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            let _ = window.show();

            let mut abd = APPBARDATA::default();
            abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
            abd.hWnd = hwnd;
            
            // Clean slate registration
            SHAppBarMessage(ABM_REMOVE, &mut abd);
            SHAppBarMessage(ABM_NEW, &mut abd);
            abd.uEdge = ABE_BOTTOM;
            abd.rc = RECT { 
                left: m_pos.x, 
                top: m_pos.y + m_size.height as i32 - pr, 
                right: m_pos.x + m_size.width as i32, 
                bottom: m_pos.y + m_size.height as i32 
            };
            SHAppBarMessage(ABM_QUERYPOS, &mut abd);
            SHAppBarMessage(ABM_SETPOS, &mut abd);
        }
    }
}

fn unregister_appbar_native(hwnd: HWND) {
    unsafe {
        let mut abd = APPBARDATA::default();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        abd.hWnd = hwnd;
        SHAppBarMessage(ABM_REMOVE, &mut abd);
    }
}
