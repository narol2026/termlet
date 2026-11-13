//! Application-level cache for non-essential user preferences.
//!
//! Data stored here persists across restarts but lives in the OS cache
//! directory (e.g. `~/.cache/termlet/` on Linux), so it may be cleared by
//! system cleanup tools or when the application is uninstalled.
//!
//! This is intentionally separate from [`crate::core::storage`], the `SQLite`
//! side of the app.

use serde::{Deserialize, Serialize};

use crate::core::ids::HostId;

/// One command in the per-session MRU command history.
///
/// The history used to be a plain JSON array of command strings; it is now an
/// array of objects so each entry can also carry the time the command started
/// executing. Legacy string-only files are intentionally not migrated — the
/// user opted to let old history be discarded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandHistoryEntry {
    /// The exact command text captured from the shell-integration hook.
    pub command: String,
    /// Unix timestamp (seconds since epoch) when the command started
    /// executing.
    pub executed_at: i64,
}

/// Load the persisted command history for a host scope.
///
/// Local terminals share one file (`~/.termlet/command_history/local.json`);
/// each SSH host keeps its own (`<host_id>.json`). Returns an empty list if
/// the file is missing or unreadable — this is user cache data, so loss is
/// acceptable.
pub fn load_command_history(host_id: Option<HostId>) -> Vec<CommandHistoryEntry> {
    let path = crate::core::paths::command_history_file(host_id);
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Save the command history for a host scope.
///
/// Writes a JSON array of [`CommandHistoryEntry`] objects to
/// `~/.termlet/command_history/`.
/// Failures are silently ignored (best-effort cache).
pub fn save_command_history(host_id: Option<HostId>, commands: &[CommandHistoryEntry]) {
    let path = crate::core::paths::command_history_file(host_id);
    if let Ok(json) = serde_json::to_string(commands) {
        let _ = crate::core::paths::write_private_atomic(&path, json.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::paths;
    use crate::core::test_support::prepare_data_dir;

    /// A host id no other test uses, so that sharing one scratch data directory
    /// across the whole test binary cannot make two tests read each other's
    /// cache file.
    fn host_id(n: u32) -> HostId {
        HostId(90_000 + n)
    }

    #[test]
    fn command_history_round_trips_in_order_with_its_timestamps() {
        prepare_data_dir();

        let id = host_id(4);
        save_command_history(
            Some(id),
            &[
                CommandHistoryEntry {
                    command: "git status".to_string(),
                    executed_at: 1_700_000_000,
                },
                CommandHistoryEntry {
                    command: "cargo test".to_string(),
                    executed_at: 1_700_000_060,
                },
            ],
        );

        let loaded = load_command_history(Some(id));
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].command, "git status");
        assert_eq!(loaded[1].command, "cargo test");
        assert_eq!(loaded[1].executed_at, 1_700_000_060);

        // Writing an empty list is how the history is cleared; it must not be
        // mistaken for "nothing was ever saved".
        save_command_history(Some(id), &[]);
        assert!(load_command_history(Some(id)).is_empty());
    }

    #[test]
    fn each_scope_keeps_its_own_history_file() {
        prepare_data_dir();

        let id = host_id(5);
        assert_ne!(
            paths::command_history_file(Some(id)),
            paths::command_history_file(None),
            "local terminals share one file, not the host's"
        );
        assert_ne!(
            paths::command_history_file(Some(id)),
            paths::command_history_file(Some(host_id(6)))
        );

        save_command_history(
            Some(id),
            &[CommandHistoryEntry {
                command: "ls".to_string(),
                executed_at: 1,
            }],
        );
        assert!(
            load_command_history(Some(host_id(6))).is_empty(),
            "one host's history leaked into another's scope"
        );
    }

    #[test]
    fn a_history_file_in_the_old_or_broken_format_reads_as_empty() {
        prepare_data_dir();

        // The history used to be a plain array of strings. It is deliberately
        // not migrated, so the only requirement is that reading it back is
        // treated as "no history" instead of panicking.
        let id = host_id(7);
        let path = paths::command_history_file(Some(id));
        std::fs::create_dir_all(path.parent().expect("a file under a directory")).unwrap();
        std::fs::write(&path, br#"["ls -la", "pwd"]"#).unwrap();

        assert!(load_command_history(Some(id)).is_empty());
    }
}
