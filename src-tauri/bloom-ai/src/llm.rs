//! One call to an OpenAI-compatible chat-completions endpoint. Not streamed:
//! replies are short, and one event per reply keeps the UI idle meanwhile.

use serde_json::{json, Value};

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
