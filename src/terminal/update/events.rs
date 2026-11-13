//! Terminal backend events and scrollbar timers.

use std::collections::HashSet;

use iced::Task;

use super::{
    App, DomainMessage, close_session, close_split_pane_by_session, command_exit_line,
    command_run_line, cwd_change_line, sessions::schedule_auto_reconnect, trace_backend_call,
};
use crate::app::SessionId;
use crate::core::data::LogCategory;
use crate::core::diagnostics::Level;
use crate::core::message::Message;
use crate::terminal::supports_osc_shell_integration;
use iced_term::actions::Action as TerminalAction;
use iced_term::{Command as TerminalCommand, OscMessage};

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::IcedTermEvent(iced_term::Event::BackendCall(term_id, cmd)) => {
            let session_id = SessionId(term_id as u32);
            let Some(session_idx) = app
                .terminal
                .sessions
                .iter()
                .position(|s| s.id == session_id)
            else {
                app.add_log(
                    LogCategory::Terminal,
                    format!(
                        "BackendCall for unknown session_id={session_id} (session already closed?)"
                    ),
                );
                return Task::none();
            };

            let osc_shell_integration =
                supports_osc_shell_integration(app.terminal.sessions[session_idx].host_id);

            if app.active_terminal_session_id() != Some(session_id) {
                app.terminal.sessions[session_idx].has_new_output = true;
            }

            // Any scroll (wheel, keyboard, drag) shows the scrollbar and arms a
            // 2 s timer; the generation counter keeps only the newest timer
            // able to hide it again.
            let is_scroll = matches!(cmd, iced_term::BackendCommand::Scroll(_));
            let mut scrollbar_hide_task: Option<Task<Message>> = None;
            if is_scroll {
                let session = &mut app.terminal.sessions[session_idx];
                session.term_scrollbar_generation += 1;
                let generation = session.term_scrollbar_generation;
                session.terminal.set_scrollbar_visible(true);
                scrollbar_hide_task = Some(Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        (session_id, generation)
                    },
                    |(sid, g)| {
                        Message::Terminal(crate::terminal::message::Message::HideTerminalScrollbar(
                            sid, g,
                        ))
                    },
                ));
            }

            let host_id = app.terminal.sessions[session_idx].host_id;
            let label = app.terminal.sessions[session_idx].label.clone();
            // Read the configured ring size up front: the loop below borrows the
            // session mutably, and the limit is a property of the app, not the
            // session.
            let history_limit = app.preferences.command_history_limit;

            trace_backend_call(session_id, &label, host_id, &cmd);

            // `merge_actions` keeps only the last reporting event of a batch, so
            // a fast command's OSC 633 ; C is shadowed by the trailing D / A /
            // OSC 7 and never reaches the merged action. Record every executed
            // command — and, for the same reason, its exit code — from the raw
            // events instead, and snapshot the saved snippets up front so the
            // frequency tracker can skip them.
            let saved_scripts: HashSet<String> = app
                .snippets
                .list
                .iter()
                .map(|s| s.script.trim().to_string())
                .collect();
            let mut command_frequency_notify: Option<Message> = None;
            let mut session_log_lines: Vec<String> = Vec::new();
            if osc_shell_integration
                && let iced_term::BackendCommand::ProcessAlacrittyEvents(events) = &cmd
            {
                let session = &mut app.terminal.sessions[session_idx];
                for event in events {
                    if matches!(event, iced_term::AlacrittyEvent::Osc(_)) {
                        session.osc_integration_detected = true;
                    }
                    match event {
                        iced_term::AlacrittyEvent::Osc(OscMessage::CommandExecuted(command)) => {
                            // The command may change the directory itself, or
                            // launch a nested shell in another directory (btop,
                            // vim's :shell, ssh, ...). Until the next OSC 7
                            // arrives, the cached CWD is no longer authoritative.
                            session.working_directory_synced = false;
                            crate::terminal::history::record(session, command, history_limit);
                            session_log_lines.push(command_run_line(session_id, command));
                            if let Some(hit) = crate::terminal::frequency::observe(
                                session.host_id,
                                command,
                                saved_scripts.contains(command.trim()),
                            ) {
                                command_frequency_notify = Some(Message::CommandFrequencyNotify {
                                    command: hit.command,
                                    count: hit.count,
                                });
                            }
                        }
                        iced_term::AlacrittyEvent::Osc(OscMessage::CommandFinished(code)) => {
                            if let Some(code) = code
                                && *code != 0
                            {
                                session_log_lines.push(command_exit_line(
                                    session_id,
                                    *code,
                                    crate::terminal::history::in_flight_command(
                                        session.working_directory_synced,
                                        &session.recent_commands,
                                    ),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            for line in session_log_lines {
                app.add_log(LogCategory::Terminal, line);
            }

            let action = app.terminal.sessions[session_idx]
                .terminal
                .handle(TerminalCommand::ProxyToBackend(cmd));

            let base_task = match action {
                TerminalAction::Shutdown => {
                    // For SSH sessions: show a per-terminal reconnect/close
                    // dialog instead of auto-closing the tab. Local terminals
                    // still close immediately.
                    if host_id.is_some() {
                        if app.terminal.sessions[session_idx].disconnected {
                            return Task::none();
                        }
                        let cwd_before_disconnect =
                            app.terminal.sessions[session_idx].working_directory.clone();
                        let cwd_synced_before_disconnect =
                            app.terminal.sessions[session_idx].working_directory_synced;
                        app.terminal.sessions[session_idx].disconnected = true;
                        app.terminal.sessions[session_idx].disconnect_dismissed = false;
                        app.terminal.sessions[session_idx].disconnect_anim = 0.0;
                        let retry_generation = app.terminal.sessions[session_idx]
                            .auto_reconnect
                            .begin_outage();
                        app.add_log(
                            LogCategory::Terminal,
                            format!(
                                "SSH terminal disconnected: session_id={}, host_id={:?}, label=\"{}\", cwd={:?}, cwd_synced={}, shell_integration={}, auto_reconnect={}",
                                session_id,
                                host_id,
                                label,
                                cwd_before_disconnect,
                                cwd_synced_before_disconnect,
                                app.preferences.shell_integration,
                                if retry_generation.is_some() {
                                    "scheduled"
                                } else {
                                    "exhausted"
                                }
                            ),
                        );
                        retry_generation.map_or_else(Task::none, |generation| {
                            schedule_auto_reconnect(session_id, generation)
                        })
                    } else {
                        let close_task = close_split_pane_by_session(app, session_id)
                            .unwrap_or_else(|| close_session(app, session_id));
                        app.add_log(
                            LogCategory::Terminal,
                            format!(
                                "Terminal session ended: session_id={session_id}, label=\"{label}\""
                            ),
                        );
                        let toast_message = "Terminal session ended".to_string();
                        let toast_task =
                            Task::perform(async move { toast_message }, Message::ShowToast);
                        Task::batch([close_task, toast_task])
                    }
                }
                TerminalAction::ChangeTitle(_title) => {
                    // OSC 0/2 titles carry CWD only heuristically, and a wrong
                    // path is worse than none. OSC 7 is the sole authority.
                    Task::none()
                }
                TerminalAction::Osc(_) if !osc_shell_integration => Task::none(),
                TerminalAction::Osc(message) => match message {
                    OscMessage::WorkingDirectory(dir) => {
                        let mut previous_directory: Option<Option<String>> = None;
                        if let Some(session) = app.terminal.sessions.get_mut(session_idx) {
                            if session.working_directory.as_deref() != Some(dir.as_str()) {
                                previous_directory = Some(session.working_directory.clone());
                            }
                            session.working_directory = Some(dir.clone());
                            session.working_directory_synced = true;

                            if let Some(hid) = session.host_id
                                && let Err(error) = crate::core::paths::write_private_atomic(
                                    &crate::core::paths::cwd_cache_file(hid),
                                    dir.as_bytes(),
                                )
                            {
                                crate::core::diagnostics::log(
                                    Level::Warn,
                                    "TERM",
                                    format!(
                                        "failed to write cwd cache for session_id={session_id}, host_id={hid}: {error}"
                                    ),
                                );
                            }
                        }
                        if let Some(previous) = previous_directory {
                            app.add_log(
                                LogCategory::Terminal,
                                cwd_change_line(session_id, previous.as_deref(), &dir),
                            );
                        }
                        Task::none()
                    }
                    OscMessage::PromptStart => Task::none(),
                    OscMessage::CommandExecuted(_command) => {
                        // The raw-events scan above already recorded this
                        // command.
                        app.terminal.sessions[session_idx].working_directory_synced = false;
                        Task::none()
                    }
                    _ => Task::none(),
                },
                TerminalAction::Ignore => Task::none(),
            };

            let mut tasks = vec![base_task];
            if let Some(msg) = command_frequency_notify {
                // Delay the "used N times" prompt slightly so it does not cover
                // freshly printed output.
                tasks.push(Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        msg
                    },
                    |msg| msg,
                ));
            }
            if let Some(hide_task) = scrollbar_hide_task {
                tasks.push(hide_task);
            }
            Task::batch(tasks)
        }
        DomainMessage::HideTerminalScrollbar(session_id, expected_gen) => {
            // A newer scroll bumps the generation, and this timer is stale.
            if let Some(session) = app.terminal.sessions.iter().find(|s| s.id == session_id)
                && session.term_scrollbar_generation == expected_gen
            {
                session.terminal.set_scrollbar_visible(false);
            }
            Task::none()
        }
        _ => unreachable!("terminal event handler received another message"),
    }
}
