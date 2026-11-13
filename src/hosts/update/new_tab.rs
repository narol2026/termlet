//! Handles new-tab host search and draft lifecycle.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::{App, DraftId};
use crate::core::data::{Host, LogCategory};
use crate::core::message::Message;
use crate::hosts::message::Message as DomainMessage;
use crate::terminal::{Draft, TabKind};
use crate::update::scroll_to_active_tab;
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::OpenNewTab => {
            super::navigation::close_popups(app);
            let draft_id = allocate_draft(app);
            let tab_id = app.allocate_tab_id(TabKind::Draft(draft_id));
            app.seed_tab_width_anims();
            app.activate_tab(tab_id);
            scroll_to_active_tab(app)
        }
        DomainMessage::NewTabQueryChanged(query) => {
            let Some(draft_id) = app.active_draft_id() else {
                return Task::none();
            };
            if let Some(draft) = app.terminal.drafts.get_mut(&draft_id) {
                draft.query = query;
                // The next ArrowDown should select the first new result.
                draft.cursor.clear();
            }
            Task::none()
        }
        DomainMessage::NewTabSelectUp => move_selection(app, false),
        DomainMessage::NewTabSelectDown => move_selection(app, true),
        DomainMessage::NewTabConnect => connect_selected_host(app),
        DomainMessage::CloseNewTab(draft_id) => close_draft(app, draft_id),
        DomainMessage::NewTabHovered { id, entered } => {
            if entered {
                app.terminal.hovered_new_tab_id = Some(id);
            } else if app.terminal.hovered_new_tab_id == Some(id) {
                app.terminal.hovered_new_tab_id = None;
            }
            Task::none()
        }
        _ => Task::none(),
    }
}

pub(super) fn allocate_draft(app: &mut App) -> DraftId {
    let draft_id = DraftId(app.terminal.next_draft_id as u32);
    app.terminal.next_draft_id += 1;
    app.terminal.drafts.insert(
        draft_id,
        Draft {
            query: String::new(),
            cursor: crate::core::list_cursor::ListCursor::new(),
            ssh: crate::ssh_connect::SshConnectState::default(),
        },
    );
    draft_id
}

fn move_selection(app: &mut App, forward: bool) -> Task<Message> {
    let Some(draft_id) = app.active_draft_id() else {
        return Task::none();
    };
    let query = app
        .terminal
        .drafts
        .get(&draft_id)
        .map(|draft| draft.query.clone())
        .unwrap_or_default();
    let host_ids = {
        let hosts = crate::views::new_tab::new_tab_hosts(app, query.as_str());
        hosts.iter().map(|host| host.id).collect::<Vec<_>>()
    };
    if host_ids.is_empty() {
        return Task::none();
    }
    let Some(draft) = app.terminal.drafts.get_mut(&draft_id) else {
        return Task::none();
    };
    if forward {
        draft.cursor.move_next(&host_ids);
    } else {
        draft.cursor.move_previous(&host_ids);
    }
    Task::none()
}

fn connect_selected_host(app: &mut App) -> Task<Message> {
    // Resolve from the same ordered list the view draws. The cursor is a
    // preference; the first visible host is the fallback.
    let selected_host = app.active_draft_id().and_then(|draft_id| {
        let draft = app.terminal.drafts.get(&draft_id)?;
        let hosts = crate::views::new_tab::new_tab_hosts(app, &draft.query);
        let host_ids = hosts.iter().map(|host| host.id).collect::<Vec<_>>();
        let host_id = draft.cursor.selected_or_first(&host_ids).copied()?;
        hosts
            .iter()
            .find(|host| host.id == host_id)
            .map(|host| (**host).clone())
    });
    if let Some(host) = &selected_host {
        app.hosts.record_recent(host.id);
    }

    let Some(host) = selected_host.or_else(|| resolve_new_tab_host(app)) else {
        return Task::done(Message::ShowErrorToast(
            t!("new_tab.no_matching_hosts").to_string(),
        ));
    };

    let recent_ids = app.hosts.recent_ids.clone();
    let recent_save = app.storage_write_task(move |storage| storage.save_recent_ids(&recent_ids));
    let draft_id = app.active_draft_id();
    super::connect::start_ssh_connect(app, host, draft_id);
    recent_save
}

fn close_draft(app: &mut App, draft_id: DraftId) -> Task<Message> {
    let log_line = app.terminal.drafts.get(&draft_id).and_then(|draft| {
        draft.ssh.attempt.pending_host.as_ref().map(|host| {
            format!(
                "CloseNewTab: closing SSH connection to {}@{}:{} (state was {:?})",
                host.username, host.hostname, host.port, draft.ssh.attempt.state
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
    Task::none()
}

pub(super) fn resolve_new_tab_host(app: &mut App) -> Option<Host> {
    let active_id = app.active_draft_id()?;
    let query = app
        .terminal
        .drafts
        .get(&active_id)
        .map(|draft| draft.query.trim().to_lowercase())
        .unwrap_or_default();

    let host = app
        .hosts
        .list
        .iter()
        .find(|host| {
            host.name.to_lowercase().contains(&query)
                || host.hostname.to_lowercase().contains(&query)
                || host.username.to_lowercase().contains(&query)
        })
        .cloned()?;
    app.hosts.record_recent(host.id);
    Some(host)
}
