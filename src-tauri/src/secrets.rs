//! Where API keys and the clipboard key live.
//!
//! Installed builds use the OS keychain. Dev builds (`bun dev`) keep them in a private file
//! instead: every rebuild is a new unsigned binary, and macOS asks for the keychain password
//! again for each one, so "Always Allow" never sticks while developing.

#[cfg(debug_assertions)]
use std::path::Path;
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
pub fn get(service: &str, account: &str) -> Result<Option<String>, String> {
    file_get(
        DEV_FILE.get().ok_or("Secrets aren't ready yet")?,
        service,
        account,
    )
}

#[cfg(debug_assertions)]
pub fn set(service: &str, account: &str, secret: Option<&str>) -> Result<(), String> {
    let path = DEV_FILE.get().ok_or("Secrets aren't ready yet")?;
    file_set(path, service, account, secret)
}

#[cfg(debug_assertions)]
fn file_read(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| err.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
        Err(err) => Err(err.to_string()),
    }
}

#[cfg(debug_assertions)]
fn file_get(path: &Path, service: &str, account: &str) -> Result<Option<String>, String> {
    Ok(file_read(path)?
        .get(&format!("{service}/{account}"))
        .and_then(|value| value.as_str())
        .map(str::to_string))
}

#[cfg(debug_assertions)]
fn file_set(path: &Path, service: &str, account: &str, secret: Option<&str>) -> Result<(), String> {
    let mut all = file_read(path)?;
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

/// Somewhere to keep secrets, so code that needs them can be tested without the keychain.
pub trait Keychain: Send + Sync {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String>;
    fn set(&self, service: &str, account: &str, secret: Option<&str>) -> Result<(), String>;
}

/// The keychain (or, in dev builds, the file) the app really uses.
pub struct System;

impl Keychain for System {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        get(service, account)
    }

    fn set(&self, service: &str, account: &str, secret: Option<&str>) -> Result<(), String> {
        set(service, account, secret)
    }
}

/// Secrets held in memory only, for tests.
#[cfg(test)]
#[derive(Default)]
pub struct Memory(std::sync::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl Keychain for Memory {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
        let all = self.0.lock().map_err(|err| err.to_string())?;
        Ok(all.get(&format!("{service}/{account}")).cloned())
    }

    fn set(&self, service: &str, account: &str, secret: Option<&str>) -> Result<(), String> {
        let mut all = self.0.lock().map_err(|err| err.to_string())?;
        let key = format!("{service}/{account}");
        match secret {
            Some(secret) => all.insert(key, secret.to_string()),
            None => all.remove(&key),
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(keychain: &dyn Keychain) {
        assert_eq!(keychain.get("svc", "a").unwrap(), None);
        keychain.set("svc", "a", Some("one")).unwrap();
        keychain.set("svc", "b", Some("two")).unwrap();
        keychain.set("other", "a", Some("three")).unwrap();
        assert_eq!(keychain.get("svc", "a").unwrap().as_deref(), Some("one"));
        keychain.set("svc", "a", Some("uno")).unwrap();
        assert_eq!(keychain.get("svc", "a").unwrap().as_deref(), Some("uno"));
        // Deleting twice is fine, and leaves the other entries alone.
        keychain.set("svc", "a", None).unwrap();
        keychain.set("svc", "a", None).unwrap();
        assert_eq!(keychain.get("svc", "a").unwrap(), None);
        assert_eq!(keychain.get("svc", "b").unwrap().as_deref(), Some("two"));
        assert_eq!(
            keychain.get("other", "a").unwrap().as_deref(),
            Some("three")
        );
    }

    #[test]
    fn memory_secrets_round_trip() {
        round_trip(&Memory::default());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn dev_file_secrets_round_trip() {
        struct File(PathBuf);
        impl Keychain for File {
            fn get(&self, service: &str, account: &str) -> Result<Option<String>, String> {
                file_get(&self.0, service, account)
            }
            fn set(
                &self,
                service: &str,
                account: &str,
                secret: Option<&str>,
            ) -> Result<(), String> {
                file_set(&self.0, service, account, secret)
            }
        }
        let dir = std::env::temp_dir().join(format!("zephyr-secrets-{}", uuid::Uuid::new_v4()));
        let file = File(dir.join("nested").join("dev-secrets.json"));
        round_trip(&file);
        let _ = std::fs::remove_dir_all(dir);
    }
}
