//! Terminal session focus, reconnect, startup, and lifecycle updates.

use iced::Task;

use super::launch::{app_terminal_bindings, ssh_terminal_settings};
use super::{
    App, DomainMessage, apply_terminal_focus, close_session, close_split_pane_by_session,
    create_local_session, create_ssh_session, poll_ssh_health, request_terminal_focus,
    ssh_bridge_unavailable_task, ssh_health_path,
};
use crate::app::SessionId;
use crate::core::data::LogCategory;
use crate::core::message::Message;
use crate::core::theme;
use crate::terminal::SshSessionSecrets;
use crate::terminal::state::AUTO_RECONNECT_DELAY;

/// Schedule the next automatic reconnect for the current outage generation.
pub(super) fn schedule_auto_reconnect(session_id: SessionId, generation: u64) -> Task<Message> {
    Task::perform(
        async move {
            tokio::time::sleep(AUTO_RECONNECT_DELAY).await;
            (session_id, generation)
        },
        |(session_id, generation)| {
            Message::Terminal(DomainMessage::DisconnectedAutoReconnect {
                session_id,
                generation,
            })
        },
    )
}

/// Reconnect an SSH terminal in place while preserving its tab and session id.
fn reconnect(app: &mut App, session_id: SessionId, source: &str) -> Task<Message> {
    let host_id = app
        .terminal
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .and_then(|session| session.host_id);
    let Some(host_id) = host_id else {
        app.add_log(
            LogCategory::Terminal,
            format!("Reconnect ({source}): session_id={session_id} has no host_id"),
        );
        return Task::none();
    };
    let Some(host) = app
        .hosts
        .list
        .iter()
        .find(|host| host.id == host_id)
        .cloned()
    else {
        app.add_log(
            LogCategory::Terminal,
            format!("Reconnect ({source}): host_id={host_id} not found"),
        );
        return Task::none();
    };
    let ssh_secrets = app
        .terminal
        .sessions
        .iter()
        .find(|session| session.id == session_id)
        .map_or_else(SshSessionSecrets::default, |session| {
            session.ssh_secrets.clone()
        });
    let label = if host.name.is_empty() {
        host.hostname.clone()
    } else {
        host.name.clone()
    };
    app.add_log(
        LogCategory::Terminal,
        format!(
            "Reconnecting SSH session in-place ({source}): session_id={session_id}, host={}@{}:{}, shell_integration={}",
            host.username,
            host.hostname,
            host.port,
            app.preferences.shell_integration
        ),
    );

    // Replace the terminal widget in place and preserve the session id.
    // Delete the stale health file so a fresh bridge never inherits the
    // previous connection's unstable state.
    let health_path = ssh_health_path(session_id);
    let _ = std::fs::remove_file(&health_path);
    let term_palette = theme::term_palette(app.effective_theme_mode());
    let settings = match ssh_terminal_settings(
        &host,
        &ssh_secrets,
        term_palette,
        app.preferences.terminal_font_size,
        Some(&health_path),
        app.preferences.shell_integration,
    ) {
        Ok(settings) => settings,
        Err(error) => {
            let task = ssh_bridge_unavailable_task(app, &error);
            return retry_after_failure(app, session_id, task);
        }
    };
    match iced_term::Terminal::new(u64::from(session_id.0), settings) {
        Ok(mut terminal) => {
            terminal.handle(iced_term::Command::AddBindings(app_terminal_bindings()));
            let focus = request_terminal_focus(app, session_id);

            let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            else {
                app.add_log(
                    LogCategory::Terminal,
                    format!(
                        "Reconnect: terminal session_id={session_id} disappeared after creating its PTY"
                    ),
                );
                return Task::none();
            };
            session.terminal = terminal;
            session.label = label;
            session.disconnected = false;
            session.disconnect_dismissed = false;
            session.disconnect_anim = 0.0;
            session.connection_unstable = false;
            session.osc_integration_detected = false;
            focus
        }
        Err(error) => {
            app.add_log(
                LogCategory::Terminal,
                format!("Reconnect failed to create PTY: session_id={session_id}: {error}"),
            );
            retry_after_failure(app, session_id, Task::none())
        }
    }
}

/// Keep the terminal disconnected and schedule the next outage attempt.
fn retry_after_failure(app: &mut App, session_id: SessionId, task: Task<Message>) -> Task<Message> {
    let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return task;
    };
    session.disconnected = true;
    session.disconnect_anim = 0.0;

    let Some(generation) = session.auto_reconnect.begin_outage() else {
        app.add_log(
            LogCategory::Terminal,
            format!("Automatic SSH reconnect exhausted for session_id={session_id}"),
        );
        return task;
    };
    Task::batch([task, schedule_auto_reconnect(session_id, generation)])
}

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::FocusTerminalWidget(session_id) => apply_terminal_focus(app, session_id),
        DomainMessage::DisconnectedReconnect(session_id) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            {
                session.auto_reconnect.reset_for_manual_attempt();
            }
            reconnect(app, session_id, "manual")
        }
        DomainMessage::DisconnectedAutoReconnect {
            session_id,
            generation,
        } => {
            let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
            else {
                return Task::none();
            };
            if !session.disconnected || !session.auto_reconnect.begin_attempt(generation) {
                return Task::none();
            }
            app.add_log(
                LogCategory::Terminal,
                format!("Automatic SSH reconnect attempt for session_id={session_id}"),
            );
            reconnect(app, session_id, "automatic")
        }
        DomainMessage::DisconnectedDismiss(session_id) => {
            let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
            else {
                return Task::none();
            };
            if !session.disconnected {
                return Task::none();
            }
            session.disconnect_dismissed = true;
            session.disconnect_anim = 0.0;
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "User dismissed disconnected terminal dialog: session_id={session_id}, terminal kept for review"
                ),
            );
            request_terminal_focus(app, session_id)
        }
        DomainMessage::DisconnectedClose(session_id) => {
            app.add_log(
                LogCategory::Terminal,
                format!("User closed disconnected terminal: session_id={session_id}"),
            );
            close_split_pane_by_session(app, session_id)
                .unwrap_or_else(|| close_session(app, session_id))
        }
        DomainMessage::StartupOpenSsh(host_id) => {
            if let Some(host) = app.hosts.list.iter().find(|h| h.id == host_id).cloned() {
                app.add_log(
                    LogCategory::Terminal,
                    format!("Startup SSH session to \"{}\"", host.name),
                );
                match create_ssh_session(app, &host, &SshSessionSecrets::default()) {
                    Ok((_, task)) => task,
                    Err(error) => ssh_bridge_unavailable_task(app, &error),
                }
            } else {
                Task::perform(
                    async move { format!("Host #{host_id} not found") },
                    Message::ShowWarningToast,
                )
            }
        }
        DomainMessage::StartupOpenLocal => {
            app.add_log(LogCategory::Terminal, "Startup local terminal session");
            create_local_session(app)
        }
        DomainMessage::RestoreWorkspace(snapshot) => {
            crate::workspace::restore_workspace(app, &snapshot)
        }
        DomainMessage::SshHealthTick => {
            poll_ssh_health(app);
            Task::none()
        }
        _ => unreachable!("terminal session handler received another message"),
    }
}
