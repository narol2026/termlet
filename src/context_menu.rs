//! Shared keyboard routing for context-menu actions.

// Rust guideline compliant 2026-10-09

use crate::app::{ActiveTab, App, Workspace};
use crate::core::context_menu::ContextMenuAction as Action;
use crate::core::grid_cursor::GridDirection;
use crate::core::list_cursor::ListCursor;
use crate::core::message::Message;
use crate::hosts::HostsContextMenu;
use crate::keychain::KeychainContextMenu;
use crate::known_hosts::KnownHostContextMenu;
use crate::port_forward::PortForwardContextMenu;
use crate::sftp::{SftpContextMenu, SftpPane, SftpSidebarContextMenu};
use crate::snippets::SnippetContextMenu;
use crate::terminal::TerminalContextMenu;

impl HostsContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) const fn actions() -> &'static [Action] {
        &[Action::HostEdit, Action::HostDuplicate, Action::HostDelete]
    }
}

impl TerminalContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) fn actions(&self) -> Vec<Action> {
        let mut actions = Vec::with_capacity(7);
        if !self.is_split {
            actions.push(Action::TerminalDuplicate);
        }
        actions.push(Action::TerminalNewWindow);
        if !self.is_split {
            actions.push(Action::TerminalRename);
        }
        actions.extend([
            Action::TerminalToggleLock,
            Action::TerminalClose,
            Action::TerminalCloseOthers,
        ]);
        if self.has_tabs_to_right {
            actions.push(Action::TerminalCloseRight);
        }
        actions
    }
}

impl SftpContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) const fn actions() -> &'static [Action] {
        &[
            Action::SftpTransfer,
            Action::SftpNewFolder,
            Action::SftpRename,
            Action::SftpDelete,
        ]
    }
}

impl SftpSidebarContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) fn actions(&self) -> Vec<Action> {
        let mut actions = vec![Action::SftpSidebarDuplicate];
        if self.can_close {
            actions.push(Action::SftpSidebarClose);
        }
        actions
    }
}

impl SnippetContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) fn actions(&self) -> Vec<Action> {
        match self {
            Self::Snippet { .. } => vec![
                Action::SnippetEdit,
                Action::SnippetCopy,
                Action::SnippetDelete,
            ],
            Self::Package { .. } => {
                vec![Action::SnippetPackageEdit, Action::SnippetPackageDelete]
            }
        }
    }

    /// Returns the keyboard cursor for this menu.
    pub(crate) fn cursor(&self) -> &ListCursor<Action> {
        match self {
            Self::Snippet { cursor, .. } | Self::Package { cursor, .. } => cursor,
        }
    }

    /// Returns the mutable keyboard cursor for this menu.
    pub(crate) fn cursor_mut(&mut self) -> &mut ListCursor<Action> {
        match self {
            Self::Snippet { cursor, .. } | Self::Package { cursor, .. } => cursor,
        }
    }
}

impl KeychainContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) const fn actions(&self) -> &'static [Action] {
        match self {
            Self::Key { .. } => &[Action::KeychainEdit, Action::KeychainDelete],
            Self::Identity { .. } => &[Action::IdentityEdit, Action::IdentityDelete],
        }
    }

    /// Returns the keyboard cursor for this menu.
    pub(crate) fn cursor(&self) -> &ListCursor<Action> {
        match self {
            Self::Key { cursor, .. } | Self::Identity { cursor, .. } => cursor,
        }
    }

    /// Returns the mutable keyboard cursor for this menu.
    pub(crate) fn cursor_mut(&mut self) -> &mut ListCursor<Action> {
        match self {
            Self::Key { cursor, .. } | Self::Identity { cursor, .. } => cursor,
        }
    }
}

impl PortForwardContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) const fn actions() -> &'static [Action] {
        &[
            Action::PortForwardToggle,
            Action::PortForwardEdit,
            Action::PortForwardLogs,
            Action::PortForwardDuplicate,
            Action::PortForwardDelete,
        ]
    }
}

impl KnownHostContextMenu {
    /// Returns the actions in their displayed order.
    pub(crate) const fn actions() -> &'static [Action] {
        &[Action::KnownHostDetails, Action::KnownHostDelete]
    }
}

/// Creates a context-menu cursor focused on its first action.
pub(crate) fn initial_cursor(actions: &[Action]) -> ListCursor<Action> {
    let mut cursor = ListCursor::new();
    if let Some(first) = actions.first() {
        cursor.select(*first);
    }
    cursor
}

/// Whether any context menu is currently open.
pub(crate) fn is_open(app: &App) -> bool {
    let sftp_menu_open = app.active_tab == ActiveTab::Workspace(Workspace::Sftp)
        && (app.sftp.sidebar_context_menu.is_some() || app.sftp.state.context_menu.is_some());
    app.hosts.context_menu.is_some()
        || app.port_forwards.context_menu.is_some()
        || app.snippets.context_menu.is_some()
        || app.keychain.context_menu.is_some()
        || app.known_hosts.context_menu.is_some()
        || app.terminal.context_menu.is_some()
        || sftp_menu_open
}

/// Moves the cursor in the topmost open context menu.
pub(crate) fn move_cursor(app: &mut App, direction: GridDirection) {
    if app.active_tab == ActiveTab::Workspace(Workspace::Sftp) {
        if let Some(menu) = app.sftp.sidebar_context_menu.as_mut() {
            let actions = menu.actions();
            move_in(&mut menu.cursor, &actions, direction);
            return;
        }
        if let Some(menu) = app.sftp.state.context_menu.as_mut() {
            let actions = SftpContextMenu::actions();
            move_in(&mut menu.cursor, actions, direction);
            return;
        }
    }
    if let Some(menu) = app.hosts.context_menu.as_mut() {
        let actions = HostsContextMenu::actions();
        move_in(&mut menu.cursor, actions, direction);
    } else if let Some(menu) = app.port_forwards.context_menu.as_mut() {
        let actions = PortForwardContextMenu::actions();
        move_in(&mut menu.cursor, actions, direction);
    } else if let Some(menu) = app.snippets.context_menu.as_mut() {
        let actions = menu.actions();
        move_in(menu.cursor_mut(), &actions, direction);
    } else if let Some(menu) = app.keychain.context_menu.as_mut() {
        let actions = menu.actions();
        move_in(menu.cursor_mut(), actions, direction);
    } else if let Some(menu) = app.known_hosts.context_menu.as_mut() {
        let actions = KnownHostContextMenu::actions();
        move_in(&mut menu.cursor, actions, direction);
    } else if let Some(menu) = app.terminal.context_menu.as_mut() {
        let actions = menu.actions();
        move_in(&mut menu.cursor, &actions, direction);
    }
}

/// Returns the close message for the topmost open context menu.
pub(crate) fn close_message(app: &App) -> Option<Message> {
    if app.active_tab == ActiveTab::Workspace(Workspace::Sftp) {
        if app.sftp.sidebar_context_menu.is_some() {
            return Some(Message::Sftp(
                crate::sftp::message::Message::SftpSidebarContextMenuClose,
            ));
        }
        if app.sftp.state.context_menu.is_some() {
            return Some(Message::Sftp(
                crate::sftp::message::Message::SftpContextMenuClose,
            ));
        }
    }
    if app.hosts.context_menu.is_some() {
        Some(Message::Hosts(
            crate::hosts::message::Message::HostsContextMenuClose,
        ))
    } else if app.port_forwards.context_menu.is_some() {
        Some(Message::PortForward(
            crate::port_forward::message::Message::PortForwardContextMenuClose,
        ))
    } else if app.snippets.context_menu.is_some() {
        Some(Message::Snippets(
            crate::snippets::message::Message::SnippetContextMenuClose,
        ))
    } else if app.keychain.context_menu.is_some() {
        Some(Message::Keychain(
            match app.keychain.context_menu.as_ref() {
                Some(KeychainContextMenu::Identity { .. }) => {
                    crate::keychain::message::Message::IdentityContextMenuClose
                }
                _ => crate::keychain::message::Message::KeychainContextMenuClose,
            },
        ))
    } else if app.known_hosts.context_menu.is_some() {
        Some(Message::KnownHosts(
            crate::known_hosts::message::Message::KnownHostContextMenuClose,
        ))
    } else if app.terminal.context_menu.is_some() {
        Some(Message::Terminal(
            crate::terminal::message::Message::TermContextMenuClose,
        ))
    } else {
        None
    }
}

fn move_in(cursor: &mut ListCursor<Action>, actions: &[Action], direction: GridDirection) {
    match direction {
        GridDirection::Up | GridDirection::Left => cursor.move_previous(actions),
        GridDirection::Down | GridDirection::Right => cursor.move_next(actions),
    }
}

/// Returns the message for the focused action in the topmost context menu.
pub(crate) fn activation(app: &App) -> Option<Message> {
    if app.active_tab == ActiveTab::Workspace(Workspace::Sftp) {
        if let Some(menu) = app.sftp.sidebar_context_menu.as_ref() {
            return sftp_sidebar_activation(menu);
        }
        if let Some(menu) = app.sftp.state.context_menu.as_ref() {
            return sftp_activation(menu);
        }
    }
    if let Some(menu) = app.hosts.context_menu.as_ref() {
        return host_activation(menu);
    }
    if let Some(menu) = app.port_forwards.context_menu.as_ref() {
        return port_forward_activation(app, menu);
    }
    if let Some(menu) = app.snippets.context_menu.as_ref() {
        return snippet_activation(menu);
    }
    if let Some(menu) = app.keychain.context_menu.as_ref() {
        return keychain_activation(menu);
    }
    if let Some(menu) = app.known_hosts.context_menu.as_ref() {
        return known_host_activation(menu);
    }
    if let Some(menu) = app.terminal.context_menu.as_ref() {
        return terminal_activation(menu);
    }
    None
}

fn host_activation(menu: &HostsContextMenu) -> Option<Message> {
    use crate::hosts::message::Message as HostMessage;

    let action = *menu.cursor.selected_or_first(HostsContextMenu::actions())?;
    Some(Message::Hosts(match action {
        Action::HostEdit => HostMessage::EditHost(menu.host_id),
        Action::HostDuplicate => HostMessage::DuplicateHost(menu.host_id),
        Action::HostDelete => HostMessage::HostDeleteRequest(menu.host_id),
        _ => return None,
    }))
}

fn terminal_activation(menu: &TerminalContextMenu) -> Option<Message> {
    use crate::terminal::message::Message as TermMessage;

    let action = *menu.cursor.selected_or_first(&menu.actions())?;
    Some(Message::Terminal(match action {
        Action::TerminalDuplicate => TermMessage::DuplicateSession(menu.session_id),
        Action::TerminalNewWindow => TermMessage::DuplicateInNewWindow(menu.session_id),
        Action::TerminalRename => TermMessage::TermRenameStart(menu.session_id),
        Action::TerminalToggleLock => TermMessage::TermToggleLock(menu.session_id),
        Action::TerminalClose => TermMessage::TermClose(menu.session_id),
        Action::TerminalCloseOthers => TermMessage::TermCloseOthers(menu.session_id),
        Action::TerminalCloseRight => TermMessage::TermCloseRight(menu.session_id),
        _ => return None,
    }))
}

fn sftp_activation(menu: &SftpContextMenu) -> Option<Message> {
    use crate::sftp::message::Message as SftpMessage;

    let action = *menu.cursor.selected_or_first(SftpContextMenu::actions())?;
    Some(Message::Sftp(match action {
        Action::SftpTransfer if menu.pane == SftpPane::Local => SftpMessage::SftpUploadSelected,
        Action::SftpTransfer => SftpMessage::SftpDownloadSelected,
        Action::SftpNewFolder => SftpMessage::SftpNewFolderRequest {
            pane: menu.pane,
            directory: menu.directory.clone(),
        },
        Action::SftpRename => SftpMessage::SftpRenameRequest,
        Action::SftpDelete => SftpMessage::SftpDeleteRequest,
        _ => return None,
    }))
}

fn sftp_sidebar_activation(menu: &SftpSidebarContextMenu) -> Option<Message> {
    use crate::sftp::message::Message as SftpMessage;

    let action = *menu.cursor.selected_or_first(&menu.actions())?;
    Some(Message::Sftp(match action {
        Action::SftpSidebarDuplicate => SftpMessage::SftpDuplicateTab(menu.session_id),
        Action::SftpSidebarClose => SftpMessage::SftpCloseTab(menu.session_id),
        _ => return None,
    }))
}

fn snippet_activation(menu: &SnippetContextMenu) -> Option<Message> {
    use crate::snippets::message::Message as SnippetMessage;

    let action = *menu.cursor().selected_or_first(&menu.actions())?;
    Some(match menu {
        SnippetContextMenu::Snippet { id, .. } => Message::Snippets(match action {
            Action::SnippetEdit => SnippetMessage::SnippetEdit(*id),
            Action::SnippetCopy => SnippetMessage::SnippetCopyScript(*id),
            Action::SnippetDelete => SnippetMessage::SnippetDeleteRequest(*id),
            _ => return None,
        }),
        SnippetContextMenu::Package { id, .. } => Message::Snippets(match action {
            Action::SnippetPackageEdit => SnippetMessage::SnippetPackageEdit(*id),
            Action::SnippetPackageDelete => SnippetMessage::SnippetPackageDeleteRequest(*id),
            _ => return None,
        }),
    })
}

fn keychain_activation(menu: &KeychainContextMenu) -> Option<Message> {
    use crate::keychain::message::Message as KeychainMessage;

    let action = *menu.cursor().selected_or_first(menu.actions())?;
    Some(match menu {
        KeychainContextMenu::Key { id, .. } => Message::Keychain(match action {
            Action::KeychainEdit => KeychainMessage::KeychainEdit(*id),
            Action::KeychainDelete => KeychainMessage::KeychainDeleteRequest(*id),
            _ => return None,
        }),
        KeychainContextMenu::Identity { id, .. } => Message::Keychain(match action {
            Action::IdentityEdit => KeychainMessage::IdentityEdit(*id),
            Action::IdentityDelete => KeychainMessage::IdentityDeleteRequest(*id),
            _ => return None,
        }),
    })
}

fn port_forward_activation(app: &App, menu: &PortForwardContextMenu) -> Option<Message> {
    use crate::port_forward::PortForwardStatus;
    use crate::port_forward::message::Message as ForwardMessage;

    let action = *menu
        .cursor
        .selected_or_first(PortForwardContextMenu::actions())?;
    let running_or_starting = matches!(
        app.port_forwards.status_for(menu.forward_id),
        PortForwardStatus::Running | PortForwardStatus::Starting
    );
    Some(Message::PortForward(match action {
        Action::PortForwardToggle if running_or_starting => {
            ForwardMessage::PortForwardStop(menu.forward_id)
        }
        Action::PortForwardToggle => ForwardMessage::PortForwardStart(menu.forward_id),
        Action::PortForwardEdit => ForwardMessage::PortForwardEdit(menu.forward_id),
        Action::PortForwardLogs => ForwardMessage::PortForwardShowLogs(menu.forward_id),
        Action::PortForwardDuplicate => ForwardMessage::PortForwardDuplicate(menu.forward_id),
        Action::PortForwardDelete => ForwardMessage::PortForwardDeleteRequest(menu.forward_id),
        _ => return None,
    }))
}

fn known_host_activation(menu: &KnownHostContextMenu) -> Option<Message> {
    use crate::known_hosts::message::Message as KnownHostMessage;

    let action = *menu
        .cursor
        .selected_or_first(KnownHostContextMenu::actions())?;
    Some(Message::KnownHosts(match action {
        Action::KnownHostDetails => KnownHostMessage::KnownHostShowDetails(menu.id),
        Action::KnownHostDelete => KnownHostMessage::KnownHostDeleteRequest(menu.id),
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::{activation, initial_cursor};
    use crate::app::{ActiveTab, SessionId, Workspace};
    use crate::core::context_menu::ContextMenuAction;
    use crate::core::data::FileEntryType;
    use crate::core::list_cursor::ListCursor;
    use crate::core::message::Message;
    use crate::core::test_support::app;
    use crate::port_forward::{PortForwardContextMenu, PortForwardStatus};
    use crate::sftp::{SftpContextMenu, SftpPane};
    use crate::terminal::TerminalContextMenu;
    use iced::Point;

    #[test]
    fn an_initial_cursor_focuses_the_first_action() {
        let cursor = initial_cursor(&[ContextMenuAction::HostEdit, ContextMenuAction::HostDelete]);

        assert_eq!(cursor.selected_key(), Some(&ContextMenuAction::HostEdit));
    }

    #[test]
    fn an_empty_menu_has_no_focused_action() {
        let cursor = initial_cursor(&[]);

        assert_eq!(cursor.selected_key(), None);
    }

    #[test]
    fn terminal_actions_include_only_applicable_entries() {
        let mut menu = TerminalContextMenu {
            session_id: SessionId(1),
            position: Point::ORIGIN,
            has_tabs_to_right: true,
            is_split: true,
            locked: false,
            cursor: ListCursor::new(),
        };
        menu.cursor = initial_cursor(&menu.actions());

        assert_eq!(
            menu.actions(),
            [
                ContextMenuAction::TerminalNewWindow,
                ContextMenuAction::TerminalToggleLock,
                ContextMenuAction::TerminalClose,
                ContextMenuAction::TerminalCloseOthers,
                ContextMenuAction::TerminalCloseRight,
            ]
        );
    }

    #[test]
    fn activation_uses_the_focused_context_action() {
        let mut app = app();
        let mut menu = crate::hosts::HostsContextMenu {
            host_id: crate::core::data::HostId(7),
            position: Point::ORIGIN,
            cursor: ListCursor::new(),
        };
        menu.cursor = initial_cursor(crate::hosts::HostsContextMenu::actions());
        menu.cursor.move_next(&[
            ContextMenuAction::HostEdit,
            ContextMenuAction::HostDuplicate,
        ]);
        app.hosts.context_menu = Some(menu);

        assert!(matches!(
            activation(&app),
            Some(Message::Hosts(
                crate::hosts::message::Message::DuplicateHost(crate::core::data::HostId(7))
            ))
        ));
    }

    #[test]
    fn sftp_transfer_action_depends_on_the_pane() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        let mut menu = SftpContextMenu {
            pane: SftpPane::Local,
            directory: "/tmp".into(),
            name: "notes.txt".into(),
            file_type: FileEntryType::File,
            position: Point::ORIGIN,
            cursor: ListCursor::new(),
        };
        menu.cursor = initial_cursor(crate::sftp::SftpContextMenu::actions());
        app.sftp.state.context_menu = Some(menu);

        assert!(matches!(
            activation(&app),
            Some(Message::Sftp(
                crate::sftp::message::Message::SftpUploadSelected
            ))
        ));
    }

    #[test]
    fn port_forward_toggle_turns_off_a_running_forward() {
        let mut app = app();
        let id = crate::core::data::PortForwardId(3);
        app.port_forwards.runtime.insert(
            id,
            crate::port_forward::state::PortForwardRuntime {
                status: PortForwardStatus::Running,
                logs: Vec::new(),
            },
        );
        let mut menu = PortForwardContextMenu {
            forward_id: id,
            position: Point::ORIGIN,
            cursor: ListCursor::new(),
        };
        menu.cursor = initial_cursor(crate::port_forward::PortForwardContextMenu::actions());
        app.port_forwards.context_menu = Some(menu);

        assert!(matches!(
            activation(&app),
            Some(Message::PortForward(
                crate::port_forward::message::Message::PortForwardStop(id)
            )) if id == crate::core::data::PortForwardId(3)
        ));
    }
}
