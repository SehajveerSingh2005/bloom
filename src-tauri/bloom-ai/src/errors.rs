//! Stable error codes for external integrations. The text the model and the
//! user see is `CODE: actionable message`.

pub const SEARCH_FAILED: &str = "SEARCH_FAILED";
pub const SEARCH_BLOCKED: &str = "SEARCH_BLOCKED";
pub const SEARCH_NOT_CONFIGURED: &str = "SEARCH_NOT_CONFIGURED";
pub const MODEL_TOOL_CALL_FAILED: &str = "MODEL_TOOL_CALL_FAILED";

pub fn coded(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

/// True for text that already starts with an `UPPER_SNAKE` code and a colon.
pub fn is_coded(text: &str) -> bool {
    text.split_once(": ").is_some_and(|(code, _)| {
        code.len() > 3 && code.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
    })
}
