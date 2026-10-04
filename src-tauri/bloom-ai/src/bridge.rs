//! Requests that wait for Bloom: "may I send/run this?" and "set the volume".
//! Each gets an id; Bloom's answer (confirm_reply / bloom_result) wakes it.
//! A request from the user's phone also asks in their own WhatsApp chat
//! (selfchat.rs), which answers by the same id.

use crate::protocol::{emit, ConfirmKind, Out};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot};

pub enum Answer {
    Confirm(bool),
    Bloom { ok: bool, detail: String },
}

#[derive(Default)]
pub struct Bridge {
    next: AtomicU64,
    /// Open questions by id, with their request's task.
    pending: Mutex<HashMap<u64, (u64, oneshot::Sender<Answer>)>>,
    /// Confirms of requests from the phone, as (id, question), for selfchat.rs.
    pub phone: Mutex<Option<mpsc::UnboundedSender<(u64, String)>>>,
}

impl Bridge {
    fn ask(&self, task: u64, message: impl FnOnce(u64) -> Out) -> (u64, oneshot::Receiver<Answer>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, (task, tx));
        emit(&message(id));
        (id, rx)
    }

    /// True only on an explicit yes. A cancel or a dropped request is a no.
    pub async fn confirm(&self, task: u64, kind: ConfirmKind, title: String, body: String) -> bool {
        let question = format!("{title}\n{body}");
        // A request from the phone with nobody there to ask: no.
        let phone = match crate::selfchat::is_phone(task) {
            false => None,
            true => match self.phone.lock().unwrap().clone() {
                Some(tx) if !tx.is_closed() => Some(tx),
                _ => return false,
            },
        };
        let (id, rx) = self.ask(task, |id| Out::Confirm {
            task,
            id,
            kind,
            title,
            body,
        });
        if phone.is_some_and(|tx| tx.send((id, question)).is_err()) {
            self.answer(id, Answer::Confirm(false));
        }
        matches!(rx.await, Ok(Answer::Confirm(true)))
    }

    /// The question is still waiting for its answer.
    pub fn is_open(&self, id: u64) -> bool {
        self.pending.lock().unwrap().contains_key(&id)
    }

    pub async fn bloom(&self, action: &str, value: Value) -> Result<String, String> {
        let action = action.to_string();
        // Not a request's question: task 0 is never one from the phone.
        match self.ask(0, |id| Out::Bloom { id, action, value }).1.await {
            Ok(Answer::Bloom { ok: true, detail }) => Ok(detail),
            Ok(Answer::Bloom { detail, .. }) => Err(detail),
            _ => Err("Bloom did not answer.".into()),
        }
    }

    pub fn answer(&self, id: u64, answer: Answer) {
        if let Some((_, tx)) = self.pending.lock().unwrap().remove(&id) {
            let _ = tx.send(answer);
        }
    }

    /// On cancel: every open question resolves as "no", except those of a
    /// request from the phone, which the phone or its 5 minutes settle.
    pub fn drop_all(&self) {
        self.pending
            .lock()
            .unwrap()
            .retain(|_, (task, _)| crate::selfchat::is_phone(*task));
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
    async fn a_phone_question_with_nobody_to_ask_is_no_at_once() {
        let phone = 2_000_000_000;
        let bridge = Bridge::default();
        assert!(
            !bridge
                .confirm(phone, ConfirmKind::Email, "t".into(), "b".into())
                .await
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *bridge.phone.lock().unwrap() = Some(tx);
        drop(rx);
        assert!(
            !bridge
                .confirm(phone, ConfirmKind::Email, "t".into(), "b".into())
                .await
        );
        assert!(bridge.pending.lock().unwrap().is_empty());
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
