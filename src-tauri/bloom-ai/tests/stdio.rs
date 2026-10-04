//! Runs the real binary over pipes, the way Bloom does.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// The agent's answer to `{"type":"prompt","task":1,"text":"hi"}` with an empty settings file.
const EXPECTED: &str = r#"{"type":"reply","task":1,"text":"echo: hi"}"#;

#[test]
fn answers_over_stdio_and_exits_when_stdin_closes() {
    let settings = std::env::temp_dir().join(format!("bloom-ai-stdio-{}.json", std::process::id()));
    std::fs::write(&settings, "{}").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_bloom-ai"))
        .arg("--settings")
        .arg(&settings)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), r#"{"type":"ready"}"#);

    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"type":"prompt","task":1,"text":"hi"}}"#).unwrap();
    line.clear();
    out.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), EXPECTED);

    // Closing the pipe must end the process; a hang here is the bug.
    drop(stdin);
    assert!(child.wait().unwrap().success());
}
