//! Test helpers: a tiny HTTP server with canned answers, temp folders, a Ctx.

use crate::agent::{Ctx, Shared};
use crate::config::Config;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

/// Answers each request with the next body (HTTP 200, JSON). Returns the base
/// URL and a channel that yields each request body.
pub fn mock_server(bodies: Vec<String>) -> (String, mpsc::Receiver<String>) {
    mock_server_status("200 OK", bodies)
}

/// Like `mock_server`, answering with the given HTTP status line.
pub fn mock_server_status(
    status: &'static str,
    bodies: Vec<String>,
) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for body in bodies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut request = vec![0; len];
            reader.read_exact(&mut request).unwrap();
            tx.send(String::from_utf8_lossy(&request).into_owned())
                .unwrap();
            write!(
                stream,
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status,
                body.len(),
                body
            )
            .unwrap();
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

pub fn ctx() -> Ctx {
    let shared = Shared {
        bridge: Default::default(),
        data_dir: temp_dir(),
        settings_path: PathBuf::new(),
        http: http(),
        memory: Default::default(),
        endpoints: Default::default(),
    };
    Ctx {
        task: 1,
        cfg: Config::from_map(&Default::default()),
        shared: Arc::new(shared),
        tainted: false,
        saved_this_task: HashSet::new(),
    }
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
