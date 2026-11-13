//! Port-forward collection and form message handling.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::App;
use crate::core::data::{LogCategory, PortForward, PortForwardId, PortForwardKind};
use crate::core::forms::FormDraft;
use crate::core::message::Message;
use crate::port_forward::message::Message as DomainMessage;
use crate::update::restore_scrolls;
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::PortForwardSearchChanged(query) => {
            app.port_forwards.search_query = query;
        }
        DomainMessage::PortForwardShowForm => {
            app.port_forwards.cursor.clear();
            app.port_forwards.logs_open = false;
            let was_visible = app.port_forwards.form.visible && !app.port_forwards.form_closing;
            app.port_forwards.form.reset();
            app.port_forwards.form.visible = true;
            app.port_forwards.form_anim = if was_visible { 1.0 } else { 0.0 };
            app.port_forwards.form_closing = false;
            app.port_forwards.form.host_id = app.hosts.list.first().map(|host| host.id);
        }
        DomainMessage::PortForwardHideForm => {
            crate::cursor::close_active_dropdown(app);
            app.port_forwards.form_closing = true;
        }
        DomainMessage::PortForwardEdit(id) => {
            app.port_forwards.context_menu = None;
            open_editor(app, id);
        }
        DomainMessage::PortForwardDeleteRequest(id) => {
            app.port_forwards.context_menu = None;
            app.port_forwards.delete_pending = Some(id);
            return restore_scrolls(app);
        }
        DomainMessage::PortForwardDeleteCancel => {
            app.port_forwards.delete_pending = None;
            return restore_scrolls(app);
        }
        DomainMessage::PortForwardDelete(id) => return delete(app, id),
        DomainMessage::PortForwardSelect(id) => activate(app, id),
        DomainMessage::PortForwardShowLogs(id) => {
            app.port_forwards.context_menu = None;
            app.port_forwards.cursor.select(id);
            app.port_forwards.logs_open = true;
            return restore_scrolls(app);
        }
        DomainMessage::PortForwardHideLogs => {
            app.port_forwards.logs_open = false;
            return restore_scrolls(app);
        }
        DomainMessage::PortForwardCopyLogs(id) => {
            let text = app
                .port_forwards
                .runtime_for(id)
                .map(|runtime| runtime.logs.join("\n"))
                .unwrap_or_default();
            if !text.is_empty() {
                return Task::batch([
                    iced::clipboard::write(text),
                    Task::done(Message::ShowToast(t!("app.logs_copied").to_string())),
                ]);
            }
        }
        DomainMessage::PortForwardFormName(value) => app.port_forwards.form.name = value,
        DomainMessage::PortForwardFormKind(value) => app.port_forwards.form.kind = value,
        DomainMessage::PortForwardFormHost(id) => {
            app.port_forwards.form.host_id = Some(id);
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::PortForwardHost,
            );
        }
        DomainMessage::PortForwardFormBindAddress(value) => {
            app.port_forwards.form.bind_address = value;
        }
        DomainMessage::PortForwardFormListenPort(value) => {
            app.port_forwards.form.listen_port = value;
        }
        DomainMessage::PortForwardFormDestHost(value) => {
            app.port_forwards.form.dest_host = value;
        }
        DomainMessage::PortForwardFormDestPort(value) => {
            app.port_forwards.form.dest_port = value;
        }
        DomainMessage::PortForwardFormToggleHostPicker => {
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::PortForwardHost,
            );
        }
        DomainMessage::PortForwardFormHostSearch(value) => {
            app.port_forwards.form.host_picker.set_query(value);
        }
        DomainMessage::PortForwardFormSubmit => return submit_with_warning(app),
        DomainMessage::PortForwardToggleView => {
            app.port_forwards.grid_view = !app.port_forwards.grid_view;
            let value = if app.port_forwards.grid_view {
                "true"
            } else {
                "false"
            };
            return app.storage_write_task(move |storage| {
                storage.save_preference("port_forwards_grid_view", value)
            });
        }
        DomainMessage::PortForwardDuplicate(id) => return duplicate(app, id),
        DomainMessage::PortForwardCreated { forward } => {
            app.port_forwards.list.push(forward);
        }
        DomainMessage::PortForwardHoverEnter(id) => {
            app.port_forwards.hovered_id = Some(id);
        }
        DomainMessage::PortForwardHoverLeave(id) if app.port_forwards.hovered_id == Some(id) => {
            app.port_forwards.hovered_id = None;
        }
        DomainMessage::PortForwardContextMenuOpen(id) => {
            activate(app, id);
            app.port_forwards.context_menu = Some(crate::port_forward::PortForwardContextMenu {
                forward_id: id,
                position: app.window.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::PortForwardToggle,
                    crate::core::context_menu::ContextMenuAction::PortForwardEdit,
                    crate::core::context_menu::ContextMenuAction::PortForwardLogs,
                    crate::core::context_menu::ContextMenuAction::PortForwardDuplicate,
                    crate::core::context_menu::ContextMenuAction::PortForwardDelete,
                ]),
            });
            return restore_scrolls(app);
        }
        DomainMessage::PortForwardContextMenuClose => {
            app.port_forwards.context_menu = None;
            return restore_scrolls(app);
        }
        DomainMessage::PortForwardViewCursorMoved(point) => {
            app.port_forwards.cursor_pos = point;
        }
        _ => {}
    }
    Task::none()
}

fn delete(app: &mut App, id: PortForwardId) -> Task<Message> {
    let name = app
        .port_forwards
        .list
        .iter()
        .find(|forward| forward.id == id)
        .map(|forward| forward.name.clone())
        .unwrap_or_default();
    super::lifecycle::stop_by_id(app, id);
    app.port_forwards.list.retain(|forward| forward.id != id);
    app.port_forwards.runtime.remove(&id);
    if app.port_forwards.cursor.selected_key() == Some(&id) {
        app.port_forwards.cursor.clear();
        app.port_forwards.logs_open = false;
    }
    app.port_forwards.delete_pending = None;
    if app.port_forwards.form.editing_id == Some(id) {
        app.port_forwards.form.visible = false;
        app.port_forwards.form.reset();
        app.needs_scroll_restore = true;
    }
    app.add_log(
        LogCategory::PortForward,
        t!("port_forward.deleted_log", name = &name).to_string(),
    );
    let save = app.storage_task(
        move |storage| storage.delete_port_forward(id),
        |result| match result {
            Ok(()) => Message::Noop,
            Err(error) => Message::ShowErrorToast(error),
        },
    );
    Task::batch([save, restore_scrolls(app)])
}

fn activate(app: &mut App, id: PortForwardId) {
    if app.port_forwards.form.visible {
        open_editor(app, id);
    } else {
        app.port_forwards.cursor.select(id);
    }
}

fn open_editor(app: &mut App, id: PortForwardId) {
    app.port_forwards.cursor.select(id);
    let was_visible = app.port_forwards.form.visible && !app.port_forwards.form_closing;
    let Some(forward) = app
        .port_forwards
        .list
        .iter()
        .find(|forward| forward.id == id)
        .cloned()
    else {
        return;
    };
    let port_forwards = &mut app.port_forwards;
    let form = &mut port_forwards.form;
    form.visible = true;
    form.editing_id = Some(id);
    port_forwards.form_anim = if was_visible { 1.0 } else { 0.0 };
    port_forwards.form_closing = false;
    form.original = Some(forward.clone());
    form.name = forward.name;
    form.kind = forward.kind;
    form.host_id = Some(forward.host_id);
    form.bind_address = forward.bind_address;
    form.listen_port = forward.listen_port.to_string();
    form.dest_host = forward.dest_host;
    form.dest_port = forward.dest_port.to_string();
    form.host_picker.reset();
}

fn submit_with_warning(app: &mut App) -> Task<Message> {
    let should_warn = app.port_forwards.form.save_enabled()
        && is_non_loopback_bind(app.port_forwards.form.bind_address.trim());
    let task = submit_form(app);
    if should_warn {
        let bind = app.port_forwards.form.bind_address.trim().to_string();
        return Task::batch([
            task,
            Task::done(Message::ShowWarningToast(
                t!("port_forward.non_loopback_warning", bind = &bind).to_string(),
            )),
        ]);
    }
    task
}

fn submit_form(app: &mut App) -> Task<Message> {
    let (host_id, listen_port, name, editing_id, kind, bind_address, dest_host, dest_port) = {
        let form = &app.port_forwards.form;
        if !form.save_enabled() {
            return Task::none();
        }
        // The save gate validates these fields; the fallbacks keep a future
        // loosening from panicking.
        let Some(host_id) = form.host_id else {
            return Task::none();
        };
        let Ok(listen_port) = form.listen_port.trim().parse::<u16>() else {
            return Task::none();
        };
        let needs_destination = form.kind != PortForwardKind::Dynamic;
        let (dest_host, dest_port) = if needs_destination {
            let Ok(dest_port) = form.dest_port.trim().parse::<u16>() else {
                return Task::none();
            };
            (form.dest_host.trim().to_string(), dest_port)
        } else {
            (String::new(), 0)
        };
        let name = if form.name.trim().is_empty() {
            default_forward_name(
                form.kind,
                &form.bind_address,
                listen_port,
                &dest_host,
                dest_port,
            )
        } else {
            form.name.trim().to_string()
        };
        (
            host_id,
            listen_port,
            name,
            form.editing_id,
            form.kind,
            form.bind_address.trim().to_string(),
            dest_host,
            dest_port,
        )
    };

    if let Some(id) = editing_id {
        if app.port_forwards.trust.pending.contains_key(&id)
            || app.port_forwards.password.has_pending(id)
        {
            super::lifecycle::stop_by_id(app, id);
        }
        let updated = app
            .port_forwards
            .list
            .iter_mut()
            .find(|forward| forward.id == id)
            .map(|forward| {
                forward.name.clone_from(&name);
                forward.kind = kind;
                forward.host_id = host_id;
                forward.bind_address = bind_address;
                forward.listen_port = listen_port;
                forward.dest_host = dest_host;
                forward.dest_port = dest_port;
                forward.clone()
            });
        if let Some(forward) = updated {
            app.add_log(
                LogCategory::PortForward,
                format!("Updated port forward \"{name}\""),
            );
            app.port_forwards.form.visible = false;
            app.port_forwards.form.reset();
            app.needs_scroll_restore = true;
            return app.storage_write_task(move |storage| storage.upsert_port_forward(&forward));
        }
    } else {
        let name_for_log = name.clone();
        app.add_log(
            LogCategory::PortForward,
            format!("Created port forward \"{name_for_log}\""),
        );
        app.port_forwards.form.visible = false;
        app.port_forwards.form.reset();
        app.needs_scroll_restore = true;
        return app.storage_task(
            move |storage| {
                let forward_id = storage.next_port_forward_id()?;
                let forward = PortForward {
                    id: forward_id,
                    name,
                    kind,
                    host_id,
                    bind_address,
                    listen_port,
                    dest_host,
                    dest_port,
                };
                storage.upsert_port_forward(&forward)?;
                Ok(forward)
            },
            |result| match result {
                Ok(forward) => Message::PortForward(
                    crate::port_forward::message::Message::PortForwardCreated { forward },
                ),
                Err(error) => Message::ShowErrorToast(error),
            },
        );
    }

    app.port_forwards.form.visible = false;
    app.port_forwards.form.reset();
    app.needs_scroll_restore = true;
    Task::none()
}

fn duplicate(app: &mut App, id: PortForwardId) -> Task<Message> {
    app.port_forwards.context_menu = None;
    let Some(source) = app
        .port_forwards
        .list
        .iter()
        .find(|forward| forward.id == id)
        .cloned()
    else {
        return Task::none();
    };
    app.storage_task(
        move |storage| {
            let new_id = storage.next_port_forward_id()?;
            let mut duplicate = source.clone();
            duplicate.id = new_id;
            duplicate.name = next_copy_name(&source.name);
            storage.upsert_port_forward(&duplicate)?;
            Ok(duplicate)
        },
        |result| match result {
            Ok(forward) => {
                Message::PortForward(crate::port_forward::message::Message::PortForwardCreated {
                    forward,
                })
            }
            Err(error) => Message::ShowErrorToast(error),
        },
    )
}

pub(super) fn default_forward_name(
    kind: PortForwardKind,
    bind: &str,
    listen_port: u16,
    dest_host: &str,
    dest_port: u16,
) -> String {
    let bind = if bind.trim().is_empty() { "*" } else { bind };
    match kind {
        PortForwardKind::Local | PortForwardKind::Remote => {
            format!("{bind}:{listen_port} → {dest_host}:{dest_port}")
        }
        PortForwardKind::Dynamic => format!("{bind}:{listen_port}"),
    }
}

pub(super) fn is_non_loopback_bind(address: &str) -> bool {
    let address = address.trim();
    if address.is_empty() {
        return false;
    }
    !matches!(
        address,
        "127.0.0.1" | "localhost" | "::1" | "[::1]" | "0:0:0:0:0:0:0:1"
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
