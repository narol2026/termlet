//! Host-list offline export/import: JSON payload serialization + age
//! (passphrase) encryption.
//!
//! Field policy:
//! - Saved SSH passwords ARE exported inside the encrypted payload (per user
//!   decision).
//! - `id` is not exported; imported hosts get fresh IDs from the caller.
//! - `key_path` (the path to the private key file, not the key material) is
//!   always exported, like passwords.
//! - `tags` are exported so imports don't lose categorization.
//! - `expected_fingerprint` is a session-level value (`None` on persisted
//!   hosts) and is omitted.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use secrecy::ExposeSecret;

use crate::core::data::{AuthType, Host};

/// A host as it appears in an export file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportHost {
    pub name: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    /// `"Password"` or `"Key"` — stable string, matches the DB convention.
    pub auth_type: String,
    /// Saved SSH password. Always exported per user decision; `None` for
    /// key-auth hosts or hosts without a stored password.
    pub password: Option<String>,
    /// Path to the private key file (never the key material). Always exported.
    pub key_path: Option<String>,
    /// Name of the saved host used as this host's `ProxyJump`. IDs are local and
    /// therefore not exported.
    pub jump_host_name: Option<String>,
    pub os_type: String,
    pub tags: Vec<String>,
}

/// Versioned envelope so the schema can evolve without breaking importers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportFile {
    pub version: u32,
    pub exported_at: String,
    pub host_count: usize,
    pub hosts: Vec<ExportHost>,
}

pub const EXPORT_VERSION: u32 = 1;
/// Default file name for a portable host export.
pub const HOST_EXPORT_FILE_NAME: &str = "termlet-hosts.termlet";
/// Display name for the host export file filter.
pub const HOST_EXPORT_FILTER_NAME: &str = "TERMLET";
/// Extension used by new host exports.
pub const HOST_EXPORT_EXTENSION: &str = "termlet";
/// Legacy extension still accepted for older host exports.
pub const HOST_EXPORT_LEGACY_EXTENSION: &str = "age";
/// File extensions accepted by the host import picker.
pub const HOST_IMPORT_EXTENSIONS: &[&str] = &[HOST_EXPORT_EXTENSION, HOST_EXPORT_LEGACY_EXTENSION];

/// Serialize a host list into pretty JSON bytes (never encrypted).
pub fn hosts_to_export_json(hosts: &[Host]) -> Result<Vec<u8>, String> {
    let list = hosts
        .iter()
        .map(|h| ExportHost {
            name: h.name.clone(),
            hostname: h.hostname.clone(),
            port: h.port,
            username: h.username.clone(),
            auth_type: auth_type_to_str(h.auth_type).to_string(),
            password: h.password.as_ref().map(|p| p.expose_secret().to_string()),
            key_path: h.key_path.clone(),
            jump_host_name: h.jump_host_id.and_then(|jump_host_id| {
                hosts
                    .iter()
                    .find(|candidate| candidate.id == jump_host_id)
                    .map(|candidate| candidate.name.clone())
            }),
            os_type: h.os_type.to_str().to_string(),
            tags: h.tags.clone(),
        })
        .collect::<Vec<_>>();

    let file = ExportFile {
        version: EXPORT_VERSION,
        exported_at: now_timestamp(),
        host_count: list.len(),
        hosts: list,
    };

    serde_json::to_vec_pretty(&file).map_err(|e| format!("failed to serialize export: {e}"))
}

/// Parse decrypted export JSON and validate it.
pub fn parse_export_json(text: &str) -> Result<Vec<ExportHost>, String> {
    let file: ExportFile =
        serde_json::from_str(text).map_err(|e| format!("invalid export file: {e}"))?;

    if file.version != EXPORT_VERSION {
        return Err(format!(
            "unsupported export version {} (expected {EXPORT_VERSION})",
            file.version
        ));
    }

    for h in &file.hosts {
        if h.hostname.trim().is_empty() {
            return Err("export contains a host with an empty hostname".to_string());
        }
        if h.port == 0 {
            return Err(format!("export contains invalid port 0 for \"{}\"", h.name));
        }
        if !matches!(h.auth_type.as_str(), "Password" | "Key") {
            return Err(format!(
                "export contains unknown auth_type \"{}\" for \"{}\"",
                h.auth_type, h.name
            ));
        }
    }

    Ok(file.hosts)
}

/// ASCII armor header that prefixes armored age files produced by
/// `encrypt_export`. Binary age files start with `age-encryption.org/v1`.
const ARMOR_HEADER: &[u8] = b"-----BEGIN AGE ENCRYPTED FILE-----";

/// Encrypt plaintext bytes into an ASCII-armored age file.
/// The passphrase must be non-empty (callers guard this).
pub fn encrypt_export(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    use age::armor::{ArmoredWriter, Format};
    use age::secrecy::SecretString;
    use std::io::Write;

    if passphrase.trim().is_empty() {
        return Err("cannot encrypt with an empty passphrase".to_string());
    }

    let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(passphrase.to_owned()));

    let mut out = Vec::new();
    let mut armored = ArmoredWriter::wrap_output(&mut out, Format::AsciiArmor)
        .map_err(|e| format!("failed to start armor: {e}"))?;
    let mut writer = encryptor
        .wrap_output(&mut armored)
        .map_err(|e| format!("failed to start encryption: {e}"))?;
    writer
        .write_all(data)
        .map_err(|e| format!("failed to write ciphertext: {e}"))?;
    // finish() is mandatory — without it the trailer is missing and the file
    // cannot be decrypted.
    writer
        .finish()
        .map_err(|e| format!("failed to finish encryption: {e}"))?;
    armored
        .finish()
        .map_err(|e| format!("failed to finish armor: {e}"))?;
    Ok(out)
}

/// Decrypt an age file (binary or armored) with a passphrase.
pub fn decrypt_export(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    use age::secrecy::SecretString;
    use std::io::Read;

    let input: Box<dyn Read + '_> = if data.starts_with(ARMOR_HEADER) {
        Box::new(age::armor::ArmoredReader::new(data))
    } else {
        Box::new(data)
    };
    let decryptor = age::Decryptor::new(input).map_err(|e| format!("not a valid age file: {e}"))?;

    // age 0.11's scrypt Identity caps the work factor it will accept at a
    // live-benchmarked value (max = measured_target + 4). That makes
    // decryption machine/load dependent: a file encrypted with a higher
    // work factor (e.g. the age backup at log2N=19, produced on a fast
    // machine) can be rejected with `ExcessiveWork` even with the correct
    // passphrase. The CLI accepts the full spec range, so raise the cap to
    // match. Bound it at 22 (age spec max) to still guard against malicious
    // files, per the age crate's own recommendation.
    let mut identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
    identity.set_max_work_factor(22);
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| format!("decryption failed: {e}"))?;

    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .map_err(|e| format!("failed to read plaintext: {e}"))?;
    Ok(out)
}

fn auth_type_to_str(a: AuthType) -> &'static str {
    match a {
        AuthType::Password => "Password",
        AuthType::Key => "Key",
    }
}

fn now_timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::{HostId, OsType, Protocol};
    use secrecy::SecretString;

    fn sample_hosts() -> Vec<Host> {
        vec![
            Host {
                id: HostId(1),
                name: "web".to_string(),
                hostname: "10.0.0.1".to_string(),
                port: 22,
                protocol: Protocol::Ssh,
                username: "root".to_string(),
                auth_type: AuthType::Key,
                jump_host_id: None,
                identity_id: None,
                key_path: Some("~/.ssh/id_ed25519".to_string()),
                key_passphrase: None,
                password: Some(SecretString::from("super-secret")),
                os_type: OsType::Ubuntu,
                tags: vec!["prod".to_string(), "eu-west".to_string()],
                expected_fingerprint: None,
            },
            Host {
                id: HostId(2),
                name: "db".to_string(),
                hostname: "10.0.0.2".to_string(),
                port: 2222,
                protocol: Protocol::Ssh,
                username: "deploy".to_string(),
                auth_type: AuthType::Password,
                jump_host_id: None,
                identity_id: None,
                key_path: None,
                key_passphrase: None,
                password: Some(SecretString::from("hunter2")),
                os_type: OsType::Unknown,
                tags: vec![],
                expected_fingerprint: None,
            },
        ]
    }

    #[test]
    fn export_includes_password_and_key_path_never_id() {
        let json = hosts_to_export_json(&sample_hosts()).unwrap();
        let text = String::from_utf8(json).unwrap();
        assert!(text.contains("super-secret"), "password should be exported");
        assert!(text.contains("hunter2"), "password should be exported");
        assert!(text.contains("\"password\""), "password field present");
        assert!(text.contains("id_ed25519"), "key_path should be exported");
        assert!(!text.contains("\"id\""), "id field must not be present");
        assert!(text.contains("10.0.0.1"));
    }

    #[test]
    fn export_uses_termlet_extension_and_import_accepts_legacy_age() {
        assert_eq!(
            std::path::Path::new(HOST_EXPORT_FILE_NAME).extension(),
            Some(std::ffi::OsStr::new(HOST_EXPORT_EXTENSION))
        );
        assert_eq!(HOST_IMPORT_EXTENSIONS, ["termlet", "age"]);
    }

    #[test]
    fn parse_roundtrip() {
        let json = hosts_to_export_json(&sample_hosts()).unwrap();
        let text = String::from_utf8(json).unwrap();
        let parsed = parse_export_json(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].hostname, "10.0.0.1");
        assert_eq!(parsed[0].auth_type, "Key");
        assert_eq!(parsed[0].password.as_deref(), Some("super-secret"));
        assert_eq!(parsed[0].key_path.as_deref(), Some("~/.ssh/id_ed25519"));
        assert_eq!(parsed[1].auth_type, "Password");
        assert_eq!(parsed[1].password.as_deref(), Some("hunter2"));
        assert_eq!(parsed[0].os_type, "Ubuntu");
        assert_eq!(parsed[0].tags, vec!["prod", "eu-west"]);
        assert!(parsed[1].tags.is_empty());
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let err = parse_export_json(r#"{"version":99,"exported_at":"","host_count":0,"hosts":[]}"#)
            .unwrap_err();
        assert!(err.contains("version"));
    }

    #[test]
    fn parse_rejects_bad_auth_type() {
        let err = parse_export_json(
            r#"{"version":1,"exported_at":"","host_count":1,"hosts":[{"name":"x","hostname":"h","port":22,"username":"u","auth_type":"Nope","key_path":null,"os_type":"Unknown","tags":[]}]}"#,
        )
        .unwrap_err();
        assert!(err.contains("auth_type"));
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let data = b"hello termlet export";
        let encrypted = encrypt_export(data, "correct horse battery staple").unwrap();
        let armored = String::from_utf8(encrypted.clone()).unwrap();
        assert!(armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
        assert!(!armored.contains("hello termlet export"));

        let decrypted = decrypt_export(&encrypted, "correct horse battery staple").unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn wrong_password_fails() {
        let encrypted = encrypt_export(b"data", "right").unwrap();
        decrypt_export(&encrypted, "wrong").unwrap_err();
    }

    #[test]
    fn empty_passphrase_encrypt_rejected() {
        encrypt_export(b"data", "   ").unwrap_err();
    }
}
