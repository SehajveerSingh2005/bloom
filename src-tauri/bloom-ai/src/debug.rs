//! debug.log: an opt-in record (`bloom-ai-debug` = "true") of what the agent
//! heard and did, for finding out afterwards why something happened: wake
//! scores, requests, tool calls and their results, replies. One JSON line per
//! event in the ai folder (so "Delete AI altogether" removes it). Past
//! ROTATE_BYTES the file becomes debug.old.log and a new one starts, so both
//! together stay around 1 MB. Callers check the setting; with it off nothing
//! is written. Secrets never pass through here: keys and passwords travel
//! only as `set_secret` messages, and a `value` under that name is redacted.

use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const ROTATE_BYTES: u64 = 512 * 1024;
/// Tool results are cut to this many characters.
pub const RESULT_CHARS: usize = 300;
/// Anything else (a long email body, a file's content) is cut to this.
const DETAIL_CHARS: usize = 4000;

pub fn log(dir: &Path, event: &str, detail: &str) {
    let path = dir.join("debug.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() >= ROTATE_BYTES) {
        let _ = std::fs::rename(&path, dir.join("debug.old.log"));
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let line =
        serde_json::json!({ "at_ms": at, "event": event, "detail": cut(detail, DETAIL_CHARS) });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// The first `max` characters of `text`, with "..." if anything was cut.
pub fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text.to_string(),
    }
}

/// A tool call for the log, with any secret value blanked out.
pub fn call(name: &str, args: &Value) -> String {
    let mut args = args.clone();
    if name == "set_secret" {
        if let Some(value) = args.get_mut("value") {
            *value = Value::from("[redacted]");
        }
    }
    format!("{name} {args}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn appends_json_lines_and_rotates() {
        let dir = crate::testutil::temp_dir();
        log(&dir, "request", "hello");
        let first: Value = serde_json::from_str(
            std::fs::read_to_string(dir.join("debug.log"))
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            (first["event"].as_str(), first["detail"].as_str()),
            (Some("request"), Some("hello"))
        );
        // Fill past the cap: the next line starts a new file.
        std::fs::write(dir.join("debug.log"), vec![b'x'; ROTATE_BYTES as usize]).unwrap();
        log(&dir, "reply", "done");
        assert_eq!(
            std::fs::metadata(dir.join("debug.old.log")).unwrap().len(),
            ROTATE_BYTES
        );
        let log = std::fs::read_to_string(dir.join("debug.log")).unwrap();
        assert_eq!(log.lines().count(), 1);
        assert!(log.contains("done"));
    }

    #[test]
    fn cuts_long_text_on_a_char_boundary() {
        assert_eq!(cut("short", 300), "short");
        assert_eq!(cut("ééé", 2), "éé...");
    }

    #[test]
    fn secret_values_are_redacted() {
        let line = call(
            "set_secret",
            &json!({ "name": "llm-key", "value": "sk-123" }),
        );
        assert!(
            !line.contains("sk-123") && line.contains("[redacted]"),
            "{line}"
        );
        let line = call("open", &json!({ "target": "notepad" }));
        assert_eq!(line, r#"open {"target":"notepad"}"#);
    }
}
