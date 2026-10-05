//! One call to an OpenAI-compatible chat-completions endpoint. Not streamed:
//! replies are short, and one event per reply keeps the UI idle meanwhile.

use crate::errors::{coded, MODEL_TOOL_CALL_FAILED};
use serde_json::{json, Value};

const TOOL_FAILED: &str = "The model sent a broken tool call. Try again, or pick a model with reliable tool calling (for Groq: llama-3.3-70b-versatile or openai/gpt-oss-120b).";

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
        self.post(&body).await
    }

    /// One JSON-mode completion without tools, its content parsed as a JSON
    /// object. A server that rejects JSON mode (400) is asked once without it.
    pub async fn json(&self, messages: &[Value]) -> Result<Value, String> {
        let mut body = json!({ "model": self.model, "messages": messages, "response_format": { "type": "json_object" } });
        let message = match self.post(&body).await {
            Err(e) if e.starts_with("Model error (400") => {
                if let Some(b) = body.as_object_mut() {
                    b.remove("response_format");
                }
                self.post(&body).await?
            }
            other => other?,
        };
        let text = message["content"].as_str().unwrap_or_default();
        // Some models still wrap the object in prose or a code fence.
        let (Some(a), Some(b)) = (text.find('{'), text.rfind('}')) else {
            return Err("The model sent no JSON.".into());
        };
        serde_json::from_str(&text[a..=b]).map_err(|_| "The model sent broken JSON.".into())
    }

    async fn post(&self, body: &Value) -> Result<Value, String> {
        let res = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.key)
            .json(body)
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
