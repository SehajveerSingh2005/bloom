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

#[tauri::command]
fn get_media_info() -> MediaInfo {
    get_system_media_info()
}

fn get_system_media_info() -> MediaInfo {
    unsafe {
        use windows::Win32::System::Com::{
            CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::Foundation::S_OK;

        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let com_initialized = hr.is_ok() || hr == S_OK;

        let result = (|| -> Option<MediaInfo> {
            use windows::Media::Control::{
                GlobalSystemMediaTransportControlsSessionManager,
                GlobalSystemMediaTransportControlsSessionPlaybackStatus,
            };

            let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .ok()?
                .get()
                .ok()?;

            let session = manager.GetCurrentSession().ok()?;

            let playback_info = session.GetPlaybackInfo().ok()?;
            let is_playing = playback_info.PlaybackStatus() == Ok(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);

            let media_props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;

            let title = media_props.Title().ok()?.to_string();
            let artist = media_props.Artist().ok()?.to_string();
            let has_media = !title.is_empty();

            Some(MediaInfo {
                title,
                artist,
                is_playing,
                has_media,
                artwork: None,
            })
        })();

        if com_initialized {
            CoUninitialize();
        }

        result.unwrap_or(MediaInfo {
            title: String::new(),
            artist: String::new(),
            is_playing: false,
            has_media: false,
            artwork: None,
        })
    }
}

#[tauri::command]
fn media_play() {
    control_media_session(MediaAction::Play);
}

#[tauri::command]
fn media_pause() {
    control_media_session(MediaAction::Pause);
}

#[tauri::command]
fn media_play_pause() {
    control_media_session(MediaAction::Toggle);
}

#[tauri::command]
fn media_next() {
    control_media_session(MediaAction::Next);
}

#[tauri::command]
fn media_prev() {
    control_media_session(MediaAction::Prev);
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

enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
}

fn control_media_session(action: MediaAction) {
    let vk_code = match action {
        MediaAction::Play => 0xB3,
        MediaAction::Pause => 0xB3,
        MediaAction::Toggle => 0xB3,
        MediaAction::Next => 0xB0,
        MediaAction::Prev => 0xB1,
    };

    send_media_key(vk_code);
}

fn send_media_key(vk_code: u16) {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, KEYBDINPUT, KEYEVENTF_KEYUP,
            INPUT_TYPE, VIRTUAL_KEY, KEYBD_EVENT_FLAGS,
        };

        let inputs = [
            INPUT {
                r#type: INPUT_TYPE(0),
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk_code),
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_TYPE(0),
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk_code),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

// IAudioEndpointVolume COM interface for volume control
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;

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

// Volume monitoring using Windows Core Audio API with raw COM
fn setup_volume_monitor(app_handle: AppHandle) {
    std::thread::spawn(move || {
        use windows::Win32::Media::Audio::{
            IMMDeviceEnumerator, IMMDevice,
            eRender, eConsole,
        };
        use windows::Win32::System::Com::{
            CoInitializeEx, CoUninitialize,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::Foundation::S_OK;

        unsafe {
            let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let com_initialized = hr.is_ok() || hr == S_OK;

            let result: Result<(), String> = (|| {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL)
                        .map_err(|e| format!("CoCreateInstance failed: {:?}", e))?;

                let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                    .map_err(|e| format!("GetDefaultAudioEndpoint failed: {:?}", e))?;

                // Activate the IAudioEndpointVolume interface
                let audio_endpoint_volume: IAudioEndpointVolume = device.Activate(windows::Win32::System::Com::CLSCTX_ALL, None)
                    .map_err(|e| format!("Activate failed: {:?}", e))?;

                let mut last_volume: f32 = 0.0;
                let mut last_muted: bool = false;

                // Get initial volume
                if let Ok(vol) = audio_endpoint_volume.GetMasterVolumeLevelScalar() {
                    last_volume = vol;
                }
                if let Ok(muted) = audio_endpoint_volume.GetMute() {
                    last_muted = muted.into();
                }

                let _ = app_handle.emit("volume-change", VolumeChangeEvent {
                    volume: last_volume,
                    is_muted: last_muted,
                });

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(16));

                    if let (Ok(current_volume), Ok(current_muted)) = (
                        audio_endpoint_volume.GetMasterVolumeLevelScalar(),
                        audio_endpoint_volume.GetMute()
                    ) {
                        let is_muted: bool = current_muted.into();

                        if (current_volume - last_volume).abs() > 0.001 || is_muted != last_muted {
                            last_volume = current_volume;
                            last_muted = is_muted;

                            // Emit event to all windows
                            let _ = app_handle.emit("volume-change", VolumeChangeEvent {
                                volume: current_volume,
                                is_muted,
                            });

                            // Show volume overlay window
                                if let Some(volume_window) = app_handle.get_webview_window("volume-overlay") {
                                    let _ = volume_window.show();
                                }
                        }
                    }
                }
            })();

            if com_initialized {
                CoUninitialize();
            }

            if let Err(e) = result {
                eprintln!("Failed to initialize volume monitor: {}", e);
            }
        }
    });
}

// Handle volume key events and change volume
fn handle_volume_key(vk_code: VIRTUAL_KEY) {
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, IMMDevice,
        eRender, eConsole,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::Foundation::S_OK;

    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let com_initialized = hr.is_ok() || hr == S_OK;

        let result: Result<(), String> = (|| {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&windows::Win32::Media::Audio::MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("CoCreateInstance failed: {:?}", e))?;

            let device: IMMDevice = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|e| format!("GetDefaultAudioEndpoint failed: {:?}", e))?;

            // Activate using raw COM
            let audio_endpoint_volume: IAudioEndpointVolume = device.Activate(windows::Win32::System::Com::CLSCTX_ALL, None)
                .map_err(|e| format!("Activate failed: {:?}", e))?;

            if let Ok(current_volume) = audio_endpoint_volume.GetMasterVolumeLevelScalar() {
                match vk_code {
                    VK_VOLUME_MUTE => {
                        if let Ok(current_muted) = audio_endpoint_volume.GetMute() {
                            let _ = audio_endpoint_volume.SetMute(!current_muted.as_bool(), std::ptr::null());
                        }
                    }
                    VK_VOLUME_UP => {
                        let new_volume = (current_volume + 0.05).min(1.0);
                        let _ = audio_endpoint_volume.SetMasterVolumeLevelScalar(new_volume, std::ptr::null());
                    }
                    VK_VOLUME_DOWN => {
                        let new_volume = (current_volume - 0.05).max(0.0);
                        let _ = audio_endpoint_volume.SetMasterVolumeLevelScalar(new_volume, std::ptr::null());
                    }
                    _ => {}
                }
            }

            Ok(())
        })();

        if com_initialized {
            CoUninitialize();
        }

        if let Err(e) = result {
            eprintln!("Failed to handle volume key: {}", e);
        }
    }
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
                handle_volume_key(vk_code);
            }
            // Always swallow these keys to prevent native OSD
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
        .invoke_handler(tauri::generate_handler![
            get_media_info,
            media_play,
            media_pause,
            media_play_pause,
            media_next,
            media_prev,
            open_wifi_settings,
            open_notification_center
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            let hwnd = window.hwnd().unwrap();
            register_appbar(hwnd);

            let _hook = setup_keyboard_hook();
            setup_volume_monitor(app.handle().clone());
            setup_audio_visualization(app.handle().clone());

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
            bottom: 48,
        };

        SHAppBarMessage(ABM_SETPOS, &mut abd);

        let _ = SetWindowPos(
            hwnd,
            None,
            abd.rc.left,
            abd.rc.top,
            abd.rc.right - abd.rc.left,
            abd.rc.bottom - abd.rc.top,
            SWP_NOZORDER,
        );
    }
}
