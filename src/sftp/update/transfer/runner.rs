//! Executes individual SFTP file and folder transfers.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::core::data::Host;
use crate::sftp::{SftpTransferFailure, SftpTransferProgress};
use rust_i18n::t;

pub(crate) enum TransferKind {
    Upload {
        local_path: PathBuf,
        remote_path: String,
        overwrite: bool,
        resume_offset: Option<u64>,
    },
    /// Upload through a temporary path, then rename on success.
    UploadAtomic {
        local_path: PathBuf,
        remote_path: String,
    },
    Download {
        remote_path: String,
        local_path: PathBuf,
        resume_offset: Option<u64>,
    },
    RemoteToRemote {
        source_host: Host,
        source_path: String,
        dest_path: String,
        overwrite: bool,
        resume_offset: Option<u64>,
    },
    /// Remote copy through a temporary destination path.
    RemoteToRemoteAtomic {
        source_host: Host,
        source_path: String,
        dest_path: String,
    },
}

pub(crate) async fn transfer_file(
    host: Host,
    transfer: TransferKind,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<String, SftpTransferFailure> {
    match transfer {
        TransferKind::Upload {
            local_path,
            remote_path,
            overwrite,
            resume_offset,
        } => {
            crate::ssh::upload_sftp_file(
                host.clone(),
                local_path.clone(),
                remote_path.clone(),
                progress,
                overwrite,
                resume_offset,
            )
            .await?;
            crate::ssh::verify_upload_checksum(&host, &local_path, &remote_path).await?;
            let name = local_file_name(&local_path);
            Ok(t!("sftp.uploaded", name = &name).to_string())
        }
        TransferKind::UploadAtomic {
            local_path,
            remote_path,
        } => {
            crate::ssh::upload_sftp_file_atomic(
                host.clone(),
                local_path.clone(),
                remote_path.clone(),
                progress,
            )
            .await?;
            crate::ssh::verify_upload_checksum(&host, &local_path, &remote_path).await?;
            let name = local_file_name(&local_path);
            Ok(t!("sftp.uploaded", name = &name).to_string())
        }
        TransferKind::Download {
            remote_path,
            local_path,
            resume_offset,
        } => {
            crate::ssh::download_sftp_file(
                host.clone(),
                remote_path.clone(),
                local_path.clone(),
                progress,
                resume_offset,
            )
            .await?;
            crate::ssh::verify_download_checksum(&host, &remote_path, &local_path).await?;
            Ok(t!("sftp.downloaded", path = &remote_path).to_string())
        }
        TransferKind::RemoteToRemote {
            source_host,
            source_path,
            dest_path,
            overwrite,
            resume_offset,
        } => {
            crate::ssh::copy_sftp_file_between_hosts(
                source_host.clone(),
                host.clone(),
                source_path.clone(),
                dest_path.clone(),
                progress,
                overwrite,
                resume_offset,
            )
            .await?;
            crate::ssh::verify_remote_to_remote_checksum(
                &source_host,
                &host,
                &source_path,
                &dest_path,
            )
            .await?;
            Ok(t!("sftp.copied", path = &source_path).to_string())
        }
        TransferKind::RemoteToRemoteAtomic {
            source_host,
            source_path,
            dest_path,
        } => {
            crate::ssh::copy_sftp_file_between_hosts_atomic(
                source_host.clone(),
                host.clone(),
                source_path.clone(),
                dest_path.clone(),
                progress,
            )
            .await?;
            crate::ssh::verify_remote_to_remote_checksum(
                &source_host,
                &host,
                &source_path,
                &dest_path,
            )
            .await?;
            Ok(t!("sftp.copied", path = &source_path).to_string())
        }
    }
}

pub(crate) async fn transfer_folder(
    host: Host,
    local_path: PathBuf,
    remote_path: String,
    progress: Arc<Mutex<SftpTransferProgress>>,
    overwrite: bool,
) -> Result<String, SftpTransferFailure> {
    crate::ssh::upload_sftp_folder(host, local_path.clone(), remote_path, progress, overwrite)
        .await?;
    let name = local_file_name(&local_path);
    Ok(t!("sftp.uploaded_folder", name = &name).to_string())
}

pub(crate) async fn transfer_folder_atomic(
    host: Host,
    local_path: PathBuf,
    remote_path: String,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<String, SftpTransferFailure> {
    crate::ssh::upload_sftp_folder_atomic(host, local_path.clone(), remote_path, progress).await?;
    let name = local_file_name(&local_path);
    Ok(t!("sftp.uploaded_folder", name = &name).to_string())
}

pub(super) async fn transfer_folder_download(
    host: Host,
    remote_path: String,
    local_path: PathBuf,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<String, SftpTransferFailure> {
    crate::ssh::download_sftp_folder(host, remote_path.clone(), local_path, progress).await?;
    Ok(t!("sftp.downloaded_folder", path = &remote_path).to_string())
}

pub(crate) async fn transfer_folder_remote_to_remote(
    source_host: Host,
    dest_host: Host,
    source_path: String,
    dest_path: String,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<String, SftpTransferFailure> {
    crate::ssh::copy_sftp_folder_between_hosts(
        source_host,
        dest_host,
        source_path.clone(),
        dest_path,
        progress,
    )
    .await?;
    Ok(t!("sftp.copied_folder", path = &source_path).to_string())
}

fn local_file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}
