use iced::Task;
use iced::widget::text_editor;
use std::path::Path;

use crate::app::App;
use crate::core::data::{FileEntryType, Host, LogCategory, read_local_text_preview};
use crate::core::ids::SftpSessionId;
use crate::core::message::Message;
use crate::sftp::message::{Message as DomainMessage, PreviewPayload};
use crate::sftp::{
    SftpContextMenu, SftpDeletePending, SftpNewFolderPending, SftpPane, SftpPreview,
    SftpPreviewSource, SftpPreviewState, SftpRenamePending,
};
use rust_i18n::t;

use super::browse::list_remote_pane;
use super::session::with_sftp_session;
use super::shared::{
    collect_delete_info, current_remote_host, duplicate_name_candidate, join_remote_path,
    request_local_dir_load,
};

pub(super) fn open_preview(
    app: &mut App,
    source: SftpPreviewSource,
    directory: String,
    file_name: String,
) -> Task<Message> {
    let location_label = preview_location_label(app, source, &directory);
    update(
        app,
        DomainMessage::SftpPreviewOpen {
            source,
            directory,
            file_name,
            location_label,
        },
    )
}

fn preview_location_label(app: &App, source: SftpPreviewSource, directory: &str) -> String {
    match source {
        SftpPreviewSource::Local => t!("sftp.preview_local", directory = &directory).to_string(),
        SftpPreviewSource::LeftRemote => {
            let host = current_remote_host(app, SftpPane::Local)
                .map_or_else(|| t!("sftp.location_remote").to_string(), |h| h.name);
            t!("sftp.preview_remote", host = &host, directory = &directory).to_string()
        }
        SftpPreviewSource::RightRemote => {
            let host = current_remote_host(app, SftpPane::Remote)
                .map_or_else(|| t!("sftp.location_remote").to_string(), |h| h.name);
            t!("sftp.preview_remote", host = &host, directory = &directory).to_string()
        }
    }
}

fn load_preview_task(
    app: &App,
    source: SftpPreviewSource,
    directory: &str,
    file_name: &str,
    session_id: SftpSessionId,
) -> Task<Message> {
    match source {
        SftpPreviewSource::Local => {
            let path = Path::new(&directory).join(file_name);
            let result = read_local_text_preview(&path).map(|content| PreviewPayload {
                text: content.text,
                total_bytes: content.total_bytes,
            });
            let directory_ret = directory.to_owned();
            let file_name_ret = file_name.to_owned();
            Task::perform(async move { result }, move |result| {
                Message::Sftp(crate::sftp::message::Message::SftpPreviewLoaded {
                    source,
                    directory: directory_ret.clone(),
                    file_name: file_name_ret.clone(),
                    result,
                    session_id,
                })
            })
        }
        SftpPreviewSource::LeftRemote | SftpPreviewSource::RightRemote => {
            let host = match source {
                SftpPreviewSource::LeftRemote => current_remote_host(app, SftpPane::Local),
                SftpPreviewSource::RightRemote => current_remote_host(app, SftpPane::Remote),
                SftpPreviewSource::Local => return Task::none(),
            };
            let Some(host) = host else {
                return Task::perform(
                    async { t!("sftp.no_remote_host").to_string() },
                    Message::ShowToast,
                );
            };
            let remote_path = join_remote_path(directory, file_name);
            let directory_ret = directory.to_owned();
            let file_name_ret = file_name.to_owned();
            Task::perform(
                async move {
                    crate::ssh::sftp_read_text_preview(host, remote_path)
                        .await
                        .map(|content| PreviewPayload {
                            text: content.text,
                            total_bytes: content.total_bytes,
                        })
                },
                move |result| {
                    Message::Sftp(crate::sftp::message::Message::SftpPreviewLoaded {
                        source,
                        directory: directory_ret.clone(),
                        file_name: file_name_ret.clone(),
                        result,
                        session_id,
                    })
                },
            )
        }
    }
}

fn lookup_entry(app: &App, pane: SftpPane, name: &str) -> Option<(String, FileEntryType)> {
    let pane = app.sftp.state.pane(pane);
    let entry = pane.entries.iter().find(|entry| entry.name == name)?;
    Some((pane.path.clone(), entry.file_type.clone()))
}

fn perform_delete(
    app: &App,
    pane: SftpPane,
    directory: &str,
    name: &str,
    file_type: FileEntryType,
    session_id: SftpSessionId,
) -> Task<Message> {
    let directory_for_msg = directory.to_owned();
    let name_for_msg = name.to_owned();
    let make_finished = move |result: Result<String, String>| {
        Message::Sftp(crate::sftp::message::Message::SftpDeleteFinished {
            pane,
            directory: directory_for_msg.clone(),
            name: name_for_msg.clone(),
            result,
            session_id,
        })
    };

    if app.sftp.state.pane(pane).is_remote() {
        if let Some(host) = current_remote_host(app, pane) {
            let path = join_remote_path(directory, name);
            let display = name.to_owned();
            return Task::perform(
                delete_remote_with_rm_rf(host, path, display, file_type),
                make_finished,
            );
        }
        if pane == SftpPane::Local {
            return Task::perform(
                async { t!("sftp.error.no_source_remote").to_string() },
                Message::ShowToast,
            );
        }
        return Task::perform(
            async { t!("sftp.no_remote_host").to_string() },
            Message::ShowToast,
        );
    }

    if pane == SftpPane::Local {
        let path = Path::new(&directory).join(name);
        let display = name.to_owned();
        return Task::perform(
            async move { delete_local(&path, &display, &file_type) },
            make_finished,
        );
    }

    let Some(host) = current_remote_host(app, pane) else {
        return Task::perform(
            async { t!("sftp.no_remote_host").to_string() },
            Message::ShowToast,
        );
    };
    let path = join_remote_path(directory, name);
    let display = name.to_owned();
    Task::perform(
        delete_remote_with_rm_rf(host, path, display, file_type),
        make_finished,
    )
}

fn delete_local(path: &Path, display: &str, file_type: &FileEntryType) -> Result<String, String> {
    let result = match file_type {
        FileEntryType::Folder => std::fs::remove_dir_all(path),
        FileEntryType::File | FileEntryType::Symlink => std::fs::remove_file(path),
    };
    match result {
        Ok(()) => Ok(t!("sftp.deleted_result", path = &display).to_string()),
        Err(err) => Err(t!(
            "sftp.delete_failed",
            path = &display,
            error = &err.to_string()
        )
        .to_string()),
    }
}

async fn delete_remote(
    host: Host,
    path: String,
    display: String,
    file_type: FileEntryType,
) -> Result<String, String> {
    let outcome = match file_type {
        FileEntryType::Folder => delete_remote_dir(host, path).await,
        FileEntryType::File | FileEntryType::Symlink => {
            crate::ssh::sftp_remove_file(host, path).await
        }
    };
    outcome.map(|()| t!("sftp.deleted_result", path = &display).to_string())
}

async fn delete_remote_dir(host: Host, path: String) -> Result<(), String> {
    crate::ssh::sftp_remove_dir_recursive(host, path).await
}

/// Delete a remote entry, trying `rm -rf` first on Unix-like hosts and falling
/// back to SFTP recursive deletion. Whatever the user asked to delete is what
/// gets deleted: the confirmation prompt is the only gate.
async fn delete_remote_with_rm_rf(
    host: Host,
    path: String,
    display: String,
    file_type: FileEntryType,
) -> Result<String, String> {
    if file_type == FileEntryType::Folder && host.os_type.is_unix_like() {
        // Try `rm -rf` for near-instant deletion (one round-trip).
        if crate::ssh::try_rm_rf_fast(host.clone(), path.clone()).await {
            return Ok(t!("sftp.deleted_result", path = &display).to_string());
        }
    }
    delete_remote(host, path, display, file_type).await
}

fn refresh_after_delete(
    app: &mut App,
    pane: SftpPane,
    directory: &str,
    deleted_names: &[String],
) -> Task<Message> {
    let pane_state = app.sftp.state.pane_mut(pane);
    if pane_state.path == directory {
        // In-place removal preserves scroll position like an upload upsert.
        pane_state
            .entries
            .retain(|entry| !deleted_names.contains(&entry.name));
        pane_state
            .selected
            .retain(|name| !deleted_names.contains(name));
        if pane_state
            .hovered
            .as_ref()
            .is_some_and(|name| deleted_names.contains(name))
        {
            pane_state.hovered = None;
        }
    }
    Task::none()
}

fn refresh_pane_after_change(app: &mut App, pane: SftpPane, directory: &str) -> Task<Message> {
    if app.sftp.state.pane(pane).path != directory {
        return Task::none();
    }
    if app.sftp.state.pane(pane).is_remote() {
        return current_remote_host(app, pane)
            .map_or_else(Task::none, |host| list_remote_pane(app, host, pane));
    }
    if pane == SftpPane::Local {
        return request_local_dir_load(app);
    }
    Task::none()
}

fn perform_rename(
    app: &App,
    pane: SftpPane,
    directory: &str,
    old_name: &str,
    new_name: &str,
    _file_type: FileEntryType,
    session_id: SftpSessionId,
) -> Task<Message> {
    let old_name = old_name.to_owned();
    let new_name = new_name.to_owned();
    let dir_for_finish = directory.to_owned();
    let make_finished = move |result: Result<String, String>| {
        Message::Sftp(crate::sftp::message::Message::SftpRenameFinished {
            pane,
            directory: dir_for_finish.clone(),
            result,
            session_id,
        })
    };

    if app.sftp.state.pane(pane).is_remote() {
        let Some(host) = current_remote_host(app, pane) else {
            return Task::perform(
                async { t!("sftp.no_remote_host").to_string() },
                Message::ShowToast,
            );
        };
        let old_path = join_remote_path(directory, &old_name);
        let new_path = join_remote_path(directory, &new_name);
        let display = t!("sftp.renamed", old = &old_name, new = &new_name).to_string();
        return Task::perform(
            async move {
                crate::ssh::sftp_rename_file(host, old_path, new_path)
                    .await
                    .map(|()| display)
            },
            make_finished,
        );
    }

    if pane != SftpPane::Local {
        return Task::perform(
            async { t!("sftp.no_remote_host").to_string() },
            Message::ShowToast,
        );
    }
    let old_path = Path::new(&directory).join(&old_name);
    let new_path = Path::new(&directory).join(&new_name);
    let display = t!("sftp.renamed", old = &old_name, new = &new_name).to_string();
    Task::perform(
        async move {
            std::fs::rename(&old_path, &new_path)
                .map(|()| display)
                .map_err(|error| format!("Failed to rename {old_name}: {error}"))
        },
        make_finished,
    )
}

fn perform_new_folder(
    app: &App,
    pane: SftpPane,
    directory: &str,
    name: &str,
    session_id: SftpSessionId,
) -> Task<Message> {
    let name = name.to_owned();
    let dir_for_finish = directory.to_owned();
    let make_finished = move |result: Result<String, String>| {
        Message::Sftp(crate::sftp::message::Message::SftpNewFolderFinished {
            pane,
            directory: dir_for_finish.clone(),
            result,
            session_id,
        })
    };

    if app.sftp.state.pane(pane).is_remote() {
        let Some(host) = current_remote_host(app, pane) else {
            return Task::perform(
                async { t!("sftp.no_remote_host").to_string() },
                Message::ShowToast,
            );
        };
        let folder_path = join_remote_path(directory, &name);
        let display = t!("sftp.created_folder", name = &name).to_string();
        return Task::perform(
            async move {
                crate::ssh::sftp_create_remote_dir(host, folder_path)
                    .await
                    .map(|()| display)
            },
            make_finished,
        );
    }

    if pane != SftpPane::Local {
        return Task::perform(
            async { t!("sftp.no_remote_host").to_string() },
            Message::ShowToast,
        );
    }
    let folder_path = Path::new(&directory).join(&name);
    let display = t!("sftp.created_folder", name = &name).to_string();
    Task::perform(
        async move {
            std::fs::create_dir(&folder_path)
                .map(|()| display)
                .map_err(|error| format!("Failed to create folder {name}: {error}"))
        },
        make_finished,
    )
}

/// Resolve an available duplicate name by trying (1), (2), (3), …
pub(super) async fn resolve_duplicate_name(
    host: &Host,
    remote_path: &str,
) -> Result<String, String> {
    for n in 1_u64..1000 {
        let candidate = duplicate_name_candidate(remote_path, n);
        if !crate::ssh::remote_path_exists(host.clone(), candidate.clone()).await? {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Could not find an available name for {remote_path}"
    ))
}

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    let session_id = app.sftp.active_session;
    match msg {
        DomainMessage::SftpPreviewOpen {
            source,
            directory,
            file_name,
            location_label,
        } => {
            app.sftp.state.preview = Some(SftpPreview {
                source,
                directory: directory.clone(),
                file_name: file_name.clone(),
                location_label,
                state: SftpPreviewState::Loading,
            });
            return load_preview_task(app, source, &directory, &file_name, session_id);
        }
        DomainMessage::SftpPreviewLoaded {
            source,
            directory,
            file_name,
            result,
            session_id,
        } => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                let Some(preview) = app.sftp.state.preview.as_mut() else {
                    return Task::none();
                };
                if preview.source != source
                    || preview.directory != directory
                    || preview.file_name != file_name
                {
                    return Task::none();
                }
                let file_name_for_log = file_name.clone();
                preview.state = match result {
                    Ok(payload) => SftpPreviewState::Ready {
                        content: text_editor::Content::with_text(&payload.text),
                        total_bytes: payload.total_bytes,
                    },
                    Err(error) => SftpPreviewState::Failed(error),
                };
                if matches!(preview.state, SftpPreviewState::Ready { .. }) {
                    app.add_log(
                        LogCategory::Sftp,
                        t!("sftp.previewed_file", name = &file_name_for_log).to_string(),
                    );
                }
                Task::none()
            });
        }
        DomainMessage::SftpPreviewAction(action) => {
            let Some(preview) = app.sftp.state.preview.as_mut() else {
                return Task::none();
            };
            // The preview is read-only: apply navigation/selection/scroll
            // actions so text stays selectable, but drop real edits.
            if let SftpPreviewState::Ready { content, .. } = &mut preview.state
                && !action.is_edit()
            {
                content.perform(action);
            }
        }
        DomainMessage::SftpPreviewClose => {
            app.sftp.state.preview = None;
        }
        DomainMessage::SftpContextMenuOpen { pane, name } => {
            let Some((directory, file_type)) = lookup_entry(app, pane, &name) else {
                return Task::none();
            };
            // Preserve multi-selection when right-clicking an already-selected item
            let pane_state = app.sftp.state.pane_mut(pane);
            if !pane_state.selected.contains(&name) {
                pane_state.selected = vec![name.clone()];
                pane_state.last_clicked = Some(name.clone());
            }
            app.sftp.state.context_menu = Some(SftpContextMenu {
                pane,
                directory,
                name,
                file_type,
                position: app.sftp.state.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::SftpTransfer,
                    crate::core::context_menu::ContextMenuAction::SftpNewFolder,
                    crate::core::context_menu::ContextMenuAction::SftpRename,
                    crate::core::context_menu::ContextMenuAction::SftpDelete,
                ]),
            });
            return Task::none();
        }
        DomainMessage::SftpContextMenuClose => {
            app.sftp.state.context_menu = None;
            return Task::none();
        }
        DomainMessage::SftpDeleteRequest => {
            let Some(menu) = app.sftp.state.context_menu.take() else {
                return Task::none();
            };
            // If the right-clicked item is in the multi-selection, delete all selected.
            // Otherwise, delete just the right-clicked item.
            let selected = &app.sftp.state.pane(menu.pane).selected;
            let (names, file_types) = if selected.contains(&menu.name) && selected.len() > 1 {
                collect_delete_info(app, menu.pane, &selected.clone())
            } else {
                (vec![menu.name.clone()], vec![menu.file_type])
            };
            let total = names.len();
            app.sftp.state.delete_confirm = Some(SftpDeletePending {
                pane: menu.pane,
                directory: menu.directory,
                names,
                file_types,
                in_progress: false,
                remaining: total,
                failures: Vec::new(),
            });
        }
        DomainMessage::SftpDeleteCancel => {
            if let Some(pending) = app.sftp.state.delete_confirm.as_ref()
                && pending.in_progress
            {
                return Task::none();
            }
            app.sftp.state.delete_confirm = None;
        }
        DomainMessage::SftpDeleteConfirm => {
            let Some(pending) = app.sftp.state.delete_confirm.as_mut() else {
                return Task::none();
            };
            if pending.in_progress {
                return Task::none();
            }
            pending.in_progress = true;
            let pane = pending.pane;
            let directory = pending.directory.clone();
            let names: Vec<String> = pending.names.clone();
            let file_types: Vec<FileEntryType> = pending.file_types.clone();
            let total = names.len();
            pending.remaining = total;
            // Keep confirm alive so SftpDeleteFinished can decrement the counter
            // and show an aggregated toast when all deletes complete.
            let tasks: Vec<Task<Message>> = names
                .iter()
                .zip(file_types.iter())
                .map(|(name, file_type)| {
                    perform_delete(
                        app,
                        pane,
                        &directory.clone(),
                        &name.clone(),
                        file_type.clone(),
                        session_id,
                    )
                })
                .collect();
            return Task::batch(tasks);
        }
        DomainMessage::SftpDeleteFinished {
            pane,
            directory,
            name,
            result,
            session_id,
        } => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                if let Ok(ref message) = result {
                    app.add_log(
                        LogCategory::Sftp,
                        t!("sftp.deleted_log", message = message.as_str()).to_string(),
                    );
                }
                let Some(pending) = app.sftp.state.delete_confirm.as_mut() else {
                    // Confirm was already cleared (e.g. session switched): only a
                    // late failure still needs to reach the user.
                    return match result {
                        Err(error) => Task::perform(async move { error }, Message::ShowErrorToast),
                        Ok(_) => Task::none(),
                    };
                };
                if let Err(ref error) = result {
                    pending.failures.push((name, error.clone()));
                }
                pending.remaining = pending.remaining.saturating_sub(1);
                if pending.remaining > 0 {
                    // Still waiting for the remaining deletes — aggregate and
                    // report once everything has finished.
                    return Task::none();
                }
                // All deletes have finished. Only the entries that were actually
                // removed should disappear from the view; a failed delete must
                // stay listed (and its error must be shown), otherwise the row
                // would seem deleted and then "come back" on the next refresh.
                let failures = pending.failures.clone();
                let total = pending.names.len();
                let single_name = pending.names.first().cloned();
                let failed_names: Vec<&String> = failures.iter().map(|(n, _)| n).collect();
                let deleted_names: Vec<String> = pending
                    .names
                    .iter()
                    .filter(|n| !failed_names.contains(n))
                    .cloned()
                    .collect();
                app.sftp.state.delete_confirm = None;
                let refresh = refresh_after_delete(app, pane, &directory, &deleted_names);
                if failures.is_empty() {
                    let toast = if total == 1 {
                        let name = single_name.unwrap_or_default();
                        t!("sftp.deleted_toast", scope = &name).to_string()
                    } else {
                        let items = t!("sftp.delete_items_count", count = total).to_string();
                        t!("sftp.deleted_toast", scope = &items).to_string()
                    };
                    Task::batch([
                        refresh,
                        Task::perform(async move { toast }, Message::ShowToast),
                    ])
                } else {
                    // At least one delete failed: report the error instead of
                    // claiming success. `failures.last()` is the most recent one.
                    let error = failures.last().map(|(_, e)| e.clone()).unwrap_or_default();
                    Task::batch([
                        refresh,
                        Task::perform(async move { error }, Message::ShowErrorToast),
                    ])
                }
            });
        }
        DomainMessage::SftpRenameRequest => {
            let Some(menu) = app.sftp.state.context_menu.take() else {
                return Task::none();
            };
            app.sftp.state.rename_pending = Some(SftpRenamePending {
                pane: menu.pane,
                directory: menu.directory,
                old_name: menu.name.clone(),
                file_type: menu.file_type,
                new_name: menu.name,
                in_progress: false,
            });
            return Task::none();
        }
        DomainMessage::SftpRenameCancel => {
            if let Some(pending) = app.sftp.state.rename_pending.as_ref()
                && pending.in_progress
            {
                return Task::none();
            }
            app.sftp.state.rename_pending = None;
        }
        DomainMessage::SftpRenameNewNameChanged(new_name) => {
            if let Some(pending) = app.sftp.state.rename_pending.as_mut() {
                pending.new_name = new_name;
            }
        }
        DomainMessage::SftpRenameConfirm => {
            let Some(pending) = app.sftp.state.rename_pending.as_ref() else {
                return Task::none();
            };
            if pending.in_progress {
                return Task::none();
            }
            let trimmed = pending.new_name.trim().to_string();
            if trimmed.is_empty() || trimmed == pending.old_name {
                app.sftp.state.rename_pending = None;
                return Task::none();
            }
            let forbidden = ['/', '\\', '\0'];
            if trimmed.contains(forbidden.as_slice()) {
                return Task::perform(
                    async { t!("sftp.rename_forbidden_chars").to_string() },
                    Message::ShowToast,
                );
            }
            // Mark the pending rename in flight, then hand the fields to
            // `perform_rename` as owned values. Copying them out inside the
            // borrow's scope keeps the mutable borrow from overlapping the
            // `&App` that `perform_rename` needs.
            let (pane, directory, old_name, new_name, file_type) = {
                let Some(pending) = app.sftp.state.rename_pending.as_mut() else {
                    return Task::none();
                };
                pending.new_name = trimmed;
                pending.in_progress = true;
                (
                    pending.pane,
                    pending.directory.clone(),
                    pending.old_name.clone(),
                    pending.new_name.clone(),
                    pending.file_type.clone(),
                )
            };
            return perform_rename(
                app, pane, &directory, &old_name, &new_name, file_type, session_id,
            );
        }
        DomainMessage::SftpRenameFinished {
            pane,
            directory,
            result,
            session_id,
        } => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                app.sftp.state.rename_pending = None;
                let refresh = refresh_pane_after_change(app, pane, &directory);
                match result {
                    Ok(message) => {
                        app.add_log(
                            LogCategory::Sftp,
                            t!("sftp.renamed_log", message = &message).to_string(),
                        );
                        Task::batch([
                            refresh,
                            Task::perform(async move { message }, Message::ShowToast),
                        ])
                    }
                    Err(error) => Task::batch([
                        refresh,
                        Task::perform(async move { error }, Message::ShowToast),
                    ]),
                }
            });
        }
        DomainMessage::SftpNewFolderRequest { pane, directory } => {
            // Close context menu if open (triggered from either toolbar or context menu)
            app.sftp.state.context_menu = None;
            app.sftp.state.new_folder_pending = Some(SftpNewFolderPending {
                pane,
                directory,
                name: String::new(),
                in_progress: false,
            });
            return Task::none();
        }
        DomainMessage::SftpNewFolderCancel => {
            if let Some(pending) = app.sftp.state.new_folder_pending.as_ref()
                && pending.in_progress
            {
                return Task::none();
            }
            app.sftp.state.new_folder_pending = None;
        }
        DomainMessage::SftpNewFolderNameChanged(name) => {
            if let Some(pending) = app.sftp.state.new_folder_pending.as_mut() {
                pending.name = name;
            }
        }
        DomainMessage::SftpNewFolderConfirm => {
            let Some(pending) = app.sftp.state.new_folder_pending.as_ref() else {
                return Task::none();
            };
            if pending.in_progress {
                return Task::none();
            }
            let trimmed = pending.name.trim().to_string();
            if trimmed.is_empty() {
                return Task::none();
            }
            let forbidden = ['/', '\\', '\0'];
            if trimmed.contains(forbidden.as_slice()) {
                return Task::perform(
                    async { t!("sftp.new_folder_forbidden_chars").to_string() },
                    Message::ShowErrorToast,
                );
            }
            // As for rename above: mark it in flight and copy the fields out
            // while the mutable borrow is in scope.
            let (pane, directory, name) = {
                let Some(pending) = app.sftp.state.new_folder_pending.as_mut() else {
                    return Task::none();
                };
                pending.name = trimmed;
                pending.in_progress = true;
                (
                    pending.pane,
                    pending.directory.clone(),
                    pending.name.clone(),
                )
            };
            return perform_new_folder(app, pane, &directory, &name, session_id);
        }
        DomainMessage::SftpNewFolderFinished {
            pane,
            directory,
            result,
            session_id,
        } => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                app.sftp.state.new_folder_pending = None;
                let refresh = refresh_pane_after_change(app, pane, &directory);
                match result {
                    Ok(message) => {
                        app.add_log(
                            LogCategory::Sftp,
                            t!("sftp.created_folder_log", message = &message).to_string(),
                        );
                        Task::batch([
                            refresh,
                            Task::perform(async move { message }, Message::ShowToast),
                        ])
                    }
                    Err(error) => Task::batch([
                        refresh,
                        Task::perform(async move { error }, Message::ShowErrorToast),
                    ]),
                }
            });
        }
        _ => unreachable!("unexpected sftp message in file_ops: {:?}", msg),
    }
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::FileEntry;
    use crate::core::test_support::app;

    // The file operations themselves run inside `Task::perform` futures, which
    // never execute without the iced runtime — so what these tests pin is the
    // dialog state machine in front of them: what opens the prompt, what the
    // prompt refuses, and what it does on the way out.

    fn entry(name: &str, file_type: FileEntryType) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            file_type,
            size: Some(1),
            modified: String::new(),
            modified_epoch: Some(0),
            permissions: String::new(),
            kind: String::new(),
        }
    }

    fn context_menu(app: &mut App, pane: SftpPane, name: &str, file_type: FileEntryType) {
        app.sftp.state.context_menu = Some(SftpContextMenu {
            pane,
            directory: "/srv/app".to_string(),
            name: name.to_string(),
            file_type,
            position: iced::Point::ORIGIN,
            cursor: crate::context_menu::initial_cursor(&[
                crate::core::context_menu::ContextMenuAction::SftpTransfer,
                crate::core::context_menu::ContextMenuAction::SftpNewFolder,
                crate::core::context_menu::ContextMenuAction::SftpRename,
                crate::core::context_menu::ContextMenuAction::SftpDelete,
            ]),
        });
    }

    #[test]
    fn deleting_a_single_row_asks_about_just_that_row() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);

        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);

        let pending = app.sftp.state.delete_confirm.as_ref().expect("a prompt");
        assert_eq!(pending.names, ["notes.txt"]);
        assert_eq!(pending.file_types, [FileEntryType::File]);
        assert_eq!(pending.remaining, 1);
        assert!(!pending.in_progress);
        assert!(
            app.sftp.state.context_menu.is_none(),
            "the menu closes behind the prompt"
        );
    }

    #[test]
    fn deleting_a_row_that_is_part_of_a_selection_deletes_the_whole_selection() {
        let mut app = app();
        app.sftp.state.left.pane.entries = vec![
            entry("a.txt", FileEntryType::File),
            entry("b.txt", FileEntryType::File),
            entry("dir", FileEntryType::Folder),
        ];
        app.sftp.state.left.pane.selected = vec!["a.txt".into(), "b.txt".into(), "dir".into()];
        context_menu(&mut app, SftpPane::Local, "b.txt", FileEntryType::File);

        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);

        let pending = app.sftp.state.delete_confirm.as_ref().expect("a prompt");
        assert_eq!(pending.names.len(), 3, "every selected row is queued");
        assert!(
            pending.file_types.contains(&FileEntryType::Folder),
            "the prompt needs each row's type so it can warn about folders"
        );
    }

    #[test]
    fn right_clicking_outside_the_selection_deletes_only_that_row() {
        let mut app = app();
        app.sftp.state.left.pane.selected = vec!["a.txt".into(), "b.txt".into()];
        context_menu(
            &mut app,
            SftpPane::Local,
            "untouched.txt",
            FileEntryType::File,
        );

        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);

        let pending = app.sftp.state.delete_confirm.as_ref().expect("a prompt");
        assert_eq!(pending.names, ["untouched.txt"]);
    }

    #[test]
    fn asking_to_delete_without_a_context_menu_opens_nothing() {
        let mut app = app();

        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);

        assert!(app.sftp.state.delete_confirm.is_none());
    }

    #[test]
    fn a_delete_prompt_can_be_cancelled_until_it_starts_working() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);

        let _ = update(&mut app, DomainMessage::SftpDeleteCancel);
        assert!(app.sftp.state.delete_confirm.is_none());

        // Once the deletion is running there is nothing left to cancel, so the
        // prompt stays up and reports progress instead.
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);
        app.sftp.state.delete_confirm.as_mut().unwrap().in_progress = true;
        let _ = update(&mut app, DomainMessage::SftpDeleteCancel);
        assert!(app.sftp.state.delete_confirm.is_some());
    }

    #[test]
    fn confirming_a_delete_marks_the_prompt_as_working() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);

        let _ = update(&mut app, DomainMessage::SftpDeleteConfirm);

        let pending = app.sftp.state.delete_confirm.as_ref().expect("still up");
        assert!(
            pending.in_progress,
            "the prompt turns into a progress panel"
        );
        assert_eq!(pending.remaining, 1);
    }

    #[test]
    fn confirming_twice_does_not_queue_the_same_delete_twice() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpDeleteRequest);
        let _ = update(&mut app, DomainMessage::SftpDeleteConfirm);

        // The user clicks again while the first pass is still running.
        let _ = update(&mut app, DomainMessage::SftpDeleteConfirm);

        assert!(app.sftp.state.delete_confirm.as_ref().unwrap().in_progress);
        assert_eq!(
            app.sftp.state.delete_confirm.as_ref().unwrap().remaining,
            1,
            "the second click must not restart or duplicate the batch"
        );
    }

    #[test]
    fn renaming_starts_from_the_current_name() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);

        let _ = update(&mut app, DomainMessage::SftpRenameRequest);

        let pending = app.sftp.state.rename_pending.as_ref().expect("a prompt");
        assert_eq!(pending.old_name, "notes.txt");
        assert_eq!(pending.new_name, "notes.txt", "the name is preselected");
        assert_eq!(pending.pane, SftpPane::Local);
        assert!(!pending.in_progress);
    }

    #[test]
    fn renaming_can_be_abandoned_before_it_starts() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpRenameRequest);
        let _ = update(
            &mut app,
            DomainMessage::SftpRenameNewNameChanged("notes-2.txt".into()),
        );
        assert_eq!(
            app.sftp.state.rename_pending.as_ref().unwrap().new_name,
            "notes-2.txt"
        );

        let _ = update(&mut app, DomainMessage::SftpRenameCancel);
        assert!(app.sftp.state.rename_pending.is_none());
    }

    #[test]
    fn a_rename_to_the_same_name_closes_the_prompt_without_doing_work() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpRenameRequest);

        let _ = update(&mut app, DomainMessage::SftpRenameConfirm);

        assert!(
            app.sftp.state.rename_pending.is_none(),
            "there is nothing to rename"
        );
    }

    #[test]
    fn a_rename_that_is_only_whitespace_is_refused() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpRenameRequest);
        let _ = update(
            &mut app,
            DomainMessage::SftpRenameNewNameChanged("   ".into()),
        );

        let _ = update(&mut app, DomainMessage::SftpRenameConfirm);

        assert!(app.sftp.state.rename_pending.is_none());
        assert!(
            !app.sftp
                .state
                .rename_pending
                .as_ref()
                .is_some_and(|p| p.in_progress),
            "nothing was started"
        );
    }

    #[test]
    fn a_rename_containing_a_separator_is_refused_and_the_prompt_stays_open() {
        let mut app = app();
        context_menu(&mut app, SftpPane::Local, "notes.txt", FileEntryType::File);
        let _ = update(&mut app, DomainMessage::SftpRenameRequest);
        let _ = update(
            &mut app,
            DomainMessage::SftpRenameNewNameChanged("../escape.txt".into()),
        );

        let _ = update(&mut app, DomainMessage::SftpRenameConfirm);

        let pending = app
            .sftp
            .state
            .rename_pending
            .as_ref()
            .expect("the prompt stays open so the name can be fixed");
        assert!(!pending.in_progress);
        assert_eq!(pending.new_name, "../escape.txt");
    }

    #[test]
    fn creating_a_folder_starts_from_an_empty_name() {
        let mut app = app();

        let _ = update(
            &mut app,
            DomainMessage::SftpNewFolderRequest {
                pane: SftpPane::Local,
                directory: "/srv/app".into(),
            },
        );

        let pending = app
            .sftp
            .state
            .new_folder_pending
            .as_ref()
            .expect("a prompt");
        assert!(pending.name.is_empty());
        assert_eq!(pending.directory, "/srv/app");
        assert!(!pending.in_progress);

        let _ = update(
            &mut app,
            DomainMessage::SftpNewFolderNameChanged("reports".into()),
        );
        assert_eq!(
            app.sftp.state.new_folder_pending.as_ref().unwrap().name,
            "reports"
        );

        let _ = update(&mut app, DomainMessage::SftpNewFolderCancel);
        assert!(app.sftp.state.new_folder_pending.is_none());
    }

    #[test]
    fn an_empty_folder_name_is_not_created_and_the_prompt_waits() {
        let mut app = app();
        let _ = update(
            &mut app,
            DomainMessage::SftpNewFolderRequest {
                pane: SftpPane::Local,
                directory: "/srv/app".into(),
            },
        );

        let _ = update(&mut app, DomainMessage::SftpNewFolderConfirm);

        // Pressing Create with nothing typed leaves the prompt up and starts
        // nothing, rather than creating a folder called "".
        let pending = app
            .sftp
            .state
            .new_folder_pending
            .as_ref()
            .expect("the prompt stays open");
        assert!(!pending.in_progress);
    }

    #[test]
    fn a_folder_name_with_a_separator_is_refused() {
        let mut app = app();
        let _ = update(
            &mut app,
            DomainMessage::SftpNewFolderRequest {
                pane: SftpPane::Local,
                directory: "/srv/app".into(),
            },
        );
        let _ = update(
            &mut app,
            DomainMessage::SftpNewFolderNameChanged("nested/path".into()),
        );

        let _ = update(&mut app, DomainMessage::SftpNewFolderConfirm);

        let pending = app
            .sftp
            .state
            .new_folder_pending
            .as_ref()
            .expect("the prompt stays open");
        assert!(!pending.in_progress);
    }

    #[test]
    fn the_preview_closes_and_reports_where_it_came_from() {
        let mut app = app();
        let location = preview_location_label(&app, SftpPreviewSource::Local, "/srv/app");
        assert!(
            location.contains("/srv/app"),
            "the preview header names the directory it is showing: {location}"
        );

        app.sftp.state.preview = Some(SftpPreview {
            source: SftpPreviewSource::Local,
            directory: "/srv/app".into(),
            file_name: "notes.txt".into(),
            location_label: location,
            state: SftpPreviewState::Ready {
                content: text_editor::Content::with_text("hello"),
                total_bytes: Some(5),
            },
        });

        let _ = update(&mut app, DomainMessage::SftpPreviewClose);

        assert!(app.sftp.state.preview.is_none());
    }

    #[test]
    fn an_entry_is_looked_up_in_the_pane_that_asked() {
        let mut app = app();
        app.sftp.state.left.pane.path = "/home/narol".to_string();
        app.sftp
            .state
            .left
            .pane
            .entries
            .push(entry("notes.txt", FileEntryType::File));
        app.sftp.state.right.pane.path = "/srv/app".to_string();
        app.sftp
            .state
            .right
            .pane
            .entries
            .push(entry("notes.txt", FileEntryType::Folder));

        assert_eq!(
            lookup_entry(&app, SftpPane::Local, "notes.txt"),
            Some(("/home/narol".to_string(), FileEntryType::File)),
            "the local pane looks in the local listing"
        );
        assert_eq!(
            lookup_entry(&app, SftpPane::Remote, "notes.txt"),
            Some(("/srv/app".to_string(), FileEntryType::Folder)),
            "the remote pane looks in the remote listing"
        );
        assert_eq!(
            lookup_entry(&app, SftpPane::Local, "gone.txt"),
            None,
            "a name that is not in the listing resolves to nothing"
        );
    }

    #[test]
    fn a_left_remote_replaces_and_then_restores_the_local_listing() {
        let mut app = app();
        app.sftp.state.left.pane.path = "/home/narol".to_string();
        app.sftp
            .state
            .left
            .pane
            .entries
            .push(entry("home-notes.txt", FileEntryType::File));

        let remote = app.sftp.state.enter_remote(SftpPane::Local);
        remote.connected = true;
        remote.path = "/srv/app".to_string();
        app.sftp
            .state
            .left
            .pane
            .entries
            .push(entry("remote-notes.txt", FileEntryType::File));

        assert_eq!(
            lookup_entry(&app, SftpPane::Local, "remote-notes.txt"),
            Some(("/srv/app".to_string(), FileEntryType::File))
        );
        assert_eq!(
            lookup_entry(&app, SftpPane::Local, "home-notes.txt"),
            None,
            "the local directory behind the remote pane is out of scope"
        );

        app.sftp.state.disconnect_left_remote();
        assert_eq!(app.sftp.state.left.pane.path, "/home/narol");
        assert_eq!(
            lookup_entry(&app, SftpPane::Local, "home-notes.txt"),
            Some(("/home/narol".to_string(), FileEntryType::File)),
            "disconnecting restores the local pane exactly as it was"
        );
    }

    #[test]
    fn a_preview_label_names_the_directory_and_the_host_it_came_from() {
        let mut app = app();
        app.sftp.state.left.pane.host_id = Some(crate::core::ids::HostId(7));
        app.hosts.list = vec![crate::core::test_support::host(7, "web", "10.0.0.1")];

        let local = preview_location_label(&app, SftpPreviewSource::Local, "/home/narol");
        assert!(
            local.contains("/home/narol"),
            "a local preview names its directory: {local}"
        );
        assert!(
            !local.contains("web"),
            "and no host, because the file is on this machine: {local}"
        );

        let remote = preview_location_label(&app, SftpPreviewSource::LeftRemote, "/srv/app");
        assert!(
            remote.contains("web") && remote.contains("/srv/app"),
            "a remote preview names both the hop and the directory: {remote}"
        );

        // The right pane has not connected to anything, so its label has no
        // host name to use.
        let unconnected = preview_location_label(&app, SftpPreviewSource::RightRemote, "/srv/app");
        assert!(
            unconnected.contains("/srv/app") && !unconnected.contains("web"),
            "a pane with no host falls back to a generic name: {unconnected}"
        );
        assert_ne!(remote, unconnected);
    }
}
