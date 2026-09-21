use std::sync::{atomic::{AtomicBool, AtomicU8, AtomicI64, AtomicI32, Ordering}, Mutex, OnceLock};
use std::sync::mpsc::{channel, Sender};
use std::sync::{
	atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering},
	Mutex, OnceLock,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use windows::core::BOOL;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::System::Threading::{
	OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
	CallNextHookEx, GetWindowLongW, GetWindowThreadProcessId, IsWindowVisible, SetWindowsHookExW,
	GWL_EXSTYLE, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEMOVE, WS_EX_TOOLWINDOW,
};
use wmi::{COMLibrary, WMIConnection};

static KEYBOARD_HOOK_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
/// Physical Win key state, tracked so Win+1-9 can be claimed while the Win key
/// itself keeps flowing to the shell (a lone Win tap must still open Start).
static WIN_KEY_DOWN: AtomicBool = AtomicBool::new(false);
/// Digit (1-9) of the currently held Win+Number combo, 0 when none. Key
/// auto-repeat re-fires the keydown; only the first press may toggle an app.
static WIN_NUMBER_HELD: AtomicU8 = AtomicU8::new(0);
/// Virtual key Microsoft documents as "unassigned", used as the mask key.
/// See `send_start_menu_mask`.
const MASK_VK: u16 = 0xE8;

pub fn setup_keyboard_hook(app_handle: AppHandle) -> windows::Win32::UI::WindowsAndMessaging::HHOOK {
    let _ = KEYBOARD_HOOK_APP_HANDLE.set(app_handle);
    unsafe { windows::Win32::UI::WindowsAndMessaging::SetWindowsHookExA(windows::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL, Some(keyboard_hook_proc), None, 0).expect("Failed") }
}

/// Maps the top-row digit keys `1`-`9` to the zero-based dock slot.
fn win_number_index(vk: u16) -> Option<u8> {
    match vk {
        0x31..=0x39 => Some((vk - 0x31) as u8),
        _ => None,
    }
}

/// The dock's Win+Number replacement can be turned off in Settings > Dock.
fn dock_win_number_enabled() -> bool {
    let Some(app) = KEYBOARD_HOOK_APP_HANDLE.get() else { return true };
    crate::utils::get_setting_str(app, "bloom-dock-win-number-enabled")
        .map(|v| v != "false")
        .unwrap_or(true)
}

/// Physical Win state straight from the OS. The tracked flag can go stale when
/// a keyup is never delivered (secure desktop, keyboard unplugged, hook
/// timeout); without this check a stale flag would swallow digits forever.
fn win_key_physically_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LWIN, VK_RWIN};
    unsafe {
        (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
    }
}

/// Explorer opens the Start menu when it only sees a Win keydown and keyup.
/// Swallowing a Win+Number combo would look exactly like that on Win release,
/// so a tap of an unassigned key is injected first — the same trick as
/// AutoHotkey's `#MenuMaskKey` — making the shell treat Win as a real modifier.
fn send_start_menu_mask() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY};
    let mask = VIRTUAL_KEY(MASK_VK);
    let inputs = [
        INPUT {
            r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: mask, wScan: 0, dwFlags: Default::default(), time: 0, dwExtraInfo: 0 } },
        },
        INPUT {
            r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: mask, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } },
        },
    ];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32); }
}

/// Hands the slot to the dock window, whose click handler already knows how to
/// focus a running window or launch a pinned app.
fn emit_dock_win_number(index: u8) {
    let Some(app) = KEYBOARD_HOOK_APP_HANDLE.get().cloned() else { return };
    // Never block the input pipeline on WebView IPC.
    tauri::async_runtime::spawn(async move {
        let _ = app.emit_to("dock", "dock-win-number", index);
    });
}

unsafe extern "system" fn keyboard_hook_proc(code: i32, wparam: windows::Win32::Foundation::WPARAM, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN, WM_KEYUP, WM_SYSKEYUP, LLKHF_INJECTED};
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_VOLUME_MUTE, VK_VOLUME_UP, VK_VOLUME_DOWN, VK_LWIN, VK_RWIN, VIRTUAL_KEY};
    if code >= 0 {
        let kb = *(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk_code = VIRTUAL_KEY(kb.vkCode as u16);
        let is_down = wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize;
        let is_up = wparam.0 == WM_KEYUP as usize || wparam.0 == WM_SYSKEYUP as usize;

        // Only physical presses drive the Win+Number replacement; injected
        // events (our own Start taps and mask key) must not re-enter it.
        if (kb.flags.0 & LLKHF_INJECTED.0) == 0 {
            if vk_code == VK_LWIN || vk_code == VK_RWIN {
                if is_down {
                    WIN_KEY_DOWN.store(true, Ordering::Relaxed);
                    // A fresh Win press always starts a fresh combo, even if the
                    // previous digit's keyup was missed.
                    WIN_NUMBER_HELD.store(0, Ordering::Relaxed);
                } else if is_up {
                    WIN_KEY_DOWN.store(false, Ordering::Relaxed);
                    WIN_NUMBER_HELD.store(0, Ordering::Relaxed);
                }
            } else if WIN_KEY_DOWN.load(Ordering::Relaxed) {
                if let Some(index) = win_number_index(vk_code.0) {
                    let slot = index + 1;
                    if is_up {
                        let _ = WIN_NUMBER_HELD.compare_exchange(slot, 0, Ordering::Relaxed, Ordering::Relaxed);
                    } else if is_down {
                        if !win_key_physically_down() {
                            WIN_KEY_DOWN.store(false, Ordering::Relaxed);
                            WIN_NUMBER_HELD.store(0, Ordering::Relaxed);
                        } else if NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed)
                            && dock_win_number_enabled()
                        {
                            if WIN_NUMBER_HELD.swap(slot, Ordering::Relaxed) != slot {
                                send_start_menu_mask();
                                emit_dock_win_number(index);
                            }
                            return windows::Win32::Foundation::LRESULT(1);
                        }
                    }
                }
            }
        }

        if vk_code == VK_VOLUME_MUTE || vk_code == VK_VOLUME_UP || vk_code == VK_VOLUME_DOWN {
            if is_down { handle_volume_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
        if vk_code.0 == 0x216 || vk_code.0 == 0x217 {
            if is_down { handle_brightness_key_event(vk_code); }
            return windows::Win32::Foundation::LRESULT(1);
        }
    }
    windows::Win32::UI::WindowsAndMessaging::CallNextHookEx(None, code, wparam, lparam)
}

fn handle_volume_key_event(vk_code: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) {
	use std::sync::atomic::AtomicU64;
	static LAST_TIME: AtomicU64 = AtomicU64::new(0);
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_millis() as u64;
	let last = LAST_TIME.load(std::sync::atomic::Ordering::Relaxed);
	if now - last < 50 {
		return;
	}
	LAST_TIME.store(now, std::sync::atomic::Ordering::Relaxed);
	if let Some(sender) = crate::state::COMMAND_SENDER.get() {
		use windows::Win32::UI::Input::KeyboardAndMouse::{
			VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
		};
		let cmd = match vk_code {
			VK_VOLUME_MUTE => Some(SystemCommand::VolumeMute),
			VK_VOLUME_UP => Some(SystemCommand::VolumeUp),
			VK_VOLUME_DOWN => Some(SystemCommand::VolumeDown),
			_ => None,
		};
		if let Some(cmd) = cmd {
			let _ = sender.send(cmd);
		}
	}
}

fn handle_brightness_key_event(vk_code: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) {
	if let Some(sender) = crate::state::COMMAND_SENDER.get() {
		let cmd = if vk_code.0 == 0x216 {
			Some(SystemCommand::BrightnessDown)
		} else if vk_code.0 == 0x217 {
			Some(SystemCommand::BrightnessUp)
		} else {
			None
		};
		if let Some(cmd) = cmd {
			let _ = sender.send(cmd);
		}
	}
}
use crate::state::*;
use crate::types::*;
use crate::utils::*;

pub fn setup_taskbar_hook() {
	unsafe {
		use windows::Win32::UI::Accessibility::SetWinEventHook;
		use windows::Win32::UI::WindowsAndMessaging::{
			EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_SHOW, WINEVENT_OUTOFCONTEXT,
		};

		// Hook both "Show" and "Location Change" (happen when maximizing/switching apps)
		let _show_hook = SetWinEventHook(
			EVENT_OBJECT_SHOW,
			EVENT_OBJECT_SHOW,
			None,
			Some(taskbar_event_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);
		let _loc_hook = SetWinEventHook(
			EVENT_OBJECT_LOCATIONCHANGE,
			EVENT_OBJECT_LOCATIONCHANGE,
			None,
			Some(taskbar_event_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);
	}
}

static WINDOW_CHANGE_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static LAST_WINDOW_CHANGE_MS: AtomicI64 = AtomicI64::new(0);

pub fn setup_window_change_hook(app_handle: AppHandle) {
	unsafe {
		let _ = WINDOW_CHANGE_APP_HANDLE.set(app_handle);

		use windows::Win32::UI::Accessibility::SetWinEventHook;
		use windows::Win32::UI::WindowsAndMessaging::{
			EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_SHOW,
			WINEVENT_OUTOFCONTEXT,
		};

		// Hook create and destroy events for top-level windows
		let _create_hook = SetWinEventHook(
			EVENT_OBJECT_CREATE,
			EVENT_OBJECT_CREATE,
			None,
			Some(window_change_event_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);
		let _destroy_hook = SetWinEventHook(
			EVENT_OBJECT_DESTROY,
			EVENT_OBJECT_DESTROY,
			None,
			Some(window_change_event_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);
		// Hook show/hide events (fires when windows become visible/invisible)
		let _show_hook = SetWinEventHook(
			EVENT_OBJECT_SHOW,
			EVENT_OBJECT_SHOW,
			None,
			Some(window_change_event_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);
		let _hide_hook = SetWinEventHook(
			EVENT_OBJECT_HIDE,
			EVENT_OBJECT_HIDE,
			None,
			Some(window_change_event_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);

		// Store hooks to prevent them from being dropped
		Box::leak(Box::new(_create_hook));
	}
}

unsafe extern "system" fn window_change_event_proc(
	_hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
	event: u32,
	hwnd: HWND,
	id_object: i32,
	id_child: i32,
	_event_thread: u32,
	_ms_event_time: u32,
) {
	if hwnd.0.is_null() {
		return;
	}

	// A top-level window appeared/disappeared (or focus moved): capture UI
	// state may have changed. Cheap flag; the worker thread does the scan.
	{
		use windows::Win32::UI::WindowsAndMessaging::{
			EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, EVENT_OBJECT_SHOW,
		};
		if id_object == 0
			&& id_child == 0
			&& (event == EVENT_OBJECT_SHOW || event == EVENT_OBJECT_HIDE || event == EVENT_OBJECT_DESTROY)
		{
			CAPTURE_RECHECK.store(true, Ordering::Relaxed);
		}
	}

	use windows::Win32::UI::WindowsAndMessaging::{
		GetWindowLongW, GetWindowThreadProcessId, IsWindow, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
	};

	if !IsWindow(Some(hwnd)).as_bool() {
		return;
	}

	// Skip tool windows (docks, trays, etc.)
	let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
	if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
		return;
	}

	// Skip our own process
	let my_pid = std::process::id();
	let mut pid = 0u32;
	GetWindowThreadProcessId(hwnd, Some(&mut pid));
	if pid == my_pid {
		return;
	}

	// Debounce bursts: closing a window (especially a UWP app) emits several
	// show/hide/destroy events within milliseconds. Every event stores its
	// timestamp and schedules an emit; only the newest one actually fires, so
	// the final state of a burst is always reported. Dropping the trailing
	// events would leave closed apps visible in the dock.
	let now = crate::utils::get_now_ms();
	LAST_WINDOW_CHANGE_MS.store(now, Ordering::Relaxed);

	if let Some(app_handle) = WINDOW_CHANGE_APP_HANDLE.get().cloned() {
		tauri::async_runtime::spawn(async move {
			tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
			if LAST_WINDOW_CHANGE_MS.load(Ordering::Relaxed) == now {
				let _ = app_handle.emit("windows-changed", ());
			}
		});
	}
}

pub fn setup_thumbnail_capture(_app_handle: AppHandle) {
	unsafe {
		use windows::Win32::UI::Accessibility::SetWinEventHook;
		use windows::Win32::UI::WindowsAndMessaging::{
			EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
			WINEVENT_OUTOFCONTEXT,
		};

		let hook = SetWinEventHook(
			EVENT_SYSTEM_MINIMIZESTART,
			EVENT_SYSTEM_MINIMIZEEND,
			None,
			Some(thumbnail_capture_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);
		Box::leak(Box::new(hook));

		let focus_hook = SetWinEventHook(
			EVENT_SYSTEM_FOREGROUND,
			EVENT_SYSTEM_FOREGROUND,
			None,
			Some(focus_event_proc),
			0,
			0,
			WINEVENT_OUTOFCONTEXT,
		);
		Box::leak(Box::new(focus_hook));
	}
}

unsafe extern "system" fn focus_event_proc(
	_hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
	_event: u32,
	hwnd: HWND,
	_id_object: i32,
	_id_child: i32,
	_event_thread: u32,
	_ms_event_time: u32,
) {
	if hwnd.0.is_null() {
		return;
	}
	// Foreground moved — e.g. Snipping Tool opened/closed or got minimised.
	CAPTURE_RECHECK.store(true, Ordering::Relaxed);
	use windows::Win32::UI::WindowsAndMessaging::{
		GetWindowLongW, IsWindow, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
	};

	if !IsWindow(Some(hwnd)).as_bool() {
		return;
	}
	let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
	if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
		return;
	}

	let my_pid = std::process::id();
	let mut pid = 0u32;
	windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
	if pid == my_pid {
		return;
	}

	let hwnd_raw = hwnd.0 as isize;
	if let Some(map) = crate::state::FOCUS_TIMESTAMPS.get() {
		if let Ok(mut guard) = map.lock() {
			guard.insert(hwnd_raw, crate::utils::get_now_ms());
		}
	}

	// Keep the thumbnail cache warm for the focused window so that hovering a
	// later-minimized window uses the cached image instead of restoring it.
	std::thread::spawn(move || {
		use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, IsIconic, IsWindow};

		std::thread::sleep(std::time::Duration::from_millis(200));
		let hwnd = HWND(hwnd_raw as *mut _);
		unsafe {
			if !IsWindow(Some(hwnd)).as_bool() || IsIconic(hwnd).as_bool() {
				return;
			}
		}

		// Skip fullscreen windows (games, video players): capturing them can hitch.
		if crate::utils::is_window_fullscreen(hwnd) {
			return;
		}

		let mut text = [0u16; 2];
		unsafe {
			if GetWindowTextW(hwnd, &mut text) == 0 {
				return;
			}
		}

		// Refresh at most once per window every two seconds
		if let Some(cache) = crate::state::THUMBNAIL_CACHE.get() {
			if let Ok(guard) = cache.lock() {
				if let Some((_, ts)) = guard.get(&hwnd_raw) {
					if crate::utils::get_now_ms() - ts < 2000 {
						return;
					}
				}
			}
		}

		if THUMB_CAPTURE_IN_FLIGHT.swap(true, Ordering::Relaxed) {
			return;
		}
		let _guard = ThumbnailCaptureGuard;

		if let Some(img) = crate::utils::capture_hwnd_to_base64(hwnd, 320, 200) {
			if let Some(cache) = crate::state::THUMBNAIL_CACHE.get() {
				if let Ok(mut guard) = cache.lock() {
					guard.insert(hwnd_raw, (img, crate::utils::get_now_ms()));
				}
			}
		}
	});
}

static THUMB_CAPTURE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct ThumbnailCaptureGuard;

impl Drop for ThumbnailCaptureGuard {
	fn drop(&mut self) {
		THUMB_CAPTURE_IN_FLIGHT.store(false, Ordering::Relaxed);
	}
}

unsafe extern "system" fn thumbnail_capture_proc(
	_hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
	event: u32,
	hwnd: HWND,
	_id_object: i32,
	_id_child: i32,
	_event_thread: u32,
	_ms_event_time: u32,
) {
	if hwnd.0.is_null() {
		return;
	}

	// Only the restore event is useful here. On MINIMIZESTART the window is
	// mid-animation and PrintWindow can capture a black frame, which would
	// overwrite a good cached thumbnail; the focus hook keeps the cache warm
	// before a window is minimized.
	if event != windows::Win32::UI::WindowsAndMessaging::EVENT_SYSTEM_MINIMIZEEND {
		return;
	}

	use windows::Win32::UI::WindowsAndMessaging::{
		GetWindowLongW, IsWindow, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
	};

	if !IsWindow(Some(hwnd)).as_bool() {
		return;
	}
	let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
	if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
		return;
	}

	let my_pid = std::process::id();
	let mut pid = 0u32;
	windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
	if pid == my_pid {
		return;
	}

	let mut text = [0u16; 512];
	let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut text);
	if len == 0 {
		return;
	}

	let hwnd_raw = hwnd.0 as isize;

	std::thread::spawn(move || {
		std::thread::sleep(std::time::Duration::from_millis(150));

		// Skip if another capture refreshed this window very recently
		if let Some(cache) = crate::state::THUMBNAIL_CACHE.get() {
			if let Ok(guard) = cache.lock() {
				if let Some((_, ts)) = guard.get(&hwnd_raw) {
					if crate::utils::get_now_ms() - ts < 300 {
						return;
					}
				}
			}
		}

		let hwnd = HWND(hwnd_raw as *mut _);
		if let Some(img) = crate::utils::capture_hwnd_to_base64(hwnd, 320, 200) {
			if let Some(cache) = crate::state::THUMBNAIL_CACHE.get() {
				if let Ok(mut guard) = cache.lock() {
					guard.insert(hwnd_raw, (img, crate::utils::get_now_ms()));
					if guard.len() > 15 {
						use windows::Win32::UI::WindowsAndMessaging::IsWindow;
						guard.retain(|&k, _| unsafe { IsWindow(Some(HWND(k as *mut _))).as_bool() });
					}
				}
			}
		}
	});
}

unsafe extern "system" fn taskbar_event_proc(
	_h_win_event_hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
	_event: u32,
	hwnd: HWND,
	_id_object: i32,
	_id_child: i32,
	_dw_event_thread: u32,
	_dwms_event_time: u32,
) {
	if NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed) {
		let mut class_name = [0u8; 256];
		let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
		let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");

		if class_str == "Shell_TrayWnd" || class_str == "Shell_SecondaryTrayWnd" {
			// Taskbar is trying to show or move: slap it back down.
			set_taskbar_visibility(false, false);
		}
	}
}

pub fn setup_audio_visualization(app_handle: AppHandle) {
	std::thread::spawn(move || {
		use windows::Win32::Media::Audio::{
			eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
			AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
		};
		use windows::Win32::System::Com::{
			CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
		};
		unsafe {
			let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

			const NUM_BANDS: usize = 5;
			let mut max_band_energies = [0.01f32; NUM_BANDS];
			let mut prev_values = [0.1f32; NUM_BANDS];

			loop {
				let _: Result<(), String> = (|| {
					let enumerator: IMMDeviceEnumerator = CoCreateInstance(
						&windows::Win32::Media::Audio::MMDeviceEnumerator,
						None,
						CLSCTX_ALL,
					)
					.map_err(|e| format!("CoCreateInstance failed: {:?}", e))?;

					let device: IMMDevice = enumerator
						.GetDefaultAudioEndpoint(eRender, eConsole)
						.map_err(|e| format!("GetDefaultAudioEndpoint failed: {:?}", e))?;

					let current_id_str = if let Ok(id) = device.GetId() {
						let id_str = windows::core::PCWSTR::from_raw(id.0)
							.to_string()
							.unwrap_or_default();
						CoTaskMemFree(Some(id.0 as *const _));
						id_str
					} else {
						String::new()
					};

					let audio_client: IAudioClient = device
						.Activate(CLSCTX_ALL, None)
						.map_err(|e| format!("Activate failed: {:?}", e))?;

					let format_ptr = audio_client
						.GetMixFormat()
						.map_err(|e| format!("GetMixFormat failed: {:?}", e))?;

					let channels = (*format_ptr).nChannels as usize;
					let bits_per_sample = (*format_ptr).wBitsPerSample;
					let bytes_per_sample = (bits_per_sample / 8) as usize;

					if bytes_per_sample == 0 || channels == 0 || bytes_per_sample > 4 {
						CoTaskMemFree(Some(format_ptr as *const _));
						return Err(format!(
							"Invalid audio format: channels={}, bits={}",
							channels, bits_per_sample
						));
					}

					let buffer_duration = 10_000_000i64;
					audio_client
						.Initialize(
							AUDCLNT_SHAREMODE_SHARED,
							AUDCLNT_STREAMFLAGS_LOOPBACK,
							buffer_duration,
							0,
							format_ptr,
							Some(std::ptr::null()),
						)
						.map_err(|e| format!("Initialize failed: {:?}", e))?;

					CoTaskMemFree(Some(format_ptr as *const _));

					let capture_client: IAudioCaptureClient = audio_client
						.GetService()
						.map_err(|e| format!("GetService failed: {:?}", e))?;

					audio_client
						.Start()
						.map_err(|e| format!("Start failed: {:?}", e))?;

					const FFT_SIZE: usize = 512;
					let mut fft_buffer = vec![0.0f32; FFT_SIZE];
					let mut fft_input: Vec<rustfft::num_complex::Complex<f32>> =
						vec![rustfft::num_complex::Complex::new(0.0, 0.0); FFT_SIZE];
					let mut buffer_pos = 0;

					let mut planner = rustfft::FftPlanner::<f32>::new();
					let fft = planner.plan_fft_forward(FFT_SIZE);

					let mut last_device_check = std::time::Instant::now();
					loop {
						std::thread::sleep(std::time::Duration::from_millis(32));

						if last_device_check.elapsed().as_secs() >= 2 {
							last_device_check = std::time::Instant::now();
							if let Ok(new_device) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) {
								if let Ok(new_id) = new_device.GetId() {
									let new_str = windows::core::PCWSTR::from_raw(new_id.0)
										.to_string()
										.unwrap_or_default();
									CoTaskMemFree(Some(new_id.0 as *const _));
									if current_id_str != new_str {
										return Err("Default device changed".into());
									}
								}
							}
						}

						// Skip all heavy processing if nothing is playing
						if !ANY_MEDIA_PLAYING.load(Ordering::Relaxed) {
							// Still gotta clear the buffer to avoid lag when it starts
							loop {
								let len = match capture_client.GetNextPacketSize() {
									Ok(l) => l,
									Err(_) => {
										return Err("Device invalidated".into());
									}
								};
								if len == 0 {
									break;
								}
								let mut data_ptr: *mut u8 = std::ptr::null_mut();
								let mut num_frames = 0u32;
								let mut flags = 0u32;
								if capture_client
									.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
									.is_err()
								{
									return Err("Device invalidated".into());
								}
								let _ = capture_client.ReleaseBuffer(num_frames);
							}
							continue;
						}

						let mut device_invalidated = false;
						loop {
							let packet_length = match capture_client.GetNextPacketSize() {
								Ok(len) => len,
								Err(_) => {
									device_invalidated = true;
									break;
								}
							};
							if packet_length == 0 {
								break;
							}
							let mut data_ptr: *mut u8 = std::ptr::null_mut();
							let mut num_frames = 0u32;
							let mut flags = 0u32;

							if capture_client
								.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
								.is_err()
							{
								device_invalidated = true;
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
										let window = 0.5
											* (1.0
												- (2.0 * std::f32::consts::PI * buffer_pos as f32 / FFT_SIZE as f32).cos());
										fft_buffer[buffer_pos] = normalized * window;
										buffer_pos += 1;
									}
									if buffer_pos >= FFT_SIZE {
										// Run FFT on the windowed buffer
										for (i, &sample) in fft_buffer.iter().enumerate() {
											fft_input[i] = rustfft::num_complex::Complex::new(sample, 0.0);
										}
										fft.process(&mut fft_input);

										let band_ranges = [(1, 2), (2, 6), (6, 18), (18, 60), (60, 200)];
										let mut output = [0.0f32; NUM_BANDS];
										for (band_idx, &(bin_start, bin_end)) in band_ranges.iter().enumerate() {
											let mut total_mag = 0.0f32;
											let mut count = 0u32;
											for bin in bin_start..bin_end {
												if bin >= FFT_SIZE / 2 {
													break;
												}
												let mag = fft_input[bin].norm();
												total_mag += mag;
												count += 1;
											}
											let avg_mag = total_mag / count.max(1) as f32;
											let mut scaled_mag = avg_mag;
											let weighting = [1.2, 1.2, 1.5, 2.8, 5.0];
											scaled_mag *= weighting[band_idx];
											if scaled_mag > max_band_energies[band_idx] {
												max_band_energies[band_idx] = scaled_mag;
											} else {
												max_band_energies[band_idx] *= 0.99;
											}
											let target = (scaled_mag / max_band_energies[band_idx].max(0.12))
												.min(1.0)
												.powf(0.75);
											let is_rising = target > prev_values[band_idx];
											let smooth_factor = if is_rising { 0.10 } else { 0.20 };
											output[band_idx] =
												prev_values[band_idx] * smooth_factor + target * (1.0 - smooth_factor);
											output[band_idx] = output[band_idx].clamp(0.18, 1.0);
											prev_values[band_idx] = output[band_idx];
										}
										let _ = app_handle.emit(
											"audio-visualization",
											AudioVisualizationData {
												frequencies: output.to_vec(),
											},
										);
										buffer_pos = 0;
									}
								}
								let _ = capture_client.ReleaseBuffer(num_frames);
							}
						}

						if device_invalidated {
							return Err("Device invalidated".into());
						}
					}
				})();

				// Wait before retrying (increased to 2500ms to allow Windows to fully update default endpoint)
				std::thread::sleep(std::time::Duration::from_millis(2500));
			}
			// if com_initialized { CoUninitialize(); }
		}
	});
}

pub fn setup_system_worker(app_handle: AppHandle) -> Sender<SystemCommand> {
	let (tx, rx) = channel::<SystemCommand>();
	let handle_system = app_handle.clone();
	std::thread::spawn(move || {
		use base64::{engine::general_purpose, Engine as _};
		use windows::Media::Control::{
			GlobalSystemMediaTransportControlsSessionManager,
			GlobalSystemMediaTransportControlsSessionPlaybackStatus,
		};
		use windows::Storage::Streams::DataReader;
		use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
		use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator};
		use windows::Win32::System::Com::{
			CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
		};

		unsafe {
			let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
			let enumerator = CoCreateInstance::<_, IMMDeviceEnumerator>(
				&windows::Win32::Media::Audio::MMDeviceEnumerator,
				None,
				CLSCTX_ALL,
			)
			.ok();
			let mut device = enumerator
				.as_ref()
				.and_then(|e| e.GetDefaultAudioEndpoint(eRender, eConsole).ok());
			let mut audio_endpoint_volume: Option<IAudioEndpointVolume> = device
				.as_ref()
				.and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok());

			let mut current_device_id = String::new();
			if let Some(ref d) = device {
				if let Ok(id) = d.GetId() {
					current_device_id = windows::core::PCWSTR::from_raw(id.0)
						.to_string()
						.unwrap_or_default();
					CoTaskMemFree(Some(id.0 as *const _));
				}
			}

			let mut manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
				.and_then(|op| op.get())
				.ok();
			let mut last_processed_media = std::time::Instant::now();
			let mut last_device_check = std::time::Instant::now();
			#[allow(clippy::type_complexity)]
			let mut last_emitted_info: Option<(String, String, bool, bool, Option<String>, i64, i64)> = None;
			let mut last_volume: f32 = -1.0;
			let mut last_muted: bool = false;

			let hide_osd = || {
				use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, ShowWindow, SW_HIDE};
				let class1 = windows::core::PCSTR(c"NativeHWNDHost".as_ptr() as *const u8);
				if let Ok(hwnd1) = FindWindowA(class1, windows::core::PCSTR::null()) {
					let _ = ShowWindow(hwnd1, SW_HIDE);
				}
			};

			loop {
				// Check for device change
				if last_device_check.elapsed().as_secs() >= 2 {
					last_device_check = std::time::Instant::now();
					if let Some(ref enum_ref) = enumerator {
						if let Ok(new_device) = enum_ref.GetDefaultAudioEndpoint(eRender, eConsole) {
							if let Ok(id) = new_device.GetId() {
								let new_id = windows::core::PCWSTR::from_raw(id.0)
									.to_string()
									.unwrap_or_default();
								CoTaskMemFree(Some(id.0 as *const _));
								if new_id != current_device_id {
									current_device_id = new_id;
									device = Some(new_device);
									audio_endpoint_volume = device
										.as_ref()
										.and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok());
									// Reset last_volume to force an update event
									last_volume = -1.0;
								}
							}
						}
					}
				}

				if manager.is_none() {
					manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
						.and_then(|op| op.get())
						.ok();
				}
				while let Ok(cmd) = rx.try_recv() {
					if let Some(ref aev) = audio_endpoint_volume {
						match cmd {
							SystemCommand::VolumeMute => {
								if let Ok(muted) = aev.GetMute() {
									let _ = aev.SetMute(!muted.as_bool(), std::ptr::null());
									hide_osd();
								}
							}
							SystemCommand::VolumeUp => {
								if let (Ok(vol), Ok(muted)) = (aev.GetMasterVolumeLevelScalar(), aev.GetMute()) {
									let _ = aev.SetMasterVolumeLevelScalar((vol + 0.05).min(1.0), std::ptr::null());
									if muted.as_bool() {
										let _ = aev.SetMute(false, std::ptr::null());
									}
									hide_osd();
								}
							}
							SystemCommand::VolumeDown => {
								if let Ok(vol) = aev.GetMasterVolumeLevelScalar() {
									let _ = aev.SetMasterVolumeLevelScalar((vol - 0.05).max(0.0), std::ptr::null());
									hide_osd();
								}
							}
							SystemCommand::SetVolume(volume) => {
								let _ = aev.SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), std::ptr::null());
								if volume > 0.0 {
									let _ = aev.SetMute(false, std::ptr::null());
								}
								hide_osd();
							}
							SystemCommand::MediaPlayPause => {
								if let Some(ref mgr) = manager {
									if let Ok(session) = mgr.GetCurrentSession() {
										let _ = session.TryTogglePlayPauseAsync();
									}
								}
							}
							SystemCommand::MediaNext => {
								if let Some(ref mgr) = manager {
									if let Ok(session) = mgr.GetCurrentSession() {
										let _ = session.TrySkipNextAsync();
									}
								}
							}
							SystemCommand::MediaPrevious => {
								if let Some(ref mgr) = manager {
									if let Ok(session) = mgr.GetCurrentSession() {
										let _ = session.TrySkipPreviousAsync();
									}
								}
							}
							SystemCommand::MediaSeek(position_ms) => {
								if let Some(ref mgr) = manager {
									if let Ok(session) = mgr.GetCurrentSession() {
										let ticks = position_ms * 10_000;
										let _ = session.TryChangePlaybackPositionAsync(ticks);
									}
								}
							}
							SystemCommand::ToggleVisibility(visible) => {
								let _ = handle_system.emit("visibility-change", visible);
								if let Some(w) = handle_system.get_webview_window("bottom-corners") {
									if visible {
										let _ = w.show();
									} else {
										let _ = w.hide();
									}
								}
							}
							SystemCommand::BrightnessUp => {
								let new_val = (CURRENT_BRIGHTNESS.load(Ordering::Relaxed) + 10).min(100);
								CURRENT_BRIGHTNESS.store(new_val, Ordering::Relaxed);
								LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), Ordering::Relaxed);
								let _ = handle_system.emit(
									"brightness-change",
									BrightnessChangeEvent {
										brightness: new_val,
									},
								);
								if let Some(tx) = BRIGHTNESS_SENDER.get() {
									let _ = tx.send(new_val);
								}
								hide_osd();
							}
							SystemCommand::BrightnessDown => {
								let current = CURRENT_BRIGHTNESS.load(Ordering::Relaxed);
								let new_val = current.saturating_sub(10);
								CURRENT_BRIGHTNESS.store(new_val, Ordering::Relaxed);
								LAST_BRIGHTNESS_CHANGE.store(get_now_ms(), Ordering::Relaxed);
								let _ = handle_system.emit(
									"brightness-change",
									BrightnessChangeEvent {
										brightness: new_val,
									},
								);
								if let Some(tx) = BRIGHTNESS_SENDER.get() {
									let _ = tx.send(new_val);
								}
								hide_osd();
							}
						}
					}
				}
				if let Some(ref aev) = audio_endpoint_volume {
					if let (Ok(vol), Ok(muted)) = (aev.GetMasterVolumeLevelScalar(), aev.GetMute()) {
						let is_muted: bool = muted.into();
						if (vol - last_volume).abs() > 0.001 || is_muted != last_muted {
							last_volume = vol;
							last_muted = is_muted;
							crate::state::CURRENT_VOLUME.store((vol * 100.0) as u32, Ordering::Relaxed);
							let _ = handle_system.emit(
								"volume-change",
								VolumeChangeEvent {
									volume: vol,
									is_muted,
								},
							);
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
									let is_playing = session
										.GetPlaybackInfo()
										.ok()
										.and_then(|p| p.PlaybackStatus().ok())
										== Some(GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
									if let Ok(props) = session.TryGetMediaPropertiesAsync().and_then(|op| op.get()) {
										let title = props.Title().unwrap_or_default().to_string();
										if !title.is_empty() {
											let artist = props.Artist().unwrap_or_default().to_string();
											let mut artwork = None;
											let mut got_art_from_cache = false;
											if let Some((ref last_title, ref last_artist, _, _, ref last_art, _, _)) =
												last_emitted_info
											{
												if last_title == &title && last_artist == &artist {
													artwork = last_art.as_ref().map(|art| vec![art.clone()]);
													got_art_from_cache = true;
												}
											}

											if !got_art_from_cache {
												artwork = (|| -> Option<Vec<String>> {
													let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
													let content_type = stream
														.ContentType()
														.ok()?
														.to_string()
														.split(',')
														.next()
														.unwrap_or("image/jpeg")
														.trim()
														.to_string();
													let reader = DataReader::CreateDataReader(&stream).ok()?;
													let mut all_bytes = Vec::new();
													let chunk_size = 65536u32;
													loop {
														let loaded = reader.LoadAsync(chunk_size).ok()?.get().ok()?;
														if loaded == 0 {
															break;
														}
														let mut chunk = vec![0u8; loaded as usize];
														reader.ReadBytes(&mut chunk).ok()?;
														all_bytes.extend_from_slice(&chunk);
													}
													if all_bytes.is_empty() {
														return None;
													}
													Some(vec![format!(
														"data:{};base64,{}",
														content_type,
														general_purpose::STANDARD.encode(all_bytes)
													)])
												})();
											}

											// Extract timeline properties for progress bar
											let (position_ms, duration_ms, seek_enabled) = {
												let defaults = (0i64, 0i64, false);
												match session.GetTimelineProperties() {
													Ok(timeline) => {
														let start = timeline
															.StartTime()
															.map(|t| t.Duration / 10_000)
															.unwrap_or(0);
														let end = timeline.EndTime().map(|t| t.Duration / 10_000).unwrap_or(0);
														let pos = timeline
															.Position()
															.map(|t| t.Duration / 10_000)
															.unwrap_or(0);
														let dur = (end - start).max(0);
														// If duration is 0 but position > 0, some players don't report start/end
														// but still track position — use position as fallback duration indicator
														let effective_dur = if dur > 0 { dur } else { 0 };
														// Check if seeking is supported
														let seek = session
															.GetPlaybackInfo()
															.ok()
															.and_then(|pi| pi.Controls().ok())
															.map(|c| c.IsPlaybackPositionEnabled().unwrap_or(false))
															.unwrap_or(false);
														(pos, effective_dur, seek)
													}
													Err(_) => defaults,
												}
											};

											let now_ms = std::time::SystemTime::now()
												.duration_since(std::time::UNIX_EPOCH)
												.map(|d| d.as_millis() as u64)
												.unwrap_or(0);
											let info = MediaInfo {
												title,
												artist,
												is_playing,
												has_media: true,
												artwork,
												position_ms,
												duration_ms,
												seek_enabled,
												position_updated_at: now_ms,
											};
											if is_playing {
												best_info = Some(info);
												break;
											} else if best_info.is_none() {
												best_info = Some(info);
											}
										}
									}
								}
							}
						}
					}
					let current = best_info.unwrap_or(MediaInfo {
						title: "".into(),
						artist: "".into(),
						is_playing: false,
						has_media: false,
						artwork: None,
						position_ms: 0,
						duration_ms: 0,
						seek_enabled: false,
						position_updated_at: 0,
					});
					let art_str = current.artwork.as_ref().and_then(|a| a.first()).cloned();
					if last_emitted_info.as_ref().is_none_or(|(t, a, p, h, art, pos, _dur)| {
                        t != &current.title || a != &current.artist || p != &current.is_playing || h != &current.has_media || art != &art_str ||
                        // For position: emit if >1s difference (avoid spamming on every poll)
                        (*pos - current.position_ms).abs() > 1000
                    }) {
                        let _ = handle_system.emit("media-update", current.clone());
                        ANY_MEDIA_PLAYING.store(current.is_playing, Ordering::Relaxed);
                        last_emitted_info = Some((current.title, current.artist, current.is_playing, current.has_media, art_str, current.position_ms, current.duration_ms));
                    }
				}
				std::thread::sleep(std::time::Duration::from_millis(64));
			}
		}
	});

	let handle_brightness = app_handle.clone();
	std::thread::spawn(move || {
		let com_lib = match COMLibrary::new() {
			Ok(lib) => lib,
			Err(_) => return,
		};
		let wmi_con = match WMIConnection::with_namespace_path("root\\WMI", com_lib) {
			Ok(con) => con,
			Err(_) => return,
		};
		let mut last_brightness = match wmi_con.query::<WmiMonitorBrightness>() {
			Ok(res) => res
				.first()
				.map(|b| b.current_brightness as u32)
				.unwrap_or(50),
			Err(_) => 50,
		};
		CURRENT_BRIGHTNESS.store(last_brightness, Ordering::Relaxed);
		loop {
			if get_now_ms() - LAST_BRIGHTNESS_CHANGE.load(Ordering::Relaxed) < 2000 {
				std::thread::sleep(std::time::Duration::from_millis(500));
				continue;
			}
			if let Ok(results) = wmi_con.query::<WmiMonitorBrightness>() {
				if let Some(b) = results.first() {
					let brightness = b.current_brightness as u32;
					if brightness != last_brightness {
						last_brightness = brightness;
						CURRENT_BRIGHTNESS.store(brightness, Ordering::Relaxed);
						let _ =
							handle_brightness.emit("brightness-change", BrightnessChangeEvent { brightness });
					}
				}
			}
			std::thread::sleep(std::time::Duration::from_millis(1500));
		}
	});

	let tx_clone = tx.clone();
	let handle_visibility = app_handle.clone();
	std::thread::spawn(move || {
		use windows::Win32::Foundation::{POINT, RECT};
		use windows::Win32::Graphics::Gdi::ClientToScreen;
		use windows::Win32::UI::WindowsAndMessaging::{
			GetClientRect, GetForegroundWindow, GetWindowLongW, GetWindowRect, IsIconic, IsZoomed,
			GWL_STYLE, WS_CAPTION, WS_MAXIMIZE,
		};
		let mut last_visible = true;
		let mut last_dock_overlap: Option<bool> = None;
		let mut last_notch_overlap: Option<bool> = None;
		let mut last_dock_maximized: Option<bool> = None;
		let mut last_fg_maximized = false;
		let mut last_hwnd = HWND(std::ptr::null_mut());
		let mut last_emit = Instant::now();
		let mut is_known_shell = false;
		let my_process_id = std::process::id();
		let mut last_monitor_update = Instant::now() - Duration::from_secs(5);
		let mut cached_scale = 1.0f64;

		loop {
			unsafe {
				let now = Instant::now();
				if now.duration_since(last_monitor_update) > Duration::from_millis(1000) {
					if let Some(m) = handle_visibility.primary_monitor().ok().flatten() {
						cached_scale = m.scale_factor();
						last_monitor_update = now;
					}
				}
				use windows::Win32::Graphics::Gdi::{
					GetMonitorInfoA, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
				};
				let mut hwnd = GetForegroundWindow();

				// Find the first meaningful window for overlap detection.
				// We skip Bloom windows, invisible windows, minimized windows, and 'cloaked' system ghosts.
				let mut check_count = 0;
				while !hwnd.is_invalid() && check_count < 15 {
					let mut process_id = 0u32;
					GetWindowThreadProcessId(hwnd, Some(&mut process_id));

					let mut class_name = [0u8; 256];
					let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
					let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");

					let is_bloom = process_id == my_process_id || class_str.contains("Bloom");
					let is_visible = IsWindowVisible(hwnd).as_bool();
					let is_iconic = IsIconic(hwnd).as_bool();

					let mut cloaked = 0u32;
					let is_cloaked = DwmGetWindowAttribute(
						hwnd,
						windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
						&mut cloaked as *mut _ as *mut _,
						4,
					)
					.is_ok()
						&& cloaked != 0;

					let mut rect = RECT::default();
					let has_valid_rect = GetWindowRect(hwnd, &mut rect).is_ok()
						&& (rect.right - rect.left) > 0
						&& (rect.bottom - rect.top) > 0;

					if is_bloom || !is_visible || is_iconic || is_cloaked || !has_valid_rect {
						hwnd = windows::Win32::UI::WindowsAndMessaging::GetWindow(
							hwnd,
							windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT,
						)
						.unwrap_or_default();
						check_count += 1;
					} else {
						break;
					}
				}

				let mut should_overlap = false;
				let mut should_notch_overlap = false;
				let mut should_maximized = false;
				let mut current_is_fs = false;

				// Cheap check every tick: the foreground hwnd doesn't change when
				// the user clicks maximize, so without this the adaptive dock
				// would wait for the 3s fallback recompute to react.
				let fg_is_maximized = !hwnd.is_invalid() && IsZoomed(hwnd).as_bool();

				if !hwnd.is_invalid()
					&& (hwnd != last_hwnd
						|| fg_is_maximized != last_fg_maximized
						|| last_emit.elapsed() >= Duration::from_secs(3))
				{
					last_hwnd = hwnd;
					last_fg_maximized = fg_is_maximized;
					let mut class_name = [0u8; 256];
					let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
					let class_str = std::str::from_utf8(&class_name[..len as usize]).unwrap_or("");
					let mut text = [0u16; 512];
					let text_len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut text);
					let title = String::from_utf16_lossy(&text[..text_len as usize]);

					let is_desktop = class_str == "Progman" || class_str == "WorkerW";
					let is_start = (class_str == "Windows.UI.Core.CoreWindow" || class_str == "SimpleWindow")
						&& (title == "Start" || title == "Search");
					let is_shell =
						class_str == "Shell_TrayWnd" || class_str == "Shell_SecondaryTrayWnd" || is_start;

					is_known_shell = is_desktop || is_shell;
				}

				if !hwnd.is_invalid() {
					if is_known_shell {
						should_overlap = false;
						current_is_fs = false;
					} else {
						// Current hwnd is now guaranteed to be visible, non-iconic and non-cloaked
						use windows::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, WS_EX_TOOLWINDOW};
						let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
						let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;

						let is_transient = (ex_style & WS_EX_TOOLWINDOW.0) != 0;

						if !is_transient {
							let mut rect = RECT::default();
							let dwm_res = DwmGetWindowAttribute(
								hwnd,
								DWMWA_EXTENDED_FRAME_BOUNDS,
								&mut rect as *mut _ as *mut _,
								std::mem::size_of::<RECT>() as u32,
							);
							let has_rect = dwm_res.is_ok() || GetWindowRect(hwnd, &mut rect).is_ok();

							if has_rect {
								let h_monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
								let mut mi = MONITORINFO {
									cbSize: std::mem::size_of::<MONITORINFO>() as u32,
									..Default::default()
								};

								if GetMonitorInfoA(h_monitor, &mut mi).as_bool() {
									let screen_rect = mi.rcMonitor;
									let is_maximized = IsZoomed(hwnd).as_bool() || (style & WS_MAXIMIZE.0) != 0;
									let is_maximized_standard = is_maximized && (style & WS_CAPTION.0) != 0;

									let mut is_client_fullscreen = false;
									let mut client_rect = RECT::default();
									if GetClientRect(hwnd, &mut client_rect).is_ok() {
										let mut top_left = POINT {
											x: client_rect.left,
											y: client_rect.top,
										};
										let mut bottom_right = POINT {
											x: client_rect.right,
											y: client_rect.bottom,
										};
										let _ = ClientToScreen(hwnd, &mut top_left);
										let _ = ClientToScreen(hwnd, &mut bottom_right);

										is_client_fullscreen = top_left.x <= screen_rect.left
											&& top_left.y <= screen_rect.top
											&& bottom_right.x >= screen_rect.right
											&& bottom_right.y >= screen_rect.bottom;
									}

									let is_matches_screen = rect.left <= screen_rect.left
										&& rect.top <= screen_rect.top
										&& rect.right >= screen_rect.right
										&& rect.bottom >= screen_rect.bottom;

									// Truly fullscreen means client covers screen, OR window matches screen but is not just a standard maximized window
									current_is_fs =
										(is_client_fullscreen || is_matches_screen) && !is_maximized_standard;

									if current_is_fs || is_maximized {
										should_overlap = true;
										should_notch_overlap = true;
										// Standard maximized windows leave the dock's
										// reserved strip empty on both sides, so the dock
										// can stretch to a full taskbar. True fullscreen
										// windows cover the screen and should not.
										should_maximized = is_maximized && !current_is_fs;
									} else {
										should_overlap = false;
										if let Ok(dock_rect_lock) = DOCK_RECT.lock() {
											if let Some(dr) = *dock_rect_lock {
												let scale = cached_scale;
												let d_left = (dr.x as f64 * scale) as i32;
												let d_right = d_left + (dr.width as f64 * scale) as i32;
												let res_h = (56.0 * scale) as i32;
												let trigger_y = screen_rect.bottom - res_h;

												if rect.left < d_right - 4
													&& rect.right > d_left + 4
													&& rect.bottom > trigger_y + 4
												{
													should_overlap = true;
												}
											}
										}

										if let Ok(notch_rect_lock) = NOTCH_RECT.lock() {
											if let Some(nr) = *notch_rect_lock {
												let scale = cached_scale;
												let n_left = (nr.x as f64 * scale) as i32;
												let n_right = n_left + (nr.width as f64 * scale) as i32;
												let res_h = (36.0 * scale) as i32;
												let trigger_y = screen_rect.top + res_h;

												if rect.left < n_right - 4
													&& rect.right > n_left + 4
													&& rect.top < trigger_y - 4
												{
													should_notch_overlap = true;
												}
											}
										}
									}
								}
							}
						} else {
							should_overlap = false;
							current_is_fs = false;
						}
					}
				} else {
					last_hwnd = hwnd;
					should_overlap = false;
					current_is_fs = false;
					is_known_shell = false;
				}

				// Only report dock overlap when the dock window is actually visible.
				// On autostart, the overlap thread starts before init_dock shows the window,
				// and without this guard it emits dock-overlap:true which hides the dock
				// right after init_dock emits dock-overlap:false.
				let dock_visible = handle_visibility
					.get_webview_window("dock")
					.is_some_and(|w| w.is_visible().unwrap_or(false));
				let effective_dock_overlap = should_overlap && dock_visible;

				// Update overlap state
				CURRENT_DOCK_OVERLAP.store(
					if effective_dock_overlap { 1 } else { 0 },
					Ordering::Relaxed,
				);
				CURRENT_NOTCH_OVERLAP.store(if should_notch_overlap { 1 } else { 0 }, Ordering::Relaxed);
				CURRENT_FOREGROUND_FULLSCREEN.store(current_is_fs, Ordering::Relaxed);

				if Some(effective_dock_overlap) != last_dock_overlap
					|| last_emit.elapsed() >= Duration::from_secs(3)
				{
					let _ = handle_visibility.emit("dock-overlap", effective_dock_overlap);
					last_dock_overlap = Some(effective_dock_overlap);
					last_emit = Instant::now();
				}

				if Some(should_notch_overlap) != last_notch_overlap
					|| last_emit.elapsed() >= Duration::from_secs(3)
				{
					let _ = handle_visibility.emit("notch-overlap", should_notch_overlap);
					last_notch_overlap = Some(should_notch_overlap);
				}

				// Adaptive dock signal: a standard maximized foreground window.
				// Unlike dock-overlap this is not guarded by dock visibility, so
				// init_dock can emit the current value when the dock is enabled.
				CURRENT_FOREGROUND_MAXIMIZED.store(should_maximized, Ordering::Relaxed);
				if Some(should_maximized) != last_dock_maximized
					|| last_emit.elapsed() >= Duration::from_secs(3)
				{
					let _ = handle_visibility.emit("dock-maximized", should_maximized);
					last_dock_maximized = Some(should_maximized);
				}

				// Update full-screen visibility (hides TopBar/Corners)
				if current_is_fs && last_visible {
					let _ = tx_clone.send(SystemCommand::ToggleVisibility(false));
					last_visible = false;
				} else if !current_is_fs && !last_visible {
					let _ = tx_clone.send(SystemCommand::ToggleVisibility(true));
					last_visible = true;
				}

				// Enforce native taskbar hiding (periodic check)
				if NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed) {
					use windows::Win32::UI::WindowsAndMessaging::{FindWindowA, IsWindowVisible};
					let tray_class = windows::core::PCSTR(c"Shell_TrayWnd".as_ptr() as *const u8);
					let secondary_tray_class =
						windows::core::PCSTR(c"Shell_SecondaryTrayWnd".as_ptr() as *const u8);

					let mut should_rehide = false;
					if let Ok(tray_hwnd) = FindWindowA(tray_class, windows::core::PCSTR::null()) {
						if IsWindowVisible(tray_hwnd).as_bool() {
							should_rehide = true;
						}
					}
					if !should_rehide {
						if let Ok(secondary_tray_hwnd) =
							FindWindowA(secondary_tray_class, windows::core::PCSTR::null())
						{
							if IsWindowVisible(secondary_tray_hwnd).as_bool() {
								should_rehide = true;
							}
						}
					}

					if should_rehide {
						set_taskbar_visibility(false, false);
					}
				}
			}

			// Capture UI state is event-driven: WinEvent callbacks set the
			// recheck flag on top-level window show/hide/destroy and foreground
			// changes, so the scan below only runs when something changed
			// (throttled to coalesce bursts of window events).
			let scan_now = now_ms();
			if CAPTURE_RECHECK.load(Ordering::Relaxed)
				&& scan_now - CAPTURE_LAST_SCAN_MS.load(Ordering::Relaxed) >= 400
			{
				CAPTURE_RECHECK.store(false, Ordering::Relaxed);
				CAPTURE_LAST_SCAN_MS.store(scan_now, Ordering::Relaxed);
				let active = is_capture_ui_present();
				if active != CAPTURE_UI_ACTIVE.load(Ordering::Relaxed) {
					CAPTURE_UI_ACTIVE.store(active, Ordering::Relaxed);
					apply_capture_ui_state(&handle_visibility, active);
				}
			}
			std::thread::sleep(std::time::Duration::from_millis(150));
		}
	});

	tx
}

fn set_physical_monitors_brightness(brightness: u32) {
	unsafe {
		use windows::core::BOOL;
		use windows::Win32::Foundation::{LPARAM, RECT};
		use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};

		unsafe extern "system" fn monitor_enum_proc(
			hmonitor: HMONITOR,
			_: HDC,
			_: *mut RECT,
			lparam: LPARAM,
		) -> BOOL {
			let brightness = lparam.0 as u32;
			#[repr(C)]
			#[derive(Clone, Copy)]
			struct PHYSICAL_MONITOR {
				h_physical_monitor: usize,
				sz_physical_monitor_description: [u16; 128],
			}
			#[link(name = "dxva2")]
			extern "system" {
				fn GetNumberOfPhysicalMonitorsFromHMONITOR(
					hMonitor: HMONITOR,
					pdwNumberOfPhysicalMonitors: *mut u32,
				) -> BOOL;
				fn GetPhysicalMonitorsFromHMONITOR(
					hMonitor: HMONITOR,
					dwPhysicalMonitorArraySize: u32,
					pPhysicalMonitorArray: *mut PHYSICAL_MONITOR,
				) -> BOOL;
				fn DestroyPhysicalMonitors(
					dwPhysicalMonitorArraySize: u32,
					pPhysicalMonitorArray: *mut PHYSICAL_MONITOR,
				) -> BOOL;
				fn SetMonitorBrightness(hMonitor: usize, dwNewBrightness: u32) -> BOOL;
			}

			let mut count = 0u32;
			if GetNumberOfPhysicalMonitorsFromHMONITOR(hmonitor, &mut count).as_bool() && count > 0 {
				let mut monitors = vec![std::mem::zeroed::<PHYSICAL_MONITOR>(); count as usize];
				if GetPhysicalMonitorsFromHMONITOR(hmonitor, count, monitors.as_mut_ptr()).as_bool() {
					for mon in &monitors {
						if mon.h_physical_monitor != 0 {
							let _ = SetMonitorBrightness(mon.h_physical_monitor, brightness);
						}
					}
					let _ = DestroyPhysicalMonitors(count, monitors.as_mut_ptr());
				}
			}
			true.into()
		}

		let _ = EnumDisplayMonitors(
			None,
			None,
			Some(monitor_enum_proc),
			LPARAM(brightness as isize),
		);
	}
}

pub fn setup_brightness_worker() {
	let (tx, rx) = channel::<u32>();
	let _ = BRIGHTNESS_SENDER.set(tx);
	std::thread::spawn(move || unsafe {
		// Direct WMI COM + DXVA2 implementation (zero child processes spawned).
		use windows::Win32::System::Com::{
			CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
		};
		use windows::Win32::System::Variant::{VariantClear, VARENUM, VARIANT};
		use windows::Win32::System::Wmi::{
			IWbemClassObject, IWbemLocator, WbemLocator, WBEM_GENERIC_FLAG_TYPE,
		};

		let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

		let locator: IWbemLocator = match CoCreateInstance(&WbemLocator, None, CLSCTX_ALL) {
			Ok(l) => l,
			Err(_) => {
				let _ = CoUninitialize();
				return;
			}
		};
		let ns = windows::core::BSTR::from("root\\WMI");
		let empty_bstr = windows::core::BSTR::new();
		let services = match locator.ConnectServer(
			&ns,
			&empty_bstr,
			&empty_bstr,
			&empty_bstr,
			0,
			&empty_bstr,
			None,
		) {
			Ok(s) => s,
			Err(_) => {
				let _ = CoUninitialize();
				return;
			}
		};

		while let Ok(brightness) = rx.recv() {
			let brightness = brightness.min(100);
			// 1. Laptop internal panel via WMI WmiMonitorBrightnessMethods
			let wql = windows::core::BSTR::from("WQL");
			let q = windows::core::BSTR::from("SELECT * FROM WmiMonitorBrightnessMethods");
			if let Ok(enum_obj) = services.ExecQuery(&wql, &q, WBEM_GENERIC_FLAG_TYPE(0), None) {
				let mut row = [None::<IWbemClassObject>; 1];
				let mut returned = 0u32;
				while enum_obj.Next(-1i32, &mut row, &mut returned).is_ok() && returned > 0 {
					if let Some(obj) = row[0].take() {
						let mut var = VARIANT::default();
						if obj
							.Get(windows::core::w!("__RELPATH"), 0i32, &mut var, None, None)
							.is_ok()
						{
							let relpath_str = var.Anonymous.Anonymous.Anonymous.bstrVal.to_string();
							let _ = VariantClear(&mut var);
							if !relpath_str.is_empty() {
								let obj_path = windows::core::BSTR::from(relpath_str.as_str());
								let method_name = windows::core::BSTR::from("WmiSetBrightness");

								let mut in_cls: Option<IWbemClassObject> = None;
								if obj
									.GetMethod(
										windows::core::w!("WmiSetBrightness"),
										0i32,
										&mut in_cls,
										std::ptr::null_mut(),
									)
									.is_ok()
								{
									if let Some(in_cls) = in_cls {
										if let Ok(in_params) = in_cls.SpawnInstance(0i32) {
											let mut b_var = VARIANT::default();
											let b_anon = &mut b_var.Anonymous.Anonymous;
											b_anon.vt = VARENUM(17); // VT_UI1
											b_anon.Anonymous.bVal = brightness as u8;
											let _ = in_params.Put(windows::core::w!("Brightness"), 0i32, &b_var, 0);

											let mut t_var = VARIANT::default();
											let t_anon = &mut t_var.Anonymous.Anonymous;
											t_anon.vt = VARENUM(3); // VT_I4
											t_anon.Anonymous.lVal = 0i32;
											let _ = in_params.Put(windows::core::w!("Timeout"), 0i32, &t_var, 0);

											let _ = services.ExecMethod(
												&obj_path,
												&method_name,
												WBEM_GENERIC_FLAG_TYPE(0),
												None,
												Some(&in_params),
												None,
												None,
											);
										}
									}
								}
							}
						}
					}
				}
			}

			// 2. Desktop external monitor via Physical Monitor API (DXVA2 DDC/CI)
			set_physical_monitors_brightness(brightness);
		}
		let _ = CoUninitialize();
	});
}

static MOUSE_HOOK_APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static MH_LAST_MAIN_IGNORE: AtomicI32 = AtomicI32::new(-1);
static MH_LAST_DOCK_IGNORE: AtomicI32 = AtomicI32::new(-1);
static MH_LAST_OV_IGNORE: AtomicI32 = AtomicI32::new(-1);
static MH_LAST_EDGE_HOVER: AtomicI32 = AtomicI32::new(-1);
static MH_LAST_TOP_EDGE_HOVER: AtomicI32 = AtomicI32::new(-1);
static MH_LAST_LEFT_EDGE_HOVER: AtomicI32 = AtomicI32::new(-1);
static MH_LAST_RIGHT_EDGE_HOVER: AtomicI32 = AtomicI32::new(-1);
static MH_DOCK_EXPIRY_MS: AtomicI64 = AtomicI64::new(0);
static MH_TOPBAR_EXPIRY_MS: AtomicI64 = AtomicI64::new(0);
static MH_LEFT_EXPIRY_MS: AtomicI64 = AtomicI64::new(0);
static MH_RIGHT_EXPIRY_MS: AtomicI64 = AtomicI64::new(0);
static MH_LAST_MONITOR_UPDATE_MS: AtomicI64 = AtomicI64::new(0);
static MH_CACHED_MON_POS: Mutex<Option<(i32, i32)>> = Mutex::new(None);
static MH_CACHED_MON_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static MH_LAST_PROCESS_MS: AtomicI64 = AtomicI64::new(0);
static CAPTURE_UI_ACTIVE: AtomicBool = AtomicBool::new(false);
static CAPTURE_RECHECK: AtomicBool = AtomicBool::new(true);
static CAPTURE_LAST_SCAN_MS: AtomicI64 = AtomicI64::new(0);

fn now_ms() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis() as i64
}

/// True while a screen-capture UI (Windows Snipping Tool) has a visible window.
/// Bloom's notch sits exactly where that toolbar lives, so it must get out of
/// the way even if the capture window isn't recognised as fullscreen.
unsafe extern "system" fn capture_ui_enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
	use windows::Win32::UI::WindowsAndMessaging::IsIconic;
	let found = &mut *(lparam.0 as *mut bool);
	if *found {
		return BOOL(0);
	}
	if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
		return BOOL(1);
	}

	let mut pid = 0u32;
	GetWindowThreadProcessId(hwnd, Some(&mut pid));
	if pid != 0 && pid != std::process::id() {
		if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
			let mut buf = [0u16; 512];
			let mut len = buf.len() as u32;
			if QueryFullProcessImageNameW(
				handle,
				PROCESS_NAME_WIN32,
				windows::core::PWSTR(buf.as_mut_ptr()),
				&mut len,
			)
			.is_ok()
			{
				let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
				let name = path.rsplit('\\').next().unwrap_or("");
				if name == "snippingtool.exe"
					|| name == "screenclippinghost.exe"
					|| name == "screensketch.exe"
				{
					*found = true;
				}
			}
			let _ = CloseHandle(handle);
		}
	}

	if !*found {
		use windows::Win32::UI::WindowsAndMessaging::GetClassNameA;
		let mut class_buf = [0u8; 256];
		let len = GetClassNameA(hwnd, &mut class_buf);
		let class = std::str::from_utf8(&class_buf[..len as usize])
			.unwrap_or("")
			.to_lowercase();
		if class.contains("snipping") {
			*found = true;
		}
	}

	BOOL(1)
}

fn is_capture_ui_present() -> bool {
	use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
	let mut found = false;
	unsafe {
		let _ = EnumWindows(
			Some(capture_ui_enum_proc),
			LPARAM(&mut found as *mut bool as isize),
		);
	}
	found
}

fn apply_capture_ui_state(app: &AppHandle, active: bool) {
	if let Some(main_win) = app.get_webview_window("main") {
		if active {
			let _ = main_win.set_ignore_cursor_events(true);
			let _ = main_win.hide();
		} else {
			let _ = main_win.show();
			if MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
				register_appbar(main_win.clone());
			} else if let Ok(hwnd) = main_win.hwnd() {
				re_assert_topmost(hwnd);
			}
		}
	}
	if active {
		if let Some(dock_win) = app.get_webview_window("dock") {
			let _ = dock_win.set_ignore_cursor_events(true);
		}
		if let Some(ov_win) = app.get_webview_window("overlay") {
			let _ = ov_win.set_ignore_cursor_events(true);
		}
	}
	// Force the mouse hook to re-evaluate hit-testing on the next move.
	MH_LAST_MAIN_IGNORE.store(-1, Ordering::Relaxed);
	MH_LAST_DOCK_IGNORE.store(-1, Ordering::Relaxed);
	MH_LAST_OV_IGNORE.store(-1, Ordering::Relaxed);
}

pub fn setup_mouse_hook(app_handle: AppHandle) {
	let _ = MOUSE_HOOK_APP_HANDLE.set(app_handle);
	unsafe {
		SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), None, 0)
			.expect("Failed to install mouse hook");
	}
}

unsafe extern "system" fn mouse_hook_proc(
	code: i32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
	if code >= 0 && wparam.0 == WM_MOUSEMOVE as usize {
		// Throttle to ~30fps (32ms) to match old polling cadence.
		// Without this, state checks and set_ignore_cursor_events fire on
		// every pixel of cursor movement, causing notch flicker at edges.
		let now = now_ms();
		let last = MH_LAST_PROCESS_MS.load(Ordering::Relaxed);
		if now - last < 32 {
			return CallNextHookEx(None, code, wparam, lparam);
		}
		MH_LAST_PROCESS_MS.store(now, Ordering::Relaxed);

		if let Some(app_handle) = MOUSE_HOOK_APP_HANDLE.get() {
			let pt = &*(lparam.0 as *const MSLLHOOKSTRUCT);
			let cursor = pt.pt;

			// While a capture UI (Snipping Tool) is up, Bloom is fully
			// click-through and skipped entirely so the tool owns the screen.
			if CAPTURE_UI_ACTIVE.load(Ordering::Relaxed) {
				if MH_LAST_MAIN_IGNORE.load(Ordering::Relaxed) != 1 {
					if let Some(w) = app_handle.get_webview_window("main") {
						let _ = w.set_ignore_cursor_events(true);
					}
					MH_LAST_MAIN_IGNORE.store(1, Ordering::Relaxed);
				}
				if MH_LAST_DOCK_IGNORE.load(Ordering::Relaxed) != 1 {
					if let Some(w) = app_handle.get_webview_window("dock") {
						let _ = w.set_ignore_cursor_events(true);
					}
					MH_LAST_DOCK_IGNORE.store(1, Ordering::Relaxed);
				}
				if MH_LAST_OV_IGNORE.load(Ordering::Relaxed) != 1 {
					if let Some(w) = app_handle.get_webview_window("overlay") {
						let _ = w.set_ignore_cursor_events(true);
					}
					MH_LAST_OV_IGNORE.store(1, Ordering::Relaxed);
				}
				if MH_LAST_EDGE_HOVER.swap(0, Ordering::Relaxed) != 0 {
					let _ = app_handle.emit("dock-edge-hover", false);
				}
				if MH_LAST_TOP_EDGE_HOVER.swap(0, Ordering::Relaxed) != 0 {
					let _ = app_handle.emit("notch-edge-hover", false);
				}
				return CallNextHookEx(None, code, wparam, lparam);
			}

			// Refresh cached monitor info every 1s
			if now - MH_LAST_MONITOR_UPDATE_MS.load(Ordering::Relaxed) > 1000 {
				if let Ok(Some(monitor)) = app_handle.primary_monitor() {
					let pos = *monitor.position();
					let size = *monitor.size();
					*MH_CACHED_MON_POS.lock().unwrap() = Some((pos.x, pos.y));
					*MH_CACHED_MON_SIZE.lock().unwrap() = Some((size.width, size.height));
					MH_LAST_MONITOR_UPDATE_MS.store(now, Ordering::Relaxed);
				}
			}

			let cached_pos = MH_CACHED_MON_POS.lock().unwrap().unwrap_or((0, 0));
			let cached_size = MH_CACHED_MON_SIZE.lock().unwrap().unwrap_or((1920, 1080));
			let mon_x = cached_pos.0;
			let mon_y = cached_pos.1;
			let mon_w = cached_size.0 as i32;
			let mon_h = cached_size.1 as i32;

			let fg_fs = CURRENT_FOREGROUND_FULLSCREEN.load(Ordering::Relaxed);

			// --- Dock Interaction ---
			if fg_fs {
				// Fullscreen foreground app or capture overlay: the dock must not
				// intercept input (e.g. a Snipping Tool selection ending at the
				// bottom edge), so keep it click-through.
				if MH_LAST_DOCK_IGNORE.load(Ordering::Relaxed) != 1 {
					if let Some(dock_win) = app_handle.get_webview_window("dock") {
						let _ = dock_win.set_ignore_cursor_events(true);
					}
					MH_LAST_DOCK_IGNORE.store(1, Ordering::Relaxed);
				}
				if MH_LAST_EDGE_HOVER.swap(0, Ordering::Relaxed) != 0 {
					let _ = app_handle.emit("dock-edge-hover", false);
				}
			} else if let Some(dock_win) = app_handle.get_webview_window("dock") {
				if dock_win.is_visible().unwrap_or(false) {
					let mut is_click_interactive = false;
					let mut is_hovered = false;
					let mut dock_span: Option<(i32, i32)> = None;

					let dock_rect_val = DOCK_WINDOW_RECT.lock().ok().and_then(|g| *g);

					if let Some((win_pos, win_size)) = dock_rect_val {
						let in_window = cursor.x >= win_pos.x
							&& cursor.x <= (win_pos.x + win_size.width as i32)
							&& cursor.y >= win_pos.y
							&& cursor.y <= (win_pos.y + win_size.height as i32);

						if in_window {
							if let Ok(region) = DOCK_RECT.try_lock() {
								if let Some(r) = *region {
									let scale = dock_win.scale_factor().unwrap_or(1.0);
									let pad_x = (5.0 * scale) as i32;
									let pad_y_top = (8.0 * scale) as i32;
									let pad_y_bottom = (5.0 * scale) as i32;
									// Hysteresis keeps the dock interactive a little past
									// its bounds once grabbed, so removing the edge-forced
									// interactivity doesn't reintroduce boundary flicker.
									let hyst = if MH_LAST_DOCK_IGNORE.load(Ordering::Relaxed) == 0 {
										(10.0 * scale) as i32
									} else {
										0
									};
									let rx = win_pos.x + (r.x as f64 * scale) as i32 - pad_x - hyst;
									let ry = win_pos.y + (r.y as f64 * scale) as i32 - pad_y_top - hyst;
									let rw = (r.width as f64 * scale) as i32 + (pad_x * 2) + (hyst * 2);
									let rh = (r.height as f64 * scale) as i32 + pad_y_top + pad_y_bottom + (hyst * 2);
									if cursor.x >= rx
										&& cursor.x <= (rx + rw)
										&& cursor.y >= ry
										&& cursor.y <= (ry + rh)
									{
										is_click_interactive = true;
									}
									dock_span = Some((rx, rx + rw));
								}
							}

							if !is_click_interactive && MENU_IS_OPEN.load(Ordering::Relaxed) {
								if let Ok(rect) = MENU_RECT.try_lock() {
									if let Some(r) = *rect {
										let scale = dock_win.scale_factor().unwrap_or(1.0);
										let rx = win_pos.x + (r.x as f64 * scale) as i32 - (5.0 * scale) as i32;
										let ry = win_pos.y + (r.y as f64 * scale) as i32 - (5.0 * scale) as i32;
										let rw = (r.width as f64 * scale) as i32 + (10.0 * scale) as i32;
										let rh = (r.height as f64 * scale) as i32 + (10.0 * scale) as i32;
										if cursor.x >= rx
											&& cursor.x <= (rx + rw)
											&& cursor.y >= ry
											&& cursor.y <= (ry + rh)
										{
											is_click_interactive = true;
										}
									}
								}
							}
						}
					}

					// Hot-edge detection (Bottom edge)
					let in_dock_hover = DOCK_IS_HOVERED.load(Ordering::Relaxed);
					let scale = dock_win.scale_factor().unwrap_or(1.0);
					let at_bottom_edge = cursor.y >= (mon_y + mon_h - (8.0 * scale) as i32)
						&& cursor.x >= mon_x
						&& cursor.x <= (mon_x + mon_w);

					if at_bottom_edge || in_dock_hover {
						is_hovered = true;
						MH_DOCK_EXPIRY_MS.store(now + 500, Ordering::Relaxed);
					}

					// Approaching along the bottom edge keeps the dock interactive
					// while near its horizontal span, so the reveal can't be clicked
					// through mid-animation. The corners stay click-through.
					if at_bottom_edge {
						if let Some((span_left, span_right)) = dock_span {
							let edge_pad = (60.0 * scale) as i32;
							if cursor.x >= span_left - edge_pad && cursor.x <= span_right + edge_pad {
								is_click_interactive = true;
							}
						}
					}

					let final_dock_hover = is_hovered || now < MH_DOCK_EXPIRY_MS.load(Ordering::Relaxed);
					let prev = MH_LAST_EDGE_HOVER.load(Ordering::Relaxed);
					let new_val = if final_dock_hover { 1 } else { 0 };
					if prev != new_val {
						let _ = app_handle.emit("dock-edge-hover", final_dock_hover);
						MH_LAST_EDGE_HOVER.store(new_val, Ordering::Relaxed);
					}

					let should_ignore = !is_click_interactive && !MENU_IS_OPEN.load(Ordering::Relaxed);
					let prev_ignore = MH_LAST_DOCK_IGNORE.load(Ordering::Relaxed);
					let new_ignore = if should_ignore { 1 } else { 0 };
					if prev_ignore != new_ignore {
						if let Ok(hwnd) = dock_win.hwnd() {
							re_assert_topmost(hwnd);
						}
						let _ = dock_win.set_ignore_cursor_events(should_ignore);
						MH_LAST_DOCK_IGNORE.store(new_ignore, Ordering::Relaxed);
					}
				}
			}

			// --- Main (TopBar) Interaction ---
			if !fg_fs {
				if let Some(main_win) = app_handle.get_webview_window("main") {
					if main_win.is_visible().unwrap_or(false) {
						let in_notch_hover = NOTCH_IS_HOVERED.load(Ordering::Relaxed);
						let mut is_notch_hovered = false;
						let scale = main_win.scale_factor().unwrap_or(1.0);
						let at_top_edge = cursor.y <= (mon_y + (8.0 * scale) as i32)
							&& cursor.x >= mon_x
							&& cursor.x <= (mon_x + mon_w);

						if at_top_edge || in_notch_hover {
							is_notch_hovered = true;
							MH_TOPBAR_EXPIRY_MS.store(now + 500, Ordering::Relaxed);
						}

						let mut is_click_interactive = false;
						let main_rect_val = MAIN_WINDOW_RECT.lock().ok().and_then(|g| *g);

						if let Some((win_pos, _)) = main_rect_val {
							if let Ok(region) = NOTCH_RECT.try_lock() {
								if let Some(r) = *region {
									let scale = main_win.scale_factor().unwrap_or(1.0);
									let pad_x = (20.0 * scale) as i32;
									let pad_y_bottom = (5.0 * scale) as i32;
									// Hysteresis keeps the notch interactive a little past
									// its bounds once grabbed, so removing the edge-forced
									// interactivity doesn't reintroduce boundary flicker.
									let hyst = if MH_LAST_MAIN_IGNORE.load(Ordering::Relaxed) == 0 {
										(10.0 * scale) as i32
									} else {
										0
									};
									let rx = win_pos.x + (r.x as f64 * scale) as i32 - pad_x - hyst;
									let rw = (r.width as f64 * scale) as i32 + (pad_x * 2) + (hyst * 2);
									let ry_top = win_pos.y;
									let ry_bottom =
										win_pos.y + (r.height as f64 * scale) as i32 + pad_y_bottom + hyst;

									if cursor.x >= rx
										&& cursor.x <= (rx + rw)
										&& cursor.y >= ry_top
										&& cursor.y <= ry_bottom
									{
										is_click_interactive = true;
									}

									// Approaching along the top edge keeps the window
									// interactive while near the notch's horizontal
									// span, so peek/hover can't flicker at the
									// boundary. The screen corners stay click-through.
									let edge_pad = (60.0 * scale) as i32;
									if at_top_edge && cursor.x >= rx - edge_pad && cursor.x <= rx + rw + edge_pad {
										is_click_interactive = true;
									}
								}
							}
						}

						let final_notch_hover =
							is_notch_hovered || now < MH_TOPBAR_EXPIRY_MS.load(Ordering::Relaxed);
						let prev = MH_LAST_TOP_EDGE_HOVER.load(Ordering::Relaxed);
						let new_val = if final_notch_hover { 1 } else { 0 };
						if prev != new_val {
							let _ = app_handle.emit("notch-edge-hover", final_notch_hover);
							MH_LAST_TOP_EDGE_HOVER.store(new_val, Ordering::Relaxed);
						}

						let final_ignore = !is_click_interactive && !MENU_IS_OPEN.load(Ordering::Relaxed);
						let prev_ignore = MH_LAST_MAIN_IGNORE.load(Ordering::Relaxed);
						let new_ignore = if final_ignore { 1 } else { 0 };
						if prev_ignore != new_ignore {
							if let Ok(hwnd) = main_win.hwnd() {
								re_assert_topmost(hwnd);
							}
							let _ = main_win.set_ignore_cursor_events(final_ignore);
							MH_LAST_MAIN_IGNORE.store(new_ignore, Ordering::Relaxed);
						}
					}
				}
			} else {
				if let Some(main_win) = app_handle.get_webview_window("main") {
					let prev = MH_LAST_MAIN_IGNORE.load(Ordering::Relaxed);
					if prev != 1 {
						if let Ok(hwnd) = main_win.hwnd() {
							re_assert_topmost(hwnd);
						}
						let _ = main_win.set_ignore_cursor_events(true);
						MH_LAST_MAIN_IGNORE.store(1, Ordering::Relaxed);
					}
				}
				let prev = MH_LAST_TOP_EDGE_HOVER.load(Ordering::Relaxed);
				if prev != 0 {
					let _ = app_handle.emit("notch-edge-hover", false);
					MH_LAST_TOP_EDGE_HOVER.store(0, Ordering::Relaxed);
				}
			}

			// --- Left Edge (Volume) ---
			if !fg_fs {
				let at_left_edge =
					cursor.x <= (mon_x + 8) && cursor.y >= mon_y && cursor.y <= (mon_y + mon_h);

				if at_left_edge {
					MH_LEFT_EXPIRY_MS.store(now + 500, Ordering::Relaxed);
				}

				let final_left_hover = now < MH_LEFT_EXPIRY_MS.load(Ordering::Relaxed);
				let prev = MH_LAST_LEFT_EDGE_HOVER.load(Ordering::Relaxed);
				let new_val = if final_left_hover { 1 } else { 0 };
				if prev != new_val {
					let _ = app_handle.emit("volume-edge-hover", final_left_hover);
					MH_LAST_LEFT_EDGE_HOVER.store(new_val, Ordering::Relaxed);
				}
			} else {
				let prev = MH_LAST_LEFT_EDGE_HOVER.load(Ordering::Relaxed);
				if prev != 0 {
					let _ = app_handle.emit("volume-edge-hover", false);
					MH_LAST_LEFT_EDGE_HOVER.store(0, Ordering::Relaxed);
				}
			}

			// --- Right Edge (Brightness) ---
			if !fg_fs {
				let at_right_edge =
					cursor.x >= (mon_x + mon_w - 8) && cursor.y >= mon_y && cursor.y <= (mon_y + mon_h);

				if at_right_edge {
					MH_RIGHT_EXPIRY_MS.store(now + 500, Ordering::Relaxed);
				}

				let final_right_hover = now < MH_RIGHT_EXPIRY_MS.load(Ordering::Relaxed);
				let prev = MH_LAST_RIGHT_EDGE_HOVER.load(Ordering::Relaxed);
				let new_val = if final_right_hover { 1 } else { 0 };
				if prev != new_val {
					let _ = app_handle.emit("brightness-edge-hover", final_right_hover);
					MH_LAST_RIGHT_EDGE_HOVER.store(new_val, Ordering::Relaxed);
				}
			} else {
				let prev = MH_LAST_RIGHT_EDGE_HOVER.load(Ordering::Relaxed);
				if prev != 0 {
					let _ = app_handle.emit("brightness-edge-hover", false);
					MH_LAST_RIGHT_EDGE_HOVER.store(0, Ordering::Relaxed);
				}
			}

			// --- Overlay cursor passthrough ---
			if let Some(ov_win) = app_handle.get_webview_window("overlay") {
				if fg_fs {
					let prev = MH_LAST_OV_IGNORE.load(Ordering::Relaxed);
					if prev != 1 {
						if let Ok(hwnd) = ov_win.hwnd() {
							re_assert_topmost(hwnd);
						}
						let _ = ov_win.set_ignore_cursor_events(true);
						MH_LAST_OV_IGNORE.store(1, Ordering::Relaxed);
					}
				} else {
					let over_left = if let Ok(Some(m)) = ov_win.primary_monitor() {
						let ms = m.size();
						let mp = m.position();
						let sc = m.scale_factor();
						let nw = (42.0 * sc) as i32;
						let nh = (196.0 * sc) as i32;
						let nx = mp.x;
						let ny = mp.y + (ms.height as i32 / 2) - (nh / 2);
						cursor.x >= nx && cursor.x <= nx + nw && cursor.y >= ny && cursor.y <= ny + nh
					} else {
						false
					};

					let over_right = if let Ok(Some(m)) = ov_win.primary_monitor() {
						let ms = m.size();
						let mp = m.position();
						let sc = m.scale_factor();
						let nw = (42.0 * sc) as i32;
						let nh = (196.0 * sc) as i32;
						let nx = mp.x + ms.width as i32 - nw;
						let ny = mp.y + (ms.height as i32 / 2) - (nh / 2);
						cursor.x >= nx && cursor.x <= nx + nw && cursor.y >= ny && cursor.y <= ny + nh
					} else {
						false
					};

					let should_ignore = !(over_left || over_right);
					let prev = MH_LAST_OV_IGNORE.load(Ordering::Relaxed);
					let new_val = if should_ignore { 1 } else { 0 };
					if prev != new_val {
						if let Ok(hwnd) = ov_win.hwnd() {
							re_assert_topmost(hwnd);
						}
						let _ = ov_win.set_ignore_cursor_events(should_ignore);
						MH_LAST_OV_IGNORE.store(new_val, Ordering::Relaxed);
					}
				}
			}
		}
	}
	CallNextHookEx(None, code, wparam, lparam)
}

pub fn trigger_app_scan() {
	if IS_SCANNING.load(Ordering::Relaxed) {
		return;
	}
	IS_SCANNING.store(true, Ordering::Relaxed);

	std::thread::spawn(|| {
		use windows::Win32::System::Com::{
			CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
		};
		use windows::Win32::UI::Shell::{
			FOLDERID_AppsFolder, IEnumIDList, ILCombine, ILFree, IShellFolder, SHGetDesktopFolder,
			SHGetKnownFolderIDList, SHGetNameFromIDList, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_FILESYSPATH,
			SIGDN_NORMALDISPLAY, SIGDN_URL,
		};
		let mut apps = Vec::new();
		unsafe {
			let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
			// Use a scope to ensure COM objects are dropped before CoUninitialize
			{
				if let Ok(pidl_apps) = SHGetKnownFolderIDList(&FOLDERID_AppsFolder, 0, None) {
					if let Ok(desktop) = SHGetDesktopFolder() {
						if let Ok(apps_folder) = desktop.BindToObject::<_, IShellFolder>(pidl_apps, None) {
							let mut enum_id: Option<IEnumIDList> = None;
							let res = apps_folder.EnumObjects(
								HWND(std::ptr::null_mut()),
								(windows::Win32::UI::Shell::SHCONTF_FOLDERS.0
									| windows::Win32::UI::Shell::SHCONTF_NONFOLDERS.0) as u32,
								&mut enum_id,
							);

							if res.is_ok() {
								if let Some(enum_id) = enum_id {
									// Must be a real array the enumerator can write into;
									// `&mut [pidl_item]` would write into a temporary copy.
									let mut pidl_buf: [*mut windows::Win32::UI::Shell::Common::ITEMIDLIST; 1] =
										[std::ptr::null_mut()];
									let mut fetched = 0;
									while enum_id.Next(&mut pidl_buf, Some(&mut fetched)).is_ok() && fetched > 0 {
										let pidl_item = pidl_buf[0];
										pidl_buf[0] = std::ptr::null_mut();
										if pidl_item.is_null() {
											continue;
										}

										// Child PIDLs from EnumObjects are relative; SHGetNameFromIDList
										// needs an absolute PIDL, otherwise every call fails with E_INVALIDARG.
										let absolute_pidl =
											ILCombine(Some(pidl_apps as *const _), Some(pidl_item as *const _));
										if absolute_pidl.is_null() {
											CoTaskMemFree(Some(pidl_item as *const _));
											continue;
										}

										let name =
											if let Ok(n_ptr) = SHGetNameFromIDList(absolute_pidl, SIGDN_NORMALDISPLAY) {
												let s = String::from_utf16_lossy(windows::core::PCWSTR(n_ptr.0).as_wide());
												CoTaskMemFree(Some(n_ptr.0 as *const _));
												s
											} else {
												"Unknown".to_string()
											};

										// Parsing name: a full exe path for Win32 apps, an
										// AppUserModelID for packaged apps (Store/UWP/PWAs).
										let path = if let Ok(p_ptr) =
											SHGetNameFromIDList(absolute_pidl, SIGDN_DESKTOPABSOLUTEPARSING)
										{
											let s = String::from_utf16_lossy(windows::core::PCWSTR(p_ptr.0).as_wide());
											CoTaskMemFree(Some(p_ptr.0 as *const _));
											s
										} else if let Ok(p_ptr) = SHGetNameFromIDList(absolute_pidl, SIGDN_FILESYSPATH)
										{
											let s = String::from_utf16_lossy(windows::core::PCWSTR(p_ptr.0).as_wide());
											CoTaskMemFree(Some(p_ptr.0 as *const _));
											s
										} else if let Ok(p_ptr) = SHGetNameFromIDList(absolute_pidl, SIGDN_URL) {
											let s = String::from_utf16_lossy(windows::core::PCWSTR(p_ptr.0).as_wide());
											CoTaskMemFree(Some(p_ptr.0 as *const _));
											s
										} else {
											name.clone()
										};

										if is_launchable_entry(&name, &path) {
											// AUMIDs have no executable of their own; storing the
											// whole id here would make exe-name matching think any
											// browser window belongs to this app.
											let executable = if path.contains('\\') || path.contains('/') {
												std::path::Path::new(&path)
													.file_name()
													.and_then(|n| n.to_str())
													.map(|s| s.to_string())
											} else {
												None
											};
											apps.push(AppInfo {
												name,
												path,
												icon: None,
												is_running: false,
												hwnd: None,
												executable,
												all_hwnds: None,
											});
										}
										ILFree(Some(absolute_pidl as *const _));
										CoTaskMemFree(Some(pidl_item as *const _));
									}
								}
							}
						}
					}
					CoTaskMemFree(Some(pidl_apps as *const _));
				}
			} // Close COM scope
			CoUninitialize();
		}

		// Scan Start Menu .lnk shortcuts (depth-limited, no .exe scanning)
		// This catches Win32 apps that FOLDERID_AppsFolder may miss
		let mut start_menu_dirs: Vec<String> = Vec::new();
		if let Ok(programdata) = std::env::var("PROGRAMDATA") {
			start_menu_dirs.push(format!(
				r"{}\Microsoft\Windows\Start Menu\Programs",
				programdata
			));
		}
		if let Ok(appdata) = std::env::var("APPDATA") {
			start_menu_dirs.push(format!(
				r"{}\Microsoft\Windows\Start Menu\Programs",
				appdata
			));
		}

		// Shortcut resolution uses IShellLinkW, which needs COM on this thread.
		unsafe {
			let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
		}
		for dir in &start_menu_dirs {
			let root = std::path::Path::new(dir);
			if root.exists() {
				collect_shortcuts(root, &mut apps, 0);
			}
		}
		unsafe {
			CoUninitialize();
		}

		if let Some(c) = INSTALLED_APPS_CACHE.get() {
			if let Ok(mut lock) = c.lock() {
				*lock = apps;
			}
		}
		IS_SCANNING.store(false, Ordering::Relaxed);
	});
}

/// Filters out shell entries that are not real launchable apps (web links,
/// documents, protocol handlers) so the add-app list stays clean.
fn is_launchable_entry(name: &str, path: &str) -> bool {
	if name.is_empty() || name == "Unknown" || name.to_lowercase().contains("uninstall") {
		return false;
	}
	let lower = path.to_lowercase();
	if lower.is_empty() {
		return false;
	}
	if lower.starts_with("http://")
		|| lower.starts_with("https://")
		|| lower.starts_with("file:")
		|| lower.starts_with("steam:")
		|| lower.starts_with("::{")
		|| (lower.starts_with("shell:") && !lower.starts_with("shell:appsfolder"))
	{
		return false;
	}
	true
}

fn collect_shortcuts(dir: &std::path::Path, apps: &mut Vec<AppInfo>, depth: i32) {
	if depth > 3 {
		return;
	}
	if let Ok(entries) = std::fs::read_dir(dir) {
		for entry in entries.flatten() {
			let path = entry.path();
			if path.is_dir() {
				collect_shortcuts(&path, apps, depth + 1);
			} else if path
				.extension()
				.is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
			{
				let name = path.file_stem().unwrap().to_string_lossy().to_string();
				if name.to_lowercase().contains("uninstall") || name.starts_with("Install") {
					continue;
				}

				// Keep the .lnk itself: its args identify PWAs and arguments must be
				// passed through when launching. The resolved target is stored as the
				// executable name so pinned entries still match running windows.
				let path_str = path.to_string_lossy().to_string();
				let executable = crate::utils::resolve_shortcut(&path_str).and_then(|(target, _args)| {
					let file = std::path::Path::new(&target)
						.file_name()
						.and_then(|n| n.to_str())?
						.to_string();
					// UWP shortcuts launch explorer.exe with a shell:AppsFolder argument;
					// storing that would wrongly match File Explorer windows.
					if file.eq_ignore_ascii_case("explorer.exe") {
						None
					} else {
						Some(file)
					}
				});

				if !apps.iter().any(|a| a.name == name || a.path == path_str) {
					apps.push(AppInfo {
						name,
						path: path_str,
						icon: None,
						is_running: false,
						hwnd: None,
						executable,
						all_hwnds: None,
					});
				}
			}
		}
	}
}

pub fn sync_overlays(app: &AppHandle) {
	// Don't reposition while splash is playing
	if crate::state::OVERLAY_IN_SPLASH.load(Ordering::Relaxed) {
		return;
	}
	// Full-screen overlay — notches render at left/right edges via CSS
	// The window covers the entire primary monitor so it never needs repositioning
	if let Some(ov_win) = app.get_webview_window("overlay") {
		if let Ok(Some(monitor)) = ov_win.primary_monitor() {
			let size = monitor.size();
			let pos = monitor.position();
			let _ = ov_win.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
			let _ = ov_win.set_size(tauri::PhysicalSize::new(size.width, size.height));
		}
		if let Ok(hwnd) = ov_win.hwnd() {
			re_assert_topmost(hwnd);
		}
	}
}

pub fn register_appbar(window: tauri::WebviewWindow) {
	if let Ok(Some(monitor)) = window.app_handle().primary_monitor() {
		let m_size = monitor.size();
		let m_pos = monitor.position();
		let hwnd = window.hwnd().unwrap();
		let scale = monitor.scale_factor();
		let bloom_scale = crate::utils::get_bloom_scale(window.app_handle());
		let ph = ((420.0 * bloom_scale) * scale) as i32;
		let pr = ((40.0 * bloom_scale) * scale) as i32; // Scale the reserved top screen space

		unsafe {
			use windows::Win32::Foundation::RECT;
			use windows::Win32::UI::Shell::{
				SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_SETPOS, APPBARDATA,
			};
			use windows::Win32::UI::WindowsAndMessaging::{
				GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
				SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, WS_EX_NOACTIVATE as WS_EX_NA,
				WS_EX_TOOLWINDOW,
			};

			// Set styles first
			let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
			ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
			let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);

			let mut abd = APPBARDATA {
				cbSize: std::mem::size_of::<APPBARDATA>() as u32,
				hWnd: hwnd,
				..Default::default()
			};

			if !MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
				SHAppBarMessage(ABM_NEW, &mut abd);
				MAIN_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
			}

			abd.uEdge = ABE_TOP;
			abd.rc = RECT {
				left: m_pos.x,
				top: m_pos.y,
				right: m_pos.x + m_size.width as i32,
				bottom: m_pos.y + pr,
			};

			SHAppBarMessage(ABM_QUERYPOS, &mut abd);
			SHAppBarMessage(ABM_SETPOS, &mut abd);

			// Use the shell-approved rect for the final position, but keep our ph height for the window
			let final_width = abd.rc.right - abd.rc.left;

			let mut current_rect = RECT::default();
			let mut already_positioned = false;
			if GetWindowRect(hwnd, &mut current_rect).is_ok() {
				let current_width = current_rect.right - current_rect.left;
				let current_height = current_rect.bottom - current_rect.top;
				if current_rect.left == abd.rc.left
					&& current_rect.top == abd.rc.top
					&& current_width == final_width
					&& current_height == ph
				{
					already_positioned = true;
				}
			}

			if !already_positioned {
				let _ = SetWindowPos(
					hwnd,
					None,
					abd.rc.left,
					abd.rc.top,
					final_width,
					ph,
					SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
				);
			}

			// Re-assert topmost after repositioning — use re_assert_topmost instead of
			// set_always_on_top(true) to include SWP_NOACTIVATE and re-stamp WS_EX_NOACTIVATE.
			// This prevents WM_ACTIVATE from reaching WebView2, which caused bloom windows
			// to blank/hide when other windows were minimized or closed.
			re_assert_topmost(hwnd);

			if !window.is_visible().unwrap_or(false) {
				let _ = window.show();
			}
		}
	} else {
		let w = window.clone();
		tauri::async_runtime::spawn(async move {
			for _ in 0..10 {
				tokio::time::sleep(std::time::Duration::from_millis(500)).await;
				if let Ok(Some(_monitor)) = w.app_handle().primary_monitor() {
					register_appbar(w);
					break;
				}
			}
		});
	}
}

pub fn register_dock_appbar(window: tauri::WebviewWindow) {
	register_dock_appbar_inner(window, 0);
}

fn register_dock_appbar_inner(window: tauri::WebviewWindow, attempt: i32) {
	if let Ok(Some(monitor)) = window.app_handle().primary_monitor() {
		let m_size = monitor.size();
		let m_pos = monitor.position();
		let hwnd = window.hwnd().unwrap();
		let scale = monitor.scale_factor();
		let bloom_scale = crate::utils::get_bloom_scale(window.app_handle());

		// ph = full physical window height. outer_size() can return 0 before the
		// window has rendered. Never guess a value — bail and let the retry wrapper handle it.
		let ph = window.outer_size().map(|s| s.height as i32).unwrap_or(0);
		if ph <= 0 {
			if attempt < 10 {
				let w = window.clone();
				tauri::async_runtime::spawn(async move {
					tokio::time::sleep(std::time::Duration::from_millis(200)).await;
					register_dock_appbar_inner(w, attempt + 1);
				});
			}
			return;
		}

		let pr = ((56.0 * bloom_scale) * scale) as i32;

		unsafe {
			use windows::Win32::Foundation::RECT;
			use windows::Win32::UI::Shell::{
				SHAppBarMessage, ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_SETPOS, APPBARDATA,
			};
			use windows::Win32::UI::WindowsAndMessaging::{
				GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE,
				SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER, WS_EX_NOACTIVATE as WS_EX_NA,
				WS_EX_TOOLWINDOW,
			};

			// Set extended styles (ToolWindow and NoActivate)
			let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as usize;
			ex_style |= (WS_EX_TOOLWINDOW.0 | WS_EX_NA.0) as usize;
			let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);

			// Hide native taskbar first to free up space
			set_taskbar_visibility(false, false);

			let mut abd = APPBARDATA {
				cbSize: std::mem::size_of::<APPBARDATA>() as u32,
				hWnd: hwnd,
				..Default::default()
			};

			if !DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) {
				SHAppBarMessage(ABM_NEW, &mut abd);
				DOCK_APPBAR_REGISTERED.store(true, Ordering::Relaxed);
			}

			abd.uEdge = ABE_BOTTOM;
			abd.rc = RECT {
				left: m_pos.x,
				top: m_pos.y + m_size.height as i32 - pr,
				right: m_pos.x + m_size.width as i32,
				bottom: m_pos.y + m_size.height as i32,
			};

			SHAppBarMessage(ABM_QUERYPOS, &mut abd);
			SHAppBarMessage(ABM_SETPOS, &mut abd);

			// Critical: Force the window to the actual bottom of the screen,
			// ignoring what ABM_SETPOS might have tried to "correct" (like stacking on invisible taskbar)
			let final_y = m_pos.y + m_size.height as i32 - ph;
			let final_width = abd.rc.right - abd.rc.left;

			let mut current_rect = RECT::default();
			let mut already_positioned = false;
			if GetWindowRect(hwnd, &mut current_rect).is_ok() {
				let current_width = current_rect.right - current_rect.left;
				let current_height = current_rect.bottom - current_rect.top;
				if current_rect.left == abd.rc.left
					&& current_rect.top == final_y
					&& current_width == final_width
					&& current_height == ph
				{
					already_positioned = true;
				}
			}

			if !already_positioned {
				let _ = SetWindowPos(
					hwnd,
					None,
					abd.rc.left,
					final_y,
					final_width,
					ph,
					SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
				);
			}

			// Re-assert topmost — same reason as register_appbar.
			re_assert_topmost(hwnd);

			// Always ensure the window is visible — show() is idempotent
			if !window.is_visible().unwrap_or(false) {
				let _ = window.show();
			}
		}
	} else {
		let w = window.clone();
		tauri::async_runtime::spawn(async move {
			for _ in 0..10 {
				tokio::time::sleep(std::time::Duration::from_millis(500)).await;
				if let Ok(Some(_monitor)) = w.app_handle().primary_monitor() {
					register_dock_appbar(w);
					break;
				}
			}
		});
	}
}

/// True when a top-level dialog is a Windows property sheet, detected by its
/// tab-control child. This is locale-independent, unlike matching the
/// "Properties" window title.
unsafe fn dialog_has_tab_control(hwnd: HWND) -> bool {
	unsafe extern "system" fn child_proc(child: HWND, lparam: LPARAM) -> BOOL {
		let found = &mut *(lparam.0 as *mut bool);
		let mut class_name = [0u8; 64];
		let len = windows::Win32::UI::WindowsAndMessaging::GetClassNameA(child, &mut class_name);
		if std::str::from_utf8(&class_name[..len as usize]).unwrap_or("") == "SysTabControl32" {
			*found = true;
			return BOOL(0);
		}
		BOOL(1)
	}

	let mut found = false;
	let _ = windows::Win32::UI::WindowsAndMessaging::EnumChildWindows(
		Some(hwnd),
		Some(child_proc),
		LPARAM(&mut found as *mut bool as isize),
	);
	found
}

pub unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
	let apps = &mut *(lparam.0 as *mut Vec<AppInfo>);

	if IsWindowVisible(hwnd).as_bool() {
		// DWM-cloaked windows are not actually on screen: closed/suspended UWP
		// apps keep a cloaked frame alive, and windows on other virtual desktops
		// are shell-cloaked. Neither belongs in the dock. IsWindowVisible stays
		// true for them, so this needs the DWM check.
		{
			let mut cloaked = 0u32;
			let size = std::mem::size_of::<u32>() as u32;
			if windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
				hwnd,
				windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
				&mut cloaked as *mut _ as *mut _,
				size,
			)
			.is_ok()
				&& cloaked != 0
			{
				return true.into();
			}
		}

		let mut text = [0u16; 512];
		let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(hwnd, &mut text);
		if len > 0 {
			let title = String::from_utf16_lossy(&text[..len as usize]);

			let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;

			// Basic filter for top-level app windows
			// We include windows without captions if they don't have the ToolWindow style,
			// as many games and modern apps (like Spotify/Valorant) lack WS_CAPTION.
			if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
				return true.into();
			}

			// Filter out system containers and background stuff
			if title == "Program Manager" || title == "Bloom" || title == "Bloom Dock" {
				return true.into();
			}

			let mut process_id = 0u32;
			GetWindowThreadProcessId(hwnd, Some(&mut process_id));

			if let Ok(process_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)
			{
				let mut path_buf = [0u16; 1024];
				let mut path_len = path_buf.len() as u32;
				if QueryFullProcessImageNameW(
					process_handle,
					PROCESS_NAME_WIN32,
					windows::core::PWSTR(path_buf.as_mut_ptr()),
					&mut path_len,
				)
				.is_ok()
				{
					let path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
					let lowercase_path = path.to_lowercase();

					let mut class_name = [0u8; 256];
					let class_len =
						windows::Win32::UI::WindowsAndMessaging::GetClassNameA(hwnd, &mut class_name);
					let window_class = std::str::from_utf8(&class_name[..class_len as usize]).unwrap_or("");

					// Explorer hosts folder windows (CabinetWClass/ExploreWClass) and
					// shell property sheets (`#32770` dialogs that contain a tab
					// control). Both are tracked so the Properties window shows up
					// under the File Explorer dock item.
					let is_explorer_window = window_class == "CabinetWClass"
						|| window_class == "ExploreWClass"
						|| (window_class == "#32770" && dialog_has_tab_control(hwnd));

					// Filter out Bloom itself (except the Settings window) and some common background processes
					if (lowercase_path.contains("bloom.exe") && title != "Settings")
						|| lowercase_path.contains("conhost.exe")
						|| (lowercase_path.contains("explorer.exe") && !is_explorer_window)
						|| lowercase_path.contains("shellexperiencehost.exe")
						|| lowercase_path.contains("searchhost.exe")
						|| lowercase_path.contains("textinputhost.exe")
						|| (lowercase_path.contains("applicationframehost.exe")
							&& window_class != "ApplicationFrameWindow")
					{
						let _ = CloseHandle(process_handle);
						return true.into();
					}

					let name = Path::new(&path)
						.file_name()
						.and_then(|n| n.to_str())
						.unwrap_or(&title)
						.replace(".exe", "");

					let exe_name = Path::new(&path)
						.file_name()
						.and_then(|n| n.to_str())
						.map(|s| s.to_string());

					// The visible top-level window of a UWP app is the
					// ApplicationFrameWindow owned by ApplicationFrameHost.exe; the
					// app's own Windows.UI.Core.CoreWindow is a separate top-level
					// window and not the one users interact with. The frame carries
					// the title and package identity, so track it and skip the inner
					// core window to avoid duplicate dock items.
					let is_uwp_core_window = window_class == "Windows.UI.Core.CoreWindow";
					let is_uwp_frame = window_class == "ApplicationFrameWindow";

					let is_browser_host = lowercase_path.contains("msedge.exe")
						|| lowercase_path.contains("chrome.exe")
						|| lowercase_path.contains("brave.exe");

					// A browser window and an installed PWA running under the same
					// browser can share a title, so the title alone cannot tell them
					// apart. The window's AppUserModelID can: PWA windows carry a web
					// app id (or a package id for Store-installed PWAs), while regular
					// browser windows only carry the browser itself ("MSEdge", "Chrome",
					// "Brave", ...). A missing id falls back to the title so unusual
					// hosts keep the previous behaviour.
					let window_aumid =
						if is_uwp_frame || lowercase_path.contains("\\windowsapps\\") || is_browser_host {
							crate::utils::get_window_app_user_model_id(hwnd)
						} else {
							None
						};
					let is_browser_pwa = is_browser_host
						&& window_aumid
							.as_deref()
							.map_or(false, crate::commands::is_browser_pwa_aumid);

					let final_name = if ((is_browser_host && (is_browser_pwa || window_aumid.is_none()))
						|| name == "ApplicationFrameHost"
						|| name == "SystemSettings")
						&& !title.is_empty()
					{
						// Extract a cleaner name from the window title for host processes (PWAs, UWP apps)
						title
							.split(" - ")
							.next()
							.map(|s| s.trim())
							.unwrap_or(&title)
							.to_string()
					} else if name == "explorer" && title.is_empty() {
						"File Explorer".to_string()
					} else {
						name.clone()
					};

					// Only avoid adding the exact same window handle (HWND) multiple times
					let already_exists = apps.iter().any(|a| a.hwnd == Some(hwnd.0 as isize));

					if !already_exists && !is_uwp_core_window {
						// Packaged apps (Store/UWP/PWAs) expose their package identity on the
						// window. Using it as the path makes pinned shell-app entries match
						// their running windows and gives the icon code an exact AUMID.
						let path =
							if is_uwp_frame || lowercase_path.contains("\\windowsapps\\") || is_browser_host {
								match window_aumid {
									Some(aumid) if aumid.contains('!') => aumid,
									_ => path,
								}
							} else {
								path
							};
						apps.push(AppInfo {
							name: final_name,
							path,
							icon: None,
							is_running: true,
							hwnd: Some(hwnd.0 as isize),
							executable: exe_name,
							all_hwnds: None,
						});
					}
				}
				let _ = CloseHandle(process_handle);
			}
		}
	}
	true.into()
}

pub fn unregister_appbar_native(hwnd: HWND) {
	unsafe {
		use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_REMOVE, APPBARDATA};
		let mut abd = APPBARDATA {
			cbSize: std::mem::size_of::<APPBARDATA>() as u32,
			hWnd: hwnd,
			..Default::default()
		};
		SHAppBarMessage(ABM_REMOVE, &mut abd);
	}
}

fn reposition_all_windows(app_handle: &AppHandle) {
	if MAIN_APPBAR_REGISTERED.load(Ordering::Relaxed) {
		if let Some(main_win) = app_handle.get_webview_window("main") {
			register_appbar(main_win);
		}
	}
	// Only reposition the dock if it's enabled in settings.
	// Without this guard, power events (plug/unplug, wake) would re-show
	// a dock that the user had previously disabled.
	let dock_enabled =
		get_setting_str(app_handle, "bloom-dock-enabled").unwrap_or_else(|| "true".to_string());
	if dock_enabled == "true" {
		if let Some(dock_win) = app_handle.get_webview_window("dock") {
			if DOCK_APPBAR_REGISTERED.load(Ordering::Relaxed) {
				register_dock_appbar(dock_win);
			} else {
				// Auto-hide mode: reposition dock at bottom of screen
				reposition_autohide_dock(app_handle, dock_win);
			}
		}
	}
	if NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed) {
		set_taskbar_visibility(false, false);
	}
	sync_overlays(app_handle);
	// Re-assert topmost on all windows after repositioning to recover from any z-order loss
	if let Some(main_win) = app_handle.get_webview_window("main") {
		if let Ok(hwnd) = main_win.hwnd() {
			re_assert_topmost(hwnd);
		}
	}
	if let Some(dock_win) = app_handle.get_webview_window("dock") {
		if let Ok(hwnd) = dock_win.hwnd() {
			re_assert_topmost(hwnd);
		}
	}
}

fn reposition_autohide_dock(app_handle: &AppHandle, dock_win: tauri::WebviewWindow) {
	let dock_clone = dock_win.clone();
	let ah = app_handle.clone();
	tauri::async_runtime::spawn(async move {
		for _attempt in 0..5 {
			tokio::time::sleep(std::time::Duration::from_millis(200)).await;
			let hwnd_val = match dock_clone.hwnd() {
				Ok(h) => h.0 as isize,
				Err(_) => continue,
			};
			let ph = match dock_clone.outer_size() {
				Ok(s) => s.height as i32,
				Err(_) => continue,
			};
			if ph <= 10 {
				continue;
			}
			let monitor_info = dock_clone.primary_monitor().ok().flatten().map(|m| {
				let s = m.size();
				let p = m.position();
				(s.width as i32, s.height as i32, p.x, p.y)
			});
			if let Some((m_w, m_h, m_x, m_y)) = monitor_info {
				let final_y = m_y + m_h - ph;
				unsafe {
					use windows::Win32::Foundation::HWND;
					use windows::Win32::UI::WindowsAndMessaging::{
						SetWindowPos, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOZORDER,
					};
					let _ = SetWindowPos(
						HWND(hwnd_val as *mut _),
						None,
						m_x,
						final_y,
						m_w,
						ph,
						SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
					);
				}
				if let Ok(hwnd) = dock_clone.hwnd() {
					re_assert_topmost(hwnd);
				}
				let _ = dock_clone.show();
				break;
			}
		}
		if NATIVE_TASKBAR_HIDDEN.load(Ordering::Relaxed) {
			set_taskbar_visibility(false, false);
		}
		sync_overlays(&ah);
	});
}

pub fn setup_display_change_monitor(app_handle: AppHandle) {
	let _ = crate::state::DISPLAY_MONITOR_HANDLE.set(app_handle);

	std::thread::spawn(|| unsafe {
		use windows::Win32::System::LibraryLoader::GetModuleHandleW;
		use windows::Win32::UI::WindowsAndMessaging::*;

		let class_name = windows::core::PCSTR(c"BloomDisplayMonitor".as_ptr() as *const u8);
		let h_inst = GetModuleHandleW(None).unwrap_or_default().into();

		let wnd_class = WNDCLASSEXA {
			cbSize: std::mem::size_of::<WNDCLASSEXA>() as u32,
			lpfnWndProc: Some(display_monitor_proc),
			hInstance: h_inst,
			lpszClassName: class_name,
			..Default::default()
		};

		if RegisterClassExA(&wnd_class) == 0 {
			return;
		}

		let hwnd = CreateWindowExA(
			WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0),
			class_name,
			windows::core::PCSTR::null(),
			WINDOW_STYLE::default(),
			0,
			0,
			0,
			0,
			None,
			None,
			Some(h_inst),
			None,
		);

		let hwnd = match hwnd {
			Ok(h) => h,
			Err(_) => return,
		};

		let mut msg = MSG::default();
		while GetMessageW(&mut msg, Some(hwnd), 0, 0).as_bool() {
			let _ = TranslateMessage(&msg);
			DispatchMessageW(&msg);
		}
	});
}

unsafe extern "system" fn display_monitor_proc(
	hwnd: HWND,
	msg: u32,
	wparam: windows::Win32::Foundation::WPARAM,
	lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
	use windows::Win32::Foundation::LRESULT;
	use windows::Win32::UI::WindowsAndMessaging::*;

	const WM_DISPLAYCHANGE: u32 = 0x007E;
	const WM_POWERBROADCAST: u32 = 0x0218;
	const PBT_APMPOWERSTATUSCHANGE: u32 = 0x000A;
	const PBT_APMRESUMESUSPEND: u32 = 0x0007;
	const WM_DWMCOLORIZATIONCOLORCHANGED: u32 = 0x0320;
	const WM_SETTINGCHANGE: u32 = 0x001A;

	match msg {
		WM_DISPLAYCHANGE => {
			let now = get_now_ms();
			let last = LAST_DISPLAY_CHANGE_MS.load(Ordering::Relaxed);
			if now - last > 500 {
				LAST_DISPLAY_CHANGE_MS.store(now, Ordering::Relaxed);
				if let Some(app_handle) = DISPLAY_MONITOR_HANDLE.get() {
					let ah = app_handle.clone();
					tauri::async_runtime::spawn(async move {
						tokio::time::sleep(std::time::Duration::from_millis(200)).await;
						reposition_all_windows(&ah);
					});
				}
			}
			LRESULT(0)
		}
		WM_POWERBROADCAST => {
			let pw = wparam.0 as u32;
			if pw == PBT_APMPOWERSTATUSCHANGE || pw == PBT_APMRESUMESUSPEND {
				let now = get_now_ms();
				let last = LAST_DISPLAY_CHANGE_MS.load(Ordering::Relaxed);
				if now - last > 1000 {
					LAST_DISPLAY_CHANGE_MS.store(now, Ordering::Relaxed);
					if let Some(app_handle) = DISPLAY_MONITOR_HANDLE.get() {
						let ah = app_handle.clone();
						tauri::async_runtime::spawn(async move {
							// Longer delay for wake from sleep to allow display to fully initialize
							let delay = if pw == PBT_APMRESUMESUSPEND { 500 } else { 300 };
							tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
							reposition_all_windows(&ah);
						});
					}
				}
			}
			LRESULT(0)
		}
		WM_DWMCOLORIZATIONCOLORCHANGED | WM_SETTINGCHANGE => {
			if let Some(app_handle) = DISPLAY_MONITOR_HANDLE.get() {
				let color_hex = if let Some(c) = crate::commands::get_windows_accent_color() {
					c
				} else {
					let mut color = 0u32;
					let mut opaque = windows::core::BOOL(0);
					if unsafe {
						windows::Win32::Graphics::Dwm::DwmGetColorizationColor(&mut color, &mut opaque).is_ok()
					} {
						let r = ((color >> 16) & 0xff) as u8;
						let g = ((color >> 8) & 0xff) as u8;
						let b = (color & 0xff) as u8;
						format!("#{:02x}{:02x}{:02x}", r, g, b)
					} else {
						"#0078d4".to_string()
					}
				};
				let _ = app_handle.emit("system-accent-changed", color_hex);
			}
			DefWindowProcW(hwnd, msg, wparam, lparam)
		}
		_ => DefWindowProcW(hwnd, msg, wparam, lparam),
	}
}

#[cfg(test)]
mod tests {
    use super::win_number_index;

    #[test]
    fn win_number_maps_top_row_digits_only() {
        assert_eq!(win_number_index(0x31), Some(0));
        assert_eq!(win_number_index(0x35), Some(4));
        assert_eq!(win_number_index(0x39), Some(8));
        // 0, letters and numpad digits are not dock slots
        assert_eq!(win_number_index(0x30), None);
        assert_eq!(win_number_index(0x41), None);
        assert_eq!(win_number_index(0x61), None);
    }
}
