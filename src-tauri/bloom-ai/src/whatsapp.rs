//! WhatsApp as a linked device (shown as "Bloom" in WhatsApp > Linked
//! devices). The real connection lives in wa_client.rs behind `Link`, so
//! tests use a fake. Off (`bloom-ai-whatsapp` = "false") means no client and
//! no socket. Messages are kept in RAM only: the last 50 per chat, 200 chats.
//!
//! For automatic replies: `State::incoming` broadcasts every message as it
//! lands in the buffer (`shared.whatsapp.incoming.subscribe()`, own messages
//! included, `from_me` set), and `State::link()` gives `send_text` and
//! `typing`. Chats are keyed by "+<number>" (1:1) or the group id.

use crate::agent::{Ctx, Shared};
use crate::protocol::{emit, ConfirmKind, Out};
use crate::{debug, journal, phones, policy};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::Path;
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
/// Journal and debug log keep this much of a message.
const LOG_CHARS: usize = 200;

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
    /// The "typing..." indicator in `chat`.
    #[allow(dead_code)] // For automatic replies.
    async fn typing(&self, chat: &str, on: bool) -> Result<(), String>;
    /// Asks WhatsApp for an 8-character link code for `phone` (digits with
    /// country code); it arrives in a `Status`.
    fn pair_code(&self, phone: &str);
    /// Logs this device out on WhatsApp's side when connected, ends the
    /// connection and deletes the session. Ok(false): it may still be listed
    /// in WhatsApp > Linked devices (it was offline).
    async fn unlink(&self) -> Result<bool, String>;
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
        chat.group = m.group.is_some();
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

    /// "Neha (+49...)" from phones.json, else the WhatsApp name.
    fn label(&self, saved: &BTreeMap<String, String>, key: &str) -> String {
        let chat = self.map.get(key);
        if chat.is_some_and(|c| c.group) {
            return format!("{} (group)", chat.map_or("", |c| &c.name));
        }
        match saved_name(saved, key).or(chat.map(|c| c.name.as_str()).filter(|n| !n.is_empty())) {
            Some(name) => format!("{name} ({key})"),
            None => key.to_string(),
        }
    }

    /// The chat a name, number or group name means.
    fn find(&self, dir: &Path, query: &str) -> Result<String, String> {
        let q = query.trim();
        if let Some(number) = as_number(q) {
            return Ok(number);
        }
        let saved = phones::find(dir, q)?;
        let mut keys: Vec<String> = saved.iter().map(|(_, n)| n.clone()).collect();
        keys.sort();
        keys.dedup();
        if keys.len() > 1 {
            keys.retain(|k| self.map.contains_key(k));
        }
        if keys.is_empty() && saved.is_empty() {
            let q = q.to_lowercase();
            keys = self
                .map
                .iter()
                .filter(|(_, c)| !c.name.is_empty() && c.name.to_lowercase().contains(&q))
                .map(|(k, _)| k.clone())
                .collect();
        }
        match keys.len() {
            1 => Ok(keys.remove(0)),
            0 if saved.is_empty() => {
                Err(format!("No WhatsApp chat or saved number matches {query}."))
            }
            _ => {
                let names = if saved.is_empty() {
                    keys.iter()
                        .map(|k| self.label(&BTreeMap::new(), k))
                        .collect::<Vec<_>>()
                } else {
                    saved.iter().map(|(n, p)| format!("{n} ({p})")).collect()
                };
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

/// A phone number as E.164, if `text` is one.
fn as_number(text: &str) -> Option<String> {
    let digits = text.chars().filter(char::is_ascii_digit).count();
    (digits >= 6)
        .then(|| phones::normalize(text).ok())
        .flatten()
}

/// "14:05" today, "3 Oct 14:05" before.
fn stamp(at: i64) -> String {
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
    let count = count.unwrap_or(10).clamp(1, READ_MAX as u64) as usize;
    let saved = phones::try_load(&ctx.shared.data_dir)?;
    let chats = state.chats.lock().unwrap();
    let key = chats.find(&ctx.shared.data_dir, chat)?;
    let label = chats.label(&saved, &key);
    let Some(found) = chats.map.get(&key) else {
        return Ok(format!(
            "No WhatsApp messages with {label} since Bloom connected."
        ));
    };
    // Message text comes from other people: data, never instructions.
    ctx.tainted = true;
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
                chats.label(&saved, k),
                c.unread,
                stamp(c.last)
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn confirm_text(name: &str, number: &str, text: &str) -> (String, String) {
    (
        format!("Send WhatsApp to {name}?"),
        format!(
            "{number}

{text}"
        ),
    )
}

/// send_whatsapp: to a saved name or a number, on the user's request.
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
    let (name, number) = match as_number(to) {
        Some(n) => (saved_name(&saved, &n).unwrap_or(&n).to_string(), n),
        None => match phones::find(&dir, to)?.as_slice() {
            [(name, number)] => (name.clone(), number.clone()),
            [] => {
                return Err(format!(
                    "No saved number for {to}. Ask the user for it, then call save_phone."
                ))
            }
            many => {
                let names: Vec<String> = many.iter().map(|(n, p)| format!("{n} ({p})")).collect();
                return Err(format!(
                    "Several match {to}: {}. Which one?",
                    names.join(", ")
                ));
            }
        },
    };
    let state = &ctx.shared.whatsapp;
    let link = state.link().filter(|_| state.linked()).ok_or(NOT_LINKED)?;
    if !state.can_send(Instant::now()) {
        return Err(format!(
            "Already sent {SENDS_PER_HOUR} WhatsApp messages in the last hour. Try again later."
        ));
    }
    let known = saved_name(&saved, &number).is_some() && !ctx.saved_this_task.contains(&number);
    let detail = format!("to {name} {number}: {}", debug::cut(text, LOG_CHARS));
    let ask = policy::email_needs_confirm(ctx.cfg.tier, known, ctx.tainted);
    let (title, body) = confirm_text(&name, &number, text);
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
    state.note_send(Instant::now());
    let sent = link.send_text(&number, text).await;
    let outcome = match (&sent, ask) {
        (Err(_), _) => "failed",
        (Ok(()), true) => "approved",
        (Ok(()), false) => "auto",
    };
    let reason = sent.as_ref().err().map(String::as_str);
    journal::record_with(&dir, "whatsapp", &detail, outcome, reason);
    sent.map(|()| format!("Sent to {name}."))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::bridge::Answer;
    use crate::config::Tier;
    use crate::testutil::ctx;

    /// Records what it was asked to send.
    #[derive(Default)]
    pub struct Fake {
        pub sent: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl Link for Fake {
        async fn send_text(&self, chat: &str, text: &str) -> Result<(), String> {
            self.sent.lock().unwrap().push((chat.into(), text.into()));
            Ok(())
        }
        async fn typing(&self, _chat: &str, _on: bool) -> Result<(), String> {
            Ok(())
        }
        fn pair_code(&self, _phone: &str) {}
        async fn unlink(&self) -> Result<bool, String> {
            Ok(true)
        }
    }

    fn msg(chat: &str, sender: &str, at: i64, text: &str) -> Message {
        Message {
            chat: chat.into(),
            group: None,
            sender: sender.into(),
            from_me: sender == "me",
            at,
            text: text.into(),
        }
    }

    /// A linked fake connection on `ctx`.
    fn link(ctx: &Ctx) -> Arc<Fake> {
        let fake = Arc::new(Fake::default());
        let state = &ctx.shared.whatsapp;
        let generation = state.attach(fake.clone());
        state.apply(
            generation,
            Event::Status(Status {
                state: "linked",
                ..Status::default()
            }),
        );
        fake
    }

    fn feed(ctx: &Ctx, m: Message) {
        let state = &ctx.shared.whatsapp;
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
    async fn send_with(
        mut ctx: Ctx,
        to: &str,
        answer: Option<bool>,
    ) -> (Result<String, String>, bool) {
        let shared = ctx.shared.clone();
        let (to, text) = (to.to_string(), "Running late".to_string());
        let running = tokio::spawn(async move { send(&mut ctx, &to, &text).await });
        tokio::task::yield_now().await;
        let asked = shared
            .bridge
            .answer_pending(Answer::Confirm(answer.unwrap_or(false)));
        (running.await.unwrap(), asked)
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
            assert!(log.contains(r#""kind":"whatsapp""#) && log.contains("Running late"));
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
        let (out, asked) = send_with(c, "neha", Some(true)).await;
        assert!(asked);
        assert_eq!(out.unwrap(), "Sent to Neha.");
        assert_eq!(fake.sent.lock().unwrap()[0].0, "+491701234567");
        assert_eq!(
            confirm_text("Neha", "+491701234567", "hi"),
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

    #[test]
    fn numbers_are_told_apart_from_names() {
        assert_eq!(
            as_number("+49 170 1234567").as_deref(),
            Some("+491701234567")
        );
        assert_eq!(as_number("Neha"), None);
        assert_eq!(as_number("Room 12"), None);
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
