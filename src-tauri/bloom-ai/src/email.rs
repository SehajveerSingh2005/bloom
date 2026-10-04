//! Email: contacts the agent has learned, mail-server presets, SMTP sending.

use crate::config::Config;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    pub smtp: String,
    pub port: u16,
    pub imap: String,
    /// Microsoft accounts: XOAUTH2 with a token from outlook.rs, no password.
    pub oauth: bool,
}

fn server(smtp: &str, port: u16, imap: &str, oauth: bool) -> Server {
    Server {
        smtp: smtp.into(),
        port,
        imap: imap.into(),
        oauth,
    }
}

/// Known providers by the address's domain.
pub fn preset(address: &str) -> Option<Server> {
    let domain = address.rsplit_once('@')?.1.to_ascii_lowercase();
    Some(match domain.as_str() {
        "gmail.com" | "googlemail.com" => server("smtp.gmail.com", 465, "imap.gmail.com", false),
        "yahoo.com" | "ymail.com" => {
            server("smtp.mail.yahoo.com", 465, "imap.mail.yahoo.com", false)
        }
        "icloud.com" | "me.com" | "mac.com" => {
            server("smtp.mail.me.com", 587, "imap.mail.me.com", false)
        }
        "zoho.com" => server("smtp.zoho.com", 465, "imap.zoho.com", false),
        "fastmail.com" => server("smtp.fastmail.com", 465, "imap.fastmail.com", false),
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" => {
            server("smtp-mail.outlook.com", 587, "outlook.office365.com", true)
        }
        _ => return None,
    })
}

/// The sender's server: Settings' host/port if given, else the preset.
pub fn server_for(cfg: &Config) -> Result<Server, String> {
    if cfg.email.is_empty() {
        return Err("Set up email in Settings > AI first.".into());
    }
    let preset = preset(&cfg.email);
    if !cfg.smtp_host.is_empty() {
        let port = if cfg.smtp_port == 0 {
            465
        } else {
            cfg.smtp_port
        };
        let imap = preset
            .as_ref()
            .map(|p| p.imap.clone())
            .unwrap_or_else(|| cfg.smtp_host.replacen("smtp.", "imap.", 1));
        let oauth = preset.is_some_and(|p| p.oauth);
        return Ok(server(&cfg.smtp_host, port, &imap, oauth));
    }
    preset.ok_or_else(|| {
        format!(
            "Bloom doesn't know the mail server for {}. Add it in Settings > AI.",
            cfg.email
        )
    })
}

pub fn is_email(s: &str) -> bool {
    let s = s.trim();
    match s.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !s.contains(char::is_whitespace)
        }
        None => false,
    }
}

/// contacts.json: display name to address.
pub fn load_contacts(dir: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(dir.join("contacts.json"))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn save_contact(dir: &Path, name: &str, address: &str) -> Result<(), String> {
    let address = address.trim().to_lowercase();
    if !is_email(&address) {
        return Err(format!("{address} is not an email address"));
    }
    let mut contacts = load_contacts(dir);
    contacts.insert(name.trim().to_string(), address);
    let json = serde_json::to_string_pretty(&contacts).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("contacts.json"), json).map_err(|e| e.to_string())
}

/// Contacts whose name or address contains every word of the query.
pub fn find(contacts: &BTreeMap<String, String>, query: &str) -> Vec<(String, String)> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    contacts
        .iter()
        .filter(|(name, address)| {
            let hay = format!("{} {}", name.to_lowercase(), address);
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|(n, a)| (n.clone(), a.clone()))
        .collect()
}

pub fn is_known(contacts: &BTreeMap<String, String>, address: &str) -> bool {
    contacts.values().any(|a| a.eq_ignore_ascii_case(address))
}

/// Blocking: call from spawn_blocking.
pub fn send(
    server: &Server,
    from: &str,
    secret: &str,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    use lettre::message::header::ContentType;
    use lettre::transport::smtp::authentication::{Credentials, Mechanism};
    use lettre::{Message, SmtpTransport, Transport};

    let message = Message::builder()
        .from(
            from.parse()
                .map_err(|e| format!("bad sender address: {e}"))?,
        )
        .to(to
            .parse()
            .map_err(|e| format!("bad recipient address: {e}"))?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| e.to_string())?;
    // 465 is TLS from the first byte; anything else (587) upgrades with STARTTLS.
    let builder = if server.port == 465 {
        SmtpTransport::relay(&server.smtp)
    } else {
        SmtpTransport::starttls_relay(&server.smtp)
    }
    .map_err(|e| e.to_string())?;
    let mechanism = if server.oauth {
        Mechanism::Xoauth2
    } else {
        Mechanism::Plain
    };
    builder
        .port(server.port)
        .credentials(Credentials::new(from.to_string(), secret.to_string()))
        .authentication(vec![mechanism])
        .build()
        .send(&message)
        .map(|_| ())
        .map_err(|e| format!("Sending failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;
    use std::collections::HashMap;

    #[test]
    fn email_shapes() {
        assert!(is_email("neha@example.com"));
        for bad in [
            "neha",
            "neha@",
            "@x.com",
            "a@b",
            "a@.com",
            "a@b.",
            "a b@c.com",
            "a@b@c.com",
        ] {
            assert!(!is_email(bad), "{bad}");
        }
    }

    #[test]
    fn contacts_round_trip_and_search() {
        let dir = temp_dir();
        save_contact(&dir, "Neha Aggarwal", " Neha@Example.com ").unwrap();
        save_contact(&dir, "Sam", "sam@x.org").unwrap();
        assert!(save_contact(&dir, "Bad", "nope").is_err());
        let contacts = load_contacts(&dir);
        assert_eq!(
            find(&contacts, "neha aggarwal"),
            vec![("Neha Aggarwal".into(), "neha@example.com".into())]
        );
        assert_eq!(find(&contacts, "NEHA").len(), 1);
        assert!(find(&contacts, "neha sam").is_empty());
        assert!(find(&contacts, "").is_empty());
        assert!(is_known(&contacts, "NEHA@example.com"));
        assert!(!is_known(&contacts, "x@y.com"));
    }

    #[test]
    fn presets_and_overrides() {
        assert_eq!(preset("me@gmail.com").unwrap().smtp, "smtp.gmail.com");
        assert!(preset("me@hotmail.com").unwrap().oauth);
        assert_eq!(preset("me@mycompany.com"), None);

        let cfg = |pairs: &[(&str, &str)]| {
            let map: HashMap<String, serde_json::Value> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), serde_json::json!(v)))
                .collect();
            Config::from_map(&map)
        };
        assert!(server_for(&cfg(&[])).is_err());
        assert!(server_for(&cfg(&[("bloom-ai-email", "me@mycompany.com")])).is_err());
        let custom = server_for(&cfg(&[
            ("bloom-ai-email", "me@mycompany.com"),
            ("bloom-ai-smtp-host", "smtp.mycompany.com"),
            ("bloom-ai-smtp-port", "587"),
        ]))
        .unwrap();
        assert_eq!(
            custom,
            server("smtp.mycompany.com", 587, "imap.mycompany.com", false)
        );
    }
}
