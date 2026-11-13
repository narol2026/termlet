use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use iced::Task;
use iced::widget::pane_grid;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::app::{ActiveTab, App, SessionId, SplitId, TabId};
use crate::core::data::{HostId, LogCategory};
use crate::core::message::Message;
use crate::terminal::{Split, TabKind};

pub mod export;
pub mod recovery;
pub mod state;

#[doc(inline)]
pub use state::{PendingWorkspaceApply, WorkspaceImportDialog, WorkspaceManagerState};

const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub version: u32,
    pub tabs: Vec<SnapshotTab>,
    pub active_tab: Option<usize>,
    pub sessions: Vec<SnapshotSession>,
    pub splits: Vec<SnapshotSplit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub id: String,
    pub name: String,
    pub updated_at: u64,
    pub snapshot: WorkspaceSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSession {
    pub kind: SnapshotSessionKind,
    pub label_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SnapshotSessionKind {
    Local { start_dir: Option<String> },
    Ssh { host_id: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SnapshotTab {
    Session { session: usize },
    Split { split: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSplit {
    pub root: SnapshotLayoutNode,
    pub focus_session: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SnapshotLayoutNode {
    Pane {
        session: usize,
    },
    Split {
        axis: SnapshotAxis,
        ratio: f32,
        a: Box<SnapshotLayoutNode>,
        b: Box<SnapshotLayoutNode>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAxis {
    Horizontal,
    Vertical,
}

impl SnapshotAxis {
    pub fn to_pane_grid_axis(self) -> pane_grid::Axis {
        match self {
            Self::Horizontal => pane_grid::Axis::Horizontal,
            Self::Vertical => pane_grid::Axis::Vertical,
        }
    }
}

/// Capture the current terminal workspace as a restart-safe snapshot.
///
/// Draft tabs and unsplit pane sessions are intentionally not represented as
/// top-level tabs: split pane sessions live only inside their split layout.
pub fn capture(app: &App) -> Option<WorkspaceSnapshot> {
    if app.terminal.sessions.is_empty() {
        return None;
    }

    let mut session_index: HashMap<SessionId, usize> = HashMap::new();
    let sessions: Vec<SnapshotSession> = app
        .terminal
        .sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            session_index.insert(session.id, index);
            snapshot_session(session)
        })
        .collect();

    let mut split_index: HashMap<SplitId, usize> = HashMap::new();
    let mut splits: Vec<SnapshotSplit> = Vec::new();
    let mut tabs: Vec<SnapshotTab> = Vec::new();
    let mut tab_snapshot_index: HashMap<TabId, usize> = HashMap::new();

    for tab_id in &app.terminal.tabs {
        let Some(tab) = app.terminal.tab_registry.get(tab_id) else {
            continue;
        };

        let snapshot_tab = match tab.kind {
            TabKind::Draft(_) => continue,
            TabKind::Session(session_id) => {
                let session = session_index.get(&session_id).copied()?;
                SnapshotTab::Session { session }
            }
            TabKind::Split(split_id) => {
                let split = if let Some(index) = split_index.get(&split_id).copied() {
                    index
                } else {
                    let split = app.terminal.splits.get(&split_id)?;
                    let root =
                        node_to_snapshot(split.panes.layout(), &split.panes, &session_index)?;
                    let focus_session = split
                        .focus
                        .and_then(|pane| split.panes.get(pane).copied())
                        .and_then(|session_id| session_index.get(&session_id).copied())
                        .unwrap_or(0);
                    let index = splits.len();
                    splits.push(SnapshotSplit {
                        root,
                        focus_session,
                    });
                    split_index.insert(split_id, index);
                    index
                };
                SnapshotTab::Split { split }
            }
        };

        tab_snapshot_index.insert(*tab_id, tabs.len());
        tabs.push(snapshot_tab);
    }

    if tabs.is_empty() {
        return None;
    }

    let active_tab = match app.active_tab {
        ActiveTab::Tab(tab_id) => tab_snapshot_index.get(&tab_id).copied(),
        ActiveTab::Workspace(_) => None,
    };

    Some(WorkspaceSnapshot {
        version: SNAPSHOT_VERSION,
        tabs,
        active_tab,
        sessions,
        splits,
    })
}

fn snapshot_session(session: &crate::terminal::TerminalSession) -> SnapshotSession {
    let kind = match session.host_id {
        Some(host_id) => SnapshotSessionKind::Ssh { host_id: host_id.0 },
        None => SnapshotSessionKind::Local {
            start_dir: if session.working_directory_synced {
                session.working_directory.clone()
            } else {
                None
            },
        },
    };

    SnapshotSession {
        kind,
        label_override: Some(session.label.clone()),
    }
}

fn node_to_snapshot(
    node: &pane_grid::Node,
    panes: &pane_grid::State<SessionId>,
    session_index: &HashMap<SessionId, usize>,
) -> Option<SnapshotLayoutNode> {
    match node {
        pane_grid::Node::Pane(pane) => {
            let session_id = panes.get(*pane)?;
            let session = session_index.get(session_id).copied()?;
            Some(SnapshotLayoutNode::Pane { session })
        }
        pane_grid::Node::Split {
            axis, ratio, a, b, ..
        } => {
            let axis = match axis {
                pane_grid::Axis::Horizontal => SnapshotAxis::Horizontal,
                pane_grid::Axis::Vertical => SnapshotAxis::Vertical,
            };
            Some(SnapshotLayoutNode::Split {
                axis,
                ratio: *ratio,
                a: Box::new(node_to_snapshot(a, panes, session_index)?),
                b: Box::new(node_to_snapshot(b, panes, session_index)?),
            })
        }
    }
}

/// Rebuild a `pane_grid::Configuration` from a saved layout node.
///
/// `sessions` maps snapshot session indices to the newly allocated runtime
/// session ids. A `None` entry means that session could not be restored; the
/// pane is pruned and any split whose children all disappear is collapsed to
/// its single surviving child (or dropped entirely). This keeps one missing
/// pane from discarding its healthy siblings.
pub fn node_to_configuration(
    node: &SnapshotLayoutNode,
    sessions: &[Option<SessionId>],
) -> Option<pane_grid::Configuration<SessionId>> {
    match node {
        SnapshotLayoutNode::Pane { session } => sessions
            .get(*session)
            .copied()
            .flatten()
            .map(pane_grid::Configuration::Pane),
        SnapshotLayoutNode::Split { axis, ratio, a, b } => {
            let a = node_to_configuration(a, sessions);
            let b = node_to_configuration(b, sessions);
            match (a, b) {
                (Some(a), Some(b)) => Some(pane_grid::Configuration::Split {
                    axis: axis.to_pane_grid_axis(),
                    ratio: *ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            }
        }
    }
}

pub fn restore_workspace(app: &mut App, snapshot: &WorkspaceSnapshot) -> Task<Message> {
    if !app.terminal.sessions.is_empty() || !app.terminal.splits.is_empty() {
        app.add_log(
            LogCategory::Terminal,
            "Restore workspace skipped: terminal sessions already exist".to_string(),
        );
        return Task::none();
    }

    let mut session_ids: Vec<Option<SessionId>> = Vec::with_capacity(snapshot.sessions.len());
    let mut tasks: Vec<Task<Message>> = Vec::with_capacity(snapshot.sessions.len());

    for (index, spec) in snapshot.sessions.iter().enumerate() {
        let (session_id, task) = match &spec.kind {
            SnapshotSessionKind::Local { start_dir } => {
                crate::terminal::update::create_local_session_entity_with_dir(
                    app,
                    start_dir.as_deref(),
                )
            }
            SnapshotSessionKind::Ssh { host_id } => {
                let host_id = HostId(*host_id);
                if let Some(host) = app
                    .hosts
                    .list
                    .iter()
                    .find(|host| host.id == host_id)
                    .cloned()
                {
                    match crate::terminal::update::create_ssh_session_entity(
                        app,
                        &host,
                        &crate::terminal::SshSessionSecrets::default(),
                    ) {
                        Ok(created) => created,
                        Err(error) => {
                            tasks.push(crate::terminal::update::ssh_bridge_unavailable_task(
                                app, &error,
                            ));
                            session_ids.push(None);
                            continue;
                        }
                    }
                } else {
                    app.add_log(
                        LogCategory::Terminal,
                        format!(
                            "Restore workspace skipped missing host_id={host_id} for session index {index}"
                        ),
                    );
                    session_ids.push(None);
                    continue;
                }
            }
        };

        let session_id = app
            .terminal
            .sessions
            .iter()
            .any(|session| session.id == session_id)
            .then_some(session_id);

        if let Some(session_id) = session_id {
            crate::terminal::update::apply_restored_session_label(
                app,
                session_id,
                spec.label_override.as_deref(),
            );
        }

        session_ids.push(session_id);
        tasks.push(task);
    }

    let mut restored_tab_ids: Vec<Option<TabId>> = Vec::with_capacity(snapshot.tabs.len());
    for (position, snapshot_tab) in snapshot.tabs.iter().enumerate() {
        match snapshot_tab {
            SnapshotTab::Session { session } => {
                if let Some(session_id) = session_ids.get(*session).copied().flatten() {
                    restored_tab_ids.push(Some(crate::terminal::update::allocate_tab_at(
                        app,
                        TabKind::Session(session_id),
                        position,
                    )));
                } else {
                    restored_tab_ids.push(None);
                }
            }
            SnapshotTab::Split { split } => {
                let Some(split_spec) = snapshot.splits.get(*split) else {
                    restored_tab_ids.push(None);
                    continue;
                };
                let Some(split) = restore_split(app, split_spec, &session_ids) else {
                    app.add_log(
                        LogCategory::Terminal,
                        format!("Restore workspace skipped invalid split index {split}"),
                    );
                    restored_tab_ids.push(None);
                    continue;
                };
                let split_id = split.id;
                app.terminal.splits.insert(split_id, split);
                restored_tab_ids.push(Some(crate::terminal::update::allocate_tab_at(
                    app,
                    TabKind::Split(split_id),
                    position,
                )));
            }
        }
    }

    if let Some(active_index) = snapshot.active_tab
        && let Some(tab_id) = restored_tab_ids.get(active_index).copied().flatten()
    {
        app.activate_tab(tab_id);
    }

    app.seed_tab_width_anims();
    let scroll = crate::update::scroll_to_active_tab(app);
    tasks.push(scroll);

    app.add_log(
        LogCategory::Terminal,
        format!(
            "Restore workspace: {} session(s), {} tab(s), {} split(s)",
            app.terminal.sessions.len(),
            app.terminal.tabs.len(),
            app.terminal.splits.len()
        ),
    );

    Task::batch(tasks)
}

fn restore_split(
    app: &mut App,
    spec: &SnapshotSplit,
    session_ids: &[Option<SessionId>],
) -> Option<Split> {
    let config = node_to_configuration(&spec.root, session_ids)?;
    let panes = pane_grid::State::with_configuration(config);
    let focus = session_ids
        .get(spec.focus_session)
        .copied()
        .flatten()
        .and_then(|session_id| pane_for_session(&panes, session_id))
        .or_else(|| panes.iter().next().map(|(pane, _)| *pane));
    let id = app.allocate_split_id();

    Some(Split {
        id,
        panes,
        focus,
        previous_focus: None,
    })
}

fn pane_for_session(
    panes: &pane_grid::State<SessionId>,
    session_id: SessionId,
) -> Option<pane_grid::Pane> {
    panes
        .iter()
        .find(|(_, pane_session_id)| **pane_session_id == session_id)
        .map(|(pane, _)| *pane)
}

fn workspace_dir() -> PathBuf {
    crate::core::paths::workspace_dir()
}

/// Workspace ids are UUID strings and are also used as the on-disk filename.
/// Reject anything else so a tampered/corrupt `SavedWorkspace.id` can never
/// escape the workspaces directory (path traversal) or target a foreign file.
fn is_valid_workspace_id(id: &str) -> bool {
    Uuid::parse_str(id).is_ok()
}

fn workspace_file(id: &str) -> PathBuf {
    debug_assert!(is_valid_workspace_id(id));
    workspace_dir().join(format!("{id}.json"))
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn ensure_workspace_dir() -> Result<(), String> {
    crate::core::paths::ensure_workspace_dir_private()
        .map_err(|error| format!("workspace directory creation failed: {error}"))
}

fn write_workspace(saved: &SavedWorkspace) -> Result<(), String> {
    let path = workspace_file(&saved.id);
    let json = serde_json::to_vec_pretty(saved).map_err(|error| {
        format!(
            "workspace serialization failed: path={}: {error}",
            path.display()
        )
    })?;
    crate::core::paths::write_private_atomic(&path, json)
        .map_err(|error| format!("workspace write failed: path={}: {error}", path.display()))
}

/// Save a named copy of the current snapshot. The name is trimmed; an empty
/// name is rejected by the caller before this function is invoked.
pub fn save_named_workspace(
    name: &str,
    snapshot: &WorkspaceSnapshot,
) -> Result<SavedWorkspace, String> {
    ensure_workspace_dir()?;
    let saved = SavedWorkspace {
        id: Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        updated_at: now_unix_seconds(),
        snapshot: snapshot.clone(),
    };
    write_workspace(&saved)?;
    Ok(saved)
}

/// Overwrite the most recently updated workspace that already has `name`.
/// If no matching workspace exists this behaves like
/// [`save_named_workspace`], so the overwrite path remains safe to call even
/// after the workspace list changes underneath us.
pub fn overwrite_workspace_by_name(
    name: &str,
    snapshot: &WorkspaceSnapshot,
) -> Result<SavedWorkspace, String> {
    let name = name.trim().to_string();
    ensure_workspace_dir()?;

    let target = list_workspaces()
        .into_iter()
        .filter(|saved| saved.name == name)
        .max_by_key(|saved| saved.updated_at);

    if let Some(mut saved) = target {
        saved.snapshot = snapshot.clone();
        saved.updated_at = now_unix_seconds();
        write_workspace(&saved)?;
        Ok(saved)
    } else {
        save_named_workspace(&name, snapshot)
    }
}

/// Load every saved named workspace, newest first.
pub fn list_workspaces() -> Vec<SavedWorkspace> {
    if let Err(error) = crate::core::paths::ensure_workspace_dir_private() {
        crate::core::diagnostics::log(
            crate::core::diagnostics::Level::Warn,
            "WORKSPACE",
            format!("workspace directory creation failed: {error}"),
        );
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(workspace_dir()) else {
        return Vec::new();
    };

    let mut workspaces: Vec<SavedWorkspace> = entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "json")
                && entry
                    .path()
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(is_valid_workspace_id)
        })
        .filter_map(|entry| {
            let path = entry.path();
            let id = path.file_stem().and_then(|stem| stem.to_str())?.to_string();
            let data = std::fs::read_to_string(&path).ok()?;
            let mut saved = serde_json::from_str::<SavedWorkspace>(&data)
                .map_err(|error| {
                    crate::core::diagnostics::log(
                        crate::core::diagnostics::Level::Warn,
                        "WORKSPACE",
                        format!("workspace parse failed: path={}: {error}", path.display()),
                    );
                })
                .ok()?;
            // The filename is the source of truth; never trust the JSON `id`.
            saved.id = id;
            Some(saved)
        })
        .collect();

    workspaces.sort_by_key(|saved| std::cmp::Reverse(saved.updated_at));
    workspaces
}

pub fn load_workspace(id: &str) -> Option<SavedWorkspace> {
    if !is_valid_workspace_id(id) {
        return None;
    }
    let path = workspace_file(id);
    let data = std::fs::read_to_string(&path).ok()?;
    let mut saved = serde_json::from_str::<SavedWorkspace>(&data)
        .map_err(|error| {
            crate::core::diagnostics::log(
                crate::core::diagnostics::Level::Warn,
                "WORKSPACE",
                format!("workspace parse failed: path={}: {error}", path.display()),
            );
        })
        .ok()?;
    // Canonicalize to the validated filename so callers that later re-write
    // (e.g. rename) never round-trip a tampered JSON `id` back into a path.
    saved.id = id.to_string();
    Some(saved)
}

pub fn rename_workspace(id: &str, name: &str) -> Result<(), String> {
    if !is_valid_workspace_id(id) {
        return Err(format!("invalid workspace id: {id}"));
    }
    let Some(mut saved) = load_workspace(id) else {
        return Err(format!("workspace not found: {id}"));
    };
    saved.name = name.trim().to_string();
    saved.updated_at = now_unix_seconds();
    write_workspace(&saved)
}

pub fn delete_workspace(id: &str) -> Result<(), String> {
    if !is_valid_workspace_id(id) {
        return Err(format!("invalid workspace id: {id}"));
    }
    let path = workspace_file(id);
    std::fs::remove_file(&path)
        .map_err(|error| format!("workspace delete failed: path={}: {error}", path.display()))
}

/// Import a list of exported workspaces, assigning each a fresh id.
///
/// Duplicates are detected by name against the existing on-disk list and
/// against each other within the import (both are skipped). Returns
/// `(new_count, dup_count)`.
pub fn import_workspaces(
    workspaces: &[crate::workspace::export::ExportWorkspace],
) -> Result<(usize, usize), String> {
    ensure_workspace_dir()?;
    let mut existing: HashSet<String> = list_workspaces()
        .into_iter()
        .map(|saved| saved.name)
        .collect();

    let mut new_count = 0usize;
    let mut dup_count = 0usize;

    for ws in workspaces {
        let name = ws.name.trim().to_string();
        if name.is_empty() {
            dup_count += 1;
            continue;
        }
        if !existing.insert(name.clone()) {
            dup_count += 1;
            continue;
        }

        let saved = SavedWorkspace {
            id: Uuid::new_v4().to_string(),
            name,
            updated_at: ws.updated_at,
            snapshot: ws.snapshot.clone(),
        };
        write_workspace(&saved)?;
        new_count += 1;
    }

    Ok((new_count, dup_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_layout_round_trip() {
        let snapshot = WorkspaceSnapshot {
            version: SNAPSHOT_VERSION,
            tabs: vec![SnapshotTab::Split { split: 0 }],
            active_tab: Some(0),
            sessions: vec![
                SnapshotSession {
                    kind: SnapshotSessionKind::Local {
                        start_dir: Some("/tmp".to_string()),
                    },
                    label_override: Some("local".to_string()),
                },
                SnapshotSession {
                    kind: SnapshotSessionKind::Ssh { host_id: 100 },
                    label_override: None,
                },
            ],
            splits: vec![SnapshotSplit {
                root: SnapshotLayoutNode::Split {
                    axis: SnapshotAxis::Vertical,
                    ratio: 0.5,
                    a: Box::new(SnapshotLayoutNode::Pane { session: 0 }),
                    b: Box::new(SnapshotLayoutNode::Pane { session: 1 }),
                },
                focus_session: 1,
            }],
        };

        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: WorkspaceSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, SNAPSHOT_VERSION);
        assert_eq!(parsed.tabs.len(), 1);
        assert_eq!(parsed.sessions.len(), 2);
        assert_eq!(parsed.splits.len(), 1);
    }

    #[test]
    fn node_to_configuration_maps_restored_session_ids() {
        let sessions = vec![Some(SessionId(10)), None, Some(SessionId(20))];
        let node = SnapshotLayoutNode::Split {
            axis: SnapshotAxis::Vertical,
            ratio: 0.4,
            a: Box::new(SnapshotLayoutNode::Pane { session: 0 }),
            b: Box::new(SnapshotLayoutNode::Pane { session: 2 }),
        };

        let configuration = node_to_configuration(&node, &sessions).unwrap();
        let pane_grid::Configuration::Split { axis, ratio, a, b } = configuration else {
            panic!("expected split configuration");
        };
        assert!(matches!(axis, pane_grid::Axis::Vertical));
        assert_eq!(ratio, 0.4);

        let pane_grid::Configuration::Pane(session_id) = *a else {
            panic!("expected left pane");
        };
        assert_eq!(session_id, SessionId(10));

        let pane_grid::Configuration::Pane(session_id) = *b else {
            panic!("expected right pane");
        };
        assert_eq!(session_id, SessionId(20));
    }

    #[test]
    fn node_to_configuration_prunes_missing_pane() {
        let sessions = vec![Some(SessionId(10)), None];
        let node = SnapshotLayoutNode::Split {
            axis: SnapshotAxis::Horizontal,
            ratio: 0.5,
            a: Box::new(SnapshotLayoutNode::Pane { session: 0 }),
            b: Box::new(SnapshotLayoutNode::Pane { session: 1 }),
        };

        let configuration = node_to_configuration(&node, &sessions).unwrap();
        let pane_grid::Configuration::Pane(session_id) = configuration else {
            panic!("expected a single surviving pane");
        };
        assert_eq!(session_id, SessionId(10));
    }

    #[test]
    fn node_to_configuration_none_when_all_panes_missing() {
        let sessions = vec![None, None];
        let node = SnapshotLayoutNode::Split {
            axis: SnapshotAxis::Horizontal,
            ratio: 0.5,
            a: Box::new(SnapshotLayoutNode::Pane { session: 0 }),
            b: Box::new(SnapshotLayoutNode::Pane { session: 1 }),
        };

        assert!(node_to_configuration(&node, &sessions).is_none());
    }

    // ---- the saved-workspace files ----
    //
    // These write real files, so each one takes a directory of its own:
    // `prepare_workspace_dir` points this thread's `workspace_dir()` at a fresh
    // scratch directory. Sharing the app-level data directory instead would
    // leak into the tests that boot the real app, which draws one row per saved
    // workspace.

    use crate::core::test_support::prepare_workspace_dir;
    use crate::workspace::export::ExportWorkspace;

    /// The smallest snapshot that is still worth persisting.
    fn snapshot() -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            version: SNAPSHOT_VERSION,
            tabs: Vec::new(),
            active_tab: None,
            sessions: Vec::new(),
            splits: Vec::new(),
        }
    }

    /// A saved workspace with a distinct id, for the import tests.
    fn exported(name: &str) -> ExportWorkspace {
        ExportWorkspace {
            name: name.to_string(),
            updated_at: 1_700_000_000,
            snapshot: snapshot(),
        }
    }

    #[test]
    fn a_named_workspace_survives_a_trip_through_the_disk() {
        prepare_workspace_dir("round-trip");

        let saved = save_named_workspace("  termlet round trip  ", &snapshot()).expect("saves");

        assert_eq!(saved.name, "termlet round trip", "the name is trimmed");
        assert!(
            is_valid_workspace_id(&saved.id),
            "the id doubles as the filename, so it has to be a uuid: {}",
            saved.id
        );
        assert!(saved.updated_at > 0, "a save is stamped with a time");

        let loaded = load_workspace(&saved.id).expect("loads back");
        assert_eq!(loaded.name, saved.name);
        assert_eq!(loaded.updated_at, saved.updated_at);
        assert_eq!(loaded.id, saved.id);

        assert!(
            list_workspaces()
                .iter()
                .any(|candidate| candidate.id == saved.id),
            "a saved workspace turns up in the list"
        );
    }

    #[test]
    fn overwriting_by_name_reuses_the_newest_match_and_saves_otherwise() {
        prepare_workspace_dir("overwrite");

        let first = save_named_workspace("termlet overwrite target", &snapshot()).expect("saves");

        let overwritten = overwrite_workspace_by_name("termlet overwrite target", &snapshot())
            .expect("overwrites");

        assert_eq!(
            overwritten.id, first.id,
            "an existing name is overwritten in place instead of piling up copies"
        );
        assert!(
            overwritten.updated_at >= first.updated_at,
            "the overwrite re-stamps the file"
        );

        let fresh = overwrite_workspace_by_name("termlet overwrite new", &snapshot())
            .expect("falls back to a save");
        assert_ne!(fresh.id, first.id);
        assert_eq!(fresh.name, "termlet overwrite new");
    }

    #[test]
    fn renaming_updates_the_name_without_moving_the_file() {
        prepare_workspace_dir("rename");

        let saved = save_named_workspace("termlet rename source", &snapshot()).expect("saves");

        rename_workspace(&saved.id, "  termlet renamed  ").expect("renames");

        let loaded = load_workspace(&saved.id).expect("still there under the same id");
        assert_eq!(loaded.name, "termlet renamed");
        assert!(
            list_workspaces()
                .iter()
                .all(|other| other.id != saved.id || other.name == "termlet renamed"),
            "the old name is gone"
        );
    }

    #[test]
    fn deleting_removes_the_file_and_a_second_delete_reports_it() {
        prepare_workspace_dir("delete");

        let saved = save_named_workspace("termlet delete me", &snapshot()).expect("saves");

        delete_workspace(&saved.id).expect("deletes");
        assert!(load_workspace(&saved.id).is_none());
        assert!(
            !list_workspaces().iter().any(|other| other.id == saved.id),
            "the id leaves the list too"
        );

        assert!(
            delete_workspace(&saved.id).is_err(),
            "deleting what is already gone is reported rather than ignored"
        );
    }

    #[test]
    fn an_id_that_is_not_a_uuid_is_refused_everywhere() {
        prepare_workspace_dir("bad-id");

        for id in ["", "not-a-uuid", "../../etc/passwd", "6b20d9a5"] {
            assert!(!is_valid_workspace_id(id), "{id:?} is not an id");
            assert!(load_workspace(id).is_none(), "{id:?} loads nothing");
            assert!(
                rename_workspace(id, "nope").is_err(),
                "{id:?} cannot be renamed"
            );
            assert!(
                delete_workspace(id).is_err(),
                "{id:?} cannot be deleted — a path traversal here would delete a foreign file"
            );
        }
    }

    #[test]
    fn listing_ignores_foreign_files_and_keeps_reading_the_rest() {
        prepare_workspace_dir("listing");
        let dir = workspace_dir();
        std::fs::create_dir_all(&dir).expect("the workspace directory exists");

        let good = save_named_workspace("termlet listing survivor", &snapshot()).expect("saves");

        // A file that is not named after an id, a note that is not JSON, and a
        // JSON file whose contents do not parse: all three are skipped without
        // taking the good workspace down with them.
        std::fs::write(dir.join("notes.json"), b"{}").expect("write notes.json");
        std::fs::write(dir.join(format!("{}.txt", Uuid::new_v4())), b"{}").expect("write .txt");
        std::fs::write(dir.join(format!("{}.json", Uuid::new_v4())), b"{ not json")
            .expect("write a broken file");

        let listed = list_workspaces();
        assert!(
            listed.iter().any(|candidate| candidate.id == good.id),
            "one unreadable file must not hide the readable ones"
        );
    }

    #[test]
    fn the_filename_wins_over_an_id_tampered_into_the_json() {
        prepare_workspace_dir("forged-id");
        let dir = workspace_dir();
        std::fs::create_dir_all(&dir).expect("the workspace directory exists");

        // The file claims to be someone else's workspace id.
        let real = Uuid::new_v4().to_string();
        let forged = Uuid::new_v4().to_string();
        let body = format!(
            r#"{{"id":"{forged}","name":"termlet forged","updated_at":42,"snapshot":{{"version":1,"tabs":[],"active_tab":null,"sessions":[],"splits":[]}}}}"#
        );
        std::fs::write(dir.join(format!("{real}.json")), body).expect("write the tampered file");

        let loaded = load_workspace(&real).expect("the real id still resolves");
        assert_eq!(
            loaded.id, real,
            "the id is taken from the filename, so a later rename cannot write to a forged path"
        );

        assert!(
            list_workspaces().iter().all(|other| other.id != forged),
            "the forged id never enters the list"
        );
    }

    #[test]
    fn importing_skips_blank_names_and_duplicates_on_both_sides() {
        prepare_workspace_dir("import");

        save_named_workspace("termlet import clash", &snapshot()).expect("saves the clash");

        let (new_count, dup_count) = import_workspaces(&[
            exported("termlet import fresh one"),
            exported("termlet import clash"),
            // Twice in the same import: the second copy is a duplicate too.
            exported("termlet import fresh two"),
            exported("termlet import fresh two"),
            exported("   "),
        ])
        .expect("imports");

        assert_eq!(new_count, 2, "only the two genuinely new names land");
        assert_eq!(dup_count, 3);

        let names: Vec<String> = list_workspaces()
            .into_iter()
            .map(|saved| saved.name)
            .collect();
        assert!(names.contains(&"termlet import fresh one".to_string()));
        assert!(names.contains(&"termlet import fresh two".to_string()));
    }

    #[test]
    fn restoring_into_a_terminal_that_already_has_work_is_skipped() {
        let mut app = crate::core::test_support::app();
        let panes = pane_grid::State::new(SessionId(1)).0;
        let id = app.allocate_split_id();
        app.terminal.splits.insert(
            id,
            Split {
                id,
                panes,
                focus: None,
                previous_focus: None,
            },
        );

        let _ = restore_workspace(
            &mut app,
            &WorkspaceSnapshot {
                version: SNAPSHOT_VERSION,
                tabs: vec![SnapshotTab::Session { session: 0 }],
                active_tab: Some(0),
                sessions: vec![SnapshotSession {
                    kind: SnapshotSessionKind::Local { start_dir: None },
                    label_override: None,
                }],
                splits: Vec::new(),
            },
        );

        assert!(
            app.terminal.tabs.is_empty(),
            "a workspace is only restored into an empty terminal, so a stale snapshot cannot \
             duplicate what the user already has open"
        );
    }

    #[test]
    fn restoring_skips_a_session_whose_host_is_no_longer_saved() {
        let mut app = crate::core::test_support::app();

        let _ = restore_workspace(
            &mut app,
            &WorkspaceSnapshot {
                version: SNAPSHOT_VERSION,
                tabs: vec![SnapshotTab::Session { session: 0 }],
                active_tab: Some(0),
                sessions: vec![SnapshotSession {
                    kind: SnapshotSessionKind::Ssh { host_id: 4242 },
                    label_override: None,
                }],
                splits: Vec::new(),
            },
        );

        assert!(
            app.terminal.sessions.is_empty() && app.terminal.tabs.is_empty(),
            "a host that was deleted since the save is reported, not dialled"
        );
    }

    #[test]
    fn restoring_drops_a_tab_that_points_at_a_split_that_is_not_in_the_snapshot() {
        let mut app = crate::core::test_support::app();

        let _ = restore_workspace(
            &mut app,
            &WorkspaceSnapshot {
                version: SNAPSHOT_VERSION,
                tabs: vec![SnapshotTab::Split { split: 9 }],
                active_tab: Some(0),
                sessions: Vec::new(),
                splits: Vec::new(),
            },
        );

        assert!(
            app.terminal.tabs.is_empty(),
            "an out-of-range split index leaves the workspace without that tab"
        );
    }

    #[test]
    fn restore_split_focuses_the_session_the_snapshot_remembered() {
        let mut app = crate::core::test_support::app();
        let session_ids = vec![Some(SessionId(10)), Some(SessionId(20))];
        let spec = SnapshotSplit {
            root: SnapshotLayoutNode::Split {
                axis: SnapshotAxis::Vertical,
                ratio: 0.5,
                a: Box::new(SnapshotLayoutNode::Pane { session: 0 }),
                b: Box::new(SnapshotLayoutNode::Pane { session: 1 }),
            },
            focus_session: 1,
        };

        let split = restore_split(&mut app, &spec, &session_ids).expect("the split restores");

        assert_eq!(split.panes.len(), 2);
        let focused = split.focus.expect("one pane takes focus");
        assert_eq!(
            split.panes.get(focused),
            Some(&SessionId(20)),
            "focus follows the session index the snapshot recorded, not the pane order"
        );

        // A split whose remembered session never came back still opens on one
        // of its surviving panes rather than restoring with no focus at all.
        let spec = SnapshotSplit {
            focus_session: 1,
            ..spec
        };
        let split = restore_split(&mut app, &spec, &[Some(SessionId(10)), None])
            .expect("the surviving pane is still a split");
        let focused = split.focus.expect("the surviving pane takes focus");
        assert_eq!(split.panes.get(focused), Some(&SessionId(10)));
    }

    #[test]
    fn restore_split_gives_up_when_no_pane_survived() {
        let mut app = crate::core::test_support::app();
        let spec = SnapshotSplit {
            root: SnapshotLayoutNode::Pane { session: 0 },
            focus_session: 0,
        };

        assert!(
            restore_split(&mut app, &spec, &[None]).is_none(),
            "a split with no live session is dropped instead of restoring empty"
        );
    }
}
