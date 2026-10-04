//! File tools: create files in the user's own folders (never overwriting), and
//! decide what `open` may launch.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Downloads, Documents or Desktop, following OneDrive redirection.
pub fn folder(name: &str) -> Result<PathBuf, String> {
    let dir = match name.to_ascii_lowercase().as_str() {
        "downloads" => dirs::download_dir(),
        "documents" => dirs::document_dir(),
        "desktop" => dirs::desktop_dir(),
        other => {
            return Err(format!(
                "folder must be downloads, documents or desktop, not {other}"
            ))
        }
    };
    dir.ok_or_else(|| format!("can't find the {name} folder"))
}

pub fn write_file(folder_name: &str, name: &str, content: &str) -> Result<PathBuf, String> {
    write_new(&folder(folder_name)?, &plain_name(name)?, content)
}

/// A bare file name: no folders, drive or reserved characters.
pub fn plain_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with('.')
        || name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        });
    if bad {
        Err(format!("not a plain file name: {name}"))
    } else {
        Ok(name.to_string())
    }
}

/// Creates `name`, or `stem (2).ext`, `stem (3).ext` and so on. Never replaces
/// a file: `create_new` fails if the name exists, even if it appeared a moment ago.
pub fn write_new(dir: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    for n in 1..1000 {
        let path = if n == 1 {
            dir.join(name)
        } else {
            dir.join(format!("{stem} ({n}){ext}"))
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(content.as_bytes())
                    .map_err(|e| e.to_string())?;
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("too many files called {name}"))
}

#[derive(Debug, PartialEq)]
pub enum OpenTarget {
    /// A web or ms-settings: link, or a file or folder path: the shell opens it.
    Shell(String),
    /// A bare name: Bloom looks it up among installed apps.
    App(String),
}

/// Extensions that run code. `open` refuses them: programs and scripts go
/// through run_powershell, which the security level guards.
const RUNNABLE: [&str; 21] = [
    "exe", "com", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "msi",
    "msp", "scr", "pif", "cpl", "hta", "lnk", "reg", "jar",
];

pub fn classify_open(target: &str) -> Result<OpenTarget, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("nothing to open".into());
    }
    let lower = target.to_ascii_lowercase();
    if let Some((scheme, _)) = lower.split_once(':') {
        // A one-letter "scheme" is a drive letter (C:\...).
        let is_scheme = scheme.len() > 1
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if is_scheme {
            return if matches!(scheme, "http" | "https" | "ms-settings") {
                Ok(OpenTarget::Shell(target.into()))
            } else {
                Err(format!(
                    "{scheme}: links can't be opened, only web and ms-settings: links"
                ))
            };
        }
    }
    if target.contains('\\') || target.contains('/') {
        let ext = Path::new(&lower)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if RUNNABLE.contains(&ext) {
            return Err("programs and scripts can't be opened by path; use run_powershell".into());
        }
        return Ok(OpenTarget::Shell(target.into()));
    }
    Ok(OpenTarget::App(target.into()))
}

/// Opens a link, file or folder with its default handler.
#[cfg(windows)]
pub fn shell_open(target: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (verb, file) = (wide("open"), wide(target));
    let result = unsafe {
        ShellExecuteW(
            None,
            windows::core::PCWSTR(verb.as_ptr()),
            windows::core::PCWSTR(file.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value above 32.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!("Windows couldn't open {target}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    #[test]
    fn never_overwrites() {
        let dir = temp_dir();
        let a = write_new(&dir, "groceries.txt", "eggs").unwrap();
        let b = write_new(&dir, "groceries.txt", "milk").unwrap();
        let c = write_new(&dir, "groceries.txt", "rice").unwrap();
        assert_eq!(a.file_name().unwrap(), "groceries.txt");
        assert_eq!(b.file_name().unwrap(), "groceries (2).txt");
        assert_eq!(c.file_name().unwrap(), "groceries (3).txt");
        assert_eq!(std::fs::read_to_string(a).unwrap(), "eggs");
    }

    #[test]
    fn names_without_extension_get_a_suffix_too() {
        let dir = temp_dir();
        write_new(&dir, "notes", "1").unwrap();
        assert_eq!(
            write_new(&dir, "notes", "2").unwrap().file_name().unwrap(),
            "notes (2)"
        );
    }

    #[test]
    fn only_plain_names() {
        assert!(plain_name("groceries.txt").is_ok());
        for bad in [
            "", "..", "../x.txt", "a/b.txt", "a\\b.txt", "C:x.txt", "x.", "a?.txt",
        ] {
            assert!(plain_name(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn open_classification() {
        use OpenTarget::*;
        assert_eq!(
            classify_open("https://example.com"),
            Ok(Shell("https://example.com".into()))
        );
        assert_eq!(
            classify_open("ms-settings:bluetooth"),
            Ok(Shell("ms-settings:bluetooth".into()))
        );
        assert_eq!(
            classify_open(r"C:\Users\me\notes.txt"),
            Ok(Shell(r"C:\Users\me\notes.txt".into()))
        );
        assert_eq!(
            classify_open(r"C:\Users\me"),
            Ok(Shell(r"C:\Users\me".into()))
        );
        assert_eq!(classify_open("Spotify"), Ok(App("Spotify".into())));
        assert!(classify_open(r"C:\Users\me\Downloads\setup.exe").is_err());
        assert!(classify_open(r"C:\x\run.PS1").is_err());
        assert!(classify_open("file:///C:/x.txt").is_err());
        assert!(classify_open("ms-msdt:/id x").is_err());
        assert!(classify_open("  ").is_err());
    }
}
