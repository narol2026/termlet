use iced::widget::Id;
use iced::{Point, Rectangle};
use secrecy::SecretString;

use crate::core::data::SftpSortColumn;
use crate::core::data::{HostId, ServerKeyInfo};
use crate::core::ids::SftpSessionId;
use crate::sftp::SftpPane;

#[derive(Debug, Clone)]
pub struct PreviewPayload {
    pub text: String,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
// Variants carry the module prefix (iced message-enum convention) so they stay
// unambiguous once composed into the app-wide `Message`.
#[allow(
    clippy::enum_variant_names,
    reason = "variant names follow the message kinds in the protocol"
)]
pub enum Message {
    SftpLocalNavigate(String),
    SftpLocalBack,
    SftpLocalForward,
    /// Local directory listing completed off the UI thread. The `u64` is the
    /// generation counter used to discard stale results after rapid navigation.
    SftpLocalEntriesLoaded(
        SftpSessionId,
        u64,
        Result<Vec<crate::core::data::FileEntry>, String>,
    ),
    SftpLocalFilter(String),
    SftpRemoteFilter(String),
    SftpToggleHidden(SftpPane),
    SftpRefreshLocal,
    SftpRefreshRemote,
    SftpShowLocalHostPicker,
    SftpHideLocalHostPicker,
    SftpShowHostPicker,
    SftpHideHostPicker,
    SftpHostPickerFilter(String),
    SftpHostPickerConnect,
    SftpConnectLocalHost(HostId),
    SftpConnectHost(HostId),
    SftpPasswordChanged {
        pane: SftpPane,
        password: SecretString,
    },
    SftpTogglePasswordVisible {
        pane: SftpPane,
    },
    SftpPasswordSubmit {
        pane: SftpPane,
    },
    SftpKeyProbeFinished {
        pane: SftpPane,
        session_id: SftpSessionId,
        result: Result<ServerKeyInfo, String>,
    },
    SftpKeyConfirmAccept {
        pane: SftpPane,
    },
    SftpKeyConfirmContinue {
        pane: SftpPane,
    },
    SftpDisconnectLocalRemote,
    SftpDisconnect,
    /// Open a terminal tab for the currently connected remote host.
    SftpOpenTerminal {
        pane: SftpPane,
    },
    SftpLocalEntryClicked(String),
    SftpLocalEntryDoubleClicked(String),
    SftpRemoteEntryClicked(String),
    SftpRemoteEntryDoubleClicked(String),
    SftpRemoteNavigate(String),
    SftpRemoteBack,
    SftpRemoteForward,
    SftpRemoteListed {
        pane: SftpPane,
        path: String,
        result: Result<Vec<crate::core::data::FileEntry>, String>,
        session_id: SftpSessionId,
        /// Generation counter used to discard stale results after navigation
        /// or after the remote session has been disconnected.
        generation: u64,
        host_id: Option<HostId>,
        password: Option<SecretString>,
        save_password: bool,
    },
    SftpUploadSelected,
    SftpDownloadSelected,
    /// Ask before discarding the in-flight transfer.
    SftpCancelTransferRequest,
    SftpCancelTransfer,
    /// Dismiss the discard-transfer confirmation without cancelling.
    SftpCancelTransferCancel,
    SftpCancelQueuedTransfer(usize),
    SftpClearTransferQueue,
    SftpDismissTransferResult,
    SftpDismissTransferFailure(usize),
    SftpTransferFinished {
        target: crate::sftp::SftpTransferTarget,
        result: Result<String, crate::sftp::SftpTransferFailure>,
        session_id: SftpSessionId,
    },
    SftpTransferEntryRefreshed {
        target: crate::sftp::SftpTransferTarget,
        result: Result<Option<crate::core::data::FileEntry>, String>,
    },
    SftpPathEditStart(SftpPane),
    SftpPathEditChange(SftpPane, String),
    SftpPathEditCancel(SftpPane),
    SftpPathEditSubmit(SftpPane),
    /// Dispatched after every mouse press to check whether the path
    /// text input has lost focus — if so, cancels edit mode.
    SftpPathEditBlurCheck,
    SftpToggleFilter(SftpPane),
    SftpSortBy {
        pane: SftpPane,
        column: SftpSortColumn,
    },
    SftpFileDropped {
        pane: SftpPane,
        name: String,
        is_file: bool,
        selected_names: Vec<String>,
    },
    SftpDropZonesFound {
        pane: SftpPane,
        name: String,
        is_file: bool,
        selected_names: Vec<String>,
        zones: Vec<(Id, Rectangle)>,
    },
    SftpRowHoverEnter {
        pane: SftpPane,
        name: String,
    },
    SftpRowHoverExit {
        pane: SftpPane,
        name: String,
    },
    SftpPreviewOpen {
        source: crate::sftp::SftpPreviewSource,
        directory: String,
        file_name: String,
        location_label: String,
    },
    SftpPreviewLoaded {
        source: crate::sftp::SftpPreviewSource,
        directory: String,
        file_name: String,
        result: Result<PreviewPayload, String>,
        session_id: SftpSessionId,
    },
    /// Relays `text_editor` interactions (cursor/selection/scroll) for the
    /// read-only preview. Edit actions are intentionally dropped in the update
    /// handler so the preview cannot modify the file.
    SftpPreviewAction(iced::widget::text_editor::Action),
    SftpPreviewClose,
    SftpContextMenuOpen {
        pane: SftpPane,
        name: String,
    },
    SftpViewCursorMoved(Point),
    SftpContextMenuClose,
    SftpExternalFilesDropped(Vec<std::path::PathBuf>),
    SftpExternalFilesHovered(Vec<std::path::PathBuf>),
    SftpExternalDropZonesHovered {
        paths: Vec<std::path::PathBuf>,
        zones: Vec<(Id, Rectangle)>,
    },
    SftpExternalFilesHoverLeft,
    SftpExternalCopyDone {
        message: String,
        session_id: SftpSessionId,
    },
    SftpExternalDropZonesFound {
        paths: Vec<std::path::PathBuf>,
        point: Point,
        zones: Vec<(Id, Rectangle)>,
    },
    SftpDeleteRequest,
    SftpDeleteCancel,
    SftpDeleteConfirm,
    SftpDeleteFinished {
        pane: SftpPane,
        directory: String,
        name: String,
        result: Result<String, String>,
        session_id: SftpSessionId,
    },
    SftpOverwriteCancel,
    SftpOverwriteReplace,
    SftpOverwriteReplaceAll,
    SftpOverwriteDuplicate,
    SftpOverwriteMerge,
    SftpRenameRequest,
    SftpRenameCancel,
    SftpRenameNewNameChanged(String),
    SftpRenameConfirm,
    SftpRenameFinished {
        pane: SftpPane,
        directory: String,
        result: Result<String, String>,
        session_id: SftpSessionId,
    },
    SftpNewFolderRequest {
        pane: SftpPane,
        directory: String,
    },
    SftpNewFolderCancel,
    SftpNewFolderNameChanged(String),
    SftpNewFolderConfirm,
    SftpNewFolderFinished {
        pane: SftpPane,
        directory: String,
        result: Result<String, String>,
        session_id: SftpSessionId,
    },
    SftpNewTab,
    SftpCloseTab(SftpSessionId),
    SftpSwitchTab(SftpSessionId),
    SftpToggleSidebar,
    SftpDuplicateTab(SftpSessionId),
    SftpSidebarContextMenuOpen {
        session_id: SftpSessionId,
    },
    SftpSidebarContextMenuClose,
}
