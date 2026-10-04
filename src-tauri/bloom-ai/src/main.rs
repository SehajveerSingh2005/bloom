#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! bloom-ai: Bloom's optional AI agent. Bloom starts it on first use and talks
//! to it over stdin/stdout, one JSON object per line (see protocol.rs). It
//! exits when Bloom closes the pipe, so it never outlives Bloom.

mod protocol;

use protocol::{emit, In, Out};

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(serve());
}

async fn serve() {
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
    while let Some(line) = rx.recv().await {
        match protocol::parse(&line) {
            Ok(In::Prompt { task, text }) => emit(&Out::Reply {
                task,
                text: format!("echo: {text}"),
            }),
            Ok(other) => emit(&Out::Error {
                task: None,
                message: format!("not supported yet: {other:?}"),
            }),
            Err(e) => emit(&Out::Error {
                task: None,
                message: format!("bad message: {e}"),
            }),
        }
    }
}
