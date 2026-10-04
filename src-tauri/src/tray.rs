//! Enumerates the legacy notification-area (tray) icons by reading the shell's
//! toolbar controls, and forwards clicks to the owning application.

use crate::types::TrayApp;
use std::collections::HashSet;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetWindowThreadProcessId, SendMessageTimeoutW, HICON,
    SMTO_ABORTIFHUNG,
};

const WM_USER: u32 = 0x0400;
const TB_GETBUTTON: u32 = WM_USER + 23;
const TB_BUTTONCOUNT: u32 = WM_USER + 24;
const TB_GETBUTTONTEXTW: u32 = WM_USER + 75;
const TBSTATE_HIDDEN: u8 = 0x08;

/// Mirrors the 64-bit `TBBUTTON` layout.
#[allow(dead_code)]
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct TbButton {
    i_bitmap: i32,
    id_command: i32,
    fs_state: u8,
    fs_style: u8,
    reserved: [u8; 6],
    dw_data: usize,
    i_string: isize,
}

/// The shell's private per-icon record stored in `TBBUTTON::dwData` (64-bit layout).
#[allow(dead_code)]
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct TrayData {
    hwnd: usize,
    uid: u32,
    callback_message: u32,
    reserved: [u32; 2],
    hicon: usize,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn find_child(parent: HWND, class: &str) -> Option<HWND> {
    let class = wide(class);
    FindWindowExW(Some(parent), None, PCWSTR(class.as_ptr()), PCWSTR::null())
        .ok()
        .filter(|h| !h.0.is_null())
}

unsafe fn find_top(class: &str) -> Option<HWND> {
    let class = wide(class);
    FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null())
        .ok()
        .filter(|h| !h.0.is_null())
}

/// The toolbar holding the visible tray icons (`Shell_TrayWnd` > `TrayNotifyWnd` >
/// `SysPager` > `ToolbarWindow32`) and the one holding the overflow icons.
unsafe fn tray_toolbars() -> Vec<(HWND, bool)> {
    let mut out = Vec::new();
    if let Some(tray) = find_top("Shell_TrayWnd") {
        if let Some(notify) = find_child(tray, "TrayNotifyWnd") {
            let pager = find_child(notify, "SysPager").unwrap_or(notify);
            if let Some(tb) = find_child(pager, "ToolbarWindow32") {
                out.push((tb, false));
            }
        }
    }
    if let Some(overflow) = find_top("NotifyIconOverflowWindow") {
        if let Some(tb) = find_child(overflow, "ToolbarWindow32") {
            out.push((tb, true));
        }
    }
    out
}

unsafe fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> Option<isize> {
    let mut result = 0usize;
    let ok = SendMessageTimeoutW(
        hwnd,
        msg,
        WPARAM(wparam),
        LPARAM(lparam),
        SMTO_ABORTIFHUNG,
        500,
        Some(&mut result),
    );
    (ok.0 != 0).then_some(result as isize)
}

unsafe fn read_remote<T: Default + Copy>(process: HANDLE, addr: usize) -> Option<T> {
    let mut value = T::default();
    ReadProcessMemory(
        process,
        addr as *const _,
        &mut value as *mut T as *mut _,
        std::mem::size_of::<T>(),
        None,
    )
    .ok()?;
    Some(value)
}

unsafe fn read_toolbar(
    toolbar: HWND,
    overflow: bool,
    out: &mut Vec<TrayApp>,
    seen: &mut HashSet<String>,
) {
    let mut pid = 0u32;
    GetWindowThreadProcessId(toolbar, Some(&mut pid));
    let Ok(process) = OpenProcess(
        PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION,
        false,
        pid,
    ) else {
        return;
    };

    // The toolbar writes into memory of *its* process, so the button/text buffers
    // have to be allocated inside explorer.exe.
    const TEXT_CHARS: usize = 256;
    let button_size = std::mem::size_of::<TbButton>();
    let remote = VirtualAllocEx(
        process,
        None,
        button_size + TEXT_CHARS * 2,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_READWRITE,
    );
    if !remote.is_null() {
        let remote_button = remote as usize;
        let remote_text = remote_button + button_size;
        let count = send(toolbar, TB_BUTTONCOUNT, 0, 0).unwrap_or(0).max(0) as usize;

        for index in 0..count {
            if send(toolbar, TB_GETBUTTON, index, remote_button as isize) != Some(1) {
                continue;
            }
            let Some(button) = read_remote::<TbButton>(process, remote_button) else {
                continue;
            };
            if button.fs_state & TBSTATE_HIDDEN != 0 || button.dw_data == 0 {
                continue;
            }
            let Some(data) = read_remote::<TrayData>(process, button.dw_data) else {
                continue;
            };
            if data.hwnd == 0 {
                continue;
            }

            let id = format!("{}:{}", data.hwnd, data.uid);
            if !seen.insert(id.clone()) {
                continue;
            }

            let owner = HWND(data.hwnd as *mut _);
            let mut owner_pid = 0u32;
            GetWindowThreadProcessId(owner, Some(&mut owner_pid));
            if owner_pid == 0 {
                continue;
            }
            let path = crate::commands::process_image_path(owner_pid).unwrap_or_default();

            let mut tooltip = String::new();
            let len = send(
                toolbar,
                TB_GETBUTTONTEXTW,
                button.id_command as usize,
                remote_text as isize,
            )
            .unwrap_or(0);
            if len > 0 {
                let mut text = vec![0u16; TEXT_CHARS];
                if ReadProcessMemory(
                    process,
                    remote_text as *const _,
                    text.as_mut_ptr() as *mut _,
                    TEXT_CHARS * 2,
                    None,
                )
                .is_ok()
                {
                    let end = text.iter().position(|&c| c == 0).unwrap_or(TEXT_CHARS);
                    tooltip = String::from_utf16_lossy(&text[..end]);
                }
            }

            let name = if path.is_empty() {
                tooltip.clone()
            } else {
                crate::commands::friendly_process_name(&path)
            };
            let icon = if data.hicon != 0 {
                crate::utils::icon_to_base64(HICON(data.hicon as *mut _))
            } else {
                None
            };

            out.push(TrayApp {
                id,
                name,
                tooltip,
                path,
                icon,
                hwnd: data.hwnd as isize,
                uid: data.uid,
                callback_message: data.callback_message,
                overflow,
            });
        }
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
    }
    let _ = CloseHandle(process);
}

pub unsafe fn list_tray_apps() -> Vec<TrayApp> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for (toolbar, overflow) in tray_toolbars() {
        read_toolbar(toolbar, overflow, &mut apps, &mut seen);
    }
    apps
}

/// Replays a mouse click on the icon's callback window, the same message pair the
/// shell sends when the icon is clicked in the real tray.
pub unsafe fn click_tray_app(hwnd: isize, uid: u32, callback_message: u32, right: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, PostMessageW, SetForegroundWindow, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP,
    };

    let owner = HWND(hwnd as *mut _);
    if callback_message == 0 {
        return;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(owner, Some(&mut pid));
    if pid != 0 {
        let _ = AllowSetForegroundWindow(pid);
    }
    let _ = SetForegroundWindow(owner);

    let (down, up) = if right {
        (WM_RBUTTONDOWN, WM_RBUTTONUP)
    } else {
        (WM_LBUTTONDOWN, WM_LBUTTONUP)
    };
    for msg in [down, up] {
        let _ = PostMessageW(
            Some(owner),
            callback_message,
            WPARAM(uid as usize),
            LPARAM(msg as isize),
        );
    }
}
