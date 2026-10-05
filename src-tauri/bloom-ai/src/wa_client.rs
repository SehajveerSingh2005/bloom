//! The real WhatsApp connection (whatsapp-rust), on its own thread with its
//! own current-thread runtime, so decrypting and the session store never
//! hold up a request. Commands go in over a channel, events come out over
//! another. Lost connections retry after 5 s, 30 s, then every 5 minutes.
//! Nothing here logs: QR and link codes go to Settings only.
//!
//! Contacts: history sync stays off, but app-state sync is on (the library
//! does it on link). On connect the groups the user is in are fetched and,
//! at most once a day, the address book (`critical_unblock_low`) is synced
//! afresh; later address-book changes arrive as `ContactUpdate`s. Both land
//! in whatsapp\contacts.json (wa_contacts.rs).

use crate::wa_contacts::{self, Contact, Group};
use crate::whatsapp::{remove_session, Event, Link, Message, Status};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;
use whatsapp_rust::sync_task::MajorSyncTask;
use whatsapp_rust::wacore::appstate::patch_decode::WAPatchName;
use whatsapp_rust::wacore::store::DevicePropsOverride;

enum Cmd {
    Send(String, String, oneshot::Sender<Result<(), String>>),
    Typing(String, bool, oneshot::Sender<Result<(), String>>),
    PairCode(String),
    Unlink(oneshot::Sender<Result<bool, String>>),
}

struct Real(mpsc::UnboundedSender<Cmd>);

const GONE: &str = "The WhatsApp connection stopped.";

impl Real {
    async fn ask<T>(&self, cmd: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        self.0.send(cmd(tx)).ok()?;
        rx.await.ok()
    }
}

#[async_trait::async_trait]
impl Link for Real {
    async fn send_text(&self, chat: &str, text: &str) -> Result<(), String> {
        let (chat, text) = (chat.to_string(), text.to_string());
        self.ask(|tx| Cmd::Send(chat, text, tx))
            .await
            .unwrap_or(Err(GONE.into()))
    }

    async fn typing(&self, chat: &str, on: bool) -> Result<(), String> {
        let chat = chat.to_string();
        self.ask(|tx| Cmd::Typing(chat, on, tx))
            .await
            .unwrap_or(Err(GONE.into()))
    }

    fn pair_code(&self, phone: &str) {
        let _ = self.0.send(Cmd::PairCode(phone.into()));
    }

    async fn unlink(&self) -> Result<bool, String> {
        self.ask(Cmd::Unlink).await.unwrap_or(Err(GONE.into()))
    }
}

/// The previous connection's thread: a new one waits for it to end, so two
/// never use the session at once.
static LAST: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Connects with the session in `dir` (created on first link). Dropping
/// every returned `Link` disconnects and ends the thread.
pub fn start(dir: PathBuf, events: mpsc::UnboundedSender<Event>) -> Arc<dyn Link> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut last = LAST.lock().unwrap();
    let previous = last.take();
    *last = Some(std::thread::spawn(move || {
        if let Some(previous) = previous {
            let _ = previous.join();
        }
        run(&dir, &events, rx);
    }));
    Arc::new(Real(tx))
}

/// How a connection's supervisor ended.
enum Exit {
    Quit,
    /// `bool`: nothing stays linked on WhatsApp's side.
    Unlink(oneshot::Sender<Result<bool, String>>, bool),
    /// WhatsApp logged this device out (removed on the phone).
    LoggedOut,
}

fn run(dir: &Path, events: &mpsc::UnboundedSender<Event>, mut cmds: mpsc::UnboundedReceiver<Cmd>) {
    let mut logged_out = false;
    loop {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        let exit = rt.block_on(supervise(dir, events, &mut cmds, logged_out));
        // Ends every task still holding the session store, so it can be deleted.
        drop(rt);
        match exit {
            Exit::Quit => return,
            Exit::Unlink(done, told) => {
                let _ = done.send(remove_session(dir).map(|()| told));
                return;
            }
            Exit::LoggedOut => {
                let _ = remove_session(dir);
                logged_out = true;
            }
        }
    }
}

/// 5 s, 30 s, then every 5 minutes.
fn backoff(attempt: usize) -> Duration {
    Duration::from_secs([5, 30, 300][attempt.min(2)])
}

/// What one connection saw, written by its event handler.
#[derive(Default)]
struct Seen {
    status: Status,
    connected: bool,
    paired: bool,
    logged_out: bool,
    /// The QR codes ran out (or WhatsApp refused us): wait for the user.
    stopped: bool,
    groups: HashMap<String, String>,
    /// Address-book entries not saved yet: (phone number or LID, name).
    pending: Vec<(Jid, String)>,
    /// The connect-time sync is running: updates wait for it.
    refreshing: bool,
}

struct Conn {
    seen: Mutex<Seen>,
    events: mpsc::UnboundedSender<Event>,
    /// The session folder, which holds contacts.json.
    dir: PathBuf,
}

impl Conn {
    fn status(&self, change: impl FnOnce(&mut Status)) {
        let mut seen = self.seen.lock().unwrap();
        change(&mut seen.status);
        let _ = self.events.send(Event::Status(seen.status.clone()));
    }

    async fn handle(&self, event: &whatsapp_rust::prelude::Event, client: &Arc<Client>) {
        use whatsapp_rust::prelude::Event as E;
        match event {
            E::PairingQrCode(qr) => self.status(|s| {
                s.state = "not_linked";
                s.qr = Some(qr.code.clone());
            }),
            E::PairingCode(pc) => self.status(|s| s.code = Some(pc.code.clone())),
            E::PairingCodeError(_) => self.status(|s| {
                s.error =
                    Some("WhatsApp didn't give a link code. Wait a minute and try again.".into())
            }),
            E::PairSuccess(_) => {
                self.seen.lock().unwrap().paired = true;
                self.status(|s| {
                    s.state = "connecting";
                    s.qr = None;
                    s.code = None;
                    s.error = None;
                });
            }
            E::PairError(_) => self.status(|s| s.error = Some("Linking failed. Try again.".into())),
            E::Connected(_) => {
                self.seen.lock().unwrap().connected = true;
                let number = client.pn().map(|j| format!("+{}", j.user_base()));
                let name = Some(client.push_name()).filter(|n| !n.is_empty());
                let synced = wa_contacts::load(&self.dir).ok().map(|b| counts(&b));
                self.status(|s| {
                    *s = Status {
                        state: "linked",
                        number,
                        name,
                        synced,
                        ..Status::default()
                    }
                });
                offline(client).await;
                self.refresh(client).await;
            }
            // Recorded by `Book`: a change made on the phone, or the library's
            // own (re)sync, is merged at once unless `refresh` is running.
            E::ContactUpdate(_) => self.save(client, false, None).await,
            // The library goes "online" once it learns the user's name, too.
            E::SelfPushNameUpdated(u) => {
                let name = Some(u.new_name.clone()).filter(|n| !n.is_empty());
                self.status(|s| s.name = name);
                offline(client).await
            }
            E::LoggedOut(_) => self.seen.lock().unwrap().logged_out = true,
            E::PairingQrCodesExhausted(x) if x.disconnected => self.stopped(None),
            E::StreamReplaced(_) => {
                self.stopped(Some("WhatsApp was opened by another copy of Bloom.".into()))
            }
            E::ClientOutdated(_) => self.stopped(Some(
                "WhatsApp says this version of Bloom is too old.".into(),
            )),
            E::Messages(batch) => {
                for m in batch.iter() {
                    if let Some(m) = self.convert(client, &m.message, &m.info).await {
                        let _ = self.events.send(Event::Message(m));
                    }
                }
            }
            E::MarkChatAsReadUpdate(u) if u.action.read == Some(true) => {
                if let Some(chat) = chat_key(client, &u.jid, None).await {
                    let _ = self.events.send(Event::Read(chat));
                }
            }
            _ => {}
        }
    }

    /// The groups the user is in, and the whole address book when the saved
    /// one is a day old (or missing).
    async fn refresh(&self, client: &Arc<Client>) {
        self.seen.lock().unwrap().refreshing = true;
        let groups = client.groups().get_participating().await.ok().map(|all| {
            all.into_values()
                // A community's parent group takes no messages.
                .filter(|m| !m.is_parent_group)
                .map(|m| Group {
                    jid: m.id.to_non_ad_string(),
                    members: m
                        .size
                        .map(|n| n as usize)
                        .or(Some(m.participants.len()).filter(|n| *n > 0)),
                    subject: m.subject,
                })
                .collect()
        });
        let full = wa_contacts::stale(&self.dir);
        if full {
            // Names arrive through `Book` while this runs. A failed sync is
            // retried by the library, and its names arrive the same way.
            let task = MajorSyncTask::AppStateSync {
                name: WAPatchName::CriticalUnblockLow,
                full_sync: true,
            };
            client.process_sync_task(task).await;
        }
        self.seen.lock().unwrap().refreshing = false;
        self.save(client, full, groups).await;
    }

    /// Saves what `Book` recorded. `full`: it is the whole address book.
    async fn save(&self, client: &Arc<Client>, full: bool, groups: Option<Vec<Group>>) {
        let pending = {
            let mut seen = self.seen.lock().unwrap();
            if seen.refreshing {
                return;
            }
            std::mem::take(&mut seen.pending)
        };
        if pending.is_empty() && groups.is_none() {
            return;
        }
        let mut contacts = Vec::new();
        for (jid, name) in pending {
            if let Some(number) = chat_key(client, &jid, None).await {
                contacts.push(Contact { name, number });
            }
        }
        match wa_contacts::update(&self.dir, full, contacts, groups) {
            Ok(book) => self.status(|s| s.synced = Some(counts(&book))),
            Err(e) => self.status(|s| s.error = Some(e)),
        }
    }

    fn stopped(&self, error: Option<String>) {
        self.seen.lock().unwrap().stopped = true;
        self.status(|s| {
            s.qr = None;
            s.error = error;
        });
    }

    async fn convert(
        &self,
        client: &Arc<Client>,
        msg: &wa::Message,
        info: &MessageInfo,
    ) -> Option<Message> {
        let text = describe(msg)?;
        let src = &info.source;
        let group = if src.is_group {
            Some(self.group_name(client, &src.chat).await)
        } else {
            None
        };
        let alt = if src.is_from_me {
            src.recipient_alt.as_ref()
        } else {
            src.sender_alt.as_ref()
        };
        let chat = match &group {
            Some(_) => src.chat.to_non_ad_string(),
            None => chat_key(client, &src.chat, alt).await?,
        };
        let sender = if src.is_from_me {
            "me".to_string()
        } else if !info.push_name.is_empty() {
            info.push_name.clone()
        } else {
            chat_key(client, &src.sender, src.sender_alt.as_ref())
                .await
                .unwrap_or_default()
        };
        Some(Message {
            chat,
            group,
            sender,
            from_me: src.is_from_me,
            at: info.timestamp.timestamp(),
            text,
            forwarded: forwarded(msg),
        })
    }

    /// The group's subject, asked once per group.
    async fn group_name(&self, client: &Arc<Client>, jid: &Jid) -> String {
        let key = jid.to_non_ad_string();
        if let Some(name) = self.seen.lock().unwrap().groups.get(&key) {
            return name.clone();
        }
        let name = match client.groups().get_metadata(jid).await {
            Ok(meta) => meta.subject,
            Err(_) => return "Group".into(),
        };
        self.seen.lock().unwrap().groups.insert(key, name.clone());
        name
    }
}

/// Records address-book names as app-state sync dispatches them. Runs inline,
/// so a full sync's names are all in by the time it returns.
struct Book(Arc<Conn>);

impl EventHandler for Book {
    fn handle_event(&self, event: Arc<whatsapp_rust::prelude::Event>) {
        if let whatsapp_rust::prelude::Event::ContactUpdate(u) = &*event {
            if let Some(entry) = address_entry(&u.jid, &u.action) {
                self.0.seen.lock().unwrap().pending.push(entry);
            }
        }
    }

    fn interest(&self) -> EventInterest {
        EventInterest::of(&[EventKind::ContactUpdate])
    }
}

/// (the phone-number JID if given, else the chat's JID; the saved name).
fn address_entry(
    jid: &Jid,
    action: &wa::sync_action_value::ContactAction,
) -> Option<(Jid, String)> {
    let name = [&action.full_name, &action.first_name]
        .into_iter()
        .flatten()
        .map(|n| n.trim())
        .find(|n| !n.is_empty())?;
    let pn = action
        .pn_jid
        .as_deref()
        .and_then(|p| p.parse::<Jid>().ok())
        .filter(Jid::is_pn);
    Some((pn.unwrap_or_else(|| jid.clone()), name.to_string()))
}

/// (contacts, groups) for Settings.
fn counts(book: &wa_contacts::Book) -> (usize, usize) {
    (book.contacts.len(), book.groups.len())
}

/// Without this the phone stops showing notifications while Bloom is linked.
async fn offline(client: &Arc<Client>) {
    let _ = client.presence().set_unavailable().await;
}

/// "+<number>" for a person (LIDs looked up), None for status updates,
/// channels and the like.
async fn chat_key(client: &Arc<Client>, jid: &Jid, alt: Option<&Jid>) -> Option<String> {
    if !jid.is_pn() && !jid.is_lid() {
        return None;
    }
    if let Some(key) = direct_key(jid, alt, client.lid().as_ref(), client.pn().as_ref()) {
        return Some(key);
    }
    match client.get_lid_pn_entry(jid).await {
        Ok(Some(entry)) => Some(format!("+{}", entry.phone_number)),
        _ => Some(jid.to_non_ad_string()),
    }
}

/// "+<number>" when no lookup is needed: a phone-number JID, the user's own
/// LID (their own chat may come by it), or a phone-number `alt`.
fn direct_key(
    jid: &Jid,
    alt: Option<&Jid>,
    own_lid: Option<&Jid>,
    own_pn: Option<&Jid>,
) -> Option<String> {
    let number = |j: &Jid| format!("+{}", j.user_base());
    if jid.is_pn() {
        return Some(number(jid));
    }
    if let (Some(lid), Some(pn)) = (own_lid, own_pn) {
        if jid.is_lid() && jid.user_base() == lid.user_base() {
            return Some(number(pn));
        }
    }
    alt.filter(|a| a.is_pn()).map(number)
}

/// Forwarded (marked so by WhatsApp): someone else's words, even in the
/// user's own chat. Plain `conversation` text can't carry the mark: a
/// forward always comes as extended text or media.
fn forwarded(msg: &wa::Message) -> bool {
    let m = msg.get_base_message();
    let infos = [
        m.extended_text_message.as_option().map(|x| &x.context_info),
        m.image_message.as_option().map(|x| &x.context_info),
        m.video_message.as_option().map(|x| &x.context_info),
        m.audio_message.as_option().map(|x| &x.context_info),
        m.document_message.as_option().map(|x| &x.context_info),
        m.sticker_message.as_option().map(|x| &x.context_info),
        m.location_message.as_option().map(|x| &x.context_info),
        m.live_location_message.as_option().map(|x| &x.context_info),
        m.contact_message.as_option().map(|x| &x.context_info),
        m.contacts_array_message
            .as_option()
            .map(|x| &x.context_info),
    ];
    infos
        .into_iter()
        .flatten()
        .filter_map(|c| c.as_option())
        .any(|c| c.is_forwarded == Some(true) || c.forwarding_score.unwrap_or(0) > 0)
}

/// The text, or "[photo] caption" and the like; None for reactions, edits
/// and other protocol messages.
fn describe(msg: &wa::Message) -> Option<String> {
    if let Some(text) = msg.text_content() {
        return Some(text.to_string());
    }
    let m = msg.get_base_message();
    let kind = if m.image_message.is_set() {
        "[photo]"
    } else if m.video_message.is_set() {
        "[video]"
    } else if m.audio_message.is_set() {
        "[voice message]"
    } else if m.document_message.is_set() {
        "[document]"
    } else if m.sticker_message.is_set() {
        "[sticker]"
    } else if m.location_message.is_set() || m.live_location_message.is_set() {
        "[location]"
    } else if m.contact_message.is_set() || m.contacts_array_message.is_set() {
        "[contact]"
    } else {
        return None;
    };
    Some(match msg.get_caption().filter(|c| !c.is_empty()) {
        Some(caption) => format!("{kind} {caption}"),
        None => kind.to_string(),
    })
}

/// A person's "+<number>" or a group id as a JID.
fn jid(chat: &str) -> Result<Jid, String> {
    match chat.strip_prefix('+') {
        Some(digits) => Ok(Jid::pn(digits)),
        None => chat
            .parse()
            .map_err(|_| format!("{chat} is not a WhatsApp chat")),
    }
}

/// Between connections: waits out `wait` (forever if None) while answering
/// commands. Returns the exit, or None to connect. `paired`: the session
/// is linked on the phone.
async fn between(
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
    wait: Option<Duration>,
    pair: &mut Option<String>,
    paired: bool,
) -> Option<Exit> {
    let sleep = async {
        match wait {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => return None,
            cmd = cmds.recv() => match cmd {
                None => return Some(Exit::Quit),
                Some(Cmd::Unlink(done)) => return Some(Exit::Unlink(done, !paired)),
                Some(Cmd::PairCode(phone)) => {
                    *pair = Some(phone);
                    return None;
                }
                Some(Cmd::Send(_, _, done) | Cmd::Typing(_, _, done)) => {
                    let _ = done.send(Err("WhatsApp isn't connected right now.".into()));
                }
            }
        }
    }
}

async fn supervise(
    dir: &Path,
    events: &mpsc::UnboundedSender<Event>,
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
    logged_out: bool,
) -> Exit {
    let mut attempt = 0;
    let mut pair = None;
    let mut wait = None;
    let mut idle = logged_out;
    let mut paired = false;
    // "+<number>" while a linked session exists: Settings offers Unlink.
    let mut number: Option<String>;
    if logged_out {
        let _ = events.send(Event::Status(Status {
            state: "not_linked",
            error: Some("WhatsApp unlinked Bloom. Link it again to keep using WhatsApp.".into()),
            ..Status::default()
        }));
    }
    loop {
        if idle || wait.is_some() {
            if let Some(exit) = between(cmds, wait.take(), &mut pair, paired).await {
                return exit;
            }
            idle = false;
        }
        let conn = Arc::new(Conn {
            seen: Mutex::default(),
            events: events.clone(),
            dir: dir.to_path_buf(),
        });
        conn.status(|s| s.state = "connecting");
        let bot = match build(dir, &conn, pair.take()).await {
            Ok(bot) => bot,
            Err(e) => {
                conn.status(|s| s.error = Some(e));
                wait = Some(backoff(attempt));
                attempt += 1;
                continue;
            }
        };
        let client = bot.client();
        paired = client.pn().is_some();
        number = client.pn().map(|j| format!("+{}", j.user_base()));
        conn.status(|s| s.number = number.clone());
        // Reconnects follow our own backoff below.
        client
            .enable_auto_reconnect
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let mut handle = bot.spawn();
        let end = loop {
            tokio::select! {
                _ = &mut handle => break None,
                cmd = cmds.recv() => match cmd {
                    None => break Some(Exit::Quit),
                    Some(Cmd::Unlink(done)) => {
                        let told = client.pn().is_none()
                            || (client.is_connected() && client.is_logged_in());
                        break Some(Exit::Unlink(done, told));
                    }
                    Some(Cmd::PairCode(phone)) if !client.is_logged_in() => {
                        if client.is_connected() {
                            let c = client.clone();
                            tokio::spawn(async move {
                                // Failures arrive as PairingCodeError.
                                let _ = c.pair_with_code(code_options(phone)).await;
                            });
                        } else {
                            pair = Some(phone);
                            break None;
                        }
                    }
                    Some(Cmd::PairCode(_)) => {}
                    Some(Cmd::Send(chat, text, done)) => {
                        let c = client.clone();
                        tokio::spawn(async move {
                            let sent = match jid(&chat) {
                                Ok(to) => c.send_text(to, text).await.map(drop).map_err(|e| e.to_string()),
                                Err(e) => Err(e),
                            };
                            let _ = done.send(sent);
                        });
                    }
                    Some(Cmd::Typing(chat, on, done)) => {
                        let c = client.clone();
                        tokio::spawn(async move {
                            let typed = match jid(&chat) {
                                Ok(to) if on => c.chatstate().send_composing(&to).await.map_err(|e| e.to_string()),
                                Ok(to) => c.chatstate().send_paused(&to).await.map_err(|e| e.to_string()),
                                Err(e) => Err(e),
                            };
                            let _ = done.send(typed);
                        });
                    }
                }
            }
        };
        match end {
            Some(Exit::Unlink(done, told)) => {
                client.logout().await;
                let _ = (&mut handle).await;
                return Exit::Unlink(done, told);
            }
            Some(exit) => {
                handle.shutdown().await;
                return exit;
            }
            None if pair.is_some() => {
                // A link code was asked for while offline: start over with it.
                handle.shutdown().await;
                continue;
            }
            None => {}
        }
        drop(client);
        let seen = std::mem::take(&mut *conn.seen.lock().unwrap());
        if seen.logged_out {
            return Exit::LoggedOut;
        }
        if seen.paired {
            // WhatsApp drops the socket right after linking; reconnect at once.
            attempt = 0;
            continue;
        }
        if seen.stopped {
            conn.status(|s| {
                s.state = "not_linked";
                s.number = number.clone();
            });
            idle = true;
            continue;
        }
        if seen.connected {
            attempt = 0;
        }
        conn.status(|s| {
            s.state = "connecting";
            s.number = number.clone();
        });
        wait = Some(backoff(attempt));
        attempt += 1;
    }
}

fn code_options(phone: String) -> PairCodeOptions {
    PairCodeOptions {
        phone_number: phone,
        ..Default::default()
    }
}

async fn build(dir: &Path, conn: &Arc<Conn>, pair: Option<String>) -> Result<Bot, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    let db = dir.join("session.db");
    let store = SqliteStore::new(&db.to_string_lossy())
        .await
        .map_err(|e| format!("Couldn't open the WhatsApp session: {e}"))?;
    let c = conn.clone();
    let mut builder = Bot::builder()
        .with_backend(store)
        // Old chats stay on the phone: only new messages are kept, in RAM.
        .skip_history_sync()
        // Sent messages are kept in session.db only for retry receipts, which
        // come within seconds; 120 s (swept every few minutes) instead of the
        // default 2 h keeps message text off the disk as far as possible.
        .with_cache_config(whatsapp_rust::CacheConfig {
            sent_message_ttl_secs: 120,
            ..Default::default()
        })
        .with_device_props(DevicePropsOverride::new().with_os("Bloom"))
        .with_event_handler(Book(conn.clone()))
        .on_event(move |event, client| {
            let c = c.clone();
            async move { c.handle(&event, &client).await }
        });
    if let Some(phone) = pair {
        builder = builder.with_pair_code(code_options(phone));
    }
    builder
        .build()
        .await
        .map_err(|e| format!("Couldn't start WhatsApp: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_5s_30s_then_5min() {
        let secs: Vec<u64> = (0..5).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(secs, [5, 30, 300, 300, 300]);
    }

    #[test]
    fn chats_parse_as_jids() {
        assert_eq!(
            jid("+491701234567").unwrap().to_string(),
            "491701234567@s.whatsapp.net"
        );
        assert_eq!(jid("1203630@g.us").unwrap().to_string(), "1203630@g.us");
    }

    #[test]
    fn media_is_described() {
        let text = wa::Message::text("hi");
        assert_eq!(describe(&text).as_deref(), Some("hi"));
        let photo = wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                caption: Some("look".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(describe(&photo).as_deref(), Some("[photo] look"));
        assert_eq!(describe(&wa::Message::default()), None);
    }

    #[test]
    fn forwards_are_marked() {
        let typed = wa::Message::text("Janice, hi");
        assert!(!forwarded(&typed));
        let forward = typed.prepare_for_forward();
        assert_eq!(describe(&forward).as_deref(), Some("Janice, hi"));
        assert!(forwarded(&forward));
        let photo = wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                caption: Some("Janice, hi".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(!forwarded(&photo));
        assert!(forwarded(&photo.prepare_for_forward()));
        // Only the score set (no flag) still counts.
        let mut scored = photo.clone();
        scored.image_message.as_option_mut().unwrap().context_info =
            MessageField::some(wa::ContextInfo {
                forwarding_score: Some(2),
                ..Default::default()
            });
        assert!(forwarded(&scored));
        // A reply quoting a forward: the user's own text, not a forward.
        let quoting = wa::Message {
            extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
                text: Some("YES".into()),
                context_info: MessageField::some(wa::ContextInfo {
                    quoted_message: MessageField::some(*forward),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(describe(&quoting).as_deref(), Some("YES"));
        assert!(!forwarded(&quoting));
    }

    #[test]
    fn address_book_entries_take_the_number_and_the_best_name() {
        use wa::sync_action_value::ContactAction;
        let lid = Jid::lid("12345");
        let full = ContactAction {
            full_name: Some(" Neha Sharma ".into()),
            first_name: Some("Neha".into()),
            pn_jid: Some("491701234567@s.whatsapp.net".into()),
            ..Default::default()
        };
        let (jid, name) = address_entry(&lid, &full).unwrap();
        assert_eq!(
            (jid.to_string().as_str(), name.as_str()),
            ("491701234567@s.whatsapp.net", "Neha Sharma")
        );
        // No number given: the chat's JID (a LID is looked up when saving).
        let first = ContactAction {
            full_name: Some(String::new()),
            first_name: Some("Sam".into()),
            ..Default::default()
        };
        assert_eq!(
            address_entry(&lid, &first),
            Some((lid.clone(), "Sam".into()))
        );
        assert_eq!(address_entry(&lid, &ContactAction::default()), None);
    }

    #[test]
    fn the_own_lid_is_the_own_number() {
        let (lid, pn) = (Jid::lid("98765"), Jid::pn("4915550000001"));
        let own = |j: &Jid| direct_key(j, None, Some(&lid), Some(&pn));
        assert_eq!(own(&Jid::lid("98765")).as_deref(), Some("+4915550000001"));
        assert_eq!(own(&pn).as_deref(), Some("+4915550000001"));
        // Someone else's LID needs a lookup, and its fallback key is never
        // the own number.
        let other = Jid::lid("12345");
        assert_eq!(own(&other), None);
        assert!(other.to_non_ad_string().ends_with("@lid"));
        let alt = Jid::pn("491701234567");
        assert_eq!(
            direct_key(&other, Some(&alt), Some(&lid), Some(&pn)).as_deref(),
            Some("+491701234567")
        );
        // Not linked yet: no own LID to match.
        assert_eq!(direct_key(&Jid::lid("98765"), None, None, None), None);
    }
}
