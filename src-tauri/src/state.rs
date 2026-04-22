use std::sync::{atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicI64}, Mutex, OnceLock};
use std::sync::mpsc::Sender;
use std::collections::HashMap;
use crate::types::{SystemCommand, IntRect, AppInfo};
use tauri::{PhysicalPosition, PhysicalSize};

pub static mut COMMAND_SENDER: Option<Sender<SystemCommand>> = None;
pub static MAIN_APPBAR_REGISTERED: AtomicBool = AtomicBool::new(false);
pub static DOCK_APPBAR_REGISTERED: AtomicBool = AtomicBool::new(false);
pub static CURRENT_DOCK_OVERLAP: AtomicI32 = AtomicI32::new(-1);
pub static NATIVE_TASKBAR_HIDDEN: AtomicBool = AtomicBool::new(false);


pub static DOCK_RECT: Mutex<Option<IntRect>> = Mutex::new(None);
pub static DOCK_IS_HOVERED: AtomicBool = AtomicBool::new(false);
pub static MENU_IS_OPEN: AtomicBool = AtomicBool::new(false);
pub static MENU_RECT: Mutex<Option<IntRect>> = Mutex::new(None);
pub static ICON_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

pub static INSTALLED_APPS_CACHE: OnceLock<Mutex<Vec<AppInfo>>> = OnceLock::new();
pub static IS_SCANNING: AtomicBool = AtomicBool::new(false);

pub static BRIGHTNESS_SENDER: OnceLock<Sender<u32>> = OnceLock::new();
pub static CURRENT_BRIGHTNESS: AtomicU32 = AtomicU32::new(50);
pub static LAST_BRIGHTNESS_CHANGE: AtomicI64 = AtomicI64::new(0);
pub static ANY_MEDIA_PLAYING: AtomicBool = AtomicBool::new(false);

pub static MAIN_WINDOW_RECT: Mutex<Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>> = Mutex::new(None);
pub static DOCK_WINDOW_RECT: Mutex<Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>> = Mutex::new(None);
