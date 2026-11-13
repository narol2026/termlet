//! SSH key generation backed by russh and the Rust SSH key libraries.
//!
//! This module has no dependency on the system `ssh` or `ssh-keygen`
//! executables. It generates the private key in memory, optionally encrypts it
//! with the user's passphrase, and writes an OpenSSH private key plus its
//! matching `.pub` file.

// Rust guideline compliant 2026-10-09

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, EcdsaCurve, HashAlg, PrivateKey};
use secrecy::{ExposeSecret, SecretString};

use crate::core::data::KeyType;

/// Public metadata returned after a key pair is written.
#[derive(Debug)]
pub(crate) struct GeneratedKey {
    pub(crate) public_key: String,
    pub(crate) fingerprint: String,
}

/// Generate and write an OpenSSH key pair without replacing existing files.
pub(crate) fn generate_keypair(
    path: &Path,
    key_type: KeyType,
    passphrase: &SecretString,
    comment: &str,
) -> Result<GeneratedKey, String> {
    let public_path = public_key_path(path)?;
    if path.exists() {
        return Err(format!(
            "refusing to overwrite existing private key at {}",
            path.display()
        ));
    }
    if public_path.exists() {
        return Err(format!(
            "refusing to overwrite existing public key at {}",
            public_path.display()
        ));
    }

    prepare_parent(path)?;

    let mut rng = rand::rng();
    let mut private_key = PrivateKey::random(&mut rng, algorithm(key_type))
        .map_err(|error| format!("failed to generate {} key: {error}", key_type.label()))?;
    private_key.set_comment(comment);

    let public_key = private_key
        .public_key()
        .to_openssh()
        .map_err(|error| format!("failed to encode public key: {error}"))?;
    let fingerprint = private_key
        .public_key()
        .fingerprint(HashAlg::Sha256)
        .to_string();

    if !passphrase.expose_secret().is_empty() {
        private_key = private_key
            .encrypt(&mut rng, passphrase.expose_secret().as_bytes())
            .map_err(|error| format!("failed to encrypt private key: {error}"))?;
    }

    let private_key = private_key
        .to_openssh(LineEnding::LF)
        .map_err(|error| format!("failed to encode private key: {error}"))?;
    let mut public_key_file = public_key.clone();
    if !public_key_file.ends_with('\n') {
        public_key_file.push('\n');
    }

    write_new_file(path, private_key.as_bytes(), true)?;
    if let Err(error) = write_new_file(&public_path, public_key_file.as_bytes(), false) {
        let _ = fs::remove_file(path);
        return Err(error);
    }

    Ok(GeneratedKey {
        public_key,
        fingerprint,
    })
}

/// Return the `.pub` path that belongs to a private key path.
pub(crate) fn public_key_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("private key path has no file name: {}", path.display()))?;
    let mut public_name = OsString::from(file_name);
    public_name.push(".pub");
    Ok(path.with_file_name(public_name))
}

fn algorithm(key_type: KeyType) -> Algorithm {
    match key_type {
        KeyType::ED25519 => Algorithm::Ed25519,
        KeyType::ECDSA => Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        },
        KeyType::RSA => Algorithm::Rsa { hash: None },
    }
}

fn prepare_parent(path: &Path) -> Result<(), String> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };

    #[cfg(unix)]
    let existed = parent.exists();
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "failed to create key directory {}: {error}",
            parent.display()
        )
    })?;

    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!(
                "failed to restrict key directory {}: {error}",
                parent.display()
            )
        })?;
    }

    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8], private: bool) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(not(unix))]
    let _ = private;

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 })
    };

    let mut file = options
        .open(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    let write_result = file
        .write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("failed to write {}: {error}", path.display()));
    drop(file);

    if write_result.is_err() {
        let _ = fs::remove_file(path);
    }
    write_result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(tag: &str) -> PathBuf {
        crate::core::test_support::scratch_dir(&format!(
            "termlet-keygen-{}-{tag}",
            std::process::id()
        ))
        .join(".ssh")
        .join("id_ed25519")
    }

    #[test]
    fn key_algorithms_match_the_offered_types() {
        assert_eq!(algorithm(KeyType::ED25519), Algorithm::Ed25519);
        assert_eq!(
            algorithm(KeyType::ECDSA),
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP256
            }
        );
        assert_eq!(algorithm(KeyType::RSA), Algorithm::Rsa { hash: None });
    }

    #[test]
    fn ed25519_generation_writes_matching_private_and_public_keys() {
        let path = test_path("ed25519");
        let generated = generate_keypair(
            &path,
            KeyType::ED25519,
            &SecretString::from(String::new()),
            "termlet-test",
        )
        .expect("the key pair is generated");

        assert!(path.is_file());
        assert!(path.with_extension("pub").is_file());
        assert!(generated.public_key.starts_with("ssh-ed25519 "));
        assert!(generated.fingerprint.starts_with("SHA256:"));

        let private_key =
            PrivateKey::read_openssh_file(&path).expect("the private key is OpenSSH format");
        assert_eq!(private_key.algorithm(), Algorithm::Ed25519);
        assert!(!private_key.is_encrypted());
        assert_eq!(
            private_key.public_key().to_openssh().expect("public key"),
            generated.public_key
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .expect("private key metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn a_passphrase_encrypts_the_private_key() {
        let path = test_path("encrypted");
        generate_keypair(
            &path,
            KeyType::ECDSA,
            &SecretString::from("correct horse"),
            "termlet-test",
        )
        .expect("the encrypted key pair is generated");

        let private_key =
            PrivateKey::read_openssh_file(&path).expect("the encrypted key is readable");
        assert!(private_key.is_encrypted());
        assert!(
            private_key.decrypt("wrong passphrase").is_err(),
            "the wrong passphrase must not decrypt the key"
        );
        assert_eq!(
            private_key
                .decrypt("correct horse")
                .expect("the passphrase decrypts the key")
                .algorithm(),
            Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP256
            }
        );
    }

    #[test]
    fn rsa_generation_uses_the_rsa_algorithm() {
        let path = test_path("rsa").with_file_name("id_rsa");
        let generated = generate_keypair(
            &path,
            KeyType::RSA,
            &SecretString::from(String::new()),
            "termlet-test",
        )
        .expect("the RSA key pair is generated");

        assert!(generated.public_key.starts_with("ssh-rsa "));
        assert_eq!(
            PrivateKey::read_openssh_file(&path)
                .expect("the RSA key is readable")
                .algorithm(),
            Algorithm::Rsa { hash: None }
        );
    }

    #[test]
    fn generation_never_overwrites_an_existing_key() {
        let path = test_path("existing");
        generate_keypair(
            &path,
            KeyType::ED25519,
            &SecretString::from(String::new()),
            "first",
        )
        .expect("the first key pair is generated");
        let original = fs::read(&path).expect("read the first private key");

        let error = generate_keypair(
            &path,
            KeyType::ED25519,
            &SecretString::from(String::new()),
            "second",
        )
        .expect_err("an existing private key is never replaced");

        assert!(error.contains("refusing to overwrite"), "{error}");
        assert_eq!(fs::read(&path).expect("read the original key"), original);
    }
}
