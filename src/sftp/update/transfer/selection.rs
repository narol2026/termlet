//! Handles uploads and downloads for the selected SFTP rows.

// Rust guideline compliant 2026-10-09

use std::path::Path;

use iced::Task;

use super::queue;
use crate::app::App;
use crate::core::data::{FileEntry, FileEntryType, Host, LogCategory};
use crate::core::message::Message;
use crate::sftp::message::Message as DomainMessage;
use crate::sftp::{
    SftpOverwritePending, SftpPane, SftpQueuedTransfer, SftpTransferDirection, SftpTransferTarget,
};
use rust_i18n::t;

use super::super::shared::{current_remote_host, join_remote_path};

pub(super) fn update(app: &mut App, msg: &DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::SftpUploadSelected => upload_selected(app),
        DomainMessage::SftpDownloadSelected => download_selected(app),
        _ => Task::none(),
    }
}

fn upload_selected(app: &mut App) -> Task<Message> {
    app.sftp.state.context_menu = None;
    app.sftp.state.clear_transfer_modal();
    let Some(host) = current_remote_host(app, SftpPane::Remote) else {
        return toast("sftp.no_remote_host");
    };
    let selected = std::mem::take(&mut app.sftp.state.left.pane.selected);
    let Some(name) = selected.first().cloned() else {
        return toast("sftp.select_local_first");
    };
    if selected.len() > 1 {
        enqueue_remaining_uploads(app, &selected[1..], &host);
    }
    app.add_log(
        LogCategory::Sftp,
        t!("sftp.upload_started", name = &name).to_string(),
    );

    if app.sftp.state.left.pane.connected {
        upload_remote_to_remote(app, &name, &host)
    } else {
        upload_local(app, &name, &host)
    }
}

fn enqueue_remaining_uploads(app: &mut App, names: &[String], host: &Host) {
    if app.sftp.state.left.pane.connected {
        let Some(source_host) = current_remote_host(app, SftpPane::Local) else {
            return;
        };
        for name in names {
            let Some(entry) = find_entry(app, SftpPane::Local, name) else {
                continue;
            };
            let target = SftpTransferTarget::Remote {
                directory: app.sftp.state.right.pane.path.clone(),
                name: name.clone(),
            };
            let item = remote_item(
                name,
                SftpTransferDirection::Upload,
                target,
                remote_spec(
                    host,
                    &source_host,
                    &app.sftp.state.left.pane.path,
                    &app.sftp.state.right.pane.path,
                    &entry,
                ),
                &entry,
            );
            app.sftp.state.transfer_queue.push(item);
        }
        return;
    }

    for name in names {
        let local_path = Path::new(&app.sftp.state.left.pane.path).join(name);
        if let Some(item) = local_upload_item(
            name,
            SftpPane::Remote,
            host,
            &app.sftp.state.right.pane.path,
            &local_path,
        ) {
            app.sftp.state.transfer_queue.push(item);
        }
    }
}

fn upload_remote_to_remote(app: &mut App, name: &str, host: &Host) -> Task<Message> {
    let Some(source_host) = current_remote_host(app, SftpPane::Local) else {
        return toast("sftp.error.no_source_remote");
    };
    let Some(entry) = find_entry(app, SftpPane::Local, name) else {
        return toast("sftp.source_file_gone");
    };
    let target = SftpTransferTarget::Remote {
        directory: app.sftp.state.right.pane.path.clone(),
        name: name.to_string(),
    };
    let item = remote_item(
        name,
        SftpTransferDirection::Upload,
        target,
        remote_spec(
            host,
            &source_host,
            &app.sftp.state.left.pane.path,
            &app.sftp.state.right.pane.path,
            &entry,
        ),
        &entry,
    );
    start_or_enqueue(app, item)
}

fn upload_local(app: &mut App, name: &str, host: &Host) -> Task<Message> {
    let local_path = Path::new(&app.sftp.state.left.pane.path).join(name);
    let Some(item) = local_upload_item(
        name,
        SftpPane::Remote,
        host,
        &app.sftp.state.right.pane.path,
        &local_path,
    ) else {
        return toast("sftp.error.only_files_folders");
    };
    start_or_enqueue(app, item)
}

fn local_upload_item(
    name: &str,
    pane: SftpPane,
    host: &Host,
    remote_dir: &str,
    local_path: &Path,
) -> Option<SftpQueuedTransfer> {
    let target = remote_target(pane, remote_dir, name);
    let remote_path = join_remote_path(remote_dir, name);
    if local_path.is_dir() {
        let (file_count, total_size) = crate::core::data::count_files_recursive(local_path).ok()?;
        Some(queue::upload_folder(
            name.to_string(),
            target,
            host.clone(),
            local_path.to_path_buf(),
            remote_path,
            total_size,
            file_count,
        ))
    } else if local_path.is_file() {
        let total_bytes = local_path.metadata().map_or(0, |metadata| metadata.len());
        Some(queue::upload_file(
            name.to_string(),
            target,
            host.clone(),
            local_path.to_path_buf(),
            remote_path,
            total_bytes,
        ))
    } else {
        None
    }
}

fn download_selected(app: &mut App) -> Task<Message> {
    app.sftp.state.context_menu = None;
    app.sftp.state.clear_transfer_modal();
    let Some(host) = current_remote_host(app, SftpPane::Remote) else {
        return toast("sftp.no_remote_host");
    };
    let selected = std::mem::take(&mut app.sftp.state.right.pane.selected);
    let Some(name) = selected.first().cloned() else {
        return toast("sftp.error.select_remote_first");
    };
    if selected.len() > 1 {
        enqueue_remaining_downloads(app, &selected[1..], &host);
    }
    app.add_log(
        LogCategory::Sftp,
        t!("sftp.download_started", name = &name).to_string(),
    );
    let Some(entry) = app
        .sftp
        .state
        .right
        .pane
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .cloned()
    else {
        return toast("sftp.error.remote_file_gone");
    };
    if entry.file_type == FileEntryType::Symlink {
        return toast("sftp.error.symlink_no_download");
    }
    let remote_path = join_remote_path(&app.sftp.state.right.pane.path, &name);
    if app.sftp.state.left.pane.connected {
        download_remote_to_remote(app, &name, &host, &entry)
    } else {
        download_local(app, &name, host, remote_path, &entry)
    }
}

fn enqueue_remaining_downloads(app: &mut App, names: &[String], host: &Host) {
    if app.sftp.state.left.pane.connected {
        let Some(dest_host) = current_remote_host(app, SftpPane::Local) else {
            return;
        };
        for name in names {
            let Some(entry) = find_entry(app, SftpPane::Remote, name) else {
                continue;
            };
            let target = SftpTransferTarget::LeftRemote {
                directory: app.sftp.state.left.pane.path.clone(),
                name: name.clone(),
            };
            let item = remote_item(
                name,
                SftpTransferDirection::Download,
                target,
                remote_spec(
                    &dest_host,
                    host,
                    &app.sftp.state.right.pane.path,
                    &app.sftp.state.left.pane.path,
                    &entry,
                ),
                &entry,
            );
            app.sftp.state.transfer_queue.push(item);
        }
        return;
    }
    for name in names {
        let Some(entry) = find_entry(app, SftpPane::Remote, name) else {
            continue;
        };
        let target = SftpTransferTarget::Local {
            directory: app.sftp.state.left.pane.path.clone(),
            name: name.clone(),
        };
        let remote_path = join_remote_path(&app.sftp.state.right.pane.path, name);
        let local_path = Path::new(&app.sftp.state.left.pane.path).join(name);
        let item = if entry.file_type == FileEntryType::Folder {
            queue::download_folder(
                name.clone(),
                target,
                host.clone(),
                remote_path,
                local_path,
                entry.size.unwrap_or(0),
            )
        } else {
            queue::download_file(
                name.clone(),
                target,
                host.clone(),
                remote_path,
                local_path,
                entry.size.unwrap_or(0),
            )
        };
        app.sftp.state.transfer_queue.push(item);
    }
}

fn download_remote_to_remote(
    app: &mut App,
    name: &str,
    host: &Host,
    entry: &FileEntry,
) -> Task<Message> {
    let Some(dest_host) = current_remote_host(app, SftpPane::Local) else {
        return toast("sftp.error.no_dest_remote");
    };
    let target = SftpTransferTarget::LeftRemote {
        directory: app.sftp.state.left.pane.path.clone(),
        name: name.to_string(),
    };
    let item = remote_item(
        name,
        SftpTransferDirection::Download,
        target,
        remote_spec(
            &dest_host,
            host,
            &app.sftp.state.right.pane.path,
            &app.sftp.state.left.pane.path,
            entry,
        ),
        entry,
    );
    start_or_enqueue(app, item)
}

fn download_local(
    app: &mut App,
    name: &str,
    host: Host,
    remote_path: String,
    entry: &FileEntry,
) -> Task<Message> {
    let local_path = Path::new(&app.sftp.state.left.pane.path).join(name);
    let target = SftpTransferTarget::Local {
        directory: app.sftp.state.left.pane.path.clone(),
        name: name.to_string(),
    };
    if entry.file_type == FileEntryType::Folder {
        if local_path.exists() && local_path.is_dir() {
            app.sftp.state.transfer_progress = None;
            app.sftp.state.transfer_status = None;
            app.sftp.state.show_overwrite_modal(SftpOverwritePending {
                host,
                file_name: name.to_string(),
                remote_path: remote_path.clone(),
                source_path: remote_path.clone(),
                local_path: Some(local_path.clone()),
                source_host: None,
                total_bytes: 0,
                target,
                in_progress: false,
                is_folder: true,
            });
            return Task::none();
        }
        let item = queue::download_folder(
            name.to_string(),
            target,
            host,
            remote_path,
            local_path,
            entry.size.unwrap_or(0),
        );
        return start_or_enqueue(app, item);
    }

    let remote_size = entry.size.unwrap_or(0);
    let (resume_offset, resumed, transferred) = match std::fs::metadata(&local_path) {
        Ok(metadata) if metadata.len() < remote_size => {
            (Some(metadata.len()), true, metadata.len())
        }
        Ok(_) => {
            app.sftp.state.transfer_progress = None;
            app.sftp.state.transfer_status = None;
            app.sftp.state.show_overwrite_modal(SftpOverwritePending {
                host,
                file_name: name.to_string(),
                remote_path: remote_path.clone(),
                source_path: remote_path,
                local_path: Some(local_path),
                source_host: None,
                total_bytes: remote_size,
                target,
                in_progress: false,
                is_folder: false,
            });
            return Task::none();
        }
        _ => (None, false, 0),
    };
    if app.sftp.state.transfer_progress.is_some() {
        let item = queue::download_file(
            name.to_string(),
            target,
            host,
            remote_path,
            local_path,
            remote_size,
        );
        return queue::enqueue_transfer(app, item);
    }
    queue::initiate_file_download(
        app,
        queue::DownloadStart {
            host,
            remote_path,
            local_path,
            name: name.to_string(),
            remote_size,
            target,
            resume_offset,
            resumed,
            initial_transferred: transferred,
        },
    )
}

fn remote_item(
    name: &str,
    direction: SftpTransferDirection,
    target: SftpTransferTarget,
    remote: queue::RemoteSpec,
    entry: &FileEntry,
) -> SftpQueuedTransfer {
    if entry.file_type == FileEntryType::Folder {
        queue::remote_folder(name.to_string(), direction, target, remote)
    } else {
        queue::remote_file(name.to_string(), direction, target, remote)
    }
}

fn remote_spec(
    dest_host: &Host,
    source_host: &Host,
    source_directory: &str,
    destination_directory: &str,
    entry: &FileEntry,
) -> queue::RemoteSpec {
    queue::RemoteSpec {
        dest_host: dest_host.clone(),
        source_host: source_host.clone(),
        source_path: join_remote_path(source_directory, &entry.name),
        dest_path: join_remote_path(destination_directory, &entry.name),
        total_bytes: entry.size.unwrap_or(0),
    }
}

fn start_or_enqueue(app: &mut App, item: SftpQueuedTransfer) -> Task<Message> {
    if app.sftp.state.transfer_progress.is_some() {
        queue::enqueue_transfer(app, item)
    } else {
        queue::start_item(app, item)
    }
}

fn find_entry(app: &App, pane: SftpPane, name: &str) -> Option<crate::core::data::FileEntry> {
    app.sftp
        .state
        .pane(pane)
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .cloned()
}

fn remote_target(pane: SftpPane, directory: &str, name: &str) -> SftpTransferTarget {
    let directory = directory.to_string();
    let name = name.to_string();
    match pane {
        SftpPane::Local => SftpTransferTarget::LeftRemote { directory, name },
        SftpPane::Remote => SftpTransferTarget::Remote { directory, name },
    }
}

fn toast(key: &str) -> Task<Message> {
    let message = match key {
        "sftp.no_remote_host" => t!("sftp.no_remote_host").to_string(),
        "sftp.select_local_first" => t!("sftp.select_local_first").to_string(),
        "sftp.error.select_remote_first" => t!("sftp.error.select_remote_first").to_string(),
        "sftp.error.no_source_remote" => t!("sftp.error.no_source_remote").to_string(),
        "sftp.source_file_gone" => t!("sftp.source_file_gone").to_string(),
        "sftp.error.only_files_folders" => t!("sftp.error.only_files_folders").to_string(),
        "sftp.error.remote_file_gone" => t!("sftp.error.remote_file_gone").to_string(),
        "sftp.error.symlink_no_download" => t!("sftp.error.symlink_no_download").to_string(),
        "sftp.error.no_dest_remote" => t!("sftp.error.no_dest_remote").to_string(),
        _ => key.to_string(),
    };
    Task::perform(async move { message }, Message::ShowToast)
}
