//! actions.log: JSON lines for every email or script, whatever the outcome.
//! A run that actually starts writes `auto-started` or `approved-started`
//! first, then its final line (`auto`, `approved` or `failed`), so a Stop
//! mid-run leaves the started line. `declined` has no started line.
//! It lives in the ai folder, so "Delete AI altogether" removes it.

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn record(dir: &Path, kind: &str, detail: &str, outcome: &str) {
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = serde_json::json!({ "at": at, "kind": kind, "outcome": outcome, "detail": detail });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("actions.log"))
    {
        let _ = writeln!(file, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_one_json_line_per_action() {
        let dir = crate::testutil::temp_dir();
        record(&dir, "script", "Get-Date", "auto");
        record(&dir, "email", "to a@b.co: hi", "declined");
        let log = std::fs::read_to_string(dir.join("actions.log")).unwrap();
        let lines: Vec<serde_json::Value> = log
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["outcome"], "declined");
    }
}
