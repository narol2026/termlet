//! Builds and starts SFTP transfer queue entries.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use iced::Task;

use super::runner::{
    TransferKind, transfer_file, transfer_folder, transfer_folder_download,
    transfer_folder_remote_to_remote,
};
use crate::app::App;
use crate::core::data::Host;
use crate::core::message::Message;
use crate::sftp::message::Message as DomainMessage;
use crate::sftp::{
    MAX_TRANSFER_QUEUE, SftpQueuedTransfer, SftpQueuedTransferKind, SftpTransferDirection,
    SftpTransferFailure, SftpTransferProgress, SftpTransferTarget,
};
use rust_i18n::t;

type TransferFuture =
    Pin<Box<dyn Future<Output = Result<String, SftpTransferFailure>> + Send + 'static>>;

pub(crate) struct RemoteSpec {
    pub dest_host: Host,
    pub source_host: Host,
    pub source_path: String,
    pub dest_path: String,
    pub total_bytes: u64,
}

pub(crate) struct DownloadStart {
    pub host: Host,
    pub remote_path: String,
    pub local_path: PathBuf,
    pub name: String,
    pub remote_size: u64,
    pub target: SftpTransferTarget,
    pub resume_offset: Option<u64>,
    pub resumed: bool,
    pub initial_transferred: u64,
}

pub(super) fn upload_file(
    file_name: String,
    target: SftpTransferTarget,
    host: Host,
    local_path: PathBuf,
    remote_path: String,
    total_bytes: u64,
) -> SftpQueuedTransfer {
    SftpQueuedTransfer {
        file_name,
        direction: SftpTransferDirection::Upload,
        total_bytes,
        total_files: 1,
        target,
        kind: SftpQueuedTransferKind::UploadFile {
            host,
            local_path,
            remote_path,
        },
    }
}

pub(super) fn upload_folder(
    file_name: String,
    target: SftpTransferTarget,
    host: Host,
    local_path: PathBuf,
    remote_path: String,
    total_bytes: u64,
    total_files: u64,
) -> SftpQueuedTransfer {
    SftpQueuedTransfer {
        file_name,
        direction: SftpTransferDirection::Upload,
        total_bytes,
        total_files,
        target,
        kind: SftpQueuedTransferKind::UploadFolder {
            host,
            local_path,
            remote_path,
        },
    }
}

pub(super) fn download_file(
    file_name: String,
    target: SftpTransferTarget,
    host: Host,
    remote_path: String,
    local_path: PathBuf,
    remote_size: u64,
) -> SftpQueuedTransfer {
    SftpQueuedTransfer {
        file_name,
        direction: SftpTransferDirection::Download,
        total_bytes: remote_size,
        total_files: 1,
        target,
        kind: SftpQueuedTransferKind::DownloadFile {
            host,
            remote_path,
            local_path,
            remote_size,
        },
    }
}

pub(super) fn download_folder(
    file_name: String,
    target: SftpTransferTarget,
    host: Host,
    remote_path: String,
    local_path: PathBuf,
    total_bytes: u64,
) -> SftpQueuedTransfer {
    SftpQueuedTransfer {
        file_name,
        direction: SftpTransferDirection::Download,
        total_bytes,
        total_files: 0,
        target,
        kind: SftpQueuedTransferKind::DownloadFolder {
            host,
            remote_path,
            local_path,
        },
    }
}

pub(super) fn remote_file(
    file_name: String,
    direction: SftpTransferDirection,
    target: SftpTransferTarget,
    remote: RemoteSpec,
) -> SftpQueuedTransfer {
    SftpQueuedTransfer {
        file_name,
        direction,
        total_bytes: remote.total_bytes,
        total_files: 1,
        target,
        kind: SftpQueuedTransferKind::RemoteToRemoteFile {
            dest_host: remote.dest_host,
            source_host: remote.source_host,
            source_path: remote.source_path,
            dest_path: remote.dest_path,
        },
    }
}

pub(super) fn remote_folder(
    file_name: String,
    direction: SftpTransferDirection,
    target: SftpTransferTarget,
    remote: RemoteSpec,
) -> SftpQueuedTransfer {
    SftpQueuedTransfer {
        file_name,
        direction,
        total_bytes: 0,
        total_files: 0,
        target,
        kind: SftpQueuedTransferKind::RemoteToRemoteFolder {
            dest_host: remote.dest_host,
            source_host: remote.source_host,
            source_path: remote.source_path,
            dest_path: remote.dest_path,
        },
    }
}

pub(crate) fn initiate_file_download(app: &mut App, start: DownloadStart) -> Task<Message> {
    let session_id = app.sftp.active_session;
    let progress = Arc::new(Mutex::new(SftpTransferProgress::file(
        start.name.clone(),
        SftpTransferDirection::Download,
        start.remote_size,
        start.initial_transferred,
        start.resumed,
    )));
    app.sftp.state.transfer_progress = Some(Arc::clone(&progress));
    perform_transfer(
        app,
        session_id,
        start.target,
        t!("sftp.downloading", name = &start.name).to_string(),
        Arc::clone(&progress),
        TransferKind::Download {
            remote_path: start.remote_path,
            local_path: start.local_path,
            resume_offset: start.resume_offset,
        },
        start.host,
    )
}

pub(crate) fn initiate_folder_download(
    app: &mut App,
    host: Host,
    remote_path: String,
    local_path: PathBuf,
    name: &str,
    target: SftpTransferTarget,
) -> Task<Message> {
    let session_id = app.sftp.active_session;
    let progress = Arc::new(Mutex::new(SftpTransferProgress::folder(
        name.to_string(),
        SftpTransferDirection::Download,
        0,
        0,
    )));
    app.sftp.state.transfer_progress = Some(Arc::clone(&progress));
    app.sftp.state.transfer_status = Some(t!("sftp.downloading_folder", name = name).to_string());
    Task::perform(
        transfer_folder_download(host, remote_path, local_path, progress),
        move |result| {
            Message::Sftp(DomainMessage::SftpTransferFinished {
                target: target.clone(),
                result,
                session_id,
            })
        },
    )
}

fn perform_transfer(
    app: &mut App,
    session_id: crate::core::ids::SftpSessionId,
    target: SftpTransferTarget,
    status: String,
    progress: Arc<Mutex<SftpTransferProgress>>,
    kind: TransferKind,
    host: Host,
) -> Task<Message> {
    app.sftp.state.transfer_status = Some(status);
    Task::perform(transfer_file(host, kind, progress), move |result| {
        Message::Sftp(DomainMessage::SftpTransferFinished {
            target: target.clone(),
            result,
            session_id,
        })
    })
}

pub(super) fn start_item(app: &mut App, item: SftpQueuedTransfer) -> Task<Message> {
    let target = item.target.clone();
    let session_id = app.sftp.active_session;
    let progress = Arc::new(Mutex::new(progress_for(&item)));
    app.sftp.state.transfer_status = Some(status_for(&item));
    app.sftp.state.transfer_progress = Some(Arc::clone(&progress));
    let future = execute(item, Arc::clone(&progress));
    Task::perform(future, move |result| {
        Message::Sftp(DomainMessage::SftpTransferFinished {
            target: target.clone(),
            result,
            session_id,
        })
    })
}

fn progress_for(item: &SftpQueuedTransfer) -> SftpTransferProgress {
    let file_name = item.file_name.clone();
    match &item.kind {
        SftpQueuedTransferKind::UploadFile { .. } => {
            SftpTransferProgress::file(file_name, item.direction, item.total_bytes, 0, false)
        }
        SftpQueuedTransferKind::UploadFolder { .. } => SftpTransferProgress::folder(
            file_name,
            item.direction,
            if item.total_files > 20 && item.total_bytes < 60 * 1024 * 1024 {
                0
            } else {
                item.total_bytes
            },
            item.total_files,
        ),
        SftpQueuedTransferKind::DownloadFile {
            local_path,
            remote_size,
            ..
        } => {
            let (transferred, resumed) = match std::fs::metadata(local_path) {
                Ok(metadata) if metadata.len() > 0 && metadata.len() < *remote_size => {
                    (metadata.len(), true)
                }
                _ => (0, false),
            };
            SftpTransferProgress::file(
                file_name,
                item.direction,
                *remote_size,
                transferred,
                resumed,
            )
        }
        SftpQueuedTransferKind::DownloadFolder { .. } => {
            SftpTransferProgress::folder(file_name, item.direction, 0, 0)
        }
        SftpQueuedTransferKind::RemoteToRemoteFile { .. } => {
            SftpTransferProgress::file(file_name, item.direction, item.total_bytes, 0, false)
        }
        SftpQueuedTransferKind::RemoteToRemoteFolder { .. } => {
            SftpTransferProgress::folder(file_name, item.direction, 0, 0)
        }
    }
}

fn status_for(item: &SftpQueuedTransfer) -> String {
    match item.kind {
        SftpQueuedTransferKind::UploadFile { .. } => {
            t!("sftp.uploading", name = &item.file_name).to_string()
        }
        SftpQueuedTransferKind::UploadFolder { .. } => {
            t!("sftp.uploading_folder", name = &item.file_name).to_string()
        }
        SftpQueuedTransferKind::DownloadFile { .. } => {
            t!("sftp.downloading", name = &item.file_name).to_string()
        }
        SftpQueuedTransferKind::DownloadFolder { .. } => {
            t!("sftp.downloading_folder", name = &item.file_name).to_string()
        }
        SftpQueuedTransferKind::RemoteToRemoteFile { .. } => {
            t!("sftp.copying", name = &item.file_name).to_string()
        }
        SftpQueuedTransferKind::RemoteToRemoteFolder { .. } => {
            t!("sftp.copying_folder", name = &item.file_name).to_string()
        }
    }
}

fn execute(item: SftpQueuedTransfer, progress: Arc<Mutex<SftpTransferProgress>>) -> TransferFuture {
    match item.kind {
        SftpQueuedTransferKind::UploadFile {
            host,
            local_path,
            remote_path,
        } => Box::pin(transfer_file(
            host,
            TransferKind::Upload {
                local_path,
                remote_path,
                overwrite: false,
                resume_offset: None,
            },
            progress,
        )),
        SftpQueuedTransferKind::UploadFolder {
            host,
            local_path,
            remote_path,
        } => Box::pin(transfer_folder(
            host,
            local_path,
            remote_path,
            progress,
            false,
        )),
        SftpQueuedTransferKind::DownloadFile {
            host,
            remote_path,
            local_path,
            remote_size,
        } => {
            let resume_offset = match std::fs::metadata(&local_path) {
                Ok(metadata) if metadata.len() > 0 && metadata.len() < remote_size => {
                    Some(metadata.len())
                }
                _ => None,
            };
            Box::pin(transfer_file(
                host,
                TransferKind::Download {
                    remote_path,
                    local_path,
                    resume_offset,
                },
                progress,
            ))
        }
        SftpQueuedTransferKind::DownloadFolder {
            host,
            remote_path,
            local_path,
        } => Box::pin(transfer_folder_download(
            host,
            remote_path,
            local_path,
            progress,
        )),
        SftpQueuedTransferKind::RemoteToRemoteFile {
            dest_host,
            source_host,
            source_path,
            dest_path,
        } => Box::pin(transfer_file(
            dest_host,
            TransferKind::RemoteToRemote {
                source_host,
                source_path,
                dest_path,
                overwrite: false,
                resume_offset: None,
            },
            progress,
        )),
        SftpQueuedTransferKind::RemoteToRemoteFolder {
            dest_host,
            source_host,
            source_path,
            dest_path,
        } => Box::pin(transfer_folder_remote_to_remote(
            source_host,
            dest_host,
            source_path,
            dest_path,
            progress,
        )),
    }
}

pub(crate) fn start_next_queued_transfer(app: &mut App) -> Task<Message> {
    if app.sftp.state.transfer_queue.is_empty() {
        app.sftp.state.overwrite_apply_all = None;
        Task::none()
    } else {
        let item = app.sftp.state.transfer_queue.remove(0);
        start_item(app, item)
    }
}

pub(super) fn enqueue_transfer(app: &mut App, item: SftpQueuedTransfer) -> Task<Message> {
    if app.sftp.state.transfer_queue.len() >= MAX_TRANSFER_QUEUE {
        return Task::perform(
            async { t!("sftp.queue_full", max = MAX_TRANSFER_QUEUE).to_string() },
            Message::ShowToast,
        );
    }
    let name = item.file_name.clone();
    app.sftp.state.transfer_queue.push(item);
    Task::perform(
        async move { t!("sftp.queued", name = &name).to_string() },
        Message::ShowToast,
    )
}

pub(crate) fn resolve_duplicate_local_name(local_path: &Path) -> Result<PathBuf, String> {
    let path = local_path.to_string_lossy();
    for number in 1_u64..1000 {
        let candidate = super::super::shared::duplicate_name_candidate(&path, number);
        let candidate_path = Path::new(&candidate);
        if !candidate_path.exists() {
            return Ok(candidate_path.to_path_buf());
        }
    }
    Err(t!(
        "sftp.error.naming_conflict",
        path = &local_path.display().to_string()
    )
    .to_string())
}
