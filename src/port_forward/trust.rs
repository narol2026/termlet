//! Host-key confirmation before a port-forward worker can authenticate.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use iced::Task;
use rust_i18n::t;

use crate::app::App;
use crate::core::data::{Host, PortForward, PortForwardId, ServerKeyInfo};
use crate::core::message::Message;
use crate::ssh::known_hosts_file;
use crate::ssh_connect::{ConnectionAttempt, SshConnectionState};

use super::message::Message as DomainMessage;
use super::state::PortForwardStatus;

#[derive(Debug, Default)]
pub(crate) struct TrustState {
    pub pending: HashMap<PortForwardId, PendingTrust>,
    next_request: u64,
}

#[derive(Debug)]
pub(crate) struct PendingTrust {
    pub request: u64,
    pub forward: PortForward,
    pub attempt: ConnectionAttempt,
}

impl TrustState {
    pub fn confirmation(&self) -> Option<&PendingTrust> {
        self.pending
            .values()
            .filter(|pending| pending.attempt.pending_key_info.is_some())
            .min_by_key(|pending| pending.request)
    }

    pub fn matches(&self, id: PortForwardId, request: u64) -> bool {
        self.pending
            .get(&id)
            .is_some_and(|pending| pending.request == request)
    }
}

pub(super) fn start(app: &mut App, host: Host, forward: PortForward) -> Task<Message> {
    if host.expected_fingerprint.is_some() || known_hosts_file::is_known(&host.hostname, host.port)
    {
        return super::update::begin_authentication(app, host, forward);
    }

    let trust = &mut app.port_forwards.trust;
    let request = trust.next_request;
    trust.next_request = trust.next_request.wrapping_add(1);
    let id = forward.id;
    let hostname = host.hostname.clone();
    let port = host.port;
    trust.pending.insert(
        id,
        PendingTrust {
            request,
            forward,
            attempt: ConnectionAttempt::new(host, None),
        },
    );

    // Probing aborts the SSH handshake before any credentials are sent.
    Task::perform(crate::ssh::probe_host_key(hostname, port), move |result| {
        Message::PortForward(DomainMessage::PortForwardKeyProbed {
            id,
            request,
            result,
        })
    })
}

pub(super) fn probe_finished(
    app: &mut App,
    id: PortForwardId,
    request: u64,
    result: Result<ServerKeyInfo, String>,
) -> Task<Message> {
    if !app.port_forwards.trust.matches(id, request) {
        return Task::none();
    }
    match result {
        Ok(key_info) => {
            if let Some(pending) = app.port_forwards.trust.pending.get_mut(&id) {
                pending.attempt.pending_key_info = Some(key_info);
                pending.attempt.state = SshConnectionState::WaitingForKeyConfirmation;
            }
            if let Some(runtime) = app.port_forwards.runtime.get_mut(&id) {
                runtime.logs.push(t!("ssh.log_waiting_key").to_string());
            }
        }
        Err(error) => {
            app.port_forwards.trust.pending.remove(&id);
            if let Some(runtime) = app.port_forwards.runtime.get_mut(&id) {
                runtime.status = PortForwardStatus::Failed;
                runtime.logs.push(error.clone());
            }
            return Task::done(Message::ShowErrorToast(error));
        }
    }
    Task::none()
}

pub(super) fn confirm(app: &mut App, id: PortForwardId, request: u64, save: bool) -> Task<Message> {
    confirm_with(
        app,
        id,
        request,
        save,
        |key| known_hosts_file::append(&key.hostname, key.port, &key.public_key),
        super::update::begin_authentication,
    )
}

fn confirm_with(
    app: &mut App,
    id: PortForwardId,
    request: u64,
    save: bool,
    persist: impl FnOnce(&ServerKeyInfo) -> Result<(), String>,
    start_worker: impl FnOnce(&mut App, Host, PortForward) -> Task<Message>,
) -> Task<Message> {
    let key_info = {
        let Some(pending) = app.port_forwards.trust.pending.get(&id) else {
            return Task::none();
        };
        if pending.request != request {
            return Task::none();
        }
        let Some(key_info) = pending.attempt.pending_key_info.as_ref() else {
            return Task::none();
        };
        key_info.clone()
    };
    if save && let Err(error) = persist(&key_info) {
        // Keep the prompt open: failing to save is not permission to continue.
        return Task::done(Message::ShowErrorToast(error));
    }

    let Some(pending) = app.port_forwards.trust.pending.remove(&id) else {
        return Task::none();
    };
    // Pin the exact key the user saw, including when it was also saved to disk.
    // A changed key between the probe and the real handshake must be rejected.
    let Some(mut host) = pending.attempt.pending_host else {
        return Task::none();
    };
    host.expected_fingerprint = Some(key_info.fingerprint.clone());
    let start = start_worker(app, host, pending.forward);
    if save {
        Task::batch([start, crate::known_hosts::update::reload_known_hosts_task()])
    } else {
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::{HostId, PortForwardKind};
    use crate::core::test_support::{app, drain, host};
    use crate::port_forward::update::update;

    fn start_unknown(kind: PortForwardKind) -> (App, PortForwardId, u64) {
        let mut app = app();
        let id = PortForwardId(1);
        let mut target = host(1, "unknown", "port-forward-trust-test.invalid");
        target.port = 7190;
        app.hosts.list.push(target);
        app.port_forwards.list.push(PortForward {
            id,
            name: "test tunnel".into(),
            kind,
            host_id: HostId(1),
            bind_address: "127.0.0.1".into(),
            listen_port: 0,
            dest_host: "127.0.0.1".into(),
            dest_port: 80,
        });
        let _ = update(&mut app, DomainMessage::PortForwardStart(id));
        let request = app.port_forwards.trust.pending[&id].request;
        (app, id, request)
    }

    fn key() -> ServerKeyInfo {
        ServerKeyInfo {
            hostname: "port-forward-trust-test.invalid".into(),
            port: 7190,
            key_type: "ssh-ed25519".into(),
            public_key: "test public key".into(),
            fingerprint: "SHA256:confirmed".into(),
        }
    }

    #[test]
    fn every_forward_kind_waits_for_trust_before_starting_a_worker() {
        for kind in [
            PortForwardKind::Local,
            PortForwardKind::Remote,
            PortForwardKind::Dynamic,
        ] {
            let (mut app, id, request) = start_unknown(kind);
            assert!(!app.port_forward_manager.signal(id));
            assert!(app.port_forwards.trust.confirmation().is_none());

            let _ = update(
                &mut app,
                DomainMessage::PortForwardKeyProbed {
                    id,
                    request,
                    result: Ok(key()),
                },
            );
            assert!(app.port_forwards.trust.confirmation().is_some());
            assert!(app.has_any_dialog_open());
            assert_eq!(
                app.port_forwards.status_for(id),
                PortForwardStatus::Starting
            );
            assert!(!app.port_forward_manager.signal(id));
        }
    }

    #[test]
    fn first_time_password_target_prompts_for_the_key_then_the_password() {
        let mut app = app();
        let id = PortForwardId(1);
        let mut target = host(1, "unknown", "port-forward-trust-test.invalid");
        target.port = 7190;
        target.password = None;
        app.hosts.list.push(target);
        app.port_forwards.list.push(PortForward {
            id,
            name: "test tunnel".into(),
            kind: PortForwardKind::Local,
            host_id: HostId(1),
            bind_address: "127.0.0.1".into(),
            listen_port: 0,
            dest_host: "127.0.0.1".into(),
            dest_port: 80,
        });
        let _ = update(&mut app, DomainMessage::PortForwardStart(id));
        let request = app.port_forwards.trust.pending[&id].request;
        let _ = update(
            &mut app,
            DomainMessage::PortForwardKeyProbed {
                id,
                request,
                result: Ok(key()),
            },
        );
        assert!(app.port_forwards.trust.confirmation().is_some());
        assert!(app.port_forwards.password.prompt().is_none());

        let _ = update(
            &mut app,
            DomainMessage::PortForwardKeyConfirm {
                id,
                request,
                save: false,
            },
        );

        assert!(app.port_forwards.trust.confirmation().is_none());
        assert!(app.port_forwards.password.prompt().is_some());
        assert_eq!(
            app.port_forwards
                .password
                .prompt()
                .unwrap()
                .attempt
                .pending_host
                .as_ref()
                .unwrap()
                .expected_fingerprint
                .as_deref(),
            Some("SHA256:confirmed")
        );
        assert_eq!(
            app.port_forwards.status_for(id),
            PortForwardStatus::Starting
        );
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn cancelled_attempts_cannot_confirm_or_overwrite_a_new_attempt() {
        let (mut app, id, old_request) = start_unknown(PortForwardKind::Local);
        let _ = update(&mut app, DomainMessage::PortForwardStop(id));
        assert_eq!(app.port_forwards.status_for(id), PortForwardStatus::Stopped);
        let _ = update(&mut app, DomainMessage::PortForwardStart(id));
        let request = app.port_forwards.trust.pending[&id].request;
        assert_ne!(request, old_request);

        for message in [
            DomainMessage::PortForwardKeyProbed {
                id,
                request: old_request,
                result: Ok(key()),
            },
            DomainMessage::PortForwardKeyProbed {
                id,
                request: old_request,
                result: Err("old error".into()),
            },
            DomainMessage::PortForwardKeyConfirm {
                id,
                request: old_request,
                save: false,
            },
            DomainMessage::PortForwardKeyCancel {
                id,
                request: old_request,
            },
        ] {
            let _ = update(&mut app, message);
        }
        assert!(app.port_forwards.trust.matches(id, request));
        assert!(app.port_forwards.trust.confirmation().is_none());
        assert_eq!(
            app.port_forwards.status_for(id),
            PortForwardStatus::Starting
        );
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn deleted_forwards_ignore_late_probe_results() {
        let (mut app, id, request) = start_unknown(PortForwardKind::Remote);
        let _ = update(&mut app, DomainMessage::PortForwardDelete(id));
        let _ = probe_finished(&mut app, id, request, Ok(key()));
        assert!(app.port_forwards.trust.confirmation().is_none());
        assert!(app.port_forwards.runtime_for(id).is_none());
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn probe_failures_fail_the_forward_without_authenticating() {
        let (mut app, id, request) = start_unknown(PortForwardKind::Dynamic);
        let messages = drain(probe_finished(
            &mut app,
            id,
            request,
            Err("probe failed".into()),
        ))
        .messages;
        assert!(
            matches!(messages.as_slice(), [Message::ShowErrorToast(error)] if error == "probe failed")
        );
        assert_eq!(app.port_forwards.status_for(id), PortForwardStatus::Failed);
        assert!(app.port_forwards.trust.pending.is_empty());
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn confirmation_without_a_probed_key_does_not_start_a_worker() {
        let (mut app, id, request) = start_unknown(PortForwardKind::Local);
        let _ = confirm(&mut app, id, request, false);
        assert!(app.port_forwards.trust.matches(id, request));
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn failing_to_save_keeps_the_confirmation_open_without_authenticating() {
        let (mut app, id, request) = start_unknown(PortForwardKind::Remote);
        let _ = probe_finished(&mut app, id, request, Ok(key()));
        let messages = drain(confirm_with(
            &mut app,
            id,
            request,
            true,
            |key| {
                assert_eq!(key.port, 7190);
                Err("disk full".into())
            },
            |_, _, _| panic!("saving must succeed before authentication"),
        ))
        .messages;
        assert!(
            matches!(messages.as_slice(), [Message::ShowErrorToast(error)] if error == "disk full")
        );
        assert!(app.port_forwards.trust.confirmation().is_some());
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn both_trust_choices_pin_the_confirmed_key_only_on_the_worker_host() {
        for save in [false, true] {
            let (mut app, id, request) = start_unknown(PortForwardKind::Local);
            let _ = probe_finished(&mut app, id, request, Ok(key()));
            let mut saved = false;
            let mut started = false;
            let _ = confirm_with(
                &mut app,
                id,
                request,
                save,
                |key| {
                    assert_eq!(key.hostname, "port-forward-trust-test.invalid");
                    assert_eq!(key.port, 7190);
                    assert_eq!(key.public_key, "test public key");
                    saved = true;
                    Ok(())
                },
                |_, target, forward| {
                    assert_eq!(
                        target.expected_fingerprint.as_deref(),
                        Some("SHA256:confirmed")
                    );
                    assert_eq!(target.hostname, "port-forward-trust-test.invalid");
                    assert_eq!(target.port, 7190);
                    assert_eq!(forward.id, id);
                    started = true;
                    Task::none()
                },
            );
            assert_eq!(saved, save);
            assert!(started);
            assert!(app.port_forwards.trust.pending.is_empty());
            assert!(app.hosts.list[0].expected_fingerprint.is_none());
        }
    }

    #[test]
    fn the_real_confirmation_dialog_routes_each_trust_choice_and_cancel() {
        let (mut app, id, request) = start_unknown(PortForwardKind::Local);
        let _ = crate::update::update(
            &mut app,
            Message::PortForward(DomainMessage::PortForwardKeyProbed {
                id,
                request,
                result: Ok(key()),
            }),
        );
        app.confirm_dialog_anim = 1.0;
        for (label, save) in [
            (t!("ssh.continue"), false),
            (t!("ssh.add_and_continue"), true),
        ] {
            let mut ui = iced_test::simulator(crate::views::view(&app));
            ui.find(t!("ssh.host_label").to_string())
                .expect("the host label is visible");
            ui.find("port-forward-trust-test.invalid:7190")
                .expect("the SSH address is visible");
            ui.find("SHA256:confirmed")
                .expect("the complete fingerprint is visible");
            ui.click(label.to_string())
                .expect("trust action is clickable");
            assert!(ui.into_messages().any(|message| matches!(message,
                Message::PortForward(DomainMessage::PortForwardKeyConfirm {
                    id: target, request: attempt, save: persist,
                }) if target == id && attempt == request && persist == save
            )));
        }
        let mut ui = iced_test::simulator(crate::views::view(&app));
        ui.click(t!("settings.cancel").to_string())
            .expect("cancel is clickable");
        let messages: Vec<_> = ui.into_messages().collect();
        for message in messages {
            let _ = crate::update::update(&mut app, message);
        }
        assert!(app.port_forwards.trust.confirmation().is_none());
        assert_eq!(app.port_forwards.status_for(id), PortForwardStatus::Stopped);
        assert!(!app.port_forward_manager.signal(id));
    }

    #[test]
    fn confirmation_owns_the_keyboard_and_escape_cancels_the_attempt() {
        let (mut app, id, request) = start_unknown(PortForwardKind::Local);
        let _ = crate::update::update(
            &mut app,
            Message::PortForward(DomainMessage::PortForwardKeyProbed {
                id,
                request,
                result: Ok(key()),
            }),
        );
        assert_eq!(
            app.focus.top(),
            Some(crate::focus::FocusId::Dialog(
                crate::focus::DialogId::PortForwardKeyConfirmation { id, request },
            ))
        );
        let key = iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape);
        let crate::focus::KeyRoute::Handle(task) =
            crate::focus::handle_key(&mut app, &key, iced::keyboard::Modifiers::empty())
        else {
            panic!("confirmation handles Escape");
        };
        for message in drain(task).messages {
            let _ = crate::update::update(&mut app, message);
        }
        assert!(app.port_forwards.trust.confirmation().is_none());
        assert_eq!(app.port_forwards.status_for(id), PortForwardStatus::Stopped);
        assert!(!app.port_forward_manager.signal(id));
    }
}
