//! Microsoft sign-in for Outlook, Hotmail and Live accounts. Microsoft turned
//! off app passwords for these in September 2024, so SMTP needs an OAuth
//! token. The device-code flow needs no redirect server: the user enters a
//! short code at microsoft.com/devicelogin in their own browser.

use crate::protocol::{emit, Out};
use crate::secrets;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// From the Entra app registration, set when building (see the plan's Preflight).
const CLIENT_ID: Option<&str> = option_env!("BLOOM_AI_OUTLOOK_CLIENT_ID");
const AUTHORITY: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const SCOPES: &str =
    "https://outlook.office.com/SMTP.Send https://outlook.office.com/IMAP.AccessAsUser.All offline_access";

/// The current access token and when it expires. Memory only.
static TOKEN: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn client_id() -> Result<&'static str, String> {
    CLIENT_ID.ok_or_else(|| {
        "This build of Bloom has no Outlook sign-in (BLOOM_AI_OUTLOOK_CLIENT_ID was not set)."
            .into()
    })
}

fn oauth_error(reply: &Value) -> String {
    reply["error_description"]
        .as_str()
        .or(reply["error"].as_str())
        .unwrap_or("Microsoft sign-in failed.")
        .to_string()
}

async fn post(http: &reqwest::Client, url: &str, form: &[(&str, &str)]) -> Result<Value, String> {
    http.post(url)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("Can't reach Microsoft: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Microsoft sent something unreadable: {e}"))
}

/// Keeps the tokens from a token reply: the refresh token in Credential
/// Manager, the access token in memory. Returns the access token.
fn keep(reply: &Value) -> Result<String, String> {
    let access = reply["access_token"]
        .as_str()
        .ok_or_else(|| oauth_error(reply))?;
    if let Some(refresh) = reply["refresh_token"].as_str() {
        secrets::set("outlook-refresh", refresh)?;
    }
    let ttl = reply["expires_in"].as_u64().unwrap_or(3600);
    *TOKEN.lock().unwrap() = Some((
        access.to_string(),
        Instant::now() + Duration::from_secs(ttl),
    ));
    Ok(access.to_string())
}

pub async fn login(http: &reqwest::Client) -> Result<(), String> {
    let client_id = client_id()?;
    let start = post(
        http,
        &format!("{AUTHORITY}/devicecode"),
        &[("client_id", client_id), ("scope", SCOPES)],
    )
    .await?;
    let device_code = start["device_code"]
        .as_str()
        .ok_or_else(|| oauth_error(&start))?
        .to_string();
    emit(&Out::LoginCode {
        url: start["verification_uri"]
            .as_str()
            .unwrap_or("https://microsoft.com/devicelogin")
            .into(),
        code: start["user_code"].as_str().unwrap_or_default().into(),
    });
    let mut interval = start["interval"].as_u64().unwrap_or(5);
    let deadline =
        Instant::now() + Duration::from_secs(start["expires_in"].as_u64().unwrap_or(900));
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let reply = post(
            http,
            &format!("{AUTHORITY}/token"),
            &[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", &device_code),
            ],
        )
        .await?;
        match reply["error"].as_str() {
            None => return keep(&reply).map(|_| ()),
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            Some(_) => return Err(oauth_error(&reply)),
        }
    }
    Err("Sign-in timed out. Try again.".into())
}

/// A valid access token, refreshed when it is about to expire.
pub async fn access_token(http: &reqwest::Client) -> Result<String, String> {
    let cached = TOKEN.lock().unwrap().clone();
    if let Some((token, until)) = cached {
        if until > Instant::now() + Duration::from_secs(60) {
            return Ok(token);
        }
    }
    let refresh =
        secrets::get("outlook-refresh").ok_or("Sign in with Microsoft in Settings > AI first.")?;
    let reply = post(
        http,
        &format!("{AUTHORITY}/token"),
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id()?),
            ("refresh_token", &refresh),
            ("scope", SCOPES),
        ],
    )
    .await?;
    keep(&reply)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn kept_tokens_are_reused_until_they_expire() {
        let token = keep(&json!({ "access_token": "abc", "expires_in": 3600 })).unwrap();
        assert_eq!(token, "abc");
        // Served from memory: no network, no refresh token needed.
        assert_eq!(
            access_token(&crate::testutil::http()).await,
            Ok("abc".into())
        );
    }

    #[test]
    fn errors_prefer_the_description() {
        assert_eq!(
            oauth_error(&json!({ "error": "x", "error_description": "Bad code" })),
            "Bad code"
        );
        assert_eq!(
            oauth_error(&json!({ "error": "expired_token" })),
            "expired_token"
        );
        assert!(keep(&json!({ "error": "invalid_grant" })).is_err());
    }
}
