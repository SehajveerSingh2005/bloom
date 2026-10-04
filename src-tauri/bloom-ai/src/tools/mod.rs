//! The agent's tools: JSON schemas for the model, and dispatch.

pub mod files;

use crate::agent::Ctx;
use serde_json::{json, Value};

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": { "type": "object", "properties": properties, "required": required }
        }
    })
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key].as_str().ok_or_else(|| format!("missing {key}"))
}

pub fn schema() -> Value {
    Value::Array(vec![
        tool(
            "write_file",
            "Create a new text file in the user's Downloads, Documents or Desktop folder. \
             Never overwrites: an existing name gets a (2) suffix.",
            json!({
                "folder": { "type": "string", "enum": ["downloads", "documents", "desktop"] },
                "name": { "type": "string", "description": "File name with extension, e.g. groceries.txt" },
                "content": { "type": "string" }
            }),
            &["folder", "name", "content"],
        ),
        tool(
            "open",
            "Open an installed app by name, a web link (http or https), an ms-settings: page, \
             or an existing file or folder by full path.",
            json!({ "target": { "type": "string" } }),
            &["target"],
        ),
        tool(
            "bloom_control",
            "Control the PC through Bloom: volume or brightness (0-100), media \
             (play_pause, next, previous), wifi or bluetooth (on or off).",
            json!({
                "action": { "type": "string", "enum": ["volume", "brightness", "media", "wifi", "bluetooth"] },
                "value": { "description": "A number for volume and brightness, play_pause/next/previous for media, on/off for wifi and bluetooth" }
            }),
            &["action", "value"],
        ),
        // Task 6 adds run_powershell; Task 7 adds find_contact, save_contact, send_email.
    ])
}

/// One line for the panel while a tool runs.
pub fn describe(name: &str, args: &Value) -> String {
    let arg = |key: &str| args[key].as_str().unwrap_or_default().to_string();
    match name {
        "write_file" => format!("Writing {}", arg("name")),
        "open" => format!("Opening {}", arg("target")),
        "bloom_control" => format!("Changing {}", arg("action")),
        "find_contact" => format!("Looking up {}", arg("name")),
        "save_contact" => format!("Saving {}", arg("name")),
        "send_email" => format!("Emailing {}", arg("to")),
        "run_powershell" => "Running a PowerShell script".into(),
        _ => format!("Working ({name})"),
    }
}

pub async fn call(ctx: &mut Ctx, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "write_file" => {
            let path = files::write_file(
                str_arg(args, "folder")?,
                str_arg(args, "name")?,
                str_arg(args, "content")?,
            )?;
            Ok(format!("Saved {}", path.display()))
        }
        "open" => match files::classify_open(str_arg(args, "target")?)? {
            files::OpenTarget::App(app) => ctx.shared.bridge.bloom("open_app", json!(app)).await,
            files::OpenTarget::Shell(target) => {
                files::shell_open(&target).map(|()| format!("Opened {target}"))
            }
        },
        "bloom_control" => {
            ctx.shared
                .bridge
                .bloom(str_arg(args, "action")?, args["value"].clone())
                .await
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::Answer;
    use crate::testutil::ctx;

    #[test]
    fn every_tool_has_a_name_and_object_parameters() {
        for t in schema().as_array().unwrap() {
            assert!(t["function"]["name"].is_string());
            assert_eq!(t["function"]["parameters"]["type"], "object");
        }
    }

    #[tokio::test]
    async fn bloom_control_goes_to_bloom() {
        let mut ctx = ctx();
        let shared = ctx.shared.clone();
        let args = json!({ "action": "volume", "value": 30 });
        let running = tokio::spawn(async move { call(&mut ctx, "bloom_control", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(
            1,
            Answer::Bloom {
                ok: true,
                detail: "Volume is 30%.".into(),
            },
        );
        assert_eq!(running.await.unwrap(), Ok("Volume is 30%.".into()));
    }
}
