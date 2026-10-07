//! The Supabase calls sync needs: email-code auth and the `zephyr` schema over PostgREST.
//! Requests go from Rust only; the webview never sees a token.

use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::keys::{Envelope, Key32};

pub const URL: &str = "https://vlyelwpkjgrfjahoipko.supabase.co";
/// Publishable by design: row-level security, not this key, guards the data.
pub const PUBLISHABLE_KEY: &str = "sb_publishable_xj27Hok09y4grU6NVALTKw_bglh7YvO";

fn http() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(20))
                .user_agent("Zephyr")
                .build()
                .map_err(|err| err.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

async fn send(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = request
        .header("apikey", PUBLISHABLE_KEY)
        .send()
        .await
        .map_err(|err| {
            if err.is_connect() || err.is_timeout() {
                "Couldn't reach the sync server".to_string()
            } else {
                err.to_string()
            }
        })?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    if status.is_success() {
        return Ok(body);
    }
    let message = body["msg"]
        .as_str()
        .or(body["message"].as_str())
        .or(body["error_description"].as_str())
        .or(body["error"].as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("HTTP {status}"));
    Err(message)
}

fn json_body(request: reqwest::RequestBuilder, body: &Value) -> reqwest::RequestBuilder {
    request
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
}

// Auth

#[derive(Debug, Clone)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_in: u64,
    pub user_id: String,
    pub email: String,
}

fn tokens(body: &Value) -> Result<Tokens, String> {
    let text = |value: &Value| value.as_str().map(str::to_string);
    Ok(Tokens {
        access: text(&body["access_token"]).ok_or("The sign-in reply had no token")?,
        refresh: text(&body["refresh_token"]).ok_or("The sign-in reply had no refresh token")?,
        expires_in: body["expires_in"].as_u64().unwrap_or(3600),
        user_id: text(&body["user"]["id"]).ok_or("The sign-in reply had no user")?,
        email: text(&body["user"]["email"]).unwrap_or_default(),
    })
}

/// The browser address that starts Google sign-in, returning to `redirect` with a code.
pub fn google_url(redirect: &str, challenge: &str) -> String {
    format!(
        "{URL}/auth/v1/authorize?provider=google&redirect_to={}&code_challenge={challenge}&code_challenge_method=s256",
        urlencoding::encode(redirect)
    )
}

/// Trades the code from the browser for tokens (PKCE).
pub async fn exchange_code(code: &str, verifier: &str) -> Result<Tokens, String> {
    let body = send(json_body(
        http()?.post(format!("{URL}/auth/v1/token?grant_type=pkce")),
        &json!({ "auth_code": code, "code_verifier": verifier }),
    ))
    .await?;
    tokens(&body)
}

pub async fn refresh(refresh_token: &str) -> Result<Tokens, String> {
    let body = send(json_body(
        http()?.post(format!("{URL}/auth/v1/token?grant_type=refresh_token")),
        &json!({ "refresh_token": refresh_token }),
    ))
    .await?;
    tokens(&body)
}

pub async fn logout(access: &str) -> Result<(), String> {
    send(
        http()?
            .post(format!("{URL}/auth/v1/logout"))
            .bearer_auth(access),
    )
    .await
    .map(|_| ())
}

// Rows

fn rest(
    method: reqwest::Method,
    access: &str,
    path: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let request = http()?
        .request(method.clone(), format!("{URL}/rest/v1/{path}"))
        .bearer_auth(access);
    Ok(if method == reqwest::Method::GET {
        request.header("Accept-Profile", "zephyr")
    } else {
        request
            .header("Content-Profile", "zephyr")
            .header("Accept-Profile", "zephyr")
    })
}

/// PostgREST bytea literal.
pub fn hex(bytes: &[u8]) -> String {
    format!("\\x{}", data_encoding::HEXLOWER.encode(bytes))
}

pub fn unhex(value: &Value) -> Option<Vec<u8>> {
    let text = value.as_str()?.strip_prefix("\\x")?;
    data_encoding::HEXLOWER_PERMISSIVE
        .decode(text.as_bytes())
        .ok()
}

fn key32(value: &Value) -> Option<Key32> {
    unhex(value)?.try_into().ok()
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub platform: String,
    pub display_name: String,
    #[serde(skip)]
    pub public_key: Key32,
    pub last_seen_at: Option<String>,
    pub revoked_at: Option<String>,
    pub created_at: Option<String>,
}

fn device(row: &Value) -> Option<Device> {
    Some(Device {
        id: row["id"].as_str()?.to_string(),
        platform: row["platform"].as_str().unwrap_or_default().to_string(),
        display_name: row["display_name"].as_str().unwrap_or_default().to_string(),
        public_key: key32(&row["public_key"])?,
        last_seen_at: row["last_seen_at"].as_str().map(str::to_string),
        revoked_at: row["revoked_at"].as_str().map(str::to_string),
        created_at: row["created_at"].as_str().map(str::to_string),
    })
}

pub async fn devices(access: &str) -> Result<Vec<Device>, String> {
    let body = send(rest(
        reqwest::Method::GET,
        access,
        "devices?select=id,platform,display_name,public_key,last_seen_at,revoked_at,created_at&order=created_at",
    )?)
    .await?;
    Ok(body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(device)
        .collect())
}

pub async fn add_device(
    access: &str,
    platform: &str,
    name: &str,
    public_key: &Key32,
) -> Result<Device, String> {
    let body = send(
        json_body(
            rest(reqwest::Method::POST, access, "devices")?,
            &json!({ "platform": platform, "display_name": name, "public_key": hex(public_key) }),
        )
        .header("Prefer", "return=representation"),
    )
    .await?;
    body.as_array()
        .and_then(|rows| rows.first())
        .and_then(device)
        .ok_or_else(|| "The server didn't return the new device".to_string())
}

pub async fn update_device(access: &str, id: &str, fields: &Value) -> Result<(), String> {
    send(json_body(
        rest(
            reqwest::Method::PATCH,
            access,
            &format!("devices?id=eq.{id}"),
        )?,
        fields,
    ))
    .await
    .map(|_| ())
}

#[derive(Debug, Clone)]
pub struct StoredEnvelope {
    pub key_gen: i32,
    pub kind: String,
    pub recipient_device_id: Option<String>,
    pub envelope: Envelope,
}

pub async fn envelopes(access: &str) -> Result<Vec<StoredEnvelope>, String> {
    let body = send(rest(
        reqwest::Method::GET,
        access,
        "key_envelopes?select=key_gen,kind,recipient_device_id,ephemeral_public_key,nonce,ciphertext&order=key_gen",
    )?)
    .await?;
    Ok(body
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(StoredEnvelope {
                key_gen: row["key_gen"].as_i64()? as i32,
                kind: row["kind"].as_str()?.to_string(),
                recipient_device_id: row["recipient_device_id"].as_str().map(str::to_string),
                envelope: Envelope {
                    ephemeral_public_key: key32(&row["ephemeral_public_key"]),
                    nonce: unhex(&row["nonce"])?.try_into().ok()?,
                    ciphertext: unhex(&row["ciphertext"])?,
                },
            })
        })
        .collect())
}

pub async fn add_envelope(
    access: &str,
    key_gen: i32,
    recipient_device_id: Option<&str>,
    sender_device_id: &str,
    envelope: &Envelope,
) -> Result<(), String> {
    let body = json!({
        "key_gen": key_gen,
        "kind": if recipient_device_id.is_some() { "device" } else { "recovery" },
        "recipient_device_id": recipient_device_id,
        "sender_device_id": sender_device_id,
        "ephemeral_public_key": envelope.ephemeral_public_key.map(|key| hex(&key)),
        "nonce": hex(&envelope.nonce),
        "ciphertext": hex(&envelope.ciphertext),
    });
    send(json_body(
        rest(reqwest::Method::POST, access, "key_envelopes")?,
        &body,
    ))
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytea_round_trips_through_postgrest_hex() {
        let bytes = [0u8, 1, 0xab, 0xff];
        let text = hex(&bytes);
        assert_eq!(text, "\\x0001abff");
        assert_eq!(unhex(&json!(text)).unwrap(), bytes);
        assert_eq!(unhex(&json!("\\x0001ABFF")).unwrap(), bytes);
        assert!(unhex(&json!("0001")).is_none());
    }
}
