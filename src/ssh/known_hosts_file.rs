//! OpenSSH `~/.ssh/known_hosts` file I/O.
//!
//! Wraps `russh::keys::known_hosts` for key lookups / appends and
//! `ssh_key::known_hosts::KnownHosts` for full-file parsing, so the
//! rest of the application only deals with [`crate::core::data::KnownHost`].

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use russh::keys::HashAlg;
use russh::keys::ssh_key;

use crate::core::data::KnownHost;

/// Canonical path: `~/.ssh/known_hosts`.
pub fn path() -> PathBuf {
    crate::core::paths::home_dir()
        .join(".ssh")
        .join("known_hosts")
}

/// Advisory lock protecting our own writers from each other.  It is
/// deliberately kept next to `known_hosts` so every Termlet process locks the
/// same file.  The lock is released automatically when the process exits or
/// when this guard is dropped.
struct KnownHostsLock {
    file: File,
}

impl KnownHostsLock {
    fn acquire(target: &Path) -> Result<Self, String> {
        let parent = target
            .parent()
            .ok_or_else(|| format!("known_hosts path has no parent: {}", target.display()))?;
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;

        let lock_path = parent.join(".known_hosts.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| format!("failed to open known_hosts lock: {e}"))?;

        file.lock()
            .map_err(|e| format!("failed to lock known_hosts: {e}"))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }

        Ok(Self { file })
    }
}

impl Drop for KnownHostsLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn read_optional(target: &Path) -> Result<String, String> {
    match fs::read_to_string(target) {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(format!(
            "failed to read known_hosts {}: {err}",
            target.display()
        )),
    }
}

/// Resolve a symlinked `known_hosts` to its actual target before writing.
/// Atomic rename would otherwise replace the symlink itself instead of the
/// file it points to.
fn resolve_write_target(target: &Path) -> PathBuf {
    fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf())
}

fn backup_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("known_hosts");
    target.with_file_name(format!(".{name}.bak"))
}

fn temp_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("known_hosts");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    target.with_file_name(format!(".{name}.tmp.{}.{}", process::id(), nanos))
}

#[cfg(unix)]
fn sync_path(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("failed to sync {}: {e}", path.display()))
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the Unix implementation can fail; the shared call sites keep one signature"
)]
fn sync_path(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Replace `target` atomically (or as close as the platform permits) with
/// `data`.  When `make_backup` is true and the target already exists, a
/// `.known_hosts.bak` sibling is written before the replacement.
fn write_atomic_with_backup(target: &Path, data: &[u8], make_backup: bool) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }

    if make_backup && target.exists() {
        let backup = backup_path(target);
        fs::copy(target, &backup).map_err(|e| format!("failed to back up known_hosts: {e}"))?;
        sync_path(&backup)?;
    }

    let tmp = temp_path(target);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("failed to create known_hosts temp file: {e}"))?;

    if let Ok(metadata) = fs::metadata(target) {
        file.set_permissions(metadata.permissions())
            .map_err(|e| format!("failed to preserve known_hosts permissions: {e}"))?;
    }

    let write_result = (|| -> Result<(), String> {
        file.write_all(data)
            .map_err(|e| format!("failed to write known_hosts temp file: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("failed to sync known_hosts temp file: {e}"))
    })();
    drop(file);

    if let Err(err) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp, target) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("failed to replace known_hosts: {err}"));
    }

    if let Some(parent) = target.parent()
        && let Err(err) = sync_path(parent)
    {
        return Err(format!("known_hosts was replaced, but {err}"));
    }
    Ok(())
}

fn format_host_line(hostname: &str, port: u16, key: &ssh_key::PublicKey) -> Result<String, String> {
    let openssh = key
        .to_openssh()
        .map_err(|e| format!("failed to encode public key: {e}"))?;
    let host = if port == 22 {
        hostname.to_string()
    } else {
        format!("[{hostname}]:{port}")
    };
    Ok(format!("{host} {openssh}\n"))
}

/// Return the raw 1-indexed file line numbers whose host patterns match
/// `hostname:port`. This reuses russh's `known_hosts` matcher, so hashed
/// entries (`|1|...`) are handled correctly as well as plain hostnames.
fn matching_line_numbers(target: &Path, hostname: &str, port: u16) -> Result<Vec<usize>, String> {
    russh::keys::known_hosts::known_host_keys_path(hostname, port, target)
        .map(|keys| keys.into_iter().map(|(line, _)| line).collect())
        .map_err(|e| format!("failed to read known_hosts for {hostname}:{port}: {e}"))
}

/// Parse the entire `known_hosts` file and return all entries as
/// [`KnownHost`] structs.  Returns an empty `Vec` if the file does not
/// exist or is empty.
pub fn load_all() -> Result<Vec<KnownHost>, String> {
    let p = path();
    if !p.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&p)
        .map_err(|e| format!("failed to read known_hosts {}: {e}", p.display()))?;

    let mut hosts = Vec::new();
    for (raw_index, raw_line) in content.lines().enumerate() {
        let line = raw_line
            .split_once('#')
            .map_or(raw_line, |(entry, _)| entry)
            .trim_end();
        if line.is_empty() {
            continue;
        }
        let entry: ssh_key::known_hosts::Entry = line.parse().map_err(|e| {
            format!(
                "failed to parse known_hosts file {} line {}: {e}",
                p.display(),
                raw_index + 1
            )
        })?;
        hosts.push(known_host_from_entry(&entry, raw_index + 1));
    }
    Ok(hosts)
}

fn known_host_from_entry(entry: &ssh_key::known_hosts::Entry, line_number: usize) -> KnownHost {
    let pubkey = entry.public_key();
    let pubkey_str = pubkey.to_openssh().unwrap_or_default();
    let fingerprint = pubkey.fingerprint(HashAlg::Sha256).to_string();
    let key_type = pubkey_str
        .split_whitespace()
        .next()
        .unwrap_or("ssh-unknown")
        .to_string();

    let (hostname, port, raw_pattern) = match entry.host_patterns() {
        ssh_key::known_hosts::HostPatterns::Patterns(patterns) => {
            let raw = patterns.join(",");
            let first = patterns.first().map_or("", std::string::String::as_str);
            let (h, p) = parse_host_port(first);
            (h.unwrap_or_else(|| first.to_string()), p.unwrap_or(22), raw)
        }
        ssh_key::known_hosts::HostPatterns::HashedName { .. } => {
            let raw = entry.host_patterns().to_string();
            ("(hashed)".to_string(), 22, raw)
        }
    };

    let marker = entry.marker().map(|m| match m {
        ssh_key::known_hosts::Marker::CertAuthority => "@cert-authority".to_string(),
        ssh_key::known_hosts::Marker::Revoked => "@revoked".to_string(),
    });

    KnownHost {
        line_number,
        hostname,
        port,
        key_type,
        public_key: pubkey_str,
        fingerprint,
        marker,
        raw_pattern,
    }
}

/// Parse `[hostname]:port` or plain `hostname` from a single host pattern.
/// Returns `(hostname, port)`; port is `None` when not specified.
fn parse_host_port(pattern: &str) -> (Option<String>, Option<u16>) {
    // Pattern like `[hostname]:2222`
    if let Some(rest) = pattern.strip_prefix('[')
        && let Some((host, port_str)) = rest.rsplit_once("]:")
        && let Ok(port) = port_str.parse::<u16>()
    {
        return (Some(host.to_string()), Some(port));
    }
    // Plain hostname (no port)
    (Some(pattern.to_string()), None)
}

/// Return all SHA256 fingerprints matching `hostname:port` in the
/// `known_hosts` file.
pub fn fingerprints(hostname: &str, port: u16) -> Vec<String> {
    russh::keys::known_hosts::known_host_keys_path(hostname, port, path())
        .unwrap_or_default()
        .into_iter()
        .map(|(_, pk)| pk.fingerprint(HashAlg::Sha256).to_string())
        .collect()
}

/// Convenience: returns `true` when at least one key for `hostname:port`
/// exists in the `known_hosts` file.
pub fn is_known(hostname: &str, port: u16) -> bool {
    !fingerprints(hostname, port).is_empty()
}

/// Append a new host key entry to the `known_hosts` file, skipping if an
/// identical key (same hostname + port + public key) already exists.
///
/// `public_key` must be a valid OpenSSH-format key line
/// (`"key-type base64-data"`).
pub fn append(hostname: &str, port: u16, public_key: &str) -> Result<(), String> {
    let target = resolve_write_target(&path());
    let _lock = KnownHostsLock::acquire(&target)?;
    append_locked(&target, hostname, port, public_key)
}

fn append_locked(target: &Path, hostname: &str, port: u16, public_key: &str) -> Result<(), String> {
    // Skip if an identical key already exists for this hostname:port.
    let key: ssh_key::PublicKey = public_key
        .parse()
        .map_err(|e| format!("failed to parse public key: {e}"))?;
    let new_fp = key.fingerprint(HashAlg::Sha256).to_string();

    for (_, existing_key) in
        russh::keys::known_hosts::known_host_keys_path(hostname, port, target).unwrap_or_default()
    {
        if existing_key.fingerprint(HashAlg::Sha256).to_string() == new_fp
            && existing_key.to_openssh().unwrap_or_default() == key.to_openssh().unwrap_or_default()
        {
            return Ok(()); // already exists — no-op
        }
    }

    let mut content = read_optional(target)?;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format_host_line(hostname, port, &key)?);
    write_atomic_with_backup(target, content.as_bytes(), true)
}

/// Atomically replace every existing entry for `hostname:port` with one new
/// key entry.  This is the operation used when a user accepts a changed or
/// previously unknown server key.
pub fn replace_host(hostname: &str, port: u16, public_key: &str) -> Result<(), String> {
    let target = resolve_write_target(&path());
    let _lock = KnownHostsLock::acquire(&target)?;
    replace_host_locked(&target, hostname, port, public_key)
}

fn replace_host_locked(
    target: &Path,
    hostname: &str,
    port: u16,
    public_key: &str,
) -> Result<(), String> {
    let key: ssh_key::PublicKey = public_key
        .parse()
        .map_err(|e| format!("failed to parse public key: {e}"))?;
    let remove_lines = matching_line_numbers(target, hostname, port)?;

    let content = read_optional(target)?;
    let mut out = String::with_capacity(content.len() + 128);
    for (i, line) in content.lines().enumerate() {
        if remove_lines.contains(&(i + 1)) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format_host_line(hostname, port, &key)?);
    write_atomic_with_backup(target, out.as_bytes(), true)
}

/// Delete the entry at the given 1-indexed line number from the
/// `known_hosts` file.  Rewrites the file without that line.
pub fn delete_line(line_number: usize) -> Result<(), String> {
    let target = resolve_write_target(&path());
    if !target.exists() {
        return Err("known_hosts file does not exist".to_string());
    }
    let _lock = KnownHostsLock::acquire(&target)?;
    delete_line_locked(&target, line_number)
}

fn delete_line_locked(target: &Path, line_number: usize) -> Result<(), String> {
    delete_lines_locked(target, &[line_number]).map(|_| ())
}

fn delete_lines_locked(target: &Path, line_numbers: &[usize]) -> Result<Vec<usize>, String> {
    if line_numbers.is_empty() {
        return Err("no known_hosts lines to delete".to_string());
    }

    let content = read_optional(target)?;
    let lines: Vec<&str> = content.lines().collect();
    for line_number in line_numbers {
        if *line_number == 0 || *line_number > lines.len() {
            return Err(format!(
                "line {line_number} out of range (file has {} lines)",
                lines.len()
            ));
        }
    }

    let mut out = String::with_capacity(content.len());
    for (i, line) in lines.iter().enumerate() {
        if !line_numbers.contains(&(i + 1)) {
            out.push_str(line);
            out.push('\n');
        }
    }

    write_atomic_with_backup(target, out.as_bytes(), true)?;
    Ok(line_numbers.to_vec())
}

/// Delete every `known_hosts` entry whose host pattern matches
/// `hostname:port`, including hashed entries. Returns the raw line numbers
/// that were removed.
pub fn delete_host(hostname: &str, port: u16) -> Result<Vec<usize>, String> {
    let target = resolve_write_target(&path());
    if !target.exists() {
        return Err("known_hosts file does not exist".to_string());
    }
    let _lock = KnownHostsLock::acquire(&target)?;

    delete_host_locked(&target, hostname, port)
}

fn delete_host_locked(target: &Path, hostname: &str, port: u16) -> Result<Vec<usize>, String> {
    let line_numbers = matching_line_numbers(target, hostname, port)?;
    if line_numbers.is_empty() {
        return Err(format!("no known_hosts entry matches {hostname}:{port}"));
    }
    delete_lines_locked(target, &line_numbers)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G2sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X";

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("termlet-known-hosts-{label}-{}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn known_hosts(&self) -> PathBuf {
            self.path.join("known_hosts")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn append_is_atomic_and_keeps_a_backup() {
        let dir = TestDir::new("append");
        let target = dir.known_hosts();
        fs::write(&target, format!("old.example {KEY_A}\n")).unwrap();

        append_locked(&target, "new.example", 22, KEY_B).unwrap();

        let content = fs::read_to_string(&target).unwrap();
        assert!(content.contains("old.example"));
        assert!(content.contains("new.example"));
        assert_eq!(
            fs::read_to_string(backup_path(&target)).unwrap(),
            format!("old.example {KEY_A}\n")
        );
        assert!(!fs::read_dir(&dir.path).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp.")
        }));
    }

    #[test]
    fn replace_host_replaces_all_matching_entries_in_one_write() {
        let dir = TestDir::new("replace");
        let target = dir.known_hosts();
        let original = format!(
            "example.com {KEY_A}\nexample.com,alias.example {KEY_B}\nother.example {KEY_A}\n"
        );
        fs::write(&target, &original).unwrap();

        replace_host_locked(&target, "example.com", 22, KEY_B).unwrap();

        let content = fs::read_to_string(&target).unwrap();
        assert!(!content.contains("alias.example"));
        assert_eq!(content.matches("example.com").count(), 1);
        assert!(content.contains("other.example"));
        assert!(content.contains(KEY_B));
        assert_eq!(fs::read_to_string(backup_path(&target)).unwrap(), original);
    }

    #[test]
    fn delete_line_preserves_other_lines_and_creates_backup() {
        let dir = TestDir::new("delete");
        let target = dir.known_hosts();
        let original = format!("# comment\none.example {KEY_A}\ntwo.example {KEY_B}\n");
        fs::write(&target, &original).unwrap();

        delete_line_locked(&target, 2).unwrap();

        let content = fs::read_to_string(&target).unwrap();
        assert!(content.starts_with("# comment\n"));
        assert!(!content.contains("one.example"));
        assert!(content.contains("two.example"));
        assert_eq!(fs::read_to_string(backup_path(&target)).unwrap(), original);
    }

    #[test]
    fn append_adds_missing_trailing_newline() {
        let dir = TestDir::new("newline");
        let target = dir.known_hosts();
        fs::write(&target, format!("old.example {KEY_A}")).unwrap();

        append_locked(&target, "new.example", 22, KEY_B).unwrap();

        let content = fs::read_to_string(&target).unwrap();
        let mut lines = content.lines();
        assert_eq!(
            lines.next(),
            Some(
                "old.example ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ"
            )
        );
        assert_eq!(
            lines.next(),
            Some(
                "new.example ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G2sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X"
            )
        );
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn delete_host_removes_plain_and_hashed_entries_for_the_same_host() {
        let dir = TestDir::new("delete-hashed");
        let target = dir.known_hosts();
        // These two hashed patterns both match the literal host `192.168.1.69`.
        let original = format!(
            "192.168.1.69 {KEY_A}\n\
             |1|V1UfXDPxSoiqspHM3/UBJRD9alQ=|mIRSMl6CqlggvD8J8uASWlWte44= {KEY_A}\n\
             |1|PPu/C5Y9GQDPUshREmafAPynksI=|gooCC7jJq5LVsv+v+IwLDiNKHOE= {KEY_B}\n\
             other.example {KEY_A}\n"
        );
        fs::write(&target, &original).unwrap();

        let removed = delete_host_locked(&target, "192.168.1.69", 22).unwrap();

        assert_eq!(removed, vec![1, 2, 3]);
        let content = fs::read_to_string(&target).unwrap();
        assert!(!content.contains("192.168.1.69"));
        assert!(!content.contains("|1|V1UfXDPxSoiqspHM3/UBJRD9alQ="));
        assert!(!content.contains("|1|PPu/C5Y9GQDPUshREmafAPynksI="));
        assert!(content.contains("other.example"));
        assert_eq!(fs::read_to_string(backup_path(&target)).unwrap(), original);
    }

    #[test]
    fn replace_host_removes_hashed_entries_for_the_same_host() {
        let dir = TestDir::new("replace-hashed");
        let target = dir.known_hosts();
        let original = format!(
            "|1|V1UfXDPxSoiqspHM3/UBJRD9alQ=|mIRSMl6CqlggvD8J8uASWlWte44= {KEY_A}\n\
             other.example {KEY_A}\n"
        );
        fs::write(&target, &original).unwrap();

        replace_host_locked(&target, "192.168.1.69", 22, KEY_B).unwrap();

        let content = fs::read_to_string(&target).unwrap();
        assert!(!content.contains("|1|V1UfXDPxSoiqspHM3/UBJRD9alQ="));
        assert!(content.contains("other.example"));
        assert!(content.contains(KEY_B));
        assert_eq!(fs::read_to_string(backup_path(&target)).unwrap(), original);
    }
}
