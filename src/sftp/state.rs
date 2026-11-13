//! SFTP browser, transfer, dialog, and workspace state.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::core::context_menu::ContextMenuAction;
use crate::core::data::{
    FileEntry, FileEntryType, Host, HostId, SftpSortColumn, SftpSortDirection,
};
use crate::core::ids::SftpSessionId;
use crate::core::list_cursor::ListCursor;
use crate::ssh_connect::ConnectionAttempt;
use iced::Point;
use iced::widget::text_editor;
use rust_i18n::t;

/// The SFTP file pane that receives keyboard navigation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SftpPane {
    /// The left pane, showing either the local filesystem or a source remote.
    #[default]
    Local,
    /// The right remote pane.
    Remote,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneKind {
    #[default]
    Local,
    Remote,
}

/// Navigation and connection state for one SFTP file pane.
#[derive(Debug, Clone)]
pub struct PaneState {
    pub kind: PaneKind,
    pub connected: bool,
    pub host_id: Option<HostId>,
    pub attempt: ConnectionAttempt,
    pub path: String,
    pub history: Vec<String>,
    pub history_idx: usize,
    pub entries: Vec<FileEntry>,
    pub loading: bool,
    pub error: Option<String>,
    /// Monotonic generation used to ignore stale directory-listing results.
    pub load_id: u64,
    pub filter: String,
    pub show_hidden: bool,
    pub selected: Vec<String>,
    pub last_clicked: Option<String>,
    pub focused: ListCursor<String>,
    pub hovered: Option<String>,
    pub path_edit: Option<String>,
    pub filter_open: bool,
    pub filter_anim: f32,
    pub sort_column: SftpSortColumn,
    pub sort_direction: SftpSortDirection,
}

impl PaneState {
    fn local(path: String) -> Self {
        Self {
            kind: PaneKind::Local,
            connected: false,
            host_id: None,
            attempt: ConnectionAttempt::default(),
            path,
            history: Vec::new(),
            history_idx: 0,
            entries: Vec::new(),
            loading: false,
            error: None,
            load_id: 0,
            filter: String::new(),
            show_hidden: false,
            selected: Vec::new(),
            last_clicked: None,
            focused: ListCursor::new(),
            hovered: None,
            path_edit: None,
            filter_open: false,
            filter_anim: 0.0,
            sort_column: SftpSortColumn::Name,
            sort_direction: SftpSortDirection::Ascending,
        }
        .with_history()
    }

    fn with_history(mut self) -> Self {
        if !self.path.is_empty() {
            self.history.push(self.path.clone());
        }
        self
    }

    fn remote() -> Self {
        let mut pane = Self::local(String::new());
        pane.kind = PaneKind::Remote;
        pane
    }

    pub fn is_remote(&self) -> bool {
        self.kind == PaneKind::Remote
    }
}

impl Default for PaneState {
    fn default() -> Self {
        Self::local(String::new())
    }
}

#[derive(Debug, Clone, Default)]
pub struct HostPickerState {
    pub open: bool,
    pub filter: String,
    pub cursor: ListCursor<HostId>,
}

/// One SFTP side and the local state hidden while it shows a remote.
#[derive(Debug, Clone)]
pub struct SideState {
    pub pane: PaneState,
    pub local: Option<PaneState>,
    pub host_picker: HostPickerState,
    /// Monotonic sequence shared by local and remote listings on this side.
    load_seq: u64,
}

impl SideState {
    fn local(path: String) -> Self {
        Self {
            pane: PaneState::local(path),
            local: None,
            host_picker: HostPickerState::default(),
            load_seq: 0,
        }
    }

    fn remote() -> Self {
        Self {
            pane: PaneState::remote(),
            local: None,
            host_picker: HostPickerState::default(),
            load_seq: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SftpState {
    pub left: SideState,
    pub right: SideState,
    pub active_pane: SftpPane,
    pub keyboard_modifiers: iced::keyboard::Modifiers,
    pub transfer_status: Option<String>,
    pub transfer_progress: Option<Arc<Mutex<SftpTransferProgress>>>,
    pub transfer_queue: Vec<SftpQueuedTransfer>,
    pub transfer_results: SftpTransferResults,
    pub anim_t: f32,
    pub preview: Option<SftpPreview>,
    pub context_menu: Option<SftpContextMenu>,
    pub delete_confirm: Option<SftpDeletePending>,
    /// The single transfer decision currently waiting on the user.
    ///
    /// Transfer events are reduced into this slot so a discard prompt and an
    /// overwrite prompt can never be shown at the same time.
    pub transfer_modal: Option<SftpTransferModal>,
    pub overwrite_apply_all: Option<OverwriteApplyAll>,
    pub rename_pending: Option<SftpRenamePending>,
    pub new_folder_pending: Option<SftpNewFolderPending>,
    pub cursor_pos: Point,
    pub external_drag_paths: Vec<PathBuf>,
    pub external_drop_target: Option<SftpExternalDropTarget>,
}

impl Default for SftpState {
    fn default() -> Self {
        let home = crate::core::paths::home_dir()
            .to_string_lossy()
            .into_owned();
        Self {
            left: SideState::local(home),
            right: SideState::remote(),
            active_pane: SftpPane::Local,
            keyboard_modifiers: iced::keyboard::Modifiers::empty(),
            transfer_status: None,
            transfer_progress: None,
            transfer_queue: Vec::new(),
            transfer_results: SftpTransferResults::default(),
            anim_t: 0.0,
            preview: None,
            context_menu: None,
            delete_confirm: None,
            transfer_modal: None,
            overwrite_apply_all: None,
            rename_pending: None,
            new_folder_pending: None,
            cursor_pos: Point::ORIGIN,
            external_drag_paths: Vec::new(),
            external_drop_target: None,
        }
    }
}

impl SftpState {
    pub fn side(&self, pane: SftpPane) -> &SideState {
        match pane {
            SftpPane::Local => &self.left,
            SftpPane::Remote => &self.right,
        }
    }

    pub fn side_mut(&mut self, pane: SftpPane) -> &mut SideState {
        match pane {
            SftpPane::Local => &mut self.left,
            SftpPane::Remote => &mut self.right,
        }
    }

    pub fn pane(&self, pane: SftpPane) -> &PaneState {
        &self.side(pane).pane
    }

    pub fn pane_mut(&mut self, pane: SftpPane) -> &mut PaneState {
        &mut self.side_mut(pane).pane
    }

    pub fn host_picker(&self, pane: SftpPane) -> &HostPickerState {
        &self.side(pane).host_picker
    }

    pub fn host_picker_mut(&mut self, pane: SftpPane) -> &mut HostPickerState {
        &mut self.side_mut(pane).host_picker
    }

    pub fn open_host_picker(&self) -> Option<SftpPane> {
        if self.left.host_picker.open {
            Some(SftpPane::Local)
        } else if self.right.host_picker.open {
            Some(SftpPane::Remote)
        } else {
            None
        }
    }

    /// Switches a side to remote and preserves the local state on the left.
    pub fn enter_remote(&mut self, pane: SftpPane) -> &mut PaneState {
        let side = self.side_mut(pane);
        if !side.pane.is_remote() {
            let local = std::mem::replace(&mut side.pane, PaneState::remote());
            side.local = Some(local);
        }
        &mut side.pane
    }

    fn begin_listing(&mut self, pane: SftpPane) -> u64 {
        let side = self.side_mut(pane);
        side.load_seq = side.load_seq.wrapping_add(1).max(1);
        side.pane.load_id = side.load_seq;
        side.pane.load_id
    }

    pub fn begin_local_listing(&mut self) -> u64 {
        self.begin_listing(SftpPane::Local)
    }

    pub fn overwrite_pending(&self) -> Option<&SftpOverwritePending> {
        match &self.transfer_modal {
            Some(SftpTransferModal::Overwrite(pending)) => Some(pending),
            _ => None,
        }
    }

    pub fn overwrite_pending_mut(&mut self) -> Option<&mut SftpOverwritePending> {
        match &mut self.transfer_modal {
            Some(SftpTransferModal::Overwrite(pending)) => Some(pending),
            _ => None,
        }
    }

    pub fn clear_transfer_modal(&mut self) {
        self.transfer_modal = None;
    }

    pub fn show_overwrite_modal(&mut self, pending: SftpOverwritePending) {
        self.transfer_results.dismiss_latest();
        self.transfer_modal = Some(SftpTransferModal::Overwrite(pending));
    }

    /// Take a fresh generation for a remote-directory listing on the given pane.
    ///
    /// Results echo the generation back so the handler can drop listings that
    /// were superseded by newer navigation or by disconnecting the pane.
    pub fn begin_remote_listing(&mut self, pane: SftpPane) -> u64 {
        self.begin_listing(pane)
    }

    /// Signal the session's in-flight transfer to stop and drop its bookkeeping.
    ///
    /// The worker observes the progress `cancelled` flag and returns early;
    /// clearing `transfer_progress` makes the result handler ignore the late
    /// `SftpTransferFinished` so it cannot resurrect transfer UI.
    pub fn cancel_active_transfer(&mut self) {
        if let Some(progress) = &self.transfer_progress
            && let Ok(mut progress) = progress.lock()
        {
            progress.cancelled = true;
        }
        self.transfer_progress = None;
        self.transfer_queue.clear();
        self.transfer_results.clear();
        self.transfer_status = None;
        self.transfer_modal = None;
        self.overwrite_apply_all = None;
    }

    /// Whether a transfer worker is currently tracked for this session.
    ///
    /// Used to recognize results from workers abandoned by
    /// [`Self::cancel_active_transfer`].
    pub fn has_live_transfer(&self) -> bool {
        self.transfer_progress.is_some()
    }

    /// Reset the right remote pane to its disconnected state.
    ///
    /// This invalidates any in-flight listing and transfer for the session.
    pub fn disconnect_remote(&mut self) {
        self.begin_remote_listing(SftpPane::Remote);
        self.right.pane = PaneState::remote();
        self.right.host_picker = HostPickerState::default();
        self.cancel_active_transfer();
    }

    /// Reset the left source-remote pane to its disconnected state.
    ///
    /// This invalidates any in-flight listing and transfer for the session.
    pub fn disconnect_left_remote(&mut self) {
        self.begin_remote_listing(SftpPane::Local);
        self.left.pane = self.left.local.take().unwrap_or_default();
        self.left.pane.selected.clear();
        self.left.pane.last_clicked = None;
        self.left.pane.focused.clear();
        self.left.pane.filter.clear();
        self.left.pane.path_edit = None;
        self.left.host_picker = HostPickerState::default();
        self.cancel_active_transfer();
    }
}

#[derive(Debug, Clone)]
pub struct SftpSession {
    pub id: SftpSessionId,
    pub label: String,
    pub state: SftpState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpExternalDropTarget {
    Local,
    Remote,
}

#[derive(Debug, Clone)]
pub struct SftpContextMenu {
    pub pane: SftpPane,
    pub directory: String,
    pub name: String,
    pub file_type: FileEntryType,
    pub position: Point,
    pub cursor: ListCursor<ContextMenuAction>,
}

#[derive(Debug, Clone)]
pub struct SftpSidebarContextMenu {
    pub session_id: SftpSessionId,
    pub tab_index: usize,
    pub can_close: bool,
    pub cursor: ListCursor<ContextMenuAction>,
}

#[derive(Debug, Clone)]
pub struct SftpDeletePending {
    pub pane: SftpPane,
    pub directory: String,
    pub names: Vec<String>,
    pub file_types: Vec<FileEntryType>,
    pub in_progress: bool,
    pub remaining: usize,
    /// `(name, error)` pairs for deletes that failed.
    pub failures: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct SftpOverwritePending {
    pub host: Host,
    pub file_name: String,
    pub remote_path: String,
    pub source_path: String,
    pub local_path: Option<PathBuf>,
    pub source_host: Option<Host>,
    pub total_bytes: u64,
    pub target: SftpTransferTarget,
    pub in_progress: bool,
    pub is_folder: bool,
}

#[derive(Debug, Clone)]
pub enum SftpTransferModal {
    Discard {
        file_name: String,
        /// An overwrite decision produced while the discard prompt was open.
        queued_overwrite: Option<SftpOverwritePending>,
    },
    Overwrite(SftpOverwritePending),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwriteApplyAll {
    Replace,
}

#[derive(Debug, Clone)]
pub struct SftpRenamePending {
    pub pane: SftpPane,
    pub directory: String,
    pub old_name: String,
    pub file_type: FileEntryType,
    pub new_name: String,
    pub in_progress: bool,
}

#[derive(Debug, Clone)]
pub struct SftpNewFolderPending {
    pub pane: SftpPane,
    pub directory: String,
    pub name: String,
    pub in_progress: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpPreviewSource {
    Local,
    LeftRemote,
    RightRemote,
}

#[derive(Debug, Clone)]
pub struct SftpPreview {
    pub source: SftpPreviewSource,
    pub directory: String,
    pub file_name: String,
    pub location_label: String,
    pub state: SftpPreviewState,
}

#[derive(Debug, Clone)]
pub enum SftpPreviewState {
    Loading,
    Ready {
        content: text_editor::Content,
        total_bytes: Option<u64>,
    },
    Failed(String),
}

#[derive(Debug)]
pub struct SftpTransferProgress {
    pub file_name: String,
    pub direction: SftpTransferDirection,
    pub transferred_bytes: u64,
    pub total_bytes: u64,
    pub cancelled: bool,
    pub current_file: String,
    pub files_completed: u64,
    pub total_files: u64,
    pub resumed: bool,
    pub start_time: Option<std::time::Instant>,
    pub speed: f64,
    pub last_speed_update: Option<std::time::Instant>,
}

impl SftpTransferProgress {
    /// Create progress for one file transfer.
    pub(crate) fn file(
        file_name: String,
        direction: SftpTransferDirection,
        total_bytes: u64,
        transferred_bytes: u64,
        resumed: bool,
    ) -> Self {
        Self {
            current_file: file_name.clone(),
            file_name,
            direction,
            transferred_bytes,
            total_bytes,
            cancelled: false,
            files_completed: 0,
            total_files: 1,
            resumed,
            start_time: None,
            speed: 0.0,
            last_speed_update: None,
        }
    }

    /// Create progress for a directory transfer.
    pub(crate) fn folder(
        file_name: String,
        direction: SftpTransferDirection,
        total_bytes: u64,
        total_files: u64,
    ) -> Self {
        Self {
            file_name,
            direction,
            transferred_bytes: 0,
            total_bytes,
            cancelled: false,
            current_file: String::new(),
            files_completed: 0,
            total_files,
            resumed: false,
            start_time: None,
            speed: 0.0,
            last_speed_update: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SftpTransferTarget {
    Local { directory: String, name: String },
    LeftRemote { directory: String, name: String },
    Remote { directory: String, name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpTransferDirection {
    Upload,
    Download,
}

/// Typed failure produced by an SFTP transfer.
#[derive(Debug, Clone)]
pub enum SftpTransferFailure {
    Cancelled,
    Connection {
        message: String,
    },
    FileExists {
        is_dir: bool,
        remote_path: String,
        local_path: Option<String>,
    },
    Failed {
        message: String,
    },
}

impl SftpTransferFailure {
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection {
            message: message.into(),
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Connection { message } | Self::Failed { message } => Some(message),
            Self::Cancelled | Self::FileExists { .. } => None,
        }
    }
}

impl From<String> for SftpTransferFailure {
    fn from(message: String) -> Self {
        Self::Failed { message }
    }
}

/// A transfer waiting in the queue.
#[derive(Debug, Clone)]
pub struct SftpQueuedTransfer {
    pub file_name: String,
    pub direction: SftpTransferDirection,
    pub total_bytes: u64,
    pub total_files: u64,
    pub target: SftpTransferTarget,
    pub kind: SftpQueuedTransferKind,
}

/// The concrete data needed to start a queued transfer.
#[derive(Debug, Clone)]
pub enum SftpQueuedTransferKind {
    UploadFile {
        host: Host,
        local_path: PathBuf,
        remote_path: String,
    },
    UploadFolder {
        host: Host,
        local_path: PathBuf,
        remote_path: String,
    },
    DownloadFile {
        host: Host,
        remote_path: String,
        local_path: PathBuf,
        remote_size: u64,
    },
    DownloadFolder {
        host: Host,
        remote_path: String,
        local_path: PathBuf,
    },
    RemoteToRemoteFile {
        dest_host: Host,
        source_host: Host,
        source_path: String,
        dest_path: String,
    },
    RemoteToRemoteFolder {
        dest_host: Host,
        source_host: Host,
        source_path: String,
        dest_path: String,
    },
}

pub const MAX_TRANSFER_QUEUE: usize = 20;

/// Limits how many failed transfer rows remain visible in one SFTP session.
///
/// Ten rows keep the bottom strip useful without letting historical failures
/// cover most of the file browser. Increasing this value also increases the
/// amount of UI rebuilt on every render.
const MAX_VISIBLE_FAILURES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SftpTransferResultStatus {
    Succeeded,
    Failed,
    Cancelled,
}

/// The last completed transfer shown in the inline progress area.
#[derive(Debug, Clone)]
pub struct SftpTransferResult {
    pub file_name: String,
    pub status: SftpTransferResultStatus,
    pub error: Option<String>,
}

/// Visible transfer outcomes for one SFTP session.
///
/// The ten most recent failures remain until dismissed. Success and
/// cancellation replace the single most recent non-failure result.
#[derive(Debug, Clone, Default)]
pub struct SftpTransferResults {
    latest: Option<SftpTransferResult>,
    failures: Vec<SftpTransferResult>,
}

impl SftpTransferResults {
    /// Returns the most recent successful or cancelled result.
    pub fn latest(&self) -> Option<&SftpTransferResult> {
        self.latest.as_ref()
    }

    /// Returns every retained failed result awaiting dismissal.
    pub fn failures(&self) -> &[SftpTransferResult] {
        &self.failures
    }

    pub(crate) fn record(&mut self, result: SftpTransferResult) {
        if result.status == SftpTransferResultStatus::Failed {
            self.latest = None;
            if self.failures.len() >= MAX_VISIBLE_FAILURES {
                self.failures.remove(0);
            }
            self.failures.push(result);
        } else {
            self.latest = Some(result);
        }
    }

    pub(crate) fn dismiss_latest(&mut self) {
        self.latest = None;
    }

    pub(crate) fn dismiss_failure(&mut self, index: usize) {
        if index < self.failures.len() {
            self.failures.remove(index);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.latest = None;
        self.failures.clear();
    }
}

/// State owned by the SFTP workspace.
///
/// Session-wide and sidebar state are explicit fields. The active tab's
/// file-browser state remains in [`Self::state`].
pub struct SftpDomain {
    pub state: SftpState,
    pub sessions: Vec<SftpSession>,
    pub active_session: SftpSessionId,
    pub next_session_id: usize,
    pub sidebar_show: bool,
    pub sidebar_anim: f32,
    pub sidebar_closing: bool,
    pub sidebar_context_menu: Option<SftpSidebarContextMenu>,
}

impl SftpDomain {
    pub fn new() -> Self {
        Self {
            state: SftpState::default(),
            sessions: vec![SftpSession {
                id: SftpSessionId(0),
                label: t!("sftp.location_local").to_string(),
                state: SftpState::default(),
            }],
            active_session: SftpSessionId(0),
            next_session_id: 1,
            sidebar_show: false,
            sidebar_anim: 0.0,
            sidebar_closing: false,
            sidebar_context_menu: None,
        }
    }
}
