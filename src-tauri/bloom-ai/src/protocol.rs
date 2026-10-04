//! Messages between Bloom and the agent: one JSON object per line, `type`
//! picks the variant. Bloom writes `In` to our stdin; we write `Out` to stdout.

use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum In {
    Prompt { task: u64, text: String },
    RecordStart,
    RecordStop { task: u64 },
    Cancel,
    ConfirmReply { id: u64, approved: bool },
    BloomResult { id: u64, ok: bool, detail: String },
    SetSecret { name: String, value: String },
    OutlookLogin,
    WakeOn,
    WakeOff,
    EnrollSample { index: u32 },
    EnrollBuild,
}

#[derive(Debug, Serialize, PartialEq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmKind {
    Email,
    Script,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Out {
    Ready,
    Recording {
        on: bool,
    },
    Transcript {
        task: u64,
        text: String,
    },
    Activity {
        task: u64,
        text: String,
    },
    Confirm {
        task: u64,
        id: u64,
        kind: ConfirmKind,
        title: String,
        body: String,
    },
    Reply {
        task: u64,
        text: String,
    },
    Error {
        task: Option<u64>,
        message: String,
    },
    Bloom {
        id: u64,
        action: String,
        value: serde_json::Value,
    },
    SecretSaved {
        name: String,
    },
    LoginCode {
        url: String,
        code: String,
    },
    LoginDone {
        ok: bool,
        message: String,
    },
    /// "Hello Janice" heard; request `task` is being recorded.
    Wake {
        task: u64,
    },
    EnrollSaved {
        index: u32,
    },
    EnrollDone,
}

/// Writes one message to Bloom. A failed write means Bloom is gone; the stdin
/// reader sees EOF right after and the process exits, so errors are ignored.
pub fn emit(out: &Out) {
    let mut line = serde_json::to_string(out).expect("Out always serializes");
    line.push('\n');
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(line.as_bytes());
    let _ = stdout.flush();
}

pub fn parse(line: &str) -> Result<In, String> {
    serde_json::from_str(line).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_prompt() {
        assert_eq!(
            parse(r#"{"type":"prompt","task":3,"text":"hi"}"#),
            Ok(In::Prompt {
                task: 3,
                text: "hi".into()
            })
        );
    }

    #[test]
    fn parses_unit_messages() {
        assert_eq!(parse(r#"{"type":"record_start"}"#), Ok(In::RecordStart));
        assert_eq!(parse(r#"{"type":"cancel"}"#), Ok(In::Cancel));
    }

    #[test]
    fn parses_wake_messages() {
        assert_eq!(parse(r#"{"type":"wake_on"}"#), Ok(In::WakeOn));
        assert_eq!(parse(r#"{"type":"wake_off"}"#), Ok(In::WakeOff));
        assert_eq!(
            parse(r#"{"type":"enroll_sample","index":2}"#),
            Ok(In::EnrollSample { index: 2 })
        );
        assert_eq!(parse(r#"{"type":"enroll_build"}"#), Ok(In::EnrollBuild));
        assert!(parse(r#"{"type":"enroll_sample"}"#).is_err());
    }

    #[test]
    fn wake_events_serialize() {
        let json = |out: Out| serde_json::to_string(&out).unwrap();
        assert_eq!(
            json(Out::Wake {
                task: 1_000_000_001
            }),
            r#"{"type":"wake","task":1000000001}"#
        );
        assert_eq!(
            json(Out::EnrollSaved { index: 3 }),
            r#"{"type":"enroll_saved","index":3}"#
        );
        assert_eq!(json(Out::EnrollDone), r#"{"type":"enroll_done"}"#);
    }

    #[test]
    fn rejects_unknown_types() {
        assert!(parse(r#"{"type":"nope"}"#).is_err());
    }

    #[test]
    fn confirm_serializes_flat() {
        let out = Out::Confirm {
            task: 1,
            id: 2,
            kind: ConfirmKind::Script,
            title: "t".into(),
            body: "b".into(),
        };
        assert_eq!(
            serde_json::to_string(&out).unwrap(),
            r#"{"type":"confirm","task":1,"id":2,"kind":"script","title":"t","body":"b"}"#
        );
    }

    #[test]
    fn task_less_errors_serialize_null() {
        let out = Out::Error {
            task: None,
            message: "x".into(),
        };
        assert_eq!(
            serde_json::to_string(&out).unwrap(),
            r#"{"type":"error","task":null,"message":"x"}"#
        );
    }
}
