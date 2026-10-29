//! End-to-end encrypted sync of notes and clipboard history through the user's Supabase
//! project. The server only ever holds ciphertext.
//!
//! Step 1 is accounts and keys: Google sign-in, a key pair per device, the account key
//! (AK) sealed to each approved device, a recovery key, and approving new devices from an
//! existing one. Record sync builds on this.

pub mod api;
pub mod keys;

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Digest;

use crate::secrets::Keychain;
use api::Server;
use keys::Key32;

const SERVICE: &str = "com.zephyr.app.sync";
const REFRESH: &str = "refresh-token";
const DEVICE_KEY: &str = "device-key";
const ACCOUNT_KEY: &str = "account-key";
const RECOVERY_KEY: &str = "recovery-key";
/// Keys made for a recovery envelope that may not be on the server yet.
const SETUP: &str = "setup-keys";

/// Who is signed in and as which device; ids only, nothing secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    user_id: String,
    email: String,
    device_id: String,
    /// This device made a recovery key the user hasn't confirmed saving yet.
    recovery_unsaved: bool,
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

impl Session {
    fn save(&self) {
        if self.dir.as_os_str().is_empty() {
            return;
        }
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

/// An account key and a recovery key for one generation, kept until the recovery envelope
/// is known to be on the server.
#[derive(Debug, Clone, Copy)]
struct Setup {
    generation: i32,
    account: Key32,
    recovery: Key32,
}

/// Sync's state, and the server and keychain it uses. The app has one, `live()`.
struct Engine<S, K> {
    server: S,
    keychain: K,
    session: Mutex<Session>,
    /// Held while trading the refresh token. Supabase rotates it on every use, so callers
    /// that raced with the same one would fail, and reuse can end the whole session.
    refreshing: tokio::sync::Mutex<()>,
    /// Held while working out where this device stands, so two settles can't both set up
    /// an account or register the device.
    settling: tokio::sync::Mutex<()>,
}

type Live = Engine<api::Supabase, crate::secrets::System>;

fn live() -> &'static Live {
    static LIVE: OnceLock<Live> = OnceLock::new();
    LIVE.get_or_init(|| Engine::new(api::Supabase, crate::secrets::System))
}

/// Restores the saved session at startup and settles where it stands, in the background.
pub fn init(dir: PathBuf) {
    if let Ok(mut session) = live().lock() {
        session.saved = fs::read(dir.join("sync.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        session.dir = dir;
    }
    tauri::async_runtime::spawn(async {
        let signed_in = live().secret(REFRESH).ok().flatten().is_some();
        if !signed_in {
            return;
        }
        if let Err(err) = live().settle().await {
            log::error!("sync couldn't resume: {err}");
        }
    });
}

/// Ports the sign-in callback listens on, so one Supabase redirect entry covers them.
const CALLBACK_PORTS: [u16; 5] = [47195, 47196, 47197, 47198, 47199];

/// Signs in with Google in the browser. A one-off listener on 127.0.0.1 catches the
/// redirect, and the code is exchanged with a PKCE verifier only this process knows.
pub async fn google_sign_in(open: impl FnOnce(&str) -> Result<(), String>) -> Result<(), String> {
    let sync = live();
    let (server, redirect) = crate::loopback::listen(&CALLBACK_PORTS)?;

    let verifier = data_encoding::BASE64URL_NOPAD.encode(&keys::random32()?);
    let challenge =
        data_encoding::BASE64URL_NOPAD.encode(&sha2::Sha256::digest(verifier.as_bytes()));
    sync.lock()?.phase = Phase::Browser;
    open(&api::google_url(&redirect, &challenge))?;

    let outcome = tauri::async_runtime::spawn_blocking(move || wait_for_code(&server))
        .await
        .map_err(|err| err.to_string())?;
    let finish = async {
        let code = outcome?;
        let tokens = sync.server.exchange_code(&code, &verifier).await?;
        {
            // Don't let a refresh still running for an old session overwrite this one.
            let _refreshing = sync.refreshing.lock().await;
            sync.adopt(&tokens)?;
        }
        sync.settle().await
    };
    let result = finish.await;
    if result.is_err()
        && let Ok(mut session) = sync.lock()
        && session.phase == Phase::Browser
    {
        session.phase = Phase::SignedOut;
    }
    result
}

/// Waits up to five minutes for the browser to come back with `?code=`.
fn wait_for_code(server: &tiny_http::Server) -> Result<String, String> {
    crate::loopback::wait(
        server,
        || {
            live()
                .lock()
                .map(|session| session.phase != Phase::Browser)
                .unwrap_or(true)
        },
        "Signed in to Zephyr. You can close this tab.",
        |param| match (param("code"), param("error_description").or(param("error"))) {
            (Some(code), _) => Ok(code),
            (None, Some(error)) => Err(format!("Google sign-in failed: {error}")),
            (None, None) => Err("The browser came back without a sign-in code".into()),
        },
    )
}

/// The user saved the recovery key; sync is ready.
pub fn recovery_saved() -> Result<(), String> {
    live().recovery_saved()
}

/// Unlocks a new device with the recovery key instead of another device's approval.
pub async fn use_recovery(text: &str) -> Result<(), String> {
    live().use_recovery(text).await
}

/// Checks again whether another device has approved this one.
pub async fn poll() -> Result<(), String> {
    live().poll().await
}

/// Other devices on the account waiting for this one to approve them.
pub async fn pending() -> Result<Vec<Pending>, String> {
    live().pending().await
}

/// Approves another device: seals the account key to its public key.
pub async fn approve(device_id: &str) -> Result<(), String> {
    live().approve(device_id).await
}

/// Denies or removes a device. Rotating the account key away from it is a later step.
pub async fn revoke(device_id: &str) -> Result<(), String> {
    live().revoke(device_id).await
}

pub fn recovery_key() -> Result<String, String> {
    live().recovery_key()
}

/// Signs out and forgets this device's keys; signing in again makes it a new device.
pub async fn sign_out() -> Result<(), String> {
    live().sign_out().await
}

pub fn cancel_sign_in() -> Result<(), String> {
    let mut session = live().lock()?;
    if session.phase == Phase::Browser {
        session.phase = Phase::SignedOut;
    }
    Ok(())
}

/// What Settings > Sync shows. Lists devices only when signed in.
pub async fn status() -> Result<Status, String> {
    live().status().await
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

fn decode_key(text: &str) -> Option<Key32> {
    STANDARD.decode(text.trim()).ok()?.try_into().ok()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub words: String,
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

impl<S: Server, K: Keychain> Engine<S, K> {
    fn new(server: S, keychain: K) -> Self {
        Self {
            server,
            keychain,
            session: Mutex::new(Session {
                dir: PathBuf::new(),
                saved: Saved::default(),
                phase: Phase::SignedOut,
                access: None,
                last_seen_touch: None,
            }),
            refreshing: tokio::sync::Mutex::new(()),
            settling: tokio::sync::Mutex::new(()),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Session>, String> {
        self.session
            .lock()
            .map_err(|_| "Sync is unavailable".to_string())
    }

    fn secret(&self, account: &str) -> Result<Option<String>, String> {
        self.keychain.get(SERVICE, account)
    }

    fn set_secret(&self, account: &str, value: Option<&str>) -> Result<(), String> {
        self.keychain.set(SERVICE, account, value)
    }

    fn key_secret(&self, account: &str) -> Result<Option<Key32>, String> {
        Ok(self.secret(account)?.and_then(|text| decode_key(&text)))
    }

    /// The stored account key as (generation, key).
    fn account_key(&self) -> Result<Option<(i32, Key32)>, String> {
        Ok(self.secret(ACCOUNT_KEY)?.and_then(|text| {
            let (generation, key) = text.split_once(':')?;
            Some((generation.parse().ok()?, decode_key(key)?))
        }))
    }

    fn store_account_key(&self, generation: i32, key: &Key32) -> Result<(), String> {
        self.set_secret(
            ACCOUNT_KEY,
            Some(&format!("{generation}:{}", STANDARD.encode(key))),
        )
    }

    fn setup(&self) -> Result<Option<Setup>, String> {
        Ok(self.secret(SETUP)?.and_then(|text| {
            let mut parts = text.split(':');
            Some(Setup {
                generation: parts.next()?.parse().ok()?,
                account: decode_key(parts.next()?)?,
                recovery: decode_key(parts.next()?)?,
            })
        }))
    }

    /// Makes a recovery key for `account` and keeps both before anything is uploaded, so an
    /// interrupted setup resumes with the same keys.
    fn new_setup(&self, generation: i32, account: Key32) -> Result<Setup, String> {
        let setup = Setup {
            generation,
            account,
            recovery: keys::random32()?,
        };
        self.set_secret(
            SETUP,
            Some(&format!(
                "{generation}:{}:{}",
                STANDARD.encode(setup.account),
                STANDARD.encode(setup.recovery)
            )),
        )?;
        Ok(setup)
    }

    /// This device's key pair, made on first use.
    fn device_secret(&self) -> Result<x25519_dalek::StaticSecret, String> {
        let bytes = match self.key_secret(DEVICE_KEY)? {
            Some(bytes) => bytes,
            None => {
                let bytes = keys::random32()?;
                self.set_secret(DEVICE_KEY, Some(&STANDARD.encode(bytes)))?;
                bytes
            }
        };
        Ok(keys::device_secret(bytes))
    }

    fn cached_access(&self) -> Result<Option<String>, String> {
        Ok(self
            .lock()?
            .access
            .clone()
            .filter(|(_, expires)| Instant::now() < *expires)
            .map(|(token, _)| token))
    }

    /// A current access token, refreshing it (and the rotated refresh token) when needed.
    /// Concurrent callers share one refresh.
    async fn access(&self) -> Result<String, String> {
        if let Some(token) = self.cached_access()? {
            return Ok(token);
        }
        let _refreshing = self.refreshing.lock().await;
        // Whoever held the lock before may have just refreshed.
        if let Some(token) = self.cached_access()? {
            return Ok(token);
        }
        let refresh = self.secret(REFRESH)?.ok_or("Sign in to sync first")?;
        let tokens = self.server.refresh(&refresh).await?;
        self.adopt(&tokens)?;
        Ok(tokens.access)
    }

    fn adopt(&self, tokens: &api::Tokens) -> Result<(), String> {
        self.set_secret(REFRESH, Some(&tokens.refresh))?;
        let mut session = self.lock()?;
        // Refresh a minute early so a request never carries an expiring token.
        let life = Duration::from_secs(tokens.expires_in.saturating_sub(60).max(30));
        session.access = Some((tokens.access.clone(), Instant::now() + life));
        if session.saved.user_id != tokens.user_id {
            session.saved = Saved {
                user_id: tokens.user_id.clone(),
                email: tokens.email.clone(),
                ..Saved::default()
            };
        } else if !tokens.email.is_empty() {
            session.saved.email = tokens.email.clone();
        }
        session.save();
        Ok(())
    }

    /// Makes sure this install has a live device row, registering one if needed.
    async fn ensure_device(&self, token: &str) -> Result<String, String> {
        let secret = self.device_secret()?;
        let public = keys::public_of(&secret);
        let known = self.lock()?.saved.device_id.clone();
        let devices = self.server.devices(token).await?;
        if let Some(device) = devices
            .iter()
            .find(|device| device.id == known && device.public_key == public)
        {
            if device.revoked_at.is_some() {
                // Removed from another device: this install starts over as a new one.
                self.set_secret(ACCOUNT_KEY, None)?;
                self.set_secret(RECOVERY_KEY, None)?;
                self.set_secret(SETUP, None)?;
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
            self.server
                .add_device(token, platform(), &computer_name(), &public)
                .await?
        };
        let mut session = self.lock()?;
        session.saved.device_id = device.id.clone();
        session.save();
        Ok(device.id)
    }

    /// Works out where this device stands and moves to the right phase: ready, first-time
    /// setup, or waiting for approval. Safe to run again after any step fails.
    async fn settle(&self) -> Result<(), String> {
        let _settling = self.settling.lock().await;
        let token = self.access().await?;
        let device_id = self.ensure_device(&token).await?;
        let user_id = self.lock()?.saved.user_id.clone();
        let mut envelopes = self.server.envelopes(&token).await?;

        if envelopes.is_empty() {
            // A new account: this device makes the account key and the recovery key.
            let setup = match self.setup()? {
                Some(setup) if setup.generation == 1 => setup,
                _ => {
                    let account = match self.account_key()? {
                        Some((1, account)) => account,
                        _ => keys::random32()?,
                    };
                    self.new_setup(1, account)?
                }
            };
            self.upload_recovery(&token, &user_id, &device_id, &setup)
                .await?;
            envelopes = self.server.envelopes(&token).await?;
        }
        self.adopt_setup(&envelopes, &user_id)?;
        let current = envelopes
            .iter()
            .map(|stored| stored.key_gen)
            .max()
            .ok_or("The sync server has no account key")?;

        if !self
            .account_key()?
            .is_some_and(|(generation, _)| generation >= current)
        {
            let Some(mine) = envelopes.iter().find(|stored| {
                stored.key_gen == current
                    && stored.recipient_device_id.as_deref() == Some(&device_id)
            }) else {
                let words = keys::device_words(&keys::public_of(&self.device_secret()?));
                self.lock()?.phase = Phase::AwaitingApproval { words };
                return Ok(());
            };
            let account = keys::open_device_envelope(
                &self.device_secret()?,
                &mine.envelope,
                &user_id,
                current,
                &device_id,
            )?;
            self.store_account_key(current, &account)?;
        }

        let (generation, account) = self.account_key()?.ok_or("Sync lost the account key")?;
        if generation == current {
            self.repair(&token, &user_id, &device_id, &envelopes, &account)
                .await?;
        }
        let recovery = self.key_secret(RECOVERY_KEY)?;
        let mut session = self.lock()?;
        session.phase = match recovery {
            Some(recovery) if session.saved.recovery_unsaved => Phase::ShowRecovery {
                recovery: keys::format_recovery(&recovery),
            },
            _ => Phase::Ready,
        };
        Ok(())
    }

    /// Seals the setup's account key under its recovery key and uploads it. The server keeps
    /// one recovery envelope per generation, so of two devices setting up at once only one
    /// gets in; `adopt_setup` then tells whose it was.
    async fn upload_recovery(
        &self,
        token: &str,
        user_id: &str,
        device_id: &str,
        setup: &Setup,
    ) -> Result<(), String> {
        let backup =
            keys::seal_for_recovery(&setup.account, &setup.recovery, user_id, setup.generation)?;
        if let Err(err) = self
            .server
            .add_envelope(token, setup.generation, None, device_id, &backup)
            .await
        {
            // It may have landed without the reply reaching us, or another device's did.
            let landed = self
                .server
                .envelopes(token)
                .await?
                .iter()
                .any(|stored| stored.kind == "recovery" && stored.key_gen == setup.generation);
            if !landed {
                return Err(err);
            }
        }
        Ok(())
    }

    /// Finishes a setup whose recovery envelope is on the server: if that envelope opens with
    /// the setup's recovery key, its keys are the account's and the user must save the
    /// recovery key. If it holds another device's, this setup lost and is dropped.
    fn adopt_setup(&self, envelopes: &[api::StoredEnvelope], user_id: &str) -> Result<(), String> {
        let Some(setup) = self.setup()? else {
            return Ok(());
        };
        let Some(backup) = envelopes
            .iter()
            .find(|stored| stored.kind == "recovery" && stored.key_gen == setup.generation)
        else {
            // Not uploaded yet: kept for the next try.
            return Ok(());
        };
        let ours = keys::open_recovery_envelope(
            &setup.recovery,
            &backup.envelope,
            user_id,
            setup.generation,
        )
        .is_ok_and(|account| account == setup.account);
        if ours
            && !self
                .account_key()?
                .is_some_and(|(generation, _)| generation > setup.generation)
        {
            self.store_account_key(setup.generation, &setup.account)?;
            self.set_secret(RECOVERY_KEY, Some(&STANDARD.encode(setup.recovery)))?;
            let mut session = self.lock()?;
            session.saved.recovery_unsaved = true;
            session.save();
        }
        self.set_secret(SETUP, None)
    }

    /// Puts back what an interrupted setup left out of the current generation: the
    /// recovery envelope (with a new recovery key to show) and this device's own envelope.
    async fn repair(
        &self,
        token: &str,
        user_id: &str,
        device_id: &str,
        envelopes: &[api::StoredEnvelope],
        account: &Key32,
    ) -> Result<(), String> {
        let generation = envelopes
            .iter()
            .map(|stored| stored.key_gen)
            .max()
            .unwrap_or(1);
        let has = |recipient: Option<&str>| {
            envelopes.iter().any(|stored| {
                stored.key_gen == generation && stored.recipient_device_id.as_deref() == recipient
            })
        };
        if !has(None) {
            log::info!("sync account has no recovery key; making one");
            let setup = match self.setup()? {
                Some(setup) if setup.generation == generation && setup.account == *account => setup,
                _ => self.new_setup(generation, *account)?,
            };
            self.upload_recovery(token, user_id, device_id, &setup)
                .await?;
            let envelopes = self.server.envelopes(token).await?;
            self.adopt_setup(&envelopes, user_id)?;
        }
        if !has(Some(device_id)) {
            let own = keys::seal_for_device(
                account,
                &keys::public_of(&self.device_secret()?),
                user_id,
                generation,
                device_id,
            )?;
            self.server
                .add_envelope(token, generation, Some(device_id), device_id, &own)
                .await?;
        }
        Ok(())
    }

    fn recovery_saved(&self) -> Result<(), String> {
        let mut session = self.lock()?;
        if matches!(session.phase, Phase::ShowRecovery { .. }) {
            session.phase = Phase::Ready;
        }
        if session.saved.recovery_unsaved {
            session.saved.recovery_unsaved = false;
            session.save();
        }
        Ok(())
    }

    async fn use_recovery(&self, text: &str) -> Result<(), String> {
        let recovery = keys::parse_recovery(text)?;
        let _settling = self.settling.lock().await;
        let token = self.access().await?;
        let device_id = self.ensure_device(&token).await?;
        let user_id = self.lock()?.saved.user_id.clone();
        let envelopes = self.server.envelopes(&token).await?;
        let backup = envelopes
            .iter()
            .filter(|stored| stored.kind == "recovery")
            .max_by_key(|stored| stored.key_gen)
            .ok_or("This account has no recovery key yet")?;
        let account =
            keys::open_recovery_envelope(&recovery, &backup.envelope, &user_id, backup.key_gen)?;
        self.store_account_key(backup.key_gen, &account)?;
        self.set_secret(RECOVERY_KEY, Some(&STANDARD.encode(recovery)))?;
        // Seal the key to this device too, so the next sign-in here needs nothing.
        let secret = self.device_secret()?;
        let own = keys::seal_for_device(
            &account,
            &keys::public_of(&secret),
            &user_id,
            backup.key_gen,
            &device_id,
        )?;
        if let Err(err) = self
            .server
            .add_envelope(&token, backup.key_gen, Some(&device_id), &device_id, &own)
            .await
        {
            log::info!("couldn't store this device's envelope: {err}");
        }
        self.lock()?.phase = Phase::Ready;
        Ok(())
    }

    async fn poll(&self) -> Result<(), String> {
        if matches!(self.lock()?.phase, Phase::AwaitingApproval { .. }) {
            self.settle().await?;
        }
        Ok(())
    }

    async fn pending(&self) -> Result<Vec<Pending>, String> {
        if self.lock()?.phase != Phase::Ready {
            return Ok(Vec::new());
        }
        let Some((generation, _)) = self.account_key()? else {
            return Ok(Vec::new());
        };
        let token = self.access().await?;
        let me = self.lock()?.saved.device_id.clone();
        let devices = self.server.devices(&token).await?;
        let envelopes = self.server.envelopes(&token).await?;
        self.touch_last_seen(&token, &me).await;
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

    async fn touch_last_seen(&self, token: &str, me: &str) {
        let due = self
            .lock()
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
        if self
            .server
            .update_device(token, me, &json!({ "last_seen_at": now }))
            .await
            .is_ok()
            && let Ok(mut session) = self.lock()
        {
            session.last_seen_touch = Some(Instant::now());
        }
    }

    async fn approve(&self, device_id: &str) -> Result<(), String> {
        let (generation, account) = self
            .account_key()?
            .ok_or("This device can't approve others yet")?;
        let token = self.access().await?;
        let (me, user_id) = {
            let session = self.lock()?;
            (
                session.saved.device_id.clone(),
                session.saved.user_id.clone(),
            )
        };
        let device = self
            .server
            .devices(&token)
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
        self.server
            .add_envelope(&token, generation, Some(&device.id), &me, &envelope)
            .await
    }

    async fn revoke(&self, device_id: &str) -> Result<(), String> {
        let token = self.access().await?;
        let now = chrono::Utc::now().to_rfc3339();
        self.server
            .update_device(&token, device_id, &json!({ "revoked_at": now }))
            .await
    }

    fn recovery_key(&self) -> Result<String, String> {
        if self.lock()?.phase != Phase::Ready {
            return Err("Sign in on this device first".into());
        }
        self.key_secret(RECOVERY_KEY)?
            .map(|key| keys::format_recovery(&key))
            .ok_or_else(|| {
                "This device doesn't have the recovery key. Show it on the device that set up sync."
                    .into()
            })
    }

    async fn sign_out(&self) -> Result<(), String> {
        let _settling = self.settling.lock().await;
        let me = self.lock()?.saved.device_id.clone();
        if let Ok(token) = self.access().await {
            if !me.is_empty() {
                let now = chrono::Utc::now().to_rfc3339();
                let _ = self
                    .server
                    .update_device(&token, &me, &json!({ "revoked_at": now }))
                    .await;
            }
            let _ = self.server.logout(&token).await;
        }
        for account in [REFRESH, DEVICE_KEY, ACCOUNT_KEY, RECOVERY_KEY, SETUP] {
            self.set_secret(account, None)?;
        }
        let mut session = self.lock()?;
        session.saved = Saved::default();
        session.access = None;
        session.phase = Phase::SignedOut;
        session.save();
        Ok(())
    }

    async fn status(&self) -> Result<Status, String> {
        let (phase, saved) = {
            let session = self.lock()?;
            (session.phase.clone(), session.saved.clone())
        };
        let signed_in = !matches!(phase, Phase::SignedOut | Phase::Browser);
        let devices = if signed_in {
            match self.access().await {
                Ok(token) => self.server.devices(&token).await.unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let words = if signed_in {
            self.device_secret()
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
            key_gen: self
                .account_key()
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use serde_json::Value;

    use super::api::{Device, StoredEnvelope, Tokens};
    use super::keys::Envelope;
    use super::*;
    use crate::secrets::Memory;

    const USER: &str = "6f1c0a3e-1111-2222-3333-444455556666";

    #[derive(Default)]
    struct Fake {
        state: Mutex<FakeState>,
        refreshes: AtomicUsize,
        /// Fails the next recovery upload: `Some(false)` before it lands, `Some(true)` after
        /// (a lost reply).
        fail_recovery: Mutex<Option<bool>>,
        /// Another device uploads its recovery envelope just before this one's.
        rival: AtomicBool,
    }

    #[derive(Default)]
    struct FakeState {
        refresh: String,
        issued: usize,
        devices: Vec<Device>,
        envelopes: Vec<StoredEnvelope>,
    }

    impl FakeState {
        /// Mirrors the table's unique (user_id, key_gen, kind, recipient_device_id).
        fn insert(&mut self, stored: StoredEnvelope) -> Result<(), String> {
            if self.envelopes.iter().any(|other| {
                other.key_gen == stored.key_gen
                    && other.kind == stored.kind
                    && other.recipient_device_id == stored.recipient_device_id
            }) {
                return Err("duplicate key value violates unique constraint".into());
            }
            self.envelopes.push(stored);
            Ok(())
        }
    }

    impl Server for Fake {
        async fn exchange_code(&self, _: &str, _: &str) -> Result<Tokens, String> {
            Err("not in tests".into())
        }

        async fn refresh(&self, refresh_token: &str) -> Result<Tokens, String> {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            // Long enough for concurrent callers to pile up.
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut state = self.state.lock().unwrap();
            if refresh_token != state.refresh {
                return Err("Invalid Refresh Token: Already Used".into());
            }
            state.issued += 1;
            state.refresh = format!("refresh-{}", state.issued);
            Ok(Tokens {
                access: format!("access-{}", state.issued),
                refresh: state.refresh.clone(),
                expires_in: 3600,
                user_id: USER.into(),
                email: "me@example.com".into(),
            })
        }

        async fn logout(&self, _: &str) -> Result<(), String> {
            Ok(())
        }

        async fn devices(&self, _: &str) -> Result<Vec<Device>, String> {
            Ok(self.state.lock().unwrap().devices.clone())
        }

        async fn add_device(
            &self,
            _: &str,
            platform: &str,
            name: &str,
            public_key: &Key32,
        ) -> Result<Device, String> {
            let device = Device {
                id: uuid::Uuid::new_v4().to_string(),
                platform: platform.into(),
                display_name: name.into(),
                public_key: *public_key,
                last_seen_at: None,
                revoked_at: None,
                created_at: None,
            };
            self.state.lock().unwrap().devices.push(device.clone());
            Ok(device)
        }

        async fn update_device(&self, _: &str, id: &str, fields: &Value) -> Result<(), String> {
            let mut state = self.state.lock().unwrap();
            if let Some(device) = state.devices.iter_mut().find(|device| device.id == id)
                && let Some(revoked) = fields["revoked_at"].as_str()
            {
                device.revoked_at = Some(revoked.into());
            }
            Ok(())
        }

        async fn envelopes(&self, _: &str) -> Result<Vec<StoredEnvelope>, String> {
            Ok(self.state.lock().unwrap().envelopes.clone())
        }

        async fn add_envelope(
            &self,
            _: &str,
            key_gen: i32,
            recipient_device_id: Option<&str>,
            _: &str,
            envelope: &Envelope,
        ) -> Result<(), String> {
            let stored = |envelope: Envelope| StoredEnvelope {
                key_gen,
                kind: if recipient_device_id.is_some() {
                    "device"
                } else {
                    "recovery"
                }
                .into(),
                recipient_device_id: recipient_device_id.map(str::to_string),
                envelope,
            };
            let mut state = self.state.lock().unwrap();
            if recipient_device_id.is_none() {
                if self.rival.swap(false, Ordering::SeqCst) {
                    let theirs =
                        keys::seal_for_recovery(&keys::random32()?, &keys::random32()?, USER, 1)?;
                    state.insert(stored(theirs))?;
                }
                match self.fail_recovery.lock().unwrap().take() {
                    Some(false) => return Err("Couldn't reach the sync server".into()),
                    Some(true) => {
                        state.insert(stored(envelope.clone()))?;
                        return Err("Couldn't reach the sync server".into());
                    }
                    None => {}
                }
            }
            state.insert(stored(envelope.clone()))
        }
    }

    type Test = Arc<Engine<Fake, Memory>>;

    fn signed_in(fake: Fake) -> Test {
        fake.state.lock().unwrap().refresh = "refresh-0".into();
        let sync = Engine::new(fake, Memory::default());
        sync.set_secret(REFRESH, Some("refresh-0")).unwrap();
        sync.lock().unwrap().saved.user_id = USER.into();
        Arc::new(sync)
    }

    fn run<T>(future: impl Future<Output = T>) -> T {
        tauri::async_runtime::block_on(future)
    }

    fn phase(sync: &Test) -> Phase {
        sync.lock().unwrap().phase.clone()
    }

    /// Runs `count` settles at once, as startup and the UI can.
    fn settle_together(sync: &Test, count: usize) -> Vec<Result<(), String>> {
        run(async {
            let tasks: Vec<_> = (0..count)
                .map(|_| {
                    let sync = sync.clone();
                    tauri::async_runtime::spawn(async move { sync.settle().await })
                })
                .collect();
            let mut results = Vec::new();
            for task in tasks {
                results.push(task.await.unwrap());
            }
            results
        })
    }

    /// The server holds exactly one recovery envelope and one envelope for this device, and
    /// both open to the account key this device keeps.
    fn assert_set_up(sync: &Test) {
        let (generation, account) = sync.account_key().unwrap().expect("an account key");
        let recovery = sync
            .key_secret(RECOVERY_KEY)
            .unwrap()
            .expect("a recovery key");
        let device_id = sync.lock().unwrap().saved.device_id.clone();
        let envelopes = sync.server.state.lock().unwrap().envelopes.clone();
        assert_eq!(envelopes.len(), 2, "{envelopes:?}");
        let backup = envelopes
            .iter()
            .find(|stored| stored.kind == "recovery")
            .unwrap();
        assert_eq!(
            keys::open_recovery_envelope(&recovery, &backup.envelope, USER, generation).unwrap(),
            account
        );
        let own = envelopes
            .iter()
            .find(|stored| stored.recipient_device_id.as_deref() == Some(&device_id))
            .unwrap();
        let device = sync.device_secret().unwrap();
        assert_eq!(
            keys::open_device_envelope(&device, &own.envelope, USER, generation, &device_id)
                .unwrap(),
            account
        );
        assert!(sync.setup().unwrap().is_none());
    }

    #[test]
    fn concurrent_callers_share_one_refresh() {
        let sync = signed_in(Fake::default());
        let tokens = run(async {
            let tasks: Vec<_> = (0..8)
                .map(|_| {
                    let sync = sync.clone();
                    tauri::async_runtime::spawn(async move { sync.access().await })
                })
                .collect();
            let mut tokens = Vec::new();
            for task in tasks {
                tokens.push(task.await.unwrap());
            }
            tokens
        });
        assert!(
            tokens
                .iter()
                .all(|token| token.as_deref() == Ok("access-1"))
        );
        assert_eq!(sync.server.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(sync.secret(REFRESH).unwrap().as_deref(), Some("refresh-1"));
    }

    #[test]
    fn concurrent_settles_set_up_a_new_account_once() {
        let sync = signed_in(Fake::default());
        assert!(settle_together(&sync, 4).iter().all(Result::is_ok));
        assert_set_up(&sync);
        assert_eq!(sync.server.state.lock().unwrap().devices.len(), 1);
        assert!(matches!(phase(&sync), Phase::ShowRecovery { .. }));

        sync.recovery_saved().unwrap();
        run(sync.settle()).unwrap();
        assert_eq!(phase(&sync), Phase::Ready);
        assert_set_up(&sync);
    }

    #[test]
    fn a_failed_recovery_upload_resumes_on_the_next_settle() {
        let fake = Fake::default();
        *fake.fail_recovery.lock().unwrap() = Some(false);
        let sync = signed_in(fake);
        assert!(run(sync.settle()).is_err());
        assert!(sync.server.state.lock().unwrap().envelopes.is_empty());
        assert!(sync.account_key().unwrap().is_none());
        let started = sync.setup().unwrap().expect("setup kept for the retry");

        run(sync.settle()).unwrap();
        assert_set_up(&sync);
        assert_eq!(sync.account_key().unwrap(), Some((1, started.account)));
        assert!(matches!(phase(&sync), Phase::ShowRecovery { .. }));
    }

    #[test]
    fn a_recovery_upload_whose_reply_was_lost_still_finishes() {
        let fake = Fake::default();
        *fake.fail_recovery.lock().unwrap() = Some(true);
        let sync = signed_in(fake);
        run(sync.settle()).unwrap();
        assert_set_up(&sync);
        assert!(matches!(phase(&sync), Phase::ShowRecovery { .. }));
    }

    #[test]
    fn settle_repairs_a_missing_recovery_envelope() {
        let sync = signed_in(Fake::default());
        run(sync.settle()).unwrap();
        sync.recovery_saved().unwrap();
        let (_, account) = sync.account_key().unwrap().unwrap();
        // What an interrupted first device used to leave: its own envelope, no recovery
        // envelope, and no keys kept.
        sync.server
            .state
            .lock()
            .unwrap()
            .envelopes
            .retain(|stored| stored.kind == "device");
        for secret in [ACCOUNT_KEY, RECOVERY_KEY] {
            sync.set_secret(secret, None).unwrap();
        }

        run(sync.settle()).unwrap();
        assert_set_up(&sync);
        assert_eq!(sync.account_key().unwrap(), Some((1, account)));
        let recovery = sync.key_secret(RECOVERY_KEY).unwrap().unwrap();
        assert_eq!(
            phase(&sync),
            Phase::ShowRecovery {
                recovery: keys::format_recovery(&recovery)
            }
        );
    }

    #[test]
    fn a_device_that_loses_the_setup_race_waits_for_approval() {
        let fake = Fake::default();
        fake.rival.store(true, Ordering::SeqCst);
        let sync = signed_in(fake);
        run(sync.settle()).unwrap();
        assert!(matches!(phase(&sync), Phase::AwaitingApproval { .. }));
        assert!(sync.account_key().unwrap().is_none());
        assert!(sync.key_secret(RECOVERY_KEY).unwrap().is_none());
        assert!(sync.setup().unwrap().is_none());
        assert_eq!(sync.server.state.lock().unwrap().envelopes.len(), 1);
    }
}
