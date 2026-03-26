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
