//! Local lifecycle-hook status. No chat content, credentials, or approval decisions.
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const HOOK_SCRIPT: &str = include_str!("../../scripts/codex-hook.ps1");
const HOOK_EVENTS: [&str; 8] = [
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "Stop",
    "Interrupt",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSession {
    id: String,
    title: String,
    project: String,
    status: String,
    detail: String,
    updated_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSnapshot {
    installed: bool,
    sessions: Vec<CodexSession>,
}

fn codex_root() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CODEX_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    std::env::var_os("USERPROFILE")
        .map(|path| PathBuf::from(path).join(".codex"))
        .ok_or_else(|| "Cannot locate the Codex configuration folder.".into())
}

fn state_root() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA")
        .map(|path| PathBuf::from(path).join("bloom").join("codex"))
        .ok_or_else(|| "Cannot locate the Bloom configuration folder.".into())
}

fn hook_command(script: &std::path::Path) -> String {
    format!("powershell.exe -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File \"{}\"", script.display())
}

fn hooks_installed(root: &std::path::Path, command: &str) -> bool {
    if !state_root().is_ok_and(|path| path.join("bloom-codex-hook.ps1").is_file()) {
        return false;
    }
    let Some(config) = fs::read(root.join("hooks.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };
    HOOK_EVENTS.iter().all(|event| {
        config["hooks"][event].as_array().is_some_and(|groups| {
            groups.iter().any(|group| {
                group["hooks"].as_array().is_some_and(|hooks| {
                    hooks
                        .iter()
                        .any(|hook| hook["command"].as_str() == Some(command))
                })
            })
        })
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn clipped(value: &str, length: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(length)
        .collect()
}

// The local index supplies titles without reading the conversation transcript.
// Its format is best-effort: project + id remain useful if Codex changes it.
fn session_titles(root: &std::path::Path) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    let Ok(mut file) = File::open(root.join("session_index.jsonl")) else {
        return titles;
    };
    let Ok(metadata) = file.metadata() else {
        return titles;
    };
    let start = metadata.len().saturating_sub(1024 * 1024);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return titles;
    }
    let mut bytes = Vec::new();
    if file.take(1024 * 1024).read_to_end(&mut bytes).is_err() {
        return titles;
    }
    for line in String::from_utf8_lossy(&bytes)
        .lines()
        .skip(usize::from(start > 0))
    {
        if let Ok(row) = serde_json::from_str::<Value>(line) {
            if let (Some(id), Some(title)) = (row["id"].as_str(), row["thread_name"].as_str()) {
                titles.insert(id.into(), clipped(title, 140));
            }
        }
    }
    titles
}

fn parse_session(row: &Value, titles: &HashMap<String, String>, now: u64) -> Option<CodexSession> {
    if row["schema"].as_u64()? != 1 {
        return None;
    }
    let id = row["session_id"].as_str()?;
    if !valid_id(id) {
        return None;
    }
    let updated_at = row["updated_at"].as_u64()?;
    let age = now.saturating_sub(updated_at);
    if updated_at > now + 60_000 || age > 7 * 24 * 60 * 60 * 1000 {
        return None;
    }
    let raw_status = row["status"].as_str()?;
    if raw_status == "closed" {
        return None;
    }
    let mut status = match raw_status {
        "idle" | "running" | "awaiting_approval" | "completed" | "interrupted" => raw_status,
        _ => return None,
    };
    let mut detail = clipped(row["detail"].as_str().unwrap_or(""), 240);
    // Hooks don't heartbeat; never assert that an old process is still running.
    if matches!(status, "running" | "awaiting_approval") && age > 15 * 60 * 1000 {
        status = "unknown";
        detail = "No recent signal. Open Codex to check this chat.".into();
    }
    let cwd = row["cwd"]
        .as_str()
        .unwrap_or("")
        .trim_end_matches(['\\', '/']);
    let project = clipped(cwd.rsplit(['\\', '/']).next().unwrap_or(""), 80);
    let title = titles
        .get(id)
        .filter(|title| !title.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "{} · {}",
                if project.is_empty() {
                    "Codex"
                } else {
                    &project
                },
                &id[..id.len().min(8)]
            )
        });
    Some(CodexSession {
        id: id.into(),
        title,
        project,
        status: status.into(),
        detail,
        updated_at,
    })
}

fn read_snapshot() -> Result<CodexSnapshot, String> {
    let root = state_root()?;
    let codex = codex_root()?;
    let installed = hooks_installed(&codex, &hook_command(&root.join("bloom-codex-hook.ps1")));
    if !installed {
        return Ok(CodexSnapshot {
            installed,
            sessions: Vec::new(),
        });
    }
    let titles = session_titles(&codex);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut sessions = Vec::new();
    match fs::read_dir(&root) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    continue;
                }
                let Ok(file) = File::open(&path) else {
                    continue;
                };
                let mut bytes = Vec::new();
                if file.take(32 * 1024 + 1).read_to_end(&mut bytes).is_err()
                    || bytes.len() > 32 * 1024
                {
                    continue;
                }
                if let Ok(row) = serde_json::from_slice::<Value>(&bytes) {
                    if let Some(session) = parse_session(&row, &titles, now) {
                        sessions.push(session);
                    }
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Cannot read the Codex status folder.".into()),
    }
    sessions.sort_by(|a, b| {
        let rank = |s: &str| match s {
            "awaiting_approval" => 0,
            "running" => 1,
            _ => 2,
        };
        rank(&a.status)
            .cmp(&rank(&b.status))
            .then(b.updated_at.cmp(&a.updated_at))
    });
    sessions.truncate(24);
    Ok(CodexSnapshot {
        installed,
        sessions,
    })
}

#[tauri::command]
pub async fn get_codex_sessions() -> Result<CodexSnapshot, String> {
    tauri::async_runtime::spawn_blocking(read_snapshot)
        .await
        .map_err(|_| "Cannot read Codex status.".to_string())?
}

fn configure_bridge(install: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let root = state_root()?;
    fs::create_dir_all(&root).map_err(|_| "Cannot create the Codex status folder.")?;
    let script = root.join("bloom-codex-hook.ps1");
    fs::write(&script, HOOK_SCRIPT).map_err(|_| "Cannot save the Codex status hook.")?;
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg(if install { "-Install" } else { "-Uninstall" })
        .creation_flags(0x08000000)
        .output()
        .map_err(|_| "Cannot configure Codex hooks. Check that PowerShell is available.")?;
    if !output.status.success() {
        return Err("Cannot update Codex hooks. Check hooks.json is valid and writable; existing hooks were preserved.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn configure_codex_bridge(install: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || configure_bridge(install))
        .await
        .map_err(|_| "Cannot configure the Codex bridge.".to_string())?
}

#[tauri::command]
pub fn open_codex_chat(app: tauri::AppHandle, id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !valid_id(&id) {
        return Err("Invalid Codex chat.".into());
    }
    app.opener()
        .open_url(format!("codex://threads/{id}"), None::<&str>)
        .map_err(|_| "Cannot open this chat. Check that the Codex desktop app is installed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stale_approval_is_not_presented_as_live() {
        let row = json!({"schema":1,"session_id":"chat-123","status":"awaiting_approval","updated_at":1000,"cwd":"C:\\repo\\bloom","detail":"Approval requested"});
        let session = parse_session(&row, &HashMap::new(), 901_001).unwrap();
        assert_eq!(session.status, "unknown");
        assert_eq!(session.project, "bloom");
        assert!(!session.detail.contains("Approval"));
    }

    #[test]
    fn validates_sessions_and_uses_index_title() {
        let mut row =
            json!({"schema":1,"session_id":"chat-123","status":"running","updated_at":1000});
        let titles = HashMap::from([("chat-123".into(), "Fix the island".into())]);
        assert_eq!(
            parse_session(&row, &titles, 2000).unwrap().title,
            "Fix the island"
        );
        row["session_id"] = json!("../escape");
        assert!(parse_session(&row, &titles, 2000).is_none());
        row["session_id"] = json!("chat-123");
        row["status"] = json!("closed");
        assert!(parse_session(&row, &titles, 2000).is_none());
        row["status"] = json!("running");
        assert!(parse_session(&row, &titles, 8 * 24 * 60 * 60 * 1000).is_none());
    }
}
