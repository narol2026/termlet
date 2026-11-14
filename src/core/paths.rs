//! Cross-platform path helpers.
//!
//! Wraps the [`directories`] crate so the rest of the codebase doesn't have to
//! deal with `HOME`/`USERPROFILE`/`XDG_CONFIG_HOME`/`%APPDATA%` differences
//! directly.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::BaseDirs;

use crate::core::ids::HostId;

/// The user's home directory, falling back to `/` if it can't be determined.
pub fn home_dir() -> PathBuf {
    BaseDirs::new().map_or_else(|| PathBuf::from("/"), |d| d.home_dir().to_path_buf())
}

/// Test-only redirection of [`termlet_dir`].
///
/// App-level tests construct a real [`crate::app::App`], and several of its
/// startup and message paths write inside the data directory (the `SQLite` file,
/// the diagnostics log, cached command history). Pointing the tests at a
/// scratch directory keeps a test run from touching — or depending on — the
/// developer's own `~/.termlet/`.
#[cfg(test)]
static TEST_DATA_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Redirect [`termlet_dir`] for the rest of the test process. The first call
/// wins, so every test in the binary shares one scratch directory.
#[cfg(test)]
pub fn set_test_data_dir(dir: PathBuf) {
    let _ = TEST_DATA_DIR.set(dir);
}

#[cfg(test)]
thread_local! {
    // Test-only redirection of `workspace_dir`, per thread.
    static TEST_WORKSPACE_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Point this thread's [`workspace_dir`] at `dir` for the rest of the run.
///
/// The app lists the saved workspaces when it boots and draws one row per file,
/// so a test that saves workspaces would otherwise change what the app-level
/// tests are looking at. The redirection is thread-local on purpose: the
/// app-level tests keep reading the shared directory underneath
/// [`set_test_data_dir`], and a test that wants workspaces of its own gets a
/// directory no other test can see.
#[cfg(test)]
pub fn set_test_workspace_dir(dir: PathBuf) {
    TEST_WORKSPACE_DIR.with(|current| *current.borrow_mut() = Some(dir));
}

/// The directory this thread's [`workspace_dir`] was redirected to, if any.
#[cfg(test)]
fn test_workspace_dir() -> Option<PathBuf> {
    TEST_WORKSPACE_DIR.with(|current| current.borrow().clone())
}

/// The application's config directory. All app data (config, `SQLite`, cache,
/// command history, …) lives under a single user-facing root: `~/.termlet/`.
pub fn config_dir() -> PathBuf {
    termlet_dir()
}

/// The leading tilde form of a path.
///
/// This is the single place that distinguishes the home shorthand `~`/`~/`
/// from a `~/`-prefixed subpath, so callers that expand or rewrite tilde
/// paths don't each re-implement the same `strip_prefix("~/")` / `== "~"`
/// dance. A `~user/...` path is neither form — tilde followed by a username
/// is a distinct (usually literal) construct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TildeForm {
    /// `~` or `~/` — the home directory itself.
    Bare,
    /// `~/<rest>` — a subpath under home, with `rest` excluding the `~/`.
    Subpath(String),
    /// No leading home-relative tilde (absolute, relative, or `~user/...`).
    None,
}

/// Classify the leading tilde form of `input`.
pub fn classify_tilde(input: &str) -> TildeForm {
    if input == "~" || input == "~/" {
        TildeForm::Bare
    } else if let Some(rest) = input.strip_prefix("~/") {
        TildeForm::Subpath(rest.to_string())
    } else if cfg!(windows) && input == "~\\" {
        TildeForm::Bare
    } else if cfg!(windows)
        && let Some(rest) = input.strip_prefix("~\\")
    {
        TildeForm::Subpath(rest.to_string())
    } else {
        TildeForm::None
    }
}

/// Expand a leading `~/` (or a bare `~`) in `input` to the user's home
/// directory. Other inputs are returned unchanged.
pub fn expand_home(input: &str) -> String {
    match classify_tilde(input) {
        TildeForm::Bare => home_dir().to_string_lossy().into_owned(),
        TildeForm::Subpath(rest) => home_dir().join(rest).to_string_lossy().into_owned(),
        TildeForm::None => input.to_string(),
    }
}

/// Like [`expand_home`] but takes/returns a `Path`-friendly value.
#[allow(dead_code, reason = "`Path`-flavoured sibling of `expand_home`")]
pub fn expand_home_path(input: &str) -> PathBuf {
    PathBuf::from(expand_home(input))
}

/// The application's cache directory. After the `~/.termlet/` consolidation
/// this is the same root as [`config_dir`]; data that used to live in the OS
/// cache dir (CWD and command history) now persists
/// under `~/.termlet/` and is not cleared by system cache cleanup tools.
pub fn cache_dir() -> PathBuf {
    termlet_dir()
}

/// Convenience: return `<config_dir>/<file>` and ensure the config directory
/// exists.
#[allow(dead_code, reason = "convenience wrapper around `config_dir`")]
pub fn config_file<P: AsRef<Path>>(file: P) -> std::io::Result<PathBuf> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(file))
}

/// Write `contents` to `path` atomically and owner-only.
///
/// The data is first written to a unique temporary file in the same
/// directory as `path`, restricted to mode `0600` on Unix, then renamed over
/// the destination. This keeps sensitive app-private files (command history,
/// crash reports, CWD snapshots, etc.) from being left truncated by a crash
/// or briefly exposed with default permissions during a direct write.
///
/// On Windows the temporary path receives a protected current-user DACL before
/// any contents are written. The temp-file + rename behaviour is used on every
/// platform.
pub fn write_private_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let directory = parent.unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(directory)?;
    if let Some(parent) = parent {
        if is_link_like(&fs::symlink_metadata(parent)?) {
            return Err(std::io::Error::other(format!(
                "{} is a link; refusing to write private data through it",
                parent.display()
            )));
        }
        set_owner_only_directory_permissions(parent)?;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("termlet_file");

    let mut opened = None;
    for _ in 0..32 {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = directory.join(format!(
            ".{file_name}.{}.{timestamp}.{sequence}.tmp",
            std::process::id()
        ));

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                opened = Some((candidate, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }

    let Some((tmp_path, mut file)) = opened else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "failed to allocate a unique temporary file next to the destination",
        ));
    };

    let write_result = (|| {
        set_owner_only_permissions(&tmp_path)?;
        file.write_all(contents.as_ref())?;
        file.flush()
    })();

    if let Err(e) = write_result {
        drop(file);
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    drop(file);
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn set_owner_only_permissions(path: &Path) -> std::io::Result<()> {
    crate::core::windows_permissions::restrict_to_current_user(path, false)
}

#[cfg(not(any(unix, windows)))]
fn set_owner_only_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_directory_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn set_owner_only_directory_permissions(path: &Path) -> std::io::Result<()> {
    crate::core::windows_permissions::restrict_to_current_user(path, true)
}

#[cfg(not(any(unix, windows)))]
fn set_owner_only_directory_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Restrict a file to the current user (mode `0600` on Unix, DACL on Windows).
/// Used for credential-bearing files: the `SQLite` database and the master-key
/// fallback file.
pub fn restrict_file_permissions(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("failed to inspect permissions for {}: {e}", path.display()))?;
    if is_link_like(&metadata) {
        return Err(format!(
            "refusing to change permissions through link {}",
            path.display()
        ));
    }
    set_owner_only_permissions(path)
        .map_err(|e| format!("failed to restrict permissions for {}: {e}", path.display()))
}

/// Create (if needed) and restrict the user-facing `~/.termlet/` directory to
/// the current user (`0700` on Unix, DACL on Windows).
///
/// The check runs at every startup and is idempotent. Individual private files
/// are also restricted at the point of first or replacement write, so existing
/// data created by older builds is covered as the app touches it.
pub fn ensure_termlet_dir_private() -> Result<(), String> {
    let root = termlet_dir();
    ensure_private_directory(&root).map_err(|e| format!("failed to secure {}: {e}", root.display()))
}

fn ensure_private_directory(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;

    let metadata = fs::symlink_metadata(path)?;
    if is_link_like(&metadata) {
        return Err(std::io::Error::other(format!(
            "{} is a link; refusing to change permissions through it",
            path.display()
        )));
    }

    #[cfg(windows)]
    return secure_existing_tree(path);

    #[cfg(not(windows))]
    set_owner_only_directory_permissions(path)
}

#[cfg(windows)]
fn secure_existing_tree(root: &Path) -> std::io::Result<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if is_link_like(&metadata) {
            return Err(std::io::Error::other(format!(
                "{} is a link; refusing to change permissions through it",
                path.display()
            )));
        }

        if metadata.is_dir() {
            set_owner_only_directory_permissions(&path)?;
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
        } else if metadata.is_file() {
            set_owner_only_permissions(&path)?;
        } else {
            return Err(std::io::Error::other(format!(
                "{} is not a regular file or directory",
                path.display()
            )));
        }
    }
    Ok(())
}

fn is_link_like(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }

    #[cfg(not(windows))]
    false
}

fn sqlite_sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut os_string = path.as_os_str().to_os_string();
    os_string.push(suffix);
    PathBuf::from(os_string)
}

/// Restrict the WAL and SHM sidecars for a `SQLite` database to `0600` when
/// they exist. Sidecars are created lazily by `SQLite`, so callers should run
/// this after enabling WAL mode rather than before opening the database.
pub fn restrict_sqlite_sidecars(db_path: &Path) -> Result<(), String> {
    for suffix in ["-wal", "-shm"] {
        let sidecar = sqlite_sidecar(db_path, suffix);
        match fs::symlink_metadata(&sidecar) {
            Ok(metadata) => {
                if is_link_like(&metadata) {
                    return Err(format!(
                        "refusing to change permissions through link {}",
                        sidecar.display()
                    ));
                }
                if metadata.is_file() {
                    restrict_file_permissions(&sidecar)?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "failed to inspect sqlite sidecar {}: {e}",
                    sidecar.display()
                ));
            }
        }
    }
    Ok(())
}

/// Directory where per-host working directory cache files are stored.
///
/// Returns `<cache_dir>/cwd/`. Used by the SSH bridge to persist the remote
/// CWD reported via OSC 7 escape sequences.
pub fn cwd_cache_dir() -> PathBuf {
    cache_dir().join("cwd")
}

/// Path to the CWD cache file for a given host id.
///
/// The SSH bridge writes the current remote working directory here whenever
/// the shell emits an OSC 7 sequence. The main app reads this file on demand
/// to resolve the remote CWD without parsing the shell prompt.
pub fn cwd_cache_file(host_id: HostId) -> PathBuf {
    cwd_cache_dir().join(host_id.to_string())
}

/// The unified user-facing data directory, `~/.termlet/`.
///
/// All app data — config, `SQLite` (`hosts.sqlite`), skills, logs, and cache
/// (CWD and command history) — lives under this single
/// root, so it is not cleared by OS cache cleanup tools.
pub fn termlet_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = TEST_DATA_DIR.get() {
        return dir.clone();
    }
    home_dir().join(".termlet")
}

/// Directory where per-host command history files are stored.
///
/// Returns `<termlet_dir>/command_history/`, i.e. `~/.termlet/command_history/`.
pub fn command_history_dir() -> PathBuf {
    termlet_dir().join("command_history")
}

/// Directory holding the user-facing activity log.
///
/// Returns `<termlet_dir>/logs/`, i.e. `~/.termlet/logs/`. One file per day
/// (`termlet-YYYY-MM-DD.log`), holding the same events the Logs page shows, so
/// they survive a restart and can be read without the app. This is not
/// `terminal_diagnostics.log`, which records timings, heartbeats and freeze
/// probes for troubleshooting rather than what the user did.
pub fn logs_dir() -> PathBuf {
    termlet_dir().join("logs")
}

/// Path to the command history file for a host scope.
///
/// Local terminals share `~/.termlet/command_history/local.json`; each SSH
/// host keeps `~/.termlet/command_history/<host_id>.json`. Contains a JSON
/// array of command-history entries (see [`crate::core::cache`]).
pub fn command_history_file(host_id: Option<HostId>) -> PathBuf {
    let name = match host_id {
        Some(id) => id.to_string(),
        None => "local".to_string(),
    };
    command_history_dir().join(format!("{name}.json"))
}

/// Directory containing named terminal workspace snapshots.
///
/// Each file is a JSON object of type
/// `crate::workspace::SavedWorkspace`. The directory itself is kept private
/// (`0700` on Unix) just like the rest of the Termlet data directory.
pub fn workspace_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = test_workspace_dir() {
        return dir;
    }
    termlet_dir().join("workspaces")
}

/// Ensure the named workspace directory exists and is private.
pub fn ensure_workspace_dir_private() -> std::io::Result<()> {
    ensure_private_directory(&workspace_dir())
}

/// Directory where per-host command-frequency files are stored.
///
/// Returns `<termlet_dir>/command_frequency/`, i.e.
/// `~/.termlet/command_frequency/`.
pub fn command_frequency_dir() -> PathBuf {
    termlet_dir().join("command_frequency")
}

/// Path to the command-frequency file for a host scope.
///
/// Mirrors [`command_history_file`]: local terminals share
/// `~/.termlet/command_frequency/local.json`; each SSH host keeps
/// `~/.termlet/command_frequency/<host_id>.json`. Contains a JSON object
/// with execution counts and the set of already-prompted commands.
pub fn command_frequency_file(host_id: Option<HostId>) -> PathBuf {
    let name = match host_id {
        Some(id) => id.to_string(),
        None => "local".to_string(),
    };
    command_frequency_dir().join(format!("{name}.json"))
}

#[cfg(test)]
mod tests {
    use super::{
        TildeForm, classify_tilde, ensure_private_directory, expand_home, home_dir,
        restrict_sqlite_sidecars, write_private_atomic,
    };

    #[test]
    fn write_private_atomic_restricts_permissions_and_replaces_contents() {
        let dir = std::env::temp_dir().join(format!("termlet_paths_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("private.json");

        write_private_atomic(&path, "{\"value\":1}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"value\":1}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        };
        #[cfg(windows)]
        assert!(crate::core::windows_permissions::is_current_user_only(
            &path
        ));

        write_private_atomic(&path, "replacement").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ensure_private_directory_creates_and_restricts_root() {
        let dir = std::env::temp_dir().join(format!(
            "termlet_paths_private_dir_existing_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let child = dir.join("nested").join("private.json");
        std::fs::create_dir_all(child.parent().unwrap()).unwrap();
        std::fs::write(&child, "{}").unwrap();

        ensure_private_directory(&dir).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::symlink_metadata(&dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700);
        };
        #[cfg(windows)]
        {
            assert!(crate::core::windows_permissions::is_current_user_only(&dir));
            assert!(crate::core::windows_permissions::is_current_user_only(
                child.parent().unwrap()
            ));
            assert!(crate::core::windows_permissions::is_current_user_only(
                &child
            ));
        };

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn restrict_sqlite_sidecars_sets_existing_sidecars() {
        let dir = std::env::temp_dir().join(format!(
            "termlet_paths_sqlite_sidecars_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("hosts.sqlite");
        let wal = dir.join("hosts.sqlite-wal");
        let shm = dir.join("hosts.sqlite-shm");
        std::fs::write(&wal, "wal").unwrap();
        std::fs::write(&shm, "shm").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o644)).unwrap();
            std::fs::set_permissions(&shm, std::fs::Permissions::from_mode(0o664)).unwrap();
        };

        restrict_sqlite_sidecars(&db).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&wal).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(&shm).unwrap().permissions().mode() & 0o777,
                0o600
            );
        };
        #[cfg(windows)]
        {
            assert!(crate::core::windows_permissions::is_current_user_only(&wal));
            assert!(crate::core::windows_permissions::is_current_user_only(&shm));
        };

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn restricting_a_symlink_is_refused() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!(
            "termlet_paths_symlink_permissions_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target.json");
        let link = dir.join("link.json");
        std::fs::write(&target, "{}").unwrap();
        symlink(&target, &link).unwrap();

        let error = super::restrict_file_permissions(&link).unwrap_err();
        assert!(error.contains("through link"), "unexpected error: {error}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tilde_bare_variants() {
        assert_eq!(classify_tilde("~"), TildeForm::Bare);
        assert_eq!(classify_tilde("~/"), TildeForm::Bare);
    }

    #[test]
    fn tilde_subpath() {
        assert_eq!(classify_tilde("~/work"), TildeForm::Subpath("work".into()));
    }

    #[cfg(windows)]
    #[test]
    fn tilde_accepts_windows_separators() {
        assert_eq!(classify_tilde("~\\"), TildeForm::Bare);
        assert_eq!(
            classify_tilde("~\\.ssh\\id_ed25519"),
            TildeForm::Subpath(".ssh\\id_ed25519".into())
        );
    }

    #[test]
    fn tilde_user_is_not_home_shorthand() {
        assert_eq!(classify_tilde("~someone/foo"), TildeForm::None);
        assert_eq!(classify_tilde("/home/user"), TildeForm::None);
        assert_eq!(classify_tilde("relative/path"), TildeForm::None);
        assert_eq!(classify_tilde(""), TildeForm::None);
    }

    #[test]
    fn expand_home_bare() {
        let home = home_dir().to_string_lossy().into_owned();
        assert_eq!(expand_home("~"), home);
        assert_eq!(expand_home("~/"), home);
    }

    #[test]
    fn expand_home_subpath_and_other() {
        assert_eq!(
            expand_home("~/foo"),
            home_dir().join("foo").to_string_lossy().into_owned()
        );
        assert_eq!(expand_home("/abs/path"), "/abs/path");
        assert_eq!(expand_home("~someone/foo"), "~someone/foo");
    }

    #[cfg(windows)]
    #[test]
    fn expand_home_accepts_windows_separators() {
        assert_eq!(
            expand_home("~\\.ssh\\id_ed25519"),
            home_dir()
                .join(".ssh\\id_ed25519")
                .to_string_lossy()
                .into_owned()
        );
    }
}
