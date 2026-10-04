//! Runs one script in a hidden, non-interactive PowerShell with a time limit.
//! Dropping the future (Stop button, timeout) kills the process.

use base64::Engine;
use std::process::Stdio;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(120);
const MAX_OUTPUT: usize = 8000;
/// Quiet progress bars and UTF-8 output. Added after the policy check, so the
/// `::` here never counts against the model's script.
const PRELUDE: &str = "$ProgressPreference='SilentlyContinue'; [Console]::OutputEncoding=[Text.Encoding]::UTF8; $ErrorActionPreference='Continue';\n";

pub async fn run(script: &str) -> Result<String, String> {
    let utf16: Vec<u8> = format!("{PRELUDE}{script}")
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
        &encoded,
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = tokio::time::timeout(LIMIT, cmd.output())
        .await
        .map_err(|_| "The script ran for 2 minutes and was stopped.".to_string())?
        .map_err(|e| format!("Couldn't start PowerShell: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let errors = String::from_utf8_lossy(&out.stderr);
    if !errors.trim().is_empty() {
        text.push_str("\n[errors]\n");
        // Convert CLIXML error output to readable text by extracting error messages
        let decoded_errors = if errors.contains("#< CLIXML") {
            decode_clixml_errors(&errors)
        } else {
            errors.into_owned()
        };
        text.push_str(&decoded_errors);
    }
    if text.trim().is_empty() {
        text = "(no output)".into();
    }
    Ok(clip(&text, MAX_OUTPUT))
}

/// Extracts readable error messages from PowerShell's CLIXML error output.
/// Decodes escape sequences like _x000D__x000A_ back to actual characters.
fn decode_clixml_errors(clixml: &str) -> String {
    // Extract all text within <S S="Error"> tags, then unescape PowerShell's encoding
    let mut result = String::new();
    for line in clixml.split("<S S=\"Error\">") {
        if let Some(end) = line.find("</S>") {
            let text = &line[..end];
            // Unescape PowerShell's XML encoding: _x000D_ = \r, _x000A_ = \n
            let unescaped = text
                .replace("_x000D__x000A_", "\n")
                .replace("_x000D_", "\r")
                .replace("_x000A_", "\n")
                .replace("_x0020_", " ");
            if !unescaped.is_empty() {
                result.push_str(&unescaped);
            }
        }
    }
    if result.is_empty() {
        clixml.to_string()
    } else {
        result
    }
}

/// Cuts long output at a character boundary so the model's context stays small.
pub fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[output cut at {max} bytes]", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_a_script() {
        let out = run("Write-Output ('bl' + 'oom')").await.unwrap();
        assert_eq!(out.trim(), "bloom");
    }

    #[tokio::test]
    async fn reports_errors() {
        let out = run("Get-Item Z:\\no\\such\\file").await.unwrap();
        assert!(out.contains("[errors]"));
        assert!(out.contains("Cannot find"));
        assert!(!out.contains("CLIXML"));
    }

    #[test]
    fn clips_on_char_boundaries() {
        assert_eq!(clip("abc", 10), "abc");
        assert!(clip("ééééé", 3).starts_with("é"));
    }

    #[test]
    fn decodes_clixml_errors() {
        let clixml = r#"#< CLIXML
<Objs Version="1.1.0.1" xmlns="http://schemas.microsoft.com/powershell/2004/04"><S S="Error">Get-Item : Cannot find drive. A drive with the name 'Z' does not exist._x000D__x000A_</S><S S="Error">At line:2 char:1_x000D__x000A_</S></Objs>"#;
        let decoded = decode_clixml_errors(clixml);
        assert!(decoded.contains("Cannot find"));
        assert!(!decoded.contains("_x000D__x000A_"));
        assert!(!decoded.contains("CLIXML"));
    }
}
