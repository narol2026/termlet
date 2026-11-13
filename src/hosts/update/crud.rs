//! Handles saved-host collection and record lifecycle messages.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::App;
use crate::core::data::{HostId, LogCategory};
use crate::core::message::Message;
use crate::hosts::message::Message as DomainMessage;
use crate::update::restore_scrolls;
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::SearchChanged(query) => app.hosts.search_query = query,
        DomainMessage::SelectHost(id) => select(app, id),
        // Cards under an open toolbar menu must not receive hover styling.
        DomainMessage::HostHoverEnter(id) if !toolbar_menu_open(app) => {
            app.hosts.hovered_id = Some(id);
        }
        DomainMessage::HostHoverLeave(id) if app.hosts.hovered_id == Some(id) => {
            app.hosts.hovered_id = None;
        }
        DomainMessage::EditHost(id) => return edit(app, id),
        DomainMessage::HostDeleteRequest(id) => {
            app.hosts.context_menu = None;
            app.hosts.delete_pending = Some(id);
        }
        DomainMessage::HostDeleteCancel => {
            app.hosts.delete_pending = None;
        }
        DomainMessage::DeleteHost(id) => return delete(app, id),
        DomainMessage::DuplicateHost(id) => return duplicate(app, id),
        DomainMessage::HostsContextMenuOpen(id) => {
            app.hosts.cursor.select(id);
            app.hosts.context_menu = Some(crate::hosts::HostsContextMenu {
                host_id: id,
                position: app.window.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::HostEdit,
                    crate::core::context_menu::ContextMenuAction::HostDuplicate,
                    crate::core::context_menu::ContextMenuAction::HostDelete,
                ]),
            });
            return restore_scrolls(app);
        }
        DomainMessage::HostsContextMenuClose => {
            app.hosts.context_menu = None;
            return restore_scrolls(app);
        }
        DomainMessage::HostsViewCursorMoved(point) => {
            app.hosts.cursor_pos = point;
        }
        DomainMessage::ToggleView => {
            app.hosts.grid_view = !app.hosts.grid_view;
            let value = if app.hosts.grid_view { "true" } else { "false" };
            return app.storage_write_task(move |storage| {
                storage.save_preference("hosts_grid_view", value)
            });
        }
        DomainMessage::HostCreated { host } | DomainMessage::HostDuplicated { host } => {
            app.hosts.list.insert(0, host.clone());
            app.hosts.record_recent(host.id);
        }
        DomainMessage::OsDetected { host_id, os_type } => {
            if let Some(host) = app.hosts.list.iter_mut().find(|host| host.id == host_id) {
                host.os_type = os_type;
                let host = host.clone();
                return app.storage_write_task(move |storage| storage.upsert_host(&host));
            }
        }
        DomainMessage::HostTagFilterToggle => {
            app.hosts.hovered_id = None;
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostsTagFilter,
            );
        }
        DomainMessage::HostTagFilterSelect(filter) => {
            app.hosts.tag_filter = filter;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostsTagFilter,
            );
            app.hosts.hovered_id = None;
        }
        _ => {}
    }
    Task::none()
}

fn select(app: &mut App, id: HostId) {
    app.hosts.cursor.select(id);
    if app.hosts.form.visible && !app.hosts.form_closing {
        super::form::select_host(app, id);
    }
}

fn edit(app: &mut App, id: HostId) -> Task<Message> {
    app.hosts.context_menu = None;
    if app.hosts.list.iter().any(|host| host.id == id) {
        app.hosts.cursor.select(id);
    }
    if app.hosts.form.visible && !app.hosts.form_closing && app.hosts.form.editing_id == Some(id) {
        return Task::none();
    }

    let was_visible = app.hosts.form.visible && !app.hosts.form_closing;
    super::form::edit_host(app, id);
    if app.hosts.form.editing_id == Some(id) {
        app.hosts.form.visible = true;
        app.hosts.form_anim = if was_visible { 1.0 } else { 0.0 };
        app.hosts.form_closing = false;
    }
    Task::none()
}

fn delete(app: &mut App, id: HostId) -> Task<Message> {
    let name = app
        .hosts
        .list
        .iter()
        .find(|host| host.id == id)
        .map(|host| host.name.clone())
        .unwrap_or_default();
    app.hosts.list.retain(|host| host.id != id);
    for host in &mut app.hosts.list {
        if host.jump_host_id == Some(id) {
            host.jump_host_id = None;
        }
    }
    app.hosts.recent_ids.retain(|recent_id| *recent_id != id);
    if app.hosts.cursor.selected_key() == Some(&id) {
        app.hosts.cursor.clear();
    }
    if app.hosts.form.editing_id == Some(id) {
        app.hosts.form.visible = false;
        app.hosts.form.reset();
        app.needs_scroll_restore = true;
    }
    app.hosts.delete_pending = None;
    app.add_log(
        LogCategory::Host,
        t!("hosts.deleted_log", name = &name).to_string(),
    );
    crate::port_forward::update::delete_forwards_for_host(app, id);
    let recent_ids = app.hosts.recent_ids.clone();
    app.storage_task(
        move |storage| {
            storage.delete_host(id)?;
            storage.save_recent_ids(&recent_ids)
        },
        move |result| match result {
            Ok(()) => Message::ShowToast(t!("hosts.deleted_toast", name = &name).to_string()),
            Err(error) => Message::ShowErrorToast(error),
        },
    )
}

fn duplicate(app: &mut App, id: HostId) -> Task<Message> {
    app.hosts.context_menu = None;
    let Some(source) = app.hosts.list.iter().find(|host| host.id == id).cloned() else {
        return Task::none();
    };
    let recent_ids = app.hosts.recent_ids.clone();
    app.storage_task(
        move |storage| {
            let new_id = storage.next_host_id()?;
            let mut duplicate = source.clone();
            duplicate.id = new_id;
            duplicate.name = next_copy_name(&source.name);
            storage.upsert_host(&duplicate)?;
            storage.save_recent_ids(&recent_ids)?;
            Ok(duplicate)
        },
        |result| match result {
            Ok(host) => Message::Hosts(crate::hosts::message::Message::HostDuplicated { host }),
            Err(error) => Message::ShowErrorToast(error),
        },
    )
}

fn toolbar_menu_open(app: &App) -> bool {
    matches!(
        app.active_dropdown,
        Some(
            crate::core::dropdown::DropdownTarget::HostsTransferMenu
                | crate::core::dropdown::DropdownTarget::HostsTagFilter
        )
    )
}

pub(super) fn next_copy_name(name: &str) -> String {
    if let Some(stripped) = name.strip_suffix(" Copy") {
        return format!("{stripped} Copy 2");
    }
    if let Some((prefix, number)) = name.rsplit_once(" Copy ")
        && let Ok(number) = number.parse::<usize>()
    {
        return format!("{prefix} Copy {}", number + 1);
    }
    format!("{name} Copy")
}
