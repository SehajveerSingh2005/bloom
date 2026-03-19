// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use window_vibrancy::{apply_acrylic, apply_mica};
use tauri::Manager;


use windows::Win32::UI::Shell::{
    SHAppBarMessage, APPBARDATA,
    ABM_NEW, ABM_SETPOS,
    ABE_TOP,
};

use windows::Win32::Foundation::{HWND, RECT};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
        
            let hwnd = window.hwnd().unwrap();
        
            // Apply Acrylic (glass blur)
            apply_acrylic(&window, Some((18, 18, 18, 125)))?;
        
            // Your AppBar
            register_appbar(HWND(hwnd.0 as isize));
        
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn register_appbar(hwnd: HWND) {
    unsafe {
        let mut abd = APPBARDATA::default();
        abd.cbSize = std::mem::size_of::<APPBARDATA>() as u32;
        abd.hWnd = hwnd;

        // Register
        SHAppBarMessage(ABM_NEW, &mut abd);

        abd.uEdge = ABE_TOP;

        abd.rc = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 48,
        };

        // Windows adjusts rect internally
        SHAppBarMessage(ABM_SETPOS, &mut abd);

        // 👇 THIS IS THE IMPORTANT PART
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOZORDER,
        };

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