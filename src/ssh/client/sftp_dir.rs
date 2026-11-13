use russh_sftp::client::SftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::Metadata as SftpMetadata;
use russh_sftp::protocol::{FileType as SftpFileType, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::core::data::{
    FileEntry, FileEntryType, Host, PREVIEW_MAX_FILE_SIZE, TEXT_PREVIEW_MAX_BYTES,
    TextPreviewContent, UNKNOWN_PERMISSIONS, finalize_text_preview, format_size, sort_file_entries,
};
use crate::core::time::format_system_time;
use rust_i18n::t;

use super::exec::ssh_exec_command;
use super::path_utils::shell_escape;
use super::session::{
    SFTP_REQUEST_TIMEOUT, connect_sftp, release_sftp, run_sftp_task, sftp_runtime,
};

pub async fn list_sftp_dir(
    host: Host,
    path: String,
    show_hidden: bool,
) -> Result<(String, Vec<FileEntry>, Vec<String>), String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), logs) = connect_sftp(host).await?;

        // Expand `~` to the authenticated user's remote home directory so the
        // caller can display/navigate a real absolute path, not a literal `~`.
        let resolved_path = expand_remote_home_prefix(&sftp, &path).await?;

        let mut entries =
            tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.read_dir(resolved_path.clone()))
                .await
                .map_err(|_error| "Timed out reading remote directory".to_string())?
                .map_err(|error| {
                    format!("Failed to read remote directory {resolved_path}: {error}")
                })?
                .map(|entry| file_entry_from_sftp(entry.file_name(), &entry.metadata()))
                .filter(|entry| show_hidden || !entry.name.starts_with('.'))
                .collect::<Vec<_>>();

        sort_file_entries(&mut entries);
        release_sftp(&mut session, sftp).await;
        Ok((resolved_path, entries, logs))
    })
    .await
}

/// Expand a leading `~` in a remote path against the authenticated user's home
/// directory, resolved via the SFTP server (`canonicalize(".")`).
///
/// This must round-trip through the server — a local crate like `shellexpand`
/// would expand to the *local* home, which is wrong for a remote host. Absolute
/// paths and relative paths are returned unchanged.
async fn expand_remote_home_prefix(sftp: &SftpSession, path: &str) -> Result<String, String> {
    let resolve_home = || async {
        sftp.canonicalize(".")
            .await
            .map_err(|e| format!("Failed to resolve home directory: {e}"))
    };
    match crate::core::paths::classify_tilde(path) {
        crate::core::paths::TildeForm::Bare => resolve_home().await,
        crate::core::paths::TildeForm::Subpath(rest) => {
            let home = resolve_home().await?;
            Ok(format!("{home}/{rest}"))
        }
        crate::core::paths::TildeForm::None => Ok(path.to_string()),
    }
}

/// Resolve the authenticated user's real home directory via SFTP canonicalize(".")
/// and list its contents in a single connection.
pub async fn resolve_and_list_default_sftp_dir(
    host: Host,
    show_hidden: bool,
) -> Result<(String, Vec<FileEntry>, Vec<String>), String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), logs) = connect_sftp(host).await?;
        let home_path = tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.canonicalize("."))
            .await
            .map_err(|_error| "Timed out resolving home directory".to_string())?
            .map_err(|error| format!("Failed to resolve home directory: {error}"))?;
        let mut entries = tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.read_dir(&home_path))
            .await
            .map_err(|_error| "Timed out reading remote directory".to_string())?
            .map_err(|error| format!("Failed to read remote directory {home_path}: {error}"))?
            .map(|entry| file_entry_from_sftp(entry.file_name(), &entry.metadata()))
            .filter(|entry| show_hidden || !entry.name.starts_with('.'))
            .collect::<Vec<_>>();

        sort_file_entries(&mut entries);
        release_sftp(&mut session, sftp).await;
        Ok((home_path, entries, logs))
    })
    .await
}

pub async fn sftp_file_entry(
    host: Host,
    path: String,
    name: String,
) -> Result<Option<FileEntry>, String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), _) = connect_sftp(host).await?;
        let result = match tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.symlink_metadata(path))
            .await
            .map_err(|_error| "Timed out looking up remote file".to_string())?
        {
            Ok(metadata) => Ok(Some(file_entry_from_sftp(name, &metadata))),
            Err(error) if sftp_not_found(&error) => Ok(None),
            Err(error) => Err(format!("Failed to look up remote file: {error}")),
        };
        release_sftp(&mut session, sftp).await;
        result
    })
    .await
}

pub async fn sftp_remove_file(host: Host, remote_path: String) -> Result<(), String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), _) = connect_sftp(host).await?;
        let result =
            match tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.remove_file(remote_path.clone()))
                .await
            {
                Ok(Ok(())) => Ok(()),
                Ok(Err(err)) => Err(format!("Failed to delete file: {err}")),
                Err(_) => Err("Timed out deleting file".to_string()),
            };
        release_sftp(&mut session, sftp).await;
        result
    })
    .await
}

/// Check if a remote path (file or directory) exists.
pub async fn remote_path_exists(host: Host, remote_path: String) -> Result<bool, String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), _) = connect_sftp(host).await?;
        let result = match tokio::time::timeout(
            SFTP_REQUEST_TIMEOUT,
            sftp.symlink_metadata(remote_path.clone()),
        )
        .await
        {
            Ok(Ok(_)) => Ok(true),
            Ok(Err(_)) => Ok(false),
            Err(_) => Ok(false),
        };
        release_sftp(&mut session, sftp).await;
        result
    })
    .await
}

pub async fn sftp_rename_file(
    host: Host,
    old_path: String,
    new_path: String,
) -> Result<(), String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), _) = connect_sftp(host).await?;
        let result = match tokio::time::timeout(
            SFTP_REQUEST_TIMEOUT,
            sftp.rename(old_path.clone(), new_path.clone()),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(format!("Failed to rename file: {err}")),
            Err(_) => Err("Timed out renaming file".to_string()),
        };
        release_sftp(&mut session, sftp).await;
        result
    })
    .await
}

pub async fn sftp_create_remote_dir(host: Host, remote_path: String) -> Result<(), String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), _) = connect_sftp(host).await?;
        let result =
            match tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.create_dir(remote_path.clone()))
                .await
            {
                Ok(Ok(())) => Ok(()),
                Ok(Err(err)) => Err(format!("Failed to create folder: {err}")),
                Err(_) => Err("Timed out creating folder".to_string()),
            };
        release_sftp(&mut session, sftp).await;
        result
    })
    .await
}

pub async fn sftp_remove_dir_recursive(host: Host, remote_path: String) -> Result<(), String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), _) = connect_sftp(host).await?;
        let result = remove_remote_path(&sftp, remote_path).await;
        release_sftp(&mut session, sftp).await;
        result
    })
    .await
}

pub(crate) async fn try_rm_rf_fast(host: Host, remote_path: String) -> bool {
    tokio::task::spawn_blocking(move || {
        let Ok(runtime) = sftp_runtime() else {
            return false;
        };
        let escaped = shell_escape(&remote_path);
        runtime
            .block_on(ssh_exec_command(host, format!("rm -rf {escaped}")))
            .is_ok()
    })
    .await
    .unwrap_or(false)
}

async fn remove_remote_path(sftp: &SftpSession, remote_path: String) -> Result<(), String> {
    let metadata = match tokio::time::timeout(
        SFTP_REQUEST_TIMEOUT,
        sftp.symlink_metadata(remote_path.clone()),
    )
    .await
    .map_err(|_error| "Timed out inspecting remote path".to_string())?
    {
        Ok(metadata) => metadata,
        Err(err) => return Err(format!("Failed to inspect remote path: {err}")),
    };

    if metadata.file_type() == SftpFileType::Dir {
        remove_remote_dir_contents(sftp, remote_path).await
    } else {
        match tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.remove_file(remote_path.clone()))
            .await
            .map_err(|_error| "Timed out removing file".to_string())?
        {
            Ok(()) => Ok(()),
            Err(err) => Err(format!("Failed to remove file: {err}")),
        }
    }
}

/// Recursively delete a directory. Uses metadata from `read_dir` to avoid redundant
/// `symlink_metadata` calls for every child entry — each skipped call saves one
/// network round-trip.
async fn remove_remote_dir_contents(sftp: &SftpSession, remote_path: String) -> Result<(), String> {
    let entries = tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.read_dir(remote_path.clone()))
        .await
        .map_err(|_error| "Timed out reading remote directory".to_string())?
        .map_err(|err| format!("Failed to read remote directory {remote_path}: {err}"))?;

    // Collect subdirectories to recurse into after removing files
    let mut subdirs: Vec<String> = Vec::new();

    for entry in entries {
        let name = entry.file_name();
        let child = if remote_path == "/" {
            format!("/{name}")
        } else if remote_path.ends_with('/') {
            format!("{remote_path}{name}")
        } else {
            format!("{remote_path}/{name}")
        };

        // Use metadata already available from read_dir — no extra network call needed.
        if entry.metadata().file_type() == SftpFileType::Dir {
            subdirs.push(child);
        } else {
            match tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.remove_file(child.clone()))
                .await
                .map_err(|_error| format!("Timed out removing {child}"))?
            {
                Ok(()) => {}
                Err(err) => {
                    return Err(format!("Failed to remove {child}: {err}"));
                }
            }
        }
    }

    // Recurse into subdirectories after files are removed
    for subdir in subdirs {
        Box::pin(remove_remote_dir_contents(sftp, subdir)).await?;
    }

    match tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.remove_dir(remote_path.clone()))
        .await
        .map_err(|_error| "Timed out removing directory".to_string())?
    {
        Ok(()) => Ok(()),
        Err(err) => Err(format!("Failed to remove directory: {err}")),
    }
}

pub async fn sftp_read_text_preview(
    host: Host,
    remote_path: String,
) -> Result<TextPreviewContent, String> {
    run_sftp_task(move || async move {
        let ((mut session, sftp), _) = connect_sftp(host).await?;

        let total_bytes =
            tokio::time::timeout(SFTP_REQUEST_TIMEOUT, sftp.metadata(remote_path.clone()))
                .await
                .map_err(|_error| "Timed out reading remote file metadata".to_string())?
                .ok()
                .and_then(|meta| meta.size);

        if let Some(size) = total_bytes.filter(|s| *s > PREVIEW_MAX_FILE_SIZE) {
            return Err(t!(
                "sftp.file_too_large",
                size = format_size(size),
                limit = format_size(PREVIEW_MAX_FILE_SIZE),
            )
            .to_string());
        }

        let mut remote_file = match tokio::time::timeout(
            SFTP_REQUEST_TIMEOUT,
            sftp.open(remote_path.clone()),
        )
        .await
        {
            Ok(Ok(file)) => file,
            Ok(Err(err)) => {
                release_sftp(&mut session, sftp).await;
                return Err(format!("Failed to open remote file: {err}"));
            }
            Err(_) => {
                release_sftp(&mut session, sftp).await;
                return Err("Timed out opening remote file".to_string());
            }
        };

        let mut buffer: Vec<u8> = Vec::new();
        let cap = TEXT_PREVIEW_MAX_BYTES as usize + 1;
        let mut chunk = vec![0_u8; 64 * 1024].into_boxed_slice();
        let mut truncated = false;
        loop {
            let read = match remote_file.read(&mut chunk).await {
                Ok(n) => n,
                Err(err) => {
                    let _ = remote_file.shutdown().await;
                    release_sftp(&mut session, sftp).await;
                    return Err(format!("Failed to read remote file: {err}"));
                }
            };
            if read == 0 {
                break;
            }
            let remaining = cap.saturating_sub(buffer.len());
            if remaining == 0 {
                truncated = true;
                break;
            }
            let take = read.min(remaining);
            buffer.extend_from_slice(&chunk[..take]);
            if buffer.len() > TEXT_PREVIEW_MAX_BYTES as usize {
                truncated = true;
                break;
            }
        }

        let _ = remote_file.shutdown().await;
        release_sftp(&mut session, sftp).await;

        if buffer.len() as u64 > TEXT_PREVIEW_MAX_BYTES {
            buffer.truncate(TEXT_PREVIEW_MAX_BYTES as usize);
            truncated = true;
        }

        finalize_text_preview(buffer, truncated, total_bytes)
    })
    .await
}

fn file_entry_from_sftp(name: String, metadata: &SftpMetadata) -> FileEntry {
    let file_type = match metadata.file_type() {
        SftpFileType::Dir => FileEntryType::Folder,
        SftpFileType::Symlink => FileEntryType::Symlink,
        SftpFileType::File => FileEntryType::File,
        SftpFileType::Other => FileEntryType::File,
    };
    let modified_epoch = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let modified = metadata
        .modified()
        .ok()
        .map_or_else(|| "—".to_string(), format_system_time);
    let permissions = format_sftp_permissions(metadata);
    let kind = match file_type {
        FileEntryType::Folder => "Folder".to_string(),
        FileEntryType::Symlink => "Symlink".to_string(),
        FileEntryType::File => std::path::Path::new(&name).extension().map_or_else(
            || "File".to_string(),
            |extension| format!("{} File", extension.to_string_lossy().to_uppercase()),
        ),
    };

    FileEntry {
        name,
        file_type: file_type.clone(),
        size: if file_type == FileEntryType::Folder {
            None
        } else {
            metadata.size
        },
        modified,
        modified_epoch,
        permissions,
        kind,
    }
}

fn format_sftp_permissions(metadata: &SftpMetadata) -> String {
    if metadata.permissions.is_none() {
        return UNKNOWN_PERMISSIONS.to_string();
    }

    let ft = match metadata.file_type() {
        SftpFileType::Dir => 'd',
        SftpFileType::Symlink => 'l',
        _ => '-',
    };

    format!("{}{}", ft, metadata.permissions())
}

fn sftp_not_found(error: &SftpError) -> bool {
    matches!(
        error,
        SftpError::Status(status) if status.status_code == StatusCode::NoSuchFile
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(permissions: Option<u32>) -> SftpMetadata {
        SftpMetadata {
            size: Some(1),
            uid: None,
            user: None,
            gid: None,
            group: None,
            permissions,
            atime: None,
            mtime: None,
        }
    }

    #[test]
    fn a_missing_remote_mode_is_reported_as_unknown() {
        assert_eq!(
            format_sftp_permissions(&metadata(None)),
            UNKNOWN_PERMISSIONS
        );
    }

    #[test]
    fn a_remote_mode_still_uses_the_unix_style_summary() {
        assert_eq!(
            format_sftp_permissions(&metadata(Some(0o100_644))),
            "-rw-r--r--"
        );
    }
}
