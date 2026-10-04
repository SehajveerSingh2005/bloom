//! Long-term memory: stable facts about the user, kept in `memory.json` in the
//! data dir across restarts. (Not the short-term conversation `agent::Memory`.)

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_FACTS: usize = 500;
pub const MAX_CHARS: usize = 300;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Fact {
    pub id: u64,
    pub text: String,
    pub at: u64,
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join("memory.json")
}

pub fn load(dir: &Path) -> Vec<Fact> {
    std::fs::read_to_string(path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Temp file + rename, so a crash never leaves half a file.
fn save(dir: &Path, facts: &[Fact]) -> Result<(), String> {
    let file = path(dir);
    let tmp = file.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(facts).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
}

pub fn count(dir: &Path) -> usize {
    load(dir).len()
}

/// Empties the memory (an empty list stays on disk so Reveal has a file).
pub fn clear(dir: &Path) -> Result<(), String> {
    save(dir, &[])
}

/// Makes sure the file exists, for Reveal.
pub fn ensure(dir: &Path) -> Result<PathBuf, String> {
    if !path(dir).exists() {
        save(dir, &[])?;
    }
    Ok(path(dir))
}

/// Adds a fact. Returns None when it is already known (case-insensitive).
pub fn remember(dir: &Path, text: &str) -> Result<Option<Fact>, String> {
    let text: String = text.trim().chars().take(MAX_CHARS).collect();
    if text.is_empty() {
        return Err("nothing to remember".into());
    }
    let mut facts = load(dir);
    if facts
        .iter()
        .any(|f| f.text.to_lowercase() == text.to_lowercase())
    {
        return Ok(None);
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let id = facts.iter().map(|f| f.id).max().unwrap_or(0) + 1;
    let fact = Fact { id, text, at };
    facts.push(fact.clone());
    if facts.len() > MAX_FACTS {
        facts.remove(0); // oldest
    }
    save(dir, &facts)?;
    Ok(Some(fact))
}

pub fn forget(dir: &Path, id: u64) -> Result<bool, String> {
    let mut facts = load(dir);
    let before = facts.len();
    facts.retain(|f| f.id != id);
    if facts.len() == before {
        return Ok(false);
    }
    save(dir, &facts)?;
    Ok(true)
}

fn words(s: &str) -> std::collections::HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Top 10 facts by lowercase word overlap with `query`, newest first on ties.
pub fn recall(dir: &Path, query: &str) -> Vec<Fact> {
    let q = words(query);
    // ponytail: linear scan of at most 500 facts; move to SQLite FTS5 if memory outgrows that.
    let mut hits: Vec<(usize, Fact)> = load(dir)
        .into_iter()
        .map(|f| (words(&f.text).intersection(&q).count(), f))
        .filter(|(score, _)| *score > 0)
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.id.cmp(&a.1.id)));
    hits.into_iter().take(10).map(|(_, f)| f).collect()
}

/// The "What you know about the user" prompt section; empty when no facts.
pub fn prompt_section(dir: &Path) -> String {
    let facts = load(dir);
    if facts.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = facts
        .iter()
        .rev()
        .take(20)
        .map(|f| format!("[{}] {}", f.id, f.text))
        .collect();
    format!(
        "\nWhat you know about the user (id in brackets, for forget):\n{}\n",
        lines.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    #[test]
    fn remember_dedupes_caps_and_persists() {
        let d = temp_dir();
        assert!(remember(&d, "I'm vegetarian").unwrap().is_some());
        assert!(remember(&d, "i'm VEGETARIAN").unwrap().is_none());
        let long = "x".repeat(400);
        assert_eq!(remember(&d, &long).unwrap().unwrap().text.len(), MAX_CHARS);
        assert_eq!(count(&d), 2);
        assert!(!d.join("memory.json.tmp").exists());
    }

    #[test]
    fn oldest_dropped_past_500() {
        let d = temp_dir();
        for i in 0..502 {
            remember(&d, &format!("fact {i}")).unwrap();
        }
        let f = load(&d);
        assert_eq!(f.len(), MAX_FACTS);
        assert_eq!(f[0].text, "fact 2");
    }

    #[test]
    fn recall_scores_then_newest_and_forget_works() {
        let d = temp_dir();
        remember(&d, "my manager is Sam").unwrap();
        remember(&d, "I like tea").unwrap();
        remember(&d, "Sam likes tea").unwrap();
        let r = recall(&d, "tea sam");
        assert_eq!(r[0].text, "Sam likes tea");
        assert_eq!(r.len(), 3);
        assert!(recall(&d, "zebra").is_empty());
        assert!(forget(&d, r[0].id).unwrap());
        assert!(!forget(&d, 999).unwrap());
        assert_eq!(count(&d), 2);
        clear(&d).unwrap();
        assert_eq!(count(&d), 0);
    }

    #[test]
    fn prompt_lists_newest_20_with_ids() {
        let d = temp_dir();
        assert_eq!(prompt_section(&d), "");
        for i in 0..25 {
            remember(&d, &format!("fact {i}")).unwrap();
        }
        let p = prompt_section(&d);
        assert!(p.contains("[25] fact 24") && p.contains("fact 5") && !p.contains("fact 4\n"));
    }
}
