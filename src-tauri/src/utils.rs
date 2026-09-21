use base64::{engine::general_purpose, Engine as _};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::Manager;
use windows::core::Interface;
use windows::Win32::Foundation::{HGLOBAL, HWND};
use windows::Win32::Graphics::Imaging::{
	CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppPBGRA,
	IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{
	CoCreateInstance, IPersistFile, CLSCTX_ALL, CLSCTX_INPROC_SERVER,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::HICON;

pub fn resolve_shortcut(path: &str) -> Option<(String, String)> {
	unsafe {
		let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
		let persist_file: IPersistFile = shell_link.cast().ok()?;

		let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
		persist_file
			.Load(
				windows::core::PCWSTR(wide_path.as_ptr()),
				windows::Win32::System::Com::STGM(0),
			)
			.ok()?;

		let _ = shell_link.Resolve(HWND(std::ptr::null_mut()), 1 | 16 | 32);

		let mut buffer = [0u16; 260];
		let mut data = windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW::default();
		shell_link.GetPath(&mut buffer, &mut data, 0).ok()?;

		let mut arg_buffer = [0u16; 1024];
		let _ = shell_link.GetArguments(&mut arg_buffer);

		let target = String::from_utf16_lossy(&buffer)
			.trim_matches(char::from(0))
			.to_string();
		let args = String::from_utf16_lossy(&arg_buffer)
			.trim_matches(char::from(0))
			.to_string();

		if target.trim().is_empty() {
			None
		} else {
			Some((target, args))
		}
	}
}

pub static ORIGINAL_TRAY_RECT: std::sync::Mutex<Option<windows::Win32::Foundation::RECT>> =
	std::sync::Mutex::new(None);
static ORIGINAL_SEC_TRAY_RECT: std::sync::Mutex<Option<windows::Win32::Foundation::RECT>> =
	std::sync::Mutex::new(None);
static ORIGINAL_TASKBAR_STATE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

static TASKBAR_MARKER: OnceLock<PathBuf> = OnceLock::new();

/// Point the crash-recovery marker at this user's app config dir (called from setup).
pub fn init_taskbar_marker(app: &tauri::AppHandle) {
	if let Ok(dir) = app.path().app_config_dir() {
		let _ = TASKBAR_MARKER.set(dir.join("taskbar_hidden.flag"));
	}
}

/// True if a previous Bloom session died without restoring the native taskbar.
pub fn taskbar_marker_exists() -> bool {
	TASKBAR_MARKER.get().is_some_and(|p| p.exists())
}

pub fn set_taskbar_visibility(visible: bool, always_on_top: bool) {
	// Crash-recovery marker: a hidden taskbar is persisted so the next launch can
	// undo it if we're ever force-killed (Task Manager / TerminateProcess skips cleanup).
	if visible {
		if let Some(p) = TASKBAR_MARKER.get() {
			let _ = std::fs::remove_file(p);
		}
	} else {
		if let Some(p) = TASKBAR_MARKER.get() {
			if !p.exists() {
				let _ = std::fs::write(p, b"1");
			}
		}
	}

	unsafe {
		use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, APPBARDATA};
		use windows::Win32::UI::WindowsAndMessaging::{
			FindWindowA, GetWindowRect, ShowWindow, SW_HIDE, SW_SHOW,
		};

		let tray_class = windows::core::PCSTR(c"Shell_TrayWnd".as_ptr() as *const u8);
		let secondary_tray_class =
			windows::core::PCSTR(c"Shell_SecondaryTrayWnd".as_ptr() as *const u8);

		// Save original taskbar state before modifying
		if ORIGINAL_TASKBAR_STATE.load(std::sync::atomic::Ordering::Relaxed) == -1 {
			let mut get_abd = APPBARDATA {
				cbSize: std::mem::size_of::<APPBARDATA>() as u32,
				..Default::default()
			};
			let original_state = SHAppBarMessage(ABM_GETSTATE, &mut get_abd);
			ORIGINAL_TASKBAR_STATE.store(original_state as i32, std::sync::atomic::Ordering::Relaxed);
		}

		let state_val = if visible {
			let orig = ORIGINAL_TASKBAR_STATE.load(std::sync::atomic::Ordering::Relaxed);
			if orig != -1 {
				orig as isize
			} else {
				if always_on_top {
					2
				} else {
					1
				}
			}
		} else {
			1 // Force Auto-hide when hiding
		};

		// 1. Set the taskbar state (Auto-hide or Always-on-top)
		let mut abd = APPBARDATA {
			cbSize: std::mem::size_of::<APPBARDATA>() as u32,
			lParam: windows::Win32::Foundation::LPARAM(state_val),
			..Default::default()
		};
		SHAppBarMessage(ABM_SETSTATE, &mut abd);

		// 2. Control visibility of the primary taskbar
		if let Ok(tray_hwnd) = FindWindowA(tray_class, windows::core::PCSTR::null()) {
			use windows::Win32::UI::WindowsAndMessaging::{
				GetWindowLongA, SetLayeredWindowAttributes, SetWindowLongA, SetWindowPos, GWL_EXSTYLE,
				LWA_ALPHA, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_LAYERED, WS_EX_TRANSPARENT,
			};
			if visible {
				// Revert any lingering WS_EX_LAYERED / WS_EX_TRANSPARENT left by
				// open_system_tray if the user quit before the tray thread cleaned up.
				let ex = GetWindowLongA(tray_hwnd, GWL_EXSTYLE);
				let cleaned = ex & !(WS_EX_LAYERED.0 as i32) & !(WS_EX_TRANSPARENT.0 as i32);
				if cleaned != ex {
					let _ = SetWindowLongA(tray_hwnd, GWL_EXSTYLE, cleaned);
					let _ = SetLayeredWindowAttributes(
						tray_hwnd,
						windows::Win32::Foundation::COLORREF(0),
						255,
						LWA_ALPHA,
					);
				}
				if let Ok(guard) = ORIGINAL_TRAY_RECT.lock() {
					if let Some(rect) = *guard {
						let _ = SetWindowPos(
							tray_hwnd,
							None,
							rect.left,
							rect.top,
							0,
							0,
							SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
						);
					}
				}
				let _ = ShowWindow(tray_hwnd, SW_SHOW);
			} else {
				let has_rect = ORIGINAL_TRAY_RECT
					.lock()
					.map(|g| g.is_some())
					.unwrap_or(false);
				if !has_rect {
					let mut rect = windows::Win32::Foundation::RECT::default();
					let _ = GetWindowRect(tray_hwnd, &mut rect);
					if let Ok(mut guard) = ORIGINAL_TRAY_RECT.lock() {
						*guard = Some(rect);
					}
				}
				let _ = ShowWindow(tray_hwnd, SW_HIDE);
				// Move it far off-screen to prevent any "thin line" artifacts or flashes
				let _ = SetWindowPos(
					tray_hwnd,
					None,
					-10000,
					-10000,
					0,
					0,
					SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
				);
			}
		}

		// 3. Control visibility of secondary taskbars (multi-monitor)
		if let Ok(secondary_tray_hwnd) = FindWindowA(secondary_tray_class, windows::core::PCSTR::null())
		{
			use windows::Win32::UI::WindowsAndMessaging::{
				SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
			};
			if visible {
				if let Ok(guard) = ORIGINAL_SEC_TRAY_RECT.lock() {
					if let Some(rect) = *guard {
						let _ = SetWindowPos(
							secondary_tray_hwnd,
							None,
							rect.left,
							rect.top,
							0,
							0,
							SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
						);
					}
				}
				let _ = ShowWindow(secondary_tray_hwnd, SW_SHOW);
			} else {
				let has_sec_rect = ORIGINAL_SEC_TRAY_RECT
					.lock()
					.map(|g| g.is_some())
					.unwrap_or(false);
				if !has_sec_rect {
					let mut rect = windows::Win32::Foundation::RECT::default();
					let _ = GetWindowRect(secondary_tray_hwnd, &mut rect);
					if let Ok(mut guard) = ORIGINAL_SEC_TRAY_RECT.lock() {
						*guard = Some(rect);
					}
				}
				let _ = ShowWindow(secondary_tray_hwnd, SW_HIDE);
				let _ = SetWindowPos(
					secondary_tray_hwnd,
					None,
					-10000,
					-10000,
					0,
					0,
					SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
				);
			}
		}
	}
}

/// Extracts the icon a shortcut declares for itself (`IShellLinkW::GetIconLocation`).
///
/// This is how Firefox web apps and user-customized shortcuts store their icon.
/// Returns only real image files (`.ico`, `.png`, ...). Icon references into
/// executables/DLLs are ignored so the regular target-based extraction is used
/// instead (it yields a better icon than the file's first resource).
pub fn get_shortcut_icon_location(path: &str) -> Option<String> {
	unsafe {
		let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).ok()?;
		let persist_file: IPersistFile = shell_link.cast().ok()?;
		let wide_path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
		persist_file
			.Load(
				windows::core::PCWSTR(wide_path.as_ptr()),
				windows::Win32::System::Com::STGM(0),
			)
			.ok()?;

		let mut buffer = [0u16; 512];
		let mut index = 0i32;
		shell_link.GetIconLocation(&mut buffer, &mut index).ok()?;

		let raw = String::from_utf16_lossy(&buffer)
			.trim_matches(char::from(0))
			.trim()
			.to_string();
		if raw.is_empty() {
			return None;
		}

		let location = expand_env_vars(&raw);
		let ext = std::path::Path::new(&location)
			.extension()?
			.to_str()?
			.to_lowercase();
		if !matches!(
			ext.as_str(),
			"ico" | "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif"
		) {
			return None;
		}
		if !std::path::Path::new(&location).exists() {
			return None;
		}
		Some(location)
	}
}

/// Expands `%VAR%` references using the current process environment.
fn expand_env_vars(input: &str) -> String {
	let mut result = String::with_capacity(input.len());
	let mut rest = input;
	while let Some(start) = rest.find('%') {
		result.push_str(&rest[..start]);
		let after = &rest[start + 1..];
		if let Some(end) = after.find('%') {
			let name = &after[..end];
			if !name.is_empty() && !name.contains(' ') {
				match std::env::var(name) {
					Ok(value) => result.push_str(&value),
					Err(_) => {
						result.push('%');
						result.push_str(name);
						result.push('%');
					}
				}
			} else {
				result.push('%');
				result.push_str(name);
				result.push('%');
			}
			rest = &after[end + 1..];
		} else {
			result.push_str(&rest[start..]);
			rest = "";
			break;
		}
	}
	result.push_str(rest);
	result
}

/// Reads an image file (PNG/ICO/etc.) and returns it as a base64 PNG data URI.
pub fn image_file_to_base64(path: &str) -> Option<String> {
	let bytes = std::fs::read(path).ok()?;
	if bytes.len() > 8 * 1024 * 1024 {
		return None;
	}

	if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
		use base64::Engine;
		return Some(format!(
			"data:image/png;base64,{}",
			general_purpose::STANDARD.encode(&bytes)
		));
	}

	let img = image::load_from_memory(&bytes).ok()?;
	let mut png: Vec<u8> = Vec::new();
	img
		.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
		.ok()?;
	use base64::Engine;
	Some(format!(
		"data:image/png;base64,{}",
		general_purpose::STANDARD.encode(&png)
	))
}

/// True when the window's client area covers its monitor and it is not a
/// standard (captioned) maximized window — i.e. a real fullscreen window.
pub fn is_window_fullscreen(hwnd: HWND) -> bool {
	unsafe {
		use windows::Win32::Foundation::{POINT, RECT};
		use windows::Win32::Graphics::Gdi::{
			ClientToScreen, GetMonitorInfoA, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
		};
		use windows::Win32::UI::WindowsAndMessaging::{
			GetClientRect, GetWindowLongW, IsZoomed, GWL_STYLE, WS_CAPTION, WS_MAXIMIZE,
		};

		let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
		if monitor.is_invalid() {
			return false;
		}
		let mut info = MONITORINFO {
			cbSize: std::mem::size_of::<MONITORINFO>() as u32,
			..Default::default()
		};
		if !GetMonitorInfoA(monitor, &mut info).as_bool() {
			return false;
		}
		let screen = info.rcMonitor;

		let mut client = RECT::default();
		if GetClientRect(hwnd, &mut client).is_err() {
			return false;
		}
		let mut top_left = POINT {
			x: client.left,
			y: client.top,
		};
		let mut bottom_right = POINT {
			x: client.right,
			y: client.bottom,
		};
		let _ = ClientToScreen(hwnd, &mut top_left);
		let _ = ClientToScreen(hwnd, &mut bottom_right);

		let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
		let is_maximized_standard =
			(IsZoomed(hwnd).as_bool() || (style & WS_MAXIMIZE.0) != 0) && (style & WS_CAPTION.0) != 0;

		let client_fullscreen = top_left.x <= screen.left
			&& top_left.y <= screen.top
			&& bottom_right.x >= screen.right
			&& bottom_right.y >= screen.bottom;
		client_fullscreen && !is_maximized_standard
	}
}

/// Resolves a bare executable name (e.g. `notepad.exe`, `msedge`, `wt.exe`) to a
/// full path, using the same mechanisms Windows uses: the registry `App Paths`
/// registration, then the standard executable search path, then a few well-known
/// install locations for apps that register in neither.
///
/// This matters for default pins: on Windows 11 `notepad.exe` resolves through
/// App Paths to the Store Notepad package, whose icon comes from the package
/// manifest rather than the stale stub in System32.
pub fn resolve_executable_path(name: &str) -> Option<String> {
	let trimmed = name.trim().trim_matches('"');
	if trimmed.is_empty() || trimmed.contains('\\') || trimmed.contains('/') {
		return None;
	}

	let file = if trimmed.to_lowercase().ends_with(".exe") {
		trimmed.to_string()
	} else {
		format!("{}.exe", trimmed)
	};

	if let Some(path) = app_paths_lookup(&file) {
		if std::path::Path::new(&path).exists() {
			return Some(path);
		}
	}
	if let Some(path) = search_system_path(&file) {
		return Some(path);
	}
	known_install_location(&file)
}

/// Reads `HKCU`/`HKLM\Software\Microsoft\Windows\CurrentVersion\App Paths\<file>`.
fn app_paths_lookup(file: &str) -> Option<String> {
	use windows::Win32::System::Registry::{
		RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
		KEY_READ,
	};

	unsafe {
		let subkey: Vec<u16> = format!(
			"Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{}",
			file
		)
		.encode_utf16()
		.chain(std::iter::once(0))
		.collect();

		for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
			let mut key = HKEY::default();
			if RegOpenKeyExW(
				root,
				windows::core::PCWSTR(subkey.as_ptr()),
				None,
				KEY_READ,
				&mut key,
			)
			.0 != 0
			{
				continue;
			}

			let mut buffer = [0u16; 1024];
			let mut size = (buffer.len() * 2) as u32;
			let status = RegQueryValueExW(
				key,
				windows::core::PCWSTR::null(),
				None,
				None,
				Some(buffer.as_mut_ptr() as *mut u8),
				Some(&mut size),
			);
			let _ = RegCloseKey(key);
			if status.0 != 0 {
				continue;
			}

			let value = String::from_utf16_lossy(&buffer[..(size as usize / 2).min(buffer.len())]);
			let value = value
				.trim_matches(char::from(0))
				.trim()
				.trim_matches('"')
				.to_string();
			if !value.is_empty() {
				return Some(value);
			}
		}
		None
	}
}

/// Standard executable search (PATH, System32, Windows, ...).
fn search_system_path(file: &str) -> Option<String> {
	use windows::Win32::Storage::FileSystem::SearchPathW;

	unsafe {
		let wide: Vec<u16> = file.encode_utf16().chain(std::iter::once(0)).collect();
		let mut buffer = vec![0u16; 32768];
		let len = SearchPathW(
			None,
			windows::core::PCWSTR(wide.as_ptr()),
			windows::core::PCWSTR::null(),
			Some(&mut buffer),
			None,
		);
		if len == 0 || len as usize >= buffer.len() {
			return None;
		}

		let path = String::from_utf16_lossy(&buffer[..len as usize]);
		if std::path::Path::new(&path).exists() {
			Some(path)
		} else {
			None
		}
	}
}

/// Last resort for apps that are neither registered in App Paths nor on PATH.
fn known_install_location(file: &str) -> Option<String> {
	let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
	let home = std::env::var("USERPROFILE").unwrap_or_default();

	let candidates: Vec<String> = match file.to_lowercase().as_str() {
		"code.exe" | "code-insiders.exe" => vec![
			format!(
				"{}\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe",
				home
			),
			format!("{}\\Programs\\Microsoft VS Code\\Code.exe", local),
			"C:\\Program Files\\Microsoft VS Code\\Code.exe".into(),
			"C:\\Program Files (x86)\\Microsoft VS Code\\Code.exe".into(),
		],
		"msedge.exe" => vec![
			"C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe".into(),
			"C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe".into(),
			format!("{}\\Microsoft\\Edge\\Application\\msedge.exe", local),
		],
		"notepad.exe" => vec![
			"C:\\Windows\\System32\\notepad.exe".into(),
			"C:\\Windows\\notepad.exe".into(),
		],
		"explorer.exe" => vec!["C:\\Windows\\explorer.exe".into()],
		_ => Vec::new(),
	};

	candidates
		.into_iter()
		.find(|c| std::path::Path::new(c).exists())
}

/// Reads the `System.AppUserModel.ID` shell property of a window.
///
/// This is the same identity the taskbar uses to group windows: for Store/UWP
/// apps it is the package AppUserModelID, for installed browser web apps it is
/// the browser's web app id. Unlike the process command line it is always
/// available on the window itself.
pub fn get_window_app_user_model_id(hwnd: HWND) -> Option<String> {
	unsafe {
		use windows::Win32::Foundation::PROPERTYKEY;
		use windows::Win32::System::Com::StructuredStorage::{
			PropVariantClear, PropVariantToStringAlloc,
		};
		use windows::Win32::UI::Shell::PropertiesSystem::{
			IPropertyStore, SHGetPropertyStoreForWindow,
		};

		const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
			fmtid: windows::core::GUID {
				data1: 0x9F4C2855,
				data2: 0x9F79,
				data3: 0x4B39,
				data4: [0xA8, 0xD0, 0xE1, 0xD4, 0x2D, 0xE1, 0xD5, 0xF3],
			},
			pid: 5,
		};

		let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
		let mut prop = store.GetValue(&PKEY_APP_USER_MODEL_ID).ok()?;
		let result = PropVariantToStringAlloc(&prop)
			.ok()
			.map(|pwstr| String::from_utf16_lossy(pwstr.as_wide()).trim().to_string());
		let _ = PropVariantClear(&mut prop);
		result.filter(|s| !s.is_empty())
	}
}

pub unsafe fn icon_to_base64(hicon: HICON) -> Option<String> {
	let factory: IWICImagingFactory =
		CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
	let bitmap = factory.CreateBitmapFromHICON(hicon).ok()?;
	wic_bitmap_to_base64(&factory, &bitmap)
}

/// Encodes an `HBITMAP` (as returned by `IShellItemImageFactory::GetImage`) to a base64 PNG.
pub unsafe fn hbitmap_to_base64(hbitmap: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<String> {
	let factory: IWICImagingFactory =
		CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
	let bitmap = factory
		.CreateBitmapFromHBITMAP(
			hbitmap,
			windows::Win32::Graphics::Gdi::HPALETTE::default(),
			windows::Win32::Graphics::Imaging::WICBitmapUsePremultipliedAlpha,
		)
		.ok()?;
	wic_bitmap_to_base64(&factory, &bitmap)
}

unsafe fn wic_bitmap_to_base64(
	factory: &IWICImagingFactory,
	bitmap: &windows::Win32::Graphics::Imaging::IWICBitmapSource,
) -> Option<String> {
	let stream = CreateStreamOnHGlobal(HGLOBAL(std::ptr::null_mut()), true).ok()?;
	let encoder = factory
		.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())
		.ok()?;
	encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;

	let mut frame = None;
	encoder
		.CreateNewFrame(&mut frame, std::ptr::null_mut())
		.ok()?;
	let frame = frame?;
	frame.Initialize(None).ok()?;

	let (mut width, mut height) = (0u32, 0u32);
	bitmap.GetSize(&mut width, &mut height).ok()?;
	frame.SetSize(width, height).ok()?;

	let mut format = GUID_WICPixelFormat32bppPBGRA;
	frame.SetPixelFormat(&mut format).ok()?;

	frame.WriteSource(bitmap, std::ptr::null()).ok()?;
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
	std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis() as i64
}

/// Load settings.json into the in-memory cache. Call once at startup.
pub fn init_settings_cache(app: &tauri::AppHandle) {
	use crate::state::SETTINGS_CACHE;
	use tauri::Manager;
	let _ = SETTINGS_CACHE.set(std::sync::Mutex::new(std::collections::HashMap::new()));
	if let Some(path) = app
		.path()
		.app_config_dir()
		.ok()
		.map(|p| p.join("settings.json"))
	{
		if let Ok(content) = std::fs::read_to_string(path) {
			if let Ok(settings) =
				serde_json::from_str::<std::collections::HashMap<String, serde_json::Value>>(&content)
			{
				if let Ok(mut cache) = SETTINGS_CACHE.get().unwrap().lock() {
					*cache = settings;
				}
			}
		}
	}
}

/// Replace the entire settings cache (used by the file watcher on external changes).
pub fn replace_settings_cache(new_settings: std::collections::HashMap<String, serde_json::Value>) {
	if let Ok(mut cache) = crate::state::SETTINGS_CACHE.get().unwrap().lock() {
		*cache = new_settings;
	}
}

pub fn get_bloom_scale(_app: &tauri::AppHandle) -> f64 {
	get_setting_str(_app, "bloom-scale")
		.and_then(|s| s.parse::<f64>().ok())
		.unwrap_or(1.0)
}

/// Read any string value from the settings cache. Returns None if the key is absent.
pub fn get_setting_str(_app: &tauri::AppHandle, key: &str) -> Option<String> {
	let cache = crate::state::SETTINGS_CACHE.get()?;
	let guard = cache.lock().ok()?;
	guard.get(key)?.as_str().map(|s| s.to_string())
}

/// Re-assert HWND_TOPMOST without activating the window.
///
/// Tauri's `set_always_on_top(true)` calls `SetWindowPos(HWND_TOPMOST)` without
/// `SWP_NOACTIVATE`, which causes Windows to send `WM_ACTIVATE` to the WebView2 window.
/// This activation message makes the WebView compositor briefly blank/hide the window,
/// and can also strip the `WS_EX_NOACTIVATE` extended style.
///
/// This helper uses the raw Win32 call with the correct flags and re-stamps
/// `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` to prevent both problems.
pub fn re_assert_topmost(hwnd: HWND) {
	unsafe {
		use windows::Win32::UI::WindowsAndMessaging::{
			GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
			SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSENDCHANGING, SWP_NOSIZE, WS_EX_NOACTIVATE,
			WS_EX_TOOLWINDOW,
		};
		// Set topmost without activating or notifying the window
		let _ = SetWindowPos(
			hwnd,
			Some(HWND_TOPMOST),
			0,
			0,
			0,
			0,
			SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
		);
		// Re-stamp NOACTIVATE + TOOLWINDOW — HWND_TOPMOST can cause these to be reset
		// by the shell on some Windows builds
		let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
		let new_ex = ex | WS_EX_NOACTIVATE.0 as usize | WS_EX_TOOLWINDOW.0 as usize;
		SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex as isize);
	}
}

/// Captures an HWND to a base64-encoded PNG thumbnail, scaling it down if it exceeds max_width x max_height.
pub fn capture_hwnd_to_base64(hwnd: HWND, max_width: u32, max_height: u32) -> Option<String> {
	unsafe {
		use windows::Win32::Foundation::RECT;
		use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
		use windows::Win32::Graphics::Gdi::{
			CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
			ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HDC,
		};
		use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, IsWindow, WINDOWPLACEMENT};
		#[link(name = "user32")]
		extern "system" {
			pub fn PrintWindow(hwnd: HWND, hdcBlt: HDC, nFlags: u32) -> i32;
		}

		if !IsWindow(Some(hwnd)).as_bool() {
			return None;
		}

		let mut rect = RECT::default();
		let _ = DwmGetWindowAttribute(
			hwnd,
			DWMWA_EXTENDED_FRAME_BOUNDS,
			&mut rect as *mut _ as *mut _,
			std::mem::size_of::<RECT>() as u32,
		);
		if rect.right == 0
			&& rect.bottom == 0
			&& windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect).is_err()
		{
			return None;
		}
		let mut width = rect.right - rect.left;
		let mut height = rect.bottom - rect.top;
		if width <= 10 || height <= 10 {
			let mut wp = WINDOWPLACEMENT {
				length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
				..Default::default()
			};
			if GetWindowPlacement(hwnd, &mut wp).is_ok() {
				width = wp.rcNormalPosition.right - wp.rcNormalPosition.left;
				height = wp.rcNormalPosition.bottom - wp.rcNormalPosition.top;
			}
		}
		if width <= 100 || height <= 100 || width > 7680 || height > 4320 {
			return None;
		}

		let hdc_screen = GetDC(None);
		let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
		let hbm_mem = CreateCompatibleBitmap(hdc_screen, width, height);
		let h_old = SelectObject(hdc_mem, hbm_mem.into());
		let success = PrintWindow(hwnd, hdc_mem, 2);

		let mut result = None;
		if success != 0 {
			let mut bmi = BITMAPINFO {
				bmiHeader: BITMAPINFOHEADER {
					biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
					biWidth: width,
					biHeight: -height,
					biPlanes: 1,
					biBitCount: 32,
					biCompression: BI_RGB.0,
					biSizeImage: 0,
					biXPelsPerMeter: 0,
					biYPelsPerMeter: 0,
					biClrUsed: 0,
					biClrImportant: 0,
				},
				bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default(); 1],
			};
			let mut pixels = vec![0u8; (width * height * 4) as usize];
			if GetDIBits(
				hdc_mem,
				hbm_mem,
				0,
				height as u32,
				Some(pixels.as_mut_ptr() as *mut _),
				&mut bmi,
				DIB_RGB_COLORS,
			) != 0
			{
				for chunk in pixels.chunks_exact_mut(4) {
					let b = chunk[0];
					let r = chunk[2];
					chunk[0] = r;
					chunk[2] = b;
					chunk[3] = 255;
				}
				if let Ok(Some(png_base64)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
					if let Some(mut img) = image::RgbaImage::from_raw(width as u32, height as u32, pixels) {
						if img.width() > max_width || img.height() > max_height {
							let dyn_img = image::DynamicImage::ImageRgba8(img);
							img = dyn_img
								.resize(max_width, max_height, image::imageops::FilterType::Triangle)
								.into_rgba8();
						}
						let mut buf = std::io::Cursor::new(Vec::new());
						if image::write_buffer_with_format(
							&mut buf,
							&img,
							img.width(),
							img.height(),
							image::ColorType::Rgba8,
							image::ImageFormat::Png,
						)
						.is_ok()
						{
							use base64::Engine;
							let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
							return Some(format!("data:image/png;base64,{}", b64));
						}
					}
					None
				})) {
					result = Some(png_base64);
				}
			}
		}
		SelectObject(hdc_mem, h_old);
		let _ = DeleteObject(hbm_mem.into());
		let _ = DeleteDC(hdc_mem);
		ReleaseDC(None, hdc_screen);

		result
	}
}

#[cfg(test)]
mod tests {
	use super::expand_env_vars;

	#[test]
	fn env_expansion() {
		let out = expand_env_vars("%SystemRoot%\\System32");
		assert!(out.to_lowercase().ends_with("\\system32"), "got {out}");
		assert!(!out.contains('%'), "got {out}");

		// Unknown variables stay verbatim
		assert_eq!(
			expand_env_vars("%BLOOM_NOT_A_REAL_VAR%\\x"),
			"%BLOOM_NOT_A_REAL_VAR%\\x"
		);

		// Unclosed percent is left alone
		assert_eq!(expand_env_vars("50% done"), "50% done");

		// Plain paths are untouched
		assert_eq!(expand_env_vars("C:\\plain\\path"), "C:\\plain\\path");
	}

	#[test]
	fn bare_executable_resolution() {
		use super::resolve_executable_path;

		for name in ["notepad", "notepad.exe", "msedge"] {
			let path = resolve_executable_path(name).unwrap_or_else(|| panic!("{name} did not resolve"));
			assert!(
				std::path::Path::new(&path).exists(),
				"{name} -> {path} does not exist"
			);
			assert!(path.to_lowercase().ends_with(".exe"), "{name} -> {path}");
		}

		// Already-qualified paths and nonsense are not resolved
		assert_eq!(resolve_executable_path("C:\\Windows\\notepad.exe"), None);
		assert_eq!(
			resolve_executable_path("bloom-definitely-not-installed"),
			None
		);
		assert_eq!(resolve_executable_path(""), None);
	}
}
