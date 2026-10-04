//! Requests that wait for Bloom: "may I send/run this?" and "set the volume".
//! Each gets an id; Bloom's answer (confirm_reply / bloom_result) wakes it.

use crate::protocol::{emit, ConfirmKind, Out};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::sync::oneshot;

pub enum Answer {
    Confirm(bool),
    Bloom { ok: bool, detail: String },
}

#[derive(Default)]
pub struct Bridge {
    next: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
}

impl Bridge {
    fn ask(&self, message: impl FnOnce(u64) -> Out) -> oneshot::Receiver<Answer> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        emit(&message(id));
        rx
    }

    /// True only on an explicit yes. A cancel or a dropped request is a no.
    pub async fn confirm(&self, task: u64, kind: ConfirmKind, title: String, body: String) -> bool {
        matches!(
            self.ask(|id| Out::Confirm {
                task,
                id,
                kind,
                title,
                body
            })
            .await,
            Ok(Answer::Confirm(true))
        )
    }

    pub async fn bloom(&self, action: &str, value: Value) -> Result<String, String> {
        let action = action.to_string();
        match self.ask(|id| Out::Bloom { id, action, value }).await {
            Ok(Answer::Bloom { ok: true, detail }) => Ok(detail),
            Ok(Answer::Bloom { detail, .. }) => Err(detail),
            _ => Err("Bloom did not answer.".into()),
        }
    }

    pub fn answer(&self, id: u64, answer: Answer) {
        if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(answer);
        }
    }

    /// On cancel: every open question resolves as "no".
    pub fn drop_all(&self) {
        self.pending.lock().unwrap().clear();
    }
}

#[cfg(test)]
impl Bridge {
    /// Answers the oldest open question; false if there was none.
    pub fn answer_pending(&self, answer: Answer) -> bool {
        let id = self.pending.lock().unwrap().keys().min().copied();
        id.map(|id| self.answer(id, answer)).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn confirm_waits_for_its_answer() {
        let bridge = Arc::new(Bridge::default());
        let b = bridge.clone();
        let waiting = tokio::spawn(async move {
            b.confirm(7, ConfirmKind::Email, "t".into(), "b".into())
                .await
        });
        tokio::task::yield_now().await; // lets the request register as id 1
        bridge.answer(1, Answer::Confirm(true));
        assert!(waiting.await.unwrap());
    }

    #[tokio::test]
    async fn drop_all_means_no() {
        let bridge = Arc::new(Bridge::default());
        let b = bridge.clone();
        let waiting = tokio::spawn(async move {
            b.confirm(7, ConfirmKind::Script, "t".into(), "b".into())
                .await
        });
        tokio::task::yield_now().await;
        bridge.drop_all();
        assert!(!waiting.await.unwrap());
    }

    #[tokio::test]
    async fn bloom_errors_come_back_as_err() {
        let bridge = Arc::new(Bridge::default());
        let b = bridge.clone();
        let waiting = tokio::spawn(async move { b.bloom("wifi", Value::Bool(true)).await });
        tokio::task::yield_now().await;
        bridge.answer(
            1,
            Answer::Bloom {
                ok: false,
                detail: "no radio".into(),
            },
        );
        assert_eq!(waiting.await.unwrap(), Err("no radio".into()));
    }
}
