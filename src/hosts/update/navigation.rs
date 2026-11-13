//! Routes navigation messages shared by the Vaults workspace.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::{ActiveTab, App, Workspace};
use crate::core::data::{LogCategory, NavItem, read_local_dir};
use crate::core::message::Message;
use crate::hosts::message::Message as DomainMessage;
use crate::update::{refresh_workspace_manager, restore_scrolls, scroll_to_active_tab};

pub(super) fn update(app: &mut App, msg: &DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::Nav(item) => {
            close_popups(app);
            app.active_nav = *item;
            app.active_tab = ActiveTab::Workspace(Workspace::Vaults);
            if *item == NavItem::Settings {
                refresh_workspace_manager(app);
            }
            restore_scrolls(app)
        }
        DomainMessage::SelectWorkspace(workspace) => {
            close_popups(app);
            if *workspace == Workspace::Sftp {
                app.sftp.state.left.pane.entries = read_local_dir(
                    &app.sftp.state.left.pane.path,
                    app.sftp.state.left.pane.show_hidden,
                );
            }
            let previous = app.active_tab;
            app.seed_tab_width_anims();
            app.active_tab = ActiveTab::Workspace(*workspace);
            finish_switch(app, previous, false)
        }
        DomainMessage::SelectTab(tab_id) => {
            close_popups(app);
            let previous = app.active_tab;
            app.seed_tab_width_anims();
            app.active_tab = ActiveTab::Tab(*tab_id);
            finish_switch(app, previous, true)
        }
        DomainMessage::OpenLocalTerminal => {
            app.add_log(LogCategory::Terminal, "Opened local terminal");
            crate::terminal::update::create_local_session(app)
        }
        _ => Task::none(),
    }
}

fn finish_switch(app: &mut App, previous: ActiveTab, scroll_to_tab: bool) -> Task<Message> {
    if previous == app.active_tab {
        return Task::none();
    }

    let mut tasks = Vec::new();
    if let Some(session_id) = app.terminal.editing_tab {
        tasks.push(Task::done(Message::Terminal(
            crate::terminal::message::Message::TermRenameSubmit(session_id),
        )));
    }
    tasks.push(restore_scrolls(app));
    if scroll_to_tab {
        tasks.push(scroll_to_active_tab(app));
    }
    Task::batch(tasks)
}

/// Close transient popups before leaving the current host surface.
pub(super) fn close_popups(app: &mut App) {
    crate::cursor::close_active_dropdown(app);
    app.close_page_panels();
    app.hosts.hovered_id = None;
}
