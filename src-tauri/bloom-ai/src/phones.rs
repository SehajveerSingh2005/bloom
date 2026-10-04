//! Phone contacts: phones.json maps a display name to an E.164 number.

use std::collections::BTreeMap;
use std::path::Path;

/// (ISO region, calling code) for the most common regions.
#[rustfmt::skip]
const CODES: &[(&str, &str)] = &[
    ("US", "1"), ("CA", "1"), ("GB", "44"), ("IE", "353"), ("DE", "49"), ("FR", "33"),
    ("ES", "34"), ("IT", "39"), ("PT", "351"), ("NL", "31"), ("BE", "32"), ("CH", "41"),
    ("AT", "43"), ("SE", "46"), ("NO", "47"), ("DK", "45"), ("FI", "358"), ("PL", "48"),
    ("CZ", "420"), ("GR", "30"), ("RO", "40"), ("HU", "36"), ("UA", "380"), ("RU", "7"),
    ("TR", "90"), ("IL", "972"), ("SA", "966"), ("AE", "971"), ("EG", "20"), ("ZA", "27"),
    ("NG", "234"), ("KE", "254"), ("MA", "212"), ("IN", "91"), ("PK", "92"), ("BD", "880"),
    ("LK", "94"), ("NP", "977"), ("CN", "86"), ("HK", "852"), ("TW", "886"), ("JP", "81"),
    ("KR", "82"), ("SG", "65"), ("MY", "60"), ("TH", "66"), ("VN", "84"), ("ID", "62"),
    ("PH", "63"), ("AU", "61"), ("NZ", "64"), ("MX", "52"), ("BR", "55"), ("AR", "54"),
    ("CL", "56"), ("CO", "57"), ("PE", "51"), ("VE", "58"), ("IR", "98"), ("IQ", "964"),
];

/// ISO region of the Windows user profile, e.g. "DE".
fn system_region() -> Option<String> {
    #[cfg(windows)]
    {
        let mut buf = [0u16; 16];
        // SAFETY: the buffer outlives the call and its length is passed.
        let n = unsafe { windows::Win32::Globalization::GetUserDefaultGeoName(&mut buf) };
        if n > 1 {
            return Some(String::from_utf16_lossy(&buf[..n as usize - 1]));
        }
    }
    None
}

/// E.164 with the Windows region's calling code for national numbers.
pub fn normalize(raw: &str) -> Result<String, String> {
    normalize_in(raw, system_region().as_deref())
}

pub fn normalize_in(raw: &str, region: Option<&str>) -> Result<String, String> {
    let raw = raw.trim();
    let plus = raw.starts_with('+');
    if raw
        .chars()
        .any(|c| !(c.is_ascii_digit() || " -().+".contains(c)))
        || raw.rfind('+').is_some_and(|i| i > 0)
    {
        return Err(format!("{raw} is not a phone number"));
    }
    let mut digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let full = if plus {
        digits
    } else if let Some(rest) = digits.strip_prefix("00") {
        rest.to_string()
    } else {
        let code = region
            .and_then(|r| CODES.iter().find(|(c, _)| c.eq_ignore_ascii_case(r)))
            .map(|(_, code)| *code)
            .ok_or_else(|| {
                format!(
                    "Cannot tell the country for {raw}. Ask for the full number with country \
                     code, like +49 170 1234567."
                )
            })?;
        let region = region.unwrap_or_default();
        if digits.starts_with(code) && digits.len() >= code.len() + 7 {
            // Already carries the country code, just without the plus.
            digits
        } else {
            // National trunk prefix; Italy and the Vatican keep it abroad.
            let keeps_zero = ["IT", "SM", "VA"]
                .iter()
                .any(|r| r.eq_ignore_ascii_case(region));
            if digits.starts_with('0') && !keeps_zero {
                digits.remove(0);
            }
            format!("{code}{digits}")
        }
    };
    if !(8..=15).contains(&full.len()) || full.starts_with('0') {
        return Err(format!("{raw} is not a valid phone number"));
    }
    Ok(format!("+{full}"))
}

/// phones.json: display name to number.
pub fn load(dir: &Path) -> BTreeMap<String, String> {
    try_load(dir).unwrap_or_default()
}

/// Like `load`, but a present file that is not valid JSON is an error.
pub fn try_load(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    match std::fs::read_to_string(dir.join("phones.json")) {
        Ok(c) => serde_json::from_str(&c)
            .map_err(|_| "phones.json is not valid JSON; fix or delete it".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(format!("Cannot read phones.json: {e}")),
    }
}

/// Saved numbers whose name or number contains every word of the query.
pub fn find(dir: &Path, query: &str) -> Result<Vec<(String, String)>, String> {
    Ok(crate::email::find(&try_load(dir)?, query))
}

/// The saved number if this name already has a different one.
pub fn needs_confirm(dir: &Path, name: &str, number: &str) -> Option<String> {
    crate::email::contact_needs_confirm(&load(dir), name, number)
}

/// Save an already normalised number. Never overwrites an unreadable file.
pub fn save(dir: &Path, name: &str, number: &str) -> Result<(), String> {
    let path = dir.join("phones.json");
    if path.exists() {
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("Cannot read phones.json: {e}"))?;
        if serde_json::from_str::<BTreeMap<String, String>>(&content).is_err() {
            return Err("phones.json is unreadable; fix or delete it".into());
        }
    }
    let mut phones = load(dir);
    let name = name.trim();
    phones.retain(|k, _| !k.eq_ignore_ascii_case(name));
    phones.insert(name.to_string(), number.to_string());
    let json = serde_json::to_string_pretty(&phones).map_err(|e| e.to_string())?;
    let tmp = dir.join("phones.json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    #[test]
    fn normalises_to_e164() {
        let n = |s| normalize_in(s, Some("DE"));
        assert_eq!(n("+49 (170) 123-4567").unwrap(), "+491701234567");
        assert_eq!(n("0049 170 1234567").unwrap(), "+491701234567");
        assert_eq!(n("0170 1234567").unwrap(), "+491701234567");
        assert_eq!(
            normalize_in("98765 43210", Some("in")).unwrap(),
            "+919876543210"
        );
        assert!(normalize_in("170 1234567", None)
            .unwrap_err()
            .contains("full number"));
        assert!(normalize_in("170 1234567", Some("ZZ")).is_err());
        // Italy keeps the trunk 0; a number that already has the code is not doubled.
        assert_eq!(
            normalize_in("06 1234 5678", Some("IT")).unwrap(),
            "+390612345678"
        );
        assert_eq!(
            normalize_in("1 415 555 0100", Some("US")).unwrap(),
            "+14155550100"
        );
        assert_eq!(
            normalize_in("415 555 0100", Some("US")).unwrap(),
            "+14155550100"
        );
        assert!(n("call me").is_err());
        assert!(n("12+3456789").is_err());
        assert!(n("+123").is_err());
    }

    #[test]
    fn find_and_save_round_trip() {
        let dir = temp_dir();
        save(&dir, "Neha Aggarwal", "+919876543210").unwrap();
        save(&dir, "neha aggarwal", "+919876500000").unwrap();
        assert_eq!(
            find(&dir, "NEHA").unwrap(),
            vec![("neha aggarwal".into(), "+919876500000".into())]
        );
        assert_eq!(find(&dir, "+91987").unwrap().len(), 1);
        assert_eq!(
            needs_confirm(&dir, "Neha Aggarwal", "+1555"),
            Some("+919876500000".into())
        );
        assert_eq!(needs_confirm(&dir, "Sam", "+1555"), None);
    }

    #[test]
    fn corrupt_file_is_never_overwritten() {
        let dir = temp_dir();
        std::fs::write(dir.join("phones.json"), "{bad").unwrap();
        assert!(save(&dir, "Bob", "+491701234567").is_err());
        assert!(find(&dir, "Bob").unwrap_err().contains("not valid JSON"));
        assert_eq!(
            std::fs::read_to_string(dir.join("phones.json")).unwrap(),
            "{bad"
        );
    }
}
