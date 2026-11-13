//! Port-forward lifecycle, authentication, and conflict handling.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use iced::Task;
use tokio::sync::Notify;

use crate::app::App;
use crate::core::data::{AuthType, Host, HostId, LogCategory, PortForward, PortForwardId};
use crate::core::message::Message;
use crate::port_forward::message::Message as DomainMessage;
use crate::port_forward::state::PortForwardStatus;
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::PortForwardStart(id) => {
            let name = forward_name(app, id);
            app.add_log(
                LogCategory::PortForward,
                format!("Starting port forward \"{name}\""),
            );
            return start(app, id);
        }
        DomainMessage::PortForwardKeyProbed {
            id,
            request,
            result,
        } => return super::super::trust::probe_finished(app, id, request, result),
        DomainMessage::PortForwardKeyConfirm { id, request, save } => {
            return super::super::trust::confirm(app, id, request, save);
        }
        DomainMessage::PortForwardKeyCancel { id, request }
            if app.port_forwards.trust.matches(id, request) =>
        {
            stop_by_id(app, id);
        }
        DomainMessage::PortForwardPasswordChanged { id, request, value } => {
            app.port_forwards.password.set_input(id, request, value);
        }
        DomainMessage::PortForwardPasswordToggleVisible { id, request } => {
            app.port_forwards.password.toggle_visible(id, request);
        }
        DomainMessage::PortForwardPasswordSubmit { id, request } => {
            return super::super::password::submit(app, id, request);
        }
        DomainMessage::PortForwardPasswordCancel { id, request } => {
            return super::super::password::cancel(app, id, request);
        }
        DomainMessage::PortForwardStop(id) => {
            let name = forward_name(app, id);
            app.add_log(
                LogCategory::PortForward,
                format!("Stopped port forward \"{name}\""),
            );
            stop_by_id(app, id);
        }
        DomainMessage::PortForwardToggle(id) => {
            if matches!(
                app.port_forwards.status_for(id),
                PortForwardStatus::Running | PortForwardStatus::Starting
            ) {
                stop_by_id(app, id);
            } else {
                return start(app, id);
            }
        }
        DomainMessage::PortForwardStarted { id, success, logs } => {
            return started(app, id, success, logs);
        }
        DomainMessage::PortForwardStopped {
            id,
            generation,
            logs,
        } => stopped(app, id, generation, logs),
        _ => {}
    }
    Task::none()
}

fn forward_name(app: &App, id: PortForwardId) -> String {
    app.port_forwards
        .list
        .iter()
        .find(|forward| forward.id == id)
        .map(|forward| forward.name.clone())
        .unwrap_or_default()
}

fn start(app: &mut App, id: PortForwardId) -> Task<Message> {
    let Some(forward) = app
        .port_forwards
        .list
        .iter()
        .find(|forward| forward.id == id)
        .cloned()
    else {
        return Task::none();
    };
    let Some(host) = app
        .hosts
        .list
        .iter()
        .find(|host| host.id == forward.host_id)
        .cloned()
    else {
        let runtime = app.port_forwards.runtime.entry(id).or_default();
        runtime.status = PortForwardStatus::Failed;
        runtime
            .logs
            .push(t!("port_forward.target_host_not_found").to_string());
        return Task::done(Message::ShowErrorToast(format!(
            "Port forward \"{}\" failed: host not found",
            forward.name
        )));
    };
    if matches!(
        app.port_forwards.status_for(id),
        PortForwardStatus::Running | PortForwardStatus::Starting
    ) {
        return Task::none();
    }
    if let Some(conflict) = detect_port_conflict(app, &forward, id) {
        let runtime = app.port_forwards.runtime.entry(id).or_default();
        runtime.status = PortForwardStatus::Failed;
        runtime.logs.push(conflict.clone());
        return Task::done(Message::ShowErrorToast(format!(
            "Port forward \"{}\" failed: {conflict}",
            forward.name
        )));
    }

    let runtime = app.port_forwards.runtime.entry(id).or_default();
    runtime.status = PortForwardStatus::Starting;
    runtime.logs.clear();
    runtime.logs.push(format!(
        "Starting {} forward: {}",
        forward.kind,
        forward.human_summary()
    ));
    super::super::trust::start(app, host, forward)
}

pub(crate) fn start_worker(app: &mut App, host: Host, forward: PortForward) -> Task<Message> {
    let id = forward.id;
    let shutdown = Arc::new(Notify::new());
    let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
    let (stopped_sender, stopped_receiver) = tokio::sync::oneshot::channel();
    let generation = app.port_forward_manager.spawn(
        id,
        Arc::clone(&shutdown),
        super::worker::run(host, forward, shutdown, started_sender, stopped_sender),
    );
    let started = Task::perform(started_receiver, move |result| {
        let (success, logs) = result.unwrap_or_else(|_| {
            (
                false,
                vec![t!("port_forward.worker_terminated").to_string()],
            )
        });
        Message::PortForward(DomainMessage::PortForwardStarted { id, success, logs })
    });
    let stopped = Task::perform(stopped_receiver, move |result| {
        Message::PortForward(DomainMessage::PortForwardStopped {
            id,
            generation,
            logs: result.unwrap_or_else(|_| vec![t!("port_forward.worker_dropped").to_string()]),
        })
    });
    Task::batch([started, stopped])
}

pub(crate) fn begin_authentication(
    app: &mut App,
    host: Host,
    forward: PortForward,
) -> Task<Message> {
    if host.auth_type == AuthType::Password && !host.has_password() {
        super::super::password::start(app, host, forward)
    } else {
        start_worker(app, host, forward)
    }
}

fn started(app: &mut App, id: PortForwardId, success: bool, logs: Vec<String>) -> Task<Message> {
    let name = forward_name(app, id);
    let runtime = app.port_forwards.runtime.entry(id).or_default();
    runtime.logs.extend(logs);
    runtime.status = if success {
        PortForwardStatus::Running
    } else {
        PortForwardStatus::Failed
    };
    let password_save = super::super::password::persist_after_start(app, id, success);
    if success {
        return Task::batch([
            Task::done(Message::ShowToast(format!(
                "Port forward \"{name}\" connected"
            ))),
            password_save,
        ]);
    }
    Task::done(Message::ShowErrorToast(format!(
        "Port forward \"{name}\" failed to connect"
    )))
}

fn stopped(app: &mut App, id: PortForwardId, generation: u64, logs: Vec<String>) {
    if app.port_forward_manager.remove(id, generation).is_some()
        && let Some(runtime) = app.port_forwards.runtime.get_mut(&id)
    {
        runtime.logs.extend(logs);
        if runtime.status != PortForwardStatus::Failed {
            runtime.status = PortForwardStatus::Stopped;
        }
    }
}

pub(super) fn stop_by_id(app: &mut App, id: PortForwardId) {
    if app.port_forwards.trust.pending.remove(&id).is_some() {
        mark_stopped(app, id);
        return;
    }
    if app.port_forwards.password.pending.remove(&id).is_some() {
        app.port_forwards.password.discard_save(id);
        mark_stopped(app, id);
        return;
    }
    app.port_forwards.password.discard_save(id);
    let Some(runtime) = app.port_forwards.runtime.get_mut(&id) else {
        return;
    };
    if app.port_forward_manager.signal(id) {
        runtime.status = PortForwardStatus::Stopping;
        runtime
            .logs
            .push(t!("port_forward.stop_requested").to_string());
    }
}

fn mark_stopped(app: &mut App, id: PortForwardId) {
    if let Some(runtime) = app.port_forwards.runtime.get_mut(&id) {
        runtime.status = PortForwardStatus::Stopped;
        runtime.logs.push(t!("port_forward.stopped").to_string());
    }
}

pub fn stop_all(app: &mut App) {
    let ids: Vec<PortForwardId> = app.port_forwards.runtime.keys().copied().collect();
    for id in ids {
        stop_by_id(app, id);
    }
}

/// Remove forwards whose target host is being deleted.
///
/// Database rows are removed by the foreign-key cascade, so this only
/// synchronizes workers and live application state.
pub fn delete_forwards_for_host(app: &mut App, host_id: HostId) {
    let forwards: Vec<(PortForwardId, String)> = app
        .port_forwards
        .list
        .iter()
        .filter(|forward| forward.host_id == host_id)
        .map(|forward| (forward.id, forward.name.clone()))
        .collect();
    for (id, name) in forwards {
        stop_by_id(app, id);
        app.port_forwards.list.retain(|forward| forward.id != id);
        app.port_forwards.runtime.remove(&id);
        if app.port_forwards.cursor.selected_key() == Some(&id) {
            app.port_forwards.cursor.clear();
            app.port_forwards.logs_open = false;
        }
        if app.port_forwards.delete_pending == Some(id) {
            app.port_forwards.delete_pending = None;
        }
        app.add_log(
            LogCategory::PortForward,
            t!("port_forward.deleted_log", name = &name).to_string(),
        );
    }
}

fn detect_port_conflict(
    app: &App,
    forward: &PortForward,
    current_id: PortForwardId,
) -> Option<String> {
    let bind = super::worker::bind_address_for_listen(&forward.bind_address);
    let listen_addr = format!("{bind}:{}", forward.listen_port);
    for (other_id, runtime) in &app.port_forwards.runtime {
        if *other_id == current_id
            || !matches!(
                runtime.status,
                PortForwardStatus::Running | PortForwardStatus::Starting
            )
        {
            continue;
        }
        let other = app
            .port_forwards
            .list
            .iter()
            .find(|candidate| candidate.id == *other_id)?;
        let other_bind = super::worker::bind_address_for_listen(&other.bind_address);
        let other_addr = format!("{other_bind}:{}", other.listen_port);
        if listen_addr == other_addr {
            return Some(format!(
                "Port {listen_addr} is already in use by forward \"{}\" (#{other_id})",
                other.name
            ));
        }
        if forward.listen_port == other.listen_port {
            let current_wildcard = is_wildcard(bind);
            let other_wildcard = is_wildcard(other_bind);
            if current_wildcard || other_wildcard {
                return Some(format!(
                    "Port {} is already in use by forward \"{}\" (#{other_id}) on {other_bind}:{}",
                    forward.listen_port, other.name, other.listen_port
                ));
            }
        }
    }
    check_system_port_available(&listen_addr)
}

fn is_wildcard(bind: &str) -> bool {
    bind.is_empty() || matches!(bind, "0.0.0.0" | "::" | "*")
}

pub(super) fn check_system_port_available(listen_addr: &str) -> Option<String> {
    match std::net::TcpListener::bind(listen_addr) {
        Ok(listener) => {
            drop(listener);
            None
        }
        Err(error) => Some(format!(
            "Cannot bind {listen_addr}: {error}. The port may be in use by another application."
        )),
    }
}
