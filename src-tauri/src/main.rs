#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod types;
mod state;
mod utils;
mod services;
mod commands;

use tauri::Manager;
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT};
use windows::core::BOOL;
use std::sync::atomic::Ordering;


use crate::state::*;
use crate::utils::*;
use crate::services::*;
use crate::commands::*;

static mut SINGLE_INSTANCE_EVENT_HANDLE: isize = 0;
static mut SINGLE_INSTANCE_MUTEX_HANDLE: isize = 0;

unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> BOOL {
    if ctrl_type == CTRL_C_EVENT || ctrl_type == CTRL_BREAK_EVENT || ctrl_type == CTRL_CLOSE_EVENT {
        set_taskbar_visibility(true, true);
        NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

    }
    BOOL(0)
}

fn main() {
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true);
    }

    // Single-instance enforcement
    unsafe {
        use windows::Win32::System::Threading::{CreateMutexW, CreateEventW, OpenEventW, SetEvent, SYNCHRONIZATION_ACCESS_RIGHTS};
        use windows::Win32::Foundation::{GetLastError, CloseHandle};

        let mutex_name: Vec<u16> = "BloomSingleInstance".encode_utf16().chain(std::iter::once(0)).collect();
        let event_name: Vec<u16> = "BloomOpenSettings".encode_utf16().chain(std::iter::once(0)).collect();

        let h_mutex = CreateMutexW(None, true, windows::core::PCWSTR(mutex_name.as_ptr())).ok();
        let err = GetLastError();

        if err.0 == 183 {
            // Another instance is already running — signal it to open settings
            if let Ok(h_event) = OpenEventW(SYNCHRONIZATION_ACCESS_RIGHTS(0x00100002), false, windows::core::PCWSTR(event_name.as_ptr())) {
                let _ = SetEvent(h_event);
                let _ = CloseHandle(h_event);
            }
            if let Some(h) = h_mutex { let _ = CloseHandle(h); }
            return;
        }

        if let Ok(h_event) = CreateEventW(None, false, false, windows::core::PCWSTR(event_name.as_ptr())) {
            SINGLE_INSTANCE_EVENT_HANDLE = h_event.0 as isize;
        }
        if let Some(h) = h_mutex { SINGLE_INSTANCE_MUTEX_HANDLE = h.0 as isize; }
    }

    setup_brightness_worker();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .invoke_handler(tauri::generate_handler![
            broadcast_setting,
            hide_native_osd,
            open_settings_window,
            open_wifi_settings,
            open_notification_center,
            open_system_tray,
            set_ignore_cursor_events,
            set_window_height,
            resize_settings_window,
            hide_volume_overlay,
            hide_brightness_overlay,
            media_play_pause,
            media_next,
            media_previous,
            init_dock,
            toggle_dock,
            change_dock_mode,
            change_notch_mode,
            sync_appbar,
            open_app,
            update_dock_rect,
            update_notch_rect,
            set_dock_hovered,
            set_notch_hovered,
            get_active_windows,
            get_app_icon,
            get_installed_apps,
            save_pinned_apps,
            load_pinned_apps,
            set_menu_open,
            focus_window,
            close_window,
            quit_bloom,
            restart_bloom,
            get_volume,
            get_brightness,
            set_volume,
            save_setting,
            load_settings,
            capture_window_thumbnail,
            get_wifi_state,
            set_wifi_state,
            get_bluetooth_state,
            set_bluetooth_state,
            open_bluetooth_settings,
            open_airplane_mode_settings,
            set_brightness,
            get_battery_saver_state,
            open_battery_saver_settings,
            get_system_accent_color
        ])
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            let dock_win = app.get_webview_window("dock").unwrap();

            // Sync window rects initially and on event
            let win_clone = window.clone();
            let update_main_rect = move || {
                if let (Ok(p), Ok(s)) = (win_clone.outer_position(), win_clone.outer_size()) {
                    if let Ok(mut lock) = MAIN_WINDOW_RECT.lock() {
                        *lock = Some((p, s));
                    }
                }
            };

            let dock_clone = dock_win.clone();
            let update_dock_window_rect = move || {
                if let (Ok(p), Ok(s)) = (dock_clone.outer_position(), dock_clone.outer_size()) {
                    if let Ok(mut lock) = DOCK_WINDOW_RECT.lock() {
                        *lock = Some((p, s));
                    }
                }
            };

            update_main_rect();
            update_dock_window_rect();

            let u_main = update_main_rect.clone();
            let win_for_events = window.clone();
            let handle_for_events = app.handle().clone();
            window.on_window_event(move |e| {
                match e {
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        u_main();
                        sync_overlays(&handle_for_events);
                    }
                    tauri::WindowEvent::ScaleFactorChanged { .. } => {
                        let w = win_for_events.clone();
                        let h = handle_for_events.clone();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            register_appbar(w);
                            sync_overlays(&h);
                        });
                    }
                    _ => {}
                }
            });

            let u_dock = update_dock_window_rect.clone();
            let dock_for_events = dock_win.clone();
            let handle_for_dock_events = app.handle().clone();
            dock_win.on_window_event(move |e| {
                match e {
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        u_dock();
                        sync_overlays(&handle_for_dock_events);
                    }
                    tauri::WindowEvent::ScaleFactorChanged { .. } => {
                        let h = handle_for_dock_events.clone();
                        if DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) {
                            let w = dock_for_events.clone();
                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                register_dock_appbar(w);
                                sync_overlays(&h);
                            });
                        } else {
                             sync_overlays(&h);
                        }
                    }
                    _ => {}
                }
            });

            sync_overlays(app.handle());
            setup_cursor_monitor(app.handle().clone());
            setup_display_change_monitor(app.handle().clone());
            {
                let _ = crate::state::THUMBNAIL_CACHE.set(std::sync::Mutex::new(std::collections::HashMap::new()));
                let _ = crate::state::FOCUS_TIMESTAMPS.set(std::sync::Mutex::new(std::collections::HashMap::new()));
            }
            setup_thumbnail_capture(app.handle().clone());
            trigger_app_scan();
            let tx = setup_system_worker(app.handle().clone());
            unsafe {
                COMMAND_SENDER = Some(tx.clone());
            }
            let _hook = services::setup_keyboard_hook();
            setup_taskbar_hook();
            setup_audio_visualization(app.handle().clone());

            // Listen for second-instance signal to open settings
            unsafe {
                if SINGLE_INSTANCE_EVENT_HANDLE != 0 {
                    let app_handle = app.handle().clone();
                    std::thread::spawn(move || {
                        use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
                        use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
                        let h_event = HANDLE(SINGLE_INSTANCE_EVENT_HANDLE as *mut _);
                        loop {
                            let result = WaitForSingleObject(h_event, INFINITE);
                            if result == WAIT_OBJECT_0 {
                                crate::commands::open_settings_window(app_handle.clone());
                            }
                        }
                    });
                }
            }

            if let Some(settings_win) = app.get_webview_window("settings") {
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
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
                let quit_item = MenuItem::with_id(app, "quit", "Quit Bloom", true, None::<&str>)?;
                let restart_item =
                    MenuItem::with_id(app, "restart", "Restart Bloom", true, None::<&str>)?;
                let settings_item =
                    MenuItem::with_id(app, "settings", "Open Settings", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&settings_item, &restart_item, &quit_item])?;
                let ah = app.handle().clone();
                TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("Bloom")
                    .menu(&menu)
                    .on_menu_event(move |_, event| match event.id().as_ref() {
                        "quit" => {
                            if let Some(w) = ah.get_webview_window("main") {
                                unregister_appbar_native(w.hwnd().unwrap());
                            }
                            if let Some(w) = ah.get_webview_window("dock") {
                                unregister_appbar_native(w.hwnd().unwrap());
                            }
                            set_taskbar_visibility(true, true);
                            NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

                            ah.exit(0);
                        }
                        "restart" => {
                            if let Some(w) = ah.get_webview_window("main") {
                                unregister_appbar_native(w.hwnd().unwrap());
                            }
                            if let Some(w) = ah.get_webview_window("dock") {
                                unregister_appbar_native(w.hwnd().unwrap());
                            }
                            set_taskbar_visibility(true, true);
                            NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

                            ah.restart();
                        }
                        "settings" => {
                            crate::commands::open_settings_window(ah.clone());
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            crate::commands::open_settings_window(tray.app_handle().clone());
                        }
                    })
                    .build(app)?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|_, event| {
        if let tauri::RunEvent::Exit = event {
            set_taskbar_visibility(true, true);
            NATIVE_TASKBAR_HIDDEN.store(false, Ordering::Relaxed);

        }
    });
}
