//! Crash-recovery for the terminal workspace.
//!
//! Two on-disk artifacts cooperate here, and their meaning is deliberately
//! kept separate:
//!
//! * `last-session.json` — the *what to restore* payload, a serialized
//!   [`WorkspaceSnapshot`] refreshed periodically while sessions exist.
//! * `last-session.run` — a running marker written at startup and removed on a
//!   clean shutdown. Its presence on the next launch means the previous run
//!   did not exit normally.
//!
//! Quick-launch windows (`--quick-ssh` / `--quick-local`) opt out of both
//! artifacts so a secondary window can never clobber the primary window's
//! recovery state.

use std::path::{Path, PathBuf};

use iced::Task;

use crate::app::App;
use crate::core::diagnostics::Level;
use crate::core::message::Message as RootMessage;
use crate::workspace::WorkspaceSnapshot;

const LAST_SESSION_FILE: &str = "last-session.json";
const RUNNING_MARKER_FILE: &str = "last-session.run";

#[derive(Debug, Clone)]
pub enum Message {
    AutoSaveTick,
    RestoreConfirm,
    RestoreDismiss,
}

#[derive(Debug, Default)]
pub struct State {
    /// Whether this process is a primary window that participates in
    /// autosave. Quick-launch windows leave this false.
    pub enabled: bool,
    /// Serialized bytes of the most recently written autosave, used to skip
    /// redundant writes when the layout has not changed.
    pub last_autosave_bytes: Option<Vec<u8>>,
    /// Snapshot recovered from disk after an unclean exit, waiting for the
    /// user to decide (or for the host list to finish loading).
    pub pending: Option<WorkspaceSnapshot>,
    /// Whether the recovery prompt is currently visible.
    pub show_prompt: bool,
}

impl State {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            last_autosave_bytes: None,
            pending: None,
            show_prompt: false,
        }
    }
}

pub fn last_session_path() -> PathBuf {
    super::workspace_dir().join(LAST_SESSION_FILE)
}

pub fn running_marker_path() -> PathBuf {
    super::workspace_dir().join(RUNNING_MARKER_FILE)
}

/// Serialize a snapshot to the same pretty JSON used on disk. Exposed so the
/// autosave path can compare bytes and skip redundant writes.
pub fn serialize_snapshot(snapshot: &WorkspaceSnapshot) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(snapshot)
        .map_err(|error| format!("workspace snapshot serialization failed: {error}"))
}

pub fn write_last_session(bytes: &[u8]) -> Result<(), String> {
    super::ensure_workspace_dir()?;
    let path = last_session_path();
    crate::core::paths::write_private_atomic(&path, bytes).map_err(|error| {
        format!(
            "last session write failed: path={}: {error}",
            path.display()
        )
    })
}

pub fn load_last_session() -> Option<WorkspaceSnapshot> {
    if crate::core::paths::ensure_workspace_dir_private().is_err() {
        return None;
    }
    let path = last_session_path();
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<WorkspaceSnapshot>(&data)
        .map_err(|error| {
            crate::core::diagnostics::log(
                Level::Warn,
                "RECOVERY",
                format!(
                    "last session parse failed: path={}: {error}",
                    path.display()
                ),
            );
        })
        .ok()
}

pub fn has_running_marker() -> bool {
    running_marker_path().exists()
}

pub fn mark_running() -> Result<(), String> {
    super::ensure_workspace_dir()?;
    let path = running_marker_path();
    let marker = format!("{}\n", std::process::id());
    crate::core::paths::write_private_atomic(&path, marker.as_bytes()).map_err(|error| {
        format!(
            "running marker write failed: path={}: {error}",
            path.display()
        )
    })
}

pub fn remove_snapshot() -> Result<(), String> {
    remove_file_if_exists(&last_session_path(), "last session")
}

/// Remove both the snapshot and the running marker. Called once on a clean
/// shutdown so the next launch doesn't offer a recovery prompt.
pub fn mark_clean_shutdown() -> Result<(), String> {
    remove_file_if_exists(&last_session_path(), "last session")?;
    remove_file_if_exists(&running_marker_path(), "running marker")
}

fn remove_file_if_exists(path: &Path, label: &str) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "{label} delete failed: path={}: {error}",
            path.display()
        )),
    }
}

/// Called during [`App::new`] for primary windows: detect whether the previous
/// run left a running marker, load the recoverable snapshot, and mark this run
/// as running.
pub fn start(app: &mut App) {
    if !app.workspace_recovery.enabled {
        return;
    }

    if has_running_marker() {
        app.workspace_recovery.pending = load_last_session();
    }

    if let Err(error) = mark_running() {
        crate::core::diagnostics::log(
            Level::Warn,
            "RECOVERY",
            format!("mark running failed: {error}"),
        );
    }
}

/// Surface the recovery prompt once the host list has finished loading, so
/// SSH sessions in the snapshot can actually be resolved.
pub fn hosts_loaded(app: &mut App) {
    if app.workspace_recovery.pending.is_some() && !app.show_crash_dialog {
        app.workspace_recovery.show_prompt = true;
    }
}

/// Surface the recovery prompt after the crash-report dialog is dismissed, so
/// the two modals never stack.
pub fn crash_dialog_closed(app: &mut App) {
    if app.workspace_recovery.pending.is_some() {
        app.workspace_recovery.show_prompt = true;
    }
}

pub fn update(app: &mut App, message: &Message) -> Task<RootMessage> {
    match message {
        Message::AutoSaveTick => autosave(app),
        Message::RestoreConfirm => restore(app),
        Message::RestoreDismiss => dismiss(app),
    }
}

fn autosave(app: &mut App) -> Task<RootMessage> {
    // Never touch the recovery files while a snapshot is waiting to be
    // restored (whether the prompt is already visible or still deferred until
    // the host list loads); otherwise a no-session tick would erase it out
    // from under the prompt.
    if !app.workspace_recovery.enabled || app.workspace_recovery.pending.is_some() {
        return Task::none();
    }

    let Some(snapshot) = crate::workspace::capture(app) else {
        if let Err(error) = remove_snapshot() {
            log_warn("clear stale last session failed", &error);
        }
        app.workspace_recovery.last_autosave_bytes = None;
        return Task::none();
    };

    let Ok(bytes) = serialize_snapshot(&snapshot) else {
        return Task::none();
    };
    if app.workspace_recovery.last_autosave_bytes.as_deref() == Some(bytes.as_slice()) {
        return Task::none();
    }

    if let Err(error) = write_last_session(&bytes) {
        log_warn("workspace autosave failed", &error);
        return Task::none();
    }
    app.workspace_recovery.last_autosave_bytes = Some(bytes);
    Task::none()
}

fn restore(app: &mut App) -> Task<RootMessage> {
    let Some(snapshot) = app.workspace_recovery.pending.take() else {
        app.workspace_recovery.show_prompt = false;
        return Task::none();
    };
    app.workspace_recovery.show_prompt = false;

    let task = crate::workspace::restore_workspace(app, &snapshot);
    if let Err(error) = remove_snapshot() {
        log_warn("clear last session after restore failed", &error);
    }
    app.workspace_recovery.last_autosave_bytes = None;
    task
}

fn dismiss(app: &mut App) -> Task<RootMessage> {
    app.workspace_recovery.show_prompt = false;
    app.workspace_recovery.pending = None;
    if let Err(error) = remove_snapshot() {
        log_warn("clear last session after dismiss failed", &error);
    }
    app.workspace_recovery.last_autosave_bytes = None;
    Task::none()
}

fn log_warn(context: &str, error: &str) {
    crate::core::diagnostics::log(Level::Warn, "RECOVERY", format!("{context}: {error}"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{
        SnapshotAxis, SnapshotLayoutNode, SnapshotSession, SnapshotSessionKind,
    };

    fn sample_snapshot() -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            version: 1,
            tabs: vec![crate::workspace::SnapshotTab::Session { session: 0 }],
            active_tab: Some(0),
            sessions: vec![SnapshotSession {
                kind: SnapshotSessionKind::Local {
                    start_dir: Some("/tmp".to_string()),
                },
                label_override: None,
            }],
            splits: vec![],
        }
    }

    #[test]
    fn snapshot_serialization_round_trips() {
        let snapshot = sample_snapshot();
        let bytes = serialize_snapshot(&snapshot).unwrap();
        let parsed: WorkspaceSnapshot = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.version, snapshot.version);
        assert_eq!(parsed.tabs.len(), 1);
        assert_eq!(parsed.sessions.len(), 1);
    }

    #[test]
    fn recovery_state_defaults_to_disabled() {
        let state = State::default();
        assert!(!state.enabled);
        assert!(state.pending.is_none());
        assert!(!state.show_prompt);
    }

    #[test]
    fn snapshot_axis_round_trips() {
        // Guards the serde contract relied on by the recovery payload; the
        // layout types are shared with named workspaces.
        let axis = SnapshotAxis::Vertical;
        let json = serde_json::to_string(&axis).unwrap();
        assert_eq!(json, "\"vertical\"");
        let parsed: SnapshotAxis = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SnapshotAxis::Vertical));
    }

    #[test]
    fn layout_node_is_deserializable() {
        let node = SnapshotLayoutNode::Pane { session: 0 };
        let json = serde_json::to_string(&node).unwrap();
        let parsed: SnapshotLayoutNode = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, SnapshotLayoutNode::Pane { session: 0 }));
    }
}
