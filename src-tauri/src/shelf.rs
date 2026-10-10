//! OS drop target for the notch (files and selected text).
//!
//! Tauri/wry registers its own drop targets on the WebView2 child windows that
//! exist when the webview is built. The notch window is created hidden and
//! WebView2 creates/recreates its render child later, so those targets can be
//! stale — and OLE does not fall back to ancestors, so a drag over the notch
//! finds nothing. This module revokes any existing target and registers ours on
//! the toplevel and every current descendant, after the webview has painted.

use std::cell::UnsafeCell;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use windows::core::{implement, Ref, BOOL};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{
    CoInitializeEx, IDataObject, COINIT_APARTMENTTHREADED, DVASPECT_CONTENT, FORMATETC,
    TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP,
    CF_UNICODETEXT, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;

/// Upper bound on accepted text drops (~128k UTF-16 units). Anything larger is
/// treated as unsupported rather than truncated.
const MAX_TEXT_UNITS: usize = 1 << 17;

/// Keeps every registered target alive for as long as OLE holds it. The COM
/// pointers are only ever used by OLE, which marshals calls itself.
struct TargetStore(#[allow(dead_code)] Vec<IDropTarget>);
unsafe impl Send for TargetStore {}

static SHELF_DROP_TARGETS: Mutex<Option<TargetStore>> = Mutex::new(None);

/// What a drag carries that we accept.
enum DragPayload {
    Files(Vec<String>, HDROP),
    Text(String),
}

#[implement(IDropTarget)]
struct ShelfDropTarget {
    app: AppHandle,
    hwnd: HWND,
    enter_valid: UnsafeCell<bool>,
    cursor_effect: UnsafeCell<DROPEFFECT>,
}

impl ShelfDropTarget {
    /// Files first (CF_HDROP), then selected text (CF_UNICODETEXT). `None` when
    /// the drag carries neither.
    unsafe fn extract(data: &IDataObject) -> Option<DragPayload> {
        if let Some((paths, hdrop)) = unsafe { Self::extract_files(data) } {
            return Some(DragPayload::Files(paths, hdrop));
        }
        unsafe { Self::extract_text(data) }.map(DragPayload::Text)
    }

    /// Reads CF_HDROP from the drag data. `None` when the drag carries no files.
    unsafe fn extract_files(data: &IDataObject) -> Option<(Vec<String>, HDROP)> {
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let medium = data.GetData(&format).ok()?;
        let hdrop = HDROP(medium.u.hGlobal.0 as _);
        let count = DragQueryFileW(hdrop, 0xFFFFFFFF, None);
        let mut paths = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            paths.push(String::from_utf16_lossy(&buf[..len]));
        }
        Some((paths, hdrop))
    }

    /// Reads CF_UNICODETEXT (selected text dragged from another app). `None`
    /// when absent, empty, or unreasonably large.
    unsafe fn extract_text(data: &IDataObject) -> Option<String> {
        let format = FORMATETC {
            cfFormat: CF_UNICODETEXT.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let mut medium = data.GetData(&format).ok()?;
        let ptr = GlobalLock(medium.u.hGlobal) as *const u16;
        if ptr.is_null() {
            ReleaseStgMedium(&mut medium);
            return None;
        }

        let mut len = 0usize;
        while len < MAX_TEXT_UNITS && *ptr.add(len) != 0 {
            len += 1;
        }
        let text = if len >= MAX_TEXT_UNITS {
            None
        } else {
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(
                ptr, len,
            )))
        };
        let _ = GlobalUnlock(medium.u.hGlobal);
        ReleaseStgMedium(&mut medium);

        match text {
            Some(t) if !t.trim().is_empty() => Some(t),
            _ => None,
        }
    }

    unsafe fn client_point(&self, pt: &POINTL) -> POINT {
        let mut point = POINT { x: pt.x, y: pt.y };
        let _ = ScreenToClient(self.hwnd, &mut point);
        point
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for ShelfDropTarget_Impl {
    fn DragEnter(
        &self,
        pdata: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let payload = pdata
            .as_ref()
            .and_then(|data| unsafe { ShelfDropTarget::extract(data) });
        let Some(payload) = payload else {
            unsafe {
                *self.enter_valid.get() = false;
                *self.cursor_effect.get() = DROPEFFECT_NONE;
                *effect = DROPEFFECT_NONE;
            }
            return Ok(());
        };

        unsafe {
            *self.enter_valid.get() = true;
            *self.cursor_effect.get() = DROPEFFECT_COPY;
            *effect = DROPEFFECT_COPY;
        }

        let point = unsafe { self.client_point(pt) };
        match payload {
            DragPayload::Files(paths, hdrop) => {
                let _ = self.app.emit(
                    "shelf-drag-enter",
                    serde_json::json!({ "paths": paths, "position": { "x": point.x, "y": point.y } }),
                );
                unsafe { DragFinish(hdrop) };
            }
            DragPayload::Text(text) => {
                let _ = self.app.emit(
                    "shelf-drag-enter",
                    serde_json::json!({ "text": text, "position": { "x": point.x, "y": point.y } }),
                );
            }
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe { *effect = *self.cursor_effect.get() };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if unsafe { *self.enter_valid.get() } {
            unsafe { *self.enter_valid.get() = false };
            let _ = self.app.emit("shelf-drag-leave", ());
        }
        Ok(())
    }

    fn Drop(
        &self,
        pdata: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        _effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        if unsafe { *self.enter_valid.get() } {
            unsafe { *self.enter_valid.get() = false };
            let payload = pdata
                .as_ref()
                .and_then(|data| unsafe { ShelfDropTarget::extract(data) });
            match payload {
                Some(DragPayload::Files(paths, hdrop)) => {
                    let _ = self
                        .app
                        .emit("shelf-drag-drop", serde_json::json!({ "paths": paths }));
                    unsafe { DragFinish(hdrop) };
                }
                Some(DragPayload::Text(text)) => {
                    let _ = self
                        .app
                        .emit("shelf-drag-drop", serde_json::json!({ "text": text }));
                }
                None => {}
            }
        }
        Ok(())
    }
}

/// (Re)registers the shelf drop target on the main window and all its current
/// descendants. Safe to call repeatedly; existing registrations are replaced.
pub fn setup(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = win.hwnd() else {
        return;
    };

    struct Ctx {
        app: AppHandle,
        targets: Vec<IDropTarget>,
    }

    unsafe extern "system" fn child_proc(child: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut Ctx);
        unsafe { inject(child, &ctx.app, &mut ctx.targets) };
        BOOL(1)
    }

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut ctx = Ctx {
            app: app.clone(),
            targets: Vec::new(),
        };
        inject(hwnd, app, &mut ctx.targets);
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(child_proc),
            LPARAM(&mut ctx as *mut Ctx as isize),
        );
        if let Ok(mut guard) = SHELF_DROP_TARGETS.lock() {
            *guard = Some(TargetStore(ctx.targets));
        }
    }
}

unsafe fn inject(hwnd: HWND, app: &AppHandle, out: &mut Vec<IDropTarget>) {
    let target: IDropTarget = ShelfDropTarget {
        app: app.clone(),
        hwnd,
        enter_valid: UnsafeCell::new(false),
        cursor_effect: UnsafeCell::new(DROPEFFECT_NONE),
    }
    .into();
    let _ = unsafe { RevokeDragDrop(hwnd) };
    if unsafe { RegisterDragDrop(hwnd, &target) }.is_ok() {
        out.push(target);
    }
}
