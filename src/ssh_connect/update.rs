use std::collections::HashMap;

use secrecy::SecretString;

use super::message::Message as DomainMessage;
use crate::app::{ActiveTab, App, DraftId};
use crate::core::data::{AuthType, Host, HostId, LogCategory, OsType, ServerKeyInfo};
use crate::core::diagnostics::Level;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::ssh::known_hosts_file;
use crate::ssh_connect::SshConnectionState;
use crate::terminal::{Draft, TabKind};
use crate::update::{scroll_to_active_tab, toast_on_storage_error};
use rust_i18n::t;

/// Returns a reference to the SSH connect state for the active tab's draft.
fn ssh_state(app: &App) -> Option<&crate::ssh_connect::SshConnectState> {
    let draft_id = app.active_draft_id()?;
    app.terminal.drafts.get(&draft_id).map(|d| &d.ssh)
}

/// Returns a mutable reference to the SSH connect state for the active tab's draft.
fn ssh_state_mut(app: &mut App) -> Option<&mut crate::ssh_connect::SshConnectState> {
    let draft_id = app.active_draft_id()?;
    app.terminal.drafts.get_mut(&draft_id).map(|d| &mut d.ssh)
}

/// Reference to a specific draft's SSH connect state. Borrows only `drafts`
/// so callers can still touch disjoint `App` fields (hosts, `known_hosts`, …).
fn ssh_state_for(
    drafts: &HashMap<DraftId, Draft>,
    id: DraftId,
) -> Option<&crate::ssh_connect::SshConnectState> {
    drafts.get(&id).map(|d| &d.ssh)
}

/// Mutable reference to a specific draft's SSH connect state. Borrows only
/// `drafts`, keeping disjoint `App` fields accessible to the caller.
fn ssh_state_for_mut(
    drafts: &mut HashMap<DraftId, Draft>,
    id: DraftId,
) -> Option<&mut crate::ssh_connect::SshConnectState> {
    drafts.get_mut(&id).map(|d| &mut d.ssh)
}

/// A key-auth host needs a passphrase prompt when its private key is encrypted
/// and this connection attempt hasn't been given one yet.
fn host_key_requires_passphrase(host: &Host) -> bool {
    host.auth_type == AuthType::Key
        && host.key_passphrase.is_none()
        && host
            .key_path
            .as_ref()
            .filter(|path| !path.is_empty())
            .is_some_and(|path| crate::ssh::key_requires_passphrase(path))
}

fn is_jump_stage(state: &crate::ssh_connect::SshConnectState) -> bool {
    state.attempt.jump_stage()
}

fn current_auth_host(state: &crate::ssh_connect::SshConnectState) -> Option<&Host> {
    state.attempt.current_host()
}

fn current_auth_host_mut(state: &mut crate::ssh_connect::SshConnectState) -> Option<&mut Host> {
    state.attempt.current_host_mut()
}

fn current_auth_label(state: &crate::ssh_connect::SshConnectState) -> String {
    state.attempt.current_label()
}

pub fn update(app: &mut App, msg: DomainMessage) -> iced::Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "ssh_connect::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        DomainMessage::SshRetry => {
            let Some(draft_id) = app.active_draft_id() else {
                return iced::Task::none();
            };

            // `start_ssh_connect` resolves the jump host and picks the initial
            // stage/state once, caching them on the draft. If the user edited
            // the host while it sat in Failed (e.g. to add a jump host), that
            // cache is stale, so the retry would keep dialing the old config.
            // Re-resolve from the freshly saved host so retry behaves like a
            // fresh connect.
            let (pending, cached_jump_id) = {
                let Some(state) = ssh_state_for(&app.terminal.drafts, draft_id) else {
                    return iced::Task::none();
                };
                if state.attempt.state != SshConnectionState::Failed {
                    return iced::Task::none();
                }
                (
                    state.attempt.pending_host.clone(),
                    state.attempt.jump_host.as_ref().map(|jump| jump.id),
                )
            };
            let Some(pending) = pending else {
                return iced::Task::none();
            };

            let jump_host = pending.jump_host_id.and_then(|jump_id| {
                app.hosts
                    .list
                    .iter()
                    .find(|candidate| candidate.id == jump_id)
                    .cloned()
            });
            let jump_changed = cached_jump_id != jump_host.as_ref().map(|jump| jump.id);
            let first_hop = jump_host.as_ref().unwrap_or(&pending);
            let needs_password =
                first_hop.auth_type == AuthType::Password && !first_hop.has_password();

            let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) else {
                return iced::Task::none();
            };
            state.attempt.reset_for_retry(jump_host, jump_changed);

            if jump_changed {
                // Mirror `start_ssh_connect`: restart from the first hop so a
                // newly configured (or removed) jump host takes effect.
                state.attempt.state = if needs_password {
                    SshConnectionState::WaitingForPassword
                } else {
                    SshConnectionState::Connecting
                };
                // Drop any passphrase cached for either hop so the next
                // attempt prompts again instead of replaying a stale secret.
                for host in [
                    state.attempt.pending_host.as_mut(),
                    state.attempt.jump_host.as_mut(),
                ]
                .into_iter()
                .flatten()
                {
                    if host.auth_type == AuthType::Key {
                        host.key_passphrase = None;
                    }
                }
            } else {
                state.attempt.state = SshConnectionState::Connecting;
                // A wrong passphrase is the most common key-auth failure. Clear
                // it on retry so the next tick prompts again instead of
                // retrying the same bad secret in a loop.
                if let Some(host) = current_auth_host_mut(state)
                    && host.auth_type == AuthType::Key
                {
                    host.key_passphrase = None;
                }
            }

            let label = current_auth_label(state);
            if !label.is_empty() {
                state.logs = vec![format!("Retrying SSH connection to {label}")];
            }
            iced::Task::none()
        }
        DomainMessage::SshEditHost => {
            let host_opt = app
                .active_draft_id()
                .and_then(|draft_id| ssh_state_for(&app.terminal.drafts, draft_id))
                .and_then(|s| s.attempt.pending_host.clone());
            let Some(ref host) = host_opt else {
                return iced::Task::none();
            };
            app.hosts.form.name.clone_from(&host.name);
            app.hosts.form.hostname.clone_from(&host.hostname);
            app.hosts.form.port = host.port.to_string();
            app.hosts.form.username.clone_from(&host.username);
            app.hosts.form.auth = host.auth_type;
            app.hosts.form.jump_host_id = host.jump_host_id;
            app.hosts.form.key_path = host.key_path.clone().unwrap_or_default();
            app.hosts.form.password = host.password_for_edit().into();
            app.hosts.form.editing_id = (host.id != HostId(0)).then_some(host.id);
            app.hosts.form.visible = true;
            app.hosts.form_anim = 0.0;
            app.hosts.form_closing = false;
            iced::Task::none()
        }
        DomainMessage::SshClose => {
            if let Some(draft_id) = app.active_draft_id() {
                let log_line = ssh_state_for(&app.terminal.drafts, draft_id).and_then(|s| {
                    s.attempt.pending_host.as_ref().map(|host| {
                        format!(
                            "SshClose: closing SSH connection to {}@{}:{} (state was {:?})",
                            host.username, host.hostname, host.port, s.attempt.state
                        )
                    })
                });
                if let Some(line) = log_line {
                    app.add_log(LogCategory::Terminal, line);
                }
                if let Some(tab_id) = app.tab_id_for_draft(draft_id) {
                    app.activate_neighbor_after_close(tab_id);
                    app.remove_tab(tab_id);
                }
                app.terminal.drafts.remove(&draft_id);
                return iced::Task::none();
            }
            iced::Task::none()
        }
        DomainMessage::CopySshLogs => {
            let text = ssh_state(app)
                .map(|s| s.logs.join("\n"))
                .unwrap_or_default();
            if !text.is_empty() {
                return iced::Task::batch([
                    iced::clipboard::write(text),
                    iced::Task::done(Message::ShowToast(t!("app.logs_copied").to_string())),
                ]);
            }
            iced::Task::none()
        }
        DomainMessage::SshPasswordChanged(pw) => {
            if let Some(state) = ssh_state_mut(app) {
                state.attempt.password_input = pw;
            }
            iced::Task::none()
        }
        DomainMessage::SshTogglePasswordVisible => {
            if let Some(state) = ssh_state_mut(app) {
                state.attempt.password_visible = !state.attempt.password_visible;
            }
            iced::Task::none()
        }
        DomainMessage::SshPasswordSubmit => {
            let Some(draft_id) = app.active_draft_id() else {
                return iced::Task::none();
            };
            // Phase 1: the borrow ends before `add_log`.
            let (host_label, auth_kind) = {
                let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) else {
                    return iced::Task::none();
                };
                if state.attempt.state != SshConnectionState::WaitingForPassword {
                    return iced::Task::none();
                }
                let password = state.attempt.password_input.clone();
                if let Some(host) = current_auth_host_mut(state) {
                    match host.auth_type {
                        AuthType::Key => host.key_passphrase = Some(password),
                        AuthType::Password => host.password = Some(password),
                    }
                }
                let auth_kind =
                    current_auth_host(state).map_or(AuthType::Password, |h| h.auth_type);
                let label = current_auth_label(state);
                (label, auth_kind)
            };
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "SshPasswordSubmit: re-trying auth for {host_label} with {}",
                    if auth_kind == AuthType::Key {
                        "key passphrase"
                    } else {
                        "password"
                    }
                ),
            );
            if let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) {
                state.attempt.password_input = SecretString::from(String::new());
                state.attempt.state = SshConnectionState::Connecting;
                state.attempt.auth_started = false;

                let label = current_auth_label(state);
                state.logs = vec![format!("Starting SSH authentication for {label}")];
            }
            iced::Task::none()
        }
        DomainMessage::SshTick => {
            // Drive every draft that is currently connecting, independently.
            // `auth_started` is per-draft, so each draft spawns its probe/auth
            // task exactly once and subsequent ticks are no-ops for it.
            let mut to_drive: Vec<(DraftId, Host, Option<Host>, bool)> = Vec::new();
            let mut no_target: Vec<DraftId> = Vec::new();
            for (did, draft) in &app.terminal.drafts {
                if draft.ssh.attempt.state != SshConnectionState::Connecting
                    || draft.ssh.attempt.auth_started
                {
                    continue;
                }
                match current_auth_host(&draft.ssh).cloned() {
                    Some(host) => {
                        let jump_stage = is_jump_stage(&draft.ssh);
                        let jump_for_target = if jump_stage {
                            None
                        } else {
                            draft.ssh.attempt.jump_host.clone()
                        };
                        to_drive.push((*did, host, jump_for_target, jump_stage));
                    }
                    None => no_target.push(*did),
                }
            }

            for did in no_target {
                app.add_log(
                    LogCategory::Terminal,
                    "SshTick: Connecting state but no pending_host — transitioning to Failed"
                        .to_string(),
                );
                if let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, did) {
                    state.attempt.state = SshConnectionState::Failed;
                    state
                        .logs
                        .push(t_static(t!("ssh.log_no_target")).to_string());
                }
            }

            let mut tasks = Vec::new();
            for (draft_id, host, jump_for_target, jump_stage) in to_drive {
                let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
                app.add_log(
                    LogCategory::Terminal,
                    format!("SshTick: starting authentication for {target}"),
                );

                // Encrypted private keys need a passphrase before russh can
                // load them. Prompt once and retry after the user submits it.
                let key_needs_passphrase = host_key_requires_passphrase(&host);
                if key_needs_passphrase {
                    if let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) {
                        state.attempt.state = SshConnectionState::WaitingForPassword;
                        state.attempt.auth_started = false;
                        state.attempt.password_input = SecretString::from(String::new());
                        state.attempt.password_visible = false;
                        state.logs.push(format!(
                            "Private key is encrypted; enter the passphrase for {}",
                            host.key_path.as_deref().unwrap_or("the private key")
                        ));
                    }
                    continue;
                }

                // Idempotent: later ticks see `auth_started` and do nothing.
                if let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) {
                    state.attempt.auth_started = true;
                }

                let is_known_host = known_hosts_file::is_known(&host.hostname, host.port);
                let skip_probe = ssh_state_for(&app.terminal.drafts, draft_id)
                    .is_some_and(|s| s.attempt.probe_skip);

                if !is_known_host && !skip_probe {
                    // An unknown host gets its key probed before authentication.
                    if let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) {
                        state
                            .logs
                            .push(t_static(t!("ssh.log_probing_key")).to_string());
                    }
                    let hostname = host.hostname.clone();
                    let port = host.port;
                    let probe_task = if jump_stage {
                        iced::Task::perform(
                            crate::ssh::probe_host_key(hostname, port),
                            move |result| {
                                Message::SshConnect(
                                    crate::ssh_connect::message::Message::SshKeyProbeFinished {
                                        tab_id: draft_id,
                                        result,
                                    },
                                )
                            },
                        )
                    } else if let Some(jump_host) = jump_for_target {
                        iced::Task::perform(
                            crate::ssh::probe_host_key_via_jump(jump_host, hostname, port),
                            move |result| {
                                Message::SshConnect(
                                    crate::ssh_connect::message::Message::SshKeyProbeFinished {
                                        tab_id: draft_id,
                                        result,
                                    },
                                )
                            },
                        )
                    } else {
                        iced::Task::perform(
                            crate::ssh::probe_host_key(hostname, port),
                            move |result| {
                                Message::SshConnect(
                                    crate::ssh_connect::message::Message::SshKeyProbeFinished {
                                        tab_id: draft_id,
                                        result,
                                    },
                                )
                            },
                        )
                    };
                    tasks.push(probe_task);
                } else {
                    // Known host: straight to authentication.
                    if let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) {
                        state
                            .logs
                            .push(t_static(t!("ssh.log_checking_auth")).to_string());
                    }
                    let auth_task = if jump_stage {
                        iced::Task::perform(
                            authenticate_host(host),
                            move |(success, logs, server_key)| {
                                Message::SshConnect(
                                    crate::ssh_connect::message::Message::SshAuthFinished {
                                        tab_id: draft_id,
                                        success,
                                        logs,
                                        server_key,
                                    },
                                )
                            },
                        )
                    } else if let Some(jump_host) = jump_for_target {
                        iced::Task::perform(
                            authenticate_host_via_jump(host, jump_host),
                            move |(success, logs, server_key)| {
                                Message::SshConnect(
                                    crate::ssh_connect::message::Message::SshAuthFinished {
                                        tab_id: draft_id,
                                        success,
                                        logs,
                                        server_key,
                                    },
                                )
                            },
                        )
                    } else {
                        iced::Task::perform(
                            authenticate_host(host),
                            move |(success, logs, server_key)| {
                                Message::SshConnect(
                                    crate::ssh_connect::message::Message::SshAuthFinished {
                                        tab_id: draft_id,
                                        success,
                                        logs,
                                        server_key,
                                    },
                                )
                            },
                        )
                    };
                    tasks.push(auth_task);
                }
            }
            iced::Task::batch(tasks)
        }
        DomainMessage::SshKeyProbeFinished { tab_id, result } => {
            // Discard only if the draft was closed while the probe was in flight.
            // The connection may legitimately finish in the background even when
            // the user has switched to another tab.
            if !app.terminal.drafts.contains_key(&tab_id) {
                app.add_log(
                    LogCategory::Terminal,
                    format!("SshKeyProbeFinished: discarding result for closed tab {tab_id}"),
                );
                return iced::Task::none();
            }
            let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, tab_id) else {
                return iced::Task::none();
            };
            match result {
                Ok(key_info) => {
                    let fingerprint = key_info.fingerprint.clone();
                    state.logs.push(format!(
                        "Host key captured: {} {}",
                        key_info.key_type, fingerprint
                    ));
                    state
                        .logs
                        .push(t_static(t!("ssh.log_waiting_key")).to_string());
                    state.attempt.pending_key_info = Some(key_info);
                    state.attempt.state = SshConnectionState::WaitingForKeyConfirmation;
                }
                Err(error) => {
                    state.logs.push(format!("Key probe failed: {error}"));
                    state.attempt.state = SshConnectionState::Failed;
                }
            }
            iced::Task::none()
        }
        DomainMessage::SshKeyConfirmAccept => {
            let Some(draft_id) = app.active_draft_id() else {
                return iced::Task::none();
            };
            let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) else {
                return iced::Task::none();
            };
            if state.attempt.state != SshConnectionState::WaitingForKeyConfirmation {
                return iced::Task::none();
            }

            let mut tasks: Vec<iced::Task<Message>> = Vec::new();
            if let Some(ref key_info) = state.attempt.pending_key_info {
                tasks.push(toast_on_storage_error(known_hosts_file::replace_host(
                    &key_info.hostname,
                    key_info.port,
                    &key_info.public_key,
                )));
                tasks.push(crate::known_hosts::update::reload_known_hosts_task());
                state.logs.push(format!(
                    "Host key trusted: {} {}",
                    key_info.key_type, key_info.fingerprint
                ));
            }

            state.attempt.pending_key_info = None;
            state.attempt.state = SshConnectionState::Connecting;
            state.attempt.auth_started = false; // reset so SshTick will re-trigger
            state
                .logs
                .push(t_static(t!("ssh.log_proceeding")).to_string());
            if tasks.is_empty() {
                iced::Task::none()
            } else {
                iced::Task::batch(tasks)
            }
        }
        DomainMessage::SshKeyConfirmContinue => {
            let Some(draft_id) = app.active_draft_id() else {
                return iced::Task::none();
            };
            let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, draft_id) else {
                return iced::Task::none();
            };
            if state.attempt.state != SshConnectionState::WaitingForKeyConfirmation {
                return iced::Task::none();
            }

            // Trust the key for this session only — don't persist to known_hosts.
            // The fingerprint is set on pending_host so check_server_key will still verify it.
            if let Some(ref key_info) = state.attempt.pending_key_info {
                let fingerprint = key_info.fingerprint.clone();
                let key_type = key_info.key_type.clone();
                if let Some(host) = current_auth_host_mut(state) {
                    host.expected_fingerprint = Some(fingerprint.clone());
                }
                state.logs.push(format!(
                    "Host key accepted for this session: {key_type} {fingerprint}"
                ));
            }

            state.attempt.probe_skip = true;
            state.attempt.pending_key_info = None;
            state.attempt.state = SshConnectionState::Connecting;
            state.attempt.auth_started = false;
            state
                .logs
                .push(t_static(t!("ssh.log_proceeding")).to_string());
            iced::Task::none()
        }
        DomainMessage::SshAuthFinished {
            tab_id,
            success,
            logs,
            server_key,
        } => {
            // Discard only if the draft was closed while auth was in flight.
            // The connection may legitimately finish in the background even when
            // the user has switched to another tab.
            if !app.terminal.drafts.contains_key(&tab_id) {
                app.add_log(
                    LogCategory::Terminal,
                    format!("SshAuthFinished: discarding result for closed tab {tab_id}"),
                );
                return iced::Task::none();
            }
            // Extract log messages and determine next state inside a block so the
            // mutable borrow ends before we call app.add_log.
            let (success_log, pending_none, jump_handled, jump_key_to_save, jump_host_to_save) = {
                let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, tab_id) else {
                    return iced::Task::none();
                };
                state.logs.extend(logs);
                let jump_stage = is_jump_stage(state);
                if success {
                    let success_log = current_auth_host(state).map(|h| {
                        format!(
                            "SSH authentication succeeded for {}@{}:{}",
                            h.username, h.hostname, h.port
                        )
                    });
                    let pending_none = current_auth_host(state).is_none();
                    if pending_none {
                        state.attempt.state = SshConnectionState::Failed;
                        state
                            .logs
                            .push(t_static(t!("ssh.log_no_target_after_auth")).to_string());
                        (success_log, true, false, None, None)
                    } else if jump_stage {
                        let jump_key_to_save = if state.attempt.probe_skip {
                            None
                        } else {
                            server_key.clone()
                        };
                        let jump_host_to_save = state
                            .attempt
                            .jump_host
                            .as_ref()
                            .filter(|jump| jump.id != HostId(0))
                            .cloned();
                        state.attempt.stage = crate::ssh_connect::SshConnectStage::Target;
                        state.attempt.probe_skip = false;
                        state.attempt.pending_key_info = None;
                        state.attempt.state = SshConnectionState::Connecting;
                        state.attempt.auth_started = false;
                        state
                            .logs
                            .push("Jump host authenticated; proceeding to target".to_string());
                        (
                            success_log,
                            false,
                            true,
                            jump_key_to_save,
                            jump_host_to_save,
                        )
                    } else {
                        (success_log, false, false, None, None)
                    }
                } else {
                    let fail_log = current_auth_host(state).map(|h| {
                        format!(
                            "SSH authentication failed for {}@{}:{}",
                            h.username, h.hostname, h.port
                        )
                    });
                    state.attempt.state = SshConnectionState::Failed;
                    state
                        .logs
                        .push(t_static(t!("ssh.log_auth_failed")).to_string());
                    (fail_log, false, false, None, None)
                }
            };
            // Now the mutable borrow is released; we can call add_log.
            if let Some(msg) = success_log {
                app.add_log(LogCategory::Terminal, msg);
            }
            if jump_handled {
                if !success {
                    return iced::Task::none();
                }
                let mut jump_tasks: Vec<iced::Task<Message>> = Vec::new();
                if let Some(key_info) = jump_key_to_save {
                    jump_tasks.push(toast_on_storage_error(known_hosts_file::append(
                        &key_info.hostname,
                        key_info.port,
                        &key_info.public_key,
                    )));
                    jump_tasks.push(crate::known_hosts::update::reload_known_hosts_task());
                }
                if let Some(jump_host) = jump_host_to_save
                    && let Some(stored) = app
                        .hosts
                        .list
                        .iter_mut()
                        .find(|host| host.id == jump_host.id)
                {
                    stored.password.clone_from(&jump_host.password);
                    let stored = stored.clone();
                    jump_tasks.push(app.storage_write_task(move |s| s.upsert_host(&stored)));
                }
                return iced::Task::batch(jump_tasks);
            }
            if pending_none {
                return iced::Task::none();
            }
            if !success {
                return iced::Task::none();
            }
            let Some(state) = ssh_state_for_mut(&mut app.terminal.drafts, tab_id) else {
                return iced::Task::none();
            };

            let password_save_host = if let Some(ref host) = state.attempt.pending_host {
                if host.id == HostId(0) {
                    None
                } else {
                    app.hosts
                        .list
                        .iter_mut()
                        .find(|h| h.id == host.id)
                        .map(|stored| {
                            stored.password.clone_from(&host.password);
                            stored.clone()
                        })
                }
            } else {
                None
            };

            let mut tasks: Vec<iced::Task<Message>> = Vec::new();

            // Save server key to known_hosts (skip if "Continue" was used —
            // probe_skip is true for session-only trust).
            if !state.attempt.probe_skip
                && let Some(key_info) = server_key
            {
                tasks.push(toast_on_storage_error(known_hosts_file::append(
                    &key_info.hostname,
                    key_info.port,
                    &key_info.public_key,
                )));
                tasks.push(crate::known_hosts::update::reload_known_hosts_task());
            }

            state.attempt.state = SshConnectionState::Connected;
            state
                .logs
                .push(t_static(t!("ssh.log_opening_session")).to_string());

            // Capture the host and draft id in the message so SshOpenTerminal
            // doesn't read the mutable state slot (which may be overwritten
            // by another ConnectHost during the 1s delay). OS detection is
            // chained inside SshOpenTerminal instead of here so it is skipped
            // when the draft is closed during the delay.
            // Set when the connect started; if it is somehow gone, bail out
            // rather than open a terminal for a host we no longer have.
            let Some(host_for_terminal) = state.attempt.pending_host.clone() else {
                return iced::Task::none();
            };
            let secrets_for_terminal = crate::terminal::SshSessionSecrets::from_hosts(
                &host_for_terminal,
                state.attempt.jump_host.as_ref(),
            );
            let captured_ntid = Some(tab_id);
            let password_save = match password_save_host {
                Some(stored) => app.storage_write_task(move |s| s.upsert_host(&stored)),
                None => iced::Task::none(),
            };
            let open_terminal = iced::Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                },
                move |()| {
                    Message::SshConnect(crate::ssh_connect::message::Message::SshOpenTerminal(
                        host_for_terminal,
                        secrets_for_terminal,
                        captured_ntid,
                    ))
                },
            );
            tasks.push(open_terminal);
            tasks.push(password_save);
            iced::Task::batch(tasks)
        }
        DomainMessage::SshOpenTerminal(host, secrets, captured_ntid) => {
            // The 1s delay after auth gives the user a window to close the
            // connecting tab. If the draft is gone, they cancelled — discard
            // instead of unconditionally creating a session.
            if let Some(draft_id) = captured_ntid
                && !app.terminal.drafts.contains_key(&draft_id)
            {
                app.add_log(
                    LogCategory::Terminal,
                    format!("SshOpenTerminal: draft {draft_id} closed during delay — discarding"),
                );
                return iced::Task::none();
            }

            let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "SshOpenTerminal: opening terminal for {target} (host_id={})",
                    host.id
                ),
            );

            // Remember whether the user is still viewing the connecting placeholder tab.
            let was_on_connecting_tab = captured_ntid
                .and_then(|did| app.tab_id_for_draft(did))
                .is_some_and(|tid| app.active_tab == ActiveTab::Tab(tid));
            let previous_active = app.active_tab;

            let (new_sid, mut task) =
                match crate::terminal::update::create_ssh_session(app, &host, &secrets) {
                    Ok(created) => created,
                    Err(error) => {
                        if let Some(draft_id) = captured_ntid
                            && let Some(draft) = app.terminal.drafts.get_mut(&draft_id)
                        {
                            draft.ssh.attempt.state = SshConnectionState::Failed;
                            draft
                                .ssh
                                .logs
                                .push(t!("ssh.bridge_restart_required").to_string());
                        }
                        return crate::terminal::update::ssh_bridge_unavailable_task(app, &error);
                    }
                };

            // Collapse the connecting draft into the freshly created session,
            // keeping the draft's tab id and position so the tab bar doesn't
            // reshuffle (the old retain+rewrite is gone — one entity, one slot).
            if let Some(draft_id) = captured_ntid {
                if let (Some(draft_tab), Some(session_tab)) = (
                    app.tab_id_for_draft(draft_id),
                    app.tab_id_for_session(new_sid),
                ) {
                    if let Some(draft) = app.terminal.tab_registry.get_mut(&draft_tab) {
                        draft.kind = TabKind::Session(new_sid);
                    }
                    app.remove_tab(session_tab);
                    app.activate_tab(draft_tab);
                }
                app.terminal.drafts.remove(&draft_id);
            }

            if was_on_connecting_tab {
                // Auto-scroll the topbar to show the newly created terminal tab.
                task = task.chain(scroll_to_active_tab(app));
            } else {
                // The user navigated away while the connection was in flight;
                // restore whatever they were viewing instead of yanking them to
                // the newly opened terminal.
                app.active_tab = previous_active;
            }

            if host.id != HostId(0) {
                task = task.chain(spawn_os_detect(host.clone(), host.id));
            }

            task
        }
    }
}

async fn authenticate_host(host: Host) -> (bool, Vec<String>, Option<ServerKeyInfo>) {
    crate::ssh::authenticate_host(host).await
}

async fn authenticate_host_via_jump(
    host: Host,
    jump_host: Host,
) -> (bool, Vec<String>, Option<ServerKeyInfo>) {
    crate::ssh::authenticate_host_via_jump(host, jump_host).await
}

fn spawn_os_detect(host: Host, host_id: HostId) -> iced::Task<Message> {
    iced::Task::future(async move {
        let os_type = tokio::task::spawn_blocking(move || {
            // Building a runtime only fails under resource exhaustion; the host
            // then keeps its previous OS guess instead of taking the app down.
            let Ok(runtime) = tokio::runtime::Runtime::new() else {
                return OsType::Unknown;
            };
            runtime
                .block_on(crate::ssh::detect_remote_os(host))
                .unwrap_or(OsType::Unknown)
        })
        .await
        .unwrap_or(OsType::Unknown);
        Message::Hosts(crate::hosts::message::Message::OsDetected { host_id, os_type })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh_connect::SshConnectStage;

    fn key_host(key_path: Option<String>) -> Host {
        Host {
            id: HostId(1),
            name: "web".into(),
            hostname: "example.com".into(),
            port: 22,
            protocol: crate::core::data::Protocol::Ssh,
            username: "root".into(),
            auth_type: AuthType::Key,
            jump_host_id: None,
            identity_id: None,
            key_path,
            key_passphrase: None,
            password: None,
            os_type: OsType::Unknown,
            tags: Vec::new(),
            expected_fingerprint: None,
        }
    }

    /// A scratch directory the test can drop keys into, removed on the way out.
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "termlet_ssh_connect_test_{}_{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_password_host_never_needs_a_key_passphrase() {
        let mut host = key_host(None);
        host.auth_type = AuthType::Password;
        assert!(!host_key_requires_passphrase(&host));
    }

    #[test]
    fn a_key_host_without_a_usable_path_needs_no_passphrase() {
        for key_path in [None, Some(String::new())] {
            let host = key_host(key_path);
            assert!(
                !host_key_requires_passphrase(&host),
                "there is no key file to unlock"
            );
        }

        // A path that does not resolve to a readable key is not an encrypted
        // key either — it must not send the user to a passphrase prompt.
        let missing = key_host(Some(
            std::env::temp_dir()
                .join("termlet-definitely-missing-key")
                .to_string_lossy()
                .into_owned(),
        ));
        assert!(!host_key_requires_passphrase(&missing));
    }

    #[test]
    fn an_encrypted_key_asks_for_a_passphrase_and_a_plain_one_does_not() {
        let dir = scratch_dir("passphrase");
        let plain_path = dir.join("id_ed25519");
        crate::keychain::keygen::generate_keypair(
            &plain_path,
            crate::core::data::KeyType::ED25519,
            &SecretString::from(String::new()),
            "termlet-test",
        )
        .expect("the plain key is generated");
        let mut host = key_host(Some(plain_path.to_string_lossy().into_owned()));
        assert!(
            !host_key_requires_passphrase(&host),
            "an unencrypted key must not prompt"
        );

        let encrypted_path = dir.join("id_ed25519_encrypted");
        crate::keychain::keygen::generate_keypair(
            &encrypted_path,
            crate::core::data::KeyType::ED25519,
            &SecretString::from("secret"),
            "termlet-test",
        )
        .expect("the encrypted key is generated");
        host.key_path = Some(encrypted_path.to_string_lossy().into_owned());
        assert!(
            host_key_requires_passphrase(&host),
            "an encrypted key must prompt for its passphrase"
        );

        // Once the passphrase for this attempt is known, the prompt is done.
        host.key_passphrase = Some(SecretString::from("secret"));
        assert!(!host_key_requires_passphrase(&host));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_auth_prompt_labels_the_host_it_is_authenticating() {
        let target = key_host(Some("~/.ssh/id_ed25519".into()));
        let mut jump = target.clone();
        jump.hostname = "bastion.example.com".into();
        jump.username = "jump".into();

        let mut state = crate::ssh_connect::SshConnectState {
            attempt: crate::ssh_connect::ConnectionAttempt::new(target, Some(jump)),
            ..Default::default()
        };

        // Before the jump hop is done, the prompt is about the bastion.
        state.attempt.stage = SshConnectStage::Jump;
        assert!(is_jump_stage(&state));
        assert_eq!(current_auth_label(&state), "jump@bastion.example.com:22");

        // Afterwards it moves on to the target host.
        state.attempt.stage = SshConnectStage::Target;
        assert!(!is_jump_stage(&state));
        assert_eq!(current_auth_label(&state), "root@example.com:22");

        // With nothing to authenticate, the label stays empty rather than
        // rendering a stray "@:".
        assert_eq!(
            current_auth_label(&crate::ssh_connect::SshConnectState::default()),
            ""
        );
    }
}
