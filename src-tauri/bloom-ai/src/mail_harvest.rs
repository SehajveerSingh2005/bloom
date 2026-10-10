//! mail-contacts.json: names and addresses from the headers of the user's
//! mail (From/To/Cc of the newest 2000 messages in Sent and 2000 in the
//! Inbox), fetched as IMAP envelopes only: no bodies. Built after a
//! successful email Test, by Settings > Refresh, or at most once a day when
//! find_contact misses. Names here came from other people's mail: outside
//! data. Addresses the user has sent to (`sent`) count as known recipients.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

const FILE: &str = "mail-contacts.json";
/// When the last harvest was tried (unix seconds), success or not.
const TRIED: &str = "mail-contacts.at";
const PER_FOLDER: u32 = 2000;
const DAY: i64 = 24 * 3600;
const CORRUPT: &str = "mail-contacts.json is not valid JSON; delete it to scan your mail again";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MailContact {
    pub name: String,
    pub email: String,
    /// The user has written to this address.
    pub sent: bool,
    pub count: u32,
    /// Unix seconds of the newest message seen with it.
    pub last: i64,
}

/// One address as an IMAP envelope gives it (raw bytes). A group start or
/// end marker has no host.
#[derive(Clone, Debug, Default)]
pub struct Addr {
    pub name: Option<Vec<u8>>,
    pub mailbox: Option<Vec<u8>>,
    pub host: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Default)]
pub struct Envelope {
    pub date: Option<Vec<u8>>,
    pub from: Vec<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
}

/// The mail server, behind a trait so tests use a fake.
pub trait Mailbox {
    /// Envelopes of the newest `n` messages in Sent (`sent`) or the Inbox.
    fn envelopes(&mut self, sent: bool, n: u32) -> Result<Vec<Envelope>, String>;
}

/// The harvest; missing is empty, unreadable is an error.
pub fn load(dir: &Path) -> Result<Vec<MailContact>, String> {
    match std::fs::read_to_string(dir.join(FILE)) {
        Ok(c) => serde_json::from_str(&c).map_err(|_| CORRUPT.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("Cannot read {FILE}: {e}")),
    }
}

/// Whether a find_contact miss may scan the mail now (never tried, or the
/// last try is a day old). A yes records the try, so a failing server is not
/// asked again until tomorrow.
pub fn try_now(dir: &Path, now: i64) -> bool {
    let due = std::fs::read_to_string(dir.join(TRIED))
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .is_none_or(|t| now - t >= DAY);
    if due {
        mark(dir, now);
    }
    due
}

/// Records a scan (a Test or Refresh one too), so a miss later the same day
/// does not scan again.
pub fn mark(dir: &Path, now: i64) {
    let _ = std::fs::write(dir.join(TRIED), now.to_string());
}

/// Whether a scan has ever been saved.
pub fn exists(dir: &Path) -> bool {
    dir.join(FILE).exists()
}

/// Addresses that only send automated mail.
pub fn is_noreply(address: &str) -> bool {
    let local = address.split('@').next().unwrap_or_default();
    [
        "noreply",
        "no-reply",
        "no_reply",
        "donotreply",
        "do-not-reply",
        "do_not_reply",
        "notification",
        "mailer-daemon",
        "postmaster",
        "bounce",
        "newsletter",
        "unsubscribe",
    ]
    .iter()
    .any(|k| local.contains(k))
}

/// RFC 2047 encoded words ("=?UTF-8?B?...?=", "=?iso-8859-1?Q?...?=") in a
/// display name, decoded. Whitespace between two encoded words is dropped.
/// Hand-rolled: base64 is already a dependency, Q encoding is a few lines,
/// and names only need UTF-8 and Latin-1 (others decode as lossy UTF-8).
pub fn decode_words(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    let mut after_word = false;
    while let Some(start) = rest.find("=?") {
        let decoded = decode_one(&rest[start..]);
        let Some((text, used)) = decoded else {
            out.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            after_word = false;
            continue;
        };
        let gap = &rest[..start];
        if !(after_word && gap.trim().is_empty()) {
            out.push_str(gap);
        }
        out.push_str(&text);
        rest = &rest[start + used..];
        after_word = true;
    }
    out.push_str(rest);
    out
}

/// One encoded word at the start of `s`: (text, bytes used).
fn decode_one(s: &str) -> Option<(String, usize)> {
    let body = s.strip_prefix("=?")?;
    let (charset, body) = body.split_once('?')?;
    let (enc, body) = body.split_once('?')?;
    let end = body.find("?=")?;
    let text = &body[..end];
    if text.contains(char::is_whitespace) {
        return None;
    }
    let bytes = match enc.to_ascii_uppercase().as_str() {
        "B" => {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(text)
                .ok()?
        }
        "Q" => {
            let mut out = Vec::new();
            let b = text.as_bytes();
            let mut i = 0;
            while i < b.len() {
                match b[i] {
                    b'_' => out.push(b' '),
                    b'=' => {
                        let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                        out.push(u8::from_str_radix(hex, 16).ok()?);
                        i += 2;
                    }
                    c => out.push(c),
                }
                i += 1;
            }
            out
        }
        _ => return None,
    };
    let used = 2 + charset.len() + 1 + enc.len() + 1 + end + 2;
    let charset = charset
        .split('*')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let text = match charset.as_str() {
        "iso-8859-1" | "latin1" | "windows-1252" | "us-ascii" => {
            bytes.iter().map(|&b| b as char).collect()
        }
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    };
    Some((text, used))
}

fn date(raw: &[u8]) -> i64 {
    let s = String::from_utf8_lossy(raw);
    // "... +0000 (UTC)": the comment is not part of RFC 2822's grammar here.
    let s = s.split(" (").next().unwrap_or_default().trim();
    chrono::DateTime::parse_from_rfc2822(s).map_or(0, |d| d.timestamp())
}

/// The people in these envelopes: deduped by address, the most common display
/// name kept, automated senders and the user's own address left out, sent-to
/// first and then by how often they appear.
pub fn collect(sent: &[Envelope], inbox: &[Envelope], own: &str) -> Vec<MailContact> {
    let own = own.trim().to_lowercase();
    // address -> (name counts, sent, count, last)
    let mut seen: HashMap<String, (HashMap<String, u32>, bool, u32, i64)> = HashMap::new();
    let folders = [(sent, true), (inbox, false)];
    for (envelopes, in_sent) in folders {
        for env in envelopes {
            let at = env.date.as_deref().map_or(0, date);
            let fields = [(&env.from, false), (&env.to, in_sent), (&env.cc, in_sent)];
            for (addrs, wrote) in fields {
                for a in addrs {
                    let (Some(mailbox), Some(host)) = (&a.mailbox, &a.host) else {
                        continue; // a group marker
                    };
                    let email = format!(
                        "{}@{}",
                        String::from_utf8_lossy(mailbox),
                        String::from_utf8_lossy(host)
                    )
                    .to_lowercase();
                    if email == own || is_noreply(&email) || !crate::email::is_email(&email) {
                        continue;
                    }
                    let name = a
                        .name
                        .as_deref()
                        .map(|n| decode_words(&String::from_utf8_lossy(n)))
                        .map(|n| n.trim().trim_matches(['"', '\'']).trim().to_string())
                        .filter(|n| !n.is_empty() && !n.contains('@'))
                        .unwrap_or_default();
                    let entry = seen.entry(email).or_default();
                    if !name.is_empty() {
                        *entry.0.entry(name).or_default() += 1;
                    }
                    entry.1 |= wrote;
                    entry.2 += 1;
                    entry.3 = entry.3.max(at);
                }
            }
        }
    }
    let mut out: Vec<MailContact> = seen
        .into_iter()
        .map(|(email, (names, sent, count, last))| {
            let mut names: Vec<(String, u32)> = names.into_iter().collect();
            names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            MailContact {
                name: names.into_iter().next().map(|n| n.0).unwrap_or_default(),
                email,
                sent,
                count,
                last,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.sent
            .cmp(&a.sent)
            .then(b.count.cmp(&a.count))
            .then(a.email.cmp(&b.email))
    });
    out
}

/// Scans the mailbox and saves the result. An unreadable mail-contacts.json
/// is never overwritten.
pub fn run(dir: &Path, mailbox: &mut dyn Mailbox, own: &str) -> Result<usize, String> {
    load(dir)?;
    // No Sent folder: the Inbox alone still helps. Any other failure stops
    // the scan and leaves the last result as it is.
    let sent = match mailbox.envelopes(true, PER_FOLDER) {
        Ok(sent) => sent,
        Err(e) if e == crate::imap_lookup::NO_SENT => Vec::new(),
        Err(e) => return Err(e),
    };
    let inbox = mailbox.envelopes(false, PER_FOLDER)?;
    let found = collect(&sent, &inbox, own);
    let json = serde_json::to_string_pretty(&found).map_err(|e| e.to_string())?;
    crate::people::write_file(dir, FILE, &json)?;
    Ok(found.len())
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Held while a scan runs: one at a time (a Refresh during a background
/// refresh, say, is turned away).
pub struct Running(());

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn start() -> Result<Running, String> {
    use std::sync::atomic::Ordering::SeqCst;
    RUNNING
        .compare_exchange(false, true, SeqCst, SeqCst)
        .map(|_| Running(()))
        .map_err(|_| "A mail scan is already running.".to_string())
}

/// The harvest as people for lookups: source "mail", sent-to ranked above
/// received-only, then by count.
pub fn people(dir: &Path) -> Vec<crate::people::Person> {
    use crate::people::{Person, MAIL};
    load(dir)
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let name = if m.name.is_empty() {
                m.email.split('@').next().unwrap_or_default().to_string()
            } else {
                m.name
            };
            let mut p = Person::new(&name, MAIL);
            p.add_email(&m.email, if m.sent { "sent" } else { "received" }, MAIL);
            p.weight = if m.sent { 1_000_000 } else { 0 } + m.count.min(999_999);
            p
        })
        .collect()
}

/// An address the user has written to, from the harvest.
pub fn sent_to(dir: &Path, address: &str) -> bool {
    load(dir)
        .unwrap_or_default()
        .iter()
        .any(|m| m.sent && m.email.eq_ignore_ascii_case(address))
}

/// The real server: envelopes over IMAP.
pub struct Imap<T: std::io::Read + std::io::Write>(pub imap::Session<T>);

impl<T: std::io::Read + std::io::Write> Mailbox for Imap<T> {
    fn envelopes(&mut self, sent: bool, n: u32) -> Result<Vec<Envelope>, String> {
        let folder = if sent {
            crate::imap_lookup::sent_folder(&mut self.0)?
        } else {
            "INBOX".to_string()
        };
        let total = self.0.examine(&folder).map_err(|e| e.to_string())?.exists;
        if total == 0 {
            return Ok(Vec::new());
        }
        let first = total.saturating_sub(n.saturating_sub(1)).max(1);
        let fetches = self
            .0
            .fetch(format!("{first}:{total}"), "ENVELOPE")
            .map_err(|e| e.to_string())?;
        // imap's address type lives in imap-proto, which is not a direct
        // dependency, so it is never named.
        macro_rules! owned {
            ($list:expr) => {
                $list
                    .iter()
                    .flatten()
                    .map(|a| Addr {
                        name: a.name.map(<[u8]>::to_vec),
                        mailbox: a.mailbox.map(<[u8]>::to_vec),
                        host: a.host.map(<[u8]>::to_vec),
                    })
                    .collect::<Vec<Addr>>()
            };
        }
        Ok(fetches
            .iter()
            .filter_map(|f| f.envelope())
            .map(|e| Envelope {
                date: e.date.map(<[u8]>::to_vec),
                from: owned!(e.from),
                to: owned!(e.to),
                cc: owned!(e.cc),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    fn addr(name: Option<&str>, email: &str) -> Addr {
        let (m, h) = email.split_once('@').unwrap();
        Addr {
            name: name.map(|n| n.as_bytes().to_vec()),
            mailbox: Some(m.as_bytes().to_vec()),
            host: Some(h.as_bytes().to_vec()),
        }
    }

    fn group_marker(name: Option<&str>) -> Addr {
        Addr {
            name: None,
            mailbox: name.map(|n| n.as_bytes().to_vec()),
            host: None,
        }
    }

    fn env(date: &str, from: Vec<Addr>, to: Vec<Addr>, cc: Vec<Addr>) -> Envelope {
        Envelope {
            date: Some(date.as_bytes().to_vec()),
            from,
            to,
            cc,
        }
    }

    /// A fake IMAP server: canned envelopes per folder, counting calls.
    struct Fake {
        sent: Result<Vec<Envelope>, String>,
        inbox: Result<Vec<Envelope>, String>,
        calls: usize,
    }

    impl Mailbox for Fake {
        fn envelopes(&mut self, sent: bool, n: u32) -> Result<Vec<Envelope>, String> {
            assert_eq!(n, 2000);
            self.calls += 1;
            if sent {
                self.sent.clone()
            } else {
                self.inbox.clone()
            }
        }
    }

    const ME: &str = "me@gmail.com";
    const D1: &str = "Tue, 6 Oct 2026 10:00:00 +0000";
    const D2: &str = "Wed, 7 Oct 2026 10:00:00 +0000 (UTC)";

    fn fake() -> Fake {
        let sent = vec![
            env(
                D1,
                vec![addr(Some("Me"), ME)],
                vec![addr(Some("=?UTF-8?B?Sm9zw6kgTsO6w7Fleg==?="), "jose@x.com")],
                vec![],
            ),
            env(
                D2,
                vec![addr(None, ME)],
                vec![
                    group_marker(Some("Team")),
                    addr(Some("Neha A"), "Neha@Example.com"),
                    group_marker(None),
                ],
                vec![addr(None, "sam@x.org")],
            ),
        ];
        let inbox = vec![
            env(
                D2,
                vec![addr(Some("Neha Aggarwal"), "neha@example.com")],
                vec![addr(None, ME)],
                vec![],
            ),
            env(
                D1,
                vec![addr(Some("Neha Aggarwal"), "neha@example.com")],
                vec![addr(None, ME)],
                vec![],
            ),
            env(
                D1,
                vec![addr(Some("Shop"), "no-reply@shop.com")],
                vec![addr(None, ME)],
                vec![],
            ),
            env(
                D1,
                vec![addr(Some("GitHub"), "notifications@github.com")],
                vec![addr(None, ME)],
                vec![],
            ),
            env(
                D1,
                vec![addr(
                    Some("=?iso-8859-1?Q?Ren=E9e_Dupr=E9?="),
                    "renee@fr.fr",
                )],
                vec![addr(None, ME)],
                vec![addr(Some("Sam Lee"), "sam@x.org")],
            ),
            env(
                "garbage",
                vec![addr(Some("\"Lou\""), "lou@x.com")],
                vec![],
                vec![],
            ),
        ];
        Fake {
            sent: Ok(sent),
            inbox: Ok(inbox),
            calls: 0,
        }
    }

    #[test]
    fn decodes_encoded_word_names() {
        assert_eq!(
            decode_words("=?UTF-8?B?Sm9zw6kgTsO6w7Fleg==?="),
            "José Núñez"
        );
        assert_eq!(
            decode_words("=?iso-8859-1?Q?Ren=E9e_Dupr=E9?="),
            "Renée Dupré"
        );
        assert_eq!(
            decode_words("=?utf-8?q?Ana?= =?utf-8?q?_Lima?= (work)"),
            "Ana Lima (work)"
        );
        assert_eq!(decode_words("Plain =?bad"), "Plain =?bad");
        assert_eq!(decode_words("=?UTF-8?X?abc?="), "=?UTF-8?X?abc?=");
    }

    #[test]
    fn harvest_dedupes_ranks_and_filters() {
        let dir = temp_dir();
        let mut mb = fake();
        assert_eq!(run(&dir, &mut mb, "ME@gmail.com").unwrap(), 5);
        let found = load(&dir).unwrap();
        let rows: Vec<(&str, &str, bool, u32)> = found
            .iter()
            .map(|m| (m.email.as_str(), m.name.as_str(), m.sent, m.count))
            .collect();
        assert_eq!(
            rows,
            [
                // Sent-to first, then by count; the most common name wins.
                ("neha@example.com", "Neha Aggarwal", true, 3),
                ("sam@x.org", "Sam Lee", true, 2),
                ("jose@x.com", "José Núñez", true, 1),
                ("lou@x.com", "Lou", false, 1),
                ("renee@fr.fr", "Renée Dupré", false, 1),
            ]
        );
        let neha = &found[0];
        assert_eq!(
            neha.last,
            chrono::DateTime::parse_from_rfc2822("Wed, 7 Oct 2026 10:00:00 +0000")
                .unwrap()
                .timestamp()
        );
        assert_eq!(found[3].last, 0, "an unreadable date");
        assert!(
            is_noreply("no-reply@x.com")
                && is_noreply("mailer-daemon@x.com")
                && !is_noreply("noel@x.com")
        );
        // Known recipients: only addresses the user wrote to.
        assert!(sent_to(&dir, "NEHA@example.com") && !sent_to(&dir, "renee@fr.fr"));
        let people = people(&dir);
        assert_eq!(people[0].emails[0].source, crate::people::MAIL);
        assert!(people[0].weight > people[3].weight);
    }

    #[test]
    fn once_a_day_and_a_failed_server_waits_too() {
        let dir = temp_dir();
        assert!(try_now(&dir, 1000));
        assert!(
            !try_now(&dir, 1000 + DAY - 1),
            "tried today, even if it failed"
        );
        assert!(try_now(&dir, 1000 + DAY));
        let mut mb = fake();
        mb.inbox = Err("Can't reach imap.gmail.com".into());
        assert!(run(&dir, &mut mb, ME).is_err());
        assert!(!dir.join(FILE).exists());
        // No Sent folder: the Inbox still counts.
        let mut mb = fake();
        mb.sent = Err(crate::imap_lookup::NO_SENT.into());
        assert_eq!(run(&dir, &mut mb, ME).unwrap(), 4);
        assert_eq!(mb.calls, 2);
        // Any other Sent failure stops the scan; the last result stays.
        let before = std::fs::read_to_string(dir.join(FILE)).unwrap();
        let mut mb = fake();
        mb.sent = Err("Can't reach imap.gmail.com: timed out".into());
        assert!(run(&dir, &mut mb, ME).unwrap_err().contains("timed out"));
        assert_eq!(mb.calls, 1, "the Inbox is not fetched");
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), before);
        // A Test or Refresh scan counts as today's.
        mark(&dir, 5 * DAY);
        assert!(!try_now(&dir, 5 * DAY + 60));
    }

    #[test]
    fn a_corrupt_file_is_never_overwritten() {
        let dir = temp_dir();
        std::fs::write(dir.join(FILE), "[{").unwrap();
        let mut mb = fake();
        assert!(run(&dir, &mut mb, ME)
            .unwrap_err()
            .contains("not valid JSON"));
        assert_eq!(mb.calls, 0, "no mail fetched");
        assert_eq!(std::fs::read_to_string(dir.join(FILE)).unwrap(), "[{");
        assert!(people(&dir).is_empty() && !sent_to(&dir, "a@b.com"));
    }
}
