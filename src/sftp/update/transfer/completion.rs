//! Handles transfer completion, cancellation, and overwrite decisions.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use iced::Task;

use super::super::browse::{apply_transfer_entry_update, refresh_transfer_target};
use super::super::session::with_sftp_session;
use super::super::shared::{current_remote_host, join_remote_path};
use super::queue;
use crate::app::App;
use crate::core::message::Message;
use crate::sftp::message::Message as DomainMessage;
use crate::sftp::{
    OverwriteApplyAll, SftpOverwritePending, SftpPane, SftpTransferFailure, SftpTransferModal,
    SftpTransferResult, SftpTransferResultStatus, SftpTransferTarget,
};
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::SftpCancelTransferRequest => cancel_requested(app),
        DomainMessage::SftpCancelTransferCancel => cancel_dialog_closed(app),
        DomainMessage::SftpCancelTransfer => cancel_confirmed(app),
        DomainMessage::SftpCancelQueuedTransfer(index) => cancel_queued(app, index),
        DomainMessage::SftpClearTransferQueue => clear_queue(app),
        DomainMessage::SftpDismissTransferResult => {
            app.sftp.state.transfer_results.dismiss_latest();
            Task::none()
        }
        DomainMessage::SftpDismissTransferFailure(index) => {
            app.sftp.state.transfer_results.dismiss_failure(index);
            Task::none()
        }
        DomainMessage::SftpTransferFinished {
            target,
            result,
            session_id,
        } => finished(app, &target, result, session_id),
        DomainMessage::SftpTransferEntryRefreshed { target, result } => match result {
            Ok(entry) => {
                apply_transfer_entry_update(app, &target, entry);
                Task::none()
            }
            Err(error) => Task::perform(async move { error }, Message::ShowToast),
        },
        _ => Task::none(),
    }
}

fn cancel_requested(app: &mut App) -> Task<Message> {
    if app.sftp.state.transfer_modal.is_none() && app.sftp.state.transfer_progress.is_some() {
        let file_name = app
            .sftp
            .state
            .transfer_progress
            .as_ref()
            .and_then(|progress| {
                progress
                    .lock()
                    .ok()
                    .map(|progress| progress.file_name.clone())
            })
            .unwrap_or_default();
        app.sftp.state.transfer_modal = Some(SftpTransferModal::Discard {
            file_name,
            queued_overwrite: None,
        });
    }
    Task::none()
}

fn cancel_dialog_closed(app: &mut App) -> Task<Message> {
    let queued_overwrite = match app.sftp.state.transfer_modal.take() {
        Some(SftpTransferModal::Discard {
            queued_overwrite, ..
        }) => queued_overwrite,
        _ => None,
    };
    if let Some(pending) = queued_overwrite {
        app.sftp.state.transfer_progress = None;
        app.sftp.state.show_overwrite_modal(pending);
        if app.sftp.state.overwrite_apply_all == Some(OverwriteApplyAll::Replace) {
            return super::super::update(app, DomainMessage::SftpOverwriteReplace);
        }
    }
    Task::none()
}

fn cancel_confirmed(app: &mut App) -> Task<Message> {
    let queued_overwrite = match app.sftp.state.transfer_modal.take() {
        Some(SftpTransferModal::Discard {
            queued_overwrite, ..
        }) => queued_overwrite,
        _ => None,
    };
    if let Some(progress) = app.sftp.state.transfer_progress.as_ref() {
        let _ = progress.lock().map(|mut progress| {
            progress.cancelled = true;
        });
        app.sftp.state.transfer_status = Some(t!("sftp.cancelling").to_string());
        return Task::none();
    }
    if queued_overwrite.is_some() {
        app.sftp.state.transfer_status = None;
        let next = queue::start_next_queued_transfer(app);
        return Task::batch([
            next,
            Task::perform(
                async { t!("sftp.transfer_cancelled").to_string() },
                Message::ShowWarningToast,
            ),
        ]);
    }
    Task::none()
}

fn cancel_queued(app: &mut App, index: usize) -> Task<Message> {
    if index >= app.sftp.state.transfer_queue.len() {
        return Task::none();
    }
    let removed = app.sftp.state.transfer_queue.remove(index);
    Task::perform(
        async move { t!("sftp.queue_removed", name = &removed.file_name).to_string() },
        Message::ShowToast,
    )
}

fn clear_queue(app: &mut App) -> Task<Message> {
    let count = app.sftp.state.transfer_queue.len();
    if count == 0 {
        return Task::none();
    }
    app.sftp.state.transfer_queue.clear();
    Task::perform(
        async move { t!("sftp.queue_cleared", count = count).to_string() },
        Message::ShowToast,
    )
}

fn finished(
    app: &mut App,
    target: &SftpTransferTarget,
    result: Result<String, SftpTransferFailure>,
    session_id: crate::core::ids::SftpSessionId,
) -> Task<Message> {
    with_sftp_session(app, session_id, |app, is_active| {
        if !app.sftp.state.has_live_transfer() {
            return Task::none();
        }
        app.sftp.state.transfer_status = None;
        match result {
            Ok(_) => finish(
                app,
                is_active,
                target,
                SftpTransferResultStatus::Succeeded,
                None,
            ),
            Err(SftpTransferFailure::Cancelled) => finish(
                app,
                is_active,
                target,
                SftpTransferResultStatus::Cancelled,
                None,
            ),
            Err(SftpTransferFailure::FileExists {
                is_dir,
                remote_path,
                local_path,
            }) => file_exists(app, is_active, target, is_dir, remote_path, local_path),
            Err(
                SftpTransferFailure::Connection { message }
                | SftpTransferFailure::Failed { message },
            ) => finish(
                app,
                is_active,
                target,
                SftpTransferResultStatus::Failed,
                Some(message),
            ),
        }
    })
}

fn file_exists(
    app: &mut App,
    is_active: bool,
    target: &SftpTransferTarget,
    is_dir: bool,
    remote_path: String,
    embedded_local_path: Option<String>,
) -> Task<Message> {
    let already_cancelled = app
        .sftp
        .state
        .transfer_progress
        .as_ref()
        .and_then(|progress| progress.lock().ok())
        .is_some_and(|progress| progress.cancelled);
    if already_cancelled {
        return finish(
            app,
            is_active,
            target,
            SftpTransferResultStatus::Cancelled,
            None,
        );
    }

    let (file_name, total_bytes) = app
        .sftp
        .state
        .transfer_progress
        .as_ref()
        .and_then(|progress| progress.lock().ok())
        .map_or_else(
            || ("unknown".to_string(), 0),
            |progress| (progress.file_name.clone(), progress.total_bytes),
        );
    let Some(host) = conflict_host(app, target) else {
        return conflict_host_missing(app, target);
    };
    let Some((local_path, source_host, source_path)) =
        conflict_source(app, target, &file_name, embedded_local_path)
    else {
        return conflict_local_target(app);
    };
    app.sftp.state.transfer_progress = None;
    let pending = SftpOverwritePending {
        host,
        file_name,
        remote_path,
        source_path,
        local_path,
        source_host,
        total_bytes,
        target: target.clone(),
        in_progress: false,
        is_folder: is_dir,
    };
    if let Some(SftpTransferModal::Discard {
        queued_overwrite, ..
    }) = &mut app.sftp.state.transfer_modal
    {
        *queued_overwrite = Some(pending);
        return Task::none();
    }
    app.sftp.state.show_overwrite_modal(pending);
    if app.sftp.state.overwrite_apply_all == Some(OverwriteApplyAll::Replace) {
        return super::super::update(app, DomainMessage::SftpOverwriteReplace);
    }
    Task::none()
}

fn conflict_host(app: &App, target: &SftpTransferTarget) -> Option<crate::core::data::Host> {
    match target {
        SftpTransferTarget::Remote { .. } => current_remote_host(app, SftpPane::Remote),
        SftpTransferTarget::LeftRemote { .. } => current_remote_host(app, SftpPane::Local),
        SftpTransferTarget::Local { .. } => None,
    }
}

fn conflict_host_missing(app: &mut App, target: &SftpTransferTarget) -> Task<Message> {
    app.sftp.state.transfer_progress = None;
    app.sftp.state.transfer_modal = None;
    let message = match target {
        SftpTransferTarget::Remote { .. } => t!("sftp.no_remote_host").to_string(),
        SftpTransferTarget::LeftRemote { .. } => t!("sftp.error.no_source_remote").to_string(),
        SftpTransferTarget::Local { .. } => t!("sftp.error.cannot_overwrite_local").to_string(),
    };
    Task::perform(async move { message }, Message::ShowToast)
}

fn conflict_source(
    app: &App,
    target: &SftpTransferTarget,
    file_name: &str,
    embedded_local_path: Option<String>,
) -> Option<(Option<PathBuf>, Option<crate::core::data::Host>, String)> {
    match target {
        SftpTransferTarget::Remote { .. } => {
            if app.sftp.state.left.pane.connected {
                Some((
                    None,
                    current_remote_host(app, SftpPane::Local),
                    join_remote_path(&app.sftp.state.left.pane.path, file_name),
                ))
            } else if let Some(path) = embedded_local_path {
                Some((Some(PathBuf::from(path)), None, String::new()))
            } else {
                Some((
                    Some(Path::new(&app.sftp.state.left.pane.path).join(file_name)),
                    None,
                    String::new(),
                ))
            }
        }
        SftpTransferTarget::LeftRemote { .. } => Some((
            None,
            current_remote_host(app, SftpPane::Remote),
            join_remote_path(&app.sftp.state.right.pane.path, file_name),
        )),
        SftpTransferTarget::Local { .. } => None,
    }
}

fn conflict_local_target(app: &mut App) -> Task<Message> {
    app.sftp.state.transfer_progress = None;
    app.sftp.state.transfer_modal = None;
    Task::perform(
        async { t!("sftp.error.cannot_overwrite_local").to_string() },
        Message::ShowToast,
    )
}

fn finish(
    app: &mut App,
    is_active: bool,
    target: &SftpTransferTarget,
    status: SftpTransferResultStatus,
    error: Option<String>,
) -> Task<Message> {
    record_result(app, status, error);
    app.sftp.state.transfer_progress = None;
    app.sftp.state.transfer_modal = None;
    let next = queue::start_next_queued_transfer(app);
    let mut tasks = vec![next];
    if is_active {
        tasks.push(refresh_transfer_target(app, target.clone()));
    }
    Task::batch(tasks)
}

fn record_result(app: &mut App, status: SftpTransferResultStatus, error: Option<String>) {
    let file_name = app
        .sftp
        .state
        .transfer_progress
        .as_ref()
        .and_then(|progress| {
            progress
                .lock()
                .ok()
                .map(|progress| progress.file_name.clone())
        })
        .unwrap_or_else(|| t!("sftp.transfer_tab").to_string());
    app.sftp.state.transfer_results.record(SftpTransferResult {
        file_name,
        status,
        error,
    });
}

#[cfg(test)]
pub(super) fn finish_for_test(
    app: &mut App,
    status: SftpTransferResultStatus,
    error: Option<String>,
) {
    record_result(app, status, error);
}
