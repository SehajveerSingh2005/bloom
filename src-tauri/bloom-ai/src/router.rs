//! Decides whether a request is Deep Research: a cheap phrase heuristic first,
//! and one short JSON-mode model call only when the heuristic is unsure. The
//! router never calls tools; everything but research runs the normal agent loop.

use crate::llm::Llm;
use serde_json::json;
use std::time::Duration;

/// The intents the model may answer with; only DEEP_RESEARCH changes the
/// workflow, the rest just say "not research".
const NAMES: [&str; 9] = [
    "CONTACT_LOOKUP",
    "CONTACT_UPDATE",
    "EMAIL_ACTION",
    "MESSAGE_ACTION",
    "WEB_SEARCH",
    "DEEP_RESEARCH",
    "CONTEXT_QUERY",
    "PC_ACTION",
    "GENERAL",
];

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

/// Actions (contacts, email, messages, the PC): a request that mentions
/// research but asks for one of these is the model's call.
const ACTIONS: &[&str] = &[
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
    "contact",
    "contacts",
    "phone number",
    "email address",
    "number",
    "email",
    "emails",
    "mail",
    "inbox",
    "reply to",
    "forward",
    "whatsapp",
    "message",
    "messages",
    "text",
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
    "remind",
];
/// Lookups (the user's own data, the web): sure not-research unless research is sure.
const LOOKUPS: &[&str] = &[
    "my files",
    "my documents",
    "my notes",
    "my calendar",
    "my schedule",
    "on my pc",
    "what did i",
    "did i",
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

/// Whether the request, past any polite words, starts with a research verb or phrase.
fn opens_with_research(w: &str) -> bool {
    let rest: Vec<&str> = w
        .split_whitespace()
        .skip_while(|x| POLITE.contains(x))
        .collect();
    let rest = format!(" {} ", rest.join(" "));
    RESEARCH_VERBS
        .iter()
        .chain(RESEARCH_PHRASES)
        .any(|p| rest.starts_with(&format!(" {p} ")))
}

/// The heuristic pass: research or not, and how sure it is (0..1).
pub fn heuristic(text: &str) -> (bool, f32) {
    let w = words(text);
    let research = research_score(&w);
    let action = ACTIONS.iter().any(|k| has(&w, k));
    if research >= SURE {
        // "email Sam the research on batteries": an action that mentions
        // research is the model's call, unless the request opens with it.
        return match action && !opens_with_research(&w) {
            true => (false, 0.55),
            false => (true, research),
        };
    }
    if action || LOOKUPS.iter().any(|k| has(&w, k)) {
        return (false, 0.8);
    }
    if research > 0.0 {
        return (true, research);
    }
    (false, 0.8)
}

/// One short JSON-mode call: whether the model picked DEEP_RESEARCH. None on
/// any failure, an unknown intent, or after five seconds.
async fn ask_model(llm: &Llm, text: &str) -> Option<bool> {
    let system = format!(
        "Classify the user's request for a desktop assistant. Reply with JSON only: \
         {{\"intent\": \"<one of {}>\"}}. DEEP_RESEARCH means the user wants a researched, \
         multi-source report with citations; a simple factual question is WEB_SEARCH or GENERAL.",
        NAMES.join(", ")
    );
    let messages = [
        json!({ "role": "system", "content": system }),
        json!({ "role": "user", "content": text }),
    ];
    let reply = tokio::time::timeout(MODEL_TIMEOUT, llm.json(&messages))
        .await
        .ok()?
        .ok()?;
    let name = reply["intent"].as_str()?.trim().to_ascii_uppercase();
    NAMES.contains(&name.as_str()).then(|| name == "DEEP_RESEARCH")
}

/// Whether this request is Deep Research.
pub async fn route(llm: &Llm, text: &str) -> bool {
    let (research, confidence) = heuristic(text);
    if (UNSURE_FROM..SURE).contains(&confidence) {
        return ask_model(llm, text).await.unwrap_or(research);
    }
    research
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{llm, mock_server};

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
            let (research, confidence) = heuristic(text);
            assert!(research, "{text}");
            assert!(confidence >= SURE, "{text}: {confidence}");
        }
    }

    #[test]
    fn simple_questions_and_actions_stay_out_of_research() {
        let (research, confidence) = heuristic("Who is the president of Singapore?");
        assert!(!research);
        assert!(
            confidence >= SURE,
            "a model call would be wasted: {confidence}"
        );
        for text in [
            "email Sam my research notes",
            "whatsapp Neha that I'm late",
            "save Neha's number as 98450 12345",
            "what is Neha's phone number",
            "set the volume to 30",
            "what's the latest news on the Fed",
            "what did I write in my notes yesterday",
            "tell me a joke",
            "who is the researcher behind CRISPR",
        ] {
            assert_eq!(heuristic(text), (false, 0.8), "{text}");
        }
    }

    #[test]
    fn actions_that_mention_research_are_left_to_the_model() {
        for text in [
            "email Sam the research on batteries",
            "send Neha the research on WhatsApp",
            "remind me to research the flights",
        ] {
            let (research, confidence) = heuristic(text);
            assert!(!research, "{text}");
            assert!(
                (UNSURE_FROM..SURE).contains(&confidence),
                "{text}: {confidence}"
            );
        }
        // Opening with research keeps it sure, actions or not.
        assert_eq!(
            heuristic("Do deep research on the open source movement"),
            (true, 0.9)
        );
        assert_eq!(
            heuristic("research how email spam filters work"),
            (true, 0.9)
        );
    }

    #[tokio::test]
    async fn only_unsure_requests_ask_the_model_and_failures_fall_back() {
        let unsure = "what are the causes and consequences of the 2008 crisis";
        let (research, confidence) = heuristic(unsure);
        assert!(research);
        assert!((UNSURE_FROM..SURE).contains(&confidence), "{confidence}");

        let said = |intent: &str| {
            let content = json!({ "intent": intent }).to_string();
            json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] })
                .to_string()
        };
        let (url, requests) = mock_server(vec![said("GENERAL")]);
        assert!(!route(&llm(&url), unsure).await);
        let body = requests.recv().unwrap();
        assert!(
            body.contains("json_object") && body.contains("DEEP_RESEARCH"),
            "{body}"
        );

        // Sure requests never reach the model (it would have said GENERAL).
        let (url, requests) = mock_server(vec![said("GENERAL")]);
        assert!(route(&llm(&url), "do deep research on tea").await);
        assert!(!route(&llm(&url), "Who is the president of Singapore?").await);
        assert!(requests.try_recv().is_err());

        // Unreachable model, or nonsense, keeps the heuristic's answer.
        assert!(route(&llm("http://127.0.0.1:1"), unsure).await);
        let (url, _r) = mock_server(vec![said("SHOPPING")]);
        assert!(route(&llm(&url), unsure).await);

        // A server without JSON mode is asked again without it.
        let (url, requests) = crate::testutil::mock_server_each(
            "Content-Type: application/json\r\n",
            vec![
                ("400 Bad Request", r#"{"error":{"message":"response_format not supported"}}"#.into()),
                ("200 OK", json!({ "choices": [{ "message": { "content": "```json\n{\"intent\": \"GENERAL\"}\n```" } }] }).to_string()),
            ],
        );
        assert!(!route(&llm(&url), unsure).await);
        requests.recv().unwrap();
        assert!(!requests.recv().unwrap().contains("response_format"));
    }
}
