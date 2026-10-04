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
    /// Write ai\debug.log (debug.rs).
    pub debug: bool,
    /// Bloom's saved weather location and unit (see weather.rs).
    pub weather_lat: Option<f64>,
    pub weather_lon: Option<f64>,
    pub weather_city: String,
    pub fahrenheit: bool,
    /// "Connect WhatsApp": the WhatsApp tools are offered.
    pub whatsapp: bool,
    /// `bloom-ai-enabled`.
    pub enabled: bool,
    /// Automatic WhatsApp replies (autoreply.rs).
    pub auto_reply: bool,
    /// Numbers that get them; "*" is anyone in phones.json.
    pub auto_to: Vec<String>,
    /// "How to reply", in the user's words.
    pub auto_style: String,
    /// Replies say they come from the assistant.
    pub auto_sign: bool,
    /// "Answer me in my own chat" (selfchat.rs).
    pub self_chat: bool,
}

pub const DEFAULT_STYLE: &str = "Let them know I'll get back to them soon. Be brief and friendly.";

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
        let number = |key: &str| {
            let v = map.get(key)?;
            v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
        };
        Config {
            weather_lat: number("bloom-weather-lat"),
            weather_lon: number("bloom-weather-lon"),
            weather_city: get("bloom-weather-city", ""),
            fahrenheit: get("bloom-temp-unit", "celsius") == "fahrenheit",
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
            debug: get("bloom-ai-debug", "false") == "true",
            whatsapp: get("bloom-ai-whatsapp", "false") == "true",
            enabled: get("bloom-ai-enabled", "false") == "true",
            auto_reply: get("bloom-ai-whatsapp-autoreply", "false") == "true",
            auto_to: match map.get("bloom-ai-whatsapp-auto") {
                Some(Value::String(s)) if s.trim().trim_matches('"') == "*" => vec!["*".into()],
                Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_default(),
                Some(v) => serde_json::from_value(v.clone()).unwrap_or_default(),
                None => Vec::new(),
            },
            auto_style: get("bloom-ai-whatsapp-style", DEFAULT_STYLE),
            auto_sign: get("bloom-ai-whatsapp-sign", "true") == "true",
            self_chat: get("bloom-ai-whatsapp-selfchat", "false") == "true",
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
        assert!(!c.debug);
        assert!(!c.whatsapp);
        assert!(Config::from_map(&map(&[("bloom-ai-whatsapp", "true")])).whatsapp);
        assert!(Config::from_map(&map(&[("bloom-ai-debug", "true")])).debug);
    }

    #[test]
    fn auto_reply_settings() {
        let d = Config::from_map(&HashMap::new());
        assert!(!d.enabled && !d.auto_reply && d.auto_sign && !d.self_chat);
        assert!(Config::from_map(&map(&[("bloom-ai-whatsapp-selfchat", "true")])).self_chat);
        assert!(d.auto_to.is_empty());
        assert_eq!(d.auto_style, DEFAULT_STYLE);
        let c = Config::from_map(&map(&[
            ("bloom-ai-enabled", "true"),
            ("bloom-ai-whatsapp-autoreply", "true"),
            ("bloom-ai-whatsapp-auto", r#"["+491701234567"]"#),
            ("bloom-ai-whatsapp-style", "At work until 6"),
            ("bloom-ai-whatsapp-sign", "false"),
        ]));
        assert!(c.enabled && c.auto_reply && !c.auto_sign);
        assert_eq!(c.auto_to, ["+491701234567"]);
        assert_eq!(c.auto_style, "At work until 6");
        let any = |v| Config::from_map(&map(&[("bloom-ai-whatsapp-auto", v)])).auto_to;
        assert_eq!(any("*"), ["*"]);
        assert_eq!(any(r#""*""#), ["*"]);
        assert!(any("garbage").is_empty());
    }

    #[test]
    fn weather_settings_parse_from_strings_or_numbers() {
        let mut m = map(&[
            ("bloom-weather-lat", " 28.6 "),
            ("bloom-weather-city", "Delhi"),
            ("bloom-temp-unit", "fahrenheit"),
        ]);
        m.insert("bloom-weather-lon".into(), json!(77.2));
        let c = Config::from_map(&m);
        assert_eq!((c.weather_lat, c.weather_lon), (Some(28.6), Some(77.2)));
        assert_eq!(c.weather_city, "Delhi");
        assert!(c.fahrenheit);
        let d = Config::from_map(&HashMap::new());
        assert_eq!(
            (d.weather_lat, d.weather_lon, d.fahrenheit),
            (None, None, false)
        );
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
