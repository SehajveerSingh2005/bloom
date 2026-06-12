use windows::Win32::Foundation::{HWND, HGLOBAL};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::Interface;
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::Win32::Graphics::Imaging::{IWICImagingFactory, CLSID_WICImagingFactory, GUID_ContainerFormatPng, WICBitmapEncoderNoCache, GUID_WICPixelFormat32bppPBGRA};
use base64::{Engine as _, engine::general_purpose};

pub fn resolve_shortcut(path: &str) -> Option<(String, String)> {
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
        let persist_file: IPersistFile = shell_link.cast().ok()?;
        
        let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        persist_file.Load(windows::core::PCWSTR(wide_path.as_ptr()), windows::Win32::System::Com::STGM(0)).ok()?;
        
        let _ = shell_link.Resolve(HWND(std::ptr::null_mut()), 1 | 16 | 32); 
        
        let mut buffer = [0u16; 260];
        let mut data = windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW::default();
        shell_link.GetPath(&mut buffer, &mut data, 0).ok()?;
        
        let mut arg_buffer = [0u16; 1024];
        let _ = shell_link.GetArguments(&mut arg_buffer);

        let target = String::from_utf16_lossy(&buffer).trim_matches(char::from(0)).to_string();
        let args = String::from_utf16_lossy(&arg_buffer).trim_matches(char::from(0)).to_string();
        
        if target.trim().is_empty() { None } else { Some((target, args)) }
    }
}

static mut ORIGINAL_TRAY_RECT: Option<windows::Win32::Foundation::RECT> = None;
static mut ORIGINAL_SEC_TRAY_RECT: Option<windows::Win32::Foundation::RECT> = None;

pub fn set_taskbar_visibility(visible: bool, always_on_top: bool) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE, SW_SHOW, GetWindowRect};
        use windows::Win32::UI::Shell::{SHAppBarMessage, APPBARDATA, ABM_SETSTATE};

        let tray_class = windows::core::PCSTR(b"Shell_TrayWnd\0".as_ptr());
        let secondary_tray_class = windows::core::PCSTR(b"Shell_SecondaryTrayWnd\0".as_ptr());

        // 1. Set the taskbar state (Auto-hide or Always-on-top)
        // ABS_AUTOHIDE = 0x1, ABS_ALWAYSONTOP = 0x2 
        let mut abd = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, lParam: windows::Win32::Foundation::LPARAM(if always_on_top { 2 } else { 1 }), ..Default::default() };
        SHAppBarMessage(ABM_SETSTATE, &mut abd);

        // 2. Control visibility of the primary taskbar
        if let Ok(tray_hwnd) = FindWindowA(tray_class, windows::core::PCSTR::null()) {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE};
            if visible {
                if let Some(rect) = ORIGINAL_TRAY_RECT {
                    let _ = SetWindowPos(tray_hwnd, None, rect.left, rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
                let _ = ShowWindow(tray_hwnd, SW_SHOW);
            } else {
                let has_rect = { let rect_ptr: *const Option<windows::Win32::Foundation::RECT> = std::ptr::addr_of!(ORIGINAL_TRAY_RECT); (*rect_ptr).is_some() };
                if !has_rect {
                    let mut rect = windows::Win32::Foundation::RECT::default();
                    let _ = GetWindowRect(tray_hwnd, &mut rect);
                    ORIGINAL_TRAY_RECT = Some(rect);
                }
                let _ = ShowWindow(tray_hwnd, SW_HIDE);
                // Move it far off-screen to prevent any "thin line" artifacts or flashes
                let _ = SetWindowPos(tray_hwnd, None, -10000, -10000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }

        // 3. Control visibility of secondary taskbars (multi-monitor)
        if let Ok(secondary_tray_hwnd) = FindWindowA(secondary_tray_class, windows::core::PCSTR::null()) {
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOSIZE, SWP_NOZORDER, SWP_NOACTIVATE};
            if visible {
                if let Some(rect) = ORIGINAL_SEC_TRAY_RECT {
                    let _ = SetWindowPos(secondary_tray_hwnd, None, rect.left, rect.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
                let _ = ShowWindow(secondary_tray_hwnd, SW_SHOW);
            } else {
                let has_sec_rect = { let rect_ptr: *const Option<windows::Win32::Foundation::RECT> = std::ptr::addr_of!(ORIGINAL_SEC_TRAY_RECT); (*rect_ptr).is_some() };
                if !has_sec_rect {
                    let mut rect = windows::Win32::Foundation::RECT::default();
                    let _ = GetWindowRect(secondary_tray_hwnd, &mut rect);
                    ORIGINAL_SEC_TRAY_RECT = Some(rect);
                }
                let _ = ShowWindow(secondary_tray_hwnd, SW_HIDE);
                let _ = SetWindowPos(secondary_tray_hwnd, None, -10000, -10000, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }
}


pub unsafe fn icon_to_base64(hicon: HICON) -> Option<String> {
    let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let bitmap = factory.CreateBitmapFromHICON(hicon).ok()?;
    
    let stream = CreateStreamOnHGlobal(HGLOBAL(std::ptr::null_mut()), true).ok()?;
    let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
    encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
    
    let mut frame = None;
    encoder.CreateNewFrame(&mut frame, std::ptr::null_mut()).ok()?;
    let frame = frame?;
    frame.Initialize(None).ok()?;
    
    let (mut width, mut height) = (0u32, 0u32);
    bitmap.GetSize(&mut width, &mut height).ok()?;
    frame.SetSize(width, height).ok()?;
    
    let mut format = GUID_WICPixelFormat32bppPBGRA;
    frame.SetPixelFormat(&mut format).ok()?;
    
    frame.WriteSource(&bitmap, std::ptr::null()).ok()?;
    frame.Commit().ok()?;
    encoder.Commit().ok()?;
    
    let hglobal = GetHGlobalFromStream(&stream).ok()?;
    let ptr = windows::Win32::System::Memory::GlobalLock(hglobal);
    let size = windows::Win32::System::Memory::GlobalSize(hglobal);
    
    let data = std::slice::from_raw_parts(ptr as *const u8, size);
    let base64_str = general_purpose::STANDARD.encode(data);
    
    let _ = windows::Win32::System::Memory::GlobalUnlock(hglobal);
    
    Some(format!("data:image/png;base64,{}", base64_str))
}

pub fn get_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}
