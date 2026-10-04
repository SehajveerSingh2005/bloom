//! Credentials in Windows Credential Manager. Only these names exist, so a
//! wipe knows exactly what to delete.

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
    for (i, p) in parts.iter().enumerate() {
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
    fn long_values_are_chunked_and_leave_no_stale_parts() {
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
    fn unknown_names_are_refused() {
        assert!(set("anything", "x").is_err());
    }
}
