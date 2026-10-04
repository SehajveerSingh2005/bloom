//! Credentials in Windows Credential Manager. Only these names exist, so a
//! wipe knows exactly what to delete.
//!
//! NEVER LOSE SAVED KEYS: `wipe()` is reachable only from the `--wipe` command
//! line ("Delete AI altogether"), never from a protocol message, an update or
//! a settings change. Do not change SERVICE, NAMES or the target format. A test
//! below fails if anything else calls `wipe()`.

/// Tests use their own service so they never touch real keys.
#[cfg(not(test))]
const SERVICE: &str = "bloom-ai";
#[cfg(test)]
const SERVICE: &str = "bloom-ai-test";

pub const NAMES: [&str; 4] = ["llm-key", "stt-key", "email-password", "outlook-refresh"];

/// Credential Manager refuses values over about 1280 characters, and
/// Microsoft refresh tokens can be longer, so long values are stored in parts:
/// `name`, `name#2`, `name#3`, ...
const CHUNK: usize = 1000;
const MAX_PARTS: usize = 16;

fn part(name: &str, n: usize) -> keyring::Entry {
    let key = if n == 1 {
        name.to_string()
    } else {
        format!("{name}#{n}")
    };
    keyring::Entry::new(SERVICE, &key).expect("valid credential name")
}

pub fn get(name: &str) -> Option<String> {
    let mut value = part(name, 1).get_password().ok()?;
    for n in 2..=MAX_PARTS {
        match part(name, n).get_password() {
            Ok(more) => value.push_str(&more),
            Err(_) => break,
        }
    }
    Some(value)
}

/// Saves a credential; an empty value deletes it.
pub fn set(name: &str, value: &str) -> Result<(), String> {
    if !NAMES.contains(&name) {
        return Err(format!("unknown secret {name}"));
    }
    let chars: Vec<char> = value.chars().collect();
    let parts: Vec<String> = chars.chunks(CHUNK).map(|c| c.iter().collect()).collect();
    if parts.len() > MAX_PARTS {
        return Err(format!("secret {name} is too long"));
    }
    // Highest part first, `name` itself last: a failure partway never puts the
    // new first part in front of stale later parts.
    for (i, p) in parts.iter().enumerate().rev() {
        part(name, i + 1)
            .set_password(p)
            .map_err(|e| e.to_string())?;
    }
    // Drop leftovers of a previous, longer value (all parts when emptying).
    for n in parts.len() + 1..=MAX_PARTS {
        let _ = part(name, n).delete_credential();
    }
    Ok(())
}

/// Which credentials exist, as booleans only: a value never leaves `get`.
pub fn status() -> [bool; 4] {
    NAMES.map(|name| part(name, 1).get_password().is_ok())
}

pub fn wipe() {
    for name in NAMES {
        for n in 1..=MAX_PARTS {
            let _ = part(name, n).delete_credential();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests share one Credential Manager service and call `wipe()`.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn set_get_wipe_round_trip() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set("stt-key", "abc").unwrap();
        assert_eq!(get("stt-key").as_deref(), Some("abc"));
        set("stt-key", "").unwrap();
        assert_eq!(get("stt-key"), None);
        set("llm-key", "k").unwrap();
        wipe();
        assert_eq!(get("llm-key"), None);
    }

    #[test]
    fn long_values_are_chunked_and_leave_no_stale_parts() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let long: String = (0..3500)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        set("outlook-refresh", &long).unwrap();
        assert_eq!(get("outlook-refresh").as_deref(), Some(long.as_str()));
        set("outlook-refresh", "short").unwrap();
        assert_eq!(get("outlook-refresh").as_deref(), Some("short"));
        assert!(part("outlook-refresh", 2).get_password().is_err());
        set("outlook-refresh", &long).unwrap();
        wipe();
        assert_eq!(get("outlook-refresh"), None);
        assert!(part("outlook-refresh", 4).get_password().is_err());
        assert!(set("outlook-refresh", &"x".repeat(16 * 1000 + 1)).is_err());
    }

    #[test]
    fn status_reports_existence_only() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        wipe();
        assert_eq!(status(), [false; 4]);
        set("stt-key", "abc").unwrap();
        assert_eq!(status(), [false, true, false, false]);
        wipe();
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// Saved keys are never lost: any code mentioning `wipe` (a call, a `use`,
    /// an alias) anywhere in the agent, including this file, fails here except
    /// the definition and the `--wipe` branch of main().
    #[test]
    fn wipe_is_only_reachable_from_the_wipe_flag() {
        let mut files = Vec::new();
        rust_files(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src")), &mut files);
        let mut hits = Vec::new();
        for path in files {
            let src = std::fs::read_to_string(&path).unwrap();
            let code = src.split("#[cfg(test)]\nmod tests").next().unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut before = String::new();
            for line in code.lines() {
                let t = line.trim();
                let mentions = t.contains("wipe") && !t.starts_with("//");
                let allowed = (name == "secrets.rs" && t == "pub fn wipe() {")
                    || (name == "main.rs" && t.contains(r#"a == "--wipe""#));
                if mentions && !allowed {
                    hits.push((name.clone(), t.to_string(), before.clone()));
                }
                before.push_str(line);
                before.push('\n');
            }
        }
        assert_eq!(hits.len(), 1, "wipe referenced outside --wipe: {hits:?}");
        let (file, line, before) = &hits[0];
        assert_eq!((file.as_str(), line.as_str()), ("main.rs", "secrets::wipe();"));
        assert!(before.contains("fn main()") && !before.contains("async fn serve"));
        assert!(before.trim_end().ends_with(r#"if args.iter().any(|a| a == "--wipe") {"#));
    }

    #[test]
    fn status_reports_chunked_values() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set("outlook-refresh", &"x".repeat(2500)).unwrap();
        assert_eq!(status(), [false, false, false, true]);
        wipe();
    }

    #[test]
    fn unknown_names_are_refused() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert!(set("anything", "x").is_err());
    }
}
