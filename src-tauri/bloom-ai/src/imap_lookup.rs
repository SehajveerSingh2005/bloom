//! Finds addresses in the user's Sent folder, for people the agent hasn't
//! saved yet but the user has emailed before.

use crate::email::Server;
use imap::types::NameAttribute;

struct XOAuth2<'a> {
    user: &'a str,
    token: &'a str,
}

impl imap::Authenticator for XOAuth2<'_> {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> String {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

pub type Session = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

pub const NO_SENT: &str = "No Sent folder found.";

/// A silent server fails a read or write after this, instead of hanging.
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Blocking: a logged-in IMAP session (port 993, TLS) whose socket times out.
pub fn connect(server: &Server, user: &str, secret: &str) -> Result<Session, String> {
    use std::net::ToSocketAddrs;
    let host = server.imap.as_str();
    let unreachable = |e: &dyn std::fmt::Display| format!("Can't reach {host}: {e}");
    let addr = (host, 993)
        .to_socket_addrs()
        .map_err(|e| unreachable(&e))?
        .next()
        .ok_or_else(|| unreachable(&"no address"))?;
    let tcp =
        std::net::TcpStream::connect_timeout(&addr, IO_TIMEOUT).map_err(|e| unreachable(&e))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| unreachable(&e))?;
    tcp.set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| unreachable(&e))?;
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let stream = tls.connect(host, tcp).map_err(|e| unreachable(&e))?;
    let mut client = imap::Client::new(stream);
    client.read_greeting().map_err(|e| unreachable(&e))?;
    if server.oauth {
        client
            .authenticate(
                "XOAUTH2",
                &XOAuth2 {
                    user,
                    token: secret,
                },
            )
            .map_err(|(e, _)| e.to_string())
    } else {
        client.login(user, secret).map_err(|(e, _)| e.to_string())
    }
}

/// Blocking: call from spawn_blocking. Returns (display name, address) pairs
/// from the 20 most recent sent messages that match.
pub fn sent_to(
    server: &Server,
    user: &str,
    secret: &str,
    query: &str,
) -> Result<Vec<(String, String)>, String> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    let Some(first) = words.first() else {
        return Ok(Vec::new());
    };
    let mut session = connect(server, user, secret)?;
    let result = search(&mut session, first, &words);
    let _ = session.logout();
    result
}

/// The Sent folder's name: the one flagged \Sent, else a usual name.
pub fn sent_folder<T: std::io::Read + std::io::Write>(
    session: &mut imap::Session<T>,
) -> Result<String, String> {
    let folders = session
        .list(Some(""), Some("*"))
        .map_err(|e| e.to_string())?;
    folders
        .iter()
        .find(|f| {
            f.attributes()
                .iter()
                .any(|a| matches!(a, NameAttribute::Custom(c) if c.eq_ignore_ascii_case("\\Sent")))
        })
        .or_else(|| {
            folders.iter().find(|f| {
                matches!(
                    f.name().to_lowercase().as_str(),
                    "sent" | "sent items" | "sent mail" | "[gmail]/sent mail"
                )
            })
        })
        .map(|f| f.name().to_string())
        .ok_or_else(|| NO_SENT.to_string())
}

fn search<T: std::io::Read + std::io::Write>(
    session: &mut imap::Session<T>,
    first: &str,
    words: &[String],
) -> Result<Vec<(String, String)>, String> {
    let sent = sent_folder(session)?;
    session.examine(&sent).map_err(|e| e.to_string())?;
    let safe: String = first.chars().filter(|c| !matches!(c, '"' | '\\')).collect();
    let mut ids: Vec<u32> = session
        .search(format!("TO \"{safe}\""))
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    ids.sort_unstable();
    let recent: Vec<String> = ids.iter().rev().take(20).map(u32::to_string).collect();
    if recent.is_empty() {
        return Ok(Vec::new());
    }
    let fetches = session
        .fetch(recent.join(","), "ENVELOPE")
        .map_err(|e| e.to_string())?;
    let mut found: Vec<(String, String)> = Vec::new();
    for fetch in fetches.iter() {
        let Some(envelope) = fetch.envelope() else {
            continue;
        };
        for address in envelope.to.iter().flatten() {
            let (Some(mailbox), Some(host)) = (address.mailbox, address.host) else {
                continue;
            };
            let addr = format!(
                "{}@{}",
                String::from_utf8_lossy(mailbox),
                String::from_utf8_lossy(host)
            )
            .to_lowercase();
            let name = address
                .name
                .map(|n| crate::mail_harvest::decode_words(&String::from_utf8_lossy(n)))
                .unwrap_or_default();
            if matches_all(&name, &addr, words) && !found.iter().any(|(_, a)| a == &addr) {
                found.push((name, addr));
            }
        }
    }
    Ok(found)
}

/// Every query word appears in the name or the address.
pub fn matches_all(name: &str, address: &str, words: &[String]) -> bool {
    let hay = format!("{} {}", name.to_lowercase(), address);
    words.iter().all(|w| hay.contains(w.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_words_must_match() {
        let words = vec!["neha".to_string(), "aggarwal".to_string()];
        assert!(matches_all("Neha Aggarwal", "neha@example.com", &words));
        assert!(matches_all("", "neha.aggarwal@example.com", &words));
        assert!(!matches_all("Neha Sharma", "neha@example.com", &words));
    }
}
