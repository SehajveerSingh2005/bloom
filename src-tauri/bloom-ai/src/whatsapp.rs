//! WhatsApp as a linked device (shown as "Bloom" in WhatsApp > Linked
//! devices). The real connection lives in wa_client.rs behind `Link`, so
//! tests use a fake. Off (`bloom-ai-whatsapp` = "false") means no client and
//! no socket. Messages are kept in RAM only: the last 50 per chat, 200 chats.
//!
//! For automatic replies: `State::incoming` broadcasts every message as it
//! lands in the buffer (`shared.whatsapp.incoming.subscribe()`, own messages
//! included, `from_me` set), and `State::link()` gives `send_text` and
//! `typing`. Chats are keyed by "+<number>" (1:1) or the group id.
//!
//! Names also come from whatsapp\contacts.json (wa_contacts.rs: the phone's
//! address book and the user's groups), after phones.json.

use crate::agent::{Ctx, Shared};
use crate::protocol::{emit, ConfirmKind, Out};
use crate::wa_contacts::{self, Book};
use crate::config::Tier;
use crate::tools::files;
use crate::{journal, phones, policy};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

const PER_CHAT: usize = 50;
const MAX_CHATS: usize = 200;
/// User-requested sends per rolling hour: no bulk messaging.
const SENDS_PER_HOUR: usize = 20;
const MAX_TEXT: usize = 4096;
const READ_MAX: usize = 20;
const LIST_MAX: usize = 20;
const GROUPS_MAX: usize = 50;
/// How long a text sent to the user's own chat counts as an echo.
const ECHO_FOR: Duration = Duration::from_secs(60);
/// Offered only while "Connect WhatsApp" is on.
pub const TOOLS: [&str; 5] = [
    "read_whatsapp",
    "list_whatsapp_chats",
    "list_whatsapp_groups",
    "send_whatsapp",
    "send_whatsapp_file",
];
/// Largest file Janice sends.
const MAX_FILE: u64 = 100 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// "+491701234567" for a 1:1 chat, the group's id for a group.
    pub chat: String,
    /// The group's subject, for group chats.
    pub group: Option<String>,
    /// Who wrote it: their WhatsApp name or number; "me" for the user.
    pub sender: String,
    pub from_me: bool,
    /// Unix seconds.
    pub at: i64,
    /// The text, or "[photo] caption" and the like.
    pub text: String,
    /// Forwarded from somewhere else: someone else's words.
    pub forwarded: bool,
}

/// The link as Settings shows it. `qr` and `code` are pairing credentials:
/// they go to Settings only, never to a log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// "connecting", "not_linked" or "linked"; empty means off.
    pub state: &'static str,
    pub number: Option<String>,
    pub qr: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
    /// The linked account's WhatsApp name. Not sent to Settings.
    pub name: Option<String>,
    /// (contacts, groups) in whatsapp\contacts.json.
    pub synced: Option<(usize, usize)>,
}

impl Status {
    fn emit(&self) {
        emit(&Out::WhatsappStatus {
            state: if self.state.is_empty() {
                "off"
            } else {
                self.state
            }
            .into(),
            number: self.number.clone(),
            qr: self.qr.clone(),
            code: self.code.clone(),
            error: self.error.clone(),
            contacts: self.synced.map(|s| s.0),
            groups: self.synced.map(|s| s.1),
        });
    }
}

/// What a connection reports.
#[derive(Debug)]
pub enum Event {
    Status(Status),
    Message(Message),
    /// The user read this chat on another device.
    Read(String),
}

/// The connection's commands. The connection itself starts with
/// `wa_client::start` and stops when its last `Link` is dropped.
#[async_trait::async_trait]
pub trait Link: Send + Sync {
    /// `chat`: "+<number>" or a group id.
    async fn send_text(&self, chat: &str, text: &str) -> Result<(), String>;
    /// Uploads `file` and sends it to `chat`.
    async fn send_file(&self, chat: &str, file: Outgoing) -> Result<(), String>;
    /// The "typing..." indicator in `chat`.
    async fn typing(&self, chat: &str, on: bool) -> Result<(), String>;
    /// Asks WhatsApp for an 8-character link code for `phone` (digits with
    /// country code); it arrives in a `Status`.
    fn pair_code(&self, phone: &str);
    /// Logs this device out on WhatsApp's side when connected, ends the
    /// connection and deletes the session. Ok(false): it may still be listed
    /// in WhatsApp > Linked devices (it was offline).
    async fn unlink(&self) -> Result<bool, String>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaKind {
    Image,
    Video,
    Document,
}

/// A file on its way out. `data` is the file's bytes.
pub struct Outgoing {
    pub data: Vec<u8>,
    pub kind: MediaKind,
    pub mime: &'static str,
    pub name: String,
    pub caption: Option<String>,
}

/// Where a name beyond phones.json was found.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    /// The phone's address book (synced).
    Contact,
    /// A group subject: any member can change it.
    Group,
    /// The name someone gave themselves on WhatsApp.
    Chat,
}

#[derive(Default)]
struct Chat {
    /// Group subject, or the other person's WhatsApp name.
    name: String,
    group: bool,
    messages: VecDeque<Message>,
    unread: usize,
    last: i64,
}

#[derive(Default)]
pub struct Chats {
    map: HashMap<String, Chat>,
}

impl Chats {
    pub fn push(&mut self, m: Message) {
        if !self.map.contains_key(&m.chat) && self.map.len() >= MAX_CHATS {
            let oldest = self.map.iter().min_by_key(|(_, c)| c.last);
            if let Some(key) = oldest.map(|(k, _)| k.clone()) {
                self.map.remove(&key);
            }
        }
        let chat = self.map.entry(m.chat.clone()).or_default();
        // Our own sends carry no subject.
        chat.group |= m.group.is_some();
        if let Some(subject) = &m.group {
            chat.name = subject.clone();
        } else if !m.from_me {
            chat.name = m.sender.clone();
        }
        // Writing in a chat means the user has seen it.
        chat.unread = if m.from_me { 0 } else { chat.unread + 1 };
        chat.last = chat.last.max(m.at);
        chat.messages.push_back(m);
        if chat.messages.len() > PER_CHAT {
            chat.messages.pop_front();
        }
    }

    fn read(&mut self, chat: &str) {
        if let Some(c) = self.map.get_mut(chat) {
            c.unread = 0;
        }
    }

    /// "Neha (+49...)" from phones.json, else the address book or the
    /// WhatsApp name; "Family (group)" for a group.
    fn label(&self, saved: &BTreeMap<String, String>, book: &Book, key: &str) -> String {
        if let Some((subject, _)) = self.group(book, key) {
            return format!("{subject} (group)");
        }
        let chat = self.map.get(key);
        match saved_name(saved, key)
            .or(book.name_of(key))
            .or(chat.map(|c| c.name.as_str()).filter(|n| !n.is_empty()))
        {
            Some(name) => format!("{name} ({key})"),
            None => key.to_string(),
        }
    }

    /// The subject and member count (if known), when `key` is a group.
    fn group(&self, book: &Book, key: &str) -> Option<(String, Option<usize>)> {
        if let Some(g) = book.group(key) {
            return Some((g.subject.clone(), g.members));
        }
        let chat = self.map.get(key).filter(|c| c.group)?;
        Some((chat.name.clone(), None))
    }

    /// The chats a name means beyond phones.json: synced contacts, groups
    /// (synced or seen) and, with `people`, the names people gave
    /// themselves. An exact name beats partial ones only within its own
    /// source: a renamed group or a chosen name never hides an address-book
    /// entry, the caller asks which instead.
    fn others(&self, book: &Book, query: &str, people: bool) -> Vec<(String, Source)> {
        let q = query.trim().to_lowercase();
        let exact = |found: &mut Vec<(&str, &str)>| {
            if found.iter().any(|(_, n)| n.trim().to_lowercase() == q) {
                found.retain(|(_, n)| n.trim().to_lowercase() == q);
            }
        };
        let chats = |group: bool| -> Vec<(&str, &str)> {
            self.map
                .iter()
                .filter(|(_, c)| c.group == group && !c.name.is_empty())
                .filter(|(_, c)| c.name.to_lowercase().contains(&q))
                .map(|(k, c)| (k.as_str(), c.name.as_str()))
                .collect()
        };
        let contacts = book.people(query).into_iter();
        let mut groups: Vec<(&str, &str)> = book
            .groups_named(query)
            .into_iter()
            .map(|g| (g.jid.as_str(), g.subject.as_str()))
            .collect();
        groups.extend(chats(true));
        let sources = [
            (
                Source::Contact,
                contacts
                    .map(|c| (c.number.as_str(), c.name.as_str()))
                    .collect(),
            ),
            (Source::Group, groups),
            (Source::Chat, if people { chats(false) } else { Vec::new() }),
        ];
        let mut found: Vec<(String, Source)> = Vec::new();
        for (source, mut names) in sources {
            exact(&mut names);
            for (key, _) in names {
                if !found.iter().any(|(k, _)| k == key) {
                    found.push((key.to_string(), source));
                }
            }
        }
        found
    }

    /// "Several match ...", naming what each one is.
    fn which(&self, book: &Book, query: &str, found: &[(String, Source)]) -> String {
        let names: Vec<String> = found
            .iter()
            .map(|(key, source)| match source {
                Source::Group => self.label(&BTreeMap::new(), book, key),
                Source::Contact => {
                    format!("{} (contact, {key})", book.name_of(key).unwrap_or(key))
                }
                Source::Chat => format!(
                    "{} (WhatsApp name, {key})",
                    self.map.get(key).map_or("", |c| &c.name)
                ),
            })
            .collect();
        format!("Several match {query}: {}. Which one?", names.join(", "))
    }

    /// The chat a name, number or group name means.
    fn find(&self, dir: &Path, book: &Book, query: &str) -> Result<String, String> {
        let q = query.trim();
        if let Some(number) = as_number(q) {
            return Ok(number);
        }
        let saved = phones::find(dir, q)?;
        if saved.is_empty() {
            return match self.others(book, q, true).as_slice() {
                [(key, _)] => Ok(key.clone()),
                [] => Err(format!("No WhatsApp chat or saved number matches {query}.")),
                many => Err(self.which(book, query, many)),
            };
        }
        let mut keys: Vec<String> = saved.iter().map(|(_, n)| n.clone()).collect();
        keys.sort();
        keys.dedup();
        if keys.len() > 1 {
            keys.retain(|k| self.map.contains_key(k));
        }
        match keys.len() {
            1 => Ok(keys.remove(0)),
            _ => {
                let names: Vec<String> = saved.iter().map(|(n, p)| format!("{n} ({p})")).collect();
                Err(format!(
                    "Several match {query}: {}. Which one?",
                    names.join(", ")
                ))
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

fn saved_name<'a>(saved: &'a BTreeMap<String, String>, number: &str) -> Option<&'a str> {
    saved
        .iter()
        .find(|(_, n)| *n == number)
        .map(|(k, _)| k.as_str())
}

/// whatsapp\contacts.json; empty when missing or unreadable (Settings says
/// why).
pub fn book(data_dir: &Path) -> Book {
    wa_contacts::load(&data_dir.join("whatsapp")).unwrap_or_default()
}

/// find_contact's lines for synced groups (at most 10); empty if none match.
/// Synced people reach find_contact through people::merge.
pub fn group_lines(data_dir: &Path, query: &str) -> String {
    book(data_dir)
        .groups_named(query)
        .into_iter()
        .take(10)
        .map(|g| format!("\n{} (WhatsApp group)", g.subject))
        .collect()
}

/// A phone number as E.164, if `text` is one.
fn as_number(text: &str) -> Option<String> {
    let digits = text.chars().filter(char::is_ascii_digit).count();
    (digits >= 6)
        .then(|| phones::normalize(text).ok())
        .flatten()
}

/// "14:05" today, "3 Oct 14:05" before.
pub fn stamp(at: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(t) = Local.timestamp_opt(at, 0).single() else {
        return String::new();
    };
    if t.date_naive() == Local::now().date_naive() {
        t.format("%H:%M").to_string()
    } else {
        t.format("%-d %b %H:%M").to_string()
    }
}

pub struct State {
    link: Mutex<Option<Arc<dyn Link>>>,
    /// Bumped when a connection is attached or dropped: late events from an
    /// old one are ignored.
    generation: AtomicU64,
    chats: Mutex<Chats>,
    status: Mutex<Status>,
    sends: Mutex<VecDeque<Instant>>,
    /// Texts this PC sent to the user's own chat, and when: if WhatsApp
    /// hands them back within a minute, they are not the user writing
    /// (selfchat.rs).
    echoes: Mutex<VecDeque<(String, Instant)>>,
    pub incoming: broadcast::Sender<Message>,
}

impl Default for State {
    fn default() -> State {
        State {
            link: Mutex::default(),
            generation: AtomicU64::default(),
            chats: Mutex::default(),
            status: Mutex::default(),
            sends: Mutex::default(),
            echoes: Mutex::default(),
            incoming: broadcast::channel(64).0,
        }
    }
}

impl State {
    pub fn link(&self) -> Option<Arc<dyn Link>> {
        self.link.lock().unwrap().clone()
    }

    /// Uses `link` from now on; feed its events to `apply` with the result.
    pub fn attach(&self, link: Arc<dyn Link>) -> u64 {
        *self.link.lock().unwrap() = Some(link);
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn apply(&self, generation: u64, event: Event) {
        if generation != self.generation.load(Ordering::SeqCst) {
            return;
        }
        match event {
            Event::Status(s) => {
                s.emit();
                *self.status.lock().unwrap() = s;
            }
            Event::Message(m) => {
                self.chats.lock().unwrap().push(m.clone());
                let _ = self.incoming.send(m);
            }
            Event::Read(chat) => self.chats.lock().unwrap().read(&chat),
        }
    }

    /// Drops the connection (it disconnects on its own) and forgets every
    /// message. `state` "" is off.
    fn detach(&self, status: Status) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.link.lock().unwrap().take();
        self.chats.lock().unwrap().clear();
        status.emit();
        *self.status.lock().unwrap() = status;
    }

    pub fn off(&self) {
        self.detach(Status::default());
    }

    pub fn emit_status(&self) {
        self.status.lock().unwrap().emit();
    }

    fn fail(&self, message: String) {
        let mut status = self.status.lock().unwrap();
        status.error = Some(message);
        status.emit();
    }

    /// The last `n` messages of `chat`, oldest first.
    pub fn recent(&self, chat: &str, n: usize) -> Vec<Message> {
        let chats = self.chats.lock().unwrap();
        let Some(c) = chats.map.get(chat) else {
            return Vec::new();
        };
        c.messages
            .iter()
            .skip(c.messages.len().saturating_sub(n))
            .cloned()
            .collect()
    }

    /// A message sent from this PC (on request or automatically): kept and
    /// broadcast like one from the user's other devices, so automatic replies
    /// see it. The library does not report our own sends back. In the user's
    /// own chat it is only kept, and noted as an echo: it is never a request.
    pub fn record_sent(&self, chat: &str, text: &str) {
        let m = Message {
            chat: chat.into(),
            group: None,
            sender: "me".into(),
            from_me: true,
            at: chrono::Utc::now().timestamp(),
            text: text.into(),
            forwarded: false,
        };
        self.chats.lock().unwrap().push(m.clone());
        if self.own_number().as_deref() == Some(chat) {
            self.expect_echo(text);
            return;
        }
        let _ = self.incoming.send(m);
    }

    /// `text` is about to go to the user's own chat from this PC.
    pub fn expect_echo(&self, text: &str) {
        let mut echoes = self.echoes.lock().unwrap();
        // An expired copy must not stop a fresh one.
        let now = Instant::now();
        echoes.retain(|(_, at)| now.duration_since(*at) < ECHO_FOR);
        if !echoes.iter().any(|(t, _)| t == text) {
            echoes.push_back((text.into(), Instant::now()));
            if echoes.len() > 10 {
                echoes.pop_front();
            }
        }
    }

    /// True once for each text this PC sent to the user's own chat in the
    /// last minute. Older ones are forgotten: the same words later are the
    /// user's.
    pub fn is_echo(&self, text: &str) -> bool {
        self.is_echo_at(text, Instant::now())
    }

    #[cfg(test)]
    fn age_echoes(&self, by: Duration) {
        for (_, at) in self.echoes.lock().unwrap().iter_mut() {
            *at -= by;
        }
    }

    fn is_echo_at(&self, text: &str, now: Instant) -> bool {
        let mut echoes = self.echoes.lock().unwrap();
        echoes.retain(|(_, at)| now.duration_since(*at) < ECHO_FOR);
        let found = echoes.iter().position(|(t, _)| t == text);
        found.and_then(|i| echoes.remove(i)).is_some()
    }

    /// The linked account's own chat ("Message yourself"), "+<number>".
    pub fn own_number(&self) -> Option<String> {
        let status = self.status.lock().unwrap();
        status.number.clone().filter(|_| status.state == "linked")
    }

    /// The linked account's WhatsApp name, once known.
    pub fn own_name(&self) -> Option<String> {
        self.status.lock().unwrap().name.clone()
    }

    fn linked(&self) -> bool {
        self.status.lock().unwrap().state == "linked"
    }

    /// Whether another user-requested send fits in the hourly limit.
    fn can_send(&self, now: Instant) -> bool {
        let mut sends = self.sends.lock().unwrap();
        while sends
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(3600))
        {
            sends.pop_front();
        }
        sends.len() < SENDS_PER_HOUR
    }

    fn note_send(&self, now: Instant) {
        self.sends.lock().unwrap().push_back(now);
    }
}

/// `whatsapp_on`: connects unless connected, and tells Settings the state.
pub fn on(shared: &Arc<Shared>) {
    let state = &shared.whatsapp;
    if state.link().is_some() {
        state.emit_status();
        return;
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let generation = state.attach(crate::wa_client::start(
        shared.data_dir.join("whatsapp"),
        tx,
    ));
    let s = shared.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            s.whatsapp.apply(generation, event);
        }
    });
}

pub fn pair_code(shared: &Shared, phone: &str) {
    let state = &shared.whatsapp;
    let Some(link) = state.link() else {
        return state.fail("Turn on Connect WhatsApp first.".into());
    };
    match phones::normalize(phone) {
        Ok(number) => link.pair_code(&number[1..]),
        Err(e) => state.fail(e),
    }
}

/// "Unlink": logs out on WhatsApp's side and deletes the session folder.
pub async fn unlink(shared: &Shared) {
    let state = &shared.whatsapp;
    let link = state.link();
    state.detach(Status {
        state: "connecting",
        ..Status::default()
    });
    let result = match link {
        Some(link) => link.unlink().await,
        None => remove_session(&shared.data_dir.join("whatsapp")).map(|()| false),
    };
    let error = match result {
        Ok(true) => None,
        Ok(false) => Some(
            "Unlinked on this PC. Also remove Bloom in WhatsApp > Linked devices on your phone."
                .to_string(),
        ),
        Err(e) => Some(e),
    };
    state.detach(Status {
        state: "not_linked",
        error,
        ..Status::default()
    });
}

/// Deletes the session folder; the store may hold it open for a moment.
pub fn remove_session(dir: &Path) -> Result<(), String> {
    let mut result = Ok(());
    for _ in 0..10 {
        result = match std::fs::remove_dir_all(dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => return Ok(()),
        };
        std::thread::sleep(Duration::from_millis(200));
    }
    result.map_err(|e| format!("Couldn't delete {}: {e}", dir.display()))
}

const NOT_LINKED: &str =
    "WhatsApp isn't connected. Turn on Connect WhatsApp in Settings > AI and link your phone.";

/// read_whatsapp: the last `count` messages of a chat, oldest first.
pub fn read(ctx: &mut Ctx, chat: &str, count: Option<u64>) -> Result<String, String> {
    let state = &ctx.shared.whatsapp;
    if state.link().is_none() {
        return Err(NOT_LINKED.into());
    }
    // Message text, names and group subjects (even in an ambiguity error)
    // come from other people: data, never instructions.
    ctx.tainted = true;
    let count = count.unwrap_or(10).clamp(1, READ_MAX as u64) as usize;
    let saved = phones::try_load(&ctx.shared.data_dir)?;
    let book = book(&ctx.shared.data_dir);
    let chats = state.chats.lock().unwrap();
    let key = chats.find(&ctx.shared.data_dir, &book, chat)?;
    let label = chats.label(&saved, &book, &key);
    let Some(found) = chats.map.get(&key) else {
        return Ok(format!(
            "No WhatsApp messages with {label} since Bloom connected."
        ));
    };
    let skip = found.messages.len().saturating_sub(count);
    let lines: Vec<String> = found
        .messages
        .iter()
        .skip(skip)
        .map(|m| format!("[{}] {}: {}", stamp(m.at), m.sender, m.text))
        .collect();
    Ok(format!("WhatsApp with {label}:\n{}", lines.join("\n")))
}

/// list_whatsapp_chats: newest first.
pub fn list(ctx: &mut Ctx) -> Result<String, String> {
    let state = &ctx.shared.whatsapp;
    if state.link().is_none() {
        return Err(NOT_LINKED.into());
    }
    let saved = phones::try_load(&ctx.shared.data_dir)?;
    let book = book(&ctx.shared.data_dir);
    let chats = state.chats.lock().unwrap();
    let mut keys: Vec<(&String, &Chat)> = chats.map.iter().collect();
    if keys.is_empty() {
        return Ok("No WhatsApp messages since Bloom connected.".into());
    }
    // Names come from WhatsApp: outside content.
    ctx.tainted = true;
    keys.sort_by_key(|(_, c)| std::cmp::Reverse(c.last));
    Ok(keys
        .iter()
        .take(LIST_MAX)
        .map(|(k, c)| {
            format!(
                "{}: {} unread, last {}",
                chats.label(&saved, &book, k),
                c.unread,
                stamp(c.last)
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// list_whatsapp_groups: the user's groups, latest message first (since
/// linking), then by name.
pub fn list_groups(ctx: &mut Ctx) -> Result<String, String> {
    let state = &ctx.shared.whatsapp;
    if state.link().is_none() {
        return Err(NOT_LINKED.into());
    }
    let book = book(&ctx.shared.data_dir);
    let chats = state.chats.lock().unwrap();
    let last = |key: &str| chats.map.get(key).map_or(0, |c| c.last);
    let mut groups: Vec<(&str, Option<usize>, i64)> = book
        .groups
        .iter()
        .map(|g| (g.subject.as_str(), g.members, last(&g.jid)))
        .collect();
    // Heard from since linking, not synced yet.
    groups.extend(
        chats
            .map
            .iter()
            .filter(|(k, c)| c.group && book.group(k).is_none())
            .map(|(_, c)| (c.name.as_str(), None, c.last)),
    );
    if groups.is_empty() {
        return Ok("No WhatsApp groups synced yet.".into());
    }
    // Subjects come from WhatsApp: outside content.
    ctx.tainted = true;
    groups.sort_by_key(|g| std::cmp::Reverse(g.2));
    Ok(groups
        .iter()
        .take(GROUPS_MAX)
        .map(|(subject, members, last)| {
            let mut line = subject.to_string();
            if let Some(n) = members {
                line += &format!(", {n} members");
            }
            if *last > 0 {
                line += &format!(", last message {}", stamp(*last));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// `group`: Some(member count, if known) for a group.
fn confirm_text(
    name: &str,
    number: &str,
    group: Option<Option<usize>>,
    text: &str,
) -> (String, String) {
    let to = match group {
        Some(Some(n)) => format!("Group, {n} members"),
        Some(None) => "Group".to_string(),
        None => number.to_string(),
    };
    (
        format!("Send WhatsApp to {name}?"),
        format!("{to}\n\n{text}"),
    )
}

/// Who `to` means: (display name, "+<number>" or group id, `synced`: found by
/// a name from WhatsApp, not the user's own files).
fn recipient(
    ctx: &mut Ctx,
    saved: &BTreeMap<String, String>,
    book: &Book,
    to: &str,
) -> Result<(String, String, bool), String> {
    let dir = ctx.shared.data_dir.clone();
    let shared = ctx.shared.clone();
    let state = &shared.whatsapp;
    Ok(match as_number(to) {
        Some(n) => (saved_name(saved, &n).unwrap_or(&n).to_string(), n, false),
        None => match phones::find(&dir, to)?.as_slice() {
            [(name, number)] => (name.clone(), number.clone(), false),
            [] => {
                let chats = state.chats.lock().unwrap();
                match chats.others(book, to, false).as_slice() {
                    [(key, _)] => {
                        let name = match chats.group(book, key) {
                            Some((subject, _)) => subject,
                            None => book.name_of(key).unwrap_or(key).to_string(),
                        };
                        (name, key.clone(), true)
                    }
                    [] => {
                        return Err(format!(
                            "No saved number for {to}. Ask the user for it, then call save_phone."
                        ))
                    }
                    many => {
                        // The names come from WhatsApp: outside data.
                        ctx.tainted = true;
                        return Err(chats.which(book, to, many));
                    }
                }
            }
            many => {
                let names: Vec<String> = many.iter().map(|(n, p)| format!("{n} ({p})")).collect();
                return Err(format!(
                    "Several match {to}: {}. Which one?",
                    names.join(", ")
                ));
            }
        },
    })
}

/// send_whatsapp: to a saved name, a number, or (after phones.json) a synced
/// contact or group, on the user's request.
pub async fn send(ctx: &mut Ctx, to: &str, text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("The message is empty.".into());
    }
    if text.chars().count() > MAX_TEXT {
        return Err(format!("The message is longer than {MAX_TEXT} characters."));
    }
    let dir = ctx.shared.data_dir.clone();
    let saved = phones::try_load(&dir)?;
    let book = book(&dir);
    let shared = ctx.shared.clone();
    let state = &shared.whatsapp;
    let (name, number, synced) = recipient(ctx, &saved, &book, to)?;
    if state.link().is_none() || !state.linked() {
        return Err(NOT_LINKED.into());
    }
    if !state.can_send(Instant::now()) {
        return Err(format!(
            "Already sent {SENDS_PER_HOUR} WhatsApp messages in the last hour. Try again later."
        ));
    }
    let group = state.chats.lock().unwrap().group(&book, &number);
    // A group is found by its subject, which any member can change: below
    // carte blanche that always asks, even for a group the user is in.
    let known = group.is_none()
        && saved_name(&saved, &number).is_some()
        && !ctx.saved_this_task.contains(&number);
    // No message text in actions.log: messages stay in RAM.
    let detail = format!("to {name} {number} ({} chars)", text.chars().count());
    let ask = policy::email_needs_confirm(ctx.cfg.tier, known, ctx.tainted);
    // The name typed by the user found it; from here on it is in the result.
    ctx.tainted |= synced;
    let (title, body) = confirm_text(&name, &number, group.map(|g| g.1), text);
    if ask
        && !ctx
            .shared
            .bridge
            .confirm(ctx.task, ConfirmKind::Message, title, body)
            .await
    {
        journal::record(&dir, "whatsapp", &detail, "declined");
        return Ok("The user chose not to send it.".into());
    }
    let started = if ask {
        "approved-started"
    } else {
        "auto-started"
    };
    journal::record(&dir, "whatsapp", &detail, started);
    // Turned off while the confirm was open: nothing goes out.
    let Some(link) = state.link() else {
        journal::record_with(
            &dir,
            "whatsapp",
            &detail,
            "failed",
            Some("WhatsApp is off."),
        );
        return Err("WhatsApp is off.".into());
    };
    state.note_send(Instant::now());
    let sent = link.send_text(&number, text).await;
    let outcome = match (&sent, ask) {
        (Err(_), _) => "failed",
        (Ok(()), true) => "approved",
        (Ok(()), false) => "auto",
    };
    let reason = sent.as_ref().err().map(String::as_str);
    journal::record_with(&dir, "whatsapp", &detail, outcome, reason);
    if sent.is_ok() {
        // The user wrote in this chat: automatic replies pause there.
        state.record_sent(&number, text);
    }
    sent.map(|()| format!("Sent to {name}."))
}

/// Downloads, Documents and Desktop: where a bare file name is looked for.
fn user_folders() -> Vec<PathBuf> {
    ["downloads", "documents", "desktop"]
        .iter()
        .filter_map(|n| files::folder(n).ok())
        .collect()
}

/// The local file `path` means: an absolute path, or a name (or relative
/// path) found in `folders`. Several matches are listed, never guessed.
fn find_file(path: &str, folders: &[PathBuf]) -> Result<PathBuf, String> {
    let path = path.trim();
    let mut found: Vec<String> = Vec::new();
    if Path::new(path).is_absolute() {
        found.push(files::resolve_local(path)?);
    } else {
        for folder in folders {
            if let Ok(p) = files::resolve_local(&folder.join(path).to_string_lossy()) {
                if !found.contains(&p) {
                    found.push(p);
                }
            }
        }
    }
    found.retain(|p| Path::new(p).is_file());
    match found.len() {
        0 => Err(format!(
            "No file {path} found (looked in Downloads, Documents and Desktop)."
        )),
        1 => Ok(PathBuf::from(found.remove(0))),
        _ => Err(format!(
            "Several files match {path}: {}. Which one?",
            found.join(", ")
        )),
    }
}

/// True when `path` is inside the AI data dir (settings, memory, the
/// WhatsApp session): never sent.
fn in_data_dir(path: &Path, data_dir: &Path) -> bool {
    let norm = |p: &Path| {
        let s = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = s.to_string_lossy().to_lowercase();
        PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
    };
    norm(path).starts_with(norm(data_dir))
}

/// How WhatsApp should show the file: images and mp4 inline, the rest as a
/// document with the original name.
fn media_of(name: &str) -> (MediaKind, &'static str) {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => (MediaKind::Image, "image/jpeg"),
        "png" => (MediaKind::Image, "image/png"),
        "webp" => (MediaKind::Image, "image/webp"),
        "mp4" => (MediaKind::Video, "video/mp4"),
        "pdf" => (MediaKind::Document, "application/pdf"),
        "txt" => (MediaKind::Document, "text/plain"),
        "zip" => (MediaKind::Document, "application/zip"),
        "docx" => (
            MediaKind::Document,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        "xlsx" => (
            MediaKind::Document,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        "pptx" => (
            MediaKind::Document,
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ),
        _ => (MediaKind::Document, "application/octet-stream"),
    }
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// send_whatsapp_file: a local file to a contact, number or group.
pub async fn send_file(
    ctx: &mut Ctx,
    to: &str,
    path: &str,
    caption: Option<&str>,
) -> Result<String, String> {
    send_file_in(ctx, to, path, caption, &user_folders()).await
}

async fn send_file_in(
    ctx: &mut Ctx,
    to: &str,
    path: &str,
    caption: Option<&str>,
    folders: &[PathBuf],
) -> Result<String, String> {
    let caption = caption.map(str::trim).filter(|c| !c.is_empty());
    if caption.is_some_and(|c| c.chars().count() > MAX_TEXT) {
        return Err(format!("The caption is longer than {MAX_TEXT} characters."));
    }
    let dir = ctx.shared.data_dir.clone();
    let saved = phones::try_load(&dir)?;
    let book = book(&dir);
    let shared = ctx.shared.clone();
    let state = &shared.whatsapp;
    let (name, number, synced) = recipient(ctx, &saved, &book, to)?;
    if state.link().is_none() || !state.linked() {
        return Err(NOT_LINKED.into());
    }
    if !state.can_send(Instant::now()) {
        return Err(format!(
            "Already sent {SENDS_PER_HOUR} WhatsApp messages in the last hour. Try again later."
        ));
    }
    let file = find_file(path, folders).inspect_err(|e| {
        // File names come from the disk, maybe from a download: outside data.
        ctx.tainted |= e.starts_with("Several");
    })?;
    if in_data_dir(&file, &dir) {
        return Err("That file belongs to Bloom's AI data and is never sent.".into());
    }
    let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
    if size == 0 {
        return Err("The file is empty.".into());
    }
    if size > MAX_FILE {
        return Err(format!(
            "The file is {}, over the {} limit.",
            size_text(size),
            size_text(MAX_FILE)
        ));
    }
    let file_name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let group = state.chats.lock().unwrap().group(&book, &number);
    let known = group.is_none()
        && saved_name(&saved, &number).is_some()
        && !ctx.saved_this_task.contains(&number);
    // File contents leave the PC: always asked below carte blanche. No
    // contents in actions.log, only the name and size.
    let detail = format!("file {file_name} ({size} bytes) to {name} {number}");
    let ask = ctx.cfg.tier != Tier::CarteBlanche
        || policy::email_needs_confirm(ctx.cfg.tier, known, ctx.tainted);
    ctx.tainted |= synced;
    let (_, mut body) = confirm_text(&name, &number, group.map(|g| g.1), &size_text(size));
    if let Some(c) = caption {
        body += &format!("\n\nCaption: {c}");
    }
    let title = format!("Send {file_name} to {name}?");
    if ask
        && !ctx
            .shared
            .bridge
            .confirm(ctx.task, ConfirmKind::Message, title, body)
            .await
    {
        journal::record(&dir, "whatsapp", &detail, "declined");
        return Ok("The user chose not to send it.".into());
    }
    let started = if ask {
        "approved-started"
    } else {
        "auto-started"
    };
    journal::record(&dir, "whatsapp", &detail, started);
    let failed = |reason: &str| {
        journal::record_with(&dir, "whatsapp", &detail, "failed", Some(reason));
        Err(reason.to_string())
    };
    let Some(link) = state.link() else {
        return failed("WhatsApp is off.");
    };
    let data = match std::fs::read(&file) {
        Ok(d) => d,
        Err(e) => return failed(&format!("Couldn't read the file: {e}")),
    };
    let (kind, mime) = media_of(&file_name);
    state.note_send(Instant::now());
    let sent = link
        .send_file(
            &number,
            Outgoing {
                data,
                kind,
                mime,
                name: file_name.clone(),
                caption: caption.map(str::to_string),
            },
        )
        .await;
    let outcome = match (&sent, ask) {
        (Err(_), _) => "failed",
        (Ok(()), true) => "approved",
        (Ok(()), false) => "auto",
    };
    let reason = sent.as_ref().err().map(String::as_str);
    journal::record_with(&dir, "whatsapp", &detail, outcome, reason);
    if sent.is_ok() {
        state.record_sent(&number, &format!("[file] {file_name}"));
    }
    sent.map(|()| format!("Sent {file_name} to {name}."))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::bridge::Answer;
    use crate::config::Tier;
    use crate::testutil::ctx;

    /// Records what it was asked to send, and when it sent or typed.
    #[derive(Default)]
    pub struct Fake {
        pub sent: Mutex<Vec<(String, String)>>,
        /// (chat, file, bytes sent).
        pub files: Mutex<Vec<(String, Outgoing)>>,
        /// ("send" | "typing on" | "typing off", chat, when).
        pub log: Mutex<Vec<(&'static str, String, tokio::time::Instant)>>,
    }

    #[async_trait::async_trait]
    impl Link for Fake {
        async fn send_text(&self, chat: &str, text: &str) -> Result<(), String> {
            self.sent.lock().unwrap().push((chat.into(), text.into()));
            let now = tokio::time::Instant::now();
            self.log.lock().unwrap().push(("send", chat.into(), now));
            Ok(())
        }
        async fn send_file(&self, chat: &str, file: Outgoing) -> Result<(), String> {
            self.files.lock().unwrap().push((chat.into(), file));
            Ok(())
        }
        async fn typing(&self, chat: &str, on: bool) -> Result<(), String> {
            let what = if on { "typing on" } else { "typing off" };
            let now = tokio::time::Instant::now();
            self.log.lock().unwrap().push((what, chat.into(), now));
            Ok(())
        }
        fn pair_code(&self, _phone: &str) {}
        async fn unlink(&self) -> Result<bool, String> {
            Ok(true)
        }
    }

    pub fn msg(chat: &str, sender: &str, at: i64, text: &str) -> Message {
        Message {
            chat: chat.into(),
            group: None,
            sender: sender.into(),
            from_me: sender == "me",
            at,
            text: text.into(),
            forwarded: false,
        }
    }

    /// A linked fake connection on `ctx`.
    fn link(ctx: &Ctx) -> Arc<Fake> {
        link_state(&ctx.shared.whatsapp)
    }

    /// The linked account's own number in tests.
    pub const ME: &str = "+4915550000001";

    pub fn link_state(state: &State) -> Arc<Fake> {
        let fake = Arc::new(Fake::default());
        let generation = state.attach(fake.clone());
        state.apply(
            generation,
            Event::Status(Status {
                state: "linked",
                number: Some(ME.into()),
                name: Some("Arnav Aggarwal".into()),
                ..Status::default()
            }),
        );
        fake
    }

    fn feed(ctx: &Ctx, m: Message) {
        feed_state(&ctx.shared.whatsapp, m);
    }

    pub fn feed_state(state: &State, m: Message) {
        state.apply(state.generation.load(Ordering::SeqCst), Event::Message(m));
    }

    #[test]
    fn buffer_keeps_50_per_chat_and_200_chats() {
        let mut chats = Chats::default();
        for i in 0..60 {
            chats.push(msg("+491", "Neha", i, &format!("m{i}")));
        }
        let neha = &chats.map["+491"];
        assert_eq!(neha.messages.len(), PER_CHAT);
        assert_eq!(neha.messages[0].text, "m10");
        assert_eq!(neha.unread, 60);
        chats.push(msg("+491", "me", 61, "ok"));
        assert_eq!(chats.map["+491"].unread, 0);
        for i in 0..MAX_CHATS {
            chats.push(msg(&format!("+5{i}"), "x", 100 + i as i64, "hi"));
        }
        assert_eq!(chats.map.len(), MAX_CHATS);
        // The quietest chat made room.
        assert!(!chats.map.contains_key("+491"));
        assert!(chats.map.contains_key("+50"));
    }

    #[tokio::test]
    async fn read_resolves_names_numbers_and_groups_and_taints() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        phones::save(&dir, "Neha", "+491701234567").unwrap();
        assert!(read(&mut ctx, "Neha", None).is_err(), "not linked");
        link(&ctx);
        for i in 0..25 {
            feed(&ctx, msg("+491701234567", "Neha", i, &format!("m{i}")));
        }
        let mut group = msg("123@g.us", "Sam", 30, "[photo] look");
        group.group = Some("Family".into());
        feed(&ctx, group);

        let out = read(&mut ctx, "neha", Some(50)).unwrap();
        assert!(ctx.tainted);
        assert!(
            out.starts_with("WhatsApp with Neha (+491701234567):"),
            "{out}"
        );
        assert_eq!(out.lines().count(), 1 + READ_MAX);
        assert!(out.lines().nth(1).unwrap().ends_with("Neha: m5"), "{out}");
        assert!(out.ends_with("Neha: m24"));

        let by_number = read(&mut ctx, "+49 170 1234567", Some(1)).unwrap();
        assert!(by_number.ends_with("Neha: m24"), "{by_number}");
        let recent = ctx.shared.whatsapp.recent("+491701234567", 3);
        let texts: Vec<&str> = recent.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["m22", "m23", "m24"]);
        assert!(ctx.shared.whatsapp.recent("+1", 3).is_empty());
        assert_eq!(
            ctx.shared.whatsapp.own_name().as_deref(),
            Some("Arnav Aggarwal")
        );
        let fam = read(&mut ctx, "family", None).unwrap();
        assert!(
            fam.contains("Family (group)") && fam.ends_with("Sam: [photo] look"),
            "{fam}"
        );
        let none = read(&mut ctx, "+14155550100", None).unwrap();
        assert!(none.starts_with("No WhatsApp messages"), "{none}");
        assert!(read(&mut ctx, "Bob", None).is_err());
    }

    #[tokio::test]
    async fn untouched_reads_do_not_taint_and_list_is_newest_first() {
        let mut ctx = ctx();
        link(&ctx);
        assert_eq!(
            list(&mut ctx).unwrap(),
            "No WhatsApp messages since Bloom connected."
        );
        assert!(!ctx.tainted);
        feed(&ctx, msg("+491", "Neha", 10, "a"));
        feed(&ctx, msg("+492", "Sam", 20, "b"));
        feed(&ctx, msg("+492", "Sam", 21, "c"));
        ctx.shared.whatsapp.apply(
            ctx.shared.whatsapp.generation.load(Ordering::SeqCst),
            Event::Read("+491".into()),
        );
        let out = list(&mut ctx).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("Sam (+492): 2 unread"), "{out}");
        assert!(lines[1].starts_with("Neha (+491): 0 unread"), "{out}");
        assert!(ctx.tainted);
    }

    #[tokio::test]
    async fn events_of_a_dropped_connection_are_ignored() {
        let ctx = ctx();
        let state = &ctx.shared.whatsapp;
        let old = state.attach(Arc::new(Fake::default()));
        state.off();
        state.apply(old, Event::Message(msg("+491", "Neha", 1, "late")));
        assert!(state.chats.lock().unwrap().map.is_empty());
        assert!(state.link().is_none());
    }

    #[tokio::test]
    async fn incoming_messages_are_broadcast() {
        let ctx = ctx();
        link(&ctx);
        let mut rx = ctx.shared.whatsapp.incoming.subscribe();
        feed(&ctx, msg("+491", "Neha", 1, "hi"));
        assert_eq!(rx.recv().await.unwrap().text, "hi");
    }

    /// Runs send_whatsapp, answering a confirm with `answer` if one comes.
    async fn send_with(ctx: Ctx, to: &str, answer: Option<bool>) -> (Result<String, String>, bool) {
        let (out, asked, _) = send_ctx(ctx, to, answer).await;
        (out, asked)
    }

    /// `send_with`, handing the Ctx back.
    async fn send_ctx(
        mut ctx: Ctx,
        to: &str,
        answer: Option<bool>,
    ) -> (Result<String, String>, bool, Ctx) {
        let shared = ctx.shared.clone();
        let (to, text) = (to.to_string(), "Running late".to_string());
        let running = tokio::spawn(async move {
            let out = send(&mut ctx, &to, &text).await;
            (out, ctx)
        });
        tokio::task::yield_now().await;
        let asked = shared
            .bridge
            .answer_pending(Answer::Confirm(answer.unwrap_or(false)));
        let (out, ctx) = running.await.unwrap();
        (out, asked, ctx)
    }

    /// What the connection synced: Neha and Sam from the address book, the
    /// Family group (5 members) and Family Trip.
    pub fn sync_book(data_dir: &Path) {
        use crate::wa_contacts::tests::{group, person};
        wa_contacts::update(
            &data_dir.join("whatsapp"),
            true,
            vec![
                person("Neha Sharma", "+4917000000002"),
                person("Sam", "+4917000000003"),
            ],
            Some(vec![
                group("Family", "1@g.us", Some(5)),
                group("Family Trip", "2@g.us", None),
            ]),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn own_numbers_win_over_synced_ones() {
        let mut c = ctx();
        c.cfg.tier = Tier::CarteBlanche;
        phones::save(&c.shared.data_dir, "Neha", "+491701234567").unwrap();
        sync_book(&c.shared.data_dir);
        let fake = link(&c);
        let (out, _, c) = send_ctx(c, "neha", None).await;
        assert_eq!(out.unwrap(), "Sent to Neha.");
        assert_eq!(fake.sent.lock().unwrap()[0].0, "+491701234567");
        assert!(!c.tainted, "only the user's own file was used");
        // Only in the address book: found there, and the name taints.
        let (out, _, c) = send_ctx(c, "sam", None).await;
        assert_eq!(out.unwrap(), "Sent to Sam.");
        assert_eq!(fake.sent.lock().unwrap()[1].0, "+4917000000003");
        assert!(c.tainted);
    }

    #[tokio::test]
    async fn group_sends_go_to_the_group_and_always_ask() {
        let mut c = ctx();
        c.cfg.tier = Tier::Competent;
        sync_book(&c.shared.data_dir);
        let fake = link(&c);
        let shared = c.shared.clone();
        // The exact subject wins over "Family Trip". Any member can rename a
        // group, so even one the user is in asks on the competent tier.
        let (out, asked, c) = send_ctx(c, "FAMILY", Some(true)).await;
        assert!(asked);
        assert_eq!(out.unwrap(), "Sent to Family.");
        assert_eq!(
            fake.sent.lock().unwrap()[0],
            ("1@g.us".to_string(), "Running late".to_string())
        );
        assert!(c.tainted, "the subject came back in the result");
        let kept = shared.whatsapp.recent("1@g.us", 5);
        assert!(kept[0].from_me);
        let (_, asked, c) = send_ctx(c, "family trip", Some(false)).await;
        assert!(asked);
        // Carte blanche never asks.
        let mut c = c;
        c.cfg.tier = Tier::CarteBlanche;
        let (_, asked, _) = send_ctx(c, "family trip", None).await;
        assert!(!asked);
        assert_eq!(
            confirm_text("Family", "1@g.us", Some(Some(5)), "hi"),
            (
                "Send WhatsApp to Family?".into(),
                "Group, 5 members\n\nhi".into()
            )
        );
        assert_eq!(
            confirm_text("Trip", "2@g.us", Some(None), "hi").1,
            "Group\n\nhi"
        );
        // Sends count toward the hourly limit like any other.
        assert_eq!(shared.whatsapp.sends.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_group_only_seen_in_messages_is_not_known() {
        let mut c = ctx();
        c.cfg.tier = Tier::Competent;
        let fake = link(&c);
        let mut m = msg("9@g.us", "Sam", 1, "hi");
        m.group = Some("Book Club".into());
        feed(&c, m);
        let (out, asked, _) = send_ctx(c, "book club", Some(true)).await;
        assert!(asked);
        assert_eq!(out.unwrap(), "Sent to Book Club.");
        assert_eq!(fake.sent.lock().unwrap()[0].0, "9@g.us");
    }

    #[tokio::test]
    async fn several_synced_matches_ask_which_and_taint() {
        let mut c = ctx();
        c.cfg.tier = Tier::CarteBlanche;
        sync_book(&c.shared.data_dir);
        let fake = link(&c);
        let (out, asked, c) = send_ctx(c, "fam", None).await;
        assert!(!asked);
        let err = out.unwrap_err();
        assert!(
            err.contains("Several match fam")
                && err.contains("Family (group)")
                && err.contains("Family Trip (group)"),
            "{err}"
        );
        assert!(c.tainted);
        assert!(fake.sent.lock().unwrap().is_empty());
        let (out, _, _) = send_ctx(c, "Bob", None).await;
        assert!(out.unwrap_err().contains("save_phone"));
    }

    /// A group renamed "Mom" by one of its members, and the address book's
    /// "Mom Sharma".
    fn mom_book(data_dir: &Path) {
        use crate::wa_contacts::tests::{group, person};
        wa_contacts::update(
            &data_dir.join("whatsapp"),
            true,
            vec![person("Mom Sharma", "+4917000000009")],
            Some(vec![group("Mom", "7@g.us", Some(40))]),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn a_group_named_like_a_contact_never_wins_by_itself() {
        let mut c = ctx();
        c.cfg.tier = Tier::Competent;
        mom_book(&c.shared.data_dir);
        let fake = link(&c);
        let (out, asked, c) = send_ctx(c, "Mom", None).await;
        assert!(!asked);
        let err = out.unwrap_err();
        assert!(
            err.contains("Mom (group)") && err.contains("Mom Sharma (contact, +4917000000009)"),
            "{err}"
        );
        assert!(c.tainted);
        assert!(fake.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_strangers_chosen_name_never_wins_by_itself() {
        let mut c = ctx();
        mom_book(&c.shared.data_dir);
        link(&c);
        feed(&c, msg("+4915550000666", "Mom", 1, "it's me, new number"));
        let err = read(&mut c, "mom", None).unwrap_err();
        assert!(
            err.contains("Mom (WhatsApp name, +4915550000666)")
                && err.contains("Mom Sharma (contact, +4917000000009)")
                && err.contains("Mom (group)"),
            "{err}"
        );
        assert!(c.tainted);
    }

    #[test]
    fn exact_names_win_within_one_source_ignoring_spaces() {
        use crate::wa_contacts::tests::group;
        let book = Book {
            groups: vec![
                group(" Family ", "1@g.us", None),
                group("Family Trip", "2@g.us", None),
            ],
            ..Book::default()
        };
        let chats = Chats::default();
        assert_eq!(
            chats.others(&book, " family ", false),
            [("1@g.us".to_string(), Source::Group)]
        );
        assert_eq!(chats.others(&book, "fam", false).len(), 2);
    }

    #[tokio::test]
    async fn synced_groups_are_read_and_listed_without_messages() {
        let mut c = ctx();
        sync_book(&c.shared.data_dir);
        link(&c);
        assert_eq!(
            read(&mut c, "family", None).unwrap(),
            "No WhatsApp messages with Family (group) since Bloom connected."
        );
        let mut c = ctx();
        sync_book(&c.shared.data_dir);
        link(&c);
        let mut trip = msg("2@g.us", "Sam", 50, "boarding");
        trip.group = Some("Family Trip".into());
        feed(&c, trip);
        let mut club = msg("9@g.us", "Sam", 40, "hi");
        club.group = Some("Book Club".into());
        feed(&c, club);
        feed(&c, msg("+4917000000002", "Neha S.", 30, "hi"));
        let out = list_groups(&mut c).unwrap();
        assert!(c.tainted);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "{out}");
        assert!(lines[0].starts_with("Family Trip, last message"), "{out}");
        assert!(lines[1].starts_with("Book Club, last message"), "{out}");
        assert_eq!(lines[2], "Family, 5 members");
        // Chat labels use the address-book name over the WhatsApp one.
        let chats = list(&mut c).unwrap();
        assert!(chats.contains("Neha Sharma (+4917000000002)"), "{chats}");
        let mut empty = ctx();
        link(&empty);
        assert_eq!(
            list_groups(&mut empty).unwrap(),
            "No WhatsApp groups synced yet."
        );
        assert!(!empty.tainted);
    }

    #[tokio::test]
    async fn sends_follow_the_email_tiers() {
        // (tier, saved, tainted) -> asks
        let cases = [
            (Tier::Conservative, true, false, true),
            (Tier::Competent, true, false, false),
            (Tier::Competent, false, false, true),
            (Tier::Competent, true, true, true),
            (Tier::CarteBlanche, false, true, false),
        ];
        for (tier, saved, tainted, asks) in cases {
            let mut c = ctx();
            c.cfg.tier = tier;
            c.tainted = tainted;
            if saved {
                phones::save(&c.shared.data_dir, "Neha", "+491701234567").unwrap();
            }
            let fake = link(&c);
            let dir = c.shared.data_dir.clone();
            let (out, asked) = send_with(c, "+491701234567", Some(true)).await;
            assert_eq!(asked, asks, "{tier:?} saved={saved} tainted={tainted}");
            assert!(out.unwrap().starts_with("Sent to"));
            let sent = fake.sent.lock().unwrap().clone();
            assert_eq!(
                sent,
                [("+491701234567".to_string(), "Running late".to_string())]
            );
            let log = std::fs::read_to_string(dir.join("actions.log")).unwrap();
            assert!(log.contains(r#""kind":"whatsapp""#) && log.contains("(12 chars)"));
            assert!(!log.contains("Running late"));
        }
    }

    #[tokio::test]
    async fn declined_and_unknown_recipients_send_nothing() {
        let c = ctx();
        let fake = link(&c);
        let (out, asked) = send_with(c, "+491701234567", Some(false)).await;
        assert!(asked);
        assert_eq!(out.unwrap(), "The user chose not to send it.");
        assert!(fake.sent.lock().unwrap().is_empty());

        let c = ctx();
        link(&c);
        let (out, asked) = send_with(c, "Neha", None).await;
        assert!(!asked);
        assert!(out.unwrap_err().contains("save_phone"));
    }

    #[tokio::test]
    async fn a_saved_name_sends_to_its_number() {
        let c = ctx();
        phones::save(&c.shared.data_dir, "Neha", "+491701234567").unwrap();
        let fake = link(&c);
        let shared = c.shared.clone();
        let mut rx = shared.whatsapp.incoming.subscribe();
        let (out, asked) = send_with(c, "neha", Some(true)).await;
        assert!(asked);
        assert_eq!(out.unwrap(), "Sent to Neha.");
        assert_eq!(fake.sent.lock().unwrap()[0].0, "+491701234567");
        // Kept in the chat and broadcast as the user's own message.
        let kept = shared.whatsapp.recent("+491701234567", 5);
        assert_eq!(kept.len(), 1);
        assert!(kept[0].from_me && kept[0].sender == "me" && kept[0].text == "Running late");
        assert_eq!(rx.recv().await.unwrap(), kept[0]);
        assert_eq!(
            confirm_text("Neha", "+491701234567", None, "hi"),
            (
                "Send WhatsApp to Neha?".into(),
                "+491701234567\n\nhi".into()
            )
        );
    }

    #[tokio::test]
    async fn twenty_sends_per_hour() {
        let mut c = ctx();
        c.cfg.tier = Tier::CarteBlanche;
        let fake = link(&c);
        for _ in 0..SENDS_PER_HOUR {
            send(&mut c, "+491701234567", "hi").await.unwrap();
        }
        let err = send(&mut c, "+491701234567", "hi").await.unwrap_err();
        assert!(err.contains("last hour"), "{err}");
        assert_eq!(fake.sent.lock().unwrap().len(), SENDS_PER_HOUR);
        let state = &c.shared.whatsapp;
        let later = Instant::now() + Duration::from_secs(3600);
        assert!(state.can_send(later));
    }

    #[tokio::test]
    async fn long_or_empty_messages_and_no_link_are_refused() {
        let mut c = ctx();
        c.cfg.tier = Tier::CarteBlanche;
        assert!(send(&mut c, "+491701234567", "hi")
            .await
            .unwrap_err()
            .contains("isn't connected"));
        link(&c);
        assert!(send(&mut c, "+491701234567", "  ").await.is_err());
        let long = "x".repeat(MAX_TEXT + 1);
        assert!(send(&mut c, "+491701234567", &long).await.is_err());
        assert!(send(&mut c, "+491701234567", &"x".repeat(MAX_TEXT))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn an_ambiguous_name_still_taints() {
        let mut ctx = ctx();
        link(&ctx);
        feed(&ctx, msg("+491", "Neha", 1, "a"));
        feed(&ctx, msg("+492", "Nena", 2, "b"));
        let err = read(&mut ctx, "ne", None).unwrap_err();
        assert!(err.contains("Several match"), "{err}");
        assert!(ctx.tainted);
    }

    #[tokio::test]
    async fn turning_off_during_the_confirm_sends_nothing() {
        let mut c = ctx();
        let fake = link(&c);
        let shared = c.shared.clone();
        let running = tokio::spawn(async move { send(&mut c, "+491701234567", "hi").await });
        tokio::task::yield_now().await;
        shared.whatsapp.off();
        assert!(shared.bridge.answer_pending(Answer::Confirm(true)));
        assert_eq!(running.await.unwrap(), Err("WhatsApp is off.".into()));
        assert!(fake.sent.lock().unwrap().is_empty());
    }

    #[test]
    fn echoes_count_once_and_only_for_a_minute() {
        let state = State::default();
        state.expect_echo("Done.");
        state.expect_echo("Done.");
        assert!(state.is_echo("Done."));
        assert!(!state.is_echo("Done."), "once");
        state.expect_echo("Sent.");
        let later = Instant::now() + ECHO_FOR;
        assert!(!state.is_echo_at("Sent.", later), "expired");
        // The same text sent again after the old copy expired counts afresh.
        state.expect_echo("Again.");
        state.age_echoes(ECHO_FOR);
        state.expect_echo("Again.");
        assert!(state.is_echo("Again."));
    }

    #[test]
    fn numbers_are_told_apart_from_names() {
        assert_eq!(
            as_number("+49 170 1234567").as_deref(),
            Some("+491701234567")
        );
        assert_eq!(as_number("Neha"), None);
        assert_eq!(as_number("Room 12"), None);
    }

    /// Runs send_file_in, answering a confirm with `answer` if one comes.
    async fn file_send(
        mut ctx: Ctx,
        to: &str,
        path: &str,
        folders: &[PathBuf],
        answer: bool,
    ) -> (Result<String, String>, bool, Ctx) {
        let shared = ctx.shared.clone();
        let (to, path, folders) = (to.to_string(), path.to_string(), folders.to_vec());
        let running = tokio::spawn(async move {
            let out = send_file_in(&mut ctx, &to, &path, Some(" the file "), &folders).await;
            (out, ctx)
        });
        tokio::task::yield_now().await;
        let asked = shared.bridge.answer_pending(Answer::Confirm(answer));
        let (out, ctx) = running.await.unwrap();
        (out, asked, ctx)
    }

    /// A folder holding `name` with `bytes` of content.
    fn folder_with(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = crate::testutil::temp_dir();
        std::fs::write(dir.join(name), bytes).unwrap();
        dir
    }

    #[test]
    fn media_types_follow_the_extension() {
        assert_eq!(media_of("a.JPG"), (MediaKind::Image, "image/jpeg"));
        assert_eq!(media_of("a.jpeg"), (MediaKind::Image, "image/jpeg"));
        assert_eq!(media_of("a.png"), (MediaKind::Image, "image/png"));
        assert_eq!(media_of("a.webp"), (MediaKind::Image, "image/webp"));
        assert_eq!(media_of("a.mp4"), (MediaKind::Video, "video/mp4"));
        assert_eq!(media_of("a.pdf"), (MediaKind::Document, "application/pdf"));
        assert_eq!(
            media_of("noext"),
            (MediaKind::Document, "application/octet-stream")
        );
        assert_eq!(size_text(1), "1 KB");
        assert_eq!(size_text(3 * 1024 * 1024 / 2), "1.5 MB");
    }

    #[test]
    fn files_are_found_by_path_or_by_name_in_the_user_folders() {
        let (a, b) = (folder_with("cv.pdf", b"a"), folder_with("cv.pdf", b"b"));
        let only = folder_with("notes.txt", b"n");
        let folders = [a.clone(), b.clone(), only.clone()];
        // Absolute: used as given (canonical).
        let abs = find_file(&a.join("cv.pdf").to_string_lossy(), &[]).unwrap();
        assert!(abs.ends_with("cv.pdf"));
        // A bare name found in one folder.
        let found = find_file("notes.txt", &folders).unwrap();
        assert!(found.starts_with(std::fs::canonicalize(&only).unwrap()) || found.exists());
        // In two folders: listed, not guessed.
        let err = find_file("cv.pdf", &folders).unwrap_err();
        assert!(err.starts_with("Several files match cv.pdf"), "{err}");
        assert_eq!(err.matches("cv.pdf").count(), 3, "{err}");
        assert!(find_file("nope.pdf", &folders).unwrap_err().contains("No file"));
        // A folder is not a file.
        assert!(find_file(&a.to_string_lossy(), &[]).is_err());
        assert!(find_file("..\\..", &folders).is_err());
    }

    #[tokio::test]
    async fn files_always_ask_below_carte_blanche_and_go_out_typed() {
        let folders = [folder_with("Report 1.pdf", b"%PDF-data")];
        for (tier, asks) in [
            (Tier::Conservative, true),
            (Tier::Competent, true),
            (Tier::CarteBlanche, false),
        ] {
            let mut c = ctx();
            c.cfg.tier = tier;
            // A saved, untainted recipient: text would go unasked on competent.
            phones::save(&c.shared.data_dir, "Neha", "+491701234567").unwrap();
            let fake = link(&c);
            let shared = c.shared.clone();
            let dir = c.shared.data_dir.clone();
            let (out, asked, _) = file_send(c, "neha", "report 1.pdf", &folders, true).await;
            assert_eq!(asked, asks, "{tier:?}");
            assert_eq!(out.unwrap(), "Sent Report 1.pdf to Neha.");
            let files = fake.files.lock().unwrap();
            let (chat, f) = &files[0];
            assert_eq!(chat, "+491701234567");
            assert_eq!(
                (f.kind, f.mime, f.name.as_str(), f.data.as_slice()),
                (
                    MediaKind::Document,
                    "application/pdf",
                    "Report 1.pdf",
                    &b"%PDF-data"[..]
                )
            );
            assert_eq!(f.caption.as_deref(), Some("the file"));
            let kept = shared.whatsapp.recent("+491701234567", 5);
            assert_eq!(kept[0].text, "[file] Report 1.pdf");
            assert_eq!(shared.whatsapp.sends.lock().unwrap().len(), 1);
            let log = std::fs::read_to_string(dir.join("actions.log")).unwrap();
            assert!(log.contains("Report 1.pdf") && log.contains("(9 bytes)"));
            assert!(!log.contains("%PDF-data"));
        }
    }

    #[tokio::test]
    async fn a_declined_file_sends_nothing_and_images_go_as_images() {
        let folders = [folder_with("pic.png", b"png")];
        let mut c = ctx();
        c.cfg.tier = Tier::Competent;
        let fake = link(&c);
        let (out, asked, c) = file_send(c, "+491701234567", "pic.png", &folders, false).await;
        assert!(asked);
        assert_eq!(out.unwrap(), "The user chose not to send it.");
        assert!(fake.files.lock().unwrap().is_empty());
        let (out, _, _) = file_send(c, "+491701234567", "pic.png", &folders, true).await;
        assert!(out.is_ok());
        assert_eq!(fake.files.lock().unwrap()[0].1.kind, MediaKind::Image);
    }

    #[tokio::test]
    async fn the_ai_data_dir_and_big_files_are_refused() {
        let mut c = ctx();
        c.cfg.tier = Tier::CarteBlanche;
        let fake = link(&c);
        let secret = c.shared.data_dir.join("whatsapp").join("session.db");
        std::fs::create_dir_all(secret.parent().unwrap()).unwrap();
        std::fs::write(&secret, "keys").unwrap();
        let err = send_file_in(&mut c, "+491701234567", &secret.to_string_lossy(), None, &[])
            .await
            .unwrap_err();
        assert!(err.contains("never sent"), "{err}");
        // Also through a bare name looked up in a folder that is the data dir.
        let dd = [c.shared.data_dir.clone()];
        let err = send_file_in(&mut c, "+491701234567", "actions.log", None, &dd).await;
        assert!(err.is_err());

        let dir = crate::testutil::temp_dir();
        let big = dir.join("big.bin");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_FILE + 1).unwrap();
        let err = send_file_in(&mut c, "+491701234567", &big.to_string_lossy(), None, &[])
            .await
            .unwrap_err();
        assert!(err.contains("limit"), "{err}");
        f.set_len(0).unwrap();
        let err = send_file_in(&mut c, "+491701234567", &big.to_string_lossy(), None, &[])
            .await
            .unwrap_err();
        assert!(err.contains("empty"), "{err}");
        assert!(fake.files.lock().unwrap().is_empty());
        // UNC paths are refused by resolve_local.
        assert!(find_file(r"\\server\share\x.pdf", &[]).is_err());
    }

    #[test]
    fn remove_session_deletes_the_folder() {
        let dir = crate::testutil::temp_dir().join("whatsapp");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("session.db"), "x").unwrap();
        remove_session(&dir).unwrap();
        assert!(!dir.exists());
        remove_session(&dir).unwrap();
    }
}
