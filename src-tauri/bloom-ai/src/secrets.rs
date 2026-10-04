//! Credentials in Windows Credential Manager. Only these names exist, so a
//! wipe knows exactly what to delete.

/// Tests use their own service so they never touch real keys.
#[cfg(not(test))]
const SERVICE: &str = "bloom-ai";
#[cfg(test)]
const SERVICE: &str = "bloom-ai-test";

pub const NAMES: [&str; 4] = ["llm-key", "stt-key", "email-password", "outlook-refresh"];

pub fn get(name: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, name).ok()?.get_password().ok()
}

/// Saves a credential; an empty value deletes it.
pub fn set(name: &str, value: &str) -> Result<(), String> {
    if !NAMES.contains(&name) {
        return Err(format!("unknown secret {name}"));
    }
    let entry = keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())?;
    if value.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(value).map_err(|e| e.to_string())
}

pub fn wipe() {
    for name in NAMES {
        if let Ok(entry) = keyring::Entry::new(SERVICE, name) {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_wipe_round_trip() {
        set("stt-key", "abc").unwrap();
        assert_eq!(get("stt-key").as_deref(), Some("abc"));
        set("stt-key", "").unwrap();
        assert_eq!(get("stt-key"), None);
        set("llm-key", "k").unwrap();
        wipe();
        assert_eq!(get("llm-key"), None);
    }

    #[test]
    fn unknown_names_are_refused() {
        assert!(set("anything", "x").is_err());
    }
}
