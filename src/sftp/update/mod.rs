use iced::Task;

use crate::app::App;
use crate::core::cursor_focus::CursorTarget;
use crate::core::diagnostics::{Level, perf_guard};
use crate::core::grid_cursor::GridDirection;
use crate::core::message::Message;
use crate::sftp::SftpPane;
use crate::sftp::message::Message as DomainMessage;

mod browse;
mod conflict;
mod file_ops;
mod session;
mod shared;
mod transfer;

pub(crate) use browse::list_remote_pane;
pub(crate) use transfer::{external_drag_cursor_moved, external_file_drag_point};

/// Move an SFTP collection through the shared cursor controller.
pub(crate) fn move_cursor(
    app: &mut App,
    target: CursorTarget,
    direction: GridDirection,
    extend: bool,
) -> Task<Message> {
    let forward = matches!(direction, GridDirection::Down | GridDirection::Right);
    match target {
        CursorTarget::SftpLocalFiles => {
            browse::select_file_focus_in(app, SftpPane::Local, forward, extend)
        }
        CursorTarget::SftpRemoteFiles => {
            browse::select_file_focus_in(app, SftpPane::Remote, forward, extend)
        }
        CursorTarget::SftpLocalHostPicker => {
            browse::move_host_picker_cursor_in(app, SftpPane::Local, forward)
        }
        CursorTarget::SftpRemoteHostPicker => {
            browse::move_host_picker_cursor_in(app, SftpPane::Remote, forward)
        }
        _ => Task::none(),
    }
}

/// Activate the current SFTP collection item.
pub(crate) fn activate_cursor(app: &App, target: CursorTarget) -> Task<Message> {
    match target {
        CursorTarget::SftpLocalFiles => browse::activate_file_focus_in(app, SftpPane::Local),
        CursorTarget::SftpRemoteFiles => browse::activate_file_focus_in(app, SftpPane::Remote),
        CursorTarget::SftpLocalHostPicker => browse::connect_host_picker_in(app, SftpPane::Local),
        CursorTarget::SftpRemoteHostPicker => browse::connect_host_picker_in(app, SftpPane::Remote),
        _ => Task::none(),
    }
}

pub fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    let _guard = perf_guard(
        Level::Debug,
        "UPDATE",
        "sftp::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        DomainMessage::SftpConnectLocalHost(..) => session::update(app, msg),
        DomainMessage::SftpConnectHost(..) => session::update(app, msg),
        DomainMessage::SftpPasswordChanged { .. } => session::update(app, msg),
        DomainMessage::SftpTogglePasswordVisible { .. } => session::update(app, msg),
        DomainMessage::SftpPasswordSubmit { .. } => session::update(app, msg),
        DomainMessage::SftpKeyProbeFinished { .. } => session::update(app, msg),
        DomainMessage::SftpKeyConfirmAccept { .. } => session::update(app, msg),
        DomainMessage::SftpKeyConfirmContinue { .. } => session::update(app, msg),
        DomainMessage::SftpDisconnect => session::update(app, msg),
        DomainMessage::SftpDisconnectLocalRemote => session::update(app, msg),
        DomainMessage::SftpOpenTerminal { .. } => session::update(app, msg),
        DomainMessage::SftpToggleSidebar => session::update(app, msg),
        DomainMessage::SftpNewTab => session::update(app, msg),
        DomainMessage::SftpCloseTab(..) => session::update(app, msg),
        DomainMessage::SftpSwitchTab(..) => session::update(app, msg),
        DomainMessage::SftpDuplicateTab(..) => session::update(app, msg),
        DomainMessage::SftpSidebarContextMenuOpen { .. } => session::update(app, msg),
        DomainMessage::SftpSidebarContextMenuClose => session::update(app, msg),
        DomainMessage::SftpLocalNavigate(..) => browse::update(app, msg),
        DomainMessage::SftpLocalBack => browse::update(app, msg),
        DomainMessage::SftpLocalForward => browse::update(app, msg),
        DomainMessage::SftpLocalEntriesLoaded(..) => browse::update(app, msg),
        DomainMessage::SftpLocalFilter(..) => browse::update(app, msg),
        DomainMessage::SftpRemoteFilter(..) => browse::update(app, msg),
        DomainMessage::SftpToggleFilter(..) => browse::update(app, msg),
        DomainMessage::SftpSortBy { .. } => browse::update(app, msg),
        DomainMessage::SftpToggleHidden(..) => browse::update(app, msg),
        DomainMessage::SftpRefreshLocal => browse::update(app, msg),
        DomainMessage::SftpRefreshRemote => browse::update(app, msg),
        DomainMessage::SftpShowLocalHostPicker => browse::update(app, msg),
        DomainMessage::SftpHideLocalHostPicker => browse::update(app, msg),
        DomainMessage::SftpShowHostPicker => browse::update(app, msg),
        DomainMessage::SftpHideHostPicker => browse::update(app, msg),
        DomainMessage::SftpHostPickerFilter(..) => browse::update(app, msg),
        DomainMessage::SftpHostPickerConnect => browse::update(app, msg),
        DomainMessage::SftpLocalEntryClicked(..) => browse::update(app, msg),
        DomainMessage::SftpLocalEntryDoubleClicked(..) => browse::update(app, msg),
        DomainMessage::SftpRemoteEntryClicked(..) => browse::update(app, msg),
        DomainMessage::SftpRemoteEntryDoubleClicked(..) => browse::update(app, msg),
        DomainMessage::SftpRemoteNavigate(..) => browse::update(app, msg),
        DomainMessage::SftpRemoteBack => browse::update(app, msg),
        DomainMessage::SftpRemoteForward => browse::update(app, msg),
        DomainMessage::SftpRemoteListed { .. } => browse::update(app, msg),
        DomainMessage::SftpPathEditStart(..) => browse::update(app, msg),
        DomainMessage::SftpPathEditCancel(..) => browse::update(app, msg),
        DomainMessage::SftpPathEditChange(..) => browse::update(app, msg),
        DomainMessage::SftpPathEditSubmit(..) => browse::update(app, msg),
        DomainMessage::SftpPathEditBlurCheck => browse::update(app, msg),
        DomainMessage::SftpViewCursorMoved(..) => browse::update(app, msg),
        DomainMessage::SftpExternalFilesHovered(..) => transfer::update(app, msg),
        DomainMessage::SftpExternalDropZonesHovered { .. } => transfer::update(app, msg),
        DomainMessage::SftpExternalFilesHoverLeft => transfer::update(app, msg),
        DomainMessage::SftpExternalFilesDropped(..) => transfer::update(app, msg),
        DomainMessage::SftpExternalDropZonesFound { .. } => transfer::update(app, msg),
        DomainMessage::SftpExternalCopyDone { .. } => transfer::update(app, msg),
        DomainMessage::SftpUploadSelected => transfer::update(app, msg),
        DomainMessage::SftpDownloadSelected => transfer::update(app, msg),
        DomainMessage::SftpCancelTransferRequest => transfer::update(app, msg),
        DomainMessage::SftpCancelTransferCancel => transfer::update(app, msg),
        DomainMessage::SftpCancelTransfer => transfer::update(app, msg),
        DomainMessage::SftpCancelQueuedTransfer(..) => transfer::update(app, msg),
        DomainMessage::SftpClearTransferQueue => transfer::update(app, msg),
        DomainMessage::SftpDismissTransferResult => transfer::update(app, msg),
        DomainMessage::SftpDismissTransferFailure(..) => transfer::update(app, msg),
        DomainMessage::SftpTransferFinished { .. } => transfer::update(app, msg),
        DomainMessage::SftpTransferEntryRefreshed { .. } => transfer::update(app, msg),
        DomainMessage::SftpFileDropped { .. } => transfer::update(app, msg),
        DomainMessage::SftpRowHoverEnter { .. } => transfer::update(app, msg),
        DomainMessage::SftpRowHoverExit { .. } => transfer::update(app, msg),
        DomainMessage::SftpDropZonesFound { .. } => transfer::update(app, msg),
        DomainMessage::SftpPreviewOpen { .. } => file_ops::update(app, msg),
        DomainMessage::SftpPreviewLoaded { .. } => file_ops::update(app, msg),
        DomainMessage::SftpPreviewAction(..) => file_ops::update(app, msg),
        DomainMessage::SftpPreviewClose => file_ops::update(app, msg),
        DomainMessage::SftpContextMenuOpen { .. } => file_ops::update(app, msg),
        DomainMessage::SftpContextMenuClose => file_ops::update(app, msg),
        DomainMessage::SftpDeleteRequest => file_ops::update(app, msg),
        DomainMessage::SftpDeleteCancel => file_ops::update(app, msg),
        DomainMessage::SftpDeleteConfirm => file_ops::update(app, msg),
        DomainMessage::SftpDeleteFinished { .. } => file_ops::update(app, msg),
        DomainMessage::SftpRenameRequest => file_ops::update(app, msg),
        DomainMessage::SftpRenameCancel => file_ops::update(app, msg),
        DomainMessage::SftpRenameNewNameChanged(..) => file_ops::update(app, msg),
        DomainMessage::SftpRenameConfirm => file_ops::update(app, msg),
        DomainMessage::SftpRenameFinished { .. } => file_ops::update(app, msg),
        DomainMessage::SftpNewFolderRequest { .. } => file_ops::update(app, msg),
        DomainMessage::SftpNewFolderCancel => file_ops::update(app, msg),
        DomainMessage::SftpNewFolderNameChanged(..) => file_ops::update(app, msg),
        DomainMessage::SftpNewFolderConfirm => file_ops::update(app, msg),
        DomainMessage::SftpNewFolderFinished { .. } => file_ops::update(app, msg),
        DomainMessage::SftpOverwriteCancel => conflict::update(app, &msg),
        DomainMessage::SftpOverwriteReplace => conflict::update(app, &msg),
        DomainMessage::SftpOverwriteReplaceAll => conflict::update(app, &msg),
        DomainMessage::SftpOverwriteDuplicate => conflict::update(app, &msg),
        DomainMessage::SftpOverwriteMerge => conflict::update(app, &msg),
    }
}
