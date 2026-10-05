//! web_search (Brave Search with a key, else DuckDuckGo's HTML page) and web_fetch (a page as text).

use crate::agent::{Ctx, Shared};
use crate::config::Tier;
use crate::errors::{coded, SEARCH_BLOCKED, SEARCH_FAILED, SEARCH_NOT_CONFIGURED};
use crate::protocol::ConfirmKind;
use reqwest::Url;
use std::net::IpAddr;
use std::time::Duration;

const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_CHARS: usize = 8000;
const MAX_RESULTS: usize = 8;
const MAX_HOPS: usize = 5;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Bloom-Janice/1.0";

/// Web settings kept on `Shared`. Tests point the search at a local server and
/// may reach 127.0.0.1; nothing the model sends can change either.
pub struct WebCfg {
    pub search_url: String,
    pub brave_url: String,
    /// Tests set a key here; otherwise it comes from Credential Manager.
    pub key: Option<String>,
    pub allow_loopback: bool,
    /// Never follows redirects: `fetch` follows them itself, checking each hop.
    pub http: reqwest::Client,
}

impl WebCfg {
    pub fn new() -> WebCfg {
        WebCfg {
            search_url: "https://html.duckduckgo.com/html/".into(),
            brave_url: "https://api.search.brave.com/res/v1".into(),
            key: None,
            allow_loopback: false,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("http client"),
        }
    }
}

/// URLs written in the user's own request text.
pub fn urls_in(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| w.trim_start_matches(['(', '<', '[', '"', '\'']))
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(|w| {
            w.trim_end_matches(['.', ',', ')', ';', '!', '?', '"', '\''])
                .to_string()
        })
        .collect()
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|&e| e <= 10).and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => ' ',
                _ => {
                    let n = name.strip_prefix('#')?;
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(h) => u32::from_str_radix(h, 16).ok()?,
                        None => n.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, e + 1))
        });
        match decoded {
            Some((c, n)) => {
                out.push(c);
                rest = &rest[n..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out + rest
}

/// Drops `<name ...>...</name>` blocks (case-insensitive).
fn strip_block(html: &str, name: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let (open, close) = (format!("<{name}"), format!("</{name}"));
    let mut out = String::new();
    let mut at = 0;
    while let Some(s) = lower[at..].find(&open).map(|i| i + at) {
        out.push_str(&html[at..s]);
        at = match lower[s..].find(&close) {
            Some(e) => lower[s + e..]
                .find('>')
                .map_or(html.len(), |g| s + e + g + 1),
            None => html.len(),
        };
    }
    out + &html[at.min(html.len())..]
}

/// Readable text: tags gone, entities decoded, whitespace collapsed.
fn text_of(html: &str) -> String {
    let html = strip_block(&strip_block(html, "script"), "style");
    let mut bare = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                bare.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => bare.push(c),
            _ => {}
        }
    }
    decode_entities(&bare)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The value of `name="..."` inside one opening tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')?;
    Some(decode_entities(&tag[at..at + end]))
}

#[derive(Debug)]
struct Hit {
    title: String,
    url: String,
    snippet: String,
}

/// ponytail: scraping DuckDuckGo's HTML page is fragile; swap in a keyed
/// provider (Brave, Tavily) when the markup changes and this returns nothing.
fn parse_results(html: &str) -> Vec<Hit> {
    let marks: Vec<usize> = html.match_indices("result__a").map(|(i, _)| i).collect();
    let mut hits = Vec::new();
    for (n, &m) in marks.iter().enumerate() {
        let next = marks.get(n + 1).copied().unwrap_or(html.len());
        let Some(open) = html[..m].rfind("<a ") else {
            continue;
        };
        let Some(gt) = html[m..].find('>').map(|g| g + m) else {
            continue;
        };
        let Some(href) = attr(&html[open..gt], "href") else {
            continue;
        };
        let href = if href.starts_with("//") {
            format!("https:{href}")
        } else {
            href
        };
        let Ok(link) = Url::parse(&href) else {
            continue;
        };
        let url = match link.query_pairs().find(|(k, _)| k == "uddg") {
            Some((_, v)) => v.into_owned(),
            None if link
                .host_str()
                .is_some_and(|h| h.ends_with("duckduckgo.com")) =>
            {
                continue
            }
            None => href,
        };
        let title_end = html[gt..].find("</a>").map_or(html.len(), |e| gt + e);
        let title = text_of(&html[gt + 1..title_end]);
        let snippet = html[gt..next]
            .find("result__snippet")
            .and_then(|s| {
                let s = s + gt;
                let start = html[s..next].find('>')? + s + 1;
                let end = html[start..next]
                    .find("</a>")
                    .or(html[start..next].find("</div>"))?;
                Some(text_of(&html[start..start + end]))
            })
            .unwrap_or_default();
        hits.push(Hit {
            title,
            url,
            snippet,
        });
        if hits.len() == MAX_RESULTS {
            break;
        }
    }
    hits
}

const BLOCKED: &str = "DuckDuckGo is blocking automated searches from this PC. Add a free Brave Search key in Settings > AI to search the web.";

fn search_key(web: &WebCfg) -> Option<String> {
    web.key
        .clone()
        .or_else(|| crate::secrets::get("search-key"))
        .filter(|k| !k.trim().is_empty())
}

/// Brave answers `web.results[]` (or `results[]` for news); descriptions carry
/// `<strong>` tags and entities.
fn parse_brave(json: &serde_json::Value, news: bool) -> Vec<Hit> {
    let list = if news {
        &json["results"]
    } else {
        &json["web"]["results"]
    };
    let field = |r: &serde_json::Value, k: &str| text_of(r[k].as_str().unwrap_or_default());
    list.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let url = r["url"].as_str()?.to_string();
                    let mut snippet = field(r, "description");
                    let age = field(r, if r["age"].is_string() { "age" } else { "page_age" });
                    if news && !age.is_empty() {
                        snippet = format!("({age}) {snippet}");
                    }
                    Some(Hit {
                        title: field(r, "title"),
                        url,
                        snippet,
                    })
                })
                .take(MAX_RESULTS)
                .collect()
        })
        .unwrap_or_default()
}

async fn brave(shared: &Shared, key: &str, query: &str, news: bool) -> Result<Vec<Hit>, String> {
    let path = if news { "news/search" } else { "web/search" };
    let url = Url::parse_with_params(
        &format!("{}/{path}", shared.web.brave_url),
        [("q", query), ("count", "8")],
    )
    .map_err(|e| coded(SEARCH_FAILED, &e.to_string()))?;
    let res = shared
        .http
        .get(url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", key)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| coded(SEARCH_FAILED, &format!("Can't reach Brave Search: {e}")))?;
    match res.status().as_u16() {
        401 | 403 => {
            return Err(coded(
                SEARCH_NOT_CONFIGURED,
                "Brave rejected the search key; check your Brave key in Settings > AI.",
            ))
        }
        429 => {
            return Err(coded(
                SEARCH_FAILED,
                "Brave Search is rate limiting this key; try again in a minute.",
            ))
        }
        s if !(200..300).contains(&s) => {
            return Err(coded(SEARCH_FAILED, &format!("Brave Search error ({s}).")))
        }
        _ => {}
    }
    let json: serde_json::Value = res.json().await.map_err(|e| {
        coded(SEARCH_FAILED, &format!("Brave Search sent something unreadable: {e}"))
    })?;
    Ok(parse_brave(&json, news))
}

async fn ddg(shared: &Shared, query: &str) -> Result<Vec<Hit>, String> {
    let url = Url::parse_with_params(&shared.web.search_url, [("q", query)])
        .map_err(|e| coded(SEARCH_FAILED, &e.to_string()))?;
    let res = shared
        .http
        .get(url)
        .header("User-Agent", UA)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| coded(SEARCH_FAILED, &format!("Can't reach the search service: {e}")))?;
    let status = res.status();
    // The bot challenge answers 202 with an anomaly page and no results.
    if status.as_u16() == 202 {
        return Err(coded(SEARCH_BLOCKED, BLOCKED));
    }
    if !status.is_success() {
        return Err(coded(SEARCH_FAILED, &format!("Search service error ({status}).")));
    }
    let html = res.text().await.map_err(|e| coded(SEARCH_FAILED, &e.to_string()))?;
    let hits = parse_results(&html);
    let lower = html.to_ascii_lowercase();
    // An honest zero-result page says so; anything else with no results is a challenge.
    if hits.is_empty()
        && (lower.contains("anomaly") || lower.contains("captcha") || !lower.contains("no-results"))
    {
        return Err(coded(SEARCH_BLOCKED, BLOCKED));
    }
    Ok(hits)
}

/// Keyed provider when configured, else DuckDuckGo. News needs the key.
async fn run_search(shared: &Shared, query: &str, news: bool) -> Result<Vec<Hit>, String> {
    match search_key(&shared.web) {
        Some(key) => brave(shared, &key, query, news).await,
        None if news => Err(coded(
            SEARCH_NOT_CONFIGURED,
            "News search needs a free Brave Search key. Add one in Settings > AI.",
        )),
        None => ddg(shared, query).await,
    }
}

/// Settings > Web search > Test: one query, the result count or the error.
pub async fn probe(shared: &Shared) -> Result<String, String> {
    let keyed = search_key(&shared.web).is_some();
    let n = run_search(shared, "weather", false).await?.len();
    Ok(format!(
        "{n} results from {}.",
        if keyed { "Brave Search" } else { "DuckDuckGo" }
    ))
}

pub async fn search(ctx: &mut Ctx, query: &str, news: bool) -> Result<String, String> {
    ctx.tainted = true;
    let hits = run_search(&ctx.shared, query, news).await?;
    if hits.is_empty() {
        return Ok("No results found.".into());
    }
    let mut out = String::new();
    for (i, h) in hits.iter().enumerate() {
        ctx.allowed_urls.insert(h.url.clone());
        out += &format!("{}. {}\n   {}\n   {}\n", i + 1, h.title, h.url, h.snippet);
    }
    Ok(out)
}

fn blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            a.is_loopback()
                || a.is_private()
                || a.is_link_local()
                || a.is_unspecified()
                || a.is_broadcast()
        }
        IpAddr::V6(a) => {
            let s = a.segments()[0];
            a.is_loopback()
                || a.is_unspecified()
                || s & 0xffc0 == 0xfe80
                || s & 0xfe00 == 0xfc00
                || a.to_ipv4_mapped().is_some_and(|v| blocked(v.into()))
        }
    }
}

fn refused() -> String {
    "Refused: that address is on this PC or the local network.".into()
}

/// Refuses loopback, private, link-local and `.local` hosts (any resolved
/// address counts), except on carte blanche.
/// ponytail: the connection resolves the name again, so a DNS rebinding
/// attacker could still slip through; pin the resolved IP if that matters.
async fn check_host(ctx: &Ctx, url: &Url) -> Result<(), String> {
    if ctx.cfg.tier == Tier::CarteBlanche {
        return Ok(());
    }
    let host = url.host_str().ok_or("That URL has no host.")?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let ips: Vec<IpAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => vec![ip],
        Err(_) => {
            let d = host.to_ascii_lowercase();
            if d == "localhost" || d.ends_with(".localhost") || d.ends_with(".local") {
                return Err(refused());
            }
            let port = url.port_or_known_default().unwrap_or(80);
            let found = tokio::net::lookup_host((d.as_str(), port))
                .await
                .map_err(|e| format!("Can't resolve {d}: {e}"))?;
            found.map(|s| s.ip()).collect()
        }
    };
    let loopback_ok = ctx.shared.web.allow_loopback;
    if ips.is_empty()
        || ips
            .iter()
            .any(|&ip| blocked(ip) && !(loopback_ok && ip.is_loopback()))
    {
        return Err(refused());
    }
    Ok(())
}

fn parse_http(s: &str) -> Result<Url, String> {
    let u = Url::parse(s).map_err(|_| "That is not a valid URL.".to_string())?;
    match u.scheme() {
        "http" | "https" => Ok(u),
        _ => Err("Only http and https links can be fetched.".into()),
    }
}

/// Streams the body and stops at 2 MB whatever Content-Length claims.
async fn read_capped(mut res: reqwest::Response) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.to_string())? {
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_BODY {
            body.truncate(MAX_BODY);
            break;
        }
    }
    Ok(body)
}

pub async fn fetch(ctx: &mut Ctx, url: &str) -> Result<String, String> {
    let u = parse_http(url)?;
    // Exfiltration guard (conservative only; competent has full web access):
    // once outside text is in play, only open pages the
    // user or a search result named, or ask.
    let unknown =
        ctx.cfg.tier == Tier::Conservative && ctx.tainted && !ctx.allowed_urls.contains(url);
    if !crate::tools::confirm_persist(ctx, ConfirmKind::Web, unknown, "Open this page?", url).await
    {
        return Ok("The user chose not to open it.".into());
    }
    // One budget for DNS lookups and every redirect hop.
    tokio::time::timeout(Duration::from_secs(10), hops(ctx, u))
        .await
        .map_err(|_| "The page took too long to answer.".to_string())?
}

async fn hops(ctx: &mut Ctx, mut u: Url) -> Result<String, String> {
    for _ in 0..=MAX_HOPS {
        check_host(ctx, &u).await?;
        let res = ctx
            .shared
            .web
            .http
            .get(u.clone())
            .header("User-Agent", UA)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("Can't reach {u}: {e}"))?;
        if res.status().is_redirection() {
            let loc = res
                .headers()
                .get("location")
                .and_then(|l| l.to_str().ok())
                .ok_or("Redirect without a location.")?;
            u = parse_http(u.join(loc).map_err(|e| e.to_string())?.as_str())?;
            continue;
        }
        if !res.status().is_success() {
            return Err(format!("The page answered {}", res.status()));
        }
        let ctype = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !ctype.is_empty()
            && !(ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("xml"))
        {
            let kind = ctype.split(';').next().unwrap_or("").trim();
            return Err(format!("Not a text page ({kind})."));
        }
        let html = ctype.contains("html");
        let body = String::from_utf8_lossy(&read_capped(res).await?).into_owned();
        ctx.tainted = true;
        let text = if html {
            text_of(&body)
        } else {
            body.trim().to_string()
        };
        let mut chars = text.chars();
        let mut out: String = chars.by_ref().take(MAX_CHARS).collect();
        if chars.next().is_some() {
            out += "\n[truncated]";
        }
        return Ok(if out.is_empty() {
            "The page has no readable text.".into()
        } else {
            out
        });
    }
    Err("Too many redirects.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{ctx, mock_server_full};
    use crate::Answer;
    use std::sync::Arc;

    const DDG: &str = r#"<div class="result results_links"><h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fa%3Fx%3D1%26y%3D2&amp;rut=abc">Example &amp; <b>Co</b></a></h2><a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">The <b>first</b> snippet</a></div>
<div class="result"><a class="result__a" href="https://direct.test/p">Direct</a><a class="result__snippet" href="x">Second</a></div>
<div class="result--ad"><a class="result__a" href="https://duckduckgo.com/y.js?ad=1">Ad</a></div>"#;

    fn local_ctx() -> Ctx {
        let mut c = ctx();
        Arc::get_mut(&mut c.shared).unwrap().web.allow_loopback = true;
        c
    }

    #[tokio::test]
    async fn search_parses_unwraps_redirects_and_taints() {
        let (url, _rx) =
            mock_server_full("200 OK", "Content-Type: text/html\r\n", vec![DDG.into()]);
        let mut c = local_ctx();
        Arc::get_mut(&mut c.shared).unwrap().web.search_url = url;
        let out = search(&mut c, "q", false).await.unwrap();
        assert!(
            out.contains("1. Example & Co\n   https://example.com/a?x=1&y=2\n   The first snippet"),
            "{out}"
        );
        assert!(
            out.contains("2. Direct\n   https://direct.test/p\n   Second"),
            "{out}"
        );
        assert!(!out.contains("Ad"));
        assert!(c.tainted);
        assert!(c.allowed_urls.contains("https://example.com/a?x=1&y=2"));
    }

    #[test]
    fn html_to_text_strips_decodes_and_collapses() {
        let h = "<html><head><style>p{x:y}</style><SCRIPT>var a=1;</SCRIPT></head><body><p>Hi&nbsp;&lt;you&gt; &#65;&#x42;</p>\n\n<p>there</p></body></html>";
        assert_eq!(text_of(h), "Hi <you> AB there");
    }

    #[tokio::test]
    async fn fetch_caps_chars_and_passes_plain_text() {
        let (url, _rx) = mock_server_full(
            "200 OK",
            "Content-Type: text/plain\r\n",
            vec!["a".repeat(9000), "{\"k\": 1}".into()],
        );
        let mut c = local_ctx();
        let out = fetch(&mut c, &url).await.unwrap();
        assert_eq!(out.chars().count(), 8000 + "\n[truncated]".len());
        assert!(c.tainted);
        c.allowed_urls.insert(url.clone());
        assert_eq!(fetch(&mut c, &url).await.unwrap(), "{\"k\": 1}");
    }

    #[tokio::test]
    async fn non_text_content_is_refused() {
        let (url, _rx) = mock_server_full(
            "200 OK",
            "Content-Type: image/png
",
            vec!["xx".into()],
        );
        let mut c = local_ctx();
        let err = fetch(&mut c, &url).await.unwrap_err();
        assert_eq!(err, "Not a text page (image/png).");
    }

    #[tokio::test]
    async fn body_is_capped_at_two_megabytes() {
        let (url, _rx) = mock_server_full("200 OK", "", vec!["a".repeat(3 * 1024 * 1024)]);
        let res = crate::testutil::http().get(&url).send().await.unwrap();
        assert_eq!(read_capped(res).await.unwrap().len(), MAX_BODY);
    }

    #[tokio::test]
    async fn private_hosts_are_refused_including_by_redirect() {
        let mut c = ctx(); // loopback not allowed
        for u in [
            "http://127.0.0.1:1/",
            "http://localhost/",
            "http://10.1.2.3/",
            "http://[::1]/",
            "http://printer.local/",
        ] {
            let err = fetch(&mut c, u).await.unwrap_err();
            assert!(err.starts_with("Refused"), "{u}: {err}");
        }
        assert!(fetch(&mut c, "ftp://example.com/").await.is_err());
        let (url, _rx) = mock_server_full(
            "302 Found",
            "Location: http://169.254.169.254/latest\r\n",
            vec![String::new()],
        );
        let mut c = local_ctx();
        let err = fetch(&mut c, &url).await.unwrap_err();
        assert!(err.starts_with("Refused"), "{err}");
    }

    #[test]
    fn address_classes() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.5.5",
            "192.168.1.1",
            "169.254.1.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fd00::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(blocked(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "172.32.0.1", "2606:4700::1111"] {
            assert!(!blocked(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn urls_in_request_text() {
        assert_eq!(
            urls_in("read https://a.com/x, and (http://b.org)."),
            ["https://a.com/x", "http://b.org"]
        );
    }

    #[tokio::test]
    async fn tainted_fetch_of_unknown_url_asks_known_does_not() {
        let (url, _rx) = mock_server_full(
            "200 OK",
            "Content-Type: text/plain\r\n",
            vec!["hello".into(), "hello".into()],
        );
        let mut c = local_ctx();
        c.tainted = true;
        let shared = c.shared.clone();
        let u = url.clone();
        let running = tokio::spawn(async move {
            let r = fetch(&mut c, &u).await;
            (c, r)
        });
        tokio::task::yield_now().await;
        shared.bridge.answer(1, Answer::Confirm(false));
        let (mut c, r) = running.await.unwrap();
        assert!(r.unwrap().contains("not to open"));
        // Allowed (search result or user text): no card, fetch works.
        c.allowed_urls.insert(url.clone());
        assert_eq!(fetch(&mut c, &url).await.unwrap(), "hello");
        // Carte blanche skips the card entirely.
        c.allowed_urls.clear();
        c.cfg.tier = Tier::CarteBlanche;
        assert_eq!(fetch(&mut c, &url).await.unwrap(), "hello");
    }

    fn with_search(c: &mut Ctx, url: String, key: Option<&str>) {
        let w = &mut Arc::get_mut(&mut c.shared).unwrap().web;
        w.search_url = url.clone();
        w.brave_url = url;
        w.key = key.map(str::to_string);
    }

    #[tokio::test]
    async fn ddg_bot_challenge_is_blocked_not_no_results() {
        for (status, body) in [
            ("202 Accepted", "<html><script src=\"/anomaly.js\"></script></html>"),
            ("200 OK", "<html>Please complete the captcha</html>"),
            ("200 OK", "<html><body>nothing here</body></html>"),
        ] {
            let (url, _rx) = mock_server_full(status, "Content-Type: text/html\r\n", vec![body.into()]);
            let mut c = local_ctx();
            with_search(&mut c, url, None);
            let e = search(&mut c, "q", false).await.unwrap_err();
            assert!(e.starts_with("SEARCH_BLOCKED: DuckDuckGo is blocking"), "{e}");
        }
        let page = r#"<div class="no-results">No results.</div>"#;
        let (url, _rx) = mock_server_full("200 OK", "", vec![page.into()]);
        let mut c = local_ctx();
        with_search(&mut c, url, None);
        assert_eq!(search(&mut c, "q", false).await.unwrap(), "No results found.");
    }

    #[tokio::test]
    async fn brave_json_parses_and_strips_markup() {
        let json = r#"{"web":{"results":[{"title":"Rust &amp; Co","url":"https://r.test/a","description":"The <strong>best</strong> lang"}]}}"#;
        let (url, rx) = mock_server_full("200 OK", "Content-Type: application/json\r\n", vec![json.into()]);
        let mut c = local_ctx();
        with_search(&mut c, url, Some("k"));
        let out = search(&mut c, "rust", false).await.unwrap();
        assert!(out.contains("1. Rust & Co\n   https://r.test/a\n   The best lang"), "{out}");
        assert!(c.tainted && c.allowed_urls.contains("https://r.test/a"));
        drop(rx);
    }

    #[tokio::test]
    async fn brave_news_shows_age_and_bad_keys_and_limits_have_codes() {
        let news = r#"{"results":[{"title":"T","url":"https://n.test/","description":"d","age":"2 hours ago"}]}"#;
        let (url, _rx) = mock_server_full("200 OK", "", vec![news.into()]);
        let mut c = local_ctx();
        with_search(&mut c, url, Some("k"));
        let out = search(&mut c, "q", true).await.unwrap();
        assert!(out.contains("(2 hours ago) d"), "{out}");
        for (status, code, hint) in [
            ("401 Unauthorized", "SEARCH_NOT_CONFIGURED", "check your Brave key"),
            ("403 Forbidden", "SEARCH_NOT_CONFIGURED", "check your Brave key"),
            ("429 Too Many Requests", "SEARCH_FAILED", "try again in a minute"),
        ] {
            let (url, _rx) = mock_server_full(status, "", vec!["{}".into()]);
            let mut c = local_ctx();
            with_search(&mut c, url, Some("k"));
            let e = search(&mut c, "q", false).await.unwrap_err();
            assert!(e.starts_with(code) && e.contains(hint), "{e}");
        }
    }

    #[tokio::test]
    async fn news_without_a_key_is_not_configured() {
        let mut c = local_ctx();
        with_search(&mut c, "http://127.0.0.1:1".into(), None);
        // No stored key exists for the test service.
        if crate::secrets::get("search-key").is_none() {
            let e = search(&mut c, "q", true).await.unwrap_err();
            assert!(e.starts_with("SEARCH_NOT_CONFIGURED: News search needs"), "{e}");
        }
    }

    #[tokio::test]
    async fn competent_fetch_of_unknown_url_never_asks_conservative_does() {
        let (url, _rx) = mock_server_full("200 OK", "Content-Type: text/plain\r\n", vec!["hi".into()]);
        let mut c = local_ctx();
        c.tainted = true;
        c.cfg.tier = Tier::Competent;
        assert_eq!(fetch(&mut c, &url).await.unwrap(), "hi");
    }

    /// Documents why keyless search needs a key: DDG answers bots with 202.
    #[tokio::test]
    #[ignore = "network: hits duckduckgo.com"]
    async fn live_ddg_blocks_automated_requests() {
        let c = ctx();
        let e = ddg(&c.shared, "rust").await.unwrap_err();
        assert!(e.starts_with("SEARCH_BLOCKED"), "{e}");
    }
}
