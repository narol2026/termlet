//! Handles files dragged into an SFTP pane from the operating system.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use iced::Task;
use iced::widget::Id;

use super::queue;
use crate::app::App;
use crate::core::message::Message;
use crate::sftp::message::Message as DomainMessage;
use crate::sftp::{SftpExternalDropTarget, SftpPane, SftpTransferTarget};
use rust_i18n::t;

use super::super::session::with_sftp_session;
use super::super::shared::{
    LOCAL_DROP_ZONE_ID, REMOTE_DROP_ZONE_ID, current_remote_host, external_drop_target,
    join_remote_path, request_local_dir_load,
};

pub(crate) fn cursor_moved(app: &mut App, point: iced::Point) -> Task<Message> {
    app.window.cursor_pos = point;
    if app.sftp.state.external_drag_paths.is_empty() {
        return Task::none();
    }
    let paths = app.sftp.state.external_drag_paths.clone();
    iced_drop::zones_on_point(
        move |zones| {
            Message::Sftp(DomainMessage::SftpExternalDropZonesHovered {
                paths: paths.clone(),
                zones,
            })
        },
        point,
        Some(drop_zone_ids()),
        None,
    )
}

pub(crate) fn drag_point(app: &mut App) -> iced::Point {
    if let Some(point) = crate::core::platform::cursor_position_in_window(
        app.window.position,
        app.window.scale_factor,
    ) {
        app.window.cursor_pos = point;
        point
    } else {
        app.window.cursor_pos
    }
}

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::SftpExternalFilesHovered(paths) => {
            app.sftp.state.external_drag_paths.clone_from(&paths);
            let point = drag_point(app);
            iced_drop::zones_on_point(
                move |zones| {
                    Message::Sftp(DomainMessage::SftpExternalDropZonesHovered {
                        paths: paths.clone(),
                        zones,
                    })
                },
                point,
                Some(drop_zone_ids()),
                None,
            )
        }
        DomainMessage::SftpExternalDropZonesHovered { paths, zones, .. } => {
            if app.sftp.state.external_drag_paths != paths {
                return Task::none();
            }
            let point = drag_point(app);
            app.sftp.state.external_drop_target = external_drop_target(&zones, point);
            Task::none()
        }
        DomainMessage::SftpExternalFilesHoverLeft => {
            app.sftp.state.external_drag_paths.clear();
            app.sftp.state.external_drop_target = None;
            Task::none()
        }
        DomainMessage::SftpExternalFilesDropped(paths) => {
            app.sftp.state.external_drag_paths.clear();
            app.sftp.state.external_drop_target = None;
            let point = drag_point(app);
            iced_drop::zones_on_point(
                move |zones| {
                    Message::Sftp(DomainMessage::SftpExternalDropZonesFound {
                        paths: paths.clone(),
                        point,
                        zones,
                    })
                },
                point,
                Some(drop_zone_ids()),
                None,
            )
        }
        DomainMessage::SftpExternalDropZonesFound {
            paths,
            point,
            zones,
        } => dropped(app, paths, point, &zones),
        DomainMessage::SftpExternalCopyDone {
            message,
            session_id,
        } => with_sftp_session(app, session_id, |app, _is_active| {
            Task::batch([
                request_local_dir_load(app),
                Task::perform(async move { message }, Message::ShowToast),
            ])
        }),
        _ => Task::none(),
    }
}

fn dropped(
    app: &mut App,
    paths: Vec<PathBuf>,
    point: iced::Point,
    zones: &[(Id, iced::Rectangle)],
) -> Task<Message> {
    let Some(drop_target) = external_drop_target(zones, point) else {
        return Task::none();
    };
    let valid_paths: Vec<PathBuf> = paths.into_iter().filter(|path| path.exists()).collect();
    let Some(local_path) = valid_paths.first().cloned() else {
        return Task::perform(
            async { t!("sftp.error.dropped_file_gone").to_string() },
            Message::ShowToast,
        );
    };
    let name = local_path.file_name().map_or_else(
        || "unknown".to_string(),
        |name| name.to_string_lossy().to_string(),
    );

    match drop_target {
        SftpExternalDropTarget::Remote => {
            enqueue_remaining(app, &valid_paths[1..], SftpPane::Remote);
            upload_local_path(app, SftpPane::Remote, &local_path)
        }
        SftpExternalDropTarget::Local if app.sftp.state.left.pane.connected => {
            enqueue_remaining(app, &valid_paths[1..], SftpPane::Local);
            upload_local_path(app, SftpPane::Local, &local_path)
        }
        SftpExternalDropTarget::Local => copy_to_local(app, local_path, name),
    }
}

fn enqueue_remaining(app: &mut App, paths: &[PathBuf], pane: SftpPane) {
    let Some(host) = current_remote_host(app, pane) else {
        return;
    };
    let remote_dir = app.sftp.state.pane(pane).path.clone();
    for local_path in paths {
        append_path(app, pane, &host, &remote_dir, local_path);
    }
}

fn append_path(
    app: &mut App,
    pane: SftpPane,
    host: &crate::core::data::Host,
    remote_dir: &str,
    local_path: &Path,
) {
    let name = local_path.file_name().map_or_else(
        || "unknown".to_string(),
        |name| name.to_string_lossy().to_string(),
    );
    let Ok(Some(item)) = prepare_upload(pane, host, remote_dir, local_path, name) else {
        return;
    };
    app.sftp.state.transfer_queue.push(item);
}

fn upload_local_path(app: &mut App, pane: SftpPane, local_path: &Path) -> Task<Message> {
    let Some(host) = current_remote_host(app, pane) else {
        let message = match pane {
            SftpPane::Local => t!("sftp.error.no_left_remote").to_string(),
            SftpPane::Remote => t!("sftp.error.connect_first").to_string(),
        };
        return Task::perform(async move { message }, Message::ShowToast);
    };
    let name = local_path.file_name().map_or_else(
        || "unknown".to_string(),
        |name| name.to_string_lossy().to_string(),
    );
    let remote_dir = app.sftp.state.pane(pane).path.clone();
    let item = match prepare_upload(pane, &host, &remote_dir, local_path, name) {
        Ok(Some(item)) => item,
        Ok(None) => {
            return Task::perform(
                async { t!("sftp.error.only_files_folders").to_string() },
                Message::ShowToast,
            );
        }
        Err(error) => {
            return Task::perform(
                async move { t!("sftp.error.scan_folder", error = &error).to_string() },
                Message::ShowToast,
            );
        }
    };
    if app.sftp.state.transfer_progress.is_some() {
        return queue::enqueue_transfer(app, item);
    }
    queue::start_item(app, item)
}

fn prepare_upload(
    pane: SftpPane,
    host: &crate::core::data::Host,
    remote_dir: &str,
    local_path: &Path,
    name: String,
) -> Result<Option<crate::sftp::SftpQueuedTransfer>, String> {
    let remote_path = join_remote_path(remote_dir, &name);
    let target = target_for(pane, remote_dir, &name);
    if local_path.is_dir() {
        let (file_count, total_size) = crate::core::data::count_files_recursive(local_path)?;
        Ok(Some(queue::upload_folder(
            name,
            target,
            host.clone(),
            local_path.to_path_buf(),
            remote_path,
            total_size,
            file_count,
        )))
    } else if local_path.is_file() {
        let total_bytes = local_path.metadata().map_or(0, |metadata| metadata.len());
        Ok(Some(queue::upload_file(
            name,
            target,
            host.clone(),
            local_path.to_path_buf(),
            remote_path,
            total_bytes,
        )))
    } else {
        Ok(None)
    }
}

fn target_for(pane: SftpPane, directory: &str, name: &str) -> SftpTransferTarget {
    match pane {
        SftpPane::Local => SftpTransferTarget::LeftRemote {
            directory: directory.to_string(),
            name: name.to_string(),
        },
        SftpPane::Remote => SftpTransferTarget::Remote {
            directory: directory.to_string(),
            name: name.to_string(),
        },
    }
}

fn copy_to_local(app: &mut App, local_path: PathBuf, name: String) -> Task<Message> {
    let session_id = app.sftp.active_session;
    let destination = app.sftp.state.left.pane.path.clone();
    Task::perform(
        async move { copy_path(&local_path, &name, &destination) },
        move |result| match result {
            Ok(message) => Message::Sftp(DomainMessage::SftpExternalCopyDone {
                message,
                session_id,
            }),
            Err(error) => Message::ShowErrorToast(error),
        },
    )
}

fn copy_path(source: &Path, name: &str, destination: &str) -> Result<String, String> {
    let destination = PathBuf::from(destination).join(name);
    if source.is_dir() {
        copy_dir(source, &destination)?;
    } else {
        std::fs::copy(source, &destination)
            .map_err(|error| t!("sftp.error.copy_file", error = &error.to_string()).to_string())?;
    }
    Ok(t!("sftp.copied_to", path = &destination.display().to_string()).to_string())
}

fn copy_dir(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination)
        .map_err(|error| t!("sftp.error.create_dir", error = &error.to_string()).to_string())?;
    let entries = std::fs::read_dir(source)
        .map_err(|error| t!("sftp.error.read_dir", error = &error.to_string()).to_string())?;
    for entry in entries {
        let entry = entry
            .map_err(|error| t!("sftp.error.read_entry", error = &error.to_string()).to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir(&source_path, &destination_path)?;
        } else {
            std::fs::copy(&source_path, &destination_path).map_err(|error| {
                t!("sftp.error.copy_file", error = &error.to_string()).to_string()
            })?;
        }
    }
    Ok(())
}

fn drop_zone_ids() -> Vec<Id> {
    vec![Id::from(LOCAL_DROP_ZONE_ID), Id::from(REMOTE_DROP_ZONE_ID)]
}
