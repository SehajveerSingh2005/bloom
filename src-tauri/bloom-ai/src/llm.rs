//! One call to an OpenAI-compatible chat-completions endpoint. Not streamed:
//! replies are short, and one event per reply keeps the UI idle meanwhile.

use crate::errors::{coded, MODEL_TOOL_CALL_FAILED};
use serde_json::{json, Value};

const TOOL_FAILED: &str = "The model sent a broken tool call twice. Try again, or pick a model with reliable tool calling (for Groq: llama-3.3-70b-versatile or openai/gpt-oss-120b).";

/// Groq answers 400 `tool_use_failed`; other OpenAI-compatible servers word it differently.
fn is_bad_tool_call(error: &Value) -> bool {
    let said = format!("{} {}", error["code"].as_str().unwrap_or_default(), error["message"].as_str().unwrap_or_default()).to_ascii_lowercase();
    said.contains("tool_use_failed") || said.contains("invalid_tool_call") || said.contains("invalid tool call") || said.contains("failed to call a function") || !error["failed_generation"].is_null()
}

/// True for the error `chat` returns when the model twice-over cannot call tools.
pub fn is_tool_call_failure(e: &str) -> bool {
    e.starts_with(MODEL_TOOL_CALL_FAILED)
}

pub struct Llm {
    pub http: reqwest::Client,
    pub base_url: String,
    pub model: String,
    pub key: String,
}

impl Llm {
    /// Returns the assistant message object (`content` and/or `tool_calls`).
    pub async fn chat(&self, messages: &[Value], tools: &Value) -> Result<Value, String> {
        let mut body = json!({ "model": self.model, "messages": messages });
        // Some providers reject an empty tools array.
        if tools.as_array().is_some_and(|t| !t.is_empty()) {
            body["tools"] = tools.clone();
        }
        let res = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Can't reach the model: {e}"))?;
        let status = res.status();
        let reply: Value = res
            .json()
            .await
            .map_err(|e| format!("The model sent something unreadable: {e}"))?;
        if !status.is_success() {
            if status.as_u16() == 400 && is_bad_tool_call(&reply["error"]) {
                return Err(coded(MODEL_TOOL_CALL_FAILED, TOOL_FAILED));
            }
            let detail = reply["error"]["message"]
                .as_str()
                .unwrap_or("request failed");
            return Err(format!("Model error ({status}): {detail}"));
        }
        match &reply["choices"][0]["message"] {
            Value::Object(message) => Ok(Value::Object(message.clone())),
            _ => Err("The model sent no message.".into()),
        }
    }
}
