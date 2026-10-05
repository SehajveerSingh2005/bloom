#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! bloom-ai: Bloom's optional AI agent. Bloom starts it on first use and talks
//! to it over stdin/stdout, one JSON object per line (see protocol.rs). It
//! exits when Bloom closes the pipe, so it never outlives Bloom.

mod agent;
mod autoreply;
mod bridge;
mod config;
mod debug;
mod email;
mod errors;
mod facts;
mod imap_lookup;
mod journal;
mod llm;
mod mail_harvest;
mod mcp;
mod outlook;
mod people;
mod phones;
mod policy;
mod powershell;
mod protocol;
mod secrets;
mod selfchat;
mod skills;
mod tools;
mod voice;
mod web;
mod wa_client;
mod wa_contacts;
mod wake;
mod wake_score;
mod weather;
mod whatsapp;

#[cfg(test)]
mod testutil;

use agent::Shared;
use bridge::Answer;
use protocol::{emit, In, Out};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::task::JoinHandle;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Bloom's "Delete AI altogether" runs `bloom-ai.exe --wipe` before
    // removing the folder, so stored keys and passwords go too.
    if args.iter().any(|a| a == "--wipe") {
        secrets::wipe();
        return;
    }
    // Diagnostics: `--wake-score <wake dir> [wav...]` prints how well
    // recordings match the trained wake word. Reads only.
    if let Some(i) = args.iter().position(|a| a == "--wake-score") {
        let dir = PathBuf::from(args.get(i + 1).map_or("", String::as_str));
        let wavs: Vec<PathBuf> = args.iter().skip(i + 2).map(PathBuf::from).collect();
        match wake_score::report(&dir, &wavs) {
            Ok(text) => print!("{text}"),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let settings_path = args
        .iter()
        .position(|a| a == "--settings")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_default();
    let data_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(serve(Arc::new(Shared::new(data_dir, settings_path))));
}

/// The running request, if any. One at a time: a new one cancels the old.
type Current = Option<(u64, JoinHandle<()>)>;

async fn serve(shared: Arc<Shared>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    // Blocking stdin read on its own thread. EOF (Bloom quit, crashed or killed
    // us) drops `tx`, which ends the loop below and the process with it.
    std::thread::spawn(move || {
        for line in std::io::stdin().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    emit(&Out::Ready);
    // Waits on incoming WhatsApp messages; idle while none arrive.
    tokio::spawn(autoreply::run(shared.clone()));
    tokio::spawn(selfchat::run(shared.clone()));
    let mut current: Current = None;
    let mut recorder: Option<voice::Recorder> = None;
    // Requests recording or running; the wake word is ignored while any are.
    let busy = Arc::new(AtomicUsize::new(0));
    let (wake_tx, mut wake_rx) = tokio::sync::mpsc::unbounded_channel::<wake::Event>();
    let mut wake = WakeState {
        listener: None,
        task: None,
        dir: shared.data_dir.clone(),
        settings: shared.settings_path.clone(),
        busy: busy.clone(),
        events: wake_tx,
    };
    let mut next_wake = wake::FIRST_TASK;
    loop {
        let line = tokio::select! {
            line = rx.recv() => match line {
                Some(line) => line,
                None => break,
            },
            Some(event) = wake_rx.recv() => {
                match event {
                    wake::Event::Wake => {
                        next_wake += 1;
                        wake.task = Some(next_wake);
                        emit(&Out::Wake { task: next_wake });
                        emit(&Out::Recording { on: true });
                    }
                    wake::Event::Clip { audio, busy } => {
                        // None: stopped while recording.
                        let Some(task) = wake.task.take() else { continue };
                        emit(&Out::Recording { on: false });
                        match audio {
                            Ok((samples, rate)) => {
                                cancel(&mut current, &shared, false);
                                let s = shared.clone();
                                current = Some((
                                    task,
                                    tokio::spawn(async move {
                                        let _busy = busy;
                                        let heard = voice::to_text(&samples, rate, &s).await;
                                        run_voice(task, heard, s).await
                                    }),
                                ));
                            }
                            Err(message) => emit(&Out::Error {
                                task: Some(task),
                                message,
                            }),
                        }
                    }
                }
                continue;
            }
        };
        let message = match protocol::parse(&line) {
            Ok(message) => message,
            Err(e) => {
                emit(&Out::Error {
                    task: None,
                    message: format!("bad message: {e}"),
                });
                continue;
            }
        };
        match message {
            In::Prompt { task, text } => {
                // Replaces a wake request still recording, silently like any other.
                wake.abort_request(false);
                cancel(&mut current, &shared, false);
                let s = shared.clone();
                let guard = wake::Busy::new(&busy);
                current = Some((
                    task,
                    tokio::spawn(async move {
                        let _busy = guard;
                        finish(task, agent::run(task, text, s).await)
                    }),
                ));
            }
            In::Cancel => {
                wake.abort_request(true);
                cancel(&mut current, &shared, true)
            }
            In::ConfirmReply { id, approved } => {
                shared.bridge.answer(id, Answer::Confirm(approved))
            }
            In::BloomResult { id, ok, detail } => {
                shared.bridge.answer(id, Answer::Bloom { ok, detail })
            }
            In::SetSecret { name, value } => match secrets::set(&name, &value) {
                Ok(()) => emit(&Out::SecretSaved { name }),
                Err(message) => emit(&Out::Error {
                    task: None,
                    message,
                }),
            },
            In::SecretStatus => {
                let [llm_key, stt_key, email_password, outlook, search_key] =
                    tokio::task::spawn_blocking(secrets::status)
                        .await
                        .unwrap_or([false; 5]);
                emit(&Out::SecretStatus {
                    llm_key,
                    stt_key,
                    email_password,
                    outlook,
                    search_key,
                });
            }
            In::SearchTest => {
                let s = shared.clone();
                tokio::spawn(async move {
                    let (ok, message) = match web::probe(&s).await {
                        Ok(m) => (true, m),
                        Err(e) => (false, e),
                    };
                    emit(&Out::SearchTest { ok, message });
                });
            }
            In::LibraryStatus => emit_library_status(&shared),
            In::ForgetAll => {
                if let Err(message) = facts::clear(&shared.data_dir) {
                    emit(&Out::Error {
                        task: None,
                        message,
                    });
                }
                // Refreshes the Settings count.
                emit_library_status(&shared);
            }
            In::Reveal { what } => {
                let opened = match what.as_str() {
                    "memory" => facts::ensure(&shared.data_dir)
                        .and_then(|p| tools::files::shell_open(&p.to_string_lossy())),
                    "skills" => skills::ensure(&shared.data_dir)
                        .and_then(|p| tools::files::shell_open(&p.to_string_lossy())),
                    "mcp" => mcp::ensure(&shared.data_dir)
                        .and_then(|p| tools::files::shell_open(&p.to_string_lossy())),
                    other => Err(format!("Can't open {other}.")),
                };
                if let Err(message) = opened {
                    emit(&Out::Error {
                        task: None,
                        message,
                    });
                }
            }
            In::McpReload => {
                let s = shared.clone();
                // Its own task: starting takes up to 15 s and must not block requests.
                tokio::spawn(async move {
                    s.mcp.reload(&s.data_dir).await;
                    emit_library_status(&s);
                });
            }
            In::OutlookLogin => {
                let s = shared.clone();
                // Its own task: polling waits up to 15 minutes and must not
                // block requests or be cancelled by them.
                tokio::spawn(async move {
                    let (ok, message) = match outlook::login(&s.http).await {
                        Ok(()) => (true, "Signed in.".to_string()),
                        Err(e) => (false, e),
                    };
                    emit(&Out::LoginDone { ok, message });
                });
            }
            In::TestEmail => {
                let s = shared.clone();
                // Its own task: an unreachable server takes the connect timeout.
                tokio::spawn(async move {
                    let (ok, message) = match tools::test_email(&s).await {
                        Ok(m) => (true, m),
                        Err(e) => (false, e),
                    };
                    emit(&Out::EmailTest { ok, message });
                    // A working login: scan the mail headers for contacts.
                    if ok {
                        emit(&tools::contacts(&s, "harvest", "", "", "").await);
                    }
                });
            }
            In::RecordStart => {
                wake.abort_request(false);
                // A key-up that never arrived leaves an old recorder: drop its clip.
                if let Some(old) = recorder.take() {
                    drop(tokio::task::spawn_blocking(move || old.finish()));
                }
                // The recorder releases its guard when its capture ends, even
                // if the key-up never arrives.
                recorder = Some(voice::start(wake::Busy::new(&busy)));
                emit(&Out::Recording { on: true });
            }
            In::RecordStop { task } => {
                let Some(rec) = recorder.take() else { continue };
                emit(&Out::Recording { on: false });
                cancel(&mut current, &shared, false);
                let s = shared.clone();
                let guard = wake::Busy::new(&busy);
                current = Some((
                    task,
                    tokio::spawn(async move {
                        let _busy = guard;
                        run_voice(task, voice::listen(rec, &s).await, s).await
                    }),
                ));
            }
            In::WakeOn => {
                wake.end_request(true);
                wake.start();
            }
            In::WakeOff => {
                wake.listener = None;
                wake.end_request(true);
            }
            In::EnrollSample { index } => {
                let dir = shared.data_dir.clone();
                // Saying the wake phrase for a sample must not wake her.
                let guard = wake::Busy::new(&busy);
                tokio::spawn(async move {
                    let saved =
                        tokio::task::spawn_blocking(move || wake::enroll_sample(&dir, index)).await;
                    drop(guard);
                    match saved.map_err(|e| e.to_string()).and_then(|r| r) {
                        Ok(_) => emit(&Out::EnrollSaved { index }),
                        Err(message) => emit(&Out::Error {
                            task: None,
                            message,
                        }),
                    }
                });
            }
            In::Contacts {
                action,
                id,
                value,
                label,
            } => {
                let s = shared.clone();
                // Its own task: a mail scan takes a while.
                tokio::spawn(async move {
                    emit(&tools::contacts(&s, &action, &id, &value, &label).await);
                });
            }
            In::WhatsappOn => whatsapp::on(&shared),
            In::WhatsappOff => shared.whatsapp.off(),
            In::WhatsappPairCode { phone } => whatsapp::pair_code(&shared, &phone),
            In::WhatsappUnlink => {
                let s = shared.clone();
                tokio::spawn(async move { whatsapp::unlink(&s).await });
            }
            In::EnrollBuild => match wake::build(
                &shared.data_dir,
                &config::Config::load(&shared.settings_path).name,
            ) {
                Ok(()) => {
                    // A running listener switches to the new voice.
                    if wake.listener.is_some() {
                        wake.end_request(true);
                        wake.start();
                    }
                    emit(&Out::EnrollDone);
                }
                Err(message) => emit(&Out::Error {
                    task: None,
                    message,
                }),
            },
        }
    }
}

/// Counts for Settings > Library. MCP counts are the running servers (none
/// before the first request or Reload).
fn emit_library_status(shared: &Shared) {
    let (mcp_servers, mcp_tools, mcp_errors) = shared.mcp.status();
    emit(&Out::LibraryStatus {
        memory: facts::count(&shared.data_dir),
        skills: skills::count(&shared.data_dir),
        mcp_servers,
        mcp_tools,
        mcp_errors,
    });
}

/// A spoken request: show what was heard, then run it.
async fn run_voice(task: u64, heard: Result<String, String>, shared: Arc<Shared>) {
    match heard {
        Ok(text) => {
            emit(&Out::Transcript {
                task,
                text: text.clone(),
            });
            finish(task, agent::run(task, text, shared).await);
        }
        Err(message) => emit(&Out::Error {
            task: Some(task),
            message,
        }),
    }
}

/// The wake word listener and the wake request it may be recording.
struct WakeState {
    listener: Option<wake::Listener>,
    task: Option<u64>,
    dir: PathBuf,
    settings: PathBuf,
    busy: Arc<AtomicUsize>,
    events: tokio::sync::mpsc::UnboundedSender<wake::Event>,
}

impl WakeState {
    /// (Re)starts listening; a recording in progress is dropped with the old
    /// listener.
    fn start(&mut self) {
        self.listener = None;
        let name = config::Config::load(&self.settings).name;
        match wake::Listener::start(
            &self.dir,
            &self.settings,
            &name,
            self.busy.clone(),
            self.events.clone(),
        ) {
            Ok(started) => self.listener = Some(started),
            Err(message) => emit(&Out::Error {
                task: None,
                message,
            }),
        }
    }

    /// Ends the panel's view of a wake request still recording ("Stopped."
    /// if `announce`); a clip that arrives later is dropped. Returns whether
    /// there was one.
    fn end_request(&mut self, announce: bool) -> bool {
        let Some(task) = self.task.take() else {
            return false;
        };
        emit(&Out::Recording { on: false });
        if announce {
            emit(&Out::Error {
                task: Some(task),
                message: "Stopped.".into(),
            });
        }
        true
    }

    /// Ends a wake request still recording and frees the microphone at once
    /// by restarting the listener, so the wake phrase works again right away.
    fn abort_request(&mut self, announce: bool) {
        if self.end_request(announce) && self.listener.is_some() {
            self.start();
        }
    }
}

fn finish(task: u64, result: Result<String, String>) {
    match result {
        Ok(text) => emit(&Out::Reply { task, text }),
        Err(message) => emit(&Out::Error {
            task: Some(task),
            message,
        }),
    }
}

/// Aborting the task drops its futures: an HTTP request is abandoned and a
/// running PowerShell is killed (kill_on_drop). Open confirms resolve as "no".
fn cancel(current: &mut Current, shared: &Shared, announce: bool) {
    if let Some((task, handle)) = current.take() {
        if !handle.is_finished() {
            handle.abort();
            // A replaced request stays silent: its "Stopped." would hide the new one.
            if announce {
                emit(&Out::Error {
                    task: Some(task),
                    message: "Stopped.".into(),
                });
            }
        }
    }
    shared.bridge.drop_all();
    // After the abort, so a finishing request can't remember itself again.
    if announce {
        shared.memory.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_forgets_the_conversation() {
        let shared = Shared::new(PathBuf::new(), PathBuf::new());
        shared
            .memory
            .lock()
            .unwrap()
            .remember(std::time::Instant::now(), "q", "a", true);
        cancel(&mut None, &shared, false);
        assert!(!shared
            .memory
            .lock()
            .unwrap()
            .messages(std::time::Instant::now())
            .is_empty());
        cancel(&mut None, &shared, true);
        let mut memory = shared.memory.lock().unwrap();
        assert!(memory.messages(std::time::Instant::now()).is_empty());
        assert!(!memory.tainted);
    }
}
