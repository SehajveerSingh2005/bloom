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
    write_new(&folder(folder_name)?, &text_name(name)?, content)
}

/// Only plain-text extensions: anything else could be launched later by `open`.
const TEXT: [&str; 9] = [
    "txt", "md", "csv", "json", "log", "xml", "ics", "yaml", "yml",
];

/// A plain name with a text extension; a name without one gets `.txt`.
fn text_name(name: &str) -> Result<String, String> {
    let name = plain_name(name)?;
    match Path::new(&name).extension().and_then(|e| e.to_str()) {
        None => Ok(format!("{name}.txt")),
        Some(ext) if TEXT.contains(&ext.to_ascii_lowercase().as_str()) => Ok(name),
        Some(_) => Err("write_file only creates text files (.txt, .md, .csv, ...); use run_powershell for other files".into()),
    }
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

/// The path Windows would really open: it must exist, is canonical (trailing
/// dots, `.`/`..`, 8.3 names, symlinks and separators resolved) and local: no UNC
/// and no `:` past the drive prefix (alternate data streams). Checked == opened.
pub fn resolve_local(target: &str) -> Result<String, String> {
    if target.chars().any(char::is_control) {
        return Err("that target has control characters".into());
    }
    let full = std::fs::canonicalize(target).map_err(|_| "No such file or folder.".to_string())?;
    let full = full.to_string_lossy().into_owned();
    let path = full.strip_prefix(r"\\?\").unwrap_or(&full);
    let drive = path.len() >= 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path[1..].starts_with(r":\")
        && !path[2..].contains(':');
    if !drive {
        return Err("only files and folders on a local drive can be opened".into());
    }
    Ok(path.to_string())
}

pub fn classify_open(target: &str) -> Result<OpenTarget, String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("nothing to open".into());
    }
    if target.chars().any(char::is_control) {
        return Err("that target has control characters".into());
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
        let path = resolve_local(target)?;
        let ext = Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if RUNNABLE.contains(&ext.as_str()) {
            return Err("programs and scripts can't be opened by path; use run_powershell".into());
        }
        return Ok(OpenTarget::Shell(path));
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
    fn only_text_files_are_created() {
        assert_eq!(text_name("notes").unwrap(), "notes.txt");
        assert_eq!(text_name("a.MD").unwrap(), "a.MD");
        for bad in ["run.bat", "x.url", "y.TXT.exe"] {
            assert!(text_name(bad).is_err(), "{bad} should be refused");
        }
    }

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
        let d = temp_dir().canonicalize().unwrap();
        let d = d.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let touch = |n: &str| {
            let f = format!("{d}\\{n}");
            std::fs::write(&f, "").unwrap();
            f
        };
        assert_eq!(
            classify_open("https://example.com"),
            Ok(Shell("https://example.com".into()))
        );
        assert_eq!(
            classify_open("ms-settings:bluetooth"),
            Ok(Shell("ms-settings:bluetooth".into()))
        );
        let txt = touch("notes.txt");
        assert_eq!(classify_open(&txt), Ok(Shell(txt.clone())));
        // Forward slashes and a ./ segment resolve to the same file.
        assert_eq!(classify_open(&txt.replace('\\', "/")), Ok(Shell(txt.clone())));
        assert_eq!(classify_open(&format!("{d}\\.\\notes.txt")), Ok(Shell(txt.clone())));
        assert_eq!(classify_open(&d), Ok(Shell(d.clone())));
        assert_eq!(classify_open("Spotify"), Ok(App("Spotify".into())));
        let exe = touch("evil.exe");
        assert!(classify_open(&exe).is_err());
        assert!(classify_open(&format!("{exe}.")).is_err());
        assert!(classify_open(&format!("{exe}\\.")).is_err());
        assert!(classify_open(&format!("{exe}\u{0}")).is_err());
        assert!(classify_open(&touch("run.PS1")).is_err());
        assert!(classify_open(&format!("{d}\\nope.txt")).unwrap_err().contains("No such"));
        // The default stream resolves to the file itself; named streams are refused.
        assert_eq!(classify_open(&format!("{txt}::$DATA")), Ok(Shell(txt.clone())));
        assert!(classify_open(&format!("{txt}:evil")).is_err());
        assert!(classify_open(r"\\host\share\f.txt").is_err());
        assert!(classify_open(r"\\?\UNC\h\s").is_err());
        assert!(classify_open(&format!("{d}\\a\u{7}.txt")).is_err());
        assert!(classify_open("file:///C:/x.txt").is_err());
        assert!(classify_open("ms-msdt:/id x").is_err());
        assert!(classify_open("  ").is_err());
    }
}
