//! "Answer me in my own chat" (`bloom-ai-whatsapp-selfchat`, off by default).
//!
//! A message the user writes in their own WhatsApp chat ("Message yourself")
//! that starts with the assistant's name ("Janice, ...") runs as a normal
//! request: full tools, the usual tiers. The reply goes back to that chat.
//! A question (confirm) shows on the PC and is also asked in the chat;
//! whichever answers first wins. In the chat only "YES" within 5 minutes
//! approves, and any other message cancels. Off: the chat is ignored.
//!
//! The user's own chat is the linked account's number. Only the user's own
//! messages there count; Janice's own messages are never requests (they are
//! noted as echoes and never start with her name).

use crate::agent::Shared;
use crate::bridge::Answer;
use crate::config::Config;
use crate::journal;
use crate::protocol::{emit, Out};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, Instant};

/// Task ids for requests from the phone, above wake requests' (1e9).
const FIRST_TASK: u64 = 2_000_000_000;
const PER_HOUR: usize = 30;
const HOUR: Duration = Duration::from_secs(60 * 60);
const ANSWER_WITHIN: Duration = Duration::from_secs(5 * 60);
/// Older messages are a backlog from being offline: not run.
const MAX_AGE_SECS: i64 = 5 * 60;
const MAX_TEXT: usize = 4096;
const ASK: &str = "Reply YES to approve, anything else cancels.";

static NEXT: AtomicU64 = AtomicU64::new(FIRST_TASK);

/// The request came from the phone: the PC panel only sees its questions.
pub fn is_phone(task: u64) -> bool {
    task >= FIRST_TASK
}

/// "Janice, ...", "janice: ..." or "JANICE ..." with something after it.
fn is_request(text: &str, name: &str) -> bool {
    let text = text.trim();
    let head: String = text.chars().take(name.chars().count()).collect();
    let rest = &text[head.len()..];
    let sep = |c: char| c == ',' || c == ':' || c.is_whitespace();
    head.to_lowercase() == name.to_lowercase()
        && rest.starts_with(sep)
        && !rest.trim_start_matches(sep).is_empty()
}

type Job = Pin<Box<dyn Future<Output = Result<String, String>> + Send>>;
/// Runs a request: the agent, or a fake in tests.
type Runner = Arc<dyn Fn(u64, String) -> Job + Send + Sync>;

/// Waits on the user's own chat for the life of the agent.
pub async fn run(shared: Arc<Shared>) {
    let s = shared.clone();
    let runner: Runner =
        Arc::new(move |task, text| Box::pin(crate::agent::run(task, text, s.clone())));
    run_with(shared, runner).await
}

/// The request running, and whether it showed a question on the PC.
struct Current {
    handle: JoinHandle<()>,
    asked: Arc<AtomicBool>,
}

async fn run_with(shared: Arc<Shared>, runner: Runner) {
    let (tx, mut questions) = mpsc::unbounded_channel();
    *shared.bridge.phone.lock().unwrap() = Some(tx);
    let mut rx = shared.whatsapp.incoming.subscribe();
    let mut open: Option<Open> = None;
    let mut hour: VecDeque<Instant> = VecDeque::new();
    let mut current: Option<Current> = None;
    loop {
        let deadline = open.as_ref().map(|o| o.until);
        let m = tokio::select! {
            // A question is registered before chat messages already queued.
            biased;
            q = questions.recv() => {
                let Some((id, question)) = q else { return };
                if let Some(c) = &current {
                    c.asked.store(true, Ordering::Relaxed);
                }
                open = ask(&shared, id, &question).await;
                continue;
            }
            _ = sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                if let Some(o) = open.take() {
                    shared.bridge.answer(o.id, Answer::Confirm(false));
                }
                continue;
            }
            m = rx.recv() => match m {
                Ok(m) => m,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            },
        };
        let own = shared.whatsapp.own_number();
        // A forward is someone else's words, even here: never a request or
        // an answer.
        if !m.from_me || m.forwarded || m.group.is_some() || own.as_deref() != Some(m.chat.as_str())
        {
            continue;
        }
        if shared.whatsapp.is_echo(&m.text) {
            continue;
        }
        let cfg = Config::load(&shared.settings_path);
        if !(cfg.enabled && cfg.whatsapp && cfg.self_chat)
            || chrono::Utc::now().timestamp() - m.at > MAX_AGE_SECS
        {
            continue;
        }
        // Only a message written after the question answers it.
        if let Some(o) = open.take_if(|o| m.at >= o.asked_at) {
            if shared.bridge.is_open(o.id) {
                let yes = matches!(m.text.trim(), "YES" | "yes");
                shared.bridge.answer(o.id, Answer::Confirm(yes));
                continue;
            }
        }
        if !is_request(&m.text, &cfg.name) {
            continue;
        }
        let dir = shared.data_dir.clone();
        // No message text in actions.log.
        let detail = format!("from the phone ({} chars)", m.text.chars().count());
        let now = Instant::now();
        while hour.front().is_some_and(|t| now.duration_since(*t) >= HOUR) {
            hour.pop_front();
        }
        if current.as_ref().is_some_and(|c| !c.handle.is_finished()) {
            journal::record(&dir, "whatsapp-request", &detail, "busy");
            say(&shared, "Still working on your last request.").await;
            continue;
        }
        if hour.len() >= PER_HOUR {
            journal::record(&dir, "whatsapp-request", &detail, "limited");
            say(
                &shared,
                "That's 30 requests in the last hour. Try again later.",
            )
            .await;
            continue;
        }
        hour.push_back(now);
        let task = NEXT.fetch_add(1, Ordering::Relaxed);
        journal::record(&dir, "whatsapp-request", &detail, "started");
        emit(&Out::WhatsappRequest);
        let asked = Arc::new(AtomicBool::new(false));
        let (s, a, job) = (shared.clone(), asked.clone(), runner(task, m.text));
        let handle = tokio::spawn(async move {
            let result = job.await;
            // The PC showed its question: end it there too.
            if a.load(Ordering::Relaxed) {
                crate::finish(task, result.clone());
            }
            let (outcome, reply) = match result {
                Ok(reply) => ("done", reply),
                Err(e) => ("failed", e),
            };
            journal::record(&dir, "whatsapp-request", &detail, outcome);
            say(&s, &reply).await;
        });
        current = Some(Current { handle, asked });
    }
}

/// A question waiting for its answer in the chat.
struct Open {
    id: u64,
    /// Unix seconds: older messages don't answer it.
    asked_at: i64,
    until: Instant,
}

/// Asks question `id` in the chat, unless the PC already answered it.
async fn ask(shared: &Shared, id: u64, question: &str) -> Option<Open> {
    if !shared.bridge.is_open(id) {
        return None;
    }
    let open = Open {
        id,
        asked_at: chrono::Utc::now().timestamp(),
        until: Instant::now() + ANSWER_WITHIN,
    };
    say(shared, &format!("{question}\n\n{ASK}")).await;
    Some(open)
}

/// Writes in the user's own chat. Never starts with the assistant's name, so
/// it can't read as a request.
async fn say(shared: &Shared, text: &str) {
    let state = &shared.whatsapp;
    let (Some(own), Some(link)) = (state.own_number(), state.link()) else {
        return;
    };
    let name = Config::load(&shared.settings_path).name;
    let mut text: String = text.trim().chars().take(MAX_TEXT).collect();
    if text.is_empty() {
        return;
    }
    if is_request(&text, &name) {
        text = format!("> {text}");
    }
    state.expect_echo(&text);
    if link.send_text(&own, &text).await.is_ok() {
        state.record_sent(&own, &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ConfirmKind;
    use crate::testutil::ctx;
    use crate::whatsapp::tests::{feed_state, link_state, msg, Fake, ME};
    use serde_json::{json, Value};
    use std::sync::Mutex;
    use tokio::time::sleep;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn requests_start_with_the_name() {
        for yes in [
            "Janice, what's the weather",
            "janice: mute",
            "  JANICE turn it up",
            "Janice,\nhi",
        ] {
            assert!(is_request(yes, "Janice"), "{yes}");
        }
        for no in [
            "Janice",
            "Janice, ",
            "Janicex hi",
            "hey Janice, hi",
            "Jan",
            "",
            "> Janice, done",
        ] {
            assert!(!is_request(no, "Janice"), "{no}");
        }
        assert!(is_request("émile: hi", "Émile"));
        assert!(is_phone(FIRST_TASK) && !is_phone(crate::wake::FIRST_TASK));
    }

    /// A Shared with these settings, linked as ME.
    fn shared(settings: Value) -> (Arc<Shared>, Arc<Fake>) {
        let mut c = ctx();
        let s = Arc::get_mut(&mut c.shared).unwrap();
        s.settings_path = s.data_dir.join("settings.json");
        std::fs::write(&s.settings_path, settings.to_string()).unwrap();
        let fake = link_state(&c.shared.whatsapp);
        (c.shared, fake)
    }

    fn on() -> Value {
        json!({
            "bloom-ai-enabled": "true",
            "bloom-ai-whatsapp": "true",
            "bloom-ai-whatsapp-selfchat": "true",
            "bloom-ai-model": "m",
        })
    }

    /// From the user, just now.
    fn mine(chat: &str, text: &str) -> crate::whatsapp::Message {
        msg(chat, "me", chrono::Utc::now().timestamp(), text)
    }

    type Seen = Arc<Mutex<Vec<(u64, String, Option<bool>)>>>;

    /// The engine with a fake agent that asks once, then answers "Sent." or
    /// "Not sent.". Records (task, text, the answer).
    fn start(s: &Arc<Shared>, ask: bool) -> Seen {
        let seen: Seen = Arc::default();
        let (log, sh) = (seen.clone(), s.clone());
        let runner: Runner = Arc::new(move |task, text| {
            let (log, sh) = (log.clone(), sh.clone());
            Box::pin(async move {
                let answer = match ask {
                    true => Some(
                        sh.bridge
                            .confirm(
                                task,
                                ConfirmKind::Message,
                                "Send WhatsApp to Neha?".into(),
                                "+491701234567\n\nhi".into(),
                            )
                            .await,
                    ),
                    false => None,
                };
                log.lock().unwrap().push((task, text, answer));
                Ok(match answer {
                    Some(false) => "Not sent.".into(),
                    _ => "Sent.".into(),
                })
            })
        });
        tokio::spawn(run_with(s.clone(), runner));
        seen
    }

    fn sent(fake: &Fake) -> Vec<String> {
        let sent = fake.sent.lock().unwrap();
        assert!(
            sent.iter().all(|(chat, _)| chat == ME),
            "only to the own chat"
        );
        sent.iter().map(|(_, t)| t.clone()).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn yes_in_the_chat_approves_and_the_reply_goes_there() {
        let (s, fake) = shared(on());
        let seen = start(&s, true);
        tokio::task::yield_now().await;
        let text = "Janice, text Neha hi";
        feed_state(&s.whatsapp, mine(ME, text));
        sleep(secs(1)).await;
        let question = "Send WhatsApp to Neha?\n+491701234567\n\nhi\n\nReply YES to approve, anything else cancels.";
        assert_eq!(sent(&fake), [question]);
        assert!(seen.lock().unwrap().is_empty(), "waiting for the answer");
        feed_state(&s.whatsapp, mine(ME, " YES "));
        sleep(secs(1)).await;
        let (task, asked, answer) = seen.lock().unwrap()[0].clone();
        assert!(is_phone(task));
        assert_eq!((asked.as_str(), answer), (text, Some(true)));
        assert_eq!(sent(&fake), [question, "Sent."]);
        // Janice's messages are in the chat, and never run as requests.
        let kept: Vec<String> = s
            .whatsapp
            .recent(ME, 10)
            .into_iter()
            .map(|m| m.text)
            .collect();
        assert_eq!(kept, [text, question, " YES ", "Sent."]);
        let log = std::fs::read_to_string(s.data_dir.join("actions.log")).unwrap();
        assert!(log.contains(r#""kind":"whatsapp-request""#), "{log}");
        assert!(log.contains(r#""outcome":"started""#) && log.contains(r#""outcome":"done""#));
        assert!(log.contains("(20 chars)") && !log.contains("Neha hi"));
    }

    #[tokio::test(start_paused = true)]
    async fn no_answer_in_5_minutes_or_anything_else_is_no() {
        for answer in [None, Some("yes please"), Some("Yes")] {
            let (s, fake) = shared(on());
            let seen = start(&s, true);
            tokio::task::yield_now().await;
            feed_state(&s.whatsapp, mine(ME, "Janice, text Neha hi"));
            sleep(secs(1)).await;
            match answer {
                Some(a) => feed_state(&s.whatsapp, mine(ME, a)),
                None => {
                    sleep(secs(4 * 60 + 50)).await;
                    assert!(seen.lock().unwrap().is_empty(), "still waiting");
                }
            }
            sleep(secs(20)).await;
            assert_eq!(seen.lock().unwrap()[0].2, Some(false), "{answer:?}");
            assert_eq!(sent(&fake)[1], "Not sent.");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_pc_can_answer_first() {
        let (s, fake) = shared(on());
        let seen = start(&s, true);
        tokio::task::yield_now().await;
        feed_state(&s.whatsapp, mine(ME, "Janice, text Neha hi"));
        sleep(secs(1)).await;
        assert!(s.bridge.answer_pending(Answer::Confirm(true)));
        sleep(secs(1)).await;
        assert_eq!(seen.lock().unwrap()[0].2, Some(true));
        // A late "no" in the chat answers nothing and runs nothing.
        feed_state(&s.whatsapp, mine(ME, "no"));
        sleep(secs(1)).await;
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(sent(&fake).len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_request_on_the_pc_leaves_the_phone_question_open() {
        let (s, _fake) = shared(on());
        let seen = start(&s, true);
        tokio::task::yield_now().await;
        feed_state(&s.whatsapp, mine(ME, "Janice, text Neha hi"));
        sleep(secs(1)).await;
        s.bridge.drop_all();
        feed_state(&s.whatsapp, mine(ME, "YES"));
        sleep(secs(1)).await;
        assert_eq!(seen.lock().unwrap()[0].2, Some(true));
    }

    #[tokio::test(start_paused = true)]
    async fn only_the_users_requests_in_their_own_chat_run() {
        let (s, fake) = shared(on());
        let seen = start(&s, false);
        tokio::task::yield_now().await;
        let others = msg(ME, "Neha", chrono::Utc::now().timestamp(), "Janice, hi");
        feed_state(&s.whatsapp, others);
        feed_state(&s.whatsapp, mine("+491701234567", "Janice, hi"));
        feed_state(&s.whatsapp, mine(ME, "note to self"));
        feed_state(&s.whatsapp, msg(ME, "me", 1, "Janice, from the backlog"));
        // A message this PC sent there, handed back by WhatsApp.
        s.whatsapp.expect_echo("Janice, from the PC");
        feed_state(&s.whatsapp, mine(ME, "Janice, from the PC"));
        sleep(secs(5)).await;
        assert!(seen.lock().unwrap().is_empty());
        assert!(sent(&fake).is_empty());
        feed_state(&s.whatsapp, mine(ME, "janice: hi"));
        sleep(secs(1)).await;
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(sent(&fake), ["Sent."]);
    }

    #[tokio::test(start_paused = true)]
    async fn off_ignores_the_chat() {
        for key in [
            "bloom-ai-whatsapp-selfchat",
            "bloom-ai-whatsapp",
            "bloom-ai-enabled",
        ] {
            let mut settings = on();
            settings.as_object_mut().unwrap().remove(key);
            let (s, fake) = shared(settings);
            let seen = start(&s, false);
            tokio::task::yield_now().await;
            feed_state(&s.whatsapp, mine(ME, "Janice, hi"));
            sleep(secs(5)).await;
            assert!(seen.lock().unwrap().is_empty(), "{key}");
            assert!(sent(&fake).is_empty());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn replies_never_read_as_requests() {
        let (s, fake) = shared(on());
        let seen: Seen = Arc::default();
        let log = seen.clone();
        let runner: Runner = Arc::new(move |task, text| {
            log.lock().unwrap().push((task, text, None));
            Box::pin(async { Ok("Janice, at your service".to_string()) })
        });
        tokio::spawn(run_with(s.clone(), runner));
        tokio::task::yield_now().await;
        feed_state(&s.whatsapp, mine(ME, "Janice, hi"));
        sleep(secs(1)).await;
        assert_eq!(sent(&fake), ["> Janice, at your service"]);
        // Even if WhatsApp hands it back.
        feed_state(&s.whatsapp, mine(ME, "> Janice, at your service"));
        sleep(secs(1)).await;
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn thirty_requests_an_hour() {
        let (s, fake) = shared(on());
        let seen = start(&s, false);
        tokio::task::yield_now().await;
        for i in 0..PER_HOUR + 1 {
            feed_state(&s.whatsapp, mine(ME, &format!("Janice, hi {i}")));
            sleep(secs(1)).await;
        }
        assert_eq!(seen.lock().unwrap().len(), PER_HOUR);
        assert_eq!(
            sent(&fake).last().unwrap(),
            "That's 30 requests in the last hour. Try again later."
        );
        sleep(HOUR).await;
        feed_state(&s.whatsapp, mine(ME, "Janice, again"));
        sleep(secs(1)).await;
        assert_eq!(seen.lock().unwrap().len(), PER_HOUR + 1);
    }

    #[tokio::test(start_paused = true)]
    async fn one_request_at_a_time() {
        let (s, fake) = shared(on());
        let runs = Arc::new(Mutex::new(0));
        let n = runs.clone();
        let runner: Runner = Arc::new(move |_task, _text| {
            *n.lock().unwrap() += 1;
            Box::pin(async {
                sleep(secs(60)).await;
                Ok("Done.".to_string())
            })
        });
        tokio::spawn(run_with(s.clone(), runner));
        tokio::task::yield_now().await;
        feed_state(&s.whatsapp, mine(ME, "Janice, first"));
        sleep(secs(1)).await;
        feed_state(&s.whatsapp, mine(ME, "Janice, second"));
        sleep(secs(1)).await;
        assert_eq!(*runs.lock().unwrap(), 1);
        assert_eq!(sent(&fake), ["Still working on your last request."]);
        sleep(secs(60)).await;
        assert_eq!(sent(&fake)[1], "Done.");
        let log = std::fs::read_to_string(s.data_dir.join("actions.log")).unwrap();
        assert!(log.contains(r#""outcome":"busy""#));
    }

    fn forward(text: &str) -> crate::whatsapp::Message {
        let mut m = mine(ME, text);
        m.forwarded = true;
        m
    }

    #[tokio::test(start_paused = true)]
    async fn forwards_are_never_requests_or_answers() {
        let (s, fake) = shared(on());
        let seen = start(&s, true);
        tokio::task::yield_now().await;
        // A contact's "Janice, ..." forwarded into the own chat.
        feed_state(&s.whatsapp, forward("Janice, text Neha hi"));
        sleep(secs(1)).await;
        assert!(sent(&fake).is_empty() && seen.lock().unwrap().is_empty());
        feed_state(&s.whatsapp, mine(ME, "Janice, text Neha hi"));
        sleep(secs(1)).await;
        assert_eq!(sent(&fake).len(), 1, "the question");
        // A forwarded YES (or anything forwarded) neither approves nor cancels.
        feed_state(&s.whatsapp, forward("YES"));
        feed_state(&s.whatsapp, forward("no"));
        sleep(secs(1)).await;
        assert!(seen.lock().unwrap().is_empty(), "still open");
        feed_state(&s.whatsapp, mine(ME, "YES"));
        sleep(secs(1)).await;
        assert_eq!(seen.lock().unwrap()[0].2, Some(true));
    }

    #[tokio::test(start_paused = true)]
    async fn only_messages_written_after_the_question_answer_it() {
        let (s, _fake) = shared(on());
        let answers: Arc<Mutex<Vec<bool>>> = Arc::default();
        let (log, sh) = (answers.clone(), s.clone());
        // Asks twice.
        let runner: Runner = Arc::new(move |task, _text| {
            let (log, sh) = (log.clone(), sh.clone());
            Box::pin(async move {
                for title in ["Q1", "Q2"] {
                    let yes = sh
                        .bridge
                        .confirm(task, ConfirmKind::Message, title.into(), "b".into())
                        .await;
                    log.lock().unwrap().push(yes);
                }
                Ok("Done.".to_string())
            })
        });
        tokio::spawn(run_with(s.clone(), runner));
        tokio::task::yield_now().await;
        feed_state(&s.whatsapp, mine(ME, "Janice, do two things"));
        sleep(secs(1)).await;
        // Written before the question: no answer.
        let before = chrono::Utc::now().timestamp() - 5;
        feed_state(&s.whatsapp, msg(ME, "me", before, "YES"));
        sleep(secs(1)).await;
        assert!(answers.lock().unwrap().is_empty());
        // The PC answers Q1; a YES typed for Q1 arrives late, after Q2 opened.
        assert!(s.bridge.answer_pending(Answer::Confirm(true)));
        sleep(secs(1)).await;
        feed_state(&s.whatsapp, msg(ME, "me", before, "YES"));
        sleep(secs(1)).await;
        assert_eq!(*answers.lock().unwrap(), [true], "Q2 still open");
        feed_state(&s.whatsapp, mine(ME, "no"));
        sleep(secs(1)).await;
        assert_eq!(*answers.lock().unwrap(), [true, false]);
    }

    #[tokio::test]
    async fn a_question_the_pc_answered_is_not_asked() {
        let (s, fake) = shared(on());
        assert!(ask(&s, 99, "Send?").await.is_none());
        assert!(sent(&fake).is_empty());
    }
}
