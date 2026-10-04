//! MCP client (stdio). `mcp.json` in the data dir lists servers in Claude
//! Desktop's format; their tools show up as `mcp_<server>_<tool>`. Servers
//! start with the first request (or Reload), stay up while the sidecar lives
//! and die with it (kill-on-close job). Between requests nothing of ours runs:
//! each server's reader task only wakes on that server's output.
//! Env values from mcp.json often hold API keys: they go to the child only,
//! never to debug.log, the journal or Settings.

use crate::agent::Ctx;
use crate::protocol::{emit, ConfirmKind, Out};
use crate::{debug, journal, policy, tools};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

const PROTOCOL: &str = "2025-06-18";
const START_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_TOOLS: usize = 64;
const MAX_NAME: usize = 64;
const MAX_DESC: usize = 1000;
const BODY_CHARS: usize = 1500;
const JOURNAL_ARGS: usize = 200;
const MAX_OUTPUT: usize = 8000;

pub fn path(data: &Path) -> PathBuf {
    data.join("mcp.json")
}

/// Makes sure the config exists, for Reveal.
pub fn ensure(data: &Path) -> Result<PathBuf, String> {
    let p = path(data);
    if !p.exists() {
        std::fs::write(&p, "{\n  \"mcpServers\": {}\n}\n").map_err(|e| e.to_string())?;
    }
    Ok(p)
}

/// One `mcpServers` entry. No Debug: `env` holds secrets.
#[derive(Deserialize, Clone)]
pub struct ServerCfg {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub disabled: bool,
}

/// Enabled servers by name, and one message per unusable entry.
pub fn load(data: &Path) -> (Vec<(String, ServerCfg)>, Vec<String>) {
    let Ok(text) = std::fs::read_to_string(path(data)) else {
        return (vec![], vec![]);
    };
    let file: Value = match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(e) => return (vec![], vec![format!("mcp.json: {e}")]),
    };
    let (mut servers, mut errors) = (vec![], vec![]);
    for (name, entry) in file["mcpServers"].as_object().into_iter().flatten() {
        match serde_json::from_value::<ServerCfg>(entry.clone()) {
            Ok(cfg) if !cfg.disabled => servers.push((name.clone(), cfg)),
            Ok(_) => {}
            // serde's message could quote an env value: say what's expected instead.
            Err(_) => errors.push(format!(
                "{name}: needs a \"command\" (only local stdio servers work); \"args\" and \"env\" must hold strings"
            )),
        }
    }
    (servers, errors)
}

/// `mcp_<server>_<tool>`, anything outside `[A-Za-z0-9_-]` as `_`, max 64.
pub fn tool_name(server: &str, tool: &str) -> String {
    format!("mcp_{server}_{tool}")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(MAX_NAME)
        .collect()
}

type Reply = Result<Value, String>;
/// None once the server's output has ended: new requests fail at once.
type Pending = Arc<Mutex<Option<HashMap<u64, oneshot::Sender<Reply>>>>>;
type Writer = Arc<tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>>;

const STOPPED: &str = "The MCP server stopped.";

/// JSON-RPC 2.0 over newline-delimited JSON. A reader task routes answers to
/// waiting callers by id.
pub struct Client {
    writer: Writer,
    pending: Pending,
    next: AtomicU64,
}

async fn write_line(writer: &Writer, msg: &Value) -> Result<(), String> {
    let mut w = writer.lock().await;
    w.write_all(format!("{msg}\n").as_bytes())
        .await
        .map_err(|_| STOPPED.to_string())?;
    w.flush().await.map_err(|_| STOPPED.to_string())
}

async fn read_loop(read: impl AsyncRead + Unpin, writer: Writer, pending: Pending) {
    let mut read = BufReader::new(read);
    let mut line = Vec::new();
    while matches!(read.read_until(b'\n', &mut line).await, Ok(n) if n > 0) {
        let msg: Value = serde_json::from_slice(&line).unwrap_or_default();
        line.clear();
        let id = &msg["id"];
        if let Some(method) = msg["method"].as_str() {
            // A request from the server. We offer no capabilities, so only
            // ping gets a real answer. Its own task: the reader never waits.
            if !id.is_null() {
                let reply = if method == "ping" {
                    json!({ "jsonrpc": "2.0", "id": id, "result": {} })
                } else {
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "Method not found" } })
                };
                let w = writer.clone();
                tokio::spawn(async move { write_line(&w, &reply).await });
            }
            continue;
        }
        let Some(id) = id.as_u64() else { continue };
        let waiter = pending.lock().unwrap().as_mut().and_then(|p| p.remove(&id));
        if let Some(tx) = waiter {
            let reply = match msg.get("error") {
                Some(e) => Err(e["message"].as_str().unwrap_or("MCP error").to_string()),
                None => Ok(msg["result"].clone()),
            };
            let _ = tx.send(reply);
        }
    }
    // Dropping the senders wakes every waiter with "stopped".
    pending.lock().unwrap().take();
}

impl Client {
    pub fn new(
        read: impl AsyncRead + Send + Unpin + 'static,
        write: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Client {
        let writer: Writer = Arc::new(tokio::sync::Mutex::new(Box::new(write)));
        let pending: Pending = Arc::new(Mutex::new(Some(HashMap::new())));
        tokio::spawn(read_loop(read, writer.clone(), pending.clone()));
        Client {
            writer,
            pending,
            next: AtomicU64::new(0),
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let mut msg = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(params) = params {
            msg["params"] = params;
        }
        write_line(&self.writer, &msg).await
    }

    pub async fn request(&self, method: &str, params: Value, limit: Duration) -> Reply {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap()
            .as_mut()
            .ok_or(STOPPED)?
            .insert(id, tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        write_line(&self.writer, &msg).await?;
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(STOPPED.into()),
            Err(_) => {
                if let Some(p) = self.pending.lock().unwrap().as_mut() {
                    p.remove(&id);
                }
                let cancel = json!({ "requestId": id, "reason": "timeout" });
                let _ = self.notify("notifications/cancelled", Some(cancel)).await;
                Err(format!(
                    "The MCP server didn't answer within {} s.",
                    limit.as_secs()
                ))
            }
        }
    }
}

pub struct Tool {
    /// The name the model sees (`tool_name`); set by `Mcp::new`.
    pub exposed: String,
    pub name: String,
    pub description: String,
    pub schema: Value,
}

pub struct Server {
    pub name: String,
    pub trusted: bool,
    pub tools: Vec<Tool>,
    client: Client,
    // Dropped after the client: the child dies (kill_on_drop), then the job
    // takes its children with it.
    _child: Option<tokio::process::Child>,
    #[cfg(windows)]
    _job: Option<crate::powershell::job::KillOnClose>,
}

impl Server {
    /// initialize, initialized, then tools/list until there's no nextCursor.
    pub async fn connect(name: &str, trusted: bool, client: Client) -> Result<Server, String> {
        let init = json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": { "name": "bloom-ai", "version": env!("CARGO_PKG_VERSION") }
        });
        client.request("initialize", init, START_TIMEOUT).await?;
        client.notify("notifications/initialized", None).await?;
        let mut tools = vec![];
        let mut cursor: Option<String> = None;
        loop {
            let params = cursor.map_or(json!({}), |c| json!({ "cursor": c }));
            let page = client.request("tools/list", params, START_TIMEOUT).await?;
            for t in page["tools"].as_array().into_iter().flatten() {
                let Some(tool) = t["name"].as_str() else {
                    continue;
                };
                tools.push(Tool {
                    exposed: String::new(),
                    name: tool.into(),
                    description: t["description"].as_str().unwrap_or_default().into(),
                    schema: t["inputSchema"].clone(),
                });
            }
            cursor = page["nextCursor"].as_str().map(String::from);
            // More than the cap is never used; the start timeout bounds the rest.
            if cursor.is_none() || tools.len() > MAX_TOOLS {
                break;
            }
        }
        Ok(Server {
            name: name.into(),
            trusted,
            tools,
            client,
            _child: None,
            #[cfg(windows)]
            _job: None,
        })
    }

    pub async fn call(&self, tool: &str, args: &Value, limit: Duration) -> Result<String, String> {
        let args = if args.is_object() {
            args.clone()
        } else {
            json!({})
        };
        let params = json!({ "name": tool, "arguments": args });
        let result = self.client.request("tools/call", params, limit).await?;
        let text = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| match c["type"].as_str() {
                Some("text") => c["text"].as_str().unwrap_or_default().to_string(),
                Some(kind) => format!("[{kind}]"),
                None => "[content]".into(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let text = if text.trim().is_empty() {
            "(no output)".into()
        } else {
            crate::powershell::clip(&text, MAX_OUTPUT)
        };
        if result["isError"] == true {
            Err(text)
        } else {
            Ok(text)
        }
    }
}

fn command(program: &str, cfg: &ServerCfg) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(&cfg.args)
        .envs(&cfg.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Discarded: a chatty server can never fill a pipe and block.
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // Not the data dir: a server left running must never block "Delete AI".
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

/// Starts one server process and connects to it.
pub async fn spawn(name: &str, cfg: &ServerCfg) -> Result<Server, String> {
    // ponytail: on the .cmd fallback the child is spawned before it joins the kill-on-close
    // job, so a crash in that gap can orphan it; use CREATE_SUSPENDED if it ever matters.
    let mut child = match command(&cfg.command, cfg).spawn() {
        // `npx` and friends are .cmd scripts that CreateProcess won't find by bare name.
        Err(e)
            if e.kind() == std::io::ErrorKind::NotFound
                && Path::new(&cfg.command).extension().is_none() =>
        {
            command(&format!("{}.cmd", cfg.command), cfg).spawn()
        }
        other => other,
    }
    .map_err(|e| format!("couldn't start {}: {e}", cfg.command))?;
    #[cfg(windows)]
    let job = crate::powershell::job::KillOnClose::with(&child);
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err("no pipes".into());
    };
    let mut server = Server::connect(name, cfg.trusted, Client::new(stdout, stdin)).await?;
    server._child = Some(child);
    #[cfg(windows)]
    {
        server._job = job;
    }
    Ok(server)
}

/// The started servers and what went wrong starting the rest.
pub struct Mcp {
    pub servers: Vec<Server>,
    pub errors: Vec<String>,
}

impl Mcp {
    /// Names the tools, dropping duplicates and anything past the cap.
    pub fn new(mut servers: Vec<Server>, mut errors: Vec<String>) -> Mcp {
        let (mut seen, mut left_out) = (HashSet::new(), 0);
        for s in &mut servers {
            let server = &s.name;
            s.tools.retain_mut(|t| {
                t.exposed = tool_name(server, &t.name);
                if seen.len() >= MAX_TOOLS {
                    left_out += 1;
                    return false;
                }
                seen.insert(t.exposed.clone())
            });
        }
        if left_out > 0 {
            errors.push(format!(
                "{left_out} tools left out: only the first {MAX_TOOLS} are used"
            ));
        }
        Mcp { servers, errors }
    }

    /// Starts every enabled server at once, each within 15 s. Dropping the
    /// future (Stop) kills the ones still starting.
    pub async fn start(data: &Path, task: Option<u64>) -> Mcp {
        let (cfgs, mut errors) = load(data);
        if let (Some(task), false) = (task, cfgs.is_empty()) {
            emit(&Out::Activity {
                task,
                text: "Starting MCP servers".into(),
            });
        }
        let mut set = tokio::task::JoinSet::new();
        for (i, (name, cfg)) in cfgs.into_iter().enumerate() {
            set.spawn(async move {
                let started = tokio::time::timeout(START_TIMEOUT, spawn(&name, &cfg)).await;
                (i, name, started)
            });
        }
        let mut started = vec![];
        while let Some(done) = set.join_next().await {
            let Ok((i, name, result)) = done else {
                continue;
            };
            match result {
                Ok(Ok(server)) => started.push((i, server)),
                Ok(Err(e)) => errors.push(format!("{name}: {e}")),
                Err(_) => errors.push(format!("{name}: no answer within 15 s")),
            }
        }
        started.sort_by_key(|(i, _)| *i);
        Mcp::new(started.into_iter().map(|(_, s)| s).collect(), errors)
    }

    pub fn find(&self, exposed: &str) -> Option<(&Server, &Tool)> {
        self.servers
            .iter()
            .find_map(|s| Some((s, s.tools.iter().find(|t| t.exposed == exposed)?)))
    }

    /// Function entries for the model, appended to `tools::schema()`.
    pub fn schema(&self) -> Vec<Value> {
        let mut out = vec![];
        for s in &self.servers {
            for t in &s.tools {
                let parameters = if t.schema["type"] == "object" {
                    // Some servers send `$schema`, which strict model APIs reject.
                    let mut p = t.schema.clone();
                    p.as_object_mut().map(|o| o.remove("$schema"));
                    p
                } else {
                    json!({ "type": "object", "properties": {} })
                };
                let description =
                    format!("[{} MCP] {}", s.name, debug::cut(&t.description, MAX_DESC));
                out.push(json!({
                    "type": "function",
                    "function": { "name": t.exposed, "description": description, "parameters": parameters }
                }));
            }
        }
        out
    }
}

/// Process-wide: the running set, and a lock so only one start runs at a time.
#[derive(Default)]
pub struct State {
    current: Mutex<Option<Arc<Mcp>>>,
    starting: tokio::sync::Mutex<()>,
}

impl State {
    pub fn get(&self) -> Option<Arc<Mcp>> {
        self.current.lock().unwrap().clone()
    }

    /// Starts the servers if this is the first request since start or Reload.
    /// True when it started servers (or hit errors), so Settings should refresh.
    pub async fn ensure(&self, data: &Path, task: u64) -> bool {
        let _one = self.starting.lock().await;
        if self.get().is_some() {
            return false;
        }
        let mcp = Mcp::start(data, Some(task)).await;
        let started = !mcp.servers.is_empty() || !mcp.errors.is_empty();
        *self.current.lock().unwrap() = Some(Arc::new(mcp));
        started
    }

    /// Kills the running servers and starts them again from mcp.json.
    pub async fn reload(&self, data: &Path) {
        let _one = self.starting.lock().await;
        // Gone before the new ones start (a request mid-call keeps its own).
        self.current.lock().unwrap().take();
        let mcp = Mcp::start(data, None).await;
        *self.current.lock().unwrap() = Some(Arc::new(mcp));
    }

    /// (servers, tools, errors) for Settings.
    pub fn status(&self) -> (usize, usize, Vec<String>) {
        self.get().map_or((0, 0, vec![]), |m| {
            let tools = m.servers.iter().map(|s| s.tools.len()).sum();
            (m.servers.len(), tools, m.errors.clone())
        })
    }

    #[cfg(test)]
    pub fn set(&self, mcp: Mcp) {
        *self.current.lock().unwrap() = Some(Arc::new(mcp));
    }
}

/// An `mcp_` tool call: ask per the tier, journal, call, taint.
pub async fn call(ctx: &mut Ctx, name: &str, args: &Value) -> Result<String, String> {
    let mcp = ctx.shared.mcp.get();
    let Some((server, tool)) = mcp.as_deref().and_then(|m| m.find(name)) else {
        return Err(format!(
            "unknown tool {name} (its MCP server is no longer running)"
        ));
    };
    let dir = ctx.shared.data_dir.clone();
    let detail = format!(
        "{} {} {}",
        server.name,
        tool.name,
        debug::cut(&args.to_string(), JOURNAL_ARGS)
    );
    let ask = policy::mcp_needs_confirm(ctx.cfg.tier, server.trusted, ctx.tainted);
    let title = format!("Use {}: {}?", server.name, tool.name);
    let body = debug::cut(
        &serde_json::to_string_pretty(args).unwrap_or_default(),
        BODY_CHARS,
    );
    if !tools::confirm_persist(ctx, ConfirmKind::Tool, ask, &title, &body).await {
        journal::record(&dir, "mcp", &detail, "declined");
        return Ok("The user chose not to run it.".into());
    }
    let started = if ask {
        "approved-started"
    } else {
        "auto-started"
    };
    journal::record(&dir, "mcp", &detail, started);
    // Whatever comes back, error text included, is outside content.
    ctx.tainted = true;
    let result = server.call(&tool.name, args, CALL_TIMEOUT).await;
    let outcome = match (&result, ask) {
        (Err(_), _) => "failed",
        (Ok(_), true) => "approved",
        (Ok(_), false) => "auto",
    };
    let reason = result.as_ref().err().map(String::as_str);
    journal::record_with(&dir, "mcp", &detail, outcome, reason);
    result
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::bridge::Answer;
    use crate::config::Tier;
    use crate::testutil::{ctx, temp_dir};

    /// The test server's answer to one request: `{"result": ..}` or
    /// `{"error": ..}`; None never answers.
    type Handler = fn(&Value) -> Option<Value>;

    /// A client wired to an in-process fake server. Returns every message
    /// the server received.
    pub fn fake(handler: Handler) -> (Client, Arc<Mutex<Vec<Value>>>) {
        let (ours, theirs) = tokio::io::duplex(64 * 1024);
        let (read, write) = tokio::io::split(ours);
        let seen = Arc::new(Mutex::new(vec![]));
        let log = seen.clone();
        tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(theirs);
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let msg: Value = serde_json::from_str(&line).unwrap();
                log.lock().unwrap().push(msg.clone());
                if msg["id"].is_null() {
                    continue;
                }
                if let Some(mut reply) = handler(&msg) {
                    reply["jsonrpc"] = "2.0".into();
                    reply["id"] = msg["id"].clone();
                    let _ = w.write_all(format!("{reply}\n").as_bytes()).await;
                }
            }
        });
        (Client::new(read, write), seen)
    }

    /// initialize; two pages of tools; echo, fail and hang.
    pub fn standard(msg: &Value) -> Option<Value> {
        let result = match msg["method"].as_str()? {
            "initialize" => {
                json!({ "protocolVersion": PROTOCOL, "capabilities": { "tools": {} }, "serverInfo": { "name": "fake", "version": "1" } })
            }
            "tools/list" if msg["params"]["cursor"] == "p2" => json!({ "tools": [
                { "name": "fail", "inputSchema": { "type": "object" } },
                { "name": "hang" }
            ] }),
            "tools/list" => json!({ "tools": [
                { "name": "echo", "description": "Echoes text", "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } } }
            ], "nextCursor": "p2" }),
            "tools/call" => match msg["params"]["name"].as_str()? {
                "echo" => json!({ "content": [
                    { "type": "text", "text": msg["params"]["arguments"]["text"] },
                    { "type": "image", "data": "AAAA", "mimeType": "image/png" }
                ] }),
                "fail" => {
                    json!({ "content": [{ "type": "text", "text": "boom" }], "isError": true })
                }
                _ => return None,
            },
            _ => return Some(json!({ "error": { "code": -32601, "message": "nope" } })),
        };
        Some(json!({ "result": result }))
    }

    pub async fn fake_server(name: &str, trusted: bool) -> Server {
        Server::connect(name, trusted, fake(standard).0)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn handshake_initializes_and_follows_pagination() {
        let (client, seen) = fake(standard);
        let server = Server::connect("fake", false, client).await.unwrap();
        let names: Vec<&str> = server.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["echo", "fail", "hang"]);
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0]["method"], "initialize");
        assert_eq!(seen[0]["params"]["protocolVersion"], "2025-06-18");
        assert_eq!(seen[0]["params"]["clientInfo"]["name"], "bloom-ai");
        assert_eq!(seen[1]["method"], "notifications/initialized");
        assert!(seen[1].get("id").is_none());
        assert!(seen[2]["params"].get("cursor").is_none());
        assert_eq!(seen[3]["params"]["cursor"], "p2");
    }

    #[tokio::test]
    async fn schema_drops_top_level_dollar_schema() {
        let mut s = fake_server("s", false).await;
        s.tools[0].schema = json!({ "$schema": "http://x", "type": "object", "properties": {} });
        let schema = Mcp::new(vec![s], vec![]).schema();
        assert!(schema[0]["function"]["parameters"].get("$schema").is_none());
    }

    #[tokio::test]
    async fn calls_join_text_summarise_other_content_and_surface_is_error() {
        let server = fake_server("fake", false).await;
        let out = server
            .call("echo", &json!({ "text": "hi" }), CALL_TIMEOUT)
            .await;
        assert_eq!(out, Ok("hi\n[image]".into()));
        let out = server.call("fail", &json!({}), CALL_TIMEOUT).await;
        assert_eq!(out, Err("boom".into()));
    }

    #[tokio::test]
    async fn json_rpc_errors_and_timeouts_come_back_as_err() {
        let (client, seen) = fake(standard);
        assert_eq!(
            client.request("nope", json!({}), CALL_TIMEOUT).await,
            Err("nope".into())
        );
        let hang = json!({ "name": "hang", "arguments": {} });
        let out = client
            .request("tools/call", hang, Duration::from_millis(50))
            .await;
        assert!(out.unwrap_err().contains("didn't answer"));
        tokio::time::sleep(Duration::from_millis(20)).await;
        let last = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last["method"], "notifications/cancelled");
        assert_eq!(last["params"]["requestId"], 2);
    }

    #[tokio::test]
    async fn a_server_that_exits_fails_waiting_and_new_requests() {
        let (ours, theirs) = tokio::io::duplex(1024);
        let (read, write) = tokio::io::split(ours);
        let client = Client::new(read, write);
        let waiting = tokio::spawn(async move {
            let first = client.request("x", json!({}), CALL_TIMEOUT).await;
            (first, client.request("y", json!({}), CALL_TIMEOUT).await)
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(theirs);
        let (first, second) = waiting.await.unwrap();
        assert_eq!(first, Err(STOPPED.into()));
        assert_eq!(second, Err(STOPPED.into()));
    }

    #[tokio::test]
    async fn answers_server_pings() {
        let (ours, theirs) = tokio::io::duplex(1024);
        let (read, write) = tokio::io::split(ours);
        let _client = Client::new(read, write);
        let (r, mut w) = tokio::io::split(theirs);
        w.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n")
            .await
            .unwrap();
        let line = BufReader::new(r)
            .lines()
            .next_line()
            .await
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            (reply["id"].clone(), reply["result"].clone()),
            (json!(9), json!({}))
        );
    }

    #[test]
    fn names_are_sanitised_and_capped() {
        assert_eq!(tool_name("my files", "read.file"), "mcp_my_files_read_file");
        assert_eq!(tool_name("é-x", "a/b"), "mcp__-x_a_b");
        let long = tool_name("server", &"t".repeat(100));
        assert_eq!(long.len(), 64);
        assert!(long.starts_with("mcp_server_ttt"));
    }

    fn many_tools(msg: &Value) -> Option<Value> {
        let result = match msg["method"].as_str()? {
            "initialize" => json!({}),
            _ => {
                let tools: Vec<Value> = (0..70)
                    .map(|i| json!({ "name": format!("t{i}") }))
                    .collect();
                json!({ "tools": tools })
            }
        };
        Some(json!({ "result": result }))
    }

    #[tokio::test]
    async fn sixty_four_tools_at_most_and_duplicates_dropped() {
        let a = Server::connect("a", false, fake(many_tools).0)
            .await
            .unwrap();
        let mut b = fake_server("b", false).await;
        b.tools.push(Tool {
            exposed: String::new(),
            name: "echo".into(),
            description: String::new(),
            schema: Value::Null,
        });
        let mcp = Mcp::new(vec![b, a], vec![]);
        let total: usize = mcp.servers.iter().map(|s| s.tools.len()).sum();
        assert_eq!(total, 64);
        assert_eq!(mcp.servers[0].tools.len(), 3, "duplicate echo kept");
        assert_eq!(mcp.errors, ["9 tools left out: only the first 64 are used"]);
        let schema = mcp.schema();
        assert_eq!(schema.len(), 64);
        assert_eq!(schema[0]["function"]["name"], "mcp_b_echo");
        assert_eq!(schema[0]["function"]["description"], "[b MCP] Echoes text");
        assert_eq!(
            schema[0]["function"]["parameters"]["properties"]["text"]["type"],
            "string"
        );
        // No inputSchema: an empty object schema.
        assert_eq!(
            schema[2]["function"]["parameters"],
            json!({ "type": "object", "properties": {} })
        );
        assert!(mcp.find("mcp_a_t0").is_some());
        assert!(mcp.find("mcp_a_t69").is_none());
    }

    #[test]
    fn config_skips_disabled_and_reports_bad_entries_without_values() {
        let d = temp_dir();
        let (servers, errors) = load(&d);
        assert!(servers.is_empty() && errors.is_empty());
        let p = ensure(&d).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v, json!({ "mcpServers": {} }));
        std::fs::write(
            &p,
            r#"{"mcpServers":{
                "files":{"command":"npx","args":["-y","x"],"env":{"KEY":"secret-1"},"trusted":true},
                "off":{"command":"x","disabled":true},
                "remote":{"url":"https://example.com/mcp"},
                "badenv":{"command":"x","env":{"KEY":12345}}
            }}"#,
        )
        .unwrap();
        let (servers, errors) = load(&d);
        assert_eq!(servers.len(), 1);
        let (name, cfg) = &servers[0];
        assert_eq!((name.as_str(), cfg.trusted), ("files", true));
        assert_eq!(cfg.env["KEY"], "secret-1");
        assert_eq!(errors.len(), 2);
        assert!(errors[0].starts_with("badenv:") && !errors[0].contains("12345"));
        assert!(errors[1].starts_with("remote:"));
        std::fs::write(&p, "{ not json").unwrap();
        assert!(load(&d).1[0].starts_with("mcp.json:"));
    }

    fn with_fake(ctx: &Ctx, server: Server) {
        ctx.shared.mcp.set(Mcp::new(vec![server], vec![]));
    }

    fn journal(ctx: &Ctx) -> Vec<String> {
        std::fs::read_to_string(ctx.shared.data_dir.join("actions.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| {
                let v: Value = serde_json::from_str(l).unwrap();
                format!(
                    "{} {} {}",
                    v["kind"].as_str().unwrap(),
                    v["outcome"].as_str().unwrap(),
                    v["detail"].as_str().unwrap()
                )
            })
            .collect()
    }

    #[test]
    fn confirm_policy_table() {
        use policy::mcp_needs_confirm as ask;
        for trusted in [false, true] {
            for tainted in [false, true] {
                assert!(ask(Tier::Conservative, trusted, tainted));
                assert!(!ask(Tier::CarteBlanche, trusted, tainted));
                assert_eq!(ask(Tier::Competent, trusted, tainted), !trusted || tainted);
            }
        }
    }

    #[tokio::test]
    async fn carte_blanche_calls_without_asking_taints_and_journals() {
        let mut ctx = ctx();
        ctx.cfg.tier = Tier::CarteBlanche;
        with_fake(&ctx, fake_server("fake", false).await);
        let out = tools::call(&mut ctx, "mcp_fake_echo", &json!({ "text": "yo" })).await;
        assert_eq!(out, Ok("yo\n[image]".into()));
        assert!(ctx.tainted);
        assert_eq!(
            journal(&ctx),
            [
                r#"mcp auto-started fake echo {"text":"yo"}"#,
                r#"mcp auto fake echo {"text":"yo"}"#
            ]
        );
        assert_eq!(
            tools::describe("mcp_fake_echo", &json!({})),
            "Using fake_echo"
        );
    }

    #[tokio::test]
    async fn conservative_asks_and_a_decline_does_not_call() {
        let mut ctx = ctx();
        with_fake(&ctx, fake_server("fake", true).await);
        let shared = ctx.shared.clone();
        let running = tokio::spawn(async move {
            let r = tools::call(&mut ctx, "mcp_fake_echo", &json!({ "text": "x" })).await;
            (ctx, r)
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        shared.bridge.answer(1, Answer::Confirm(false));
        let (ctx, r) = running.await.unwrap();
        assert_eq!(r, Ok("The user chose not to run it.".into()));
        assert!(!ctx.tainted);
        assert_eq!(journal(&ctx), [r#"mcp declined fake echo {"text":"x"}"#]);
    }

    #[tokio::test]
    async fn competent_runs_trusted_servers_untainted_only() {
        let mut ctx = ctx();
        ctx.cfg.tier = Tier::Competent;
        with_fake(&ctx, fake_server("fake", true).await);
        let out = tools::call(&mut ctx, "mcp_fake_fail", &json!({})).await;
        assert_eq!(out, Err("boom".into()));
        assert!(journal(&ctx)[1].starts_with("mcp failed fake fail"));
        // Now tainted: the next call asks.
        assert!(ctx.tainted);
        let shared = ctx.shared.clone();
        let running = tokio::spawn(async move {
            tools::call(&mut ctx, "mcp_fake_echo", &json!({ "text": "x" })).await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        shared.bridge.answer(1, Answer::Confirm(true));
        assert_eq!(running.await.unwrap(), Ok("x\n[image]".into()));
        // An untrusted server asks even untainted.
        let mut ctx = crate::testutil::ctx();
        ctx.cfg.tier = Tier::Competent;
        with_fake(&ctx, fake_server("other", false).await);
        let shared = ctx.shared.clone();
        let running = tokio::spawn(async move {
            tools::call(&mut ctx, "mcp_other_echo", &json!({ "text": "x" })).await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        shared.bridge.answer(1, Answer::Confirm(false));
        assert!(running.await.unwrap().unwrap().contains("not to run"));
    }

    #[tokio::test]
    async fn unknown_mcp_tools_are_errors() {
        let mut ctx = ctx();
        let out = tools::call(&mut ctx, "mcp_gone_tool", &json!({})).await;
        assert!(out.unwrap_err().contains("no longer running"));
        with_fake(&ctx, fake_server("fake", false).await);
        assert!(tools::call(&mut ctx, "mcp_fake_nope", &json!({}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn status_counts_servers_tools_and_errors() {
        let ctx = ctx();
        assert_eq!(ctx.shared.mcp.status(), (0, 0, vec![]));
        let server = fake_server("fake", false).await;
        ctx.shared
            .mcp
            .set(Mcp::new(vec![server], vec!["x: failed".into()]));
        assert_eq!(ctx.shared.mcp.status(), (1, 3, vec!["x: failed".into()]));
    }

    #[tokio::test]
    async fn a_missing_command_is_reported_and_nothing_starts() {
        let d = temp_dir();
        std::fs::write(
            path(&d),
            r#"{"mcpServers":{"ghost":{"command":"bloom-no-such-server-xyz"}}}"#,
        )
        .unwrap();
        let mcp = Mcp::start(&d, None).await;
        assert!(mcp.servers.is_empty());
        assert!(
            mcp.errors[0].starts_with("ghost: couldn't start bloom-no-such-server-xyz"),
            "{:?}",
            mcp.errors
        );
    }

    /// A real child process: a PowerShell script speaking MCP on stdio.
    #[tokio::test]
    async fn end_to_end_with_a_real_child_process() {
        let script = r#"
while ($null -ne ($l = [Console]::In.ReadLine())) {
  $m = $l | ConvertFrom-Json
  if ($null -eq $m.id) { continue }
  switch ($m.method) {
    'initialize' { $r = '{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"ps","version":"1"}}' }
    'tools/list' { $r = '{"tools":[{"name":"hello","description":"Says hi","inputSchema":{"type":"object","properties":{"who":{"type":"string"}}}}]}' }
    'tools/call' { $r = '{"content":[{"type":"text","text":"hi ' + $m.params.arguments.who + ' ' + $env:BLOOM_MCP_TEST + '"}]}' }
    default { $r = '{}' }
  }
  [Console]::Out.WriteLine('{"jsonrpc":"2.0","id":' + $m.id + ',"result":' + $r + '}')
  [Console]::Out.Flush()
}
"#;
        use base64::Engine;
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
        let d = temp_dir();
        let cfg = json!({ "mcpServers": { "ps": {
            "command": "powershell",
            "args": ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
            "env": { "BLOOM_MCP_TEST": "from-env" }
        } } });
        std::fs::write(path(&d), cfg.to_string()).unwrap();
        let state = State::default();
        state.reload(&d).await;
        assert_eq!(state.status(), (1, 1, vec![]));
        let mcp = state.get().unwrap();
        let (server, tool) = mcp.find("mcp_ps_hello").unwrap();
        let out = server
            .call(&tool.name, &json!({ "who": "Neha" }), CALL_TIMEOUT)
            .await;
        assert_eq!(out, Ok("hi Neha from-env".into()));
        // Reload with the server gone from mcp.json kills the old process.
        let pid = server._child.as_ref().unwrap().id().unwrap();
        drop(mcp);
        std::fs::write(path(&d), r#"{"mcpServers":{}}"#).unwrap();
        state.reload(&d).await;
        assert_eq!(state.status(), (0, 0, vec![]));
        let alive = || {
            let out = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
        };
        for _ in 0..20 {
            if !alive() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("server process {pid} survived the reload");
    }
}
