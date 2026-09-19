use crate::types::AppInfo;
use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/usr/share/applications"),
    ];
    if let Some(home) = env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    dirs
}

fn desktop_value(contents: &str, key: &str) -> Option<String> {
    let mut in_entry = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry {
            if let Some(value) = line.strip_prefix(&format!("{key}=")) {
                return Some(value.trim().to_owned());
            }
        }
    }
    None
}

/// Split an `Exec=` line, honouring the double quotes the desktop entry spec
/// allows around arguments that contain spaces. No shell expansion happens.
fn split_exec(exec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in exec.chars() {
        if escaped {
            current.push(character);
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character.is_whitespace() && !quoted {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn is_field_code(token: &str) -> bool {
    token.starts_with('%')
}

/// `VAR=value` assignments may prefix the command when using `env`.
fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with(|character: char| character.is_ascii_digit())
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

/// First executable token of an `Exec=` line, ignoring field codes and the
/// `env VAR=value` prefix. No shell expansion is performed.
fn exec_program(exec: &str) -> Option<String> {
    let mut tokens = split_exec(exec);
    if tokens.first().map(String::as_str) == Some("env") {
        tokens.remove(0);
    }
    tokens
        .into_iter()
        .find(|token| !is_field_code(token) && !is_env_assignment(token))
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem().and_then(|stem| stem.to_str()).map(str::to_owned)
}

fn executable_name(program: &str) -> Option<String> {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

/// A launchable freedesktop application.
#[derive(Clone, Debug)]
pub struct DesktopEntry {
    pub path: String,
    pub name: String,
    pub icon: Option<String>,
    pub executable: Option<String>,
    pub startup_wm_class: Vec<String>,
}

fn entry_from_desktop_file(path: &Path) -> Option<DesktopEntry> {
    let contents = fs::read_to_string(path).ok()?;
    if desktop_value(&contents, "Type").as_deref().unwrap_or("Application") != "Application"
        || desktop_value(&contents, "Hidden").as_deref() == Some("true")
        || desktop_value(&contents, "NoDisplay").as_deref() == Some("true")
    {
        return None;
    }
    let name = desktop_value(&contents, "Name")?;
    let executable = desktop_value(&contents, "Exec")
        .as_deref()
        .and_then(exec_program)
        .and_then(|program| executable_name(&program));
    let startup_wm_class = desktop_value(&contents, "StartupWMClass")
        .map(|value| {
            value
                .split([';', ','])
                .map(|part| part.trim().to_owned())
                .filter(|part| !part.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Some(DesktopEntry {
        path: path.to_string_lossy().into_owned(),
        name,
        icon: desktop_value(&contents, "Icon"),
        executable,
        startup_wm_class,
    })
}

/// All launchable entries, deduplicated by name with user entries winning.
fn scan_entries() -> Vec<DesktopEntry> {
    let mut entries = BTreeMap::new();
    for dir in application_dirs() {
        let Ok(read_dir) = fs::read_dir(dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(entry) = entry_from_desktop_file(&path) {
                entries.insert(entry.name.to_lowercase(), entry);
            }
        }
    }
    entries.into_values().collect()
}

/// The application inventory is small but reading it hits the filesystem, so
/// cache it briefly instead of rescanning on every window poll.
pub fn entries() -> Vec<DesktopEntry> {
    static CACHE: OnceLock<Mutex<Option<(Instant, Vec<DesktopEntry>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let Ok(mut cache) = cache.lock() else {
        return scan_entries();
    };
    if let Some((stamp, entries)) = cache.as_ref() {
        if stamp.elapsed() < Duration::from_secs(10) {
            return entries.clone();
        }
    }
    let entries = scan_entries();
    *cache = Some((Instant::now(), entries.clone()));
    entries
}

pub fn discover() -> Vec<AppInfo> {
    entries()
        .into_iter()
        .filter_map(|entry| {
            Some(AppInfo {
                name: entry.name,
                path: entry.path,
                icon: entry.icon,
                is_running: false,
                hwnd: None,
                executable: entry.executable,
                all_hwnds: None,
            })
        })
        .collect()
}

/// Lookup tables that attribute an X11 window to a `.desktop` entry.
pub struct EntryIndex {
    entries: Vec<DesktopEntry>,
    by_wm_class: HashMap<String, usize>,
    by_executable: HashMap<String, usize>,
    by_stem: HashMap<String, usize>,
}

impl EntryIndex {
    pub fn build() -> Self {
        let entries = entries();
        let mut by_wm_class = HashMap::new();
        let mut by_executable = HashMap::new();
        let mut by_stem = HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            for wm_class in &entry.startup_wm_class {
                by_wm_class.entry(wm_class.to_lowercase()).or_insert(index);
            }
            if let Some(executable) = &entry.executable {
                by_executable
                    .entry(executable.to_lowercase())
                    .or_insert(index);
            }
            if let Some(stem) = file_stem(Path::new(&entry.path)) {
                by_stem.entry(stem.to_lowercase()).or_insert(index);
            }
        }
        Self {
            entries,
            by_wm_class,
            by_executable,
            by_stem,
        }
    }

    /// Match a window using the standard hints in order of reliability:
    /// declared WM class, desktop file name, then the process executable.
    pub fn match_window(
        &self,
        wm_class: &str,
        instance: &str,
        executable: Option<&str>,
    ) -> Option<&DesktopEntry> {
        let candidates = [
            wm_class.to_lowercase(),
            instance.to_lowercase(),
        ];
        for candidate in candidates.iter().filter(|value| !value.is_empty()) {
            if let Some(index) = self
                .by_wm_class
                .get(candidate)
                .or_else(|| self.by_stem.get(candidate))
            {
                return self.entries.get(*index);
            }
        }
        let executable = executable?.to_lowercase();
        let index = self
            .by_executable
            .get(&executable)
            .or_else(|| self.by_stem.get(&executable))?;
        self.entries.get(*index)
    }
}

/// The `Icon=` value for a desktop file, used by the icon command.
pub fn icon_name_for_desktop_file(desktop_path: &str) -> Option<String> {
    let contents = fs::read_to_string(desktop_path).ok()?;
    desktop_value(&contents, "Icon")
}

/// Resolve an icon for a dock item given its path and optional display name.
///
/// Running windows report either a `.desktop` path (matched to an installed
/// application) or a raw executable path, so both are handled here.
pub fn icon_name_for(path: &str, name: Option<&str>) -> Option<String> {
    if path.ends_with(".desktop") {
        if let Some(icon) = icon_name_for_desktop_file(path) {
            return Some(icon);
        }
    }

    let index = EntryIndex::build();
    let executable = Path::new(path)
        .file_name()
        .and_then(|file| file.to_str())
        .map(|file| file.to_lowercase());
    if let Some(executable) = executable {
        if let Some(entry) = index.match_window(&executable, &executable, Some(&executable)) {
            if let Some(icon) = entry.icon.clone() {
                return Some(icon);
            }
        }
    }

    if let Some(name) = name {
        if let Some(entry) = index
            .entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
        {
            return entry.icon.clone();
        }
    }

    // Fall back to treating the file name itself as an icon name.
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
}

/// Launch a desktop file without a shell. Field codes are removed before the
/// executable and its fixed arguments are passed to `Command`.
pub fn launch(desktop_path: &str) -> Result<(), String> {
    let contents =
        fs::read_to_string(desktop_path).map_err(|e| format!("Cannot read desktop entry: {e}"))?;
    let exec = desktop_value(&contents, "Exec").ok_or("Desktop entry has no Exec field")?;
    // Keep `env` prefixes and their assignments; only field codes are dropped
    // because Bloom does not open documents through the launcher.
    let args: Vec<String> = split_exec(&exec)
        .into_iter()
        .filter(|token| !is_field_code(token))
        .collect();
    let (program, arguments) = args
        .split_first()
        .ok_or("Desktop entry has an empty Exec field")?;
    Command::new(program)
        .args(arguments)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not launch {program}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: &str = "[Desktop Entry]\nType=Application\nName=Example\nExec=/usr/bin/example --flag %U\nIcon=example\nStartupWMClass=Example;Example2\n";

    fn write_entry(name: &str, contents: &str) -> PathBuf {
        let path = env::temp_dir().join(name);
        fs::write(&path, contents).expect("write desktop entry");
        path
    }

    #[test]
    fn reads_keys_only_from_the_desktop_entry_group() {
        let contents = "[Desktop Action New]\nName=New Window\n\n[Desktop Entry]\nName=Editor\n";
        assert_eq!(desktop_value(contents, "Name").as_deref(), Some("Editor"));
    }

    #[test]
    fn exec_program_skips_field_codes_and_environment_assignments() {
        assert_eq!(
            exec_program("env GTK_THEME=dark /usr/bin/example --flag %U").as_deref(),
            Some("/usr/bin/example")
        );
        assert_eq!(exec_program("%U"), None);
        assert_eq!(exec_program("FOO=bar /usr/bin/example").as_deref(), Some("/usr/bin/example"));
    }

    #[test]
    fn quoted_arguments_stay_a_single_token() {
        assert_eq!(
            split_exec(r#""/opt/My App/app" --flag %U"#),
            vec!["/opt/My App/app", "--flag", "%U"]
        );
    }

    #[test]
    fn parses_a_desktop_entry_into_a_launchable_record() {
        let path = write_entry("bloom-test-entry.desktop", ENTRY);
        let entry = entry_from_desktop_file(&path).expect("entry should parse");
        assert_eq!(entry.name, "Example");
        assert_eq!(entry.executable.as_deref(), Some("example"));
        assert_eq!(entry.icon.as_deref(), Some("example"));
        assert_eq!(entry.startup_wm_class, vec!["Example", "Example2"]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn hidden_entries_are_ignored() {
        let path = write_entry(
            "bloom-test-hidden.desktop",
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=hidden\nNoDisplay=true\n",
        );
        assert!(entry_from_desktop_file(&path).is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn matches_a_window_to_an_entry_by_class_then_executable() {
        let index = EntryIndex {
            entries: vec![DesktopEntry {
                path: "/usr/share/applications/example.desktop".into(),
                name: "Example".into(),
                icon: Some("example".into()),
                executable: Some("example".into()),
                startup_wm_class: vec!["Example".into()],
            }],
            by_wm_class: HashMap::from([("example".to_string(), 0)]),
            by_executable: HashMap::from([("example".to_string(), 0)]),
            by_stem: HashMap::from([("example".to_string(), 0)]),
        };
        assert!(index.match_window("Example", "example", None).is_some());
        assert!(index
            .match_window("unknown", "unknown", Some("example"))
            .is_some());
        assert!(index.match_window("unknown", "unknown", Some("other")).is_none());
    }
}
