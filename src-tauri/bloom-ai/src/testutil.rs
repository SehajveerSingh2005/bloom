//! Test helpers: a tiny HTTP server with canned answers, temp folders, a Ctx.

use crate::agent::{Ctx, Shared};
use crate::config::Config;
use crate::llm::Llm;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

/// Answers each request with the next body (HTTP 200, JSON). Returns the base
/// URL and a channel that yields each request body.
pub fn mock_server(bodies: Vec<String>) -> (String, mpsc::Receiver<String>) {
    mock_server_full("200 OK", "Content-Type: application/json\r\n", bodies)
}

/// Like `mock_server`, with a status line and raw header lines (each ending CRLF).
pub fn mock_server_full(
    status: &'static str,
    headers: &'static str,
    bodies: Vec<String>,
) -> (String, mpsc::Receiver<String>) {
    mock_server_each(headers, bodies.into_iter().map(|b| (status, b)).collect())
}

/// Like `mock_server_full`, with a status line per answer.
pub fn mock_server_each(
    headers: &'static str,
    bodies: Vec<(&'static str, String)>,
) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for (status, body) in bodies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                head.push_str(&line);
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut request = vec![0; len];
            reader.read_exact(&mut request).unwrap();
            // A request with no body (GET) yields its request line and headers.
            let seen = if len == 0 { head } else { String::from_utf8_lossy(&request).into_owned() };
            tx.send(seen)
                .unwrap();
            // The client may hang up early (body cap tests).
            let _ = write!(
                stream,
                "HTTP/1.1 {}\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                headers,
                body.len(),
                body
            );
        }
    });
    (url, rx)
}

/// A fresh, empty folder per call.
pub fn temp_dir() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "bloom-ai-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// A model at `url`.
pub fn llm(url: &str) -> Llm {
    Llm {
        http: http(),
        base_url: url.into(),
        model: "m".into(),
        key: "k".into(),
    }
}

pub fn ctx() -> Ctx {
    let shared = Shared {
        data_dir: temp_dir(),
        http: http(),
        ..Shared::new(PathBuf::new(), PathBuf::new())
    };
    Ctx {
        task: 1,
        cfg: Config::from_map(&Default::default()),
        shared: Arc::new(shared),
        tainted: false,
        saved_this_task: HashSet::new(),
        allowed_urls: HashSet::new(),
    }
}

/// A Ctx whose web tools may reach 127.0.0.1 and whose searches go to `url`
/// (Brave with `key`, else DuckDuckGo).
pub fn web_ctx(url: &str, key: Option<&str>) -> Ctx {
    let mut c = ctx();
    let w = &mut Arc::get_mut(&mut c.shared).unwrap().web;
    w.allow_loopback = true;
    w.use_store = false;
    w.key = key.map(str::to_string);
    w.search_url = url.into();
    w.brave_url = url.into();
    c
}

/// A Ctx whose weather lookups all go to `url`.
pub fn ctx_with_endpoints(url: &str) -> Ctx {
    let mut c = ctx();
    let shared = Arc::get_mut(&mut c.shared).unwrap();
    shared.endpoints = crate::weather::Endpoints {
        forecast: url.into(),
        geocode: url.into(),
        ip_primary: url.into(),
        ip_fallback: url.into(),
    };
    c
}
