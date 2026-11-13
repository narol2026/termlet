//! Terminal tab actions and inline rename updates.

use iced::Task;

use super::{
    App, DomainMessage, close_terminal_guarded, close_terminal_tab_id, close_terminal_target,
    create_local_session, create_ssh_session, ssh_bridge_unavailable_task,
};
use crate::core::data::LogCategory;
use crate::core::ids::TabId;
use crate::core::message::Message;
use crate::terminal::{SshSessionSecrets, TabKind};

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::TermClose(session_id) => {
            app.terminal.context_menu = None;
            if app.terminal.editing_tab == Some(session_id) {
                app.terminal.editing_tab = None;
                app.terminal.editing_tab_text = String::new();
            }
            close_terminal_guarded(app, crate::terminal::TerminalCloseTarget::Tab(session_id))
        }
        DomainMessage::TermToggleLock(session_id) => {
            let locked = !app.tab_is_locked(session_id);
            app.set_tab_locked(session_id, locked);
            app.terminal.context_menu = None;
            Task::none()
        }
        DomainMessage::TermCloseLockedConfirm => match app.pending_locked_close.take() {
            Some(pending) => close_terminal_target(app, &pending.target),
            None => Task::none(),
        },
        DomainMessage::TermCloseLockedCancel => {
            app.pending_locked_close = None;
            Task::none()
        }
        DomainMessage::DuplicateSession(session_id) => {
            app.terminal.context_menu = None;
            let host_id = app
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.host_id);
            let ssh_secrets = app
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .map_or_else(SshSessionSecrets::default, |s| s.ssh_secrets.clone());
            if let Some(id) = host_id {
                if let Some(host) = app.hosts.list.iter().find(|h| h.id == id).cloned() {
                    app.add_log(
                        LogCategory::Terminal,
                        format!("Duplicated terminal \"{}\"", host.name),
                    );
                    match create_ssh_session(app, &host, &ssh_secrets) {
                        Ok((_, task)) => task,
                        Err(error) => ssh_bridge_unavailable_task(app, &error),
                    }
                } else {
                    Task::none()
                }
            } else {
                app.add_log(LogCategory::Terminal, "Duplicated local terminal");
                create_local_session(app)
            }
        }
        DomainMessage::DuplicateInNewWindow(session_id) => {
            app.terminal.context_menu = None;
            let host_id = app
                .terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .and_then(|s| s.host_id);

            let (args, opened) = match host_id {
                Some(id) => (
                    vec!["--quick-ssh".to_string(), id.to_string()],
                    format!("Opened terminal in new window (host_id={id})"),
                ),
                None => (
                    vec!["--quick-local".to_string()],
                    "Opened local terminal in new window".to_string(),
                ),
            };

            match crate::core::process::spawn_window(&args) {
                Ok(()) => app.add_log(LogCategory::Terminal, opened),
                Err(error) => app.add_log(
                    LogCategory::Terminal,
                    format!("Failed to open new window: {error}"),
                ),
            }
            Task::none()
        }
        DomainMessage::TermContextMenuOpen(session_id) => {
            let visible_tabs = app.visible_tab_ids();
            let pos = app
                .tab_id_for_session(session_id)
                .and_then(|tid| visible_tabs.iter().position(|t| *t == tid));
            let visible_count = visible_tabs.len();
            let has_tabs_to_right = pos.is_some_and(|idx| visible_count > idx + 1);

            app.terminal.context_menu = Some(crate::terminal::TerminalContextMenu {
                session_id,
                position: app.window.cursor_pos,
                has_tabs_to_right,
                is_split: app.split_for_session(session_id).is_some(),
                locked: app.tab_is_locked(session_id),
                cursor: crate::core::list_cursor::ListCursor::new(),
            });
            if let Some(menu) = app.terminal.context_menu.as_mut() {
                menu.cursor = crate::context_menu::initial_cursor(&menu.actions());
            }
            Task::none()
        }
        DomainMessage::TermContextMenuClose => {
            app.terminal.context_menu = None;
            Task::none()
        }
        DomainMessage::TermCloseOthers(session_id) => {
            app.terminal.context_menu = None;

            // Resolve the tab hosting this session (standalone or split); every
            // other terminal tab is closed, drafts are left untouched.
            let keep_tid = app.tab_id_for_session(session_id).or_else(|| {
                app.split_for_session(session_id)
                    .and_then(|sid| app.tab_id_for_split(sid))
            });

            let tabs_to_close: Vec<TabId> = app
                .terminal
                .tabs
                .iter()
                .copied()
                .filter(|tid| Some(*tid) != keep_tid)
                .filter(|tid| {
                    matches!(
                        app.terminal.tab_registry.get(tid).map(|t| t.kind),
                        Some(TabKind::Session(_) | TabKind::Split(_))
                    )
                })
                .filter(|tid| !app.tab_is_locked_by_id(*tid))
                .collect();

            app.add_log(
                LogCategory::Terminal,
                format!(
                    "Close others (keep session_id={}, closing {} tabs)",
                    session_id,
                    tabs_to_close.len()
                ),
            );

            let tasks: Vec<Task<Message>> = tabs_to_close
                .into_iter()
                .map(|tid| close_terminal_tab_id(app, tid))
                .collect();
            Task::batch(tasks)
        }
        DomainMessage::TermCloseRight(session_id) => {
            app.terminal.context_menu = None;

            let keep_tid = app.tab_id_for_session(session_id).or_else(|| {
                app.split_for_session(session_id)
                    .and_then(|sid| app.tab_id_for_split(sid))
            });
            let pos = keep_tid.and_then(|tid| app.terminal.tabs.iter().position(|&t| t == tid));
            let tabs_to_close: Vec<TabId> = if let Some(idx) = pos {
                app.terminal.tabs.iter().copied().skip(idx + 1).collect()
            } else {
                Vec::new()
            };

            let (terminal_tabs, new_tab_ids) = tabs_to_close.into_iter().fold(
                (Vec::new(), Vec::new()),
                |(mut terms, mut drafts), tid| {
                    match app.terminal.tab_registry.get(&tid).map(|tab| tab.kind) {
                        Some(TabKind::Session(_) | TabKind::Split(_))
                            if !app.tab_is_locked_by_id(tid) =>
                        {
                            terms.push(tid);
                        }
                        Some(TabKind::Session(_) | TabKind::Split(_)) => {}
                        Some(TabKind::Draft(did)) => drafts.push(did),
                        None => {}
                    }
                    (terms, drafts)
                },
            );

            app.add_log(
                LogCategory::Terminal,
                format!(
                    "Close to the right (session_id={}, closing {} terminal tabs and {} new-tab placeholders)",
                    session_id,
                    terminal_tabs.len(),
                    new_tab_ids.len(),
                ),
            );

            let mut tasks: Vec<Task<Message>> = terminal_tabs
                .into_iter()
                .map(|tid| close_terminal_tab_id(app, tid))
                .collect();
            tasks.extend(
                new_tab_ids
                    .into_iter()
                    .map(|id| Message::Hosts(crate::hosts::message::Message::CloseNewTab(id)))
                    .map(Task::done),
            );
            Task::batch(tasks)
        }
        DomainMessage::TermRenameStart(session_id) => {
            app.terminal.context_menu = None;
            // If already editing another tab, submit that rename first.
            if app.terminal.editing_tab != Some(session_id)
                && let Some(previous_id) = app.terminal.editing_tab
            {
                let trimmed = app.terminal.editing_tab_text.trim().to_string();
                if !trimmed.is_empty()
                    && let Some(session) = app
                        .terminal
                        .sessions
                        .iter_mut()
                        .find(|s| s.id == previous_id)
                {
                    session.label = trimmed;
                }
            }
            if let Some(session) = app.terminal.sessions.iter().find(|s| s.id == session_id) {
                app.terminal.editing_tab_text = session.label.clone();
            }
            app.terminal.editing_tab = Some(session_id);
            let input_id = iced::widget::Id::from(format!("tab_rename_input_{session_id}"));
            iced::widget::operation::select_all(input_id)
        }
        DomainMessage::TermRenameInputChanged(text) => {
            app.terminal.editing_tab_text = text;
            Task::none()
        }
        DomainMessage::TermRenameSubmit(session_id) => {
            let trimmed = app.terminal.editing_tab_text.trim().to_string();
            if !trimmed.is_empty()
                && let Some(session) = app
                    .terminal
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == session_id)
            {
                session.label = trimmed;
            }
            app.terminal.editing_tab = None;
            app.terminal.editing_tab_text = String::new();
            Task::none()
        }
        DomainMessage::TermRenameCancel => {
            app.terminal.editing_tab = None;
            app.terminal.editing_tab_text = String::new();
            Task::none()
        }
        DomainMessage::TermRenameBlurCheck => {
            if let Some(session_id) = app.terminal.editing_tab {
                let id = iced::widget::Id::from(format!("tab_rename_input_{session_id}"));
                return iced::widget::operation::is_focused(id).map(move |focused| {
                    if focused {
                        Message::Noop
                    } else {
                        Message::Terminal(crate::terminal::message::Message::TermRenameSubmit(
                            session_id,
                        ))
                    }
                });
            }
            Task::none()
        }
        _ => unreachable!("terminal tab handler received another message"),
    }
}
