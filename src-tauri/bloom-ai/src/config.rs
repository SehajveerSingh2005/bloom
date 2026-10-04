//! The agent's settings, read from Bloom's settings.json at the start of every
//! request, so a change in Settings applies to the next request without any
//! messages. Secrets are not here: see secrets.rs.

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Tier {
    #[default]
    Conservative,
    Competent,
    CarteBlanche,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// What the assistant is called (see `clean_name`).
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub stt_url: String,
    pub stt_model: String,
    pub tier: Tier,
    pub email: String,
    /// Empty: use the preset for the address's domain (email.rs).
    pub smtp_host: String,
    /// 0: use the preset.
    pub smtp_port: u16,
}

pub const DEFAULT_NAME: &str = "Janice";

/// The assistant's name: trimmed, 1-24 letters, spaces, hyphens or
/// apostrophes; anything else gives the default.
pub fn clean_name(raw: &str) -> String {
    let name = raw.trim();
    let ok = (1..=24).contains(&name.chars().count())
        && name
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\'' | '’'));
    if ok { name } else { DEFAULT_NAME }.to_string()
}

impl Config {
    pub fn from_map(map: &HashMap<String, Value>) -> Config {
        let get = |key: &str, default: &str| -> String {
            map.get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or(default)
                .to_string()
        };
        let base_url = get("bloom-ai-base-url", "https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_string();
        Config {
            name: clean_name(
                map.get("bloom-ai-name")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            stt_url: get("bloom-ai-stt-url", &base_url)
                .trim_end_matches('/')
                .to_string(),
            base_url,
            model: get("bloom-ai-model", ""),
            stt_model: get("bloom-ai-stt-model", "whisper-1"),
            tier: match get("bloom-ai-security", "conservative").as_str() {
                "competent" => Tier::Competent,
                "carte-blanche" => Tier::CarteBlanche,
                _ => Tier::Conservative,
            },
            email: get("bloom-ai-email", ""),
            smtp_host: get("bloom-ai-smtp-host", ""),
            smtp_port: get("bloom-ai-smtp-port", "0").parse().unwrap_or(0),
        }
    }

    /// A missing or broken settings.json gives the defaults.
    pub fn load(path: &Path) -> Config {
        let map = std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default();
        Config::from_map(&map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect()
    }

    #[test]
    fn defaults() {
        let c = Config::from_map(&HashMap::new());
        assert_eq!(c.base_url, "https://api.openai.com/v1");
        assert_eq!(c.stt_url, "https://api.openai.com/v1");
        assert_eq!(c.stt_model, "whisper-1");
        assert_eq!(c.model, "");
        assert_eq!(c.tier, Tier::Conservative);
        assert_eq!(c.smtp_port, 0);
    }

    #[test]
    fn speech_url_follows_the_chat_url_unless_set() {
        let c = Config::from_map(&map(&[(
            "bloom-ai-base-url",
            "https://api.groq.com/openai/v1/",
        )]));
        assert_eq!(c.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(c.stt_url, "https://api.groq.com/openai/v1");
        let c = Config::from_map(&map(&[("bloom-ai-stt-url", "http://localhost:8000/v1")]));
        assert_eq!(c.stt_url, "http://localhost:8000/v1");
    }

    #[test]
    fn tiers_parse_and_unknown_means_conservative() {
        let tier = |v| Config::from_map(&map(&[("bloom-ai-security", v)])).tier;
        assert_eq!(tier("competent"), Tier::Competent);
        assert_eq!(tier("carte-blanche"), Tier::CarteBlanche);
        assert_eq!(tier("yolo"), Tier::Conservative);
    }

    #[test]
    fn names_are_validated() {
        assert_eq!(clean_name("  Mina "), "Mina");
        assert_eq!(clean_name("Anne-Marie O'Neil"), "Anne-Marie O'Neil");
        for bad in ["", "   ", "R2D2", "a<b", &"x".repeat(25)] {
            assert_eq!(clean_name(bad), "Janice", "{bad}");
        }
        let c = Config::from_map(&map(&[("bloom-ai-name", "Mina")]));
        assert_eq!(c.name, "Mina");
        assert_eq!(Config::from_map(&HashMap::new()).name, "Janice");
    }

    #[test]
    fn missing_file_gives_defaults() {
        let c = Config::load(Path::new("Z:/no/such/settings.json"));
        assert_eq!(c, Config::from_map(&HashMap::new()));
    }
}
