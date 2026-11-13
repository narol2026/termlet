//! At-rest encryption of credentials (host passwords, identity passwords,
//! GitHub token) stored in the `SQLite` database.
//!
//! A single 256-bit master key is generated on first use and kept in the OS
//! keychain (Keychain Services on macOS, the [`keyring`] crate elsewhere), so it
//! never touches the database. Where no OS keychain exists — a headless Linux
//! session without a Secret Service provider — it falls back to a 0600 key file
//! at `~/.termlet/master.key` and logs a warning.
//!
//! Each secret is sealed with ChaCha20-Poly1305 under a fresh nonce and stored
//! as a self-describing `termlet:v1:`-prefixed base64 string.

use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use std::sync::atomic::{AtomicBool, Ordering};

/// Prefix marking an AEAD-encrypted credential.
const PREFIX: &str = "termlet:v1:";

const KEYRING_SERVICE: &str = "com.narol.termlet";
const KEYRING_ACCOUNT: &str = "master-key";
const MASTER_KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Set once the OS keychain path has failed and we have fallen back to the
/// `~/.termlet/master.key` file. The startup path reads this to surface a
/// one-time toast instead of leaving the fallback completely silent.
static KEYRING_FALLBACK: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
use std::sync::Mutex;

/// Test-only master-key override. When `Some`, [`master_key`] returns it
/// directly, bypassing the OS keychain and key file so secret round-trips are
/// hermetic and never depend on a Secret Service provider or `~/.termlet/`.
#[cfg(test)]
static TEST_KEY_OVERRIDE: Mutex<Option<[u8; MASTER_KEY_LEN]>> = Mutex::new(None);

/// Run `f` with a fixed, deterministic master key (tests only). The override is
/// serialized so concurrent tests cannot observe each other's key.
#[cfg(test)]
pub fn with_test_key<R>(f: impl FnOnce() -> R) -> R {
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap();
    *TEST_KEY_OVERRIDE.lock().unwrap() = Some([7u8; MASTER_KEY_LEN]);
    let result = f();
    *TEST_KEY_OVERRIDE.lock().unwrap() = None;
    result
}

/// Whether the OS keychain was unavailable and the master key fell back to
/// the key file. Consumed for the startup toast in [`crate::app::App::new`].
pub fn keyring_fell_back() -> bool {
    KEYRING_FALLBACK.load(Ordering::Relaxed)
}

/// Encrypt a plaintext secret with the master key, producing a
/// `termlet:v1:`-prefixed base64 string.
pub fn encrypt(plaintext: &str) -> Result<String, String> {
    seal(&master_key()?, plaintext)
}

/// Decrypt a value produced by [`encrypt`].
///
/// Non-encrypted values are rejected so plaintext credentials cannot silently
/// pass through the decryption path.
pub fn decrypt(value: &str) -> Result<String, String> {
    if !value.starts_with(PREFIX) {
        return Err("value is not an encrypted credential".to_string());
    }
    open(&master_key()?, value)
}

/// AEAD-seal `plaintext` under `key`.
fn seal(key: &[u8; MASTER_KEY_LEN], plaintext: &str) -> Result<String, String> {
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|e| format!("invalid master key: {e}"))?;

    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|e| format!("failed to generate nonce: {e}"))?;

    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|e| format!("encryption failed: {e}"))?;

    let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);

    Ok(format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(payload)
    ))
}

/// AEAD-open a value produced by [`seal`], under `key`.
fn open(key: &[u8; MASTER_KEY_LEN], value: &str) -> Result<String, String> {
    let encoded = value
        .strip_prefix(PREFIX)
        .ok_or_else(|| "not an encrypted value".to_string())?;

    let payload = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("invalid ciphertext encoding: {e}"))?;
    if payload.len() < NONCE_LEN {
        return Err("ciphertext too short".to_string());
    }

    let (nonce, ciphertext) = payload.split_at(NONCE_LEN);
    let cipher =
        ChaCha20Poly1305::new_from_slice(key).map_err(|e| format!("invalid master key: {e}"))?;

    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|e| format!("decryption failed (missing or wrong master key): {e}"))?;

    String::from_utf8(plaintext).map_err(|e| format!("decrypted secret is not UTF-8: {e}"))
}

/// Retrieve the 256-bit master key. Tries the OS keychain first; if that is
/// unavailable, falls back to a 0600 key file under `~/.termlet/`.
fn master_key() -> Result<[u8; MASTER_KEY_LEN], String> {
    #[cfg(test)]
    {
        if let Some(key) = TEST_KEY_OVERRIDE.lock().unwrap().as_ref() {
            return Ok(*key);
        }
    }
    match keyring_master_key() {
        Ok(key) => return Ok(key),
        Err(e) => {
            KEYRING_FALLBACK.store(true, Ordering::Relaxed);
            crate::core::diagnostics::log(
                crate::core::diagnostics::Level::Warn,
                "CRYPTO",
                format!("OS keychain unavailable ({e}); falling back to master key file"),
            );
        }
    }
    keyfile_master_key()
}

/// Load (or generate and store) the master key from the OS keychain.
#[cfg(not(target_os = "macos"))]
fn keyring_master_key() -> Result<[u8; MASTER_KEY_LEN], String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("keychain access failed: {e}"))?;

    match entry.get_password() {
        Ok(stored) => decode_key(&stored),
        Err(keyring::Error::NoEntry) => {
            let key = generate_key()?;
            let encoded = encode_key(&key);
            entry
                .set_password(&encoded)
                .map_err(|e| format!("failed to store master key in keychain: {e}"))?;
            Ok(key)
        }
        Err(e) => Err(format!("failed to read master key from keychain: {e}")),
    }
}

/// Load (or generate and store) the master key using the modern macOS
/// `SecItem*` Keychain APIs.
#[cfg(target_os = "macos")]
fn keyring_master_key() -> Result<[u8; MASTER_KEY_LEN], String> {
    use security_framework::passwords::{PasswordOptions, generic_password, set_generic_password};

    // errSecItemNotFound (macOS Security framework status code).
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    let read_options = PasswordOptions::new_generic_password(KEYRING_SERVICE, KEYRING_ACCOUNT);

    match generic_password(read_options) {
        Ok(bytes) => {
            let stored = std::str::from_utf8(&bytes)
                .map_err(|e| format!("master key in keychain is not UTF-8: {e}"))?;
            decode_key(stored)
        }
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => {
            let key = generate_key()?;
            let encoded = encode_key(&key);

            set_generic_password(KEYRING_SERVICE, KEYRING_ACCOUNT, encoded.as_bytes())
                .map_err(|e| format!("failed to store master key in keychain: {e}"))?;
            Ok(key)
        }
        Err(e) => Err(format!("failed to read master key from keychain: {e}")),
    }
}

/// Load (or generate and store) the master key from a 0600 key file.
fn keyfile_master_key() -> Result<[u8; MASTER_KEY_LEN], String> {
    let path = keyfile_path();
    match std::fs::read_to_string(&path) {
        Ok(encoded) => decode_key(encoded.trim()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = generate_key()?;
            crate::core::paths::write_private_atomic(&path, encode_key(&key).as_bytes())
                .map_err(|e| format!("failed to write master key file {}: {e}", path.display()))?;
            Ok(key)
        }
        Err(e) => Err(format!(
            "failed to read master key file {}: {e}",
            path.display()
        )),
    }
}

fn keyfile_path() -> std::path::PathBuf {
    crate::core::paths::config_dir().join("master.key")
}

fn generate_key() -> Result<[u8; MASTER_KEY_LEN], String> {
    let mut key = [0u8; MASTER_KEY_LEN];
    getrandom::getrandom(&mut key).map_err(|e| format!("failed to generate master key: {e}"))?;
    Ok(key)
}

fn encode_key(key: &[u8; MASTER_KEY_LEN]) -> String {
    base64::engine::general_purpose::STANDARD.encode(key)
}

fn decode_key(encoded: &str) -> Result<[u8; MASTER_KEY_LEN], String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|e| format!("invalid master key encoding: {e}"))?;
    let len = bytes.len();
    let key: [u8; MASTER_KEY_LEN] = bytes
        .try_into()
        .map_err(|_error| format!("master key must be {MASTER_KEY_LEN} bytes, got {len}"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: [u8; MASTER_KEY_LEN] = [7u8; MASTER_KEY_LEN];

    #[test]
    fn seal_open_roundtrip() {
        let encrypted = seal(&TEST_KEY, "hunter2").unwrap();
        assert!(encrypted.starts_with(PREFIX));
        assert_ne!(encrypted, "hunter2");
        assert_eq!(open(&TEST_KEY, &encrypted).unwrap(), "hunter2");
    }

    #[test]
    fn non_prefixed_value_is_rejected() {
        decrypt("plaintext-legacy").unwrap_err();
        decrypt("").unwrap_err();
    }

    #[test]
    fn ciphertext_is_nonce_unique() {
        // Same plaintext must not produce identical ciphertext (fresh nonce).
        let a = seal(&TEST_KEY, "same").unwrap();
        let b = seal(&TEST_KEY, "same").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn wrong_key_fails() {
        let encrypted = seal(&TEST_KEY, "secret").unwrap();
        let other = [9u8; MASTER_KEY_LEN];
        open(&other, &encrypted).unwrap_err();
    }
}
