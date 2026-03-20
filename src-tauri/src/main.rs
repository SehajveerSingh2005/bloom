// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use windows::Win32::UI::Shell::{
    SHAppBarMessage, APPBARDATA,
    ABM_NEW, ABM_SETPOS,
    ABE_TOP,
};
use windows::Win32::UI::Shell::ShellExecuteA;
use windows::Win32::Foundation::HWND;

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

            // Get playback info
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
        
        // Open Windows WiFi/Network settings using the ms-availablenetworks: protocol
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
        
        // Open Windows Action Center / Notification Center using shell execute
        // This directly opens the notification panel without keyboard shortcuts
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
    // Fall back to keyboard input for media control
    // This is system-wide and works with all media players
    let vk_code = match action {
        MediaAction::Play => 0xB3,      // VK_MEDIA_PLAY_PAUSE
        MediaAction::Pause => 0xB3,     // VK_MEDIA_PLAY_PAUSE
        MediaAction::Toggle => 0xB3,    // VK_MEDIA_PLAY_PAUSE
        MediaAction::Next => 0xB0,      // VK_MEDIA_NEXT_TRACK
        MediaAction::Prev => 0xB1,      // VK_MEDIA_PREV_TRACK
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
