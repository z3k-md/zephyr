//! Sync key handling. Notes and clipboard items are sealed with one account key (AK) that
//! only devices ever see. The server stores the AK sealed to each approved device's X25519
//! key ("device envelopes") and once under the user's recovery key ("recovery envelope").

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use data_encoding::{Encoding, Specification};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

pub type Key32 = [u8; 32];

const DEVICE_INFO: &[u8] = b"zephyr/envelope/v1";
const RECOVERY_INFO: &[u8] = b"zephyr/recovery/v1";

/// One sealed copy of the account key, as stored in `zephyr.key_envelopes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub ephemeral_public_key: Option<Key32>,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

pub fn random32() -> Result<Key32, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|err| err.to_string())?;
    Ok(bytes)
}

fn random_nonce() -> Result<[u8; 24], String> {
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).map_err(|err| err.to_string())?;
    Ok(nonce)
}

/// The device's long-lived X25519 secret.
pub fn device_secret(bytes: Key32) -> StaticSecret {
    StaticSecret::from(bytes)
}

pub fn public_of(secret: &StaticSecret) -> Key32 {
    PublicKey::from(secret).to_bytes()
}

fn device_aad(user_id: &str, key_gen: i32, recipient_device_id: &str) -> Vec<u8> {
    format!("zephyr/envelope/v1|{user_id}|{key_gen}|{recipient_device_id}").into_bytes()
}

fn recovery_aad(user_id: &str, key_gen: i32) -> Vec<u8> {
    format!("zephyr/recovery/v1|{user_id}|{key_gen}").into_bytes()
}

fn device_key(shared: &[u8], ephemeral: &Key32, recipient: &Key32) -> Result<Key32, String> {
    let mut salt = Vec::with_capacity(64);
    salt.extend_from_slice(ephemeral);
    salt.extend_from_slice(recipient);
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&salt), shared)
        .expand(DEVICE_INFO, &mut key)
        .map_err(|_| "Couldn't derive the envelope key".to_string())?;
    Ok(key)
}

fn recovery_wrap_key(recovery: &Key32) -> Result<Key32, String> {
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(None, recovery)
        .expand(RECOVERY_INFO, &mut key)
        .map_err(|_| "Couldn't derive the recovery key".to_string())?;
    Ok(key)
}

fn seal(key: &Key32, nonce: &[u8; 24], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String> {
    XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "bad key".to_string())?
        .encrypt(XNonce::from_slice(nonce), Payload { msg: plain, aad })
        .map_err(|_| "Couldn't seal the account key".to_string())
}

fn open(key: &Key32, nonce: &[u8; 24], aad: &[u8], sealed: &[u8]) -> Result<Key32, String> {
    let plain = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|_| "bad key".to_string())?
        .decrypt(XNonce::from_slice(nonce), Payload { msg: sealed, aad })
        .map_err(|_| "That key couldn't be opened".to_string())?;
    plain
        .try_into()
        .map_err(|_| "The account key has the wrong length".to_string())
}

/// Seals the account key to another device's public key.
pub fn seal_for_device(
    account_key: &Key32,
    recipient_public: &Key32,
    user_id: &str,
    key_gen: i32,
    recipient_device_id: &str,
) -> Result<Envelope, String> {
    let ephemeral = StaticSecret::from(random32()?);
    let ephemeral_public = public_of(&ephemeral);
    let shared = ephemeral.diffie_hellman(&PublicKey::from(*recipient_public));
    let key = device_key(shared.as_bytes(), &ephemeral_public, recipient_public)?;
    let nonce = random_nonce()?;
    let ciphertext = seal(
        &key,
        &nonce,
        &device_aad(user_id, key_gen, recipient_device_id),
        account_key,
    )?;
    Ok(Envelope {
        ephemeral_public_key: Some(ephemeral_public),
        nonce,
        ciphertext,
    })
}

/// Opens an envelope sealed to this device.
pub fn open_device_envelope(
    device: &StaticSecret,
    envelope: &Envelope,
    user_id: &str,
    key_gen: i32,
    device_id: &str,
) -> Result<Key32, String> {
    let ephemeral = envelope
        .ephemeral_public_key
        .ok_or("That envelope isn't for a device")?;
    let shared = device.diffie_hellman(&PublicKey::from(ephemeral));
    let key = device_key(shared.as_bytes(), &ephemeral, &public_of(device))?;
    open(
        &key,
        &envelope.nonce,
        &device_aad(user_id, key_gen, device_id),
        &envelope.ciphertext,
    )
}

pub fn seal_for_recovery(
    account_key: &Key32,
    recovery: &Key32,
    user_id: &str,
    key_gen: i32,
) -> Result<Envelope, String> {
    let nonce = random_nonce()?;
    let ciphertext = seal(
        &recovery_wrap_key(recovery)?,
        &nonce,
        &recovery_aad(user_id, key_gen),
        account_key,
    )?;
    Ok(Envelope {
        ephemeral_public_key: None,
        nonce,
        ciphertext,
    })
}

pub fn open_recovery_envelope(
    recovery: &Key32,
    envelope: &Envelope,
    user_id: &str,
    key_gen: i32,
) -> Result<Key32, String> {
    open(
        &recovery_wrap_key(recovery)?,
        &envelope.nonce,
        &recovery_aad(user_id, key_gen),
        &envelope.ciphertext,
    )
    .map_err(|_| "That recovery key doesn't match this account".to_string())
}

fn crockford() -> Encoding {
    let mut spec = Specification::new();
    spec.symbols.push_str("0123456789ABCDEFGHJKMNPQRSTVWXYZ");
    // Crockford reads the look-alikes as the digits they resemble, in either case.
    spec.translate.from.push_str("ILOilo");
    spec.translate.to.push_str("110110");
    spec.encoding().expect("valid base32 spec")
}

/// The recovery key as people write it down: Crockford base32 in groups of four.
pub fn format_recovery(recovery: &Key32) -> String {
    let encoded = crockford().encode(recovery);
    encoded
        .as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// Reads a typed recovery key, ignoring dashes, spaces and case.
pub fn parse_recovery(text: &str) -> Result<Key32, String> {
    let cleaned: String = text
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_uppercase())
        .collect();
    let bytes = crockford()
        .decode(cleaned.as_bytes())
        .map_err(|_| "That doesn't look like a recovery key".to_string())?;
    bytes
        .try_into()
        .map_err(|_| "That recovery key is the wrong length".to_string())
}

/// Four words both devices show during approval, so the user can tell it's the same device.
pub fn device_words(public_key: &Key32) -> String {
    let digest = Sha256::digest(public_key);
    digest[..4]
        .iter()
        .map(|byte| WORDS[*byte as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

const WORDS: [&str; 256] = [
    "acorn", "actor", "agent", "alarm", "album", "alley", "amber", "anchor", "angle", "apple",
    "apron", "arena", "arrow", "aspen", "atlas", "autumn", "badge", "bagel", "baker", "bamboo",
    "banjo", "barley", "basin", "beach", "beacon", "berry", "bison", "blade", "blanket", "blossom",
    "bonsai", "boulder", "branch", "brick", "bridge", "bronze", "bubble", "bucket", "cabin",
    "cactus", "camel", "candle", "canoe", "canyon", "carbon", "cargo", "carpet", "castle", "cedar",
    "cello", "chalk", "cherry", "chess", "cider", "circle", "citrus", "clock", "cloud", "clover",
    "cobalt", "comet", "coral", "cotton", "crane", "crater", "cricket", "crown", "crystal",
    "dahlia", "daisy", "delta", "denim", "desert", "diamond", "dolphin", "dragon", "drum", "dune",
    "eagle", "echo", "ember", "emerald", "engine", "falcon", "feather", "fern", "ferry", "fiddle",
    "finch", "flame", "flute", "forest", "fossil", "fountain", "fox", "galaxy", "garden", "garnet",
    "gazelle", "geyser", "ginger", "glacier", "globe", "goblet", "granite", "grape", "gravel",
    "harbor", "harp", "hazel", "heron", "hickory", "honey", "horizon", "iceberg", "igloo",
    "indigo", "island", "ivory", "jacket", "jade", "jaguar", "jasmine", "jelly", "jigsaw",
    "juniper", "kayak", "kernel", "kettle", "kiwi", "koala", "ladder", "lagoon", "lantern",
    "laurel", "lemon", "lily", "linen", "lizard", "llama", "lobster", "lotus", "lunar", "magnet",
    "mango", "maple", "marble", "meadow", "melon", "meteor", "mint", "mirror", "mosaic", "moss",
    "mountain", "mustard", "nectar", "nickel", "noodle", "nutmeg", "oasis", "ocean", "olive",
    "onyx", "opal", "orange", "orbit", "orchid", "otter", "oyster", "paddle", "palm", "panda",
    "paper", "parrot", "pebble", "pepper", "piano", "pillow", "pine", "planet", "plum", "pocket",
    "pollen", "pony", "poppy", "prairie", "prism", "pumpkin", "quartz", "quill", "rabbit", "radar",
    "rain", "raven", "reef", "ribbon", "river", "robin", "rocket", "rose", "ruby", "saddle",
    "saffron", "salmon", "sand", "sapphire", "satin", "scarf", "shell", "silver", "sky", "slate",
    "snow", "spark", "spice", "spruce", "squid", "star", "stone", "storm", "sugar", "summit",
    "sun", "swan", "tea", "temple", "thistle", "thunder", "tiger", "timber", "topaz", "torch",
    "trail", "tulip", "tundra", "turtle", "umbrella", "valley", "velvet", "violet", "volcano",
    "wagon", "walnut", "water", "whale", "willow", "window", "winter", "wren", "yarn", "zebra",
    "zephyr", "zinc", "yonder", "yucca",
];

#[cfg(test)]
mod tests {
    use super::*;

    const USER: &str = "6f1c0a3e-1111-2222-3333-444455556666";
    const DEVICE: &str = "0b2d9a71-aaaa-bbbb-cccc-ddddeeeeffff";

    #[test]
    fn device_envelopes_round_trip_and_reject_tampering() {
        let account = random32().unwrap();
        let device = device_secret(random32().unwrap());
        let envelope = seal_for_device(&account, &public_of(&device), USER, 1, DEVICE).unwrap();
        assert_eq!(envelope.ciphertext.len(), 48);
        assert_eq!(
            open_device_envelope(&device, &envelope, USER, 1, DEVICE).unwrap(),
            account
        );

        let mut tampered = envelope.clone();
        tampered.ciphertext[0] ^= 1;
        assert!(open_device_envelope(&device, &tampered, USER, 1, DEVICE).is_err());
        // The AAD binds the user, generation and recipient.
        assert!(open_device_envelope(&device, &envelope, USER, 2, DEVICE).is_err());
        assert!(open_device_envelope(&device, &envelope, "someone-else", 1, DEVICE).is_err());
        let other = device_secret(random32().unwrap());
        assert!(open_device_envelope(&other, &envelope, USER, 1, DEVICE).is_err());
    }

    #[test]
    fn recovery_envelopes_round_trip_and_reject_wrong_keys() {
        let account = random32().unwrap();
        let recovery = random32().unwrap();
        let envelope = seal_for_recovery(&account, &recovery, USER, 1).unwrap();
        assert!(envelope.ephemeral_public_key.is_none());
        assert_eq!(
            open_recovery_envelope(&recovery, &envelope, USER, 1).unwrap(),
            account
        );
        assert!(open_recovery_envelope(&random32().unwrap(), &envelope, USER, 1).is_err());
        let mut tampered = envelope.clone();
        tampered.nonce[3] ^= 1;
        assert!(open_recovery_envelope(&recovery, &tampered, USER, 1).is_err());
    }

    #[test]
    fn recovery_keys_survive_being_written_down() {
        let recovery = random32().unwrap();
        let shown = format_recovery(&recovery);
        assert!(shown.split('-').all(|group| group.len() <= 4));
        assert_eq!(parse_recovery(&shown).unwrap(), recovery);
        let sloppy = shown
            .to_lowercase()
            .replace('-', " ")
            .replace('0', "o")
            .replace('1', "l");
        assert_eq!(parse_recovery(&sloppy).unwrap(), recovery);
        assert!(parse_recovery("not a key").is_err());
    }

    #[test]
    fn device_words_are_stable_and_four_long() {
        let key = [7u8; 32];
        let words = device_words(&key);
        assert_eq!(words, device_words(&key));
        assert_eq!(words.split(' ').count(), 4);
        let mut unique = WORDS.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), 256);
    }
}
