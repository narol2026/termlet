//! Routes SFTP transfer messages to focused transfer modules.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::App;
use crate::core::message::Message;
use crate::sftp::message::Message as DomainMessage;

mod completion;
mod drag;
mod external;
mod queue;
mod runner;
mod selection;

pub(crate) use external::{
    cursor_moved as external_drag_cursor_moved, drag_point as external_file_drag_point,
};
pub(super) use queue::{
    DownloadStart, initiate_file_download, initiate_folder_download, resolve_duplicate_local_name,
    start_next_queued_transfer,
};
pub(super) use runner::{
    TransferKind, transfer_file, transfer_folder, transfer_folder_atomic,
    transfer_folder_remote_to_remote,
};

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        message @ (DomainMessage::SftpExternalFilesHovered(_)
        | DomainMessage::SftpExternalDropZonesHovered { .. }
        | DomainMessage::SftpExternalFilesHoverLeft
        | DomainMessage::SftpExternalFilesDropped(_)
        | DomainMessage::SftpExternalDropZonesFound { .. }
        | DomainMessage::SftpExternalCopyDone { .. }) => external::update(app, message),

        message @ (DomainMessage::SftpUploadSelected | DomainMessage::SftpDownloadSelected) => {
            selection::update(app, &message)
        }

        message @ (DomainMessage::SftpCancelTransferRequest
        | DomainMessage::SftpCancelTransferCancel
        | DomainMessage::SftpCancelTransfer
        | DomainMessage::SftpCancelQueuedTransfer(_)
        | DomainMessage::SftpClearTransferQueue
        | DomainMessage::SftpDismissTransferResult
        | DomainMessage::SftpDismissTransferFailure(_)
        | DomainMessage::SftpTransferFinished { .. }
        | DomainMessage::SftpTransferEntryRefreshed { .. }) => completion::update(app, message),

        message @ (DomainMessage::SftpFileDropped { .. }
        | DomainMessage::SftpRowHoverEnter { .. }
        | DomainMessage::SftpRowHoverExit { .. }
        | DomainMessage::SftpDropZonesFound { .. }) => drag::update(app, message),

        _ => unreachable!("unexpected sftp message in transfer: {msg:?}"),
    }
}

#[cfg(test)]
mod tests;
