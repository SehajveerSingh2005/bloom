//! The agent's tools: JSON schemas for the model, and dispatch.

pub mod files;

use crate::agent::Ctx;
use crate::protocol::ConfirmKind;
use crate::{email, journal, policy, powershell, secrets};
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
        tool(
            "run_powershell",
            "Run a short Windows PowerShell script as the user (never as admin) and get its \
             output. Use only when no other tool fits.",
            json!({
                "script": { "type": "string" },
                "purpose": { "type": "string", "description": "One short sentence shown to the user, e.g. 'List the 5 biggest files in Downloads'" }
            }),
            &["script", "purpose"],
        ),
        tool(
            "find_contact",
            "Find a person's email address by name. Call this before send_email.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        tool(
            "save_contact",
            "Remember a person's email address the user just gave you.",
            json!({ "name": { "type": "string" }, "email": { "type": "string" } }),
            &["name", "email"],
        ),
        tool(
            "send_email",
            "Send a plain-text email from the user's account. `to` must be an email address.",
            json!({
                "to": { "type": "string" },
                "subject": { "type": "string" },
                "body": { "type": "string" }
            }),
            &["to", "subject", "body"],
        ),
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
        "run_powershell" => run_powershell(ctx, args).await,
        "find_contact" => find_contact(ctx, str_arg(args, "name")?).await,
        "save_contact" => save_contact(ctx, args).await,
        "send_email" => send_email(ctx, args).await,
        _ => Err(format!("unknown tool {name}")),
    }
}

async fn run_powershell(ctx: &mut Ctx, args: &Value) -> Result<String, String> {
    let script = str_arg(args, "script")?;
    let purpose = args["purpose"]
        .as_str()
        .unwrap_or("Run this PowerShell script?");
    let ask = policy::script_needs_confirm(ctx.cfg.tier, script, ctx.tainted);
    if ask
        && !ctx
            .shared
            .bridge
            .confirm(
                ctx.task,
                ConfirmKind::Script,
                purpose.to_string(),
                script.to_string(),
            )
            .await
    {
        journal::record(&ctx.shared.data_dir, "script", script, "declined");
        return Ok("The user chose not to run it.".into());
    }
    let output = powershell::run(script).await;
    let outcome = match (&output, ask) {
        (Err(_), _) => "failed",
        (Ok(_), true) => "approved",
        (Ok(_), false) => "auto",
    };
    journal::record(&ctx.shared.data_dir, "script", script, outcome);
    // What the script printed came from outside the conversation.
    ctx.tainted = true;
    output
}

fn list(matches: &[(String, String)]) -> String {
    matches
        .iter()
        .map(|(n, a)| format!("{n} <{a}>"))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn find_contact(ctx: &mut Ctx, name: &str) -> Result<String, String> {
    let matches = email::find(&email::load_contacts(&ctx.shared.data_dir), name);
    if !matches.is_empty() {
        return Ok(list(&matches));
    }
    Ok(format!(
        "No saved contact matches {name}. Ask the user for the address, then call save_contact."
    ))
}

async fn save_contact(ctx: &mut Ctx, args: &Value) -> Result<String, String> {
    let name = str_arg(args, "name")?;
    let address = str_arg(args, "email")?;
    let trimmed_address = address.trim().to_lowercase();
    let contacts = email::load_contacts(&ctx.shared.data_dir);
    if let Some(old_address) = email::contact_needs_confirm(&contacts, name, &trimmed_address) {
        if !ctx
            .shared
            .bridge
            .confirm(
                ctx.task,
                ConfirmKind::Email,
                format!("Change {}'s address to {}?", name, trimmed_address),
                format!(
                    "Saved address: {}\nNew address: {}",
                    old_address, trimmed_address
                ),
            )
            .await
        {
            return Ok("The user kept the saved address.".into());
        }
    }
    email::save_contact(&ctx.shared.data_dir, name, address)?;
    ctx.saved_this_task.insert(trimmed_address);
    Ok(format!("Saved {name} <{}>.", address.trim()))
}

/// The password or token SMTP needs for the sender's account.
async fn mail_secret(_ctx: &Ctx, server: &email::Server) -> Result<String, String> {
    if server.oauth {
        return Err("Sign in with Microsoft in Settings > AI first.".into());
    }
    secrets::get("email-password")
        .ok_or_else(|| "Save your email app password in Settings > AI first.".into())
}

async fn send_email(ctx: &mut Ctx, args: &Value) -> Result<String, String> {
    let to = str_arg(args, "to")?.trim().to_lowercase();
    let subject = str_arg(args, "subject")?.to_string();
    let body = str_arg(args, "body")?.to_string();
    if !email::is_email(&to) {
        return Err(format!(
            "{to} is not an email address. Call find_contact first."
        ));
    }
    let server = email::server_for(&ctx.cfg)?;
    let known = email::is_known(&email::load_contacts(&ctx.shared.data_dir), &to)
        && !ctx.saved_this_task.contains(&to);
    let detail = format!("to {to}: {subject}");
    let ask = policy::email_needs_confirm(ctx.cfg.tier, known, ctx.tainted);
    if ask
        && !ctx
            .shared
            .bridge
            .confirm(
                ctx.task,
                ConfirmKind::Email,
                format!("Send this to {to}?"),
                format!("Subject: {subject}\n\n{body}"),
            )
            .await
    {
        journal::record(&ctx.shared.data_dir, "email", &detail, "declined");
        return Ok("The user chose not to send it.".into());
    }
    let secret = mail_secret(ctx, &server).await?;
    let from = ctx.cfg.email.clone();
    let recipient = to.clone();
    let sent = tokio::task::spawn_blocking(move || {
        email::send(&server, &from, &secret, &recipient, &subject, &body)
    })
    .await
    .map_err(|e| e.to_string())?;
    let outcome = match (&sent, ask) {
        (Err(_), _) => "failed",
        (Ok(()), true) => "approved",
        (Ok(()), false) => "auto",
    };
    journal::record(&ctx.shared.data_dir, "email", &detail, outcome);
    sent.map(|()| format!("Sent to {to}."))
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

    #[tokio::test]
    async fn declined_scripts_do_not_run_and_are_logged() {
        let mut ctx = ctx(); // Conservative by default
        let shared = ctx.shared.clone();
        let args = json!({ "script": "Get-Date", "purpose": "Show the date" });
        let running = tokio::spawn(async move { call(&mut ctx, "run_powershell", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert_eq!(
            running.await.unwrap(),
            Ok("The user chose not to run it.".into())
        );
        let log = std::fs::read_to_string(shared.data_dir.join("actions.log")).unwrap();
        assert!(log.contains("declined"));
    }

    #[tokio::test]
    async fn carte_blanche_runs_without_asking_and_taints() {
        let mut ctx = ctx();
        ctx.cfg.tier = crate::config::Tier::CarteBlanche;
        let out = call(
            &mut ctx,
            "run_powershell",
            &json!({ "script": "Write-Output 42", "purpose": "x" }),
        )
        .await;
        assert_eq!(out.unwrap().trim(), "42");
        assert!(ctx.tainted);
    }

    #[tokio::test]
    async fn declined_email_is_not_sent_and_is_logged() {
        let mut ctx = ctx();
        ctx.cfg.email = "me@gmail.com".into();
        let shared = ctx.shared.clone();
        let args = json!({ "to": "neha@example.com", "subject": "Soccer", "body": "Game at 5?" });
        let running = tokio::spawn(async move { call(&mut ctx, "send_email", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert_eq!(
            running.await.unwrap(),
            Ok("The user chose not to send it.".into())
        );
        let log = std::fs::read_to_string(shared.data_dir.join("actions.log")).unwrap();
        assert!(log.contains("declined") && log.contains("neha@example.com"));
    }

    #[tokio::test]
    async fn contacts_saved_this_request_are_not_known_yet() {
        let mut ctx = ctx();
        ctx.cfg.email = "me@gmail.com".into();
        ctx.cfg.tier = crate::config::Tier::Competent;
        call(
            &mut ctx,
            "save_contact",
            &json!({ "name": "Neha", "email": "neha@example.com" }),
        )
        .await
        .unwrap();
        let shared = ctx.shared.clone();
        let args = json!({ "to": "neha@example.com", "subject": "s", "body": "b" });
        let running = tokio::spawn(async move { call(&mut ctx, "send_email", &args).await });
        tokio::task::yield_now().await;
        // Competent still asks: the contact was saved during this request.
        shared.bridge.answer(1, Answer::Confirm(false));
        assert_eq!(
            running.await.unwrap(),
            Ok("The user chose not to send it.".into())
        );
    }

    #[tokio::test]
    async fn find_contact_points_to_save_contact_when_unknown() {
        let mut ctx = ctx();
        let out = call(&mut ctx, "find_contact", &json!({ "name": "Neha" }))
            .await
            .unwrap();
        assert!(out.contains("save_contact"));
    }

    #[tokio::test]
    async fn declining_address_change_keeps_old_address() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        email::save_contact(&dir, "Alice", "alice@old.com").unwrap();
        let shared = ctx.shared.clone();
        let args = json!({ "name": "Alice", "email": "alice@new.com" });
        let running = tokio::spawn(async move { call(&mut ctx, "save_contact", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert_eq!(
            running.await.unwrap(),
            Ok("The user kept the saved address.".into())
        );
        let contacts = email::load_contacts(&dir);
        assert_eq!(contacts.get("Alice"), Some(&"alice@old.com".to_string()));
    }

    #[tokio::test]
    async fn approving_address_change_updates_it() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        email::save_contact(&dir, "Alice", "alice@old.com").unwrap();
        let shared = ctx.shared.clone();
        let args = json!({ "name": "Alice", "email": "alice@new.com" });
        let running = tokio::spawn(async move { call(&mut ctx, "save_contact", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(true));
        assert!(running.await.unwrap().is_ok());
        let contacts = email::load_contacts(&dir);
        assert_eq!(contacts.get("Alice"), Some(&"alice@new.com".to_string()));
    }

    #[tokio::test]
    async fn saving_new_contact_does_not_ask() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        let args = json!({ "name": "Bob", "email": "bob@example.com" });
        let result = call(&mut ctx, "save_contact", &args).await;
        assert!(result.is_ok());
        let contacts = email::load_contacts(&dir);
        assert_eq!(contacts.get("Bob"), Some(&"bob@example.com".to_string()));
    }

    #[tokio::test]
    async fn address_change_with_different_case_replaces_old_entry() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        email::save_contact(&dir, "Alice", "alice@old.com").unwrap();
        let shared = ctx.shared.clone();
        let args = json!({ "name": "alice", "email": "alice@new.com" });
        let running = tokio::spawn(async move { call(&mut ctx, "save_contact", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(true));
        assert!(running.await.unwrap().is_ok());
        let contacts = email::load_contacts(&dir);
        assert_eq!(contacts.len(), 1, "Should have exactly one entry");
        assert_eq!(
            contacts.get("alice"),
            Some(&"alice@new.com".to_string()),
            "New entry with new casing should be present"
        );
    }
}
