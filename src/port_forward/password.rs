//! Password prompts for port-forward attempts.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use iced::Task;
use rust_i18n::t;
use secrecy::{ExposeSecret, SecretString};

use super::state::PortForwardStatus;
use crate::app::App;
use crate::core::data::{Host, HostId, PortForward, PortForwardId};
use crate::core::message::Message;
use crate::ssh_connect::{ConnectionAttempt, SshConnectionState};

#[derive(Debug, Default)]
pub(crate) struct PasswordState {
    pub(super) pending: HashMap<PortForwardId, PendingPassword>,
    pending_saves: HashMap<PortForwardId, PendingPasswordSave>,
    next_request: u64,
}

#[derive(Debug)]
pub(crate) struct PendingPassword {
    pub request: u64,
    pub forward: PortForward,
    pub attempt: ConnectionAttempt,
}

#[derive(Debug)]
struct PendingPasswordSave {
    host_id: HostId,
    password: SecretString,
}

impl PasswordState {
    pub fn prompt(&self) -> Option<&PendingPassword> {
        self.pending.values().min_by_key(|pending| pending.request)
    }

    pub fn has_pending(&self, id: PortForwardId) -> bool {
        self.pending.contains_key(&id)
    }

    pub fn set_input(&mut self, id: PortForwardId, request: u64, input: SecretString) {
        if let Some(pending) = self
            .pending
            .get_mut(&id)
            .filter(|pending| pending.request == request)
        {
            pending.attempt.password_input = input;
        }
    }

    pub fn toggle_visible(&mut self, id: PortForwardId, request: u64) {
        if let Some(pending) = self
            .pending
            .get_mut(&id)
            .filter(|pending| pending.request == request)
        {
            pending.attempt.password_visible = !pending.attempt.password_visible;
        }
    }

    pub(super) fn discard_save(&mut self, id: PortForwardId) {
        self.pending_saves.remove(&id);
    }
}

pub(super) fn start(app: &mut App, host: Host, forward: PortForward) -> Task<Message> {
    let state = &mut app.port_forwards.password;
    let request = state.next_request;
    state.next_request = state.next_request.wrapping_add(1);
    let id = forward.id;
    state.pending_saves.remove(&id);
    let mut attempt = ConnectionAttempt::new(host, None);
    attempt.state = SshConnectionState::WaitingForPassword;
    state.pending.insert(
        id,
        PendingPassword {
            request,
            forward,
            attempt,
        },
    );
    if let Some(runtime) = app.port_forwards.runtime.get_mut(&id) {
        runtime.logs.push(t!("ssh.password_required").to_string());
    }
    Task::none()
}

pub(super) fn submit(app: &mut App, id: PortForwardId, request: u64) -> Task<Message> {
    submit_with(app, id, request, |app, host, forward| {
        super::update::start_worker(app, host, forward)
    })
}

fn submit_with(
    app: &mut App,
    id: PortForwardId,
    request: u64,
    start_worker: impl FnOnce(&mut App, Host, PortForward) -> Task<Message>,
) -> Task<Message> {
    let Some(pending) = app
        .port_forwards
        .password
        .pending
        .get(&id)
        .filter(|pending| pending.request == request)
    else {
        return Task::none();
    };
    if pending.attempt.password_input.expose_secret().is_empty() {
        return Task::none();
    }

    let Some(pending) = app.port_forwards.password.pending.remove(&id) else {
        return Task::none();
    };
    let Some(mut host) = pending.attempt.pending_host.clone() else {
        return Task::none();
    };
    if host.id != HostId(0) {
        app.port_forwards.password.pending_saves.insert(
            id,
            PendingPasswordSave {
                host_id: host.id,
                password: pending.attempt.password_input.clone(),
            },
        );
    }
    host.password = Some(pending.attempt.password_input);
    if let Some(runtime) = app.port_forwards.runtime.get_mut(&id) {
        runtime.logs.push(t!("ssh.log_checking_auth").to_string());
    }
    start_worker(app, host, pending.forward)
}

pub(super) fn persist_after_start(
    app: &mut App,
    id: PortForwardId,
    success: bool,
) -> Task<Message> {
    let Some(save) = app.port_forwards.password.pending_saves.remove(&id) else {
        return Task::none();
    };
    if !success {
        return Task::none();
    }
    let Some(stored) = app
        .hosts
        .list
        .iter_mut()
        .find(|host| host.id == save.host_id)
    else {
        return Task::none();
    };
    stored.password = Some(save.password);
    let host = stored.clone();
    app.storage_write_task(move |storage| storage.upsert_host(&host))
}

pub(super) fn cancel(app: &mut App, id: PortForwardId, request: u64) -> Task<Message> {
    let matches = app
        .port_forwards
        .password
        .pending
        .get(&id)
        .is_some_and(|pending| pending.request == request);
    if matches {
        app.port_forwards.password.pending.remove(&id);
        app.port_forwards.password.discard_save(id);
        if let Some(runtime) = app.port_forwards.runtime.get_mut(&id) {
            runtime.status = PortForwardStatus::Stopped;
            runtime.logs.push(t!("port_forward.stopped").to_string());
        }
    }
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::PortForwardKind;
    use crate::core::test_support::{app, host};
    use crate::port_forward::message::Message as DomainMessage;
    use crate::port_forward::update::update;

    fn waiting_app() -> (App, PortForwardId, u64) {
        let mut app = app();
        let id = PortForwardId(1);
        let mut target = host(1, "password target", "password-target.invalid");
        target.password = None;
        target.expected_fingerprint = Some("SHA256:trusted".into());
        app.hosts.list.push(target);
        app.port_forwards.list.push(PortForward {
            id,
            name: "password tunnel".into(),
            kind: PortForwardKind::Local,
            host_id: HostId(1),
            bind_address: "127.0.0.1".into(),
            listen_port: 0,
            dest_host: "127.0.0.1".into(),
            dest_port: 80,
        });
        let _ = update(&mut app, DomainMessage::PortForwardStart(id));
        let request = app.port_forwards.password.pending[&id].request;
        (app, id, request)
    }

    #[test]
    fn password_auth_without_a_saved_password_waits_for_input() {
        let (app, id, _request) = waiting_app();

        assert!(app.port_forwards.password.prompt().is_some());
        assert!(app.has_any_dialog_open());
        assert_eq!(
            app.port_forwards.status_for(id),
            PortForwardStatus::Starting
        );
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn submitting_the_password_starts_the_forward_with_that_secret() {
        let (mut app, id, request) = waiting_app();
        let password = SecretString::from("typed secret");
        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordChanged {
                id,
                request,
                value: password,
            },
        );

        let mut submitted = None;
        let _ = submit_with(&mut app, id, request, |_, target, forward| {
            submitted = Some((target, forward));
            Task::none()
        });

        let (target, forward) = submitted.expect("the worker receives the authenticated host");
        assert_eq!(forward.id, id);
        assert_eq!(
            target.password.as_ref().map(ExposeSecret::expose_secret),
            Some("typed secret")
        );
        assert!(app.port_forwards.password.prompt().is_none());
    }

    #[test]
    fn successful_start_writes_the_password_back_to_the_saved_host() {
        let (mut app, id, request) = waiting_app();
        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordChanged {
                id,
                request,
                value: SecretString::from("typed secret"),
            },
        );
        let _ = submit_with(&mut app, id, request, |_, _, _| Task::none());
        assert!(app.hosts.list[0].password.is_none());

        let _ = persist_after_start(&mut app, id, true);

        assert_eq!(
            app.hosts.list[0]
                .password
                .as_ref()
                .map(ExposeSecret::expose_secret),
            Some("typed secret")
        );
        assert!(app.port_forwards.password.pending_saves.is_empty());
    }

    #[test]
    fn failed_start_discards_the_password_instead_of_saving_it() {
        let (mut app, id, request) = waiting_app();
        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordChanged {
                id,
                request,
                value: SecretString::from("wrong secret"),
            },
        );
        let _ = submit_with(&mut app, id, request, |_, _, _| Task::none());

        let _ = persist_after_start(&mut app, id, false);

        assert!(app.hosts.list[0].password.is_none());
        assert!(app.port_forwards.password.pending_saves.is_empty());
    }

    #[test]
    fn stopping_an_attempt_discards_its_pending_password_save() {
        let (mut app, id, request) = waiting_app();
        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordChanged {
                id,
                request,
                value: SecretString::from("unsaved secret"),
            },
        );
        let _ = submit_with(&mut app, id, request, |_, _, _| Task::none());
        assert!(!app.port_forwards.password.pending_saves.is_empty());

        let _ = update(&mut app, DomainMessage::PortForwardStop(id));

        assert!(app.port_forwards.password.pending_saves.is_empty());
        assert!(app.hosts.list[0].password.is_none());
    }

    #[test]
    fn the_password_dialog_routes_connect_and_cancel_for_the_current_attempt() {
        let (mut app, id, request) = waiting_app();
        app.port_forwards
            .password
            .pending
            .get_mut(&id)
            .unwrap()
            .attempt
            .password_input = SecretString::from("typed secret");
        app.confirm_dialog_anim = 1.0;

        let mut ui = iced_test::simulator(crate::views::view(&app));
        ui.find(t!("ssh.password_required").to_string())
            .expect("the password dialog title is visible");
        ui.find(
            t!(
                "port_forward.password_body",
                name = "password tunnel",
                target = "root@password-target.invalid:22"
            )
            .to_string(),
        )
        .expect("the body identifies the pending attempt");
        ui.click(t!("port_forward.connect").to_string())
            .expect("connect is clickable");
        assert!(ui.into_messages().any(|message| matches!(
            message,
            Message::PortForward(DomainMessage::PortForwardPasswordSubmit {
                id: target,
                request: attempt,
            }) if target == id && attempt == request
        )));

        let mut ui = iced_test::simulator(crate::views::view(&app));
        ui.click(t!("settings.cancel").to_string())
            .expect("cancel is clickable");
        assert!(ui.into_messages().any(|message| matches!(
            message,
            Message::PortForward(DomainMessage::PortForwardPasswordCancel {
                id: target,
                request: attempt,
            }) if target == id && attempt == request
        )));
    }

    #[test]
    fn empty_password_keeps_the_prompt_open() {
        let (mut app, id, request) = waiting_app();
        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordSubmit { id, request },
        );

        assert!(app.port_forwards.password.has_pending(id));
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn cancelling_the_prompt_stops_the_attempt() {
        let (mut app, id, request) = waiting_app();
        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordCancel { id, request },
        );

        assert!(app.port_forwards.password.prompt().is_none());
        assert_eq!(app.port_forwards.status_for(id), PortForwardStatus::Stopped);
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn stale_password_messages_cannot_change_a_new_attempt() {
        let (mut app, id, old_request) = waiting_app();
        let _ = update(&mut app, DomainMessage::PortForwardStop(id));
        let _ = update(&mut app, DomainMessage::PortForwardStart(id));
        let request = app.port_forwards.password.pending[&id].request;
        assert_ne!(request, old_request);

        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordChanged {
                id,
                request: old_request,
                value: SecretString::from("stale secret"),
            },
        );
        let _ = update(
            &mut app,
            DomainMessage::PortForwardPasswordCancel {
                id,
                request: old_request,
            },
        );

        assert!(app.port_forwards.password.has_pending(id));
        assert!(
            app.port_forwards.password.pending[&id]
                .attempt
                .password_input
                .expose_secret()
                .is_empty()
        );
    }

    #[test]
    fn known_key_auth_does_not_require_a_password() {
        let mut app = app();
        let id = PortForwardId(1);
        let mut target = host(1, "key target", "key-target.invalid");
        target.auth_type = crate::core::data::AuthType::Key;
        target.password = None;
        app.hosts.list.push(target);
        app.port_forwards.list.push(PortForward {
            id,
            name: "key tunnel".into(),
            kind: PortForwardKind::Local,
            host_id: HostId(1),
            bind_address: "127.0.0.1".into(),
            listen_port: 0,
            dest_host: "127.0.0.1".into(),
            dest_port: 80,
        });

        let _ = update(&mut app, DomainMessage::PortForwardStart(id));

        assert!(app.port_forwards.password.prompt().is_none());
    }
}
