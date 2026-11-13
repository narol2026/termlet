// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::io::SeekFrom;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use russh::Disconnect;
use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{FileAttributes, FileType as SftpFileType, OpenFlags, StatusCode};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite, AsyncWriteExt};

use crate::core::data::Host;
use crate::core::diagnostics::Level;
use crate::sftp::{SftpTransferFailure, SftpTransferProgress};
use rust_i18n::t;

use super::exec::ssh_exec_command;
use super::path_utils::{remote_parent_and_name, shell_escape};
use super::session::{
    SFTP_REQUEST_TIMEOUT, connect_authenticated_session, connect_sftp, release_sftp, run_sftp_task,
};
use super::sftp_dir::{
    sftp_create_remote_dir, sftp_remove_dir_recursive, sftp_remove_file, sftp_rename_file,
};

/// Suffix appended to the target path when performing atomic uploads
/// (upload-to-temp then rename). Files ≥ the threshold use the legacy
/// delete-then-upload strategy instead to avoid temporary disk pressure.
const SFTP_ATOMIC_TMP_SUFFIX: &str = ".termlet_tmp";
pub(crate) const SFTP_ATOMIC_SIZE_THRESHOLD: u64 = 100 * 1024 * 1024; // 100 MB
const SFTP_TRANSFER_BUFFER_SIZE: usize = 64 * 1024;
/// How often the shared transfer progress may be updated while data is
/// streaming. Per-chunk updates are unnecessary for the UI and, on fast links,
/// they cause enough `Mutex` churn to starve the main thread.
const SFTP_PROGRESS_UPDATE_INTERVAL: Duration = Duration::from_millis(100);
/// If a single SFTP read/write makes no progress for this long, the transfer
/// is treated as stalled (usually a dropped network connection) and fails so
/// the task can terminate instead of leaving "transferring" forever.
const SFTP_TRANSFER_STALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferSide {
    Local,
    Remote,
}

/// Close an SFTP file without letting a dropped connection turn cleanup into a
/// second indefinite hang.
async fn shutdown_sftp_file<W>(file: &mut W)
where
    W: AsyncWrite + Unpin,
{
    let _ = tokio::time::timeout(SFTP_REQUEST_TIMEOUT, file.shutdown()).await;
}

fn transfer_side_failure(side: TransferSide, message: String) -> SftpTransferFailure {
    match side {
        TransferSide::Remote => SftpTransferFailure::connection(message),
        TransferSide::Local => SftpTransferFailure::Failed { message },
    }
}

/// Mark the transfer as connected before the first data chunk arrives.
fn mark_transfer_connected(progress: &Arc<Mutex<SftpTransferProgress>>) {
    if let Ok(mut progress) = progress.lock()
        && progress.start_time.is_none()
    {
        let now = std::time::Instant::now();
        progress.start_time = Some(now);
        progress.last_speed_update = Some(now);
    }
}

/// Result of an optional post-transfer SHA-256 verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferChecksum {
    /// `sha256sum` was unavailable on at least one required remote side, so
    /// verification was skipped without changing the transfer result.
    Skipped,
    /// Both sides were hashed and the digests matched.
    Verified(String),
}

/// Verify an upload after the final remote file has been written.
pub async fn verify_upload_checksum(
    host: &Host,
    local_path: &Path,
    remote_path: &str,
) -> Result<TransferChecksum, String> {
    let Some(remote_hash) = remote_sha256(host, remote_path).await? else {
        return Ok(TransferChecksum::Skipped);
    };
    let local_hash = local_sha256(local_path).await?;
    compare_hashes(&local_hash, &remote_hash)
}

/// Verify a download after the local file has been written.
pub async fn verify_download_checksum(
    host: &Host,
    remote_path: &str,
    local_path: &Path,
) -> Result<TransferChecksum, String> {
    let Some(remote_hash) = remote_sha256(host, remote_path).await? else {
        return Ok(TransferChecksum::Skipped);
    };
    let local_hash = local_sha256(local_path).await?;
    compare_hashes(&local_hash, &remote_hash)
}

/// Verify a remote-to-remote copy. Both hosts must provide `sha256sum`;
/// otherwise the check is skipped.
pub async fn verify_remote_to_remote_checksum(
    source_host: &Host,
    dest_host: &Host,
    source_path: &str,
    dest_path: &str,
) -> Result<TransferChecksum, String> {
    let Some(source_hash) = remote_sha256(source_host, source_path).await? else {
        return Ok(TransferChecksum::Skipped);
    };
    let Some(dest_hash) = remote_sha256(dest_host, dest_path).await? else {
        return Ok(TransferChecksum::Skipped);
    };
    compare_hashes(&source_hash, &dest_hash)
}

async fn remote_sha256(host: &Host, remote_path: &str) -> Result<Option<String>, String> {
    // Use a single round-trip: when `sha256sum` is unavailable the `if`
    // exits 0 and produces no stdout, which we treat as "skip". When it is
    // available, the command's exit status and stderr propagate normally, so a
    // missing/unreadable file is still reported as a verification failure.
    let command = format!(
        "if command -v sha256sum >/dev/null 2>&1; then sha256sum -- {}; fi",
        shell_escape(remote_path)
    );
    let output = ssh_exec_command(host.clone(), command).await?;
    let output = output.trim();
    if output.is_empty() {
        return Ok(None);
    }
    // GNU coreutils prefixes the digest line with a backslash when the file
    // name contains an escaped character such as a newline.
    let output = output.strip_prefix('\\').unwrap_or(output);

    let hash = output
        .split_whitespace()
        .next()
        .ok_or_else(|| "sha256sum produced no hash in its output".to_string())?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("sha256sum returned an invalid hash: {hash}"));
    }
    Ok(Some(hash.to_ascii_lowercase()))
}

async fn local_sha256(path: &Path) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| format!("Failed to open local file for checksum: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; SFTP_TRANSFER_BUFFER_SIZE].into_boxed_slice();

    loop {
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| format!("Failed to read local file for checksum: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(to_hex(&hasher.finalize()))
}

/// Render a digest as lowercase hex.
///
/// Both the local and the remote checksum path produce the same textual form,
/// so the comparison in [`compare_hashes`] stays a plain string equality.
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn compare_hashes(actual: &str, expected: &str) -> Result<TransferChecksum, String> {
    if actual == expected {
        Ok(TransferChecksum::Verified(actual.to_string()))
    } else {
        crate::core::diagnostics::log(
            Level::Warn,
            "SFTP",
            format!("checksum mismatch: actual={actual} expected={expected}"),
        );
        Err(t!("sftp.checksum_mismatch").to_string())
    }
}

/// Best-effort detection for the network/connection failures that motivated
/// aborting a whole queue. Non-network failures (for example permissions or a
/// missing local file) should not stop every queued transfer that follows.
pub async fn upload_sftp_file(
    host: Host,
    local_path: std::path::PathBuf,
    remote_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    overwrite: bool,
    resume_offset: Option<u64>,
) -> Result<(), SftpTransferFailure> {
    run_sftp_task(move || {
        upload_sftp_file_async(
            host,
            local_path,
            remote_path,
            progress,
            overwrite,
            resume_offset,
        )
    })
    .await
}

/// Public wrapper for [`upload_sftp_file_atomic_async`].
pub async fn upload_sftp_file_atomic(
    host: Host,
    local_path: std::path::PathBuf,
    remote_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    run_sftp_task(move || upload_sftp_file_atomic_async(host, local_path, remote_path, progress))
        .await
}

pub async fn download_sftp_file(
    host: Host,
    remote_path: String,
    local_path: std::path::PathBuf,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    resume_offset: Option<u64>,
) -> Result<(), SftpTransferFailure> {
    run_sftp_task(move || {
        download_sftp_file_async(host, remote_path, local_path, progress, resume_offset)
    })
    .await
}

pub async fn copy_sftp_file_between_hosts(
    source_host: Host,
    dest_host: Host,
    source_path: String,
    dest_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    overwrite: bool,
    resume_offset: Option<u64>,
) -> Result<(), SftpTransferFailure> {
    run_sftp_task(move || {
        copy_sftp_file_between_hosts_async(
            source_host,
            dest_host,
            source_path,
            dest_path,
            progress,
            overwrite,
            resume_offset,
        )
    })
    .await
}

/// Public wrapper for [`copy_sftp_file_between_hosts_atomic_async`].
pub async fn copy_sftp_file_between_hosts_atomic(
    source_host: Host,
    dest_host: Host,
    source_path: String,
    dest_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    run_sftp_task(move || {
        copy_sftp_file_between_hosts_atomic_async(
            source_host,
            dest_host,
            source_path,
            dest_path,
            progress,
        )
    })
    .await
}

async fn upload_sftp_file_async(
    host: Host,
    local_path: std::path::PathBuf,
    remote_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    overwrite: bool,
    resume_offset: Option<u64>,
) -> Result<(), SftpTransferFailure> {
    let local_metadata = std::fs::metadata(&local_path)
        .map_err(|error| format!("Failed to open local file: {error}"))?;
    let local_size = local_metadata.len();
    let local_permissions = local_unix_permissions(&local_path)?;
    let ((mut session, sftp), _) = connect_sftp(host)
        .await
        .map_err(SftpTransferFailure::connection)?;
    mark_transfer_connected(&progress);

    // Determine whether we should attempt to resume an interrupted upload.
    // We query the remote file size *before* opening so we can choose the
    // right OpenFlags (TRUNCATE for a fresh upload, no TRUNCATE for resume).
    let mut remote_file_exists = false;
    let effective_offset: Option<u64> = if resume_offset.is_some() {
        match sftp.metadata(remote_path.clone()).await {
            Ok(metadata) => {
                if metadata.is_dir() {
                    release_sftp(&mut session, sftp).await;
                    return Err(SftpTransferFailure::FileExists {
                        is_dir: true,
                        remote_path,
                        local_path: Some(local_path.display().to_string()),
                    });
                }
                remote_file_exists = true;
                let remote_size = metadata.len();
                // Only resume when the remote file is a genuine partial copy:
                // it must be non-empty AND smaller than the local source.
                (remote_size > 0 && remote_size < local_size).then_some(remote_size)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Skipped when resuming: the caller then owns the existing partial file.
    let skip_exists_check = overwrite || resume_offset.is_some();
    if !skip_exists_check && let Ok(metadata) = sftp.metadata(remote_path.clone()).await {
        let is_dir = metadata.is_dir();
        release_sftp(&mut session, sftp).await;
        return Err(SftpTransferFailure::FileExists {
            is_dir,
            remote_path,
            local_path: Some(local_path.display().to_string()),
        });
    }

    // When the remote file already exists and we're not resuming,
    // explicitly delete it first instead of relying on TRUNCATE.
    // TRUNCATE requires write permission on the file itself and fails
    // with ETXTBUSY (SFTP Failure) for running executables, while
    // unlink only needs directory write permission and succeeds even
    // when the file is being executed (the inode stays alive until
    // the process exits).
    if remote_file_exists && effective_offset.is_none() {
        let _ = sftp.remove_file(remote_path.clone()).await;
    }

    let open_flags = if effective_offset.is_some() {
        OpenFlags::CREATE | OpenFlags::WRITE
    } else {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    };
    let mut remote_file = sftp
        .open_with_flags_and_attributes(
            remote_path.clone(),
            open_flags,
            attrs_with_permissions(local_permissions),
        )
        .await
        .map_err(|error| format_sftp_open_error(&error, &remote_path))?;
    let mut local_file = tokio::fs::File::open(&local_path)
        .await
        .map_err(|error| format!("Failed to open local file: {error}"))?;

    if effective_offset.is_some() {
        // Drives the "Resuming upload/download" label in the transfer panel.
        if let Ok(mut p) = progress.lock() {
            p.resumed = true;
        }
    }

    match pump_transfer_stream(
        &mut local_file,
        &mut remote_file,
        &progress,
        0,
        effective_offset,
        "local file",
        "remote file",
        TransferSide::Local,
        TransferSide::Remote,
    )
    .await
    {
        Ok(_) => {}
        Err(error) => {
            shutdown_sftp_file(&mut remote_file).await;
            release_sftp(&mut session, sftp).await;
            return Err(error);
        }
    }

    // Size integrity check: every write was acked by the server, but an
    // fstat size compare still guards against servers that truncate or
    // persist fewer bytes than they acknowledged.
    let remote_size = remote_file
        .metadata()
        .await
        .map_err(|error| format!("Failed to stat remote file after upload: {error}"))?
        .len();
    if remote_size != local_size {
        shutdown_sftp_file(&mut remote_file).await;
        release_sftp(&mut session, sftp).await;
        return Err(SftpTransferFailure::Failed {
            message: t!(
                "sftp.size_mismatch",
                remote = remote_size,
                local = local_size,
            )
            .to_string(),
        });
    }

    remote_file
        .shutdown()
        .await
        .map_err(|error| format!("Failed to close remote file: {error}"))?;
    if transfer_cancelled(&progress) {
        release_sftp(&mut session, sftp).await;
        Err(SftpTransferFailure::Cancelled)
    } else {
        let result = apply_remote_permissions(&sftp, &remote_path, local_permissions).await;
        release_sftp(&mut session, sftp).await;
        result.map_err(SftpTransferFailure::from)
    }
}

/// Atomic file upload: uploads to a temporary path first, then renames
/// on success. If anything fails (including cancellation), the temp file
/// is cleaned up and the original is left untouched.
///
/// This is the core of the safe-replace strategy for files smaller than
/// [`SFTP_ATOMIC_SIZE_THRESHOLD`].
async fn upload_sftp_file_atomic_async(
    host: Host,
    local_path: std::path::PathBuf,
    remote_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let tmp_path = format!("{remote_path}{SFTP_ATOMIC_TMP_SUFFIX}");

    let upload_result = upload_sftp_file_async(
        host.clone(),
        local_path,
        tmp_path.clone(),
        progress,
        /* overwrite = */ true,
        /* resume_offset = */ None,
    )
    .await;

    match upload_result {
        Ok(()) => {
            // SFTP v3 rename (SSH_FXP_RENAME) does not support
            // overwriting an existing file — the server responds with
            // SSH_FX_FILE_ALREADY_EXISTS.  Delete the original before
            // renaming temp → target.
            if let Err(e) = sftp_remove_file(host.clone(), remote_path.clone()).await {
                // Couldn't delete the original — clean up temp so we
                // don't leave litter.  Original is still intact.
                let _ = sftp_remove_file(host, tmp_path).await;
                return Err(SftpTransferFailure::Failed {
                    message: format!("Failed to remove original file before rename: {e}"),
                });
            }
            // Rename temp → target.  In the unlikely event this fails,
            // the temp file still holds the complete data — don't
            // delete it so the user can recover manually.
            sftp_rename_file(host, tmp_path.clone(), remote_path)
                .await
                .map_err(|e| SftpTransferFailure::Failed {
                    message: format!(
                        "Upload succeeded but rename failed \
                         (data preserved at {tmp_path}): {e}"
                    ),
                })
        }
        Err(e) => {
            // Upload (or cancellation) failed before touching the
            // original — clean up the partial temp file.
            let _ = sftp_remove_file(host, tmp_path).await;
            Err(e)
        }
    }
}

pub(super) async fn download_sftp_file_async(
    host: Host,
    remote_path: String,
    local_path: std::path::PathBuf,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    resume_offset: Option<u64>,
) -> Result<(), SftpTransferFailure> {
    let ((mut session, sftp), _) = connect_sftp(host)
        .await
        .map_err(SftpTransferFailure::connection)?;
    mark_transfer_connected(&progress);

    // Read the remote size up front so a premature EOF (which SFTP reports as
    // a clean end-of-file) can't silently produce a truncated local file.
    let remote_size: u64 = {
        let size =
            match tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.metadata(remote_path.clone()))
                .await
            {
                Ok(Ok(metadata)) => metadata.size,
                Ok(Err(err)) => {
                    release_sftp(&mut session, sftp).await;
                    return Err(SftpTransferFailure::connection(format!(
                        "Failed to read remote file size: {err}"
                    )));
                }
                Err(_) => {
                    release_sftp(&mut session, sftp).await;
                    return Err(SftpTransferFailure::connection(
                        "Timed out reading remote file size",
                    ));
                }
            };
        let Some(size) = size else {
            release_sftp(&mut session, sftp).await;
            return Err(SftpTransferFailure::connection(
                "Remote file size unavailable",
            ));
        };
        size
    };

    if let Some(offset) = resume_offset.filter(|&o| o > remote_size) {
        release_sftp(&mut session, sftp).await;
        return Err(SftpTransferFailure::Failed {
            message: format!(
                "Local partial file ({offset} bytes) is larger than the remote file \
             ({remote_size} bytes); restart the download"
            ),
        });
    }

    let mut remote_file = sftp
        .open(remote_path)
        .await
        .map_err(|error| format!("Failed to open remote file: {error}"))?;
    // Resume path: the file may already hold the bytes downloaded so far, so
    // keep its contents and overwrite from the resume offset instead of
    // truncating it back to zero length.
    let mut local_file = if resume_offset.is_some() {
        tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&local_path)
            .await
            .map_err(|error| format!("Failed to open local file for resume: {error}"))?
    } else {
        tokio::fs::File::create(&local_path)
            .await
            .map_err(|error| format!("Failed to create local file: {error}"))?
    };

    let transferred = match pump_transfer_stream(
        &mut remote_file,
        &mut local_file,
        &progress,
        0,
        resume_offset,
        "remote file",
        "local file",
        TransferSide::Remote,
        TransferSide::Local,
    )
    .await
    {
        Ok(transferred) => transferred,
        Err(error) => {
            shutdown_sftp_file(&mut remote_file).await;
            release_sftp(&mut session, sftp).await;
            return Err(error);
        }
    };

    shutdown_sftp_file(&mut remote_file).await;
    release_sftp(&mut session, sftp).await;

    if transfer_cancelled(&progress) {
        Err(SftpTransferFailure::Cancelled)
    } else if transferred != remote_size {
        Err(SftpTransferFailure::connection(
            t!(
                "sftp.transfer_download_incomplete_no_path",
                received = transferred,
                total = remote_size
            )
            .to_string(),
        ))
    } else {
        Ok(())
    }
}

async fn copy_sftp_file_between_hosts_async(
    source_host: Host,
    dest_host: Host,
    source_path: String,
    dest_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    overwrite: bool,
    resume_offset: Option<u64>,
) -> Result<(), SftpTransferFailure> {
    let ((mut source_session, source_sftp), _) = connect_sftp(source_host)
        .await
        .map_err(SftpTransferFailure::connection)?;
    let ((mut dest_session, dest_sftp), _) = connect_sftp(dest_host)
        .await
        .map_err(SftpTransferFailure::connection)?;
    mark_transfer_connected(&progress);
    let skip_exists_check = overwrite || resume_offset.is_some();
    if !skip_exists_check && let Ok(metadata) = dest_sftp.metadata(dest_path.clone()).await {
        let is_dir = metadata.is_dir();
        release_sftp(&mut source_session, source_sftp).await;
        release_sftp(&mut dest_session, dest_sftp).await;
        return Err(SftpTransferFailure::FileExists {
            is_dir,
            remote_path: dest_path,
            local_path: None,
        });
    }
    // When overwriting, explicitly delete the existing file first.
    // This is more robust than relying on TRUNCATE, because some SFTP
    // servers require write permission on the file itself for TRUNCATE
    // but only directory permission for CREATE.
    if overwrite && resume_offset.is_none() {
        let _ = dest_sftp.remove_file(dest_path.clone()).await;
    }
    let mut source_file = source_sftp
        .open(source_path)
        .await
        .map_err(|error| format!("Failed to open source remote file: {error}"))?;
    let open_flags = if resume_offset.is_some() {
        OpenFlags::CREATE | OpenFlags::WRITE
    } else {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    };
    let mut dest_file = dest_sftp
        .open_with_flags(dest_path.clone(), open_flags)
        .await
        .map_err(|error| format!("Failed to create destination remote file: {error}"))?;

    match pump_transfer_stream(
        &mut source_file,
        &mut dest_file,
        &progress,
        0,
        resume_offset,
        "source remote file",
        "destination remote file",
        TransferSide::Remote,
        TransferSide::Remote,
    )
    .await
    {
        Ok(_) => {}
        Err(error) => {
            shutdown_sftp_file(&mut source_file).await;
            shutdown_sftp_file(&mut dest_file).await;
            release_sftp(&mut source_session, source_sftp).await;
            release_sftp(&mut dest_session, dest_sftp).await;
            return Err(error);
        }
    }

    shutdown_sftp_file(&mut source_file).await;
    dest_file
        .shutdown()
        .await
        .map_err(|error| format!("Failed to close destination remote file: {error}"))?;
    if transfer_cancelled(&progress) {
        release_sftp(&mut source_session, source_sftp).await;
        release_sftp(&mut dest_session, dest_sftp).await;
        Err(SftpTransferFailure::Cancelled)
    } else {
        release_sftp(&mut source_session, source_sftp).await;
        release_sftp(&mut dest_session, dest_sftp).await;
        Ok(())
    }
}

/// Atomic remote-to-remote copy: copies to a temporary path on the
/// destination first, then renames on success. If anything fails,
/// the temp file is cleaned up and the original is left untouched.
async fn copy_sftp_file_between_hosts_atomic_async(
    source_host: Host,
    dest_host: Host,
    source_path: String,
    dest_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let tmp_path = format!("{dest_path}{SFTP_ATOMIC_TMP_SUFFIX}");

    let copy_result = copy_sftp_file_between_hosts_async(
        source_host.clone(),
        dest_host.clone(),
        source_path,
        tmp_path.clone(),
        progress,
        /* overwrite = */ true,
        /* resume_offset = */ None,
    )
    .await;

    match copy_result {
        Ok(()) => {
            // SFTP v3 rename (SSH_FXP_RENAME) does not support
            // overwriting an existing file — the server responds with
            // SSH_FX_FILE_ALREADY_EXISTS.  Delete the original before
            // renaming temp → target.
            if let Err(e) = sftp_remove_file(dest_host.clone(), dest_path.clone()).await {
                let _ = sftp_remove_file(dest_host, tmp_path).await;
                return Err(SftpTransferFailure::Failed {
                    message: format!("Failed to remove original file before rename: {e}"),
                });
            }
            sftp_rename_file(dest_host, tmp_path.clone(), dest_path)
                .await
                .map_err(|e| SftpTransferFailure::Failed {
                    message: format!(
                        "Remote copy succeeded but rename failed \
                         (data preserved at {tmp_path}): {e}"
                    ),
                })
        }
        Err(e) => {
            // Copy (or cancellation) failed before touching the
            // original — clean up the partial temp file.
            let _ = sftp_remove_file(dest_host, tmp_path).await;
            Err(e)
        }
    }
}

/// Format an SFTP error for display to the user, including the remote path
/// and human-readable explanations for common status codes.
fn format_sftp_open_error(error: &SftpError, remote_path: &str) -> String {
    match &error {
        SftpError::Status(status) => match status.status_code {
            StatusCode::Failure => {
                t!("sftp.create_remote_permission_full", path = remote_path).to_string()
            }
            StatusCode::PermissionDenied => {
                t!("sftp.create_remote_permission", path = remote_path).to_string()
            }
            StatusCode::NoSuchFile => {
                t!("sftp.create_remote_no_parent", path = remote_path).to_string()
            }
            _ => format!("Failed to create {remote_path}: {error}"),
        },
        _ => format!("Failed to create {remote_path}: {error}"),
    }
}

/// Pump bytes from an async reader into an async writer while keeping
/// `SftpTransferProgress` in sync. Both top-level upload/download and
/// remote-to-remote file transfers use this helper, and the folder
/// recursive implementations reuse it as well.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
async fn pump_transfer_stream<R, W>(
    reader: &mut R,
    writer: &mut W,
    progress: &Arc<std::sync::Mutex<SftpTransferProgress>>,
    transferred_before: u64,
    resume_offset: Option<u64>,
    source: &str,
    destination: &str,
    source_side: TransferSide,
    destination_side: TransferSide,
) -> Result<u64, SftpTransferFailure>
where
    R: AsyncRead + AsyncSeek + Unpin,
    W: AsyncWrite + AsyncSeek + Unpin,
{
    let mut transferred = transferred_before;

    crate::core::diagnostics::log(
        Level::Info,
        "SFTP",
        format!(
            "pump_transfer_stream: start {source} -> {destination}, \
             transferred_before={transferred_before}, resume_offset={resume_offset:?}"
        ),
    );

    if let Some(offset) = resume_offset {
        reader
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| {
                transfer_side_failure(source_side, format!("Failed to seek {source}: {error}"))
            })?;
        writer
            .seek(SeekFrom::Start(offset))
            .await
            .map_err(|error| {
                transfer_side_failure(
                    destination_side,
                    format!("Failed to seek {destination}: {error}"),
                )
            })?;
        transferred = transferred_before + offset;
    }

    update_transfer_progress(progress, transferred);
    let mut last_progress_sync = std::time::Instant::now();

    let mut buffer = vec![0_u8; SFTP_TRANSFER_BUFFER_SIZE].into_boxed_slice();

    loop {
        if transfer_cancelled(progress) {
            return Err(SftpTransferFailure::Cancelled);
        }

        let read = tokio::time::timeout(SFTP_TRANSFER_STALL_TIMEOUT, reader.read(&mut buffer))
            .await
            .map_err(|_error| {
                crate::core::diagnostics::log(
                    Level::Warn,
                    "SFTP",
                    format!(
                        "pump_transfer_stream: no progress reading {source} for {}s; \
                         treating the connection as stalled",
                        SFTP_TRANSFER_STALL_TIMEOUT.as_secs()
                    ),
                );
                transfer_side_failure(
                    source_side,
                    format!(
                        "Timed out reading {source} after {}s; the network connection may be lost",
                        SFTP_TRANSFER_STALL_TIMEOUT.as_secs()
                    ),
                )
            })?
            .map_err(|error| {
                transfer_side_failure(source_side, format!("Failed to read {source}: {error}"))
            })?;
        if read == 0 {
            break;
        }

        tokio::time::timeout(
            SFTP_TRANSFER_STALL_TIMEOUT,
            writer.write_all(&buffer[..read]),
        )
        .await
        .map_err(|_error| {
            crate::core::diagnostics::log(
                Level::Warn,
                "SFTP",
                format!(
                    "pump_transfer_stream: no progress writing {destination} for {}s; \
                         treating the connection as stalled",
                    SFTP_TRANSFER_STALL_TIMEOUT.as_secs()
                ),
            );
            transfer_side_failure(
                destination_side,
                format!(
                    "Timed out writing {destination} after {}s; the network connection may be lost",
                    SFTP_TRANSFER_STALL_TIMEOUT.as_secs()
                ),
            )
        })?
        .map_err(|error| {
            transfer_side_failure(
                destination_side,
                format!("Failed to write {destination}: {error}"),
            )
        })?;
        transferred += read as u64;
        let now = std::time::Instant::now();
        if now.duration_since(last_progress_sync) >= SFTP_PROGRESS_UPDATE_INTERVAL {
            update_transfer_progress(progress, transferred);
            last_progress_sync = now;
        }
    }

    update_transfer_progress(progress, transferred);
    crate::core::diagnostics::log(
        Level::Info,
        "SFTP",
        format!("pump_transfer_stream: finished {source} -> {destination}, {transferred} bytes"),
    );
    Ok(transferred)
}

fn transfer_cancelled(progress: &Arc<std::sync::Mutex<SftpTransferProgress>>) -> bool {
    progress.lock().map_or(true, |progress| progress.cancelled)
}

fn update_transfer_progress(
    progress: &Arc<std::sync::Mutex<SftpTransferProgress>>,
    transferred_bytes: u64,
) {
    if let Ok(mut progress) = progress.lock() {
        progress.transferred_bytes = if progress.total_bytes == 0 {
            transferred_bytes
        } else {
            transferred_bytes.min(progress.total_bytes)
        };
        let now = std::time::Instant::now();
        if progress.start_time.is_none() {
            progress.start_time = Some(now);
            progress.last_speed_update = Some(now);
        }
        // Update speed at most once per second
        let should_update = progress
            .last_speed_update
            .is_some_and(|t| now.duration_since(t).as_secs_f64() >= 1.0);
        if should_update && let Some(start) = progress.start_time {
            let elapsed = now.duration_since(start).as_secs_f64();
            if elapsed > 0.0 {
                progress.speed = progress.transferred_bytes as f64 / elapsed;
                progress.last_speed_update = Some(now);
            }
        }
    }
}

fn create_tar_gz(src_dir: &Path, dest_tar: &Path) -> Result<u64, String> {
    let _guard = crate::core::diagnostics::perf_guard_dyn(
        Level::Debug,
        "IO",
        format!("create_tar_gz({})", src_dir.display()),
        std::time::Duration::from_millis(500),
    );
    let tar_file =
        std::fs::File::create(dest_tar).map_err(|e| format!("Failed to create tar file: {e}"))?;
    let encoder = GzEncoder::new(tar_file, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    archive.follow_symlinks(false);

    let dir_name = src_dir
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("upload"))
        .to_string_lossy()
        .to_string();

    archive
        .append_dir_all(&dir_name, src_dir)
        .map_err(|e| format!("Failed to create tar archive: {e}"))?;

    let encoder = archive
        .into_inner()
        .map_err(|e| format!("Failed to finalize tar archive: {e}"))?;

    let tar_file = encoder
        .finish()
        .map_err(|e| format!("Failed to finish gzip compression: {e}"))?;

    let compressed_size = tar_file.metadata().map_or(0, |m| m.len());

    Ok(compressed_size)
}

#[cfg(unix)]
fn local_unix_permissions(path: &Path) -> Result<Option<u32>, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| {
        format!(
            "Failed to read metadata for {path}: {e}",
            path = path.display(),
            e = e
        )
    })?;
    Ok(Some(metadata.permissions().mode()))
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "Unix permission bits are unavailable on Windows; keep the shared caller signature"
)]
fn local_unix_permissions(_path: &Path) -> Result<Option<u32>, String> {
    Ok(None)
}

fn attrs_with_permissions(permissions: Option<u32>) -> FileAttributes {
    let mut attrs = FileAttributes::empty();
    attrs.permissions = permissions;
    attrs
}

async fn apply_remote_permissions(
    sftp: &SftpSession,
    remote_path: &str,
    permissions: Option<u32>,
) -> Result<(), String> {
    let Some(permissions) = permissions else {
        return Ok(());
    };

    sftp.set_metadata(
        remote_path.to_string(),
        attrs_with_permissions(Some(permissions)),
    )
    .await
    .map_err(|e| format!("Failed to set remote permissions on {remote_path}: {e}"))
}

async fn remote_extract_tar(host: Host, tar_path: String, dest_dir: String) -> Result<(), String> {
    let command = format!(
        "tar -xpzf {} -C {}",
        shell_escape(&tar_path),
        shell_escape(&dest_dir),
    );
    ssh_exec_command(host, command).await?;
    Ok(())
}

async fn upload_folder_via_archive(
    host: Host,
    local_dir: PathBuf,
    remote_dir: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    overwrite: bool,
) -> Result<(), SftpTransferFailure> {
    // Check if remote directory already exists before proceeding
    if !overwrite {
        let ((mut session, sftp), _) = connect_sftp(host.clone())
            .await
            .map_err(SftpTransferFailure::connection)?;
        mark_transfer_connected(&progress);
        if let Ok(metadata) = sftp.symlink_metadata(remote_dir.clone()).await
            && metadata.file_type() == SftpFileType::Dir
        {
            release_sftp(&mut session, sftp).await;
            return Err(SftpTransferFailure::FileExists {
                is_dir: true,
                remote_path: remote_dir,
                local_path: Some(local_dir.display().to_string()),
            });
        }
        release_sftp(&mut session, sftp).await;
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let archive_name = format!("termlet_upload_{timestamp}.tar.gz");
    let local_tar = std::env::temp_dir().join(&archive_name);
    let remote_tar = format!("/tmp/{archive_name}");

    // Step 1: Create tar.gz locally
    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.current_file = "Compressing...".to_string();
    };
    let compressed_size = create_tar_gz(&local_dir, &local_tar)?;

    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.total_bytes = compressed_size;
        p.file_name.clone_from(&archive_name);
    };

    // Step 2: Upload the archive
    upload_sftp_file_async(
        host.clone(),
        local_tar.clone(),
        remote_tar.clone(),
        std::sync::Arc::clone(&progress),
        true, // overwrite: tar archive is a temporary file
        None,
    )
    .await?;

    // Step 3: Extract on remote (extract to parent because archive
    // contains the folder name as its top-level entry)
    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.current_file = "Extracting...".to_string();
    };
    let extract_target = std::path::Path::new(&remote_dir)
        .parent()
        .map_or_else(|| "/".to_string(), |p| p.to_string_lossy().to_string());
    remote_extract_tar(host.clone(), remote_tar.clone(), extract_target).await?;

    // Step 4: Clean up remote tar
    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.current_file = "Cleaning up...".to_string();
    };
    let _ = sftp_remove_file(host, remote_tar).await;

    // Step 5: Clean up local tar
    let _ = std::fs::remove_file(&local_tar);

    Ok(())
}

async fn upload_folder_recursive(
    host: Host,
    local_dir: PathBuf,
    remote_dir: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    overwrite: bool,
) -> Result<(), SftpTransferFailure> {
    let ((mut session, sftp), _) = connect_sftp(host)
        .await
        .map_err(SftpTransferFailure::connection)?;
    mark_transfer_connected(&progress);
    let root_permissions = local_unix_permissions(&local_dir)?;

    // Check if remote directory already exists
    if !overwrite
        && let Ok(metadata) = sftp.symlink_metadata(remote_dir.clone()).await
        && metadata.file_type() == SftpFileType::Dir
    {
        release_sftp(&mut session, sftp).await;
        return Err(SftpTransferFailure::FileExists {
            is_dir: true,
            remote_path: remote_dir,
            local_path: Some(local_dir.display().to_string()),
        });
    }

    sftp_create_dir_if_not_exists_with_permissions(&sftp, &remote_dir, root_permissions).await?;

    let result =
        upload_dir_recursive_impl(&sftp, &local_dir, &remote_dir, &local_dir, &progress).await;

    release_sftp(&mut session, sftp).await;
    result
}

async fn upload_dir_recursive_impl(
    sftp: &SftpSession,
    base_dir: &Path,
    remote_base: &str,
    current_local_dir: &Path,
    progress: &Arc<std::sync::Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let entries = std::fs::read_dir(current_local_dir).map_err(|e| {
        t!(
            "sftp.dir_read_failed",
            path = current_local_dir.display(),
            e = e
        )
        .to_string()
    })?;

    for entry in entries {
        if transfer_cancelled(progress) {
            return Err(SftpTransferFailure::Cancelled);
        }

        let entry = entry.map_err(|e| t!("sftp.entry_read_failed", e = e).to_string())?;
        let ft = entry
            .file_type()
            .map_err(|e| t!("sftp.filetype_failed", e = e).to_string())?;

        if ft.is_symlink() {
            continue;
        }

        let entry_path = entry.path();
        let relative = entry_path
            .strip_prefix(base_dir)
            .map_err(|e| format!("Failed to compute relative path: {e}"))?;
        let remote_path = if remote_base.ends_with('/') {
            format!("{remote_base}{}", relative.display())
        } else {
            format!("{remote_base}/{}", relative.display())
        };

        if ft.is_dir() {
            let dir_permissions = local_unix_permissions(&entry_path)?;
            sftp_create_dir_if_not_exists_with_permissions(sftp, &remote_path, dir_permissions)
                .await?;
            Box::pin(upload_dir_recursive_impl(
                sftp,
                base_dir,
                remote_base,
                &entry_path,
                progress,
            ))
            .await?;
        } else if ft.is_file() {
            let file_size = entry.metadata().map_or(0, |m| m.len());

            {
                let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
                p.current_file = relative.to_string_lossy().to_string();
            };

            upload_single_file_over_sftp(
                sftp,
                &entry.path(),
                &remote_path,
                file_size,
                progress,
                None,
                local_unix_permissions(&entry_path)?,
            )
            .await?;

            {
                let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
                p.files_completed += 1;
            }
        }
    }

    Ok(())
}

async fn upload_single_file_over_sftp(
    sftp: &SftpSession,
    local_path: &Path,
    remote_path: &str,
    _file_size: u64,
    progress: &Arc<std::sync::Mutex<SftpTransferProgress>>,
    resume_offset: Option<u64>,
    local_permissions: Option<u32>,
) -> Result<(), SftpTransferFailure> {
    let mut local_file = tokio::fs::File::open(local_path)
        .await
        .map_err(|e| format!("Failed to open local file: {e}"))?;

    let open_flags = if resume_offset.is_some() {
        OpenFlags::CREATE | OpenFlags::WRITE
    } else {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    };
    let mut remote_file = sftp
        .open_with_flags_and_attributes(
            remote_path.to_string(),
            open_flags,
            attrs_with_permissions(local_permissions),
        )
        .await
        .map_err(|e| format_sftp_open_error(&e, remote_path))?;

    let transferred_before = progress.lock().map_or(0, |p| p.transferred_bytes);

    match pump_transfer_stream(
        &mut local_file,
        &mut remote_file,
        progress,
        transferred_before,
        resume_offset,
        "local file",
        "remote file",
        TransferSide::Local,
        TransferSide::Remote,
    )
    .await
    {
        Ok(_) => {}
        Err(error) => {
            shutdown_sftp_file(&mut remote_file).await;
            return Err(error);
        }
    }

    remote_file
        .shutdown()
        .await
        .map_err(|e| format!("Failed to close remote file: {e}"))?;

    if transfer_cancelled(progress) {
        Err(SftpTransferFailure::Cancelled)
    } else {
        apply_remote_permissions(sftp, remote_path, local_permissions).await?;
        Ok(())
    }
}

async fn sftp_create_dir_if_not_exists(sftp: &SftpSession, path: &str) -> Result<(), String> {
    if let Ok(metadata) = sftp.symlink_metadata(path.to_string()).await {
        if metadata.file_type() == SftpFileType::Dir {
            return Ok(());
        }
        return Err(t!("sftp.not_a_directory", path = &path).to_string());
    }
    sftp.create_dir(path.to_string())
        .await
        .map_err(|e| format!("Failed to create remote directory {path}: {e}"))
}

async fn sftp_create_dir_if_not_exists_with_permissions(
    sftp: &SftpSession,
    path: &str,
    permissions: Option<u32>,
) -> Result<(), String> {
    sftp_create_dir_if_not_exists(sftp, path).await?;
    apply_remote_permissions(sftp, path, permissions).await
}

async fn remote_has_tar(host: &Host) -> bool {
    let mut logs = Vec::new();
    let Ok(session) = connect_authenticated_session(host, &mut logs).await else {
        return false;
    };
    let Ok(channel) = session.channel_open_session().await else {
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await;
        return false;
    };
    if channel.exec(true, b"tar --version").await.is_err() {
        let _ = session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await;
        return false;
    }
    let mut stream = channel.into_stream();
    let mut output = Vec::new();
    // Any output means tar exists; empty or error means it doesn't
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut output)).await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;
    !output.is_empty()
}

pub async fn upload_sftp_folder(
    host: Host,
    local_path: PathBuf,
    remote_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
    overwrite: bool,
) -> Result<(), SftpTransferFailure> {
    let (file_count, total_size) = crate::core::data::count_files_recursive(&local_path)?;

    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.total_files = file_count;
    };

    run_sftp_task(move || {
        let host = host.clone();
        let local_path = local_path.clone();
        let remote_path = remote_path.clone();
        let progress = std::sync::Arc::clone(&progress);
        async move {
            let use_archive =
                file_count >= 5 && total_size < 60 * 1024 * 1024 && remote_has_tar(&host).await;

            if use_archive {
                upload_folder_via_archive(host, local_path, remote_path, progress, overwrite).await
            } else {
                upload_folder_recursive(host, local_path, remote_path, progress, overwrite).await
            }
        }
    })
    .await
}

/// Atomic folder upload (safe replace): the whole tree is first staged under a
/// unique temp sibling directory and only swapped into place once the upload
/// has fully succeeded. A failed or interrupted upload therefore never destroys
/// the original remote folder — the partial staging tree is cleaned up instead.
///
/// This is the folder counterpart of [`upload_sftp_file_atomic`] and is used
/// for overwrites smaller than [`SFTP_ATOMIC_SIZE_THRESHOLD`]; larger folders
/// keep the delete-then-upload strategy to avoid temporary disk pressure on
/// the remote.
pub async fn upload_sftp_folder_atomic(
    host: Host,
    local_path: PathBuf,
    remote_path: String,
    progress: Arc<std::sync::Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let (file_count, total_size) = crate::core::data::count_files_recursive(&local_path)?;
    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.total_files = file_count;
        p.total_bytes = total_size;
    };

    run_sftp_task(move || {
        let host = host.clone();
        let local_path = local_path.clone();
        let remote_path = remote_path.clone();
        let progress = std::sync::Arc::clone(&progress);
        async move {
            let use_archive =
                file_count >= 5 && total_size < 60 * 1024 * 1024 && remote_has_tar(&host).await;
            let (parent, name) = remote_parent_and_name(&remote_path);
            // Wrapper directory next to the target so the staged data sits on the
            // same filesystem (rename would otherwise fail across devices).
            let staging = format!("{parent}/.termlet_tmp_{}", uuid::Uuid::new_v4());

            // Stage under <staging>/<name>: both upload strategies reproduce the
            // folder's own name, so the final swap is a pure server-side rename.
            let staged_target = format!("{staging}/{name}");
            if let Err(e) = sftp_create_remote_dir(host.clone(), staging.clone()).await {
                return Err(SftpTransferFailure::Failed {
                    message: format!("Failed to create staging folder: {e}"),
                });
            }

            let upload_result = if use_archive {
                upload_folder_via_archive(
                    host.clone(),
                    local_path,
                    staged_target.clone(),
                    std::sync::Arc::clone(&progress),
                    /* overwrite = */ true,
                )
                .await
            } else {
                upload_folder_recursive(
                    host.clone(),
                    local_path,
                    staged_target.clone(),
                    std::sync::Arc::clone(&progress),
                    /* overwrite = */ true,
                )
                .await
            };

            match upload_result {
                Ok(()) => {
                    // SFTP v3 rename (SSH_FXP_RENAME) cannot overwrite an existing
                    // non-empty directory, so remove the original before swapping
                    // the staged folder into place.
                    if let Err(e) =
                        sftp_remove_dir_recursive(host.clone(), remote_path.clone()).await
                    {
                        let _ = sftp_remove_dir_recursive(host.clone(), staging.clone()).await;
                        return Err(SftpTransferFailure::Failed {
                            message: format!("Failed to remove original folder before swap: {e}"),
                        });
                    }
                    if let Err(e) =
                        sftp_rename_file(host.clone(), staged_target.clone(), remote_path.clone())
                            .await
                    {
                        // Upload already succeeded and the original is gone; keep
                        // the staged tree so the user can recover manually.
                        return Err(SftpTransferFailure::Failed {
                            message: format!(
                                "Folder upload succeeded but swap failed \
                                 (data preserved at {staged_target}): {e}"
                            ),
                        });
                    }
                    // The wrapper is empty after the staged folder moved out; drop
                    // it best-effort.
                    let _ = sftp_remove_dir_recursive(host, staging).await;
                    Ok(())
                }
                Err(e) => {
                    // Upload (or cancellation) failed before touching the original —
                    // clean up the partial staging tree.
                    let _ = sftp_remove_dir_recursive(host, staging).await;
                    Err(e)
                }
            }
        }
    })
    .await
}

async fn count_remote_files_recursive(
    host: Host,
    remote_path: String,
    progress: &Arc<Mutex<SftpTransferProgress>>,
) -> Result<(u64, u64), String> {
    let ((mut session, sftp), _) = connect_sftp(host).await?;
    mark_transfer_connected(progress);
    let result = count_remote_dir_recursive_impl(&sftp, &remote_path).await;
    release_sftp(&mut session, sftp).await;
    result
}

async fn count_remote_dir_recursive_impl(
    sftp: &SftpSession,
    path: &str,
) -> Result<(u64, u64), String> {
    let mut file_count = 0u64;
    let mut total_size = 0u64;

    let entries = sftp
        .read_dir(path.to_string())
        .await
        .map_err(|e| format!("Failed to read remote directory {path}: {e}"))?;

    for entry in entries {
        let metadata = entry.metadata();
        let ft = metadata.file_type();
        if matches!(ft, SftpFileType::Symlink) {
            continue;
        }
        if matches!(ft, SftpFileType::Dir) {
            let sub_path = if path.ends_with('/') {
                format!("{path}{}", entry.file_name())
            } else {
                format!("{path}/{}", entry.file_name())
            };
            let (sub_files, sub_size) =
                Box::pin(count_remote_dir_recursive_impl(sftp, &sub_path)).await?;
            file_count += sub_files;
            total_size += sub_size;
        } else {
            file_count += 1;
            total_size += metadata.len();
        }
    }

    Ok((file_count, total_size))
}

async fn remote_create_tar(host: Host, remote_dir: String, tar_path: String) -> Result<(), String> {
    let dir_name = Path::new(&remote_dir).file_name().map_or_else(
        || "download".to_string(),
        |n| n.to_string_lossy().to_string(),
    );
    let parent = Path::new(&remote_dir)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("/");

    let command = format!(
        "cd {} && tar czf {} {}",
        shell_escape(parent),
        shell_escape(&tar_path),
        shell_escape(&dir_name),
    );
    ssh_exec_command(host.clone(), command).await?;

    let ((mut session, sftp), _) = connect_sftp(host).await?;
    let exists = sftp.try_exists(tar_path.clone()).await.unwrap_or(false);
    release_sftp(&mut session, sftp).await;
    if !exists {
        return Err("Failed to create remote tar archive".to_string());
    }
    Ok(())
}

fn local_extract_tar(tar_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let _guard = crate::core::diagnostics::perf_guard_dyn(
        Level::Debug,
        "IO",
        format!("local_extract_tar({})", tar_path.display()),
        std::time::Duration::from_millis(500),
    );
    let tar_file =
        std::fs::File::open(tar_path).map_err(|e| format!("Failed to open tar file: {e}"))?;
    let decoder = GzDecoder::new(tar_file);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(dest_dir)
        .map_err(|e| format!("Failed to extract tar archive: {e}"))?;
    Ok(())
}

async fn remote_remove_tar(host: Host, tar_path: String) {
    let _ = sftp_remove_file(host, tar_path).await;
}

async fn download_folder_via_archive(
    host: Host,
    remote_dir: String,
    local_dir: PathBuf,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let archive_name = format!("termlet_download_{timestamp}.tar.gz");
    let remote_tar = format!("/tmp/{archive_name}");
    let local_tar = std::env::temp_dir().join(&archive_name);

    // Step 1: Create tar.gz on remote
    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.current_file = "Compressing on remote...".to_string();
    };
    remote_create_tar(host.clone(), remote_dir.clone(), remote_tar.clone()).await?;

    // Step 2: Get archive size for progress tracking
    let ((mut session, sftp), _) = connect_sftp(host.clone())
        .await
        .map_err(SftpTransferFailure::connection)?;
    mark_transfer_connected(&progress);
    let archive_size = sftp
        .symlink_metadata(remote_tar.clone())
        .await
        .map_or(0, |m| m.len());
    release_sftp(&mut session, sftp).await;

    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.total_bytes = archive_size;
        p.file_name.clone_from(&archive_name);
    };

    // Step 3: Download the archive
    download_sftp_file_async(
        host.clone(),
        remote_tar.clone(),
        local_tar.clone(),
        std::sync::Arc::clone(&progress),
        None,
    )
    .await?;

    // Step 4: Clean up remote tar
    remote_remove_tar(host.clone(), remote_tar).await;

    // Step 5: Extract locally
    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.current_file = "Extracting...".to_string();
    };
    let parent_dir = local_dir.parent().unwrap_or_else(|| Path::new("/"));
    local_extract_tar(&local_tar, parent_dir)?;

    // Step 6: Clean up local tar
    let _ = std::fs::remove_file(&local_tar);

    Ok(())
}

async fn download_folder_recursive(
    host: Host,
    remote_dir: String,
    local_dir: PathBuf,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let ((mut session, sftp), _) = connect_sftp(host)
        .await
        .map_err(SftpTransferFailure::connection)?;
    mark_transfer_connected(&progress);

    std::fs::create_dir_all(&local_dir)
        .map_err(|e| format!("Failed to create local directory: {e}"))?;

    let result = download_dir_recursive_impl(&sftp, &remote_dir, &local_dir, &progress).await;

    release_sftp(&mut session, sftp).await;
    result
}

async fn download_dir_recursive_impl(
    sftp: &SftpSession,
    remote_dir: &str,
    local_dir: &Path,
    progress: &Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let entries = sftp
        .read_dir(remote_dir.to_string())
        .await
        .map_err(|e| format!("Failed to read remote directory {remote_dir}: {e}"))?;

    for entry in entries {
        if transfer_cancelled(progress) {
            return Err(SftpTransferFailure::Cancelled);
        }

        let metadata = entry.metadata();
        let ft = metadata.file_type();
        if matches!(ft, SftpFileType::Symlink) {
            continue;
        }

        let entry_name = entry.file_name();
        let remote_path = if remote_dir.ends_with('/') {
            format!("{remote_dir}{entry_name}")
        } else {
            format!("{remote_dir}/{entry_name}")
        };
        let local_path = local_dir.join(&entry_name);

        if matches!(ft, SftpFileType::Dir) {
            std::fs::create_dir_all(&local_path).map_err(|e| {
                format!(
                    "Failed to create directory {path}: {e}",
                    path = local_path.display(),
                    e = e
                )
            })?;
            Box::pin(download_dir_recursive_impl(
                sftp,
                &remote_path,
                &local_path,
                progress,
            ))
            .await?;
        } else {
            let file_size = metadata.len();

            {
                let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
                p.current_file.clone_from(&entry_name);
            };

            download_single_file_over_sftp(sftp, &remote_path, &local_path, file_size, progress)
                .await?;

            {
                let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
                p.files_completed += 1;
            }
        }
    }

    Ok(())
}

async fn download_single_file_over_sftp(
    sftp: &SftpSession,
    remote_path: &str,
    local_path: &Path,
    file_size: u64,
    progress: &Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let mut remote_file = sftp
        .open(remote_path.to_string())
        .await
        .map_err(|e| format!("Failed to open remote file {remote_path}: {e}"))?;

    let mut local_file = tokio::fs::File::create(local_path).await.map_err(|e| {
        format!(
            "Failed to create local file {path}: {e}",
            path = local_path.display(),
            e = e
        )
    })?;

    let transferred_before = progress.lock().map_or(0, |p| p.transferred_bytes);

    let transferred = match pump_transfer_stream(
        &mut remote_file,
        &mut local_file,
        progress,
        transferred_before,
        None,
        "remote file",
        "local file",
        TransferSide::Remote,
        TransferSide::Local,
    )
    .await
    {
        Ok(transferred) => transferred,
        Err(error) => {
            shutdown_sftp_file(&mut remote_file).await;
            return Err(error);
        }
    };
    let file_transferred = transferred.saturating_sub(transferred_before);

    shutdown_sftp_file(&mut remote_file).await;

    if transfer_cancelled(progress) {
        Err(SftpTransferFailure::Cancelled)
    } else if file_transferred != file_size {
        Err(SftpTransferFailure::connection(
            t!(
                "sftp.transfer_download_incomplete",
                received = file_transferred,
                total = file_size,
                path = &remote_path
            )
            .to_string(),
        ))
    } else {
        Ok(())
    }
}

pub async fn download_sftp_folder(
    host: Host,
    remote_path: String,
    local_path: PathBuf,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let (file_count, total_size) =
        count_remote_files_recursive(host.clone(), remote_path.clone(), &progress).await?;

    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.total_files = file_count;
        p.total_bytes = total_size;
    };

    run_sftp_task(move || {
        let host = host.clone();
        let remote_path = remote_path.clone();
        let local_path = local_path.clone();
        let progress = std::sync::Arc::clone(&progress);
        async move {
            let use_archive =
                file_count > 20 && total_size < 60 * 1024 * 1024 && remote_has_tar(&host).await;

            if use_archive {
                download_folder_via_archive(host, remote_path, local_path, progress).await
            } else {
                download_folder_recursive(host, remote_path, local_path, progress).await
            }
        }
    })
    .await
}

/// Copy a folder from one remote host to another via SFTP.
///
/// Opens two SFTP sessions, counts files on the source, creates the
/// destination root directory, then recursively copies every file.
pub async fn copy_sftp_folder_between_hosts(
    source_host: Host,
    dest_host: Host,
    source_path: String,
    dest_path: String,
    progress: Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    // Count files recursively on source for progress tracking
    let (file_count, total_size) =
        count_remote_files_recursive(source_host.clone(), source_path.clone(), &progress).await?;

    {
        let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
        p.total_files = file_count;
        p.total_bytes = total_size;
    };

    run_sftp_task(move || {
        let source_host = source_host.clone();
        let dest_host = dest_host.clone();
        let source_path = source_path.clone();
        let dest_path = dest_path.clone();
        let progress = std::sync::Arc::clone(&progress);
        async move {
            let ((mut source_session, source_sftp), _) = connect_sftp(source_host)
                .await
                .map_err(SftpTransferFailure::connection)?;
            let ((mut dest_session, dest_sftp), _) = connect_sftp(dest_host)
                .await
                .map_err(SftpTransferFailure::connection)?;
            mark_transfer_connected(&progress);

            // Create the destination root directory (idempotent)
            sftp_create_dir_if_not_exists_with_permissions(&dest_sftp, &dest_path, None).await?;

            let result = copy_dir_between_hosts_recursive_impl(
                &source_sftp,
                &dest_sftp,
                &source_path,
                &dest_path,
                &progress,
            )
            .await;

            release_sftp(&mut source_session, source_sftp).await;
            release_sftp(&mut dest_session, dest_sftp).await;
            result
        }
    })
    .await
}

/// Recursively copy a directory tree between two already-connected SFTP sessions.
async fn copy_dir_between_hosts_recursive_impl(
    source_sftp: &SftpSession,
    dest_sftp: &SftpSession,
    source_dir: &str,
    dest_dir: &str,
    progress: &Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let entries = source_sftp
        .read_dir(source_dir.to_string())
        .await
        .map_err(|e| format!("Failed to read remote directory {source_dir}: {e}"))?;

    for entry in entries {
        if transfer_cancelled(progress) {
            return Err(SftpTransferFailure::Cancelled);
        }

        let metadata = entry.metadata();
        let ft = metadata.file_type();
        if matches!(ft, SftpFileType::Symlink) {
            continue;
        }

        let entry_name = entry.file_name();
        let source_subpath = if source_dir.ends_with('/') {
            format!("{source_dir}{entry_name}")
        } else {
            format!("{source_dir}/{entry_name}")
        };
        let dest_subpath = if dest_dir.ends_with('/') {
            format!("{dest_dir}{entry_name}")
        } else {
            format!("{dest_dir}/{entry_name}")
        };

        if matches!(ft, SftpFileType::Dir) {
            sftp_create_dir_if_not_exists_with_permissions(dest_sftp, &dest_subpath, None).await?;
            Box::pin(copy_dir_between_hosts_recursive_impl(
                source_sftp,
                dest_sftp,
                &source_subpath,
                &dest_subpath,
                progress,
            ))
            .await?;
        } else {
            {
                let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
                p.current_file.clone_from(&entry_name);
            };

            copy_single_file_between_sftp_sessions(
                source_sftp,
                dest_sftp,
                &source_subpath,
                &dest_subpath,
                progress,
            )
            .await?;

            {
                let mut p = progress.lock().map_err(|e| format!("Lock error: {e}"))?;
                p.files_completed += 1;
            }
        }
    }

    Ok(())
}

/// Copy a single file between two already-connected SFTP sessions.
///
/// Reads chunks from the source SFTP file and writes them to the
/// destination SFTP file in a single hop (no local buffering to disk).
async fn copy_single_file_between_sftp_sessions(
    source_sftp: &SftpSession,
    dest_sftp: &SftpSession,
    source_path: &str,
    dest_path: &str,
    progress: &Arc<Mutex<SftpTransferProgress>>,
) -> Result<(), SftpTransferFailure> {
    let mut source_file = source_sftp
        .open(source_path.to_string())
        .await
        .map_err(|e| format!("Failed to open source file {source_path}: {e}"))?;

    let mut dest_file = dest_sftp
        .open_with_flags(
            dest_path.to_string(),
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .map_err(|e| format!("Failed to create dest file {dest_path}: {e}"))?;

    let transferred_before = progress.lock().map_or(0, |p| p.transferred_bytes);

    match pump_transfer_stream(
        &mut source_file,
        &mut dest_file,
        progress,
        transferred_before,
        None,
        "source file",
        "destination file",
        TransferSide::Remote,
        TransferSide::Remote,
    )
    .await
    {
        Ok(_) => {}
        Err(error) => {
            shutdown_sftp_file(&mut source_file).await;
            shutdown_sftp_file(&mut dest_file).await;
            return Err(error);
        }
    }

    shutdown_sftp_file(&mut source_file).await;
    dest_file
        .shutdown()
        .await
        .map_err(|e| format!("Failed to close dest file {dest_path}: {e}"))?;

    Ok(())
}

#[cfg(test)]
mod checksum_tests {
    use super::*;

    #[test]
    fn marking_connected_keeps_the_original_start_time() {
        let progress = Arc::new(Mutex::new(SftpTransferProgress {
            file_name: "empty.txt".into(),
            direction: crate::sftp::SftpTransferDirection::Upload,
            transferred_bytes: 0,
            total_bytes: 0,
            cancelled: false,
            current_file: "empty.txt".into(),
            files_completed: 0,
            total_files: 1,
            resumed: false,
            start_time: None,
            speed: 0.0,
            last_speed_update: None,
        }));

        mark_transfer_connected(&progress);
        let first_start = progress.lock().unwrap().start_time;
        mark_transfer_connected(&progress);

        assert!(first_start.is_some());
        assert_eq!(progress.lock().unwrap().start_time, first_start);
    }

    #[tokio::test]
    async fn local_sha256_matches_known_digest() {
        let dir =
            std::env::temp_dir().join(format!("termlet-checksum-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.bin");
        std::fs::write(&path, b"termlet checksum").unwrap();

        let digest = local_sha256(&path).await.unwrap();

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);

        assert_eq!(digest.len(), 64);
        assert_eq!(digest, to_hex(&sha2::Sha256::digest(b"termlet checksum")));
    }

    #[test]
    fn compare_hashes_reports_match_and_mismatch() {
        let digest = "a".repeat(64);
        assert_eq!(
            compare_hashes(&digest, &digest).unwrap(),
            TransferChecksum::Verified(digest.clone())
        );
        compare_hashes(&digest, &"b".repeat(64)).unwrap_err();
    }
}
