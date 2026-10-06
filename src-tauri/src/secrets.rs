//! Where API keys and the clipboard key live.
//!
//! Installed builds use the OS keychain. Dev builds (`bun dev`) keep them in a private file
//! instead: every rebuild is a new unsigned binary, and macOS asks for the keychain password
//! again for each one, so "Always Allow" never sticks while developing.

use std::path::PathBuf;
use std::sync::OnceLock;

#[cfg_attr(not(debug_assertions), allow(dead_code))]
static DEV_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Sets the folder for the dev-build secrets file. Installed builds ignore it.
pub fn init(dir: PathBuf) {
    let _ = DEV_FILE.set(dir.join("dev-secrets.json"));
}

#[cfg(not(debug_assertions))]
pub fn get(service: &str, account: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(service, account).map_err(keychain_error)?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(keychain_error(err)),
    }
}

#[cfg(not(debug_assertions))]
pub fn set(service: &str, account: &str, secret: Option<&str>) -> Result<(), String> {
    let entry = keyring::Entry::new(service, account).map_err(keychain_error)?;
    match secret {
        Some(secret) => entry.set_password(secret).map_err(keychain_error),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(keychain_error(err)),
        },
    }
}

#[cfg(not(debug_assertions))]
fn keychain_error(err: keyring::Error) -> String {
    format!("Couldn't use the keychain: {err}")
}

#[cfg(debug_assertions)]
fn dev_read() -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let path = DEV_FILE.get().ok_or("Secrets aren't ready yet")?;
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(debug_assertions)]
pub fn get(service: &str, account: &str) -> Result<Option<String>, String> {
    Ok(dev_read()?
        .get(&format!("{service}/{account}"))
        .and_then(|value| value.as_str())
        .map(str::to_string))
}

#[cfg(debug_assertions)]
pub fn set(service: &str, account: &str, secret: Option<&str>) -> Result<(), String> {
    let path = DEV_FILE.get().ok_or("Secrets aren't ready yet")?;
    let mut all = dev_read()?;
    let key = format!("{service}/{account}");
    match secret {
        Some(secret) => {
            all.insert(key, secret.into());
        }
        None => {
            all.remove(&key);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let data = serde_json::to_vec_pretty(&all).map_err(|err| err.to_string())?;
    std::fs::write(path, data).map_err(|err| err.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
