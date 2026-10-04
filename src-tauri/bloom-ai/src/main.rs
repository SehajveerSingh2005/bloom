#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! bloom-ai: Bloom's optional AI agent. Bloom starts it on first use and talks
//! to it over stdin/stdout, one JSON object per line (see protocol.rs). It
//! exits when Bloom closes the pipe, so it never outlives Bloom.

mod agent;
mod bridge;
mod config;
mod email;
mod journal;
mod llm;
mod outlook;
mod policy;
mod powershell;
mod protocol;
mod secrets;
mod tools;

#[cfg(test)]
mod testutil;

use agent::Shared;
use bridge::Answer;
use protocol::{emit, In, Out};
use std::path::{Path, PathBuf};
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
    let mut current: Current = None;
    while let Some(line) = rx.recv().await {
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
                cancel(&mut current, &shared);
                let s = shared.clone();
                current = Some((
                    task,
                    tokio::spawn(async move { finish(task, agent::run(task, text, s).await) }),
                ));
            }
            In::Cancel => cancel(&mut current, &shared),
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
            other => emit(&Out::Error {
                task: None,
                message: format!("not supported yet: {other:?}"),
            }),
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
fn cancel(current: &mut Current, shared: &Shared) {
    if let Some((task, handle)) = current.take() {
        if !handle.is_finished() {
            handle.abort();
            emit(&Out::Error {
                task: Some(task),
                message: "Stopped.".into(),
            });
        }
    }
    shared.bridge.drop_all();
}
