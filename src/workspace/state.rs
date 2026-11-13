//! Saved-workspace manager and pending-operation state.

use crate::core::dropdown::{DropdownState, TransferMenuAction};
use crate::core::list_cursor::ListCursor;
use crate::workspace::export::ExportWorkspace;

use super::{SavedWorkspace, WorkspaceSnapshot};

#[derive(Debug, Clone, Default)]
pub struct WorkspaceManagerState {
    pub list: Vec<SavedWorkspace>,
    pub save_pending: bool,
    pub save_name: String,
    /// Set when the entered name already exists.
    ///
    /// The save dialog switches to an overwrite-or-keep-duplicate prompt until
    /// this is resolved.
    pub save_conflict: Option<String>,
    /// Snapshot captured for the pending save.
    ///
    /// Retained while the user chooses how to handle a duplicate name.
    pub save_snapshot: Option<WorkspaceSnapshot>,
    pub rename_id: Option<String>,
    pub rename_name: String,
    pub delete_id: Option<String>,
    pub hovered_id: Option<String>,
    pub cursor: ListCursor<String>,
    pub transfer_menu: DropdownState<TransferMenuAction>,
    /// Import-confirm dialog (`None` = closed).
    pub import_dialog: Option<WorkspaceImportDialog>,
}

/// State for the workspace import-confirm dialog.
///
/// Export has no in-app dialog: it serializes and opens the native save dialog
/// directly.
#[derive(Debug, Clone)]
pub struct WorkspaceImportDialog {
    pub pending_workspaces: Vec<ExportWorkspace>,
    pub new_count: usize,
    pub dup_count: usize,
}

#[derive(Debug, Clone)]
pub struct PendingWorkspaceApply {
    pub name: String,
    pub snapshot: WorkspaceSnapshot,
}
