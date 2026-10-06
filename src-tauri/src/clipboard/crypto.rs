//! Clipboard history is encrypted at rest. The key lives in the OS keychain, so the files on
//! disk are unreadable without the user's login.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

const KEYCHAIN_SERVICE: &str = "com.zephyr.app.clipboard";
const KEYCHAIN_ACCOUNT: &str = "history-key";
const NONCE_LEN: usize = 24;

pub struct Cipher(XChaCha20Poly1305);

impl Cipher {
    /// Reads the history key from the keychain, creating one the first time.
    pub fn from_keychain() -> Result<Self, String> {
        let key = match crate::secrets::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
            .map_err(|err| format!("Couldn't read the clipboard key: {err}"))?
        {
            Some(encoded) => STANDARD
                .decode(encoded.trim())
                .map_err(|_| "The clipboard key in the keychain is damaged".to_string())?,
            None => {
                let mut key = vec![0u8; 32];
                getrandom::fill(&mut key).map_err(|err| err.to_string())?;
                crate::secrets::set(
                    KEYCHAIN_SERVICE,
                    KEYCHAIN_ACCOUNT,
                    Some(&STANDARD.encode(&key)),
                )
                .map_err(|err| format!("Couldn't save the clipboard key: {err}"))?;
                key
            }
        };
        Self::from_key(&key)
    }

    pub fn from_key(key: &[u8]) -> Result<Self, String> {
        XChaCha20Poly1305::new_from_slice(key)
            .map(Self)
            .map_err(|_| "The clipboard key has the wrong length".to_string())
    }

    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|err| err.to_string())?;
        let mut out = nonce.to_vec();
        out.extend(
            self.0
                .encrypt(XNonce::from_slice(&nonce), plain)
                .map_err(|_| "Couldn't encrypt the clipboard history".to_string())?,
        );
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, String> {
        if sealed.len() < NONCE_LEN {
            return Err("The clipboard history file is damaged".into());
        }
        let (nonce, body) = sealed.split_at(NONCE_LEN);
        self.0
            .decrypt(XNonce::from_slice(nonce), body)
            .map_err(|_| "The clipboard history couldn't be decrypted".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_tampering() {
        let cipher = Cipher::from_key(&[7u8; 32]).unwrap();
        let sealed = cipher.seal(b"secret").unwrap();
        assert_ne!(&sealed[NONCE_LEN..], b"secret");
        assert_eq!(cipher.open(&sealed).unwrap(), b"secret");
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(cipher.open(&tampered).is_err());
        assert!(Cipher::from_key(&[1u8; 32]).unwrap().open(&sealed).is_err());
    }
}
