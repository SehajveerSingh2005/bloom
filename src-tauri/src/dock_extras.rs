use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Videos, SHGetFileInfoW, SHGetKnownFolderPath, SHGetStockIconInfo, KF_FLAG_DEFAULT,
    SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGSI_ICON, SHGSI_LARGEICON, SHSTOCKICONINFO,
    SIID_RECYCLER, SIID_RECYCLERFULL,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, SW_SHOWNORMAL};

use crate::utils::{get_setting_str, icon_to_base64};

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;
const DRIVE_RAMDISK: u32 = 6;

/// id, display name, known-folder GUID. Order defines the dock and settings order.
const KNOWN_FOLDERS: &[(&str, &str, GUID)] = &[
    ("desktop", "Desktop", FOLDERID_Desktop),
    ("downloads", "Downloads", FOLDERID_Downloads),
    ("documents", "Documents", FOLDERID_Documents),
    ("pictures", "Pictures", FOLDERID_Pictures),
    ("music", "Music", FOLDERID_Music),
    ("videos", "Videos", FOLDERID_Videos),
];

#[derive(Clone, Serialize)]
pub struct DockExtra {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
    pub removable: bool,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct CustomFolder {
    pub name: String,
    pub path: String,
}

static DOCK_EXTRA_ICONS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn icon_cache() -> &'static Mutex<HashMap<String, String>> {
    DOCK_EXTRA_ICONS.get_or_init(|| Mutex::new(HashMap::new()))
}

unsafe fn file_icon(path: &str) -> Option<String> {
    let wide = to_wide(path);
    let mut info = SHFILEINFOW::default();
    let res = SHGetFileInfoW(
        PCWSTR(wide.as_ptr()),
        Default::default(),
        Some(&mut info),
        std::mem::size_of::<SHFILEINFOW>() as u32,
        SHGFI_ICON | SHGFI_LARGEICON,
    );
    if res == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let icon = icon_to_base64(info.hIcon);
    let _ = DestroyIcon(info.hIcon);
    icon
}

/// Shell icons for drives and folders are cached by path: the dock re-enumerates
/// on every poll, and extracting an icon hits the disk.
fn icon_for_path(path: &str) -> Option<String> {
    cached_icon(path, || unsafe { file_icon(path) })
}

unsafe fn stock_icon(full: bool) -> Option<String> {
    let mut info = SHSTOCKICONINFO::default();
    info.cbSize = std::mem::size_of::<SHSTOCKICONINFO>() as u32;
    SHGetStockIconInfo(
        if full {
            SIID_RECYCLERFULL
        } else {
            SIID_RECYCLER
        },
        SHGSI_ICON | SHGSI_LARGEICON,
        &mut info,
    )
    .ok()?;
    let icon = icon_to_base64(info.hIcon);
    let _ = DestroyIcon(info.hIcon);
    icon
}

fn cached_icon(key: &str, extract: impl FnOnce() -> Option<String>) -> Option<String> {
    let cache = icon_cache();
    if let Ok(guard) = cache.lock() {
        if let Some(icon) = guard.get(key) {
            return Some(icon.clone());
        }
    }
    let icon = extract()?;
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key.to_string(), icon.clone());
    }
    Some(icon)
}

fn recycle_bin_icon(full: bool) -> Option<String> {
    let key = if full {
        "recycle-bin:full"
    } else {
        "recycle-bin:empty"
    };
    cached_icon(key, || unsafe { stock_icon(full) })
}

/// Windows reports the share a drive letter maps to, without touching the
/// network: a disconnected mapping must not stall the dock's poll loop.
unsafe fn mapped_remote_name(root: &str) -> Option<String> {
    use windows::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    let wide = to_wide(root);
    let mut buffer = [0u16; 512];
    let mut len = buffer.len() as u32;
    let code = WNetGetConnectionW(
        PCWSTR(wide.as_ptr()),
        Some(PWSTR(buffer.as_mut_ptr())),
        &mut len,
    )
    .0;
    if code != 0 {
        return None;
    }
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}

unsafe fn drive_label(root: &str) -> Option<String> {
    let wide = to_wide(root);
    let mut buffer = [0u16; 261];
    GetVolumeInformationW(
        PCWSTR(wide.as_ptr()),
        Some(&mut buffer),
        None,
        None,
        None,
        None,
    )
    .ok()?;
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]).trim().to_string())
}

const DRIVE_DEFAULT_NAMES: &[(u32, &str)] = &[
    (DRIVE_REMOVABLE, "Removable Disk"),
    (DRIVE_FIXED, "Local Disk"),
    (DRIVE_REMOTE, "Network Drive"),
    (DRIVE_CDROM, "DVD Drive"),
    (DRIVE_RAMDISK, "RAM Disk"),
];

fn drive_display_name(
    label: Option<&str>,
    remote: Option<&str>,
    drive_type: u32,
    letter: char,
) -> String {
    let base = label
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .or_else(|| {
            remote
                .and_then(|unc| unc.trim_end_matches('\\').rsplit('\\').next())
                .filter(|share| !share.is_empty())
                .map(|share| share.to_string())
        })
        .or_else(|| {
            DRIVE_DEFAULT_NAMES
                .iter()
                .find(|(t, _)| *t == drive_type)
                .map(|(_, name)| name.to_string())
        })
        .unwrap_or_else(|| "Drive".to_string());
    format!("{} ({}:)", base, letter)
}

fn list_drives() -> Vec<DockExtra> {
    let mask = unsafe { GetLogicalDrives() };
    let mut drives = Vec::new();
    for slot in 0..26u32 {
        if mask & (1 << slot) == 0 {
            continue;
        }
        let letter = (b'A' + slot as u8) as char;
        let root = format!("{}:\\", letter);
        let wide = to_wide(&root);
        let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        if drive_type != DRIVE_FIXED
            && drive_type != DRIVE_REMOVABLE
            && drive_type != DRIVE_REMOTE
            && drive_type != DRIVE_CDROM
            && drive_type != DRIVE_RAMDISK
        {
            continue;
        }

        let remote = drive_type == DRIVE_REMOTE;
        let unc = if remote {
            match unsafe { mapped_remote_name(&root) } {
                Some(unc) => Some(unc),
                None => continue,
            }
        } else {
            None
        };

        // Fixed and RAM disks always have media; removable, optical and mapped
        // drives must answer a volume query or they are an empty card reader /
        // unloaded disc / dead mapping.
        let label = if drive_type == DRIVE_FIXED || drive_type == DRIVE_RAMDISK {
            unsafe { drive_label(&root) }
        } else {
            match unsafe { drive_label(&root) } {
                Some(label) => Some(label),
                None => continue,
            }
        };

        drives.push(DockExtra {
            id: format!("drive:{}", letter),
            kind: "drive".into(),
            name: drive_display_name(label.as_deref(), unc.as_deref(), drive_type, letter),
            path: root.clone(),
            icon: icon_for_path(&root),
            removable: drive_type == DRIVE_REMOVABLE || drive_type == DRIVE_CDROM,
        });
    }
    drives
}

unsafe fn known_folder_path(folder: &GUID) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    let path = SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None).ok()?;
    let result = path.to_string().ok();
    CoTaskMemFree(Some(path.0 as *const core::ffi::c_void));
    result.filter(|p| !p.is_empty())
}

fn list_known_folders(enabled: &[String]) -> Vec<DockExtra> {
    KNOWN_FOLDERS
        .iter()
        .filter(|(id, _, _)| enabled.iter().any(|e| e == id))
        .filter_map(|(id, name, folder)| {
            let path = unsafe { known_folder_path(folder) }?;
            Some(DockExtra {
                id: format!("folder:{}", id),
                kind: "folder".into(),
                name: name.to_string(),
                icon: icon_for_path(&path),
                path,
                removable: false,
            })
        })
        .collect()
}

fn parse_custom_folders(raw: &str) -> Vec<CustomFolder> {
    serde_json::from_str::<Vec<CustomFolder>>(raw).unwrap_or_default()
}

fn list_custom_folders(raw: &str) -> Vec<DockExtra> {
    parse_custom_folders(raw)
        .into_iter()
        // A dead UNC path would stall `is_dir` for seconds on every poll, so
        // only local folders are existence-checked; network folders are trusted.
        .filter(|folder| {
            folder.path.starts_with("\\\\") || std::path::Path::new(&folder.path).is_dir()
        })
        .map(|folder| DockExtra {
            id: format!("custom:{}", folder.path.to_lowercase()),
            kind: "custom".into(),
            name: folder.name,
            icon: icon_for_path(&folder.path),
            path: folder.path,
            removable: false,
        })
        .collect()
}

fn parse_enabled_folders(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|id| id.trim().to_lowercase())
        .filter(|id| !id.is_empty())
        .collect()
}

struct RecycleBinState {
    full: bool,
    checked_ms: i64,
}

static RECYCLE_BIN_STATE: OnceLock<Mutex<RecycleBinState>> = OnceLock::new();

/// Whether the bin holds anything, refreshed at most once a minute: the query
/// scans every volume and the dock polls every few seconds.
fn recycle_bin_full() -> bool {
    use windows::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    let state = RECYCLE_BIN_STATE.get_or_init(|| {
        Mutex::new(RecycleBinState {
            full: false,
            checked_ms: 0,
        })
    });
    if let Ok(guard) = state.lock() {
        if crate::utils::get_now_ms() - guard.checked_ms < 60_000 {
            return guard.full;
        }
    }
    let mut info = SHQUERYRBINFO::default();
    info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
    let full =
        unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.is_ok() && info.i64NumItems > 0;
    if let Ok(mut guard) = state.lock() {
        guard.full = full;
        guard.checked_ms = crate::utils::get_now_ms();
    }
    full
}

fn recycle_bin_extra() -> DockExtra {
    let full = recycle_bin_full();
    DockExtra {
        id: "recycle-bin".into(),
        kind: "recycle-bin".into(),
        name: "Recycle Bin".into(),
        path: "shell:RecycleBinFolder".into(),
        icon: recycle_bin_icon(full),
        removable: false,
    }
}

#[tauri::command]
pub async fn get_dock_extras(app: AppHandle) -> Vec<DockExtra> {
    let position =
        get_setting_str(&app, "bloom-dock-extras-position").unwrap_or_else(|| "off".into());
    if position != "left" && position != "right" {
        return Vec::new();
    }
    let drives_enabled = get_setting_str(&app, "bloom-dock-extras-drives")
        .unwrap_or_else(|| "true".into())
        != "false";
    let bin_enabled = get_setting_str(&app, "bloom-dock-extras-recycle-bin")
        .unwrap_or_else(|| "true".into())
        != "false";
    let folders =
        get_setting_str(&app, "bloom-dock-extras-folders").unwrap_or_else(|| "downloads".into());
    let custom =
        get_setting_str(&app, "bloom-dock-extras-custom-folders").unwrap_or_else(|| "[]".into());

    tauri::async_runtime::spawn_blocking(move || {
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
        // WIC (used to encode the extracted icons) needs a COM apartment on
        // this worker thread, same as the app-icon command's thread.
        let com_initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };

        let mut extras = Vec::new();
        if drives_enabled {
            extras.extend(list_drives());
        }
        extras.extend(list_known_folders(&parse_enabled_folders(&folders)));
        extras.extend(list_custom_folders(&custom));
        if bin_enabled {
            extras.push(recycle_bin_extra());
        }

        if com_initialized {
            unsafe { CoUninitialize() };
        }
        extras
    })
    .await
    .unwrap_or_default()
}

fn shell_execute(file: &str, params: Option<&str>, verb: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    let file_wide = to_wide(file);
    let verb_wide = to_wide(verb);
    let params_wide = params.map(to_wide);
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb_wide.as_ptr()),
            PCWSTR(file_wide.as_ptr()),
            params_wide
                .as_ref()
                .map_or(PCWSTR::null(), |p| PCWSTR(p.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        return Err(format!("Could not open {}", file));
    }
    Ok(())
}

#[tauri::command]
pub async fn open_dock_extra(kind: String, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if kind == "recycle-bin" {
            shell_execute("explorer.exe", Some("shell:RecycleBinFolder"), "open")
        } else {
            shell_execute(&path, None, "open")
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ejects a drive the same way Explorer's context menu does: the "Eject" verb
/// on the drive's `FolderItem` (CSIDL 17 = This PC). `ShellExecuteW` has no
/// usable "eject" verb for drives, and Shell.Application is an in-proc
/// Apartment-threaded server, so this must run on a fresh STA thread — a
/// blocking-pool thread may already be MTA from other commands.
fn eject_drive_on_sta_thread(root: String) -> Result<(), String> {
    let failure = root.clone();
    std::thread::spawn(move || {
        use windows::core::BSTR;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::System::Variant::VARIANT;
        use windows::Win32::UI::Shell::{Folder, FolderItem, IShellDispatch, Shell};

        let com_initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
        let result = unsafe {
            (|| -> windows::core::Result<()> {
                let shell: IShellDispatch = CoCreateInstance(&Shell, None, CLSCTX_ALL)?;
                let drives: Folder = shell.NameSpace(&VARIANT::from(17i32))?;
                let item: FolderItem =
                    drives.ParseName(&BSTR::from(root.trim_end_matches('\\')))?;
                item.InvokeVerb(&VARIANT::from("Eject"))
            })()
        };
        if com_initialized {
            unsafe { CoUninitialize() };
        }
        result.map_err(|_| format!("Could not eject {}", root))
    })
    .join()
    .unwrap_or_else(|_| Err(format!("Could not eject {}", failure)))
}

#[tauri::command]
pub async fn eject_dock_extra(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        eject_drive_on_sta_thread(path.clone()).or_else(|_| {
            shell_execute(&path, None, "eject").map_err(|_| format!("Could not eject {}", path))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_drive_name_from_label_or_fallback() {
        assert_eq!(
            drive_display_name(Some("Windows"), None, DRIVE_FIXED, 'C'),
            "Windows (C:)"
        );
        assert_eq!(
            drive_display_name(Some(""), None, DRIVE_REMOVABLE, 'E'),
            "Removable Disk (E:)"
        );
        assert_eq!(
            drive_display_name(None, None, DRIVE_CDROM, 'D'),
            "DVD Drive (D:)"
        );
    }

    #[test]
    fn names_remote_drive_after_its_share() {
        assert_eq!(
            drive_display_name(None, Some("\\\\server\\projects"), DRIVE_REMOTE, 'Z'),
            "projects (Z:)"
        );
        assert_eq!(
            drive_display_name(
                Some("Games"),
                Some("\\\\server\\projects"),
                DRIVE_REMOTE,
                'Z'
            ),
            "Games (Z:)"
        );
    }

    #[test]
    fn parses_enabled_folders_loosely() {
        assert_eq!(
            parse_enabled_folders("Downloads, documents ,pictures"),
            vec!["downloads", "documents", "pictures"]
        );
        assert!(parse_enabled_folders("").is_empty());
    }

    #[test]
    fn parses_custom_folders_and_rejects_junk() {
        let folders = parse_custom_folders(r#"[{"name":"Projects","path":"D:\\Projects"}]"#);
        assert_eq!(
            folders,
            vec![CustomFolder {
                name: "Projects".into(),
                path: "D:\\Projects".into()
            }]
        );
        assert!(parse_custom_folders("not json").is_empty());
        assert!(parse_custom_folders("[{\"name\":\"x\"}]").is_empty());
    }
}
