//! Picks the workflow for a request: a cheap phrase heuristic first, and one
//! short JSON-mode model call only when the heuristic is unsure. The router
//! never calls tools; every intent but DEEP_RESEARCH runs the normal agent loop.

use crate::llm::Llm;
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    ContactLookup,
    ContactUpdate,
    EmailAction,
    MessageAction,
    WebSearch,
    DeepResearch,
    ContextQuery,
    PcAction,
    General,
}

const NAMES: [(Intent, &str); 9] = [
    (Intent::ContactLookup, "CONTACT_LOOKUP"),
    (Intent::ContactUpdate, "CONTACT_UPDATE"),
    (Intent::EmailAction, "EMAIL_ACTION"),
    (Intent::MessageAction, "MESSAGE_ACTION"),
    (Intent::WebSearch, "WEB_SEARCH"),
    (Intent::DeepResearch, "DEEP_RESEARCH"),
    (Intent::ContextQuery, "CONTEXT_QUERY"),
    (Intent::PcAction, "PC_ACTION"),
    (Intent::General, "GENERAL"),
];

impl Intent {
    fn parse(name: &str) -> Option<Intent> {
        let name = name.trim().to_ascii_uppercase();
        NAMES.iter().find(|(_, n)| *n == name).map(|(i, _)| *i)
    }
}

/// Confidence from here up is acted on without asking the model; below it
/// (and above `UNSURE_FROM`) the model gets one short call.
const SURE: f32 = 0.7;
const UNSURE_FROM: f32 = 0.4;
const MODEL_TIMEOUT: Duration = Duration::from_secs(5);

/// Always research, wherever they appear.
const RESEARCH_PHRASES: &[&str] = &[
    "deep research",
    "detailed analysis",
    "in depth analysis",
    "thorough analysis",
    "look deeply into",
    "look deep into",
    "dig deep into",
    "comprehensive report",
    "detailed report",
    "find out everything about",
];
/// Research verbs: sure when they open the request or take an object.
const RESEARCH_VERBS: &[&str] = &["research", "investigate"];
const VERB_OBJECTS: &[&str] = &[
    "on", "into", "about", "how", "what", "why", "whether", "who", "when", "where", "the", "a",
    "an", "this", "that", "these", "those",
];
const POLITE: &[&str] = &[
    "please", "can", "could", "would", "will", "you", "hey", "janice", "i", "want", "need", "to",
    "do", "some", "go", "and",
];
/// Facets of a multi-part analytical ask; three or more read as research.
const FACETS: &[&str] = &[
    "major parties",
    "key players",
    "stakeholders",
    "recent developments",
    "latest developments",
    "public sentiment",
    "public opinion",
    "next developments",
    "likely next",
    "what happens next",
    "outlook",
    "implications",
    "background",
    "timeline",
    "causes",
    "consequences",
    "impact",
    "pros and cons",
    "arguments",
    "perspectives",
    "risks",
    "trends",
];
const ANALYTIC: &[&str] = &[
    "explain",
    "analyze",
    "analyse",
    "compare",
    "assess",
    "evaluate",
    "break down",
    "overview",
];

/// Ordered: the first list with a hit wins.
const KEYWORDS: &[(Intent, &[&str])] = &[
    (
        Intent::ContactUpdate,
        &[
            "save contact",
            "add contact",
            "update contact",
            "number is",
            "email is",
            "address is",
            "number as",
            "email as",
            "to my contacts",
            "to contacts",
        ],
    ),
    (
        Intent::ContactLookup,
        &[
            "contact",
            "contacts",
            "phone number",
            "email address",
            "number",
        ],
    ),
    (
        Intent::EmailAction,
        &["email", "emails", "mail", "inbox", "reply to", "forward"],
    ),
    (
        Intent::MessageAction,
        &["whatsapp", "message", "messages", "text"],
    ),
    (
        Intent::PcAction,
        &[
            "open",
            "launch",
            "volume",
            "brightness",
            "wifi",
            "wi fi",
            "bluetooth",
            "mute",
            "unmute",
            "pause",
            "play",
            "skip",
            "turn on",
            "turn off",
            "shut down",
            "restart",
            "screenshot",
        ],
    ),
    (
        Intent::ContextQuery,
        &[
            "my files",
            "my documents",
            "my notes",
            "my calendar",
            "my schedule",
            "on my pc",
            "what did i",
            "did i",
        ],
    ),
    (
        Intent::WebSearch,
        &[
            "latest",
            "news",
            "today",
            "current",
            "currently",
            "price",
            "prices",
            "stock",
            "score",
            "search",
            "look up",
            "google",
            "this week",
            "right now",
            "weather",
        ],
    ),
];

/// Lowercase words separated by single spaces, padded so ` phrase ` matches whole words.
fn words(text: &str) -> String {
    let mut out = String::from(" ");
    for w in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        out += w;
        out.push(' ');
    }
    out
}

fn has(words: &str, phrase: &str) -> bool {
    words.contains(&format!(" {phrase} "))
}

/// The research leaning alone: sure, unsure, or none.
fn research_score(w: &str) -> f32 {
    if RESEARCH_PHRASES.iter().any(|p| has(w, p)) {
        return 0.9;
    }
    let list: Vec<&str> = w.split_whitespace().collect();
    let mut score: f32 = 0.0;
    for (i, word) in list.iter().enumerate() {
        if !RESEARCH_VERBS.contains(word) {
            continue;
        }
        let opens = list[..i].iter().all(|p| POLITE.contains(p));
        let object = list.get(i + 1).is_some_and(|n| VERB_OBJECTS.contains(n));
        score = score.max(if opens || object { 0.9 } else { 0.55 });
    }
    let facets = FACETS.iter().filter(|f| has(w, f)).count();
    let analytic = facets > 0 && ANALYTIC.iter().any(|a| has(w, a));
    match facets + analytic as usize {
        0 | 1 => score,
        2 => score.max(0.55),
        _ => score.max(0.85),
    }
}

/// The heuristic pass: an intent and how sure it is (0..1).
pub fn heuristic(text: &str) -> (Intent, f32) {
    let w = words(text);
    let research = research_score(&w);
    if research >= SURE {
        return (Intent::DeepResearch, research);
    }
    if let Some((intent, _)) = KEYWORDS
        .iter()
        .find(|(_, list)| list.iter().any(|k| has(&w, k)))
    {
        return (*intent, 0.8);
    }
    if research > 0.0 {
        return (Intent::DeepResearch, research);
    }
    (Intent::General, 0.8)
}

/// One short JSON-mode call; None on any failure or after five seconds.
async fn ask_model(llm: &Llm, text: &str) -> Option<Intent> {
    let names: Vec<&str> = NAMES.iter().map(|(_, n)| *n).collect();
    let system = format!(
        "Classify the user's request for a desktop assistant. Reply with JSON only: \
         {{\"intent\": \"<one of {}>\"}}. DEEP_RESEARCH means the user wants a researched, \
         multi-source report with citations; a simple factual question is WEB_SEARCH or GENERAL.",
        names.join(", ")
    );
    let messages = [
        json!({ "role": "system", "content": system }),
        json!({ "role": "user", "content": text }),
    ];
    let reply = tokio::time::timeout(MODEL_TIMEOUT, llm.json(&messages))
        .await
        .ok()?
        .ok()?;
    Intent::parse(reply["intent"].as_str()?)
}

/// The workflow for this request.
pub async fn route(llm: &Llm, text: &str) -> Intent {
    let (intent, confidence) = heuristic(text);
    if (UNSURE_FROM..SURE).contains(&confidence) {
        return ask_model(llm, text).await.unwrap_or(intent);
    }
    intent
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{http, mock_server};

    fn llm(url: String) -> Llm {
        Llm {
            http: http(),
            base_url: url,
            model: "m".into(),
            key: "k".into(),
        }
    }

    #[test]
    fn the_users_research_phrasings_route_to_deep_research() {
        for text in [
            "Do deep research on the Thai election",
            "Research the history of the Suez Canal",
            "Can you investigate why lithium prices fell this year?",
            "Do a detailed analysis of the EV market in India",
            "Look deeply into the Boeing strike",
            "Give me a comprehensive report on lithium mining",
            "Find out everything about the Rust 2024 edition",
            "Explain the major parties, recent developments, public sentiment and likely next developments in Bangladesh politics",
        ] {
            let (intent, confidence) = heuristic(text);
            assert_eq!(intent, Intent::DeepResearch, "{text}");
            assert!(confidence >= SURE, "{text}: {confidence}");
        }
    }

    #[test]
    fn simple_questions_and_actions_stay_out_of_research() {
        let (intent, confidence) = heuristic("Who is the president of Singapore?");
        assert!(
            matches!(intent, Intent::General | Intent::WebSearch),
            "{intent:?}"
        );
        assert!(
            confidence >= SURE,
            "a model call would be wasted: {confidence}"
        );
        for (text, want) in [
            ("email Sam my research notes", Intent::EmailAction),
            ("whatsapp Neha that I'm late", Intent::MessageAction),
            ("save Neha's number as 98450 12345", Intent::ContactUpdate),
            ("what is Neha's phone number", Intent::ContactLookup),
            ("set the volume to 30", Intent::PcAction),
            ("what's the latest news on the Fed", Intent::WebSearch),
            (
                "what did I write in my notes yesterday",
                Intent::ContextQuery,
            ),
            ("tell me a joke", Intent::General),
            ("who is the researcher behind CRISPR", Intent::General),
        ] {
            assert_eq!(heuristic(text).0, want, "{text}");
        }
    }

    #[tokio::test]
    async fn only_unsure_requests_ask_the_model_and_failures_fall_back() {
        let unsure = "what are the causes and consequences of the 2008 crisis";
        let (intent, confidence) = heuristic(unsure);
        assert_eq!(intent, Intent::DeepResearch);
        assert!((UNSURE_FROM..SURE).contains(&confidence), "{confidence}");

        let said = |intent: &str| {
            let content = json!({ "intent": intent }).to_string();
            json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] })
                .to_string()
        };
        let (url, requests) = mock_server(vec![said("GENERAL")]);
        assert_eq!(route(&llm(url), unsure).await, Intent::General);
        let body = requests.recv().unwrap();
        assert!(
            body.contains("json_object") && body.contains("DEEP_RESEARCH"),
            "{body}"
        );

        // Sure requests never reach the model (it would have said GENERAL).
        let (url, requests) = mock_server(vec![said("GENERAL")]);
        assert_eq!(
            route(&llm(url.clone()), "do deep research on tea").await,
            Intent::DeepResearch
        );
        assert_eq!(
            route(&llm(url), "Who is the president of Singapore?").await,
            Intent::General
        );
        assert!(requests.try_recv().is_err());

        // Unreachable model, or nonsense, keeps the heuristic's answer.
        assert_eq!(
            route(&llm("http://127.0.0.1:1".into()), unsure).await,
            Intent::DeepResearch
        );
        let (url, _r) = mock_server(vec![said("SHOPPING")]);
        assert_eq!(route(&llm(url), unsure).await, Intent::DeepResearch);

        // A server without JSON mode is asked again without it.
        let (url, requests) = crate::testutil::mock_server_each(
            "Content-Type: application/json\r\n",
            vec![
                ("400 Bad Request", r#"{"error":{"message":"response_format not supported"}}"#.into()),
                ("200 OK", json!({ "choices": [{ "message": { "content": "```json\n{\"intent\": \"GENERAL\"}\n```" } }] }).to_string()),
            ],
        );
        assert_eq!(route(&llm(url), unsure).await, Intent::General);
        requests.recv().unwrap();
        assert!(!requests.recv().unwrap().contains("response_format"));
    }
}
