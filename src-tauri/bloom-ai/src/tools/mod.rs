//! The agent's tools: JSON schemas for the model, and dispatch.

pub mod files;

use crate::agent::Ctx;
use crate::errors::{coded, CONTACT_AMBIGUOUS, CONTACT_NOT_FOUND};
use crate::people::{self, Found, Person};
use crate::protocol::ConfirmKind;
use crate::{
    email, facts, imap_lookup, journal, mail_harvest, outlook, phones, policy, powershell, secrets,
    skills, weather, web, whatsapp,
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
            "Look up a person in the user's contacts and return all their email addresses, phone \
             numbers and tags. A first name, last name, nickname, misspelling, email or number \
             works. Call this whenever a person is mentioned, and before send_email.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        tool(
            "save_contact",
            "Save a person's email address and/or phone number the user just gave you. Adds to \
             what is saved; changing a saved address asks the user. A person with more than one \
             email needs at least one tag (like work or personal): pass tags to keep both.",
            json!({
                "name": { "type": "string" },
                "email": { "type": "string" },
                "phone": { "type": "string" },
                "label": { "type": "string", "description": "What this address or number is, e.g. work, personal, home" },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Single lowercase words, e.g. work, family" }
            }),
            &["name"],
        ),
        tool(
            "tag_contact",
            "Add or remove tags (single lowercase words like work, family, gym) on a saved contact.",
            json!({
                "name": { "type": "string" },
                "add": { "type": "array", "items": { "type": "string" } },
                "remove": { "type": "array", "items": { "type": "string" } }
            }),
            &["name"],
        ),
        tool(
            "save_phone",
            "Remember a person's phone number the user just gave you.",
            json!({ "name": { "type": "string" }, "phone": { "type": "string" } }),
            &["name", "phone"],
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
            "web_search",
            "Search the web for current events, prices, or anything after your training. Returns titles, links and snippets. Set news to true for time-sensitive news (needs a Brave key).",
            json!({ "query": { "type": "string" }, "news": { "type": "boolean" } }),
            &["query"],
        ),
        tool(
            "web_fetch",
            "Fetch an http(s) page and return its readable text (first 8000 characters).",
            json!({ "url": { "type": "string" } }),
            &["url"],
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
        tool(
            "read_whatsapp",
            "Read the latest WhatsApp messages in a chat, oldest first. Only messages that \
             arrived while Bloom was linked are available.",
            json!({
                "chat": { "type": "string", "description": "Saved name, phone number or group name" },
                "count": { "type": "integer", "description": "How many, up to 20 (default 10)" }
            }),
            &["chat"],
        ),
        tool(
            "list_whatsapp_chats",
            "List recent WhatsApp chats, newest first, with unread counts.",
            json!({}),
            &[],
        ),
        tool(
            "list_whatsapp_groups",
            "List the WhatsApp groups the user is in, most recent first.",
            json!({}),
            &[],
        ),
        tool(
            "send_whatsapp",
            "Send a WhatsApp text message as the user to a contact, a phone number or a group.",
            json!({
                "to": { "type": "string", "description": "Contact name, phone number or group name" },
                "text": { "type": "string" }
            }),
            &["to", "text"],
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
        "save_phone" => format!("Saving {}'s number", arg("name")),
        "tag_contact" => format!("Tagging {}", arg("name")),
        "send_email" => format!("Emailing {}", arg("to")),
        "run_powershell" => "Running a PowerShell script".into(),
        "remember" => "Remembering that".into(),
        "recall" => "Checking my memory".into(),
        "forget" => "Forgetting a fact".into(),
        "web_search" => format!("Searching the web for {}", arg("query")),
        "web_fetch" => format!("Reading {}", arg("url")),
        "use_skill" => format!("Using skill {}", arg("name")),
        "read_skill_file" => format!("Reading {}", arg("file")),
        "save_skill" => format!("Saving skill {}", arg("name")),
        "read_whatsapp" => format!("Reading WhatsApp with {}", arg("chat")),
        "list_whatsapp_chats" => "Checking WhatsApp".into(),
        "list_whatsapp_groups" => "Checking WhatsApp groups".into(),
        "send_whatsapp" => format!("Messaging {} on WhatsApp", arg("to")),
        _ if name.starts_with("mcp_") => format!("Using {}", &name[4..]),
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
                let ask = open_needs_confirm(ctx, &target);
                let title = if target.starts_with("http") || target.starts_with("ms-settings") {
                    "Open this page?"
                } else if std::path::Path::new(&target).is_dir() {
                    "Open this folder?"
                } else {
                    "Open this file?"
                };
                if !confirm_persist(ctx, ConfirmKind::Web, ask, title, &target).await {
                    return Ok("The user chose not to open it.".into());
                }
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
        "save_phone" => {
            let name = str_arg(args, "name")?;
            save_number(ctx, name, str_arg(args, "phone")?, "").await
        }
        "tag_contact" => tag_contact(ctx, args),
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
        "web_search" => {
            web::search(
                ctx,
                str_arg(args, "query")?,
                args["news"].as_bool() == Some(true),
            )
            .await
        }
        "web_fetch" => web::fetch(ctx, str_arg(args, "url")?).await,
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
        "read_whatsapp" => whatsapp::read(ctx, str_arg(args, "chat")?, args["count"].as_u64()),
        "list_whatsapp_chats" => whatsapp::list(ctx),
        "list_whatsapp_groups" => whatsapp::list_groups(ctx),
        "send_whatsapp" => whatsapp::send(ctx, str_arg(args, "to")?, str_arg(args, "text")?).await,
        _ if name.starts_with("mcp_") => crate::mcp::call(ctx, name, args).await,
        _ => Err(format!("unknown tool {name}")),
    }
}

/// Exfiltration guard, inverted: while tainted, `open` asks unless the target is
/// an ms-settings: page, a URL the user or a search named, or a local folder or
/// file of a plain data type on a local drive, judged on the canonical path
/// Windows would really open. Everything else confirms.
fn open_needs_confirm(ctx: &Ctx, target: &str) -> bool {
    if !ctx.tainted {
        return false;
    }
    let t = target.trim();
    let norm = |s: &str| reqwest::Url::parse(s).map_or_else(|_| s.to_ascii_lowercase(), String::from);
    let safe = t.to_ascii_lowercase().starts_with("ms-settings:")
        || ctx.allowed_urls.iter().any(|a| norm(a) == norm(t))
        || is_plain_local_path(t);
    !safe
}

/// File types whose handlers cannot fetch anything, and that write_file (text
/// only) cannot disguise markup or formulas in: no xml, csv, md, json, ics.
const DATA_EXTS: &[&str] = &[
    "txt", "log", "pdf", "png", "jpg", "jpeg", "gif", "webp", "bmp", "mp3", "wav", "mp4", "mov",
    "docx", "xlsx", "pptx",
];

fn is_plain_local_path(t: &str) -> bool {
    let Ok(path) = files::resolve_local(t) else {
        return false;
    };
    let p = std::path::Path::new(&path);
    let ok = p.is_dir()
        || p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| DATA_EXTS.contains(&e.to_ascii_lowercase().as_str()));
    ok && is_local_drive(path.as_bytes()[0] as char)
}

#[cfg(windows)]
fn is_local_drive(letter: char) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    let root: Vec<u16> = format!("{}:\\", letter.to_ascii_uppercase())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // DRIVE_REMOVABLE = 2, DRIVE_FIXED = 3; mapped network drives are DRIVE_REMOTE (4).
    matches!(unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) }, 2 | 3)
}

#[cfg(not(windows))]
fn is_local_drive(_letter: char) -> bool {
    false
}

/// Poisoned memory would persist, so a tainted request asks before writing it.
async fn confirm_memory(ctx: &Ctx, title: &str, body: &str) -> bool {
    confirm_persist(ctx, ConfirmKind::Memory, ctx.tainted, title, body).await
}

/// Asks when `needed` (tainted, an overwrite, an MCP call), except on carte blanche.
pub(crate) async fn confirm_persist(
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

/// A list argument the model may send as an array or as "a, b".
fn list_arg(args: &Value, key: &str) -> Vec<String> {
    match &args[key] {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        Value::String(s) => s
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// What find_contact searches: people.json, then the mail harvest and (with
/// WhatsApp on) the synced address book, merged in with their source.
fn contact_view(dir: &std::path::Path, whatsapp: bool) -> Result<Vec<Person>, String> {
    let mut all = people::load(dir)?;
    for p in mail_harvest::people(dir) {
        people::merge(&mut all, p);
    }
    if whatsapp {
        for c in whatsapp::book(dir).contacts {
            let mut p = Person::new(&c.name, people::WHATSAPP);
            p.add_phone(&c.number, "", people::WHATSAPP);
            people::merge(&mut all, p);
        }
    }
    Ok(all)
}

/// Anything shown that the user did not save themselves: outside data.
fn synced(p: &Person) -> bool {
    !p.is_user()
        || p.emails.iter().any(|e| e.source != people::USER)
        || p.phones.iter().any(|x| x.source != people::USER)
}

fn source_note(source: &str, label: &str) -> String {
    match source {
        people::MAIL if label == "sent" => {
            " (from the user's mail; they have written to it)".into()
        }
        people::MAIL => " (from the user's mail)".into(),
        people::WHATSAPP => " (from WhatsApp)".into(),
        people::OUTLOOK => " (from Outlook)".into(),
        _ if label.is_empty() => String::new(),
        _ => format!(" ({label})"),
    }
}

/// Every address and number of one person, then their tags.
fn person_lines(p: &Person) -> String {
    let emails = p.emails.iter().map(|e| {
        format!(
            "{} <{}>{}",
            p.name,
            e.address,
            source_note(&e.source, &e.label)
        )
    });
    let phones = p.phones.iter().map(|x| {
        format!(
            "{} phone: {}{}",
            p.name,
            x.number,
            source_note(&x.source, &x.label)
        )
    });
    let tags = (!p.tags.is_empty()).then(|| format!("{} tags: {}", p.name, p.tags.join(", ")));
    emails
        .chain(phones)
        .chain(tags)
        .collect::<Vec<_>>()
        .join("\n")
}

async fn find_contact(ctx: &mut Ctx, query: &str) -> Result<String, String> {
    let dir = ctx.shared.data_dir.clone();
    let mut view = contact_view(&dir, ctx.cfg.whatsapp)?;
    // A miss may mean the mail has new people: scan it, at most once a day.
    if matches!(people::lookup(&view, query), Found::None) && refresh_mail_if_due(ctx).await {
        view = contact_view(&dir, ctx.cfg.whatsapp)?;
    }
    match people::lookup(&view, query) {
        Found::One(p, by_other_name) => {
            ctx.tainted |= by_other_name || synced(p);
            let mut out = person_lines(p);
            if p.emails.is_empty() {
                out += &format!(
                    "\nNo saved email for {}. Ask the user for the address, then call save_contact.",
                    p.name
                );
            } else if !p.is_user() {
                out += "\nIf the user confirms, save it with save_contact.";
            }
            return Ok(out);
        }
        Found::Many(list) => {
            ctx.tainted |= list.iter().any(|p| synced(p));
            let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
            let question = match names.as_slice() {
                [one] => format!("Did you mean {one}? Ask the user to confirm."),
                [rest @ .., last] => {
                    let count = match names.len() {
                        2 => "two".to_string(),
                        3 => "three".to_string(),
                        n => n.to_string(),
                    };
                    format!(
                        "I found {count} contacts matching {query}: {} or {last}? Ask the user which one they mean.",
                        rest.join(", ")
                    )
                }
                [] => unreachable!("Many is never empty"),
            };
            let details: Vec<String> = list.iter().map(|p| person_lines(p)).collect();
            return Ok(coded(
                CONTACT_AMBIGUOUS,
                &format!("{question}\n{}", details.join("\n")),
            ));
        }
        Found::None => {}
    }
    // Not saved and not in the scanned mail: look through older Sent mail.
    // Any failure here (no email set up, offline) just falls through.
    if let Ok(server) = email::server_for(&ctx.cfg) {
        if let Ok(secret) = mail_secret(&ctx.shared.http, &server).await {
            let (user, q) = (ctx.cfg.email.clone(), query.to_string());
            let found = tokio::task::spawn_blocking(move || {
                imap_lookup::sent_to(&server, &user, &secret, &q)
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
    let groups = if ctx.cfg.whatsapp {
        whatsapp::group_lines(&dir, query)
    } else {
        String::new()
    };
    if !groups.is_empty() {
        // Group subjects anyone in them can set: outside data.
        ctx.tainted = true;
        return Ok(format!(
            "No saved contact matches {query}. In the user's WhatsApp:{groups}"
        ));
    }
    Ok(coded(
        CONTACT_NOT_FOUND,
        &format!(
            "No saved contact matches {query}. Ask the user for the address, then call save_contact."
        ),
    ))
}

/// After a find_contact miss: scans the mail headers if it was not tried
/// today. True when the scan worked.
async fn refresh_mail_if_due(ctx: &Ctx) -> bool {
    let Ok(server) = email::server_for(&ctx.cfg) else {
        return false;
    };
    if server.oauth || !mail_harvest::try_now(&ctx.shared.data_dir, people::now()) {
        return false;
    }
    harvest_mail(&ctx.shared, &ctx.cfg).await.is_ok()
}

pub const OUTLOOK_HARVEST: &str =
    "Microsoft accounts are not scanned here: their contacts will come from Outlook sync.";

/// Scans the user's mail headers (IMAP envelopes only) into
/// mail-contacts.json. Returns how many people it found.
pub async fn harvest_mail(
    shared: &crate::agent::Shared,
    cfg: &crate::config::Config,
) -> Result<usize, String> {
    let server = email::server_for(cfg)?;
    if server.oauth {
        return Err(OUTLOOK_HARVEST.into());
    }
    let secret = mail_secret(&shared.http, &server).await?;
    let (dir, user) = (shared.data_dir.clone(), cfg.email.clone());
    tokio::task::spawn_blocking(move || {
        let mut mailbox = mail_harvest::Imap(imap_lookup::connect(&server, &user, &secret)?);
        let found = mail_harvest::run(&dir, &mut mailbox, &user);
        let _ = mailbox.0.logout();
        found
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Competent mode emails these without asking: addresses the user saved, and
/// ones they have written to before (from the mail scan).
fn known_recipient(dir: &std::path::Path, address: &str) -> bool {
    let saved = people::load(dir).is_ok_and(|all| {
        all.iter().any(|p| {
            p.emails
                .iter()
                .any(|e| e.source == people::USER && e.address == address)
        })
    });
    saved || mail_harvest::sent_to(dir, address)
}

async fn save_contact(ctx: &mut Ctx, args: &Value) -> Result<String, String> {
    let name = str_arg(args, "name")?.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    let text = |key: &str| args[key].as_str().map(str::trim).filter(|s| !s.is_empty());
    let label = text("label").unwrap_or_default();
    let tags = list_arg(args, "tags");
    let mut said = Vec::new();
    if let Some(address) = text("email") {
        said.push(save_address(ctx, name, address, label, &tags).await?);
    } else if !tags.is_empty() {
        said.push(retag(&ctx.shared.data_dir, name, &tags, &[])?);
    }
    if let Some(phone) = text("phone") {
        said.push(save_number(ctx, name, phone, label).await?);
    }
    if said.is_empty() {
        return Err("Give an email address, a phone number or tags to save.".into());
    }
    Ok(said.join("\n"))
}

/// Adds an address to `name`. A different address for someone untagged is a
/// change, so it asks; with tags both are kept. The same address under
/// another name is the same person: nothing changes.
async fn save_address(
    ctx: &mut Ctx,
    name: &str,
    raw: &str,
    label: &str,
    tags: &[String],
) -> Result<String, String> {
    let address = people::norm_email(raw);
    if !email::is_email(&address) {
        return Err(format!("{address} is not an email address"));
    }
    for t in tags {
        people::norm_tag(t)?;
    }
    let dir = ctx.shared.data_dir.clone();
    let saved = people::load(&dir)?;
    let key = people::fold(name);
    if let Some(owner) = saved
        .iter()
        .find(|p| p.has_email(&address) && people::fold(&p.name) != key)
    {
        return Ok(format!(
            "{address} is already saved for {}; nothing changed.",
            owner.name
        ));
    }
    let person = saved
        .iter()
        .find(|p| people::fold(&p.name) == key && p.is_user());
    let old = person
        .filter(|p| !p.has_email(&address))
        .and_then(|p| p.emails.iter().find(|e| e.primary).or(p.emails.first()))
        .map(|e| e.address.clone());
    let keep_both = !tags.is_empty() || person.is_some_and(|p| !p.tags.is_empty());
    // Similar names alone never merge: say so instead.
    let similar: Vec<&str> = if person.is_none() {
        saved
            .iter()
            .filter(|p| people::score(name, p).0 >= people::SURE)
            .map(|p| p.name.as_str())
            .collect()
    } else {
        Vec::new()
    };
    match old {
        Some(old) if !keep_both => {
            let approved = ctx
                .shared
                .bridge
                .confirm(
                    ctx.task,
                    ConfirmKind::Email,
                    format!("Change {name}'s address to {address}?"),
                    format!("Saved address: {old}\nNew address: {address}"),
                )
                .await;
            if !approved {
                return Ok("The user kept the saved address.".into());
            }
            email::save_contact(&dir, name, &address)?;
        }
        _ => people::update(&dir, |all| {
            if people::named(all, name).is_none() {
                all.push(Person::new(name, people::USER));
            }
            let p = people::named(all, name).expect("just added");
            p.add_tags(tags)?;
            p.add_email(&address, label, people::USER);
            Ok(())
        })?,
    }
    ctx.saved_this_task.insert(address.clone());
    let mut out = format!("Saved {name} <{address}>.");
    if !similar.is_empty() {
        out += &format!(
            " Kept apart from the similar {} (similar names are never merged; the same email or phone is).",
            similar.join(", ")
        );
    }
    Ok(out)
}

/// Sets `name`'s number. A different saved number asks first; so does a new
/// one while outside text is in the request.
async fn save_number(ctx: &mut Ctx, name: &str, raw: &str, label: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    let number = phones::normalize(raw)?;
    let dir = ctx.shared.data_dir.clone();
    let saved = people::load(&dir)?;
    let key = people::fold(name);
    let mine: Vec<&people::Phone> = saved
        .iter()
        .filter(|p| people::fold(&p.name) == key && p.is_user())
        .flat_map(|p| p.phones.iter().filter(|x| x.source == people::USER))
        .collect();
    let already = mine.iter().any(|x| x.number == number);
    let old = mine.iter().find(|x| x.primary).or(mine.first());
    let ask = if let Some(old) = old.filter(|_| !already) {
        Some((
            format!("Change {name}'s number to {number}?"),
            format!("Saved number: {}\nNew number: {number}", old.number),
            "The user kept the saved number.",
        ))
    } else if ctx.tainted && !already && ctx.cfg.tier != crate::config::Tier::CarteBlanche {
        // Outside text (a WhatsApp chat, say) may be asking: saved numbers can
        // get automatic WhatsApp replies.
        Some((
            format!("Save {name}'s number {number}?"),
            format!("{name}\n{number}"),
            "The user chose not to save it.",
        ))
    } else {
        None
    };
    if let Some((title, body, declined)) = ask {
        if !ctx
            .shared
            .bridge
            .confirm(ctx.task, ConfirmKind::Message, title, body)
            .await
        {
            return Ok(declined.into());
        }
    }
    phones::save(&dir, name, &number)?;
    if !label.is_empty() {
        people::update(&dir, |all| {
            if let Some(x) = people::named(all, name)
                .and_then(|p| p.phones.iter_mut().find(|x| x.number == number))
            {
                x.label = label.to_lowercase();
            }
            Ok(())
        })?;
    }
    // Not a known number for send_whatsapp until the next request.
    ctx.saved_this_task.insert(number.clone());
    Ok(format!("Saved {name} {number}."))
}

fn tag_contact(ctx: &mut Ctx, args: &Value) -> Result<String, String> {
    retag(
        &ctx.shared.data_dir,
        str_arg(args, "name")?,
        &list_arg(args, "add"),
        &list_arg(args, "remove"),
    )
}

/// Adds and removes tags on a saved contact: the exact name, else one clear
/// fuzzy match.
fn retag(
    dir: &std::path::Path,
    name: &str,
    add: &[String],
    remove: &[String],
) -> Result<String, String> {
    let remove: Vec<String> = remove
        .iter()
        .filter_map(|t| people::norm_tag(t).ok())
        .collect();
    people::update(dir, |all| {
        let key = people::fold(name);
        let i =
            match all.iter().position(|p| people::fold(&p.name) == key) {
                Some(i) => i,
                None => match people::lookup(all, name) {
                    Found::One(p, _) => {
                        let id = p.id.clone();
                        all.iter().position(|q| q.id == id).expect("listed")
                    }
                    Found::Many(list) => {
                        let names: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
                        return Err(coded(
                            CONTACT_AMBIGUOUS,
                            &format!(
                                "Several saved contacts match {name}: {}. Ask the user which one.",
                                names.join(", ")
                            ),
                        ));
                    }
                    Found::None => return Err(coded(
                        CONTACT_NOT_FOUND,
                        &format!(
                            "No saved contact matches {name}. Save them first with save_contact."
                        ),
                    )),
                },
            };
        let p = &mut all[i];
        p.add_tags(add)?;
        p.tags.retain(|t| !remove.contains(t));
        Ok(if p.tags.is_empty() {
            format!("{} has no tags now.", p.name)
        } else {
            format!("{} tags: {}.", p.name, p.tags.join(", "))
        })
    })
}

/// Settings > Contacts: one change to people.json, through the same rules as
/// the tools, then the list. `action`: list, add_tag, remove_tag, add_email,
/// delete or harvest (scan the mail now).
pub async fn contacts(
    shared: &crate::agent::Shared,
    action: &str,
    id: &str,
    value: &str,
    label: &str,
) -> crate::protocol::Out {
    let dir = &shared.data_dir;
    let cfg = crate::config::Config::load(&shared.settings_path);
    let edit = |change: &dyn Fn(&mut Person) -> Result<(), String>| {
        people::update(dir, |all| {
            let p = all
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("That contact is gone. Reopen the list.")?;
            change(p)
        })
    };
    let mut message = None;
    let done = match action {
        "list" => Ok(()),
        "add_tag" => edit(&|p| p.add_tags(&[value.to_string()])),
        "remove_tag" => edit(&|p| {
            p.tags.retain(|t| t != value);
            Ok(())
        }),
        "add_email" => {
            let address = people::norm_email(value);
            if email::is_email(&address) {
                people::update(dir, |all| {
                    if let Some(o) = all.iter().find(|p| p.has_email(&address) && p.id != id) {
                        return Err(format!("{address} is already saved for {}.", o.name));
                    }
                    let p = all
                        .iter_mut()
                        .find(|p| p.id == id)
                        .ok_or("That contact is gone. Reopen the list.")?;
                    p.add_email(&address, label, people::USER);
                    Ok(())
                })
            } else {
                Err(format!("{address} is not an email address"))
            }
        }
        "delete" => people::update(dir, |all| {
            all.retain(|p| p.id != id);
            Ok(())
        }),
        "harvest" => match harvest_mail(shared, &cfg).await {
            Ok(n) => {
                message = Some(format!("Found {n} people in your mail."));
                Ok(())
            }
            Err(e) => Err(e),
        },
        other => Err(format!("unknown contacts action {other}")),
    };
    let (list, unreadable) = match people::load(dir) {
        Ok(list) => (list, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    crate::protocol::Out::Contacts {
        people: list,
        mail: mail_harvest::load(dir).map_or(0, |m| m.len()),
        outlook: email::server_for(&cfg).is_ok_and(|s| s.oauth),
        message,
        error: done.err().or(unreadable),
    }
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
    let known = known_recipient(&ctx.shared.data_dir, &to) && !ctx.saved_this_task.contains(&to);
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
    async fn tainted_open_of_an_unnamed_page_asks_and_decline_does_not_open() {
        let mut ctx = ctx();
        ctx.tainted = true;
        let shared = ctx.shared.clone();
        let args = json!({ "target": "https://evil.test/?q=secret" });
        let running = tokio::spawn(async move { call(&mut ctx, "open", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert!(running.await.unwrap().unwrap().contains("not to open"));
    }

    #[test]
    fn open_confirms_unless_settings_allowed_url_or_plain_local_data_while_tainted() {
        let mut c = ctx();
        let d = crate::testutil::temp_dir().canonicalize().unwrap();
        let d = d.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        let touch = |n: &str| {
            let f = format!("{d}\\{n}");
            std::fs::write(&f, "").unwrap();
            f
        };
        let (txt, html, url, exe) = (touch("a.TXT"), touch("a.html"), touch("a.url"), touch("e.exe"));
        let risky: Vec<String> = vec![
            "https://example.com/a".into(),
            "HTTP://x".into(),
            "mailto:a@b.c?body=x".into(),
            "steam://x".into(),
            "//host/share".into(),
            "file://host/f".into(),
            r"\\host\share\f.txt".into(),
            r"\/evil/share/x.txt".into(),
            r"/\evil/share/x.txt".into(),
            format!("\"{txt}\""),
            "\"HTTP://evil/?q=1\"".into(),
            format!("'{txt}'"),
            r"\\?\UNC\h\s".into(),
            url.clone(),
            format!("{url}\u{0}"),
            format!("{url}\\."),
            format!("{url}."),
            format!("{url}::$DATA"),
            exe,
            html,
            touch("a.xml"),
            touch("a.csv"),
            touch("a.md"),
            touch("a.json"),
            touch("a.ics"),
            touch("a.yaml"),
            format!("{d}\\missing.txt"),
        ];
        assert!(risky.iter().all(|t| !open_needs_confirm(&c, t)), "untainted");
        c.tainted = true;
        for t in &risky {
            assert!(open_needs_confirm(&c, t), "{t}");
        }
        for ok in [txt.clone(), txt.replace('\\', "/"), d.clone(), "MS-SETTINGS:display".into()] {
            assert!(!open_needs_confirm(&c, &ok), "{ok}");
        }
        c.allowed_urls.insert("https://example.com/a".into());
        assert!(!open_needs_confirm(&c, "HTTPS://EXAMPLE.com/a"));
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
    async fn find_contact_returns_emails_and_phones() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        email::save_contact(&dir, "Neha", "neha@example.com").unwrap();
        phones::save(&dir, "Neha", "+919876543210").unwrap();
        phones::save(&dir, "Sam", "+14155550100").unwrap();
        let out = call(&mut ctx, "find_contact", &json!({ "name": "neha" }))
            .await
            .unwrap();
        assert!(out.contains("neha@example.com") && out.contains("+919876543210"));
        let out = call(&mut ctx, "find_contact", &json!({ "name": "Sam" }))
            .await
            .unwrap();
        assert!(out.contains("+14155550100"));
    }

    #[tokio::test]
    async fn find_contact_searches_whatsapp_after_the_users_own_files() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        crate::whatsapp::tests::sync_book(&dir);
        phones::save(&dir, "Neha", "+919876543210").unwrap();
        // Off: nothing synced is offered.
        let out = call(&mut ctx, "find_contact", &json!({ "name": "Sam" }))
            .await
            .unwrap();
        assert!(!out.contains("+4917000000003"), "{out}");
        ctx.cfg.whatsapp = true;
        let out = call(&mut ctx, "find_contact", &json!({ "name": "neha" }))
            .await
            .unwrap();
        assert!(
            out.contains("+919876543210") && !out.contains("+4917000000002"),
            "{out}"
        );
        assert!(!ctx.tainted);
        let out = call(&mut ctx, "find_contact", &json!({ "name": "sam" }))
            .await
            .unwrap();
        assert!(
            out.contains("Sam phone: +4917000000003 (from WhatsApp)"),
            "{out}"
        );
        assert!(ctx.tainted, "names from WhatsApp are outside data");
        ctx.tainted = false;
        let out = call(&mut ctx, "find_contact", &json!({ "name": "Nobody" }))
            .await
            .unwrap();
        assert!(out.contains("save_contact"), "{out}");
        assert!(!ctx.tainted, "nothing from WhatsApp in the answer");
        let out = call(&mut ctx, "find_contact", &json!({ "name": "family" }))
            .await
            .unwrap();
        assert!(
            out.contains("Family (WhatsApp group)") && out.contains("Family Trip (WhatsApp group)"),
            "{out}"
        );
    }

    #[tokio::test]
    async fn phone_only_contact_still_gets_the_save_contact_hint() {
        let mut ctx = ctx();
        phones::save(&ctx.shared.data_dir, "Sam", "+14155550100").unwrap();
        let out = call(&mut ctx, "find_contact", &json!({ "name": "Sam" }))
            .await
            .unwrap();
        assert!(
            out.contains("save_contact") && out.contains("+14155550100"),
            "{out}"
        );
        // people.json is the store now (phones.json stays only as a backup).
        std::fs::write(ctx.shared.data_dir.join("people.json"), "{bad").unwrap();
        let err = call(&mut ctx, "find_contact", &json!({ "name": "Sam" }))
            .await
            .unwrap_err();
        assert!(err.contains("not valid JSON"), "{err}");
    }

    #[tokio::test]
    async fn blank_name_cannot_save_a_phone() {
        let mut ctx = ctx();
        let args = json!({ "name": "  ", "phone": "+14155550100" });
        assert!(call(&mut ctx, "save_phone", &args).await.is_err());
    }

    #[tokio::test]
    async fn changing_a_saved_number_asks_first() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        phones::save(&dir, "Neha", "+919876543210").unwrap();
        let shared = ctx.shared.clone();
        let args = json!({ "name": "Neha", "phone": "+49 170 1234567" });
        let running = tokio::spawn(async move { call(&mut ctx, "save_phone", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert_eq!(
            running.await.unwrap(),
            Ok("The user kept the saved number.".into())
        );
        assert_eq!(phones::load(&dir).get("Neha").unwrap(), "+919876543210");

        let mut ctx = crate::testutil::ctx();
        let dir = ctx.shared.data_dir.clone();
        phones::save(&dir, "Neha", "+919876543210").unwrap();
        let shared = ctx.shared.clone();
        let args = json!({ "name": "Neha", "phone": "+49 170 1234567" });
        let running = tokio::spawn(async move { call(&mut ctx, "save_phone", &args).await });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(true));
        assert!(running.await.unwrap().is_ok());
        assert_eq!(phones::load(&dir).get("Neha").unwrap(), "+491701234567");
    }

    #[tokio::test]
    async fn saving_a_new_number_while_tainted_asks_unless_carte_blanche() {
        use crate::config::Tier;
        for (tier, approve, asks) in [
            (Tier::Competent, false, true),
            (Tier::Conservative, true, true),
            (Tier::CarteBlanche, false, false),
        ] {
            let mut ctx = ctx();
            ctx.cfg.tier = tier;
            ctx.tainted = true;
            let (dir, shared) = (ctx.shared.data_dir.clone(), ctx.shared.clone());
            let args = json!({ "name": "Eve", "phone": "+14155550100" });
            let running = tokio::spawn(async move { call(&mut ctx, "save_phone", &args).await });
            tokio::task::yield_now().await;
            let asked = shared.bridge.answer_pending(Answer::Confirm(approve));
            let out = running.await.unwrap().unwrap();
            assert_eq!(asked, asks, "{tier:?}");
            let saved = phones::load(&dir).contains_key("Eve");
            assert_eq!(saved, !asks || approve, "{tier:?}: {out}");
            if asks && !approve {
                assert_eq!(out, "The user chose not to save it.");
            }
        }
    }

    #[tokio::test]
    async fn saving_a_new_number_does_not_ask() {
        let mut ctx = ctx();
        let dir = ctx.shared.data_dir.clone();
        let args = json!({ "name": "Bob", "phone": "+1 (415) 555-0100" });
        assert!(call(&mut ctx, "save_phone", &args).await.is_ok());
        assert_eq!(phones::load(&dir).get("Bob").unwrap(), "+14155550100");
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

    /// Runs one tool call, answering any confirm with `approve`. Returns the
    /// result, whether it asked, and the Ctx.
    async fn run(
        mut c: Ctx,
        name: &str,
        args: Value,
        approve: bool,
    ) -> (Result<String, String>, bool, Ctx) {
        let shared = c.shared.clone();
        let name = name.to_string();
        let running = tokio::spawn(async move {
            let r = call(&mut c, &name, &args).await;
            (r, c)
        });
        tokio::task::yield_now().await;
        let asked = shared.bridge.answer_pending(Answer::Confirm(approve));
        let (r, c) = running.await.unwrap();
        (r, asked, c)
    }

    fn person(dir: &std::path::Path, name: &str) -> Person {
        let key = people::fold(name);
        people::load(dir)
            .unwrap()
            .into_iter()
            .find(|p| people::fold(&p.name) == key)
            .unwrap()
    }

    #[tokio::test]
    async fn save_contact_appends_with_tags_and_asks_to_change_without() {
        let c = ctx();
        let dir = c.shared.data_dir.clone();
        let args = json!({ "name": "Ben Tan", "email": "ben@home.com" });
        let (out, asked, c) = run(c, "save_contact", args, true).await;
        assert!(!asked && out.unwrap() == "Saved Ben Tan <ben@home.com>.");
        // A second address with no tags is a change: it asks, and a no keeps one.
        let args = json!({ "name": "ben tan", "email": "ben@work.com" });
        let (out, asked, c) = run(c, "save_contact", args, false).await;
        assert!(asked);
        assert_eq!(out.unwrap(), "The user kept the saved address.");
        assert_eq!(person(&dir, "Ben Tan").emails.len(), 1);
        // With tags both are kept, without asking.
        let args = json!({ "name": "Ben Tan", "email": "ben@work.com", "label": "Work", "tags": ["work", "Gym Buddies"] });
        let (out, asked, c) = run(c, "save_contact", args, false).await;
        assert!(!asked, "{out:?}");
        let p = person(&dir, "Ben Tan");
        assert_eq!(
            p.emails
                .iter()
                .map(|e| e.address.as_str())
                .collect::<Vec<_>>(),
            ["ben@home.com", "ben@work.com"]
        );
        assert_eq!(p.emails[1].label, "work");
        assert_eq!(p.tags, ["work", "gym-buddies"]);
        // Tags come off one at a time, but never the last of a two-address person.
        let args = json!({ "name": "ben", "remove": ["gym buddies"], "add": "family" });
        let (out, _, c) = run(c, "tag_contact", args, true).await;
        assert_eq!(out.unwrap(), "Ben Tan tags: work, family.");
        let args = json!({ "name": "Ben Tan", "remove": ["work", "family"] });
        let (out, _, c) = run(c, "tag_contact", args, true).await;
        assert!(out.unwrap_err().contains("need at least one tag"));
        assert_eq!(person(&dir, "Ben Tan").tags, ["work", "family"]);
        // Same email under another name: the same person, nothing changes.
        let args = json!({ "name": "Benny", "email": "BEN@work.com" });
        let (out, _, c) = run(c, "save_contact", args, true).await;
        assert!(out.unwrap().contains("already saved for Ben Tan"));
        assert_eq!(people::load(&dir).unwrap().len(), 1);
        // A similar name alone is a new person, and the reply says so.
        let args = json!({ "name": "Ben", "email": "other.ben@x.com" });
        let (out, _, c) = run(c, "save_contact", args, true).await;
        assert!(out.unwrap().contains("Kept apart from the similar Ben Tan"));
        assert_eq!(people::load(&dir).unwrap().len(), 2);
        // An email and a phone in one call.
        let args = json!({ "name": "Sam", "email": "sam@x.org", "phone": "+1 415 555 0100", "label": "mobile" });
        let (out, _, c) = run(c, "save_contact", args, true).await;
        assert_eq!(
            out.unwrap(),
            "Saved Sam <sam@x.org>.\nSaved Sam +14155550100."
        );
        assert_eq!(person(&dir, "sam").phones[0].label, "mobile");
        let (out, _, _) = run(
            c,
            "tag_contact",
            json!({ "name": "Nobody", "add": ["x"] }),
            true,
        )
        .await;
        assert!(out.unwrap_err().starts_with("CONTACT_NOT_FOUND: "));
    }

    #[tokio::test]
    async fn find_contact_is_fuzzy_and_lists_ambiguous_people() {
        let mut c = ctx();
        let dir = c.shared.data_dir.clone();
        people::update(&dir, |all| {
            for (name, address) in [
                ("Benjamin Tan", "ben@tan.com"),
                ("Alex Tan", "alex@tan.com"),
                ("Alex Lim", "alex@lim.com"),
            ] {
                let mut p = Person::new(name, people::USER);
                p.add_email(address, "", people::USER);
                all.push(p);
            }
            let ben = people::named(all, "Benjamin Tan").unwrap();
            ben.add_tags(&["work".into()])?;
            ben.add_email("benjamin@corp.com", "work", people::USER);
            Ok(())
        })
        .unwrap();
        for q in ["Ben", "Benjamin", "Ben Tan", "bent tan", "Benny"] {
            let out = call(&mut c, "find_contact", &json!({ "name": q }))
                .await
                .unwrap();
            assert!(
                out.contains("Benjamin Tan <ben@tan.com>")
                    && out.contains("Benjamin Tan <benjamin@corp.com> (work)")
                    && out.contains("Benjamin Tan tags: work"),
                "{q}: {out}"
            );
        }
        let out = call(&mut c, "find_contact", &json!({ "name": "Alex" }))
            .await
            .unwrap();
        assert!(
            out.starts_with(
                "CONTACT_AMBIGUOUS: I found two contacts matching Alex: Alex Tan or Alex Lim?"
            ),
            "{out}"
        );
        assert!(out.contains("alex@tan.com") && out.contains("alex@lim.com"));
        assert!(!c.tainted, "only the user's own contacts");
        let out = call(&mut c, "find_contact", &json!({ "name": "Zed" }))
            .await
            .unwrap();
        assert!(out.starts_with("CONTACT_NOT_FOUND: "), "{out}");
    }

    fn harvested(dir: &std::path::Path) {
        let rows = json!([
            { "name": "Priya Raman", "email": "priya@x.com", "sent": true, "count": 4, "last": 1 },
            { "name": "Priya Shop", "email": "priya@shop.com", "sent": false, "count": 9, "last": 1 },
            { "name": "Neha Mail", "email": "neha@example.com", "sent": false, "count": 1, "last": 1 }
        ]);
        std::fs::write(dir.join("mail-contacts.json"), rows.to_string()).unwrap();
    }

    #[tokio::test]
    async fn find_contact_searches_the_mail_scan_after_own_contacts() {
        let mut c = ctx();
        let dir = c.shared.data_dir.clone();
        email::save_contact(&dir, "Neha", "neha@example.com").unwrap();
        harvested(&dir);
        // The user's own contact; the scan's name for the same address is not shown.
        let out = call(&mut c, "find_contact", &json!({ "name": "neha" }))
            .await
            .unwrap();
        assert_eq!(out, "Neha <neha@example.com>");
        assert!(!c.tainted);
        // Two Priyas from the mail: sent-to listed first.
        let out = call(&mut c, "find_contact", &json!({ "name": "priya" }))
            .await
            .unwrap();
        assert!(out.starts_with("CONTACT_AMBIGUOUS: I found two contacts matching priya: Priya Raman or Priya Shop?"), "{out}");
        assert!(c.tainted, "names from mail are outside data");
        c.tainted = false;
        let out = call(&mut c, "find_contact", &json!({ "name": "Priya Raman" }))
            .await
            .unwrap();
        assert!(
            out.contains(
                "Priya Raman <priya@x.com> (from the user's mail; they have written to it)"
            ) && out.contains("save_contact"),
            "{out}"
        );
        assert!(c.tainted);
    }

    #[tokio::test]
    async fn competent_emails_people_the_user_has_written_to_without_asking() {
        use crate::config::Tier;
        for (to, asks) in [
            ("priya@x.com", false),
            ("priya@shop.com", true),
            ("neha@example.com", true),
        ] {
            let mut c = ctx();
            c.cfg.email = "me@mycompany.com".into();
            c.cfg.smtp_host = "smtp.invalid".into();
            c.cfg.tier = Tier::Competent;
            harvested(&c.shared.data_dir);
            let args = json!({ "to": to, "subject": "s", "body": "b" });
            let (_, asked, _) = run(c, "send_email", args, false).await;
            assert_eq!(asked, asks, "{to}");
        }
    }

    #[tokio::test]
    async fn settings_edits_go_through_the_same_rules() {
        let c = ctx();
        let dir = c.shared.data_dir.clone();
        email::save_contact(&dir, "Neha", "neha@example.com").unwrap();
        let id = person(&dir, "Neha").id;
        let s = &c.shared;
        let error = |out: crate::protocol::Out| match out {
            crate::protocol::Out::Contacts { error, .. } => error,
            _ => unreachable!(),
        };
        let e = error(contacts(s, "add_email", &id, "neha@work.com", "work").await);
        assert!(e.unwrap().contains("need at least one tag"));
        assert!(error(contacts(s, "add_tag", &id, "Work", "").await).is_none());
        assert!(error(contacts(s, "add_email", &id, "Neha@Work.com", "Work").await).is_none());
        let p = person(&dir, "Neha");
        assert_eq!(
            (p.tags.len(), p.emails.len(), p.emails[1].label.as_str()),
            (1, 2, "work")
        );
        assert!(error(contacts(s, "remove_tag", &id, "work", "").await).is_some());
        assert!(error(contacts(s, "add_email", &id, "bad", "").await)
            .unwrap()
            .contains("not an email"));
        match contacts(s, "delete", &id, "", "").await {
            crate::protocol::Out::Contacts {
                people,
                mail,
                outlook,
                error,
                ..
            } => {
                assert!(people.is_empty() && mail == 0 && !outlook && error.is_none());
            }
            _ => unreachable!(),
        }
        assert!(error(contacts(s, "add_tag", &id, "x", "").await)
            .unwrap()
            .contains("gone"));
        let e = error(contacts(s, "harvest", "", "", "").await).unwrap();
        assert!(e.contains("Set up email"), "{e}");
    }
}
