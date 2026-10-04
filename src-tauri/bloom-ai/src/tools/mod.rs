//! The agent's tools: JSON schemas for the model, and dispatch. Tools are
//! added by Tasks 4, 6 and 7.

use crate::agent::Ctx;
use serde_json::{json, Value};

pub fn schema() -> Value {
    json!([])
}

/// One line for the panel while a tool runs.
pub fn describe(name: &str, _args: &Value) -> String {
    format!("Working ({name})")
}

pub async fn call(_ctx: &mut Ctx, name: &str, _args: &Value) -> Result<String, String> {
    Err(format!("unknown tool {name}"))
}
