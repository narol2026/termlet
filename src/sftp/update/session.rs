use iced::Task;

use crate::app::App;
use crate::core::data::{AuthType, Host, HostId, LogCategory, ServerKeyInfo};
use crate::core::ids::SftpSessionId;
use crate::core::message::Message;
use crate::sftp::message::Message as DomainMessage;
use crate::sftp::state::HostPickerState;
use crate::sftp::{SftpPane, SftpSession, SftpSidebarContextMenu, SftpState};
use crate::ssh::known_hosts_file;
use crate::ssh_connect::SshConnectionState;
use rust_i18n::t;
use secrecy::{ExposeSecret, SecretString};

use super::browse::list_remote_pane_home;
use super::shared::{activate_pane, clear_selection, current_remote_host, request_local_dir_load};

fn resolve_jump_host(app: &App, host: &Host) -> Option<Host> {
    host.jump_host_id.and_then(|jump_host_id| {
        app.hosts
            .list
            .iter()
            .find(|candidate| candidate.id == jump_host_id)
            .cloned()
    })
}

fn sftp_jump_stage(app: &App, pane: SftpPane) -> bool {
    app.sftp.state.pane(pane).attempt.jump_stage()
}

fn sftp_pending_jump_host(app: &App, pane: SftpPane) -> Option<Host> {
    app.sftp.state.pane(pane).attempt.jump_host.clone()
}

fn sftp_pending_jump_host_mut(app: &mut App, pane: SftpPane) -> Option<&mut Host> {
    app.sftp.state.pane_mut(pane).attempt.jump_host.as_mut()
}

fn set_sftp_jump_done(app: &mut App, pane: SftpPane) {
    app.sftp.state.pane_mut(pane).attempt.stage = crate::ssh_connect::SshConnectStage::Target;
}

fn create_sftp_session(app: &mut App) -> SftpSessionId {
    let id = SftpSessionId(app.sftp.next_session_id as u32);
    app.sftp.next_session_id += 1;
    let label = format!("{} {}", t!("sftp.new_sftp"), id);
    app.sftp.sessions.push(SftpSession {
        id,
        label,
        state: SftpState::default(),
    });
    id
}

fn close_sftp_session(app: &mut App, session_id: SftpSessionId) -> Task<Message> {
    // Never close the last remaining session.
    if app.sftp.sessions.len() <= 1 {
        return Task::none();
    }

    // Cancel any in-progress transfer for this session.
    if let Some(session) = app.sftp.sessions.iter_mut().find(|s| s.id == session_id) {
        session.state.cancel_active_transfer();
    }

    let was_active = app.sftp.active_session == session_id;

    app.sftp.sessions.retain(|s| s.id != session_id);

    if was_active {
        // Switch to another session — pick the last remaining one.
        let new_id = app.sftp.sessions.last().map_or(SftpSessionId(0), |s| s.id);
        app.load_sftp_state(new_id);
    }

    Task::none()
}

fn duplicate_sftp_session(app: &mut App, session_id: SftpSessionId) -> Task<Message> {
    // Save the currently active state before cloning, so that if the user
    // is duplicating the active tab, we capture its latest live state.
    app.save_active_sftp_state();

    let Some(source_session) = app.sftp.sessions.iter().find(|s| s.id == session_id) else {
        return Task::none();
    };

    // Clone the state, but clear transfer progress/queue/status for the new tab.
    let mut new_state = source_session.state.clone();
    new_state.transfer_progress = None;
    new_state.transfer_queue.clear();
    new_state.transfer_results.clear();
    new_state.transfer_status = None;

    let base_label = if source_session.label.ends_with(" (copy)") {
        source_session.label.clone()
    } else {
        format!("{} (copy)", source_session.label)
    };

    // Ensure the label is unique by appending a counter if needed.
    let label = {
        let mut candidate = base_label;
        let mut counter = 2;
        while app.sftp.sessions.iter().any(|s| s.label == candidate) {
            candidate = format!("{} (copy) {}", source_session.label, counter);
            counter += 1;
        }
        candidate
    };

    let new_id = SftpSessionId(app.sftp.next_session_id as u32);
    app.sftp.next_session_id += 1;

    app.sftp.sessions.push(SftpSession {
        id: new_id,
        label,
        state: new_state,
    });

    app.load_sftp_state(new_id);

    Task::none()
}

/// Temporarily swap `app.sftp` to the target session, run `f`, then
/// save the mutated state and restore the original session the user was
/// viewing.  Returns `Task::none()` when the target session was closed
/// in the meantime.
///
/// All state mutations inside `f` operate on the correct session, and
/// every return path (including early returns) is automatically guarded
/// — no manual save/restore needed.
pub(super) fn with_sftp_session(
    app: &mut App,
    session_id: SftpSessionId,
    f: impl FnOnce(&mut App, bool) -> Task<Message>,
) -> Task<Message> {
    let original_active = app.sftp.active_session;
    let is_active = session_id == original_active;

    if !is_active {
        app.save_active_sftp_state();
        if let Some(s) = app.sftp.sessions.iter().find(|s| s.id == session_id) {
            app.sftp.state = s.state.clone();
            app.sftp.active_session = session_id;
        } else {
            return Task::none();
        }
    }

    let tasks = f(app, is_active);

    if !is_active {
        app.save_active_sftp_state();
        // Restore the session the user is actually viewing.
        if let Some(s) = app.sftp.sessions.iter().find(|s| s.id == original_active) {
            app.sftp.state = s.state.clone();
            app.sftp.active_session = original_active;
        }
    }

    tasks
}

fn sftp_key_confirmation_parts(
    app: &App,
    pane: SftpPane,
) -> Option<(ServerKeyInfo, Host, bool, bool)> {
    let pane_state = app.sftp.state.pane(pane);
    if !pane_state.attempt.waiting_for_key() {
        return None;
    }
    let flow = &pane_state.attempt;
    Some((
        flow.pending_key_info.clone()?,
        flow.pending_host.clone()?,
        flow.save_password,
        flow.jump_stage(),
    ))
}

fn start_sftp_auth(
    app: &mut App,
    host: Host,
    pane: SftpPane,
    save_password: bool,
    session_id: SftpSessionId,
) -> Task<Message> {
    let jump_host = resolve_jump_host(app, &host);
    let pane_state = app.sftp.state.pane_mut(pane);
    pane_state.attempt.state = SshConnectionState::Connecting;
    pane_state.loading = true;
    pane_state.error = None;
    pane_state.attempt.pending_host = Some(host.clone());
    if pane_state.attempt.jump_host.is_none() {
        pane_state.attempt.jump_host = jump_host;
    }
    pane_state.attempt.pending_key_info = None;
    pane_state.attempt.save_password = save_password;

    if sftp_jump_stage(app, pane) {
        match sftp_pending_jump_host(app, pane) {
            Some(jump) => {
                if jump.auth_type == AuthType::Password && !jump.has_password() {
                    let pane_state = app.sftp.state.pane_mut(pane);
                    pane_state.attempt.state = SshConnectionState::WaitingForPassword;
                    pane_state.loading = false;
                    return Task::none();
                }

                let jump_known = known_hosts_file::is_known(&jump.hostname, jump.port)
                    || jump.expected_fingerprint.is_some();
                if !jump_known {
                    let hostname = jump.hostname.clone();
                    let port = jump.port;
                    return Task::perform(
                        crate::ssh::probe_host_key(hostname, port),
                        move |result| {
                            Message::Sftp(crate::sftp::message::Message::SftpKeyProbeFinished {
                                pane,
                                session_id,
                                result,
                            })
                        },
                    );
                }

                set_sftp_jump_done(app, pane);
            }
            None => {
                // The jump host was cleared while the flow was mid-stage (the
                // dialog was closed or reset). Drop back to the target stage and
                // fall through to the handling below instead of panicking.
                set_sftp_jump_done(app, pane);
            }
        }
    }

    let is_known_host = known_hosts_file::is_known(&host.hostname, host.port)
        || host.expected_fingerprint.is_some();
    if is_known_host {
        return list_remote_pane_home(app, host, pane, save_password);
    }

    let hostname = host.hostname.clone();
    let port = host.port;
    let jump_host = app.sftp.state.pane(pane).attempt.jump_host.clone();
    if let Some(jump_host) = jump_host {
        Task::perform(
            crate::ssh::probe_host_key_via_jump(jump_host, hostname, port),
            move |result| {
                Message::Sftp(crate::sftp::message::Message::SftpKeyProbeFinished {
                    pane,
                    session_id,
                    result,
                })
            },
        )
    } else {
        Task::perform(crate::ssh::probe_host_key(hostname, port), move |result| {
            Message::Sftp(crate::sftp::message::Message::SftpKeyProbeFinished {
                pane,
                session_id,
                result,
            })
        })
    }
}

fn connect_sftp_pane(
    app: &mut App,
    pane: SftpPane,
    host_id: HostId,
    session_id: SftpSessionId,
) -> Task<Message> {
    activate_pane(&mut app.sftp.state, pane);
    let Some(host) = app
        .hosts
        .list
        .iter()
        .find(|host| host.id == host_id)
        .cloned()
    else {
        return Task::perform(
            async { t!("sftp.error.host_gone").to_string() },
            Message::ShowToast,
        );
    };
    let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
    let log_key = match pane {
        SftpPane::Local => "sftp.connected_left",
        SftpPane::Remote => "sftp.connected_right",
    };
    app.add_log(LogCategory::Sftp, t!(log_key, target = &target).to_string());

    let jump_host = resolve_jump_host(app, &host);
    let needs_password = jump_host
        .as_ref()
        .is_some_and(|jump| jump.auth_type == AuthType::Password && !jump.has_password())
        || (jump_host.is_none() && host.auth_type == AuthType::Password && !host.has_password());

    clear_selection(&mut app.sftp.state, pane);
    app.sftp.state.side_mut(pane).host_picker = HostPickerState::default();
    let pane_state = app.sftp.state.enter_remote(pane);
    pane_state.connected = true;
    pane_state.host_id = Some(host_id);
    pane_state.attempt = crate::ssh_connect::ConnectionAttempt::new(host.clone(), jump_host);
    pane_state.attempt.state = if needs_password {
        SshConnectionState::WaitingForPassword
    } else {
        SshConnectionState::Connecting
    };
    pane_state.path = "/".to_string();
    pane_state.history = vec!["/".to_string()];
    pane_state.history_idx = 0;
    pane_state.entries.clear();
    pane_state.loading = !needs_password;
    pane_state.error = None;
    pane_state.attempt.password_input = SecretString::from(String::new());
    pane_state.attempt.password_visible = false;
    pane_state.filter.clear();
    pane_state.path_edit = None;
    if let Some(session) = app
        .sftp
        .sessions
        .iter_mut()
        .find(|session| session.id == app.sftp.active_session)
    {
        session.label.clone_from(&host.name);
    }
    if needs_password {
        Task::none()
    } else {
        start_sftp_auth(app, host, pane, false, session_id)
    }
}

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    let session_id = app.sftp.active_session;
    match msg {
        DomainMessage::SftpConnectLocalHost(id) => {
            return connect_sftp_pane(app, SftpPane::Local, id, session_id);
        }
        DomainMessage::SftpConnectHost(id) => {
            return connect_sftp_pane(app, SftpPane::Remote, id, session_id);
        }
        DomainMessage::SftpPasswordChanged { pane, password } => {
            app.sftp.state.pane_mut(pane).attempt.password_input = password;
        }
        DomainMessage::SftpTogglePasswordVisible { pane } => {
            let visible = app.sftp.state.pane(pane).attempt.password_visible;
            app.sftp.state.pane_mut(pane).attempt.password_visible = !visible;
        }
        DomainMessage::SftpPasswordSubmit { pane } => {
            let pane_state = app.sftp.state.pane_mut(pane);
            if !pane_state.attempt.waiting_for_password() {
                return Task::none();
            }
            let password = pane_state.attempt.password_input.clone();
            if password.expose_secret().is_empty() {
                return Task::none();
            }
            pane_state.attempt.password_input = SecretString::from(String::new());

            if sftp_jump_stage(app, pane) {
                let (target_label, jump) = {
                    let Some(jump) = sftp_pending_jump_host_mut(app, pane) else {
                        return Task::none();
                    };
                    jump.password = Some(password);
                    (
                        format!("{}@{}:{}", jump.username, jump.hostname, jump.port),
                        jump.clone(),
                    )
                };
                app.add_log(
                    LogCategory::Sftp,
                    format!("SFTP jump password submitted for {target_label}"),
                );

                let mut save_task = Task::none();
                if jump.id != HostId(0)
                    && let Some(stored) = app
                        .hosts
                        .list
                        .iter_mut()
                        .find(|candidate| candidate.id == jump.id)
                {
                    stored.password.clone_from(&jump.password);
                    let stored = stored.clone();
                    save_task = app.storage_write_task(move |s| s.upsert_host(&stored));
                }

                let Some(target) = current_remote_host(app, pane) else {
                    return Task::none();
                };
                let target_save = app.sftp.state.pane(pane).attempt.save_password;
                return save_task.chain(start_sftp_auth(
                    app,
                    target,
                    pane,
                    target_save,
                    session_id,
                ));
            }

            let Some(mut host) = current_remote_host(app, pane) else {
                return Task::none();
            };
            host.password = Some(password);
            let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
            app.add_log(
                LogCategory::Sftp,
                format!("SFTP password submitted for {target}"),
            );

            app.sftp.state.pane_mut(pane).attempt.save_password = true;
            return start_sftp_auth(app, host, pane, true, session_id);
        }
        DomainMessage::SftpKeyProbeFinished {
            pane,
            session_id,
            result,
        } => {
            return with_sftp_session(app, session_id, |app, _is_active| match result {
                Ok(key_info) => {
                    let pane_state = app.sftp.state.pane_mut(pane);
                    pane_state.attempt.pending_key_info = Some(key_info);
                    pane_state.attempt.state = SshConnectionState::WaitingForKeyConfirmation;
                    pane_state.loading = false;
                    pane_state.error = None;
                    Task::none()
                }
                Err(error) => {
                    let pane_state = app.sftp.state.pane_mut(pane);
                    pane_state.attempt.pending_key_info = None;
                    pane_state.attempt.state = SshConnectionState::Failed;
                    pane_state.loading = false;
                    pane_state.error = Some(error);
                    Task::none()
                }
            });
        }
        DomainMessage::SftpKeyConfirmAccept { pane } => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                let Some((key_info, host, save_password, is_jump)) =
                    sftp_key_confirmation_parts(app, pane)
                else {
                    return Task::none();
                };

                let mut tasks = vec![
                    crate::update::toast_on_storage_error(known_hosts_file::replace_host(
                        &key_info.hostname,
                        key_info.port,
                        &key_info.public_key,
                    )),
                    crate::known_hosts::update::reload_known_hosts_task(),
                ];

                let pane_state = app.sftp.state.pane_mut(pane);
                pane_state.attempt.pending_key_info = None;
                pane_state.attempt.expected_fingerprint = None;

                if is_jump {
                    set_sftp_jump_done(app, pane);
                    let Some(target) = current_remote_host(app, pane) else {
                        return Task::none();
                    };
                    tasks.push(start_sftp_auth(
                        app,
                        target,
                        pane,
                        save_password,
                        session_id,
                    ));
                    return Task::batch(tasks);
                }

                tasks.push(start_sftp_auth(app, host, pane, save_password, session_id));
                Task::batch(tasks)
            });
        }
        DomainMessage::SftpKeyConfirmContinue { pane } => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                let Some((key_info, host, save_password, is_jump)) =
                    sftp_key_confirmation_parts(app, pane)
                else {
                    return Task::none();
                };

                if is_jump {
                    // SFTP sessions reconnect the jump host from storage, so a
                    // session-only "Continue" would not survive a later browse
                    // refresh. Persist the key for jump hosts only.
                    let mut tasks = vec![
                        crate::update::toast_on_storage_error(known_hosts_file::replace_host(
                            &key_info.hostname,
                            key_info.port,
                            &key_info.public_key,
                        )),
                        crate::known_hosts::update::reload_known_hosts_task(),
                    ];
                    app.sftp.state.pane_mut(pane).attempt.pending_key_info = None;
                    set_sftp_jump_done(app, pane);
                    let Some(target) = current_remote_host(app, pane) else {
                        return Task::none();
                    };
                    tasks.push(start_sftp_auth(
                        app,
                        target,
                        pane,
                        save_password,
                        session_id,
                    ));
                    return Task::batch(tasks);
                }

                let mut host = host;
                host.expected_fingerprint = Some(key_info.fingerprint.clone());

                let pane_state = app.sftp.state.pane_mut(pane);
                pane_state.attempt.pending_key_info = None;
                pane_state.attempt.expected_fingerprint = Some(key_info.fingerprint.clone());
                start_sftp_auth(app, host, pane, save_password, session_id)
            });
        }
        DomainMessage::SftpDisconnect => {
            let host_label = app
                .sftp
                .state
                .right
                .pane
                .host_id
                .and_then(|id| app.hosts.list.iter().find(|h| h.id == id))
                .map(|h| format!("{}@{}:{}", h.username, h.hostname, h.port))
                .unwrap_or_default();
            if !host_label.is_empty() {
                app.add_log(
                    LogCategory::Sftp,
                    t!("sftp.disconnected", host = &host_label).to_string(),
                );
            }
            app.sftp.state.disconnect_remote();
        }
        DomainMessage::SftpDisconnectLocalRemote => {
            let host_label = app
                .sftp
                .state
                .left
                .pane
                .host_id
                .and_then(|id| app.hosts.list.iter().find(|h| h.id == id))
                .map(|h| format!("{}@{}:{}", h.username, h.hostname, h.port))
                .unwrap_or_default();
            if !host_label.is_empty() {
                app.add_log(
                    LogCategory::Sftp,
                    t!("sftp.disconnected_left", host = &host_label).to_string(),
                );
            }
            app.sftp.state.disconnect_left_remote();
        }
        DomainMessage::SftpOpenTerminal { pane } => {
            let Some(host) = current_remote_host(app, pane) else {
                return Task::perform(
                    async { t!("sftp.error.host_gone").to_string() },
                    Message::ShowWarningToast,
                );
            };
            app.add_log(
                LogCategory::Sftp,
                format!(
                    "Opening terminal for {}@{}:{}",
                    host.username, host.hostname, host.port
                ),
            );
            return match crate::terminal::update::create_ssh_session(
                app,
                &host,
                &crate::terminal::SshSessionSecrets::default(),
            ) {
                Ok((_, task)) => task,
                Err(error) => crate::terminal::update::ssh_bridge_unavailable_task(app, &error),
            };
        }
        DomainMessage::SftpToggleSidebar => {
            if app.sftp.sidebar_show && !app.sftp.sidebar_closing {
                app.sftp.sidebar_closing = true;
            } else {
                app.sftp.sidebar_show = true;
                app.sftp.sidebar_anim = 0.0;
                app.sftp.sidebar_closing = false;
            }
            return Task::none();
        }
        DomainMessage::SftpNewTab => {
            let new_id = create_sftp_session(app);
            app.load_sftp_state(new_id);
            if !app.sftp.state.left.pane.connected && app.sftp.state.left.pane.entries.is_empty() {
                return request_local_dir_load(app);
            }
            return Task::none();
        }
        DomainMessage::SftpCloseTab(session_id) => {
            app.sftp.sidebar_context_menu = None;
            return close_sftp_session(app, session_id);
        }
        DomainMessage::SftpSwitchTab(session_id) => {
            if app.sftp.sessions.iter().any(|s| s.id == session_id) {
                app.load_sftp_state(session_id);
                if !app.sftp.state.left.pane.connected
                    && app.sftp.state.left.pane.entries.is_empty()
                {
                    return request_local_dir_load(app);
                }
            }
            return Task::none();
        }
        DomainMessage::SftpDuplicateTab(session_id) => {
            app.sftp.sidebar_context_menu = None;
            return duplicate_sftp_session(app, session_id);
        }
        DomainMessage::SftpSidebarContextMenuOpen { session_id } => {
            let can_close = app.sftp.sessions.len() > 1;
            let tab_index = app
                .sftp
                .sessions
                .iter()
                .position(|s| s.id == session_id)
                .unwrap_or(0);
            app.sftp.sidebar_context_menu = Some(SftpSidebarContextMenu {
                session_id,
                tab_index,
                can_close,
                cursor: crate::core::list_cursor::ListCursor::new(),
            });
            if let Some(menu) = app.sftp.sidebar_context_menu.as_mut() {
                menu.cursor = crate::context_menu::initial_cursor(&menu.actions());
            }
            return Task::none();
        }
        DomainMessage::SftpSidebarContextMenuClose => {
            app.sftp.sidebar_context_menu = None;
            return Task::none();
        }
        _ => unreachable!("unexpected sftp message in session: {:?}", msg),
    }
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::host;

    /// The session id of the tab the SFTP workspace starts on.
    fn initial_session(app: &App) -> SftpSessionId {
        app.sftp.sessions[0].id
    }

    #[test]
    fn a_new_tab_gets_its_own_session_and_becomes_active() {
        let mut app = crate::core::test_support::app();
        let first = initial_session(&app);
        let tabs_before = app.sftp.sessions.len();

        let _ = update(&mut app, DomainMessage::SftpNewTab);

        assert_eq!(app.sftp.sessions.len(), tabs_before + 1);
        let active = app.sftp.active_session;
        assert_ne!(active, first, "the new tab takes over the workspace");
        assert!(
            app.sftp.sessions.iter().any(|s| s.id == active),
            "the active session must exist"
        );
        assert!(
            !app.sftp
                .sessions
                .iter()
                .any(|s| s.id == first && s.id == active),
            "the previous tab is still there, just not active"
        );
    }

    #[test]
    fn switching_tabs_carries_that_tab_state_into_the_workspace() {
        let mut app = crate::core::test_support::app();
        let first = initial_session(&app);
        let _ = update(&mut app, DomainMessage::SftpNewTab);
        let second = app.sftp.active_session;

        // Give the first tab a distinctive state, then leave it.
        app.sftp.sessions[0].state.left.pane.path = "/srv/first".to_string();
        app.sftp.state.left.pane.path = "/srv/second".to_string();

        let _ = update(&mut app, DomainMessage::SftpSwitchTab(first));
        assert_eq!(app.sftp.active_session, first);
        assert_eq!(app.sftp.state.left.pane.path, "/srv/first");

        let _ = update(&mut app, DomainMessage::SftpSwitchTab(second));
        assert_eq!(app.sftp.active_session, second);
        assert_eq!(app.sftp.state.left.pane.path, "/srv/second");
    }

    #[test]
    fn switching_to_a_tab_that_no_longer_exists_is_ignored() {
        let mut app = crate::core::test_support::app();
        let active = app.sftp.active_session;

        let _ = update(&mut app, DomainMessage::SftpSwitchTab(SftpSessionId(404)));

        assert_eq!(app.sftp.active_session, active);
    }

    #[test]
    fn closing_a_tab_moves_the_workspace_to_a_surviving_one() {
        let mut app = crate::core::test_support::app();
        let first = initial_session(&app);
        let _ = update(&mut app, DomainMessage::SftpNewTab);
        let second = app.sftp.active_session;

        let _ = update(&mut app, DomainMessage::SftpCloseTab(second));

        assert_eq!(app.sftp.sessions.len(), 1);
        assert_eq!(
            app.sftp.active_session, first,
            "closing the active tab hands the workspace to a survivor"
        );
    }

    #[test]
    fn the_last_tab_cannot_be_closed() {
        let mut app = crate::core::test_support::app();
        let only = initial_session(&app);

        let _ = update(&mut app, DomainMessage::SftpCloseTab(only));

        assert_eq!(
            app.sftp.sessions.len(),
            1,
            "the workspace always has somewhere to browse"
        );
        assert_eq!(app.sftp.active_session, only);
    }

    #[test]
    fn closing_a_background_tab_leaves_the_active_one_alone() {
        let mut app = crate::core::test_support::app();
        let first = initial_session(&app);
        let _ = update(&mut app, DomainMessage::SftpNewTab);
        let second = app.sftp.active_session;

        let _ = update(&mut app, DomainMessage::SftpCloseTab(first));

        assert_eq!(app.sftp.sessions.len(), 1);
        assert_eq!(app.sftp.active_session, second);
    }

    #[test]
    fn duplicating_a_tab_copies_its_state_but_not_its_transfers() {
        let mut app = crate::core::test_support::app();
        let source = initial_session(&app);
        app.sftp.state.left.pane.path = "/srv/app".to_string();
        app.sftp.state.right.pane.path = "/var/log".to_string();
        app.sftp
            .state
            .transfer_queue
            .push(crate::sftp::SftpQueuedTransfer {
                file_name: "app.log".to_string(),
                direction: crate::sftp::SftpTransferDirection::Download,
                total_bytes: 1024,
                total_files: 1,
                target: crate::sftp::SftpTransferTarget::Local {
                    directory: "/srv/app".to_string(),
                    name: "app.log".to_string(),
                },
                kind: crate::sftp::SftpQueuedTransferKind::DownloadFile {
                    host: host(1, "web", "10.0.0.1"),
                    remote_path: "/var/log/app.log".to_string(),
                    local_path: std::path::PathBuf::from("/srv/app/app.log"),
                    remote_size: 1024,
                },
            });
        app.save_active_sftp_state();

        let _ = update(&mut app, DomainMessage::SftpDuplicateTab(source));

        let copy = app
            .sftp
            .sessions
            .iter()
            .find(|s| s.id == app.sftp.active_session)
            .expect("the copy becomes the active tab");
        assert_ne!(copy.id, source);
        assert_eq!(
            copy.state.left.pane.path, "/srv/app",
            "the location is copied"
        );
        assert_eq!(copy.state.right.pane.path, "/var/log");
        assert!(
            copy.state.transfer_queue.is_empty(),
            "a copy must not re-run the original's queued transfers"
        );
        assert!(
            copy.label.contains("copy"),
            "the duplicated tab is labelled as one: {}",
            copy.label
        );
    }

    #[test]
    fn duplicating_twice_keeps_the_tab_labels_apart() {
        let mut app = crate::core::test_support::app();
        let source = initial_session(&app);

        let _ = update(&mut app, DomainMessage::SftpDuplicateTab(source));
        let _ = update(&mut app, DomainMessage::SftpDuplicateTab(source));

        let mut labels: Vec<&str> = app.sftp.sessions.iter().map(|s| s.label.as_str()).collect();
        let total = labels.len();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), total, "two tabs ended up with the same label");
    }

    #[test]
    fn duplicating_a_tab_that_is_gone_changes_nothing() {
        let mut app = crate::core::test_support::app();
        let before = app.sftp.sessions.len();

        let _ = update(
            &mut app,
            DomainMessage::SftpDuplicateTab(SftpSessionId(404)),
        );

        assert_eq!(app.sftp.sessions.len(), before);
    }

    #[test]
    fn the_sidebar_slides_out_before_it_closes_and_reopening_cancels_that() {
        let mut app = crate::core::test_support::app();
        assert!(!app.sftp.sidebar_show, "the panel starts hidden");

        // Opening resets the animation so the panel slides in from the edge.
        let _ = update(&mut app, DomainMessage::SftpToggleSidebar);
        assert!(app.sftp.sidebar_show && !app.sftp.sidebar_closing);
        assert_eq!(app.sftp.sidebar_anim, 0.0);

        // Closing is an animation, not an instant hide: the panel stays mounted
        // until the tick that drives the slide reaches zero.
        let _ = update(&mut app, DomainMessage::SftpToggleSidebar);
        assert!(
            app.sftp.sidebar_closing && app.sftp.sidebar_show,
            "the panel animates out before it is removed"
        );

        // Pressing the toggle again mid-close reopens it instead of leaving the
        // workspace with a panel that is hiding.
        let _ = update(&mut app, DomainMessage::SftpToggleSidebar);
        assert!(!app.sftp.sidebar_closing && app.sftp.sidebar_show);
        assert_eq!(app.sftp.sidebar_anim, 0.0, "it slides back in");
    }

    #[test]
    fn connecting_a_local_host_fills_in_the_remote_pane_it_will_browse() {
        let mut app = crate::core::test_support::app();
        app.hosts.list = vec![host(7, "bastion", "10.0.0.7")];

        let _ = update(&mut app, DomainMessage::SftpConnectLocalHost(HostId(7)));

        assert!(app.sftp.state.left.pane.connected);
        assert_eq!(app.sftp.state.left.pane.host_id, Some(HostId(7)));
        assert_eq!(app.sftp.state.left.pane.path, "/");
        assert_eq!(app.sftp.state.left.pane.history, ["/"]);
        assert!(app.sftp.state.left.pane.entries.is_empty());
        assert_eq!(
            app.sftp.state.left.pane.attempt.state,
            SshConnectionState::Connecting,
            "a saved password connects straight away"
        );
        assert_eq!(
            app.sftp.sessions[0].label, "bastion",
            "the tab takes the host's name"
        );
    }

    #[test]
    fn connecting_a_host_that_needs_a_password_waits_for_it() {
        let mut app = crate::core::test_support::app();
        let mut host = host(8, "locked", "10.0.0.8");
        host.password = None; // nothing saved to authenticate with
        app.hosts.list = vec![host];

        let _ = update(&mut app, DomainMessage::SftpConnectLocalHost(HostId(8)));

        assert_eq!(
            app.sftp.state.left.pane.attempt.state,
            SshConnectionState::WaitingForPassword
        );
        assert!(
            !app.sftp.state.left.pane.loading,
            "there is nothing to load until the user types the password"
        );
    }

    #[test]
    fn connecting_a_host_that_was_deleted_reports_instead_of_hanging() {
        let mut app = crate::core::test_support::app();

        let _ = update(&mut app, DomainMessage::SftpConnectLocalHost(HostId(404)));

        assert!(!app.sftp.state.left.pane.connected);
        assert_eq!(
            app.sftp.state.left.pane.attempt.state,
            SshConnectionState::Idle
        );
    }

    /// A host the vault already knows, for the jump-host lookups.
    fn saved_host(id: u32) -> Host {
        host(id, "jump", "10.0.0.9")
    }

    #[test]
    fn a_jump_host_is_resolved_from_the_vault_and_missing_ones_fall_flat() {
        let mut app = crate::core::test_support::app();
        app.hosts.list = vec![saved_host(5)];

        let mut target = saved_host(1);
        target.jump_host_id = Some(HostId(5));
        assert_eq!(
            resolve_jump_host(&app, &target).map(|host| host.id),
            Some(HostId(5))
        );

        // A host that was deleted after the connection was set up: the jump
        // hop is simply not there any more.
        target.jump_host_id = Some(HostId(404));
        assert!(resolve_jump_host(&app, &target).is_none());

        target.jump_host_id = None;
        assert!(
            resolve_jump_host(&app, &target).is_none(),
            "a host with no jump configured has nothing to resolve"
        );
    }

    #[test]
    fn a_jump_stage_is_only_running_while_the_first_hop_is_being_dialled() {
        let mut app = crate::core::test_support::app();

        assert!(
            !sftp_jump_stage(&app, SftpPane::Remote) && !sftp_jump_stage(&app, SftpPane::Local),
            "nothing is a jump stage before a connection starts"
        );

        app.sftp.state.right.pane.attempt.jump_host = Some(saved_host(5));
        app.sftp.state.right.pane.attempt.stage = crate::ssh_connect::SshConnectStage::Jump;
        assert!(
            sftp_jump_stage(&app, SftpPane::Remote),
            "the right pane is on its hop"
        );
        assert!(
            !sftp_jump_stage(&app, SftpPane::Local),
            "the left pane has its own flow and is not affected"
        );

        app.sftp.state.right.pane.attempt.stage = crate::ssh_connect::SshConnectStage::Target;
        assert!(
            !sftp_jump_stage(&app, SftpPane::Remote),
            "once the hop is up, the stage is the target itself"
        );
    }

    /// The host-key part of a connection flow that is waiting on the user.
    fn waiting_flow(pane: SftpPane, app: &mut App) {
        let key = ServerKeyInfo {
            hostname: "10.0.0.1".to_string(),
            port: 22,
            key_type: "ssh-ed25519".to_string(),
            public_key: "AAAA".to_string(),
            fingerprint: "SHA256:abc".to_string(),
        };
        let host = saved_host(5);

        let pane_state = app.sftp.state.pane_mut(pane);
        pane_state.attempt.state = SshConnectionState::WaitingForKeyConfirmation;
        pane_state.attempt.pending_key_info = Some(key);
        pane_state.attempt.pending_host = Some(host);
    }

    #[test]
    fn the_host_key_prompt_parts_come_from_the_pane_that_is_waiting() {
        for pane in [SftpPane::Local, SftpPane::Remote] {
            let mut app = crate::core::test_support::app();
            waiting_flow(pane, &mut app);

            let (key, host, save_password, is_jump) =
                sftp_key_confirmation_parts(&app, pane).expect("the prompt has its parts");

            assert_eq!(key.fingerprint, "SHA256:abc");
            assert_eq!(host.id, HostId(5));
            assert!(!save_password, "an unasked-for password is not saved");
            assert!(!is_jump, "the prompt is about the target, not the hop");
            assert!(
                sftp_key_confirmation_parts(
                    &app,
                    if pane == SftpPane::Local {
                        SftpPane::Remote
                    } else {
                        SftpPane::Local
                    }
                )
                .is_none(),
                "the other pane is not the one waiting"
            );
        }
    }

    #[test]
    fn a_host_key_prompt_with_nothing_behind_it_reports_nothing() {
        let mut app = crate::core::test_support::app();

        assert!(
            sftp_key_confirmation_parts(&app, SftpPane::Remote).is_none(),
            "a pane that is not waiting for a key has no prompt"
        );

        // The state says it is waiting, but the flow never got as far as
        // recording which key and host are in question.
        app.sftp.state.right.pane.attempt.state = SshConnectionState::WaitingForKeyConfirmation;
        assert!(sftp_key_confirmation_parts(&app, SftpPane::Remote).is_none());

        app.sftp.state.right.pane.attempt.pending_key_info = Some(ServerKeyInfo {
            hostname: "10.0.0.1".to_string(),
            port: 22,
            key_type: "ssh-ed25519".to_string(),
            public_key: "AAAA".to_string(),
            fingerprint: "SHA256:abc".to_string(),
        });
        assert!(
            sftp_key_confirmation_parts(&app, SftpPane::Remote).is_none(),
            "the key alone is not enough — the host it belongs to is part of the prompt"
        );
    }
}
