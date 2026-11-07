//! Local, opt-in agent notifications. The bridge accepts data only, never commands.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};

const MAX_BODY: usize = 4096;
const MAX_HEADERS: usize = 4096;
const HOOK_SCRIPT: &str = include_str!("../../integrations/agent-hook.ps1");
const OPENCODE_PLUGIN: &str = include_str!("../../integrations/opencode-bloom.js");
const OPENCODE_PLUGIN_V1: &str = include_str!("../../integrations/legacy/opencode-bloom-v1.js");

const OPENCODE_PLUGIN_V2: &str = include_str!("../../integrations/opencode-bloom-v2.js");

fn same_plugin(left: &str, right: &str) -> bool {
    left.replace("\r\n", "\n") == right.replace("\r\n", "\n")
}

fn render_opencode_plugin(app: &tauri::AppHandle, source: &str) -> Result<String, String> {
    let path = bridge_dir(app)?.join("bridge.json");
    Ok(source.replace(
        "\"__BLOOM_BRIDGE_FILE__\"",
        &serde_json::to_string(&path).map_err(|e| e.to_string())?,
    ))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentAlert {
    pub agent: String,
    pub session: String,
    pub project: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
}

impl AgentAlert {
    fn validate(&mut self) -> Result<(), String> {
        if !["claude", "codex", "opencode"].contains(&self.agent.as_str())
            || ![
                "working",
                "ready",
                "attention",
                "error",
                "resolved",
                "stopped",
            ]
            .contains(&self.kind.as_str())
            || self.activity.as_deref().is_some_and(|value| {
                ![
                    "thinking", "reading", "editing", "running", "working", "checking",
                ]
                .contains(&value)
            })
            || self.session.is_empty()
            || self.session.len() > 256
        {
            return Err("Invalid agent, event or session".into());
        }
        self.project = self
            .project
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        if self.project.is_empty() {
            self.project = "Agent session".into();
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct AgentBridge {
    error: Mutex<Option<String>>,
    // Serializes read/merge/write so two Settings requests cannot lose hooks.
    config_lock: Mutex<()>,
    verified_agents: Mutex<HashMap<String, u64>>,
}

fn bridge_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|p| p.join("agents"))
        .map_err(|e| e.to_string())
}

fn agent_home(agent: &str) -> Result<PathBuf, String> {
    if !["claude", "codex", "opencode"].contains(&agent) {
        return Err("Unknown agent".into());
    }
    let home = std::env::var_os("USERPROFILE").ok_or("USERPROFILE is unavailable")?;
    Ok(match agent {
        "codex" => std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".codex")),
        "claude" => std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".claude")),
        _ => std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home).join(".config"))
            .join("opencode"),
    })
}

fn config_path(agent: &str) -> Result<PathBuf, String> {
    Ok(agent_home(agent)?.join(match agent {
        "claude" => "settings.json",
        "codex" => "hooks.json",
        _ => "plugins/bloom-notifications.js",
    }))
}

fn opencode_v2_path(v1_path: &Path) -> PathBuf {
    v1_path.with_extension("").join("index.js")
}

fn configure_opencode(app: &tauri::AppHandle, v1_path: &Path, install: bool) -> Result<(), String> {
    let files = [
        (
            v1_path.to_path_buf(),
            render_opencode_plugin(app, OPENCODE_PLUGIN)?,
        ),
        (
            opencode_v2_path(v1_path),
            render_opencode_plugin(app, OPENCODE_PLUGIN_V2)?,
        ),
    ];
    let legacy = render_opencode_plugin(app, OPENCODE_PLUGIN_V1)?;
    let mut originals = Vec::new();
    for (index, (path, source)) in files.iter().enumerate() {
        match std::fs::read_to_string(path) {
            Ok(old) if same_plugin(&old, source) || (index == 0 && same_plugin(&old, &legacy)) => {
                originals.push(Some(old));
            }
            Ok(_) => return Err(format!("The Bloom plugin at {} was modified. Move it aside before changing this integration.", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => originals.push(None),
            Err(error) => return Err(error.to_string()),
        }
    }

    for (index, (path, source)) in files.iter().enumerate() {
        let result = if install {
            save(path, source.as_bytes())
        } else if originals[index].is_some() {
            std::fs::remove_file(path).map_err(|e| e.to_string())
        } else {
            Ok(())
        };
        if let Err(error) = result {
            if index == 1 {
                let rollback = match &originals[0] {
                    Some(original) => save(v1_path, original.as_bytes()),
                    None if install => std::fs::remove_file(v1_path).map_err(|e| e.to_string()),
                    None => Ok(()),
                };
                if let Err(rollback_error) = rollback {
                    return Err(format!(
                        "{error}. Could not restore the OpenCode 1 plugin: {rollback_error}"
                    ));
                }
            }
            return Err(error);
        }
    }
    if !install {
        if let Some(dir) = files[1].0.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    Ok(())
}

fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().ok_or("Invalid path")?).map_err(|e| e.to_string())?;
    crate::utils::atomic_write_file(path, bytes)
}

fn save_verified_agents(app: &tauri::AppHandle, agents: &HashMap<String, u64>) {
    let result = bridge_dir(app).and_then(|dir| {
        let bytes = serde_json::to_vec(agents).map_err(|e| e.to_string())?;
        save(&dir.join("verified-agents.json"), &bytes)
    });
    if let Err(error) = result {
        eprintln!("Agent verification could not be saved: {error}");
    }
}

fn clear_agent_verification(app: &tauri::AppHandle, state: &AgentBridge, agent: &str) {
    if let Ok(mut agents) = state.verified_agents.lock() {
        if agents.remove(agent).is_some() {
            save_verified_agents(app, &agents);
        }
    }
}

// Only remove exact commands generated by Bloom, preserving other handlers in a group.
fn merge_hooks(
    config: &mut Value,
    command: &str,
    agent: &str,
    install: bool,
) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or("Agent configuration must be a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("hooks must be an object")?;
    for groups in hooks.values_mut() {
        let groups = groups.as_array_mut().ok_or("Hook event must be an array")?;
        for group in groups.iter_mut() {
            let handlers = group
                .get_mut("hooks")
                .and_then(Value::as_array_mut)
                .ok_or("Invalid hook group")?;
            handlers.retain(|h| h.get("command").and_then(Value::as_str) != Some(command));
        }
        groups.retain(|g| g["hooks"].as_array().is_some_and(|h| !h.is_empty()));
    }
    hooks.retain(|_, groups| groups.as_array().is_some_and(|g| !g.is_empty()));
    if install {
        for event in hook_events(agent) {
            let mut group =
                json!({"hooks": [{"type": "command", "command": command, "timeout": 3}]});
            if *event == "Notification" {
                group["matcher"] = json!("permission_prompt|elicitation_dialog");
            }
            hooks
                .entry(*event)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or("Invalid hook event")?
                .push(group);
        }
    }
    Ok(())
}

fn hook_events(agent: &str) -> &'static [&'static str] {
    if agent == "claude" {
        &[
            "Stop",
            "PermissionRequest",
            "Notification",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "StopFailure",
            "SessionEnd",
        ]
    } else {
        &[
            "Stop",
            "PermissionRequest",
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "Interrupt",
            "SessionEnd",
        ]
    }
}

fn has_hook(config: &Value, event: &str, command: &str) -> bool {
    config["hooks"][event].as_array().is_some_and(|groups| {
        groups.iter().any(|g| {
            g["hooks"]
                .as_array()
                .is_some_and(|hs| hs.iter().any(|h| h["command"].as_str() == Some(command)))
        })
    })
}

fn hook_command(script: &Path, agent: &str) -> Result<String, String> {
    let path = script.to_str().ok_or("Script path is not UTF-8")?;
    // This command is interpreted by agent shells. Reject shell metacharacters in
    // unusual profile paths rather than constructing an unsafe hook command.
    if path
        .chars()
        .any(|c| matches!(c, '"' | '$' | '`' | '%' | '\n' | '\r'))
    {
        return Err("Your profile path contains characters unsupported by agent hooks".into());
    }
    Ok(format!("powershell.exe -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File \"{path}\" -Agent {agent}"))
}

#[derive(Serialize)]
pub struct IntegrationStatus {
    agent: String,
    installed: bool,
    needs_update: bool,
    verified_at: Option<u64>,
    path: String,
}

#[derive(Serialize)]
pub struct BridgeStatus {
    error: Option<String>,
    integrations: Vec<IntegrationStatus>,
}

#[tauri::command]
pub fn agent_alert_status(
    app: tauri::AppHandle,
    state: State<'_, AgentBridge>,
) -> Result<BridgeStatus, String> {
    let mut integrations = Vec::new();
    let verified_agents = state.verified_agents.lock().map_err(|e| e.to_string())?;
    for agent in ["claude", "codex", "opencode"] {
        let path = config_path(agent)?;
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let (installed, needs_update) = if agent == "opencode" {
            let current_v1 = same_plugin(&content, &render_opencode_plugin(&app, OPENCODE_PLUGIN)?);
            let legacy = same_plugin(&content, &render_opencode_plugin(&app, OPENCODE_PLUGIN_V1)?);
            let v2_content = std::fs::read_to_string(opencode_v2_path(&path)).unwrap_or_default();
            let current_v2 = same_plugin(
                &v2_content,
                &render_opencode_plugin(&app, OPENCODE_PLUGIN_V2)?,
            );
            let installed = current_v1 || legacy || current_v2;
            (installed, installed && !(current_v1 && current_v2))
        } else {
            let command = hook_command(&bridge_dir(&app)?.join("agent-hook.ps1"), agent)?;
            let config = serde_json::from_str::<Value>(&content).unwrap_or(Value::Null);
            let installed = has_hook(&config, "Stop", &command);
            let complete = hook_events(agent)
                .iter()
                .all(|event| has_hook(&config, event, &command));
            (installed, installed && !complete)
        };
        integrations.push(IntegrationStatus {
            agent: agent.into(),
            installed,
            needs_update,
            verified_at: verified_agents.get(agent).copied(),
            path: path.display().to_string(),
        });
    }
    Ok(BridgeStatus {
        error: state.error.lock().map_err(|e| e.to_string())?.clone(),
        integrations,
    })
}

#[tauri::command]
pub async fn configure_agent_alert(
    app: tauri::AppHandle,
    state: State<'_, AgentBridge>,
    agent: String,
    install: bool,
) -> Result<(), String> {
    let _guard = state.config_lock.lock().map_err(|e| e.to_string())?;
    let path = config_path(&agent)?;
    if agent == "opencode" {
        configure_opencode(&app, &path, install)?;
        clear_agent_verification(&app, &state, &agent);
        return Ok(());
    }
    let command = hook_command(&bridge_dir(&app)?.join("agent-hook.ps1"), &agent)?;
    let original = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if !install && original.is_none() {
        return Ok(());
    }
    let mut config = match &original {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?,
        None => json!({}),
    };
    merge_hooks(&mut config, &command, &agent, install)?;
    if let Some(bytes) = original {
        // One pre-install backup. Never replace an existing backup.
        let backup = path.with_extension("json.bloom-backup");
        if !backup.exists() {
            save(&backup, &bytes)?;
        }
    }
    save(
        &path,
        &serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )?;
    clear_agent_verification(&app, &state, &agent);
    Ok(())
}

#[tauri::command]
pub fn test_agent_alert(
    app: tauri::AppHandle,
    agent: String,
    kind: String,
    activity: Option<String>,
) -> Result<(), String> {
    let mut alert = AgentAlert {
        agent,
        kind,
        session: "bloom-preview".into(),
        project: "Bloom preview".into(),
        activity,
    };
    alert.validate()?;
    app.emit_to("main", "agent-alert", alert)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_alerts_quiet() -> bool {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_ACCEPTS_NOTIFICATIONS};
    // Includes presentation/fullscreen/quiet states exposed by Windows.
    unsafe { SHQueryUserNotificationState().is_ok_and(|s| s != QUNS_ACCEPTS_NOTIFICATIONS) }
}

/// Minimal bounded HTTP/1 receiver: no chunking, persistence, CORS or browser origins.
fn read_request(stream: &mut impl Read, token: &str) -> Result<AgentAlert, &'static str> {
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        if headers.len() >= MAX_HEADERS {
            return Err("431 Request Header Fields Too Large");
        }
        let mut byte = [0];
        stream
            .read_exact(&mut byte)
            .map_err(|_| "400 Bad Request")?;
        headers.push(byte[0]);
    }
    let headers = std::str::from_utf8(&headers).map_err(|_| "400 Bad Request")?;
    let mut lines = headers.split("\r\n");
    if lines.next() != Some("POST /agent-alert HTTP/1.1") {
        return Err("404 Not Found");
    }
    let mut length = None;
    let mut authenticated = false;
    let mut json_type = false;
    for line in lines.filter(|l| !l.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("400 Bad Request")?;
        let value = value.trim();
        match key.to_ascii_lowercase().as_str() {
            "content-length" => {
                if length.is_some() {
                    return Err("400 Bad Request");
                }
                length = Some(value.parse::<usize>().map_err(|_| "400 Bad Request")?);
            }
            "authorization" => authenticated = value.strip_prefix("Bearer ") == Some(token),
            "content-type" => json_type = value.split(';').next() == Some("application/json"),
            "origin" | "transfer-encoding" => return Err("403 Forbidden"),
            _ => {}
        }
    }
    if !authenticated {
        return Err("401 Unauthorized");
    }
    if !json_type {
        return Err("415 Unsupported Media Type");
    }
    let length = length.ok_or("411 Length Required")?;
    if length > MAX_BODY {
        return Err("413 Content Too Large");
    }
    let mut body = vec![0; length];
    stream
        .read_exact(&mut body)
        .map_err(|_| "400 Bad Request")?;
    let mut alert: AgentAlert = serde_json::from_slice(&body).map_err(|_| "400 Bad Request")?;
    alert.validate().map_err(|_| "400 Bad Request")?;
    Ok(alert)
}

fn receive(
    app: &tauri::AppHandle,
    stream: &mut TcpStream,
    token: &str,
    recent: &mut HashMap<String, Instant>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let status = match read_request(
        &mut DeadlineReader {
            stream,
            deadline: Instant::now() + Duration::from_secs(2),
        },
        token,
    ) {
        Ok(alert) => {
            if let Ok(mut agents) = app.state::<AgentBridge>().verified_agents.lock() {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    agents.entry(alert.agent.clone())
                {
                    entry.insert(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    );
                    save_verified_agents(app, &agents);
                }
            }
            let key = format!(
                "{}:{}:{}:{}",
                alert.agent,
                alert.session,
                alert.kind,
                alert.activity.as_deref().unwrap_or_default()
            );
            recent.retain(|_, time| time.elapsed() < Duration::from_secs(2));
            let enabled = crate::utils::get_setting_str(app, "bloom-agent-alerts-enabled")
                .as_deref()
                != Some("false");
            if enabled && !recent.contains_key(&key) {
                recent.insert(key, Instant::now());
                let _ = app.emit_to("main", "agent-alert", alert);
            }
            "204 No Content"
        }
        Err(status) => status,
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
}

struct DeadlineReader<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}
impl Read for DeadlineReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "Agent request timed out")
            })?;
        self.stream
            .set_read_timeout(Some(remaining.min(Duration::from_millis(500))))?;
        self.stream.read(buffer)
    }
}

pub fn setup(app: &tauri::AppHandle) {
    let verified_agents = bridge_dir(app)
        .ok()
        .and_then(|dir| std::fs::read(dir.join("verified-agents.json")).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    app.manage(AgentBridge {
        verified_agents: Mutex::new(verified_agents),
        ..AgentBridge::default()
    });
    let start = || -> Result<(TcpListener, String), String> {
        let dir = bridge_dir(app)?;
        let token = format!(
            "{:?}",
            unsafe { windows::Win32::System::Com::CoCreateGuid() }.map_err(|e| e.to_string())?
        );
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        save(&dir.join("agent-hook.ps1"), HOOK_SCRIPT.as_bytes())?;
        save(
            &dir.join("bridge.json"),
            &serde_json::to_vec(&json!({"port": port, "token": token}))
                .map_err(|e| e.to_string())?,
        )?;
        Ok((listener, token))
    };
    match start() {
        Ok((listener, token)) => {
            let app = app.clone();
            std::thread::spawn(move || {
                let mut recent = HashMap::new();
                for mut stream in listener.incoming().flatten() {
                    receive(&app, &mut stream, &token, &mut recent);
                }
            });
        }
        Err(error) => {
            eprintln!("Agent notification bridge: {error}");
            if let Ok(mut state) = app.state::<AgentBridge>().error.lock() {
                *state = Some(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(extra: &str, body: &str) -> Vec<u8> {
        format!("POST /agent-alert HTTP/1.1\r\nAuthorization: Bearer test-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}\r\n{body}", body.len()).into_bytes()
    }
    const BODY: &str = r#"{"agent":"claude","session":"s1","project":"bloom","kind":"ready"}"#;
    #[test]
    fn receiver_requires_auth_and_rejects_browser_requests() {
        assert!(read_request(&mut &request("", BODY)[..], "test-token").is_ok());
        assert_eq!(
            read_request(&mut &request("", BODY)[..], "wrong").unwrap_err(),
            "401 Unauthorized"
        );
        assert_eq!(
            read_request(
                &mut &request("Origin: http://evil.test\r\n", BODY)[..],
                "test-token"
            )
            .unwrap_err(),
            "403 Forbidden"
        );
        assert!(read_request(
            &mut &request("Transfer-Encoding: chunked\r\n", BODY)[..],
            "test-token"
        )
        .is_err());
    }
    #[test]
    fn receiver_bounds_input_and_validates_events() {
        let working = BODY.replace("\"ready\"", "\"working\",\"activity\":\"editing\"");
        assert_eq!(
            read_request(&mut &request("", &working)[..], "test-token")
                .unwrap()
                .activity
                .as_deref(),
            Some("editing")
        );
        assert!(read_request(
            &mut &request("", &working.replace("editing", "execute-arbitrary"))[..],
            "test-token"
        )
        .is_err());
        assert!(read_request(
            &mut &request("Content-Length: 1\r\n", BODY)[..],
            "test-token"
        )
        .is_err());
        assert!(read_request(
            &mut &request("", &"x".repeat(MAX_BODY + 1))[..],
            "test-token"
        )
        .is_err());
        assert!(read_request(
            &mut &request("", &BODY.replace("claude", "unknown"))[..],
            "test-token"
        )
        .is_err());
        assert!(read_request(
            &mut &request("", &BODY.replace("ready", "execute"))[..],
            "test-token"
        )
        .is_err());
        let mut truncated = request("", BODY);
        truncated.pop();
        assert!(read_request(&mut &truncated[..], "test-token").is_err());
    }
    #[test]
    fn hook_install_is_idempotent_and_removal_preserves_user_hooks() {
        let mut config = json!({"permissions": {"allow": ["Read"]}, "hooks": {"Stop": [{"hooks": [{"type": "command", "command": "my-hook"}]}]}});
        let original = config.clone();
        merge_hooks(&mut config, "bloom-hook", "claude", true).unwrap();
        let installed = config.clone();
        merge_hooks(&mut config, "bloom-hook", "claude", true).unwrap();
        assert_eq!(config, installed);
        merge_hooks(&mut config, "bloom-hook", "claude", false).unwrap();
        assert_eq!(config, original);
    }
    #[test]
    fn malformed_config_is_rejected() {
        assert!(merge_hooks(&mut json!({"hooks": []}), "bloom", "codex", true).is_err());
        assert!(merge_hooks(
            &mut json!({"hooks": {"Stop": "bad"}}),
            "bloom",
            "codex",
            true
        )
        .is_err());
    }

    #[test]
    fn live_hooks_cover_work_and_stop_and_preserve_existing_handlers() {
        for agent in ["claude", "codex"] {
            let mut config = json!({"hooks": {"Stop": [{"hooks": [{"type":"command", "command":"bloom"}, {"type":"command", "command":"my-hook"}]}]}});
            merge_hooks(&mut config, "bloom", agent, true).unwrap();
            assert!(hook_events(agent)
                .iter()
                .all(|event| has_hook(&config, event, "bloom")));
            assert!(has_hook(&config, "Stop", "my-hook"));
            assert!(has_hook(&config, "PreToolUse", "bloom"));
            assert!(has_hook(&config, "SessionEnd", "bloom"));
            merge_hooks(&mut config, "bloom", agent, false).unwrap();
            assert!(hook_events(agent)
                .iter()
                .all(|event| !has_hook(&config, event, "bloom")));
            assert!(has_hook(&config, "Stop", "my-hook"));
        }
        assert!(same_plugin("known\r\nplugin\r\n", "known\nplugin\n"));
        assert!(!same_plugin("known plugin", "modified plugin"));
    }

    #[test]
    fn powershell_adapter_sends_authenticated_metadata_and_returns_neutral_json() {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        let id = unsafe { windows::Win32::System::Com::CoCreateGuid() }.unwrap();
        let dir = std::env::temp_dir().join(format!("bloom-agent-hook-{id:?}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("agent-hook.ps1"),
            HOOK_SCRIPT.replace(
                "# Bloom closed or unavailable: the agent should continue normally.",
                "[Console]::Error.WriteLine($_.Exception.ToString())",
            ),
        )
        .unwrap();
        for (agent, event, kind) in [
            ("claude", "Stop", "ready"),
            ("codex", "PermissionRequest", "working"),
            ("claude", "PermissionRequest", "attention"),
            ("claude", "UserPromptSubmit", "working"),
            ("codex", "PreToolUse", "working"),
            ("codex", "Interrupt", "stopped"),
        ] {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            std::fs::write(
                dir.join("bridge.json"),
                serde_json::to_vec(&json!({
                    "port": listener.local_addr().unwrap().port(), "token": "test-token"
                }))
                .unwrap(),
            )
            .unwrap();
            let server = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) => panic!("Hook did not connect: {e}"),
                    }
                };
                // Accepted Winsock sockets inherit the listener's nonblocking mode.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                struct RecordingReader<'a> {
                    stream: &'a mut TcpStream,
                    bytes: Vec<u8>,
                }
                impl Read for RecordingReader<'_> {
                    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                        let count = self.stream.read(buffer)?;
                        self.bytes.extend_from_slice(&buffer[..count]);
                        Ok(count)
                    }
                }
                let mut reader = RecordingReader {
                    stream: &mut stream,
                    bytes: Vec::new(),
                };
                let payload = read_request(&mut reader, "test-token").unwrap_or_else(|error| {
                    panic!("{error}: {}", String::from_utf8_lossy(&reader.bytes))
                });
                stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                payload
            });
            let mut child = Command::new("powershell.exe")
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(dir.join("agent-hook.ps1"))
                .args(["-Agent", agent])
                .creation_flags(0x08000000)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(&serde_json::to_vec(&json!({
                "hook_event_name": event, "session_id": "session-test", "cwd": "C:\\work\\bloom", "tool_name": "apply_patch",
                "last_assistant_message": "private response must never be sent"
            })).unwrap()).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout), "{}");
            assert!(
                output.stderr.is_empty(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let payload = server.join().unwrap();
            assert_eq!(payload.agent, agent);
            assert_eq!(payload.kind, kind);
            assert_eq!(payload.project, "bloom");
            assert_eq!(payload.session, "session-test");
            if event == "PermissionRequest" && agent == "codex" {
                assert_eq!(payload.activity.as_deref(), Some("working"));
            }
            if event == "PreToolUse" {
                assert_eq!(payload.activity.as_deref(), Some("editing"));
            }
            if event == "UserPromptSubmit" {
                assert_eq!(payload.activity.as_deref(), Some("thinking"));
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
