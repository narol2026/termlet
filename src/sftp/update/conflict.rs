use iced::Task;
use std::sync::{Arc, Mutex};

use crate::app::App;
use crate::core::message::Message;
use crate::sftp::message::Message as DomainMessage;
use crate::sftp::{
    OverwriteApplyAll, SftpTransferDirection, SftpTransferFailure, SftpTransferProgress,
    SftpTransferTarget,
};
use rust_i18n::t;

use super::file_ops::resolve_duplicate_name;
use super::transfer::{
    DownloadStart, TransferKind, initiate_file_download, initiate_folder_download,
    resolve_duplicate_local_name, start_next_queued_transfer, transfer_file, transfer_folder,
    transfer_folder_atomic, transfer_folder_remote_to_remote,
};

pub(super) fn update(app: &mut App, msg: &DomainMessage) -> Task<Message> {
    let session_id = app.sftp.active_session;
    match msg {
        DomainMessage::SftpOverwriteCancel => {
            if let Some(pending) = app.sftp.state.overwrite_pending()
                && pending.in_progress
            {
                return Task::none();
            }
            app.sftp.state.clear_transfer_modal();
            start_next_queued_transfer(app)
        }
        DomainMessage::SftpOverwriteReplace => {
            let Some(pending) = app.sftp.state.overwrite_pending_mut() else {
                return Task::none();
            };
            if pending.in_progress {
                return Task::none();
            }
            pending.in_progress = true;
            let host = pending.host.clone();
            let file_name = pending.file_name.clone();
            let remote_path = pending.remote_path.clone();
            let total_bytes = pending.total_bytes;
            let target = pending.target.clone();
            let local_path = pending.local_path.clone();
            let source_host = pending.source_host.clone();
            let source_path = pending.source_path.clone();
            let is_folder = pending.is_folder;

            // Download conflict (Local target): delete local, then download
            if matches!(target, SftpTransferTarget::Local { .. }) {
                app.sftp.state.clear_transfer_modal();
                if let Some(lp) = &local_path {
                    app.sftp.state.transfer_status =
                        Some(t!("sftp.replacing", name = &file_name).to_string());
                    let del_result = if is_folder {
                        std::fs::remove_dir_all(lp)
                            .map_err(|e| format!("Failed to remove local folder: {e}"))
                    } else {
                        std::fs::remove_file(lp)
                            .map_err(|e| format!("Failed to remove local file: {e}"))
                    };
                    if let Err(e) = del_result {
                        return Task::perform(async move { e }, Message::ShowToast);
                    }
                    let lp = lp.clone();
                    if is_folder {
                        return initiate_folder_download(
                            app,
                            host,
                            remote_path,
                            lp,
                            &file_name,
                            target,
                        );
                    }
                    return initiate_file_download(
                        app,
                        DownloadStart {
                            host,
                            remote_path,
                            local_path: lp,
                            name: file_name,
                            remote_size: total_bytes,
                            target,
                            resume_offset: None,
                            resumed: false,
                            initial_transferred: 0,
                        },
                    );
                }
                return Task::perform(
                    async { t!("sftp.error.invalid_download_overwrite").to_string() },
                    Message::ShowToast,
                );
            }

            // For folders, delete existing remote folder first, then upload fresh.
            // Folders smaller than the atomic-size threshold are instead staged to
            // a temp sibling directory and swapped in only after a fully-successful
            // upload, so a failed/interrupted transfer never loses the original.
            if is_folder {
                if let Some(lp) = local_path {
                    app.sftp.state.clear_transfer_modal();
                    app.sftp.state.transfer_status =
                        Some(t!("sftp.replacing_folder", name = &file_name).to_string());
                    let use_atomic = total_bytes < crate::ssh::SFTP_ATOMIC_SIZE_THRESHOLD;
                    let mut progress_state = SftpTransferProgress::folder(
                        file_name.clone(),
                        SftpTransferDirection::Upload,
                        total_bytes,
                        0,
                    );
                    progress_state.current_file = if use_atomic {
                        t!("sftp.uploading_ellipsis").to_string()
                    } else {
                        t!("sftp.deleting_existing_folder").to_string()
                    };
                    let progress = Arc::new(Mutex::new(progress_state));
                    app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));

                    if use_atomic {
                        let atomic_host = host;
                        let atomic_remote_path = remote_path;
                        let atomic_progress = std::sync::Arc::clone(&progress);
                        return Task::perform(
                            async move {
                                transfer_folder_atomic(
                                    atomic_host,
                                    lp,
                                    atomic_remote_path,
                                    atomic_progress,
                                )
                                .await
                            },
                            move |result| {
                                Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                                    target: target.clone(),
                                    result,
                                    session_id,
                                })
                            },
                        );
                    }

                    let del_host = host.clone();
                    let del_remote_path = remote_path.clone();
                    let up_host = host;
                    let up_remote_path = remote_path;
                    let up_progress = std::sync::Arc::clone(&progress);

                    return Task::perform(
                        async move {
                            // Step 1: Delete the existing remote folder
                            crate::ssh::sftp_remove_dir_recursive(del_host, del_remote_path)
                                .await
                                .map_err(|e| format!("Failed to remove existing folder: {e}"))?;
                            // Step 2: Upload the new folder
                            transfer_folder(up_host, lp, up_remote_path, up_progress, false).await
                        },
                        move |result| {
                            Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                                target: target.clone(),
                                result,
                                session_id,
                            })
                        },
                    );
                }
                // Remote-to-remote folder replace: delete dest, then copy from source
                if let Some(src_host) = source_host {
                    app.sftp.state.clear_transfer_modal();
                    app.sftp.state.transfer_status =
                        Some(t!("sftp.replacing_folder", name = &file_name).to_string());
                    let mut progress_state = SftpTransferProgress::folder(
                        file_name.clone(),
                        SftpTransferDirection::Upload,
                        0,
                        0,
                    );
                    progress_state.current_file = t!("sftp.deleting_existing_folder").to_string();
                    let progress = Arc::new(Mutex::new(progress_state));
                    app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));

                    let del_host = host.clone();
                    let del_remote_path = remote_path.clone();
                    let src_host2 = src_host.clone();
                    let src_path2 = source_path.clone();
                    let dest_remote_path = remote_path;
                    let dest_host = host;
                    let progress2 = std::sync::Arc::clone(&progress);
                    let target2 = target.clone();
                    let session_id2 = session_id;

                    return Task::perform(
                        async move {
                            crate::ssh::sftp_remove_dir_recursive(del_host, del_remote_path)
                                .await
                                .map_err(|e| {
                                    t!("sftp.error.remove_folder", error = &e.clone()).to_string()
                                })?;
                            transfer_folder_remote_to_remote(
                                src_host2,
                                dest_host,
                                src_path2,
                                dest_remote_path,
                                progress2,
                            )
                            .await
                        },
                        move |result| {
                            Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                                target: target2.clone(),
                                result,
                                session_id: session_id2,
                            })
                        },
                    );
                }
                app.sftp.state.clear_transfer_modal();
                return Task::perform(
                    async { t!("sftp.error.invalid_folder_overwrite").to_string() },
                    Message::ShowToast,
                );
            }

            // Single file upload conflict
            // For small files (< 100 MB): upload to a temp path then
            // rename atomically so the original survives a failure.
            // For large files (≥ 100 MB): keep the delete-then-upload
            // strategy to avoid temporary disk pressure on the remote.
            let use_atomic = total_bytes < crate::ssh::SFTP_ATOMIC_SIZE_THRESHOLD;
            app.sftp.state.clear_transfer_modal();
            app.sftp.state.transfer_status =
                Some(t!("sftp.replacing", name = &file_name).to_string());
            let mut progress_state = SftpTransferProgress::file(
                file_name.clone(),
                SftpTransferDirection::Upload,
                total_bytes,
                0,
                false,
            );
            progress_state.current_file = if use_atomic {
                t!("sftp.uploading_ellipsis").to_string()
            } else {
                t!("sftp.deleting_existing_file").to_string()
            };
            let progress = Arc::new(Mutex::new(progress_state));
            app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));

            let del_host = host.clone();
            let del_remote_path = remote_path.clone();

            if let Some(lp) = local_path {
                if use_atomic {
                    return Task::perform(
                        async move {
                            transfer_file(
                                host,
                                TransferKind::UploadAtomic {
                                    local_path: lp,
                                    remote_path,
                                },
                                progress,
                            )
                            .await
                        },
                        move |result| {
                            Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                                target: target.clone(),
                                result,
                                session_id,
                            })
                        },
                    );
                }
                Task::perform(
                    async move {
                        // Step 1: Delete the existing remote file
                        crate::ssh::sftp_remove_file(del_host, del_remote_path)
                            .await
                            .map_err(|e| format!("Failed to remove existing file: {e}"))?;
                        // Step 2: Upload the new file
                        transfer_file(
                            host,
                            TransferKind::Upload {
                                local_path: lp,
                                remote_path,
                                overwrite: false,
                                resume_offset: None,
                            },
                            progress,
                        )
                        .await
                    },
                    move |result| {
                        Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                            target: target.clone(),
                            result,
                            session_id,
                        })
                    },
                )
            } else if let Some(src_host) = source_host {
                if use_atomic {
                    return Task::perform(
                        async move {
                            transfer_file(
                                host,
                                TransferKind::RemoteToRemoteAtomic {
                                    source_host: src_host,
                                    source_path,
                                    dest_path: remote_path,
                                },
                                progress,
                            )
                            .await
                        },
                        move |result| {
                            Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                                target: target.clone(),
                                result,
                                session_id,
                            })
                        },
                    );
                }
                Task::perform(
                    async move {
                        // Step 1: Delete the existing remote file
                        crate::ssh::sftp_remove_file(del_host, del_remote_path)
                            .await
                            .map_err(|e| format!("Failed to remove existing file: {e}"))?;
                        // Step 2: Copy the new file from source
                        transfer_file(
                            host,
                            TransferKind::RemoteToRemote {
                                source_host: src_host,
                                source_path,
                                dest_path: remote_path,
                                overwrite: false,
                                resume_offset: None,
                            },
                            progress,
                        )
                        .await
                    },
                    move |result| {
                        Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                            target: target.clone(),
                            result,
                            session_id,
                        })
                    },
                )
            } else {
                app.sftp.state.clear_transfer_modal();
                Task::perform(
                    async { t!("sftp.error.invalid_overwrite").to_string() },
                    Message::ShowToast,
                )
            }
        }
        DomainMessage::SftpOverwriteReplaceAll => {
            if let Some(pending) = app.sftp.state.overwrite_pending() {
                if pending.in_progress {
                    return Task::none();
                }
            } else {
                return Task::none();
            }
            app.sftp.state.overwrite_apply_all = Some(OverwriteApplyAll::Replace);
            super::update(app, DomainMessage::SftpOverwriteReplace)
        }
        DomainMessage::SftpOverwriteDuplicate => {
            let Some(pending) = app.sftp.state.overwrite_pending() else {
                return Task::none();
            };
            if pending.in_progress {
                return Task::none();
            }
            let host = pending.host.clone();
            let file_name = pending.file_name.clone();
            let remote_path = pending.remote_path.clone();
            let total_bytes = pending.total_bytes;
            let target = pending.target.clone();
            let local_path = pending.local_path.clone();
            let source_host = pending.source_host.clone();
            let source_path = pending.source_path.clone();
            let is_folder = pending.is_folder;

            app.sftp.state.clear_transfer_modal();

            // Download conflict (Local target): resolve new local name, then download
            if matches!(target, SftpTransferTarget::Local { .. }) {
                if let Some(lp) = local_path {
                    let new_local_path = match resolve_duplicate_local_name(&lp) {
                        Ok(p) => p,
                        Err(e) => {
                            return Task::perform(async move { e }, Message::ShowToast);
                        }
                    };
                    let new_name = new_local_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map_or_else(
                            || format!("{file_name} (1)"),
                            std::string::ToString::to_string,
                        );
                    let new_dir = new_local_path
                        .parent()
                        .and_then(|d| d.to_str())
                        .map(std::string::ToString::to_string)
                        .unwrap_or_default();
                    let new_target = SftpTransferTarget::Local {
                        directory: new_dir,
                        name: new_name.clone(),
                    };
                    app.sftp.state.transfer_status =
                        Some(t!("sftp.duplicating", name = &file_name).to_string());
                    if is_folder {
                        return initiate_folder_download(
                            app,
                            host,
                            remote_path,
                            new_local_path,
                            &new_name,
                            new_target,
                        );
                    }
                    return initiate_file_download(
                        app,
                        DownloadStart {
                            host,
                            remote_path,
                            local_path: new_local_path,
                            name: new_name,
                            remote_size: total_bytes,
                            target: new_target,
                            resume_offset: None,
                            resumed: false,
                            initial_transferred: 0,
                        },
                    );
                }
                return Task::perform(
                    async { t!("sftp.error.invalid_dup_download").to_string() },
                    Message::ShowToast,
                );
            }

            // For folders, use transfer_folder to create a new copy
            if is_folder {
                if let Some(lp) = local_path {
                    app.sftp.state.transfer_status =
                        Some(t!("sftp.duplicating_folder", name = &file_name).to_string());
                    let progress = Arc::new(Mutex::new(SftpTransferProgress::folder(
                        file_name.clone(),
                        SftpTransferDirection::Upload,
                        total_bytes,
                        0,
                    )));
                    app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));
                    let host2 = host.clone();
                    let remote_path2 = remote_path.clone();
                    let target2 = target.clone();
                    return Task::perform(
                        async move {
                            let new_remote_path =
                                resolve_duplicate_name(&host2, &remote_path2).await?;
                            let new_file_name = new_remote_path
                                .rsplit_once('/')
                                .map_or_else(|| format!("{file_name} (1)"), |(_, n)| n.to_string());
                            // Update progress with resolved name
                            if let Ok(mut p) = progress.lock() {
                                p.file_name = new_file_name;
                            }
                            let result_path = new_remote_path.clone();
                            transfer_folder(host, lp, new_remote_path, progress, false)
                                .await
                                .map(|msg| (result_path, msg))
                        },
                        move |result| {
                            let (effective_target, final_result) = match result {
                                Ok((new_path, msg)) => {
                                    let new_dir = new_path
                                        .rsplit_once('/')
                                        .map(|(d, _)| d.to_string())
                                        .unwrap_or_default();
                                    let new_name = new_path
                                        .rsplit_once('/')
                                        .map(|(_, n)| n.to_string())
                                        .unwrap_or_default();
                                    let updated = match &target2 {
                                        SftpTransferTarget::Remote { .. } => {
                                            SftpTransferTarget::Remote {
                                                directory: new_dir,
                                                name: new_name,
                                            }
                                        }
                                        SftpTransferTarget::LeftRemote { .. } => {
                                            SftpTransferTarget::LeftRemote {
                                                directory: new_dir,
                                                name: new_name,
                                            }
                                        }
                                        other @ SftpTransferTarget::Local { .. } => other.clone(),
                                    };
                                    (updated, Ok(msg))
                                }
                                Err(e) => (target2, Err(e)),
                            };
                            Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                                target: effective_target,
                                result: final_result,
                                session_id,
                            })
                        },
                    );
                }
                // Remote-to-remote folder duplicate: resolve name, then copy
                if let Some(src_host) = source_host {
                    app.sftp.state.transfer_status =
                        Some(t!("sftp.duplicating_folder", name = &file_name).to_string());
                    let progress = Arc::new(Mutex::new(SftpTransferProgress::folder(
                        file_name.clone(),
                        SftpTransferDirection::Upload,
                        0,
                        0,
                    )));
                    app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));
                    let host2 = host.clone();
                    let remote_path2 = remote_path.clone();
                    let target2 = target.clone();
                    let src_host2 = src_host;
                    let src_path2 = source_path;
                    let progress2 = std::sync::Arc::clone(&progress);
                    return Task::perform(
                        async move {
                            let new_remote_path =
                                resolve_duplicate_name(&host2, &remote_path2).await?;
                            let new_file_name = new_remote_path
                                .rsplit_once('/')
                                .map_or_else(|| format!("{file_name} (1)"), |(_, n)| n.to_string());
                            if let Ok(mut p) = progress2.lock() {
                                p.file_name = new_file_name;
                            }
                            let result_path = new_remote_path.clone();
                            transfer_folder_remote_to_remote(
                                src_host2,
                                host,
                                src_path2,
                                new_remote_path,
                                progress2,
                            )
                            .await
                            .map(|msg| (result_path, msg))
                        },
                        move |result| {
                            let (effective_target, final_result) = match result {
                                Ok((new_path, msg)) => {
                                    let new_dir = new_path
                                        .rsplit_once('/')
                                        .map(|(d, _)| d.to_string())
                                        .unwrap_or_default();
                                    let new_name = new_path
                                        .rsplit_once('/')
                                        .map(|(_, n)| n.to_string())
                                        .unwrap_or_default();
                                    let updated = match &target2 {
                                        SftpTransferTarget::Remote { .. } => {
                                            SftpTransferTarget::Remote {
                                                directory: new_dir,
                                                name: new_name,
                                            }
                                        }
                                        SftpTransferTarget::LeftRemote { .. } => {
                                            SftpTransferTarget::LeftRemote {
                                                directory: new_dir,
                                                name: new_name,
                                            }
                                        }
                                        other @ SftpTransferTarget::Local { .. } => other.clone(),
                                    };
                                    (updated, Ok(msg))
                                }
                                Err(e) => (target2, Err(e)),
                            };
                            Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                                target: effective_target,
                                result: final_result,
                                session_id,
                            })
                        },
                    );
                }
                return Task::perform(
                    async { t!("sftp.error.invalid_dup_folder").to_string() },
                    Message::ShowToast,
                );
            }

            // For files, resolve the duplicate name first, then upload
            app.sftp.state.transfer_status =
                Some(t!("sftp.duplicating", name = &file_name).to_string());
            let progress = Arc::new(Mutex::new(SftpTransferProgress::file(
                file_name.clone(),
                SftpTransferDirection::Upload,
                total_bytes,
                0,
                false,
            )));
            app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));
            let host2 = host.clone();
            let remote_path2 = remote_path.clone();
            let target2 = target.clone();
            let lp2 = local_path.clone();
            let src_host2 = source_host.clone();
            let sp2 = source_path.clone();
            Task::perform(
                async move {
                    let new_remote_path = resolve_duplicate_name(&host2, &remote_path2).await?;
                    let new_file_name = new_remote_path
                        .rsplit_once('/')
                        .map_or_else(|| format!("{file_name} (1)"), |(_, n)| n.to_string());
                    // Update progress with resolved name
                    if let Ok(mut p) = progress.lock() {
                        p.file_name.clone_from(&new_file_name);
                        p.current_file.clone_from(&new_file_name);
                    }

                    let result_path = new_remote_path.clone();
                    let transfer_kind = if let Some(lp) = lp2 {
                        TransferKind::Upload {
                            local_path: lp,
                            remote_path: new_remote_path,
                            overwrite: false,
                            resume_offset: None,
                        }
                    } else if let Some(src_host) = src_host2 {
                        TransferKind::RemoteToRemote {
                            source_host: src_host,
                            source_path: sp2,
                            dest_path: new_remote_path,
                            overwrite: false,
                            resume_offset: None,
                        }
                    } else {
                        return Err(SftpTransferFailure::Failed {
                            message: t!("sftp.error.invalid_duplicate").to_string(),
                        });
                    };

                    transfer_file(host, transfer_kind, progress)
                        .await
                        .map(|msg| (result_path, msg))
                },
                move |result| {
                    let (effective_target, final_result) = match result {
                        Ok((new_path, msg)) => {
                            let new_dir = new_path
                                .rsplit_once('/')
                                .map(|(d, _)| d.to_string())
                                .unwrap_or_default();
                            let new_name = new_path
                                .rsplit_once('/')
                                .map(|(_, n)| n.to_string())
                                .unwrap_or_default();
                            let updated = match &target2 {
                                SftpTransferTarget::Remote { .. } => SftpTransferTarget::Remote {
                                    directory: new_dir,
                                    name: new_name,
                                },
                                SftpTransferTarget::LeftRemote { .. } => {
                                    SftpTransferTarget::LeftRemote {
                                        directory: new_dir,
                                        name: new_name,
                                    }
                                }
                                other @ SftpTransferTarget::Local { .. } => other.clone(),
                            };
                            (updated, Ok(msg))
                        }
                        Err(e) => (target2, Err(e)),
                    };
                    Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                        target: effective_target,
                        result: final_result,
                        session_id,
                    })
                },
            )
        }
        DomainMessage::SftpOverwriteMerge => {
            let Some(pending) = app.sftp.state.overwrite_pending() else {
                return Task::none();
            };
            if pending.in_progress || !pending.is_folder {
                return Task::none();
            }
            let host = pending.host.clone();
            let file_name = pending.file_name.clone();
            let remote_path = pending.remote_path.clone();
            let total_bytes = pending.total_bytes;
            let target = pending.target.clone();
            let local_path = pending.local_path.clone();
            // Merge: upload the local folder contents into the existing remote folder,
            // or for downloads, download remote folder into existing local folder.
            let source_host = pending.source_host.clone();
            let source_path = pending.source_path.clone();
            app.sftp.state.clear_transfer_modal();

            // Download conflict (Local target): download into existing local folder
            if matches!(target, SftpTransferTarget::Local { .. }) {
                if let Some(lp) = local_path {
                    app.sftp.state.transfer_status =
                        Some(t!("sftp.merging_folder", name = &file_name).to_string());
                    return initiate_folder_download(
                        app,
                        host,
                        remote_path,
                        lp,
                        &file_name,
                        target,
                    );
                }
                return Task::perform(
                    async { t!("sftp.error.invalid_merge_download").to_string() },
                    Message::ShowToast,
                );
            }

            if let Some(lp) = local_path {
                app.sftp.state.transfer_status =
                    Some(t!("sftp.merging_folder", name = &file_name).to_string());
                let progress = Arc::new(Mutex::new(SftpTransferProgress::folder(
                    file_name.clone(),
                    SftpTransferDirection::Upload,
                    total_bytes,
                    0,
                )));
                app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));
                return Task::perform(
                    transfer_folder(host, lp, remote_path, progress, true),
                    move |result| {
                        Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                            target: target.clone(),
                            result,
                            session_id,
                        })
                    },
                );
            }
            // Remote-to-remote folder merge: copy into existing dest folder
            if let Some(src_host) = source_host {
                app.sftp.state.transfer_status =
                    Some(t!("sftp.merging_folder", name = &file_name).to_string());
                let progress = Arc::new(Mutex::new(SftpTransferProgress::folder(
                    file_name.clone(),
                    SftpTransferDirection::Upload,
                    0,
                    0,
                )));
                app.sftp.state.transfer_progress = Some(std::sync::Arc::clone(&progress));
                return Task::perform(
                    transfer_folder_remote_to_remote(
                        src_host,
                        host,
                        source_path,
                        remote_path,
                        progress,
                    ),
                    move |result| {
                        Message::Sftp(crate::sftp::message::Message::SftpTransferFinished {
                            target: target.clone(),
                            result,
                            session_id,
                        })
                    },
                );
            }
            Task::perform(
                async { t!("sftp.error.invalid_merge_folder").to_string() },
                Message::ShowToast,
            )
        }
        _ => unreachable!("unexpected sftp message in conflict: {:?}", msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::{app, host, scratch_dir};
    use crate::sftp::SftpOverwritePending;

    // The transfer itself runs in a `Task::perform` future that never executes
    // in a unit test, so what these cover is the decision layer: which button
    // clears the prompt, what it says while the copy runs, and — for the one
    // case that really deletes something up front, replacing a local file
    // during a download — that it removed the right thing.

    /// An overwrite prompt for `target`, shaped the way the queue builds one.
    fn pending(target: SftpTransferTarget, is_folder: bool) -> SftpOverwritePending {
        SftpOverwritePending {
            host: host(1, "web", "10.0.0.1"),
            file_name: "notes.txt".to_string(),
            remote_path: "/srv/app/notes.txt".to_string(),
            source_path: "/srv/app/notes.txt".to_string(),
            local_path: None,
            source_host: None,
            total_bytes: 2048,
            target,
            in_progress: false,
            is_folder,
        }
    }

    fn local_target() -> SftpTransferTarget {
        SftpTransferTarget::Local {
            directory: "/tmp/downloads".to_string(),
            name: "notes.txt".to_string(),
        }
    }

    fn remote_target() -> SftpTransferTarget {
        SftpTransferTarget::Remote {
            directory: "/srv/app".to_string(),
            name: "notes.txt".to_string(),
        }
    }

    #[test]
    fn cancelling_a_conflict_drops_the_transfer_and_its_decision() {
        let mut app = app();
        app.sftp
            .state
            .show_overwrite_modal(pending(local_target(), false));

        let _ = update(&mut app, &DomainMessage::SftpOverwriteCancel);

        assert!(app.sftp.state.overwrite_pending().is_none());
        assert!(
            app.sftp.state.overwrite_apply_all.is_none(),
            "a skipped file must not become the rule for the rest of the queue"
        );
    }

    #[test]
    fn a_conflict_that_is_already_copying_cannot_be_cancelled() {
        let mut app = app();
        app.sftp
            .state
            .show_overwrite_modal(pending(local_target(), false));
        app.sftp.state.overwrite_pending_mut().unwrap().in_progress = true;

        let _ = update(&mut app, &DomainMessage::SftpOverwriteCancel);

        assert!(
            app.sftp.state.overwrite_pending().is_some(),
            "the prompt stays up to report progress"
        );
    }

    #[test]
    fn replacing_a_download_deletes_the_local_copy_first() {
        let dir = scratch_dir("termlet_sftp_overwrite");
        let local_file = dir.join("notes.txt");
        std::fs::write(&local_file, "old contents").unwrap();

        let mut app = app();
        let mut conflict = pending(local_target(), false);
        conflict.local_path = Some(local_file.clone());
        app.sftp.state.show_overwrite_modal(conflict);

        let _ = update(&mut app, &DomainMessage::SftpOverwriteReplace);

        assert!(
            !local_file.exists(),
            "the existing file has to go before the replacement is written"
        );
        assert!(app.sftp.state.overwrite_pending().is_none());
        assert!(
            app.sftp
                .state
                .transfer_status
                .as_deref()
                .is_some_and(|status| status.contains("notes.txt")),
            "the user is told which file is being replaced: {:?}",
            app.sftp.state.transfer_status
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replacing_a_download_with_nowhere_to_land_reports_instead_of_copying() {
        let mut app = app();
        app.sftp
            .state
            .show_overwrite_modal(pending(local_target(), false));

        let _ = update(&mut app, &DomainMessage::SftpOverwriteReplace);

        assert!(app.sftp.state.overwrite_pending().is_none());
        assert!(app.sftp.state.transfer_progress.is_none());
    }

    #[test]
    fn replacing_an_upload_clears_the_prompt_and_reports_progress() {
        let mut app = app();
        let mut conflict = pending(remote_target(), false);
        conflict.local_path = Some(std::path::PathBuf::from("/tmp/notes.txt"));
        app.sftp.state.show_overwrite_modal(conflict);

        let _ = update(&mut app, &DomainMessage::SftpOverwriteReplace);

        assert!(app.sftp.state.overwrite_pending().is_none());
        let progress = app
            .sftp
            .state
            .transfer_progress
            .as_ref()
            .expect("the transfer panel takes over from the prompt");
        let progress = progress.lock().unwrap();
        assert_eq!(progress.file_name, "notes.txt");
        assert_eq!(progress.total_bytes, 2048);
    }

    #[test]
    fn replace_all_remembers_the_decision_for_the_rest_of_the_queue() {
        let mut app = app();
        let mut conflict = pending(remote_target(), false);
        conflict.local_path = Some(std::path::PathBuf::from("/tmp/notes.txt"));
        app.sftp.state.show_overwrite_modal(conflict);

        let _ = update(&mut app, &DomainMessage::SftpOverwriteReplaceAll);

        assert_eq!(
            app.sftp.state.overwrite_apply_all,
            Some(OverwriteApplyAll::Replace)
        );
        assert!(app.sftp.state.overwrite_pending().is_none());
    }

    #[test]
    fn a_decision_with_no_prompt_open_changes_nothing() {
        let mut app = app();

        for message in [
            DomainMessage::SftpOverwriteReplace,
            DomainMessage::SftpOverwriteReplaceAll,
            DomainMessage::SftpOverwriteDuplicate,
            DomainMessage::SftpOverwriteMerge,
        ] {
            let _ = update(&mut app, &message);
        }

        assert!(app.sftp.state.overwrite_pending().is_none());
        assert!(app.sftp.state.overwrite_apply_all.is_none());
        assert!(app.sftp.state.transfer_progress.is_none());
    }

    #[test]
    fn duplicating_an_upload_keeps_the_original_and_starts_the_copy() {
        let mut app = app();
        let mut conflict = pending(remote_target(), false);
        conflict.local_path = Some(std::path::PathBuf::from("/tmp/notes.txt"));
        app.sftp.state.show_overwrite_modal(conflict);

        let _ = update(&mut app, &DomainMessage::SftpOverwriteDuplicate);

        assert!(app.sftp.state.overwrite_pending().is_none());
    }

    #[test]
    fn merging_two_folders_continues_into_the_existing_one() {
        let mut app = app();
        let mut conflict = pending(remote_target(), true);
        conflict.local_path = Some(std::path::PathBuf::from("/tmp/app"));
        app.sftp.state.show_overwrite_modal(conflict);

        let _ = update(&mut app, &DomainMessage::SftpOverwriteMerge);

        assert!(app.sftp.state.overwrite_pending().is_none());
    }
}
