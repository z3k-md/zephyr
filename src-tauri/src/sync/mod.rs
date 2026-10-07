//! End-to-end encrypted sync of notes and clipboard history through the user's Supabase
//! project. The server only ever holds ciphertext.
//!
//! Step 1 is accounts and keys: email-code sign-in, a key pair per device, the account key
//! (AK) sealed to each approved device, a recovery key, and approving new devices from an
//! existing one. Record sync builds on this.

pub mod api;
pub mod keys;

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;

use keys::Key32;

const SERVICE: &str = "com.zephyr.app.sync";
const REFRESH: &str = "refresh-token";
const DEVICE_KEY: &str = "device-key";
const ACCOUNT_KEY: &str = "account-key";
const RECOVERY_KEY: &str = "recovery-key";

/// Who is signed in and as which device; ids only, nothing secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    user_id: String,
    email: String,
    device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Phase {
    SignedOut,
    /// Google sign-in is open in the browser.
    Browser,
    /// First device: the recovery key must be saved before sync is ready.
    ShowRecovery {
        recovery: String,
    },
    /// A new device waiting for another device to approve it (or for the recovery key).
    AwaitingApproval {
        words: String,
    },
    Ready,
}

struct Session {
    dir: PathBuf,
    saved: Saved,
    phase: Phase,
    access: Option<(String, Instant)>,
    last_seen_touch: Option<Instant>,
}

fn session() -> &'static Mutex<Session> {
    static SESSION: OnceLock<Mutex<Session>> = OnceLock::new();
    SESSION.get_or_init(|| {
        Mutex::new(Session {
            dir: PathBuf::new(),
            saved: Saved::default(),
            phase: Phase::SignedOut,
            access: None,
            last_seen_touch: None,
        })
    })
}

fn lock() -> Result<std::sync::MutexGuard<'static, Session>, String> {
    session()
        .lock()
        .map_err(|_| "Sync is unavailable".to_string())
}

impl Session {
    fn save(&self) {
        let path = self.dir.join("sync.json");
        let result = serde_json::to_vec_pretty(&self.saved)
            .map_err(|err| err.to_string())
            .and_then(|data| {
                fs::create_dir_all(&self.dir).map_err(|err| err.to_string())?;
                fs::write(&path, data).map_err(|err| err.to_string())
            });
        if let Err(err) = result {
            log::error!("couldn't save sync settings: {err}");
        }
    }
}

fn secret(account: &str) -> Result<Option<String>, String> {
    crate::secrets::get(SERVICE, account)
}

fn set_secret(account: &str, value: Option<&str>) -> Result<(), String> {
    crate::secrets::set(SERVICE, account, value)
}

fn key_secret(account: &str) -> Result<Option<Key32>, String> {
    Ok(secret(account)?
        .and_then(|text| STANDARD.decode(text.trim()).ok())
        .and_then(|bytes| bytes.try_into().ok()))
}

/// The stored account key as (generation, key).
fn account_key() -> Result<Option<(i32, Key32)>, String> {
    Ok(secret(ACCOUNT_KEY)?.and_then(|text| {
        let (generation, key) = text.split_once(':')?;
        let key: Key32 = STANDARD.decode(key).ok()?.try_into().ok()?;
        Some((generation.parse().ok()?, key))
    }))
}

fn store_account_key(generation: i32, key: &Key32) -> Result<(), String> {
    set_secret(
        ACCOUNT_KEY,
        Some(&format!("{generation}:{}", STANDARD.encode(key))),
    )
}

/// Restores the saved session at startup and settles where it stands, in the background.
pub fn init(dir: PathBuf) {
    if let Ok(mut session) = lock() {
        session.saved = fs::read(dir.join("sync.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        session.dir = dir;
    }
    tauri::async_runtime::spawn(async {
        let signed_in = secret(REFRESH).ok().flatten().is_some();
        if !signed_in {
            return;
        }
        if let Err(err) = settle().await {
            log::error!("sync couldn't resume: {err}");
        }
    });
}

/// A current access token, refreshing it (and the rotated refresh token) when needed.
async fn access() -> Result<String, String> {
    if let Some((token, expires)) = lock()?.access.clone()
        && Instant::now() < expires
    {
        return Ok(token);
    }
    let refresh = secret(REFRESH)?.ok_or("Sign in to sync first")?;
    let tokens = api::refresh(&refresh).await?;
    adopt(&tokens)?;
    Ok(tokens.access)
}

fn adopt(tokens: &api::Tokens) -> Result<(), String> {
    set_secret(REFRESH, Some(&tokens.refresh))?;
    let mut session = lock()?;
    // Refresh a minute early so a request never carries an expiring token.
    let life = Duration::from_secs(tokens.expires_in.saturating_sub(60).max(30));
    session.access = Some((tokens.access.clone(), Instant::now() + life));
    if session.saved.user_id != tokens.user_id {
        session.saved = Saved {
            user_id: tokens.user_id.clone(),
            email: tokens.email.clone(),
            device_id: String::new(),
        };
    } else if !tokens.email.is_empty() {
        session.saved.email = tokens.email.clone();
    }
    session.save();
    Ok(())
}

/// Ports the sign-in callback listens on, so one Supabase redirect entry covers them.
const CALLBACK_PORTS: [u16; 5] = [47195, 47196, 47197, 47198, 47199];

/// Signs in with Google in the browser. A one-off listener on 127.0.0.1 catches the
/// redirect, and the code is exchanged with a PKCE verifier only this process knows.
pub async fn google_sign_in(open: impl FnOnce(&str) -> Result<(), String>) -> Result<(), String> {
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

    let verifier = data_encoding::BASE64URL_NOPAD.encode(&keys::random32()?);
    let challenge =
        data_encoding::BASE64URL_NOPAD.encode(&sha2::Sha256::digest(verifier.as_bytes()));
    lock()?.phase = Phase::Browser;
    open(&api::google_url(&redirect, &challenge))?;

    let outcome = tauri::async_runtime::spawn_blocking(move || wait_for_code(&server))
        .await
        .map_err(|err| err.to_string())?;
    let finish = async {
        let code = outcome?;
        let tokens = api::exchange_code(&code, &verifier).await?;
        adopt(&tokens)?;
        settle().await
    };
    let result = finish.await;
    if result.is_err()
        && let Ok(mut session) = lock()
        && session.phase == Phase::Browser
    {
        session.phase = Phase::SignedOut;
    }
    result
}

/// Waits up to five minutes for the browser to come back with `?code=`.
fn wait_for_code(server: &tiny_http::Server) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if lock()
            .map(|session| session.phase != Phase::Browser)
            .unwrap_or(true)
        {
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
        let result = match (param("code"), param("error_description").or(param("error"))) {
            (Some(code), _) => Ok(code),
            (None, Some(error)) => Err(format!("Google sign-in failed: {error}")),
            (None, None) => Err("The browser came back without a sign-in code".into()),
        };
        let page = if result.is_ok() {
            "Signed in to Zephyr. You can close this tab."
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

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// The computer's name as its owner sees it, e.g. "Zach's MacBook Air".
fn computer_name() -> String {
    #[cfg(target_os = "macos")]
    let name = std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
    #[cfg(not(target_os = "macos"))]
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok();
    let name: String = name
        .unwrap_or_default()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(64)
        .collect();
    if name.trim().is_empty() {
        "This computer".into()
    } else {
        name.trim().to_string()
    }
}

/// This device's key pair, made on first use.
fn device_secret() -> Result<x25519_dalek::StaticSecret, String> {
    let bytes = match key_secret(DEVICE_KEY)? {
        Some(bytes) => bytes,
        None => {
            let bytes = keys::random32()?;
            set_secret(DEVICE_KEY, Some(&STANDARD.encode(bytes)))?;
            bytes
        }
    };
    Ok(keys::device_secret(bytes))
}

/// Makes sure this install has a live device row, registering one if needed.
async fn ensure_device(token: &str) -> Result<String, String> {
    let secret = device_secret()?;
    let public = keys::public_of(&secret);
    let known = lock()?.saved.device_id.clone();
    let devices = api::devices(token).await?;
    if let Some(device) = devices
        .iter()
        .find(|device| device.id == known && device.public_key == public)
    {
        if device.revoked_at.is_some() {
            // Removed from another device: this install starts over as a new one.
            set_secret(ACCOUNT_KEY, None)?;
            set_secret(RECOVERY_KEY, None)?;
        } else {
            return Ok(device.id.clone());
        }
    }
    let device = if let Some(device) = devices
        .iter()
        .find(|device| device.public_key == public && device.revoked_at.is_none())
    {
        device.clone()
    } else {
        api::add_device(token, platform(), &computer_name(), &public).await?
    };
    let mut session = lock()?;
    session.saved.device_id = device.id.clone();
    session.save();
    Ok(device.id)
}

/// Works out where this device stands and moves to the right phase: ready, first-time
/// setup, or waiting for approval.
async fn settle() -> Result<(), String> {
    let token = access().await?;
    let device_id = ensure_device(&token).await?;
    let user_id = lock()?.saved.user_id.clone();
    let envelopes = api::envelopes(&token).await?;
    let current = envelopes.iter().map(|stored| stored.key_gen).max();

    if let Some((generation, _)) = account_key()?
        && Some(generation) >= current
    {
        lock()?.phase = Phase::Ready;
        return Ok(());
    }

    let Some(current) = current else {
        return first_device(&token, &user_id, &device_id).await;
    };

    if let Some(mine) = envelopes.iter().find(|stored| {
        stored.key_gen == current && stored.recipient_device_id.as_deref() == Some(&device_id)
    }) {
        let account = keys::open_device_envelope(
            &device_secret()?,
            &mine.envelope,
            &user_id,
            current,
            &device_id,
        )?;
        store_account_key(current, &account)?;
        lock()?.phase = Phase::Ready;
        return Ok(());
    }

    let words = keys::device_words(&keys::public_of(&device_secret()?));
    lock()?.phase = Phase::AwaitingApproval { words };
    Ok(())
}

/// The very first device on an account makes the account key and the recovery key.
async fn first_device(token: &str, user_id: &str, device_id: &str) -> Result<(), String> {
    let account = keys::random32()?;
    let recovery = keys::random32()?;
    let secret = device_secret()?;
    let own = keys::seal_for_device(&account, &keys::public_of(&secret), user_id, 1, device_id)?;
    api::add_envelope(token, 1, Some(device_id), device_id, &own).await?;
    let backup = keys::seal_for_recovery(&account, &recovery, user_id, 1)?;
    api::add_envelope(token, 1, None, device_id, &backup).await?;
    store_account_key(1, &account)?;
    set_secret(RECOVERY_KEY, Some(&STANDARD.encode(recovery)))?;
    lock()?.phase = Phase::ShowRecovery {
        recovery: keys::format_recovery(&recovery),
    };
    Ok(())
}

/// The user saved the recovery key; sync is ready.
pub fn recovery_saved() -> Result<(), String> {
    let mut session = lock()?;
    if matches!(session.phase, Phase::ShowRecovery { .. }) {
        session.phase = Phase::Ready;
    }
    Ok(())
}

/// Unlocks a new device with the recovery key instead of another device's approval.
pub async fn use_recovery(text: &str) -> Result<(), String> {
    let recovery = keys::parse_recovery(text)?;
    let token = access().await?;
    let device_id = ensure_device(&token).await?;
    let user_id = lock()?.saved.user_id.clone();
    let envelopes = api::envelopes(&token).await?;
    let backup = envelopes
        .iter()
        .filter(|stored| stored.kind == "recovery")
        .max_by_key(|stored| stored.key_gen)
        .ok_or("This account has no recovery key yet")?;
    let account =
        keys::open_recovery_envelope(&recovery, &backup.envelope, &user_id, backup.key_gen)?;
    store_account_key(backup.key_gen, &account)?;
    set_secret(RECOVERY_KEY, Some(&STANDARD.encode(recovery)))?;
    // Seal the key to this device too, so the next sign-in here needs nothing.
    let secret = device_secret()?;
    let own = keys::seal_for_device(
        &account,
        &keys::public_of(&secret),
        &user_id,
        backup.key_gen,
        &device_id,
    )?;
    if let Err(err) =
        api::add_envelope(&token, backup.key_gen, Some(&device_id), &device_id, &own).await
    {
        log::info!("couldn't store this device's envelope: {err}");
    }
    lock()?.phase = Phase::Ready;
    Ok(())
}

/// Checks again whether another device has approved this one.
pub async fn poll() -> Result<(), String> {
    if matches!(lock()?.phase, Phase::AwaitingApproval { .. }) {
        settle().await?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub words: String,
}

/// Other devices on the account waiting for this one to approve them.
pub async fn pending() -> Result<Vec<Pending>, String> {
    if lock()?.phase != Phase::Ready {
        return Ok(Vec::new());
    }
    let Some((generation, _)) = account_key()? else {
        return Ok(Vec::new());
    };
    let token = access().await?;
    let me = lock()?.saved.device_id.clone();
    let devices = api::devices(&token).await?;
    let envelopes = api::envelopes(&token).await?;
    touch_last_seen(&token, &me).await;
    Ok(devices
        .into_iter()
        .filter(|device| device.id != me && device.revoked_at.is_none())
        .filter(|device| {
            !envelopes.iter().any(|stored| {
                stored.key_gen == generation
                    && stored.recipient_device_id.as_deref() == Some(&device.id)
            })
        })
        .map(|device| Pending {
            words: keys::device_words(&device.public_key),
            id: device.id,
            name: device.display_name,
            platform: device.platform,
        })
        .collect())
}

async fn touch_last_seen(token: &str, me: &str) {
    let due = lock()
        .map(|session| {
            session
                .last_seen_touch
                .is_none_or(|last| last.elapsed() > Duration::from_secs(300))
        })
        .unwrap_or(false);
    if !due || me.is_empty() {
        return;
    }
    let now = chrono::Utc::now().to_rfc3339();
    if api::update_device(token, me, &json!({ "last_seen_at": now }))
        .await
        .is_ok()
        && let Ok(mut session) = lock()
    {
        session.last_seen_touch = Some(Instant::now());
    }
}

/// Approves another device: seals the account key to its public key.
pub async fn approve(device_id: &str) -> Result<(), String> {
    let (generation, account) = account_key()?.ok_or("This device can't approve others yet")?;
    let token = access().await?;
    let (me, user_id) = {
        let session = lock()?;
        (
            session.saved.device_id.clone(),
            session.saved.user_id.clone(),
        )
    };
    let device = api::devices(&token)
        .await?
        .into_iter()
        .find(|device| device.id == device_id && device.revoked_at.is_none())
        .ok_or("That device is gone")?;
    let envelope = keys::seal_for_device(
        &account,
        &device.public_key,
        &user_id,
        generation,
        &device.id,
    )?;
    api::add_envelope(&token, generation, Some(&device.id), &me, &envelope).await
}

/// Denies or removes a device. Rotating the account key away from it is a later step.
pub async fn revoke(device_id: &str) -> Result<(), String> {
    let token = access().await?;
    let now = chrono::Utc::now().to_rfc3339();
    api::update_device(&token, device_id, &json!({ "revoked_at": now })).await
}

pub fn recovery_key() -> Result<String, String> {
    if lock()?.phase != Phase::Ready {
        return Err("Sign in on this device first".into());
    }
    key_secret(RECOVERY_KEY)?
        .map(|key| keys::format_recovery(&key))
        .ok_or_else(|| {
            "This device doesn't have the recovery key. Show it on the device that set up sync."
                .into()
        })
}

/// Signs out and forgets this device's keys; signing in again makes it a new device.
pub async fn sign_out() -> Result<(), String> {
    let me = lock()?.saved.device_id.clone();
    if let Ok(token) = access().await {
        if !me.is_empty() {
            let now = chrono::Utc::now().to_rfc3339();
            let _ = api::update_device(&token, &me, &json!({ "revoked_at": now })).await;
        }
        let _ = api::logout(&token).await;
    }
    for account in [REFRESH, DEVICE_KEY, ACCOUNT_KEY, RECOVERY_KEY] {
        set_secret(account, None)?;
    }
    let mut session = lock()?;
    session.saved = Saved::default();
    session.access = None;
    session.phase = Phase::SignedOut;
    session.save();
    Ok(())
}

pub fn cancel_sign_in() -> Result<(), String> {
    let mut session = lock()?;
    if session.phase == Phase::Browser {
        session.phase = Phase::SignedOut;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    pub email: String,
    pub device_id: String,
    pub device_name: String,
    pub words: String,
    pub key_gen: Option<i32>,
    pub devices: Vec<api::Device>,
}

/// What Settings > Sync shows. Lists devices only when signed in.
pub async fn status() -> Result<Status, String> {
    let (phase, saved) = {
        let session = lock()?;
        (session.phase.clone(), session.saved.clone())
    };
    let signed_in = !matches!(phase, Phase::SignedOut | Phase::Browser);
    let devices = if signed_in {
        match access().await {
            Ok(token) => api::devices(&token).await.unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let words = if signed_in {
        device_secret()
            .map(|secret| keys::device_words(&keys::public_of(&secret)))
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Status {
        device_name: devices
            .iter()
            .find(|device| device.id == saved.device_id)
            .map(|device| device.display_name.clone())
            .unwrap_or_default(),
        key_gen: account_key()
            .ok()
            .flatten()
            .map(|(generation, _)| generation),
        phase,
        email: saved.email,
        device_id: saved.device_id,
        words,
        devices,
    })
}
