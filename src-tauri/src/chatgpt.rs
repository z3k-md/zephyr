//! Sign in with ChatGPT: Ask AI on the user's own ChatGPT plan, with no API key. OAuth with
//! PKCE through the browser and a one-off listener on 127.0.0.1; the first sign-in registers
//! the app as the user's agent (`dynamic_agent_client`) and later ones reuse the issued
//! client id. Tokens stay in the keychain (a private file in dev builds), the access token
//! only in memory, and every call goes straight from here to OpenAI.
//!
//! Follows https://developers.openai.com/siwc/token-sharing-open-source.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;

const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const REVOKE: &str = "https://auth.openai.com/api/accounts/oauth/revoke";
const JWKS: &str = "https://auth.openai.com/.well-known/jwks.json";
pub const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const AGENT_NAME: &str = "Zephyr";
/// Where ChatGPT shows and limits what the app uses of the plan.
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";

const SECRETS: &str = "com.zephyr.app.chatgpt";
const REGISTRATION: &str = "registration";
const HOST_ID: &str = "host-id";

/// Loopback ports for the browser to come back to; only the port may change between
/// sign-ins.
const CALLBACK_PORTS: [u16; 4] = [1455, 1456, 1457, 1458];

/// The saved registration for the signed-in ChatGPT account. The refresh token rotates on
/// every refresh, so the record is rewritten each time. It stays small: Windows' Credential
/// Manager holds at most 1280 characters, too few for an ID token as well.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Registration {
    client_id: String,
    subject: String,
    email: String,
    name: String,
    refresh_token: String,
    scopes: Vec<String>,
}

/// What Settings shows about the signed-in account.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub email: String,
    pub name: String,
    /// False when sign-in worked but plan use wasn't granted.
    pub plan: bool,
}

static ACCESS: Mutex<Option<(String, Instant)>> = Mutex::new(None);
static SIGNING_IN: AtomicBool = AtomicBool::new(false);
/// One refresh at a time: the refresh token rotates, so two at once would race.
static REFRESHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A form-encoded body, as OAuth endpoints take.
fn form(fields: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish()
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(AGENT_NAME)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| err.to_string())
}

fn load() -> Option<Registration> {
    let text = crate::secrets::get(SECRETS, REGISTRATION).ok().flatten()?;
    serde_json::from_str(&text).ok()
}

fn save(registration: &Registration) -> Result<(), String> {
    let text = serde_json::to_string(registration).map_err(|err| err.to_string())?;
    crate::secrets::set(SECRETS, REGISTRATION, Some(&text))
}

/// This computer's stable, opaque id for OpenAI (not a credential), made once.
fn host_id() -> Result<String, String> {
    if let Some(id) = crate::secrets::get(SECRETS, HOST_ID)? {
        return Ok(id);
    }
    let id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    crate::secrets::set(SECRETS, HOST_ID, Some(&id))?;
    Ok(id)
}

fn random() -> String {
    data_encoding::BASE64URL_NOPAD.encode(uuid::Uuid::new_v4().as_bytes())
        + &data_encoding::BASE64URL_NOPAD.encode(uuid::Uuid::new_v4().as_bytes())
}

/// The signed-in account, if any (a registration without tokens is signed out).
pub fn account() -> Option<Account> {
    let registration = load()?;
    (!registration.refresh_token.is_empty()).then(|| Account {
        email: registration.email.clone(),
        name: registration.name.clone(),
        plan: registration.scopes.iter().any(|scope| scope == PLAN_SCOPE),
    })
}

pub fn cancel_sign_in() {
    SIGNING_IN.store(false, Ordering::SeqCst);
}

/// Signs in through the browser. A returning account reuses its issued client id and goes
/// straight back; the first time, OpenAI asks the user to approve the app.
pub async fn sign_in(open: impl FnOnce(&str) -> Result<(), String>) -> Result<Account, String> {
    let server = CALLBACK_PORTS
        .iter()
        .find_map(|port| tiny_http::Server::http(("127.0.0.1", *port)).ok())
        .ok_or("Couldn't open a local port for sign-in")?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|addr| addr.port())
        .ok_or("Couldn't open a local port for sign-in")?;
    let redirect = format!("http://127.0.0.1:{port}/auth/callback");

    let saved = load();
    let returning = saved.as_ref().filter(|saved| !saved.client_id.is_empty());
    let state = random();
    let nonce = random();
    let verifier = random();
    let challenge =
        data_encoding::BASE64URL_NOPAD.encode(&sha2::Sha256::digest(verifier.as_bytes()));
    let mut url = url::Url::parse(AUTHORIZE).map_err(|err| err.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair(
                "client_id",
                returning.map_or("dynamic_agent_client", |saved| &saved.client_id),
            )
            .append_pair("ext_agent_host_id", &host_id()?)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect)
            .append_pair("scope", SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge);
        match returning {
            Some(saved) => {
                if !saved.email.is_empty() {
                    query.append_pair("login_hint", &saved.email);
                }
            }
            None => {
                query.append_pair("agent_name_hint", AGENT_NAME);
            }
        }
    }
    SIGNING_IN.store(true, Ordering::SeqCst);
    open(url.as_str())?;

    let expected_state = state.clone();
    let callback =
        tauri::async_runtime::spawn_blocking(move || wait_for_callback(&server, &expected_state))
            .await
            .map_err(|err| err.to_string())?;
    SIGNING_IN.store(false, Ordering::SeqCst);
    let (code, issued) = callback?;

    let client_id = match (returning, issued) {
        (Some(saved), Some(issued)) if issued != saved.client_id => {
            return Err("ChatGPT returned a different app registration; sign in again".into());
        }
        (Some(saved), _) => saved.client_id.clone(),
        (None, Some(issued)) if issued != "dynamic_agent_client" => issued,
        (None, _) => return Err("ChatGPT didn't finish registering Zephyr; try again".into()),
    };

    let tokens = token_request(&[
        ("grant_type", "authorization_code"),
        ("client_id", &client_id),
        ("code", &code),
        ("code_verifier", &verifier),
        ("redirect_uri", &redirect),
        ("resource", RESOURCE),
    ])
    .await?;
    let id_token = tokens["id_token"]
        .as_str()
        .ok_or("ChatGPT's reply had no ID token")?;
    let claims = verify_id_token(id_token, &client_id, &nonce).await?;
    let subject = claims["sub"].as_str().unwrap_or_default().to_string();
    if let Some(saved) = returning
        && !saved.subject.is_empty()
        && saved.subject != subject
    {
        return Err("That's a different ChatGPT account than the one saved here".into());
    }
    let registration = Registration {
        client_id,
        subject,
        email: claims["email"].as_str().unwrap_or_default().to_string(),
        name: claims["name"].as_str().unwrap_or_default().to_string(),
        refresh_token: tokens["refresh_token"]
            .as_str()
            .ok_or("ChatGPT's reply had no refresh token")?
            .to_string(),
        scopes: scopes(&tokens),
    };
    save(&registration)?;
    remember_access(&tokens);
    account().ok_or_else(|| "Couldn't save the sign-in".into())
}

/// Waits up to five minutes for the browser: the code, and on a first sign-in the client
/// id OpenAI issued.
fn wait_for_callback(
    server: &tiny_http::Server,
    state: &str,
) -> Result<(String, Option<String>), String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if !SIGNING_IN.load(Ordering::SeqCst) {
            return Err("Sign-in was cancelled".into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("Sign-in timed out. Try again.".into());
        }
        let Ok(Some(request)) = server.recv_timeout(left.min(Duration::from_secs(1))) else {
            continue;
        };
        if !request.url().starts_with("/auth/callback") {
            let _ = request.respond(tiny_http::Response::empty(404));
            continue;
        }
        let url = url::Url::parse(&format!("http://127.0.0.1{}", request.url())).ok();
        let param = |name: &str| {
            url.as_ref().and_then(|url| {
                url.query_pairs()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
            })
        };
        let result = if param("state").as_deref() != Some(state) {
            Err("The sign-in didn't match; try again".to_string())
        } else if let Some(error) = param("error") {
            Err(if error == "access_denied" {
                "Sign-in was declined in the browser".to_string()
            } else {
                format!(
                    "ChatGPT sign-in failed: {}",
                    param("error_description").unwrap_or(error)
                )
            })
        } else if let Some(code) = param("code") {
            Ok((code, param("client_id")))
        } else {
            Err("The browser came back without a sign-in code".to_string())
        };
        let page = if result.is_ok() {
            "Zephyr is connected to ChatGPT. You can close this tab."
        } else {
            "Zephyr couldn't finish signing in. Go back to Zephyr and try again."
        };
        let header = tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8")
            .expect("static header");
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>Zephyr</title><body style=\"font:16px \
             -apple-system,system-ui;background:#16131f;color:#ececee;display:grid;\
             place-items:center;height:100vh;margin:0\"><p>{page}</p>"
        );
        let _ = request.respond(tiny_http::Response::from_string(html).with_header(header));
        return result;
    }
}

/// Posts a form to the token endpoint. Errors keep OpenAI's code for the caller.
async fn token_request(fields: &[(&str, &str)]) -> Result<Value, String> {
    let response = http()?
        .post(TOKEN)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form(fields))
        .send()
        .await
        .map_err(|err| format!("Couldn't reach ChatGPT: {err}"))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    // OAuth's flat {"error", "error_description"}, or OpenAI's {"error": {"code", "message"}}.
    let code = body["error"]
        .as_str()
        .or_else(|| body["error"]["code"].as_str())
        .unwrap_or_default();
    let detail = body["error_description"]
        .as_str()
        .or_else(|| body["error"]["message"].as_str())
        .unwrap_or(code);
    Err(if detail.is_empty() {
        format!("ChatGPT sign-in failed (HTTP {status})")
    } else {
        format!("{code}: {detail}")
    })
}

/// Checks the ID token's signature against OpenAI's published keys, its issuer, audience,
/// expiry and nonce, and returns its claims.
async fn verify_id_token(token: &str, client_id: &str, nonce: &str) -> Result<Value, String> {
    let header = jsonwebtoken::decode_header(token).map_err(|err| err.to_string())?;
    let keys: JwkSet = http()?
        .get(JWKS)
        .send()
        .await
        .map_err(|err| err.to_string())?
        .json()
        .await
        .map_err(|err| err.to_string())?;
    let kid = header.kid.ok_or("The ID token names no signing key")?;
    let key = keys
        .find(&kid)
        .ok_or("The ID token's signing key isn't published")?;
    let key = DecodingKey::from_jwk(key).map_err(|err| err.to_string())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&[ISSUER]);
    let claims = jsonwebtoken::decode::<Value>(token, &key, &validation)
        .map_err(|err| format!("The ID token didn't check out: {err}"))?
        .claims;
    if claims["nonce"].as_str() != Some(nonce) {
        return Err("The ID token wasn't for this sign-in".into());
    }
    Ok(claims)
}

fn scopes(tokens: &Value) -> Vec<String> {
    tokens["scope"]
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn remember_access(tokens: &Value) {
    let Some(access) = tokens["access_token"].as_str() else {
        return;
    };
    let lifetime = tokens["expires_in"].as_u64().unwrap_or(3600);
    // Renew a few minutes early so a long answer never starts on a dying token.
    let expires = Instant::now() + Duration::from_secs(lifetime.saturating_sub(300));
    if let Ok(mut slot) = ACCESS.lock() {
        *slot = Some((access.to_string(), expires));
    }
}

/// A live access token, refreshed when it's near expiry.
pub async fn access_token() -> Result<String, String> {
    if let Ok(slot) = ACCESS.lock()
        && let Some((token, expires)) = slot.as_ref()
        && Instant::now() < *expires
    {
        return Ok(token.clone());
    }
    let _refreshing = REFRESHING.lock().await;
    // Another ask may have refreshed while this one waited.
    if let Ok(slot) = ACCESS.lock()
        && let Some((token, expires)) = slot.as_ref()
        && Instant::now() < *expires
    {
        return Ok(token.clone());
    }
    let mut registration = load()
        .filter(|saved| !saved.refresh_token.is_empty())
        .ok_or("Sign in with ChatGPT in Settings > AI")?;
    let tokens = match token_request(&[
        ("grant_type", "refresh_token"),
        ("client_id", &registration.client_id),
        ("refresh_token", &registration.refresh_token),
        ("resource", RESOURCE),
    ])
    .await
    {
        Ok(tokens) => tokens,
        Err(err) => {
            const UNUSABLE: &[&str] = &[
                "invalid_grant",
                "invalid_refresh_token",
                "token_expired",
                "refresh_token_expired",
                "refresh_token_invalidated",
                "refresh_token_reused",
            ];
            if UNUSABLE.iter().any(|code| err.starts_with(code)) {
                registration.refresh_token.clear();
                let _ = save(&registration);
                return Err("Your ChatGPT sign-in ended. Sign in again in Settings > AI.".into());
            }
            return Err(err);
        }
    };
    if let Some(refresh) = tokens["refresh_token"].as_str() {
        registration.refresh_token = refresh.to_string();
    }
    if tokens["scope"].is_string() {
        registration.scopes = scopes(&tokens);
    }
    save(&registration)?;
    let access = tokens["access_token"]
        .as_str()
        .ok_or("ChatGPT didn't return an access token")?
        .to_string();
    remember_access(&tokens);
    Ok(access)
}

/// The models this account can use, as (slug, display name), in OpenAI's order.
pub async fn models() -> Result<Vec<(String, String)>, String> {
    let token = access_token().await?;
    let response = http()?
        .get(format!("{RESOURCE}/models"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|err| format!("Couldn't reach ChatGPT: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("ChatGPT couldn't list its models (HTTP {status})"));
    }
    let body: Value = response.json().await.map_err(|err| err.to_string())?;
    Ok(body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| model["visibility"].as_str().unwrap_or("list") == "list")
        .filter_map(|model| {
            let slug = model["slug"].as_str()?.to_string();
            let name = model["display_name"].as_str().unwrap_or(&slug).to_string();
            Some((slug, name))
        })
        .collect())
}

/// Signs out: revokes the renewable session, then forgets the registration. This
/// computer's host id stays.
pub async fn sign_out() -> Result<(), String> {
    if let Ok(mut slot) = ACCESS.lock() {
        *slot = None;
    }
    let Some(registration) = load() else {
        return Ok(());
    };
    let revoked = if registration.refresh_token.is_empty() {
        true
    } else {
        let client = http()?;
        let body = form(&[
            ("token", registration.refresh_token.as_str()),
            ("token_type_hint", "refresh_token"),
            ("client_id", registration.client_id.as_str()),
        ]);
        let mut ok = false;
        for wait in [0, 1, 3] {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            let request = client
                .post(REVOKE)
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(body.clone());
            match request.send().await {
                Ok(response) if response.status().is_success() => {
                    ok = true;
                    break;
                }
                Ok(response) if !response.status().is_server_error() => break,
                _ => {}
            }
        }
        ok
    };
    // Forgetting the registration too lets the next sign-in be any account, or recover
    // after Zephyr was disconnected in ChatGPT's settings.
    crate::secrets::set(SECRETS, REGISTRATION, None)?;
    if revoked {
        Ok(())
    } else {
        Err("Signed out here, but ChatGPT didn't confirm it. You can also disconnect Zephyr in ChatGPT's settings.".into())
    }
}

/// A plain sentence for an error from ChatGPT plan use, by its code.
pub fn explain(code: &str, fallback: &str) -> String {
    match code {
        "subscription_sharing_usage_limit_exceeded" => format!(
            "You've reached the ChatGPT plan limit for Zephyr. Check or raise it at {USAGE_URL}"
        ),
        "subscription_sharing_user_not_eligible" => {
            "This ChatGPT account can't share its plan with apps (Free plans can't).".into()
        }
        "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable" => {
            "ChatGPT couldn't check your plan just now. Try again in a moment.".into()
        }
        "subscription_sharing_invalid_user" => {
            "ChatGPT didn't accept this sign-in. Sign in again in Settings > AI.".into()
        }
        _ => fallback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_errors_read_plainly() {
        assert!(explain("subscription_sharing_usage_limit_exceeded", "x").contains(USAGE_URL));
        assert!(explain("subscription_sharing_user_not_eligible", "x").contains("Free"));
        assert_eq!(explain("something_else", "fallback"), "fallback");
    }

    #[test]
    fn scopes_split_on_spaces() {
        let tokens = serde_json::json!({"scope": "openid email chatgpt.tokens.use.direct"});
        assert_eq!(
            scopes(&tokens),
            ["openid", "email", "chatgpt.tokens.use.direct"]
        );
    }
}
