//! Right-sidebar, command-history, and spotlight updates.

use iced::Task;

use super::{
    App, Message, execute_spotlight_command, open_right_sidebar, request_right_sidebar_focus,
    set_command_history_hover, spotlight_move_selection, update_active_sidebars,
};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::LogCategory;
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::ToggleRightSidebar => {
            // A split shares one sidebar across all of its panes, so toggling
            // moves every pane together. A session outside any split keeps its
            // own sidebar, even when a split exists for other sessions.
            if app.is_active_session_in_split() {
                let pane_sessions = app.active_split_pane_sessions();
                let any_open = pane_sessions.iter().any(|sid| {
                    app.terminal
                        .sessions
                        .iter()
                        .any(|s| s.id == *sid && s.sidebar.show)
                });
                if any_open {
                    for sid in &pane_sessions {
                        if let Some(s) = app.terminal.sessions.iter_mut().find(|s| s.id == *sid)
                            && s.sidebar.show
                        {
                            s.sidebar.closing = true;
                        }
                    }
                } else if let Some(focused_sid) = app.active_terminal_session_id()
                    && let Some(s) = app
                        .terminal
                        .sessions
                        .iter_mut()
                        .find(|s| s.id == focused_sid)
                {
                    open_right_sidebar(&mut s.sidebar);
                }
            } else if let Some(sidebar) = app.sidebar_mut() {
                if sidebar.show {
                    sidebar.closing = true;
                } else {
                    open_right_sidebar(sidebar);
                }
            }
            Task::none()
        }
        Message::SwitchRightTab(tab) => {
            // Sync the tab to every pane of the split, so the shared sidebar
            // state does not depend on which pane the view happens to read.
            update_active_sidebars(app, |sidebar| {
                sidebar.active_tab = tab;
                sidebar.focus_list_pending = None;
            });
            request_right_sidebar_focus(tab)
        }
        Message::FocusRightSidebarTab(tab) => {
            if app.sidebar().is_some_and(|sidebar| {
                sidebar.show && !sidebar.closing && sidebar.active_tab == tab
            }) {
                crate::focus::focus_base(
                    app,
                    crate::focus::FocusId::Cursor(CursorTarget::for_right_sidebar(tab)),
                )
            } else {
                Task::none()
            }
        }
        Message::RightSidebarSelectSnippet(id) => {
            update_active_sidebars(app, |sidebar| sidebar.cursors.select_snippet(id));
            Task::none()
        }
        Message::RightSidebarSelectCommand(command) => {
            update_active_sidebars(app, |sidebar| sidebar.cursors.select_command(&command));
            Task::none()
        }
        Message::CommandHistoryCopy(idx) => {
            let text = app.active_terminal_session_id().and_then(|session_id| {
                app.terminal
                    .sessions
                    .iter()
                    .find(|s| s.id == session_id)
                    .and_then(|s| s.recent_commands.get(idx))
                    .map(|entry| entry.command.clone())
            });
            match text {
                Some(text) => Task::batch([
                    iced::clipboard::write(text),
                    Task::done(Message::ShowToast(
                        t!("right_sidebar.command_copied").to_string(),
                    )),
                ]),
                None => Task::none(),
            }
        }
        Message::CommandHistoryRun(idx) => {
            // Writes the command to the terminal input and submits it with a
            // carriage return. Re-sorting history to the front happens here
            // rather than in the OSC 633 handler, so it also applies when the
            // shell emits nothing for the re-submitted line.
            let Some(session_id) = app.active_terminal_session_id() else {
                return Task::none();
            };
            let history_limit = app.preferences.command_history_limit;
            let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
            else {
                return Task::none();
            };
            if let Some(cmd) = session
                .recent_commands
                .get(idx)
                .map(|entry| entry.command.clone())
            {
                crate::terminal::history::record(session, &cmd, history_limit);
                let mut data = cmd.into_bytes();
                data.push(b'\r');
                session.terminal.handle(iced_term::Command::ProxyToBackend(
                    iced_term::BackendCommand::Write(data),
                ));
            }
            Task::none()
        }
        Message::CommandHistoryInfo(idx) => {
            if let Some(session_id) = app.active_terminal_session_id() {
                app.terminal.command_info = Some(crate::terminal::state::CommandInfo {
                    session_id,
                    index: idx,
                });
            }
            Task::none()
        }
        Message::CommandHistoryInfoClose => {
            app.terminal.command_info = None;
            Task::none()
        }
        Message::CommandHistoryFilter(query) => {
            update_active_sidebars(app, |sidebar| {
                sidebar.commands_filter.clone_from(&query);
                // A selection hidden by a new filter would otherwise remain
                // invisible until another query restored it.
                sidebar.cursors.clear_commands();
            });
            Task::none()
        }
        Message::CommandHistoryHoverEnter(idx) => {
            set_command_history_hover(app, Some(idx));
            Task::none()
        }
        Message::CommandHistoryHoverLeave(_) => {
            set_command_history_hover(app, None);
            Task::none()
        }
        Message::SpotlightShow(session_id) => {
            app.add_log(
                LogCategory::Terminal,
                format!("SpotlightShow: session_id={session_id} — unfocusing terminal for popup"),
            );
            let host_id = app
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.host_id);
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
            {
                let commands = crate::app::command::build_palette_commands(host_id);
                session.spotlight = Some(crate::terminal::SpotlightState::new(commands));
                return Task::none();
            }
            Task::none()
        }
        Message::SpotlightCancel(session_id) => {
            app.add_log(
                LogCategory::Terminal,
                format!("SpotlightCancel: session_id={session_id} — re-focusing terminal"),
            );
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
            {
                session.spotlight = None;
                return Task::none();
            }
            Task::none()
        }
        Message::SpotlightFilter(session_id, text) => {
            let host_id = app
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.host_id);
            let all = crate::app::command::build_palette_commands(host_id);
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                && let Some(popup) = &mut session.spotlight
            {
                let filtered = crate::app::command::filter_commands(&all, &text);
                popup.filter_text = text;
                popup.filtered_commands = filtered;
                // Keep the top match highlighted, as the Command Palette does.
                popup.reset_cursor();
                return Task::none();
            }
            Task::none()
        }
        Message::SpotlightMoveUp(session_id) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                && let Some(popup) = &mut session.spotlight
            {
                spotlight_move_selection(popup, false);
            }
            Task::none()
        }
        Message::SpotlightMoveDown(session_id) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                && let Some(popup) = &mut session.spotlight
            {
                spotlight_move_selection(popup, true);
            }
            Task::none()
        }
        Message::SpotlightPick(session_id, command_id) => {
            let kind = app
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.spotlight.as_ref())
                .and_then(|p| {
                    p.filtered_commands
                        .iter()
                        .find(|command| command.id == command_id)
                })
                .map(|c| c.kind.clone());
            match kind {
                Some(kind) => execute_spotlight_command(app, session_id, &kind),
                None => Task::none(),
            }
        }
        Message::SpotlightHover(session_id, hovered_index) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                && let Some(popup) = &mut session.spotlight
            {
                popup.hovered_index = hovered_index;
            }
            Task::none()
        }
        Message::SpotlightConfirm(session_id) => {
            let kind = app
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.spotlight.as_ref())
                .and_then(crate::terminal::SpotlightState::selected_command)
                .map(|c| c.kind.clone());
            match kind {
                Some(kind) => execute_spotlight_command(app, session_id, &kind),
                None => Task::none(),
            }
        }
        _ => unreachable!("sidebar handler received another message"),
    }
}
