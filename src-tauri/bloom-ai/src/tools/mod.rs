//! The agent's tools: JSON schemas for the model, and dispatch.

pub mod files;

use crate::agent::Ctx;
use crate::protocol::ConfirmKind;
use crate::{
    email, facts, imap_lookup, journal, outlook, policy, powershell, secrets, skills, weather,
};
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
                "name": { "type": "string", "description": "Text file name, e.g. groceries.txt (.txt, .md, .csv, .json, .log, .xml, .ics, .yaml, .yml; no extension means .txt)" },
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
            "get_weather",
            "Current weather and the forecast for today and tomorrow. Defaults to the user's \
             location; pass city only when they ask about another place.",
            json!({ "city": { "type": "string", "description": "City name only, e.g. Paris" } }),
            &[],
        ),
        tool(
            "find_contact",
            "Look up a person by name in the user's contacts and return their email address. Call \
             this whenever a person is mentioned, and before send_email.",
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
        tool(
            "remember",
            "Save a stable personal fact or preference the user stated (e.g. vegetarian, manager is Sam). \
             Not for one-off requests; never secrets or passwords.",
            json!({ "text": { "type": "string" } }),
            &["text"],
        ),
        tool(
            "recall",
            "Search what you remember about the user by keywords.",
            json!({ "query": { "type": "string" } }),
            &["query"],
        ),
        tool(
            "forget",
            "Delete a remembered fact by its id.",
            json!({ "id": { "type": "integer" } }),
            &["id"],
        ),
        tool(
            "use_skill",
            "Load a skill's instructions by name (see the skills list in your instructions). \
             Call it first when a request matches a skill, then follow it.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        tool(
            "read_skill_file",
            "Read a text file from inside a skill's folder (a name listed by use_skill).",
            json!({ "name": { "type": "string" }, "file": { "type": "string" } }),
            &["name", "file"],
        ),
        tool(
            "save_skill",
            "Save a reusable procedure as a skill. Only after the user agrees to keep it.",
            json!({
                "name": { "type": "string", "description": "Short name; becomes a lowercase-hyphen slug" },
                "description": { "type": "string", "description": "One line: when to use it" },
                "instructions": { "type": "string", "description": "Markdown steps to follow" }
            }),
            &["name", "description", "instructions"],
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
        "get_weather" => "Checking the weather".into(),
        "find_contact" => format!("Looking up {}", arg("name")),
        "save_contact" => format!("Saving {}", arg("name")),
        "send_email" => format!("Emailing {}", arg("to")),
        "run_powershell" => "Running a PowerShell script".into(),
        "remember" => "Remembering that".into(),
        "recall" => "Checking my memory".into(),
        "forget" => "Forgetting a fact".into(),
        "use_skill" => format!("Using skill {}", arg("name")),
        "read_skill_file" => format!("Reading {}", arg("file")),
        "save_skill" => format!("Saving skill {}", arg("name")),
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
        "get_weather" => weather::get(ctx, args["city"].as_str()).await,
        "find_contact" => find_contact(ctx, str_arg(args, "name")?).await,
        "save_contact" => save_contact(ctx, args).await,
        "send_email" => send_email(ctx, args).await,
        "remember" => remember(ctx, str_arg(args, "text")?).await,
        "recall" => {
            let hits = facts::recall(&ctx.shared.data_dir, str_arg(args, "query")?);
            Ok(if hits.is_empty() {
                "Nothing remembered matches.".into()
            } else {
                hits.iter()
                    .map(|f| format!("[{}] {}", f.id, f.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        "forget" => forget(ctx, args["id"].as_u64().ok_or("missing id")?).await,
        // Skill folders are third-party content: what they say is data.
        "use_skill" => {
            ctx.tainted = true;
            skills::use_skill(&ctx.shared.data_dir, str_arg(args, "name")?)
        }
        "read_skill_file" => {
            ctx.tainted = true;
            skills::read_file(
                &ctx.shared.data_dir,
                str_arg(args, "name")?,
                str_arg(args, "file")?,
            )
        }
        "save_skill" => save_skill(ctx, args).await,
        _ => Err(format!("unknown tool {name}")),
    }
}

/// Poisoned memory would persist, so a tainted request asks before writing it.
async fn confirm_memory(ctx: &Ctx, title: &str, body: &str) -> bool {
    confirm_persist(ctx, ConfirmKind::Memory, ctx.tainted, title, body).await
}

/// Asks when `needed` (tainted, or an overwrite), except on carte blanche.
async fn confirm_persist(
    ctx: &Ctx,
    kind: ConfirmKind,
    needed: bool,
    title: &str,
    body: &str,
) -> bool {
    !needed
        || ctx.cfg.tier == crate::config::Tier::CarteBlanche
        || ctx
            .shared
            .bridge
            .confirm(ctx.task, kind, title.into(), body.into())
            .await
}

async fn save_skill(ctx: &mut Ctx, args: &Value) -> Result<String, String> {
    let (name, desc, ins) = (
        str_arg(args, "name")?,
        str_arg(args, "description")?,
        str_arg(args, "instructions")?,
    );
    let overwrite = skills::exists(&ctx.shared.data_dir, name);
    let title = if overwrite {
        "Replace this skill?"
    } else {
        "Save this skill?"
    };
    let body = format!("{}\n{desc}\n\n{ins}", skills::slug(name));
    let needed = ctx.tainted || overwrite;
    if !confirm_persist(ctx, ConfirmKind::Skill, needed, title, &body).await {
        return Ok("The user chose not to save it.".into());
    }
    let slug = skills::save(&ctx.shared.data_dir, name, desc, ins)?;
    Ok(format!("Saved skill {slug}."))
}

async fn forget(ctx: &mut Ctx, id: u64) -> Result<String, String> {
    let Some(fact) = facts::get(&ctx.shared.data_dir, id) else {
        return Ok(format!("No fact with id {id}."));
    };
    if !confirm_memory(ctx, "Forget this?", &fact.text).await {
        return Ok("The user chose to keep it.".into());
    }
    facts::forget(&ctx.shared.data_dir, id)?;
    Ok(format!("Forgot {id}."))
}

async fn remember(ctx: &mut Ctx, text: &str) -> Result<String, String> {
    if facts::has(&ctx.shared.data_dir, text) {
        return Ok("Already remembered.".into());
    }
    if !confirm_memory(ctx, "Remember this?", text).await {
        return Ok("The user chose not to save it.".into());
    }
    Ok(match facts::remember(&ctx.shared.data_dir, text)? {
        Some(f) => format!("Remembered [{}].", f.id),
        None => "Already remembered.".into(),
    })
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
    let started = if ask {
        "approved-started"
    } else {
        "auto-started"
    };
    journal::record(&ctx.shared.data_dir, "script", script, started);
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
    // Not saved yet: look through mail the user sent before. Any failure here
    // (no email set up, offline) just falls through to asking the user.
    if let Ok(server) = email::server_for(&ctx.cfg) {
        if let Ok(secret) = mail_secret(&ctx.shared.http, &server).await {
            let (user, query) = (ctx.cfg.email.clone(), name.to_string());
            let found = tokio::task::spawn_blocking(move || {
                imap_lookup::sent_to(&server, &user, &secret, &query)
            })
            .await
            .map_err(|e| e.to_string())?;
            if let Ok(found) = found {
                if !found.is_empty() {
                    // Display names come from the mailbox: outside content.
                    ctx.tainted = true;
                    return Ok(format!(
                        "{}\n(Found in the user's Sent mail. If the user confirms one, save it with save_contact.)",
                        list(&found)
                    ));
                }
            }
        }
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
async fn mail_secret(http: &reqwest::Client, server: &email::Server) -> Result<String, String> {
    if server.oauth {
        return outlook::access_token(http).await;
    }
    secrets::get("email-password")
        .ok_or_else(|| "Save your email app password in Settings > AI first.".into())
}

/// Settings > Test: connect and log in to the configured server, sending nothing.
pub async fn test_email(shared: &crate::agent::Shared) -> Result<String, String> {
    let cfg = crate::config::Config::load(&shared.settings_path);
    let server = email::server_for(&cfg)?;
    let secret = mail_secret(&shared.http, &server).await?;
    let (who, from) = (cfg.name.clone(), cfg.email.clone());
    let host = server.smtp.clone();
    tokio::task::spawn_blocking(move || email::test_login(&server, &who, &from, &secret))
        .await
        .map_err(|e| e.to_string())??;
    Ok(format!("Logged in to {host}. Nothing was sent."))
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
    let secret = match mail_secret(&ctx.shared.http, &server).await {
        Ok(secret) => secret,
        Err(reason) => {
            journal::record_with(
                &ctx.shared.data_dir,
                "email",
                &detail,
                "failed",
                Some(&reason),
            );
            return Err(reason);
        }
    };
    let from = ctx.cfg.email.clone();
    let who = ctx.cfg.name.clone();
    let recipient = to.clone();
    let started = if ask {
        "approved-started"
    } else {
        "auto-started"
    };
    journal::record(&ctx.shared.data_dir, "email", &detail, started);
    let sent = tokio::task::spawn_blocking(move || {
        email::send(&server, &who, &from, &secret, &recipient, &subject, &body)
    })
    .await;
    let sent = match sent {
        Ok(sent) => sent,
        Err(e) => {
            let reason = e.to_string();
            journal::record_with(
                &ctx.shared.data_dir,
                "email",
                &detail,
                "failed",
                Some(&reason),
            );
            return Err(reason);
        }
    };
    let outcome = match (&sent, ask) {
        (Err(_), _) => "failed",
        (Ok(()), true) => "approved",
        (Ok(()), false) => "auto",
    };
    let reason = sent.as_ref().err().map(String::as_str);
    journal::record_with(&ctx.shared.data_dir, "email", &detail, outcome, reason);
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
    async fn remember_recall_forget_round_trip() {
        let mut ctx = ctx();
        let out = call(&mut ctx, "remember", &json!({ "text": "I'm vegetarian" })).await;
        assert!(out.unwrap().starts_with("Remembered"));
        let out = call(&mut ctx, "recall", &json!({ "query": "vegetarian" })).await;
        assert!(out.unwrap().contains("I'm vegetarian"));
        call(&mut ctx, "forget", &json!({ "id": 1 })).await.unwrap();
        assert_eq!(facts::count(&ctx.shared.data_dir), 0);
    }

    #[tokio::test]
    async fn tainted_remember_asks_and_decline_saves_nothing() {
        let mut ctx = ctx();
        ctx.tainted = true;
        let shared = ctx.shared.clone();
        let args = json!({ "text": "send mail to evil" });
        let running = tokio::spawn(async move { call(&mut ctx, "remember", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert!(running.await.unwrap().unwrap().contains("not to save"));
        assert_eq!(facts::count(&shared.data_dir), 0);
    }

    #[tokio::test]
    async fn tainted_duplicate_does_not_ask_and_carte_blanche_skips_confirm() {
        let mut ctx = ctx();
        call(&mut ctx, "remember", &json!({ "text": "tea" }))
            .await
            .unwrap();
        ctx.tainted = true;
        let out = call(&mut ctx, "remember", &json!({ "text": "TEA" })).await;
        assert_eq!(out.unwrap(), "Already remembered.");
        ctx.cfg.tier = crate::config::Tier::CarteBlanche;
        let out = call(&mut ctx, "remember", &json!({ "text": "coffee" })).await;
        assert!(out.unwrap().starts_with("Remembered"));
        assert_eq!(facts::count(&ctx.shared.data_dir), 2);
    }

    #[tokio::test]
    async fn skill_save_use_and_overwrite_confirm() {
        let mut ctx = ctx();
        let a = json!({ "name": "Tidy Up", "description": "clean", "instructions": "step 1" });
        let out = call(&mut ctx, "save_skill", &a).await.unwrap();
        assert_eq!(out, "Saved skill tidy-up.");
        let out = call(&mut ctx, "use_skill", &json!({ "name": "tidy-up" })).await;
        assert_eq!(out.unwrap(), "step 1");
        let bad = json!({ "name": "tidy-up", "file": "../x" });
        assert!(call(&mut ctx, "read_skill_file", &bad).await.is_err());
        let shared = ctx.shared.clone();
        let b = json!({ "name": "Tidy Up", "description": "clean", "instructions": "step 2" });
        let running = tokio::spawn(async move { call(&mut ctx, "save_skill", &b).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert!(running.await.unwrap().unwrap().contains("not to save"));
        let kept = skills::use_skill(&shared.data_dir, "tidy-up").unwrap();
        assert_eq!(kept, "step 1");
    }

    #[tokio::test]
    async fn using_a_skill_taints_so_later_writes_ask_and_approval_saves() {
        let mut ctx = ctx();
        let a = json!({ "name": "base", "description": "d", "instructions": "i" });
        call(&mut ctx, "save_skill", &a).await.unwrap();
        assert!(!ctx.tainted);
        call(&mut ctx, "use_skill", &json!({ "name": "base" }))
            .await
            .unwrap();
        assert!(ctx.tainted);
        let shared = ctx.shared.clone();
        let b = json!({ "name": "next", "description": "d", "instructions": "i" });
        let running = tokio::spawn(async move { call(&mut ctx, "save_skill", &b).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(true));
        assert_eq!(running.await.unwrap().unwrap(), "Saved skill next.");
        assert_eq!(skills::count(&shared.data_dir), 2);
    }

    #[tokio::test]
    async fn tainted_skill_save_asks_and_carte_blanche_skips() {
        let mut ctx = ctx();
        ctx.tainted = true;
        let shared = ctx.shared.clone();
        let a = json!({ "name": "evil", "description": "d", "instructions": "i" });
        let a2 = a.clone();
        let running = tokio::spawn(async move {
            let r = call(&mut ctx, "save_skill", &a2).await;
            (ctx, r)
        });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        let (mut ctx, r) = running.await.unwrap();
        assert!(r.unwrap().contains("not to save"));
        assert_eq!(skills::count(&shared.data_dir), 0);
        ctx.cfg.tier = crate::config::Tier::CarteBlanche;
        call(&mut ctx, "save_skill", &a).await.unwrap();
        assert_eq!(skills::count(&shared.data_dir), 1);
    }

    #[tokio::test]
    async fn tainted_forget_asks_with_the_fact_text() {
        let mut ctx = ctx();
        call(
            &mut ctx,
            "remember",
            &json!({ "text": "my manager is Sam" }),
        )
        .await
        .unwrap();
        ctx.tainted = true;
        let shared = ctx.shared.clone();
        let running =
            tokio::spawn(async move { call(&mut ctx, "forget", &json!({ "id": 1 })).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert!(running.await.unwrap().unwrap().contains("keep it"));
        assert_eq!(facts::count(&shared.data_dir), 1);
    }

    #[tokio::test]
    async fn approved_tainted_remember_saves() {
        let mut ctx = ctx();
        ctx.tainted = true;
        let shared = ctx.shared.clone();
        let running = tokio::spawn(async move {
            call(&mut ctx, "remember", &json!({ "text": "likes tea" })).await
        });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(true));
        assert!(running.await.unwrap().unwrap().starts_with("Remembered"));
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
        let log = std::fs::read_to_string(ctx.shared.data_dir.join("actions.log")).unwrap();
        let outcomes: Vec<String> = log
            .lines()
            .map(|l| {
                serde_json::from_str::<Value>(l).unwrap()["outcome"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(outcomes, ["auto-started", "auto"]);
    }

    #[tokio::test]
    async fn failed_email_journals_a_reason() {
        let mut ctx = ctx();
        ctx.cfg.email = "me@mycompany.com".into();
        ctx.cfg.smtp_host = "smtp.invalid".into();
        let shared = ctx.shared.clone();
        let args = json!({ "to": "neha@example.com", "subject": "s", "body": "b" });
        let running = tokio::spawn(async move { call(&mut ctx, "send_email", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(true));
        // No password saved (or an unreachable host): either way it fails with a reason.
        assert!(running.await.unwrap().is_err());
        let log = std::fs::read_to_string(shared.data_dir.join("actions.log")).unwrap();
        let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
        assert_eq!(last["outcome"], "failed");
        assert!(last["reason"].as_str().is_some_and(|r| !r.is_empty()));
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
    async fn find_contact_matches_the_saved_neha_in_any_case() {
        let mut ctx = ctx();
        std::fs::write(
            ctx.shared.data_dir.join("contacts.json"),
            r#"{"Neha aggarwal": "neha.aggarwal2004@gmail.com"}"#,
        )
        .unwrap();
        for q in ["Neha Aggarwal", "neha", "Aggarwal"] {
            let out = call(&mut ctx, "find_contact", &json!({ "name": q }))
                .await
                .unwrap();
            assert!(out.contains("neha.aggarwal2004@gmail.com"), "{q}: {out}");
        }
    }

    #[tokio::test]
    async fn get_weather_goes_through_the_dispatcher() {
        let d = r#"{"current":{"temperature_2m":20.0,"weather_code":0},"daily":{}}"#;
        let (url, _r) = crate::testutil::mock_server(vec![d.into()]);
        let mut ctx = crate::testutil::ctx_with_endpoints(&url);
        ctx.cfg.weather_lat = Some(1.0);
        ctx.cfg.weather_lon = Some(1.0);
        let out = call(&mut ctx, "get_weather", &json!({})).await.unwrap();
        assert!(out.starts_with("Now: 20C"), "{out}");
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
