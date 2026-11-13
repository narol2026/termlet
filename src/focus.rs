//! Central keyboard-focus ownership for every app surface.
//!
//! Each update resolves one [`FocusFrame`] from the current page, session, and
//! visible overlays. The frame is scoped, so moving to another page or session
//! discards transient owners before rebuilding the focus path. Key presses are
//! routed through the top owner as an explicit [`KeyRoute`], keeping local
//! handling and global-shortcut eligibility in one decision.

// Rust guideline compliant 2026-10-09

use iced::widget::{Id, operation};
use iced::{Task, keyboard};

use crate::app::{ActiveTab, App, DraftId, SessionId, Workspace};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{LogCategory, NavItem};
use crate::core::dropdown::DropdownTarget;
use crate::core::message::Message;
use crate::sftp::SftpPane;
use crate::ssh_connect::SshConnectionState;

pub(crate) use crate::overlay::Overlay as DialogId;

/// Identifies the logical surface that owns keyboard focus.
///
/// Scoped owners are discarded when this value changes. Window-wide dialogs
/// may remain in the overlay stack across scope changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusScope {
    Window,
    Vault(NavItem),
    Sftp,
    Draft(DraftId),
    Terminal(SessionId),
}

/// The result of routing a key through the current focus owner.
///
/// `Handle(Task::none())` deliberately consumes a key without acting on it.
#[derive(Debug)]
pub(crate) enum KeyRoute {
    Handle(Task<Message>),
    PassThrough,
}

/// Identifies a keyboard owner in the focus stack.
///
/// Only surfaces that participate in keyboard routing belong here. Visual or
/// pointer-only layers, such as bottom-right notifications, must stay out so
/// showing one cannot change the focused terminal or block its keystrokes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusId {
    /// A keyboard-driven collection cursor.
    Cursor(CursorTarget),
    /// A terminal session.
    Terminal(SessionId),
    /// A disconnected terminal's reconnect dialog.
    Disconnected(SessionId),
    /// The host search on a new-tab draft.
    NewTab(DraftId),
    /// The SSH password prompt.
    SshPassword,
    /// An SFTP connection password prompt.
    SftpPassword { pane: SftpPane },
    /// A context menu.
    ContextMenu,
    /// An open dropdown and its option list.
    Dropdown(DropdownTarget),
    /// A slide-out editor panel or read-only detail panel.
    Editor(EditorId),
    /// A modal confirmation, information, or text-entry dialog.
    Dialog(DialogId),
    /// An inline search surface.
    Search(SearchId),
}

/// Orders keyboard owners from the background toward the foreground.
///
/// This is deliberately separate from the visual stack in `views::view`.
/// Bottom-right notifications remain pointer-interactive without owning the
/// keyboard. Spotlight is the final keyboard layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum KeyboardLayer {
    Background,
    Editor,
    Search,
    Disconnected,
    ContextMenu,
    Dropdown,
    Dialog,
    Spotlight,
}

impl FocusId {
    const fn layer(self) -> KeyboardLayer {
        match self {
            Self::Cursor(_)
            | Self::Terminal(_)
            | Self::NewTab(_)
            | Self::SshPassword
            | Self::SftpPassword { .. } => KeyboardLayer::Background,
            Self::Disconnected(_) => KeyboardLayer::Disconnected,
            Self::Editor(_) => KeyboardLayer::Editor,
            Self::Search(SearchId::Spotlight(_)) => KeyboardLayer::Spotlight,
            Self::Search(_) => KeyboardLayer::Search,
            Self::ContextMenu => KeyboardLayer::ContextMenu,
            Self::Dropdown(_) => KeyboardLayer::Dropdown,
            Self::Dialog(_) => KeyboardLayer::Dialog,
        }
    }

    fn handle_key(
        self,
        app: &mut App,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
    ) -> KeyRoute {
        let focus_key = FocusKey::from_key(key);
        match self {
            Self::ContextMenu => KeyRoute::Handle(handle_context_menu(app, focus_key, modifiers)),
            Self::Dropdown(target) => {
                KeyRoute::Handle(handle_dropdown(app, target, focus_key, modifiers))
            }
            Self::Cursor(target) => handle_cursor(app, target, focus_key, modifiers)
                .map_or(KeyRoute::PassThrough, KeyRoute::Handle),
            Self::NewTab(_) => {
                handle_new_tab(focus_key, modifiers).map_or(KeyRoute::PassThrough, KeyRoute::Handle)
            }
            Self::Disconnected(session_id) => KeyRoute::Handle(
                handle_disconnected(session_id, focus_key, modifiers).unwrap_or_else(Task::none),
            ),
            Self::Search(search) => KeyRoute::Handle(
                handle_search(app, search, key, focus_key, modifiers).unwrap_or_else(Task::none),
            ),
            Self::Editor(editor) => KeyRoute::Handle(
                handle_editor(app, editor, focus_key, modifiers).unwrap_or_else(Task::none),
            ),
            Self::Dialog(dialog) => KeyRoute::Handle(
                handle_dialog(app, dialog, focus_key, modifiers).unwrap_or_else(Task::none),
            ),
            Self::SshPassword if focus_key == FocusKey::Escape && modifiers.is_empty() => {
                KeyRoute::Handle(Task::done(Message::SshConnect(
                    crate::ssh_connect::message::Message::SshClose,
                )))
            }
            Self::SshPassword | Self::Terminal(_) | Self::SftpPassword { .. } => {
                KeyRoute::PassThrough
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusKey {
    Escape,
    Enter,
    Up,
    Down,
    Left,
    Right,
    Tab,
    Other,
}

impl FocusKey {
    fn from_key(key: &keyboard::Key) -> Self {
        match key {
            keyboard::Key::Named(keyboard::key::Named::Escape) => Self::Escape,
            keyboard::Key::Named(keyboard::key::Named::Enter) => Self::Enter,
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Self::Up,
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Self::Down,
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Self::Left,
            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Self::Right,
            keyboard::Key::Named(keyboard::key::Named::Tab) => Self::Tab,
            _ => Self::Other,
        }
    }
}

/// Routes a key press through the current focus owner.
pub(crate) fn handle_key(
    app: &mut App,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
) -> KeyRoute {
    app.focus.top().map_or(KeyRoute::PassThrough, |owner| {
        owner.handle_key(app, key, modifiers)
    })
}

fn handle_context_menu(
    app: &mut App,
    key: FocusKey,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    if modifiers.is_empty() {
        match key {
            FocusKey::Escape => {
                return crate::context_menu::close_message(app).map_or_else(Task::none, Task::done);
            }
            FocusKey::Up | FocusKey::Down => {
                crate::context_menu::move_cursor(
                    app,
                    if key == FocusKey::Up {
                        crate::core::grid_cursor::GridDirection::Up
                    } else {
                        crate::core::grid_cursor::GridDirection::Down
                    },
                );
            }
            FocusKey::Enter => {
                return crate::context_menu::activation(app).map_or_else(Task::none, Task::done);
            }
            FocusKey::Left | FocusKey::Right | FocusKey::Tab | FocusKey::Other => {}
        }
    }
    Task::none()
}

fn handle_dropdown(
    app: &mut App,
    target: DropdownTarget,
    key: FocusKey,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    if modifiers.is_empty() {
        match key {
            FocusKey::Escape => return crate::cursor::dismiss_active_dropdown(app),
            FocusKey::Up | FocusKey::Down => {
                return crate::cursor::move_dropdown(
                    app,
                    target,
                    if key == FocusKey::Up {
                        crate::core::grid_cursor::GridDirection::Up
                    } else {
                        crate::core::grid_cursor::GridDirection::Down
                    },
                );
            }
            FocusKey::Enter => {
                return crate::cursor::dropdown_activation(app, target)
                    .map_or_else(Task::none, Task::done);
            }
            FocusKey::Left | FocusKey::Right | FocusKey::Tab | FocusKey::Other => {}
        }
    }
    Task::none()
}

fn handle_cursor(
    app: &App,
    target: CursorTarget,
    key: FocusKey,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    if !modifiers.is_empty() && modifiers != keyboard::Modifiers::SHIFT {
        return None;
    }
    let direction = match key {
        FocusKey::Up => Some(crate::core::grid_cursor::GridDirection::Up),
        FocusKey::Down => Some(crate::core::grid_cursor::GridDirection::Down),
        FocusKey::Left => Some(crate::core::grid_cursor::GridDirection::Left),
        FocusKey::Right => Some(crate::core::grid_cursor::GridDirection::Right),
        FocusKey::Escape | FocusKey::Enter | FocusKey::Tab | FocusKey::Other => None,
    };
    if let Some(direction) = direction {
        return Some(Task::done(Message::CursorMove {
            target,
            direction,
            extend: modifiers.shift(),
        }));
    }
    if key == FocusKey::Enter && modifiers.is_empty() {
        return crate::cursor::activation(app, target).map(Task::done);
    }
    None
}

fn handle_new_tab(key: FocusKey, modifiers: keyboard::Modifiers) -> Option<Task<Message>> {
    if !modifiers.is_empty() {
        return None;
    }
    match key {
        FocusKey::Up => Some(Task::done(Message::Hosts(
            crate::hosts::message::Message::NewTabSelectUp,
        ))),
        FocusKey::Down => Some(Task::done(Message::Hosts(
            crate::hosts::message::Message::NewTabSelectDown,
        ))),
        FocusKey::Escape
        | FocusKey::Enter
        | FocusKey::Left
        | FocusKey::Right
        | FocusKey::Tab
        | FocusKey::Other => None,
    }
}

fn handle_disconnected(
    session_id: SessionId,
    key: FocusKey,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    if !modifiers.is_empty() {
        return None;
    }
    match key {
        FocusKey::Escape => Some(Task::done(Message::Terminal(
            crate::terminal::message::Message::DisconnectedDismiss(session_id),
        ))),
        FocusKey::Enter => Some(Task::done(Message::Terminal(
            crate::terminal::message::Message::DisconnectedReconnect(session_id),
        ))),
        FocusKey::Up
        | FocusKey::Down
        | FocusKey::Left
        | FocusKey::Right
        | FocusKey::Tab
        | FocusKey::Other => None,
    }
}

fn handle_search(
    app: &mut App,
    search: SearchId,
    key: &keyboard::Key,
    focus_key: FocusKey,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    let no_mods = modifiers.is_empty();
    match search {
        SearchId::TabRename(_) if focus_key == FocusKey::Escape && no_mods => Some(Task::done(
            Message::Terminal(crate::terminal::message::Message::TermRenameCancel),
        )),
        SearchId::Terminal(session_id) if focus_key == FocusKey::Escape && no_mods => {
            Some(Task::done(Message::Terminal(
                crate::terminal::message::Message::SearchClose(session_id),
            )))
        }
        SearchId::Spotlight(session_id) => {
            if no_mods {
                match focus_key {
                    FocusKey::Escape => {
                        return Some(Task::done(Message::SpotlightCancel(session_id)));
                    }
                    FocusKey::Enter => {
                        return Some(Task::done(Message::SpotlightConfirm(session_id)));
                    }
                    FocusKey::Up => {
                        return Some(Task::done(Message::SpotlightMoveUp(session_id)));
                    }
                    FocusKey::Down => {
                        return Some(Task::done(Message::SpotlightMoveDown(session_id)));
                    }
                    FocusKey::Left | FocusKey::Right | FocusKey::Tab | FocusKey::Other => {}
                }
            }
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "keyboard: popup pass-through session={session_id} key={key:?} modifiers={modifiers:?}"
                ),
            );
            None
        }
        SearchId::TabRename(_)
        | SearchId::Terminal(_)
        | SearchId::SftpFilter { .. }
        | SearchId::SftpPath { .. } => None,
    }
}

fn handle_editor(
    app: &mut App,
    editor: EditorId,
    key: FocusKey,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    if key == FocusKey::Tab && editor == EditorId::Host {
        if modifiers == keyboard::Modifiers::SHIFT {
            return Some(operation::focus_previous());
        }
        if modifiers.is_empty() {
            return Some(operation::focus_next());
        }
    }
    if !modifiers.is_empty() {
        return None;
    }
    match (key, editor) {
        (FocusKey::Escape, EditorId::Host) => Some(Task::done(Message::Hosts(
            crate::hosts::message::Message::HideForm,
        ))),
        (FocusKey::Escape, EditorId::KeychainKey) => {
            app.keychain.form_closing = true;
            Some(Task::none())
        }
        (FocusKey::Escape, EditorId::KeychainIdentity) => Some(Task::done(Message::Keychain(
            crate::keychain::message::Message::IdentityHideForm,
        ))),
        (FocusKey::Escape, EditorId::PortForward) => {
            app.port_forwards.form_closing = true;
            Some(Task::none())
        }
        (FocusKey::Escape, EditorId::Snippet | EditorId::SnippetPackage) => {
            app.snippets.form_closing = true;
            Some(Task::none())
        }
        (FocusKey::Escape, EditorId::KnownHostDetails) => {
            app.known_hosts.detail_closing = true;
            Some(Task::none())
        }
        (FocusKey::Escape, EditorId::SftpPreview) => {
            app.sftp.state.preview = None;
            Some(Task::none())
        }
        (FocusKey::Enter, EditorId::Host) => Some(Task::done(Message::Hosts(
            crate::hosts::message::Message::SaveHost,
        ))),
        (FocusKey::Enter, EditorId::KeychainKey) => Some(Task::done(Message::Keychain(
            crate::keychain::message::Message::KeychainFormSubmit,
        ))),
        (FocusKey::Enter, EditorId::KeychainIdentity) => Some(Task::done(Message::Keychain(
            crate::keychain::message::Message::IdentityFormSubmit,
        ))),
        (FocusKey::Enter, EditorId::PortForward) => Some(Task::done(Message::PortForward(
            crate::port_forward::message::Message::PortForwardFormSubmit,
        ))),
        (FocusKey::Enter, EditorId::Snippet) => Some(Task::done(Message::Snippets(
            crate::snippets::message::Message::SnippetFormSubmit,
        ))),
        (FocusKey::Enter, EditorId::SnippetPackage) => Some(Task::done(Message::Snippets(
            crate::snippets::message::Message::SnippetPackageFormSubmit,
        ))),
        (FocusKey::Enter, EditorId::KnownHostDetails | EditorId::SftpPreview)
        | (
            FocusKey::Up
            | FocusKey::Down
            | FocusKey::Left
            | FocusKey::Right
            | FocusKey::Tab
            | FocusKey::Other,
            _,
        ) => None,
    }
}

fn handle_dialog(
    app: &mut App,
    dialog: DialogId,
    key: FocusKey,
    modifiers: keyboard::Modifiers,
) -> Option<Task<Message>> {
    if !modifiers.is_empty() {
        return None;
    }
    match key {
        FocusKey::Escape => Some(cancel_dialog(app, dialog)),
        FocusKey::Enter => Some(confirm_dialog(app, dialog)),
        FocusKey::Up
        | FocusKey::Down
        | FocusKey::Left
        | FocusKey::Right
        | FocusKey::Tab
        | FocusKey::Other => None,
    }
}

fn cancel_dialog(app: &mut App, dialog: DialogId) -> Task<Message> {
    dialog.cancel(app)
}

fn confirm_dialog(app: &mut App, dialog: DialogId) -> Task<Message> {
    dialog.confirm(app)
}

/// Identifies an inline search surface that owns keyboard input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SearchId {
    Terminal(SessionId),
    Spotlight(SessionId),
    TabRename(SessionId),
    SftpFilter { pane: SftpPane },
    SftpPath { pane: SftpPane },
}

/// Identifies the editor or detail panel that owns keyboard input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorId {
    Host,
    KeychainKey,
    KeychainIdentity,
    PortForward,
    Snippet,
    SnippetPackage,
    KnownHostDetails,
    SftpPreview,
}

/// The canonical focus path for the current application state.
#[derive(Debug, PartialEq, Eq)]
struct FocusFrame {
    scope: FocusScope,
    base: Option<FocusId>,
    layers: Vec<FocusId>,
}

impl FocusFrame {
    fn resolve(app: &App) -> Self {
        let scope = desired_scope(app);
        let base = desired_base(app, scope);
        let layers = desired_keyboard_layers(app, scope);

        Self {
            scope,
            base,
            layers,
        }
    }

    fn owners(&self) -> impl Iterator<Item = FocusId> + '_ {
        self.base.into_iter().chain(self.layers.iter().copied())
    }
}

/// Nested keyboard-focus owners, with the last item receiving input.
#[derive(Debug)]
pub(crate) struct FocusStack {
    scope: FocusScope,
    owners: Vec<FocusId>,
}

impl Default for FocusStack {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusStack {
    /// Creates an empty focus stack.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            scope: FocusScope::Window,
            owners: Vec::new(),
        }
    }

    /// Returns the owner receiving keyboard input.
    #[must_use]
    pub(crate) fn top(&self) -> Option<FocusId> {
        self.owners.last().copied()
    }

    /// Returns the page-level owner below all overlays.
    #[must_use]
    pub(crate) fn base(&self) -> Option<FocusId> {
        self.owners.first().copied()
    }

    /// Returns the logical scope represented by the current focus path.
    #[must_use]
    pub(crate) const fn scope(&self) -> FocusScope {
        self.scope
    }

    /// Replaces the stack with a validated focus frame.
    ///
    /// A scope change discards every owner before installing the new frame, so
    /// a page or session cannot inherit transient focus from the previous one.
    fn reconcile(&mut self, frame: &FocusFrame) -> bool {
        let previous = self.top();
        let scope_changed = self.scope != frame.scope;
        if scope_changed {
            self.scope = frame.scope;
            self.owners.clear();
        }

        let desired: Vec<_> = frame.owners().collect();
        let shared = self
            .owners
            .iter()
            .zip(&desired)
            .take_while(|(current, next)| current == next)
            .count();

        while self.owners.len() > shared {
            self.pop();
        }
        for (index, owner) in desired.into_iter().enumerate().skip(shared) {
            if index < self.owners.len() {
                self.replace_top(owner);
            } else {
                self.push(owner);
            }
        }
        self.top() != previous
    }

    fn push(&mut self, owner: FocusId) {
        if self.top() != Some(owner) {
            self.owners.push(owner);
        }
    }

    fn replace_top(&mut self, owner: FocusId) {
        if let Some(top) = self.owners.last_mut() {
            *top = owner;
        } else {
            self.owners.push(owner);
        }
    }

    fn pop(&mut self) -> Option<FocusId> {
        self.owners.pop()
    }
}

/// Reconciles focus after a state update and requests any resulting widget focus.
pub(crate) fn sync(app: &mut App) -> Task<Message> {
    let frame = FocusFrame::resolve(app);
    let scope = frame.scope;
    let changed = app.focus.reconcile(&frame);
    debug_assert_eq!(app.focus.scope(), scope);
    if changed {
        focus_top(app)
    } else {
        Task::none()
    }
}

/// Replaces the base owner while preserving the keyboard layer path.
pub(crate) fn focus_base(app: &mut App, base: FocusId) -> Task<Message> {
    let mut frame = FocusFrame::resolve(app);
    if !owner_belongs_to_scope(base, frame.scope) {
        return Task::none();
    }
    frame.base = Some(base);
    let scope = frame.scope;
    let changed = app.focus.reconcile(&frame);
    debug_assert_eq!(app.focus.scope(), scope);
    if changed {
        focus_top(app)
    } else {
        Task::none()
    }
}

/// Adopts focus after a pointer interaction with a base-layer widget.
pub(crate) fn adopt_widget(app: &mut App, id: &Id) -> Task<Message> {
    if let Some(session_id) = app
        .terminal
        .sessions
        .iter()
        .find(|session| session.terminal.widget_id() == id)
        .map(|session| session.id)
    {
        return focus_base(app, FocusId::Terminal(session_id));
    }

    if let Some(target) = cursor_target_for_id(id) {
        return focus_base(app, FocusId::Cursor(target));
    }

    Task::none()
}

fn desired_scope(app: &App) -> FocusScope {
    if let Some(session_id) = app.active_terminal_session_id() {
        return FocusScope::Terminal(session_id);
    }
    if let Some(draft_id) = app.active_draft_id() {
        return FocusScope::Draft(draft_id);
    }
    match app.active_tab {
        ActiveTab::Workspace(Workspace::Sftp) => FocusScope::Sftp,
        ActiveTab::Workspace(Workspace::Vaults) => FocusScope::Vault(app.active_nav),
        ActiveTab::Tab(_) => FocusScope::Window,
    }
}

fn desired_base(app: &App, scope: FocusScope) -> Option<FocusId> {
    let current = app.focus.base();

    if let FocusScope::Terminal(session_id) = scope {
        if let Some(owner @ FocusId::Cursor(target)) = current
            && is_right_sidebar(target)
            && app.sidebar().is_some_and(|sidebar| sidebar.show)
        {
            return Some(owner);
        }
        return Some(FocusId::Terminal(session_id));
    }

    if scope == FocusScope::Sftp {
        let sftp = &app.sftp.state;
        if sftp.left.host_picker.open {
            return Some(FocusId::Cursor(CursorTarget::SftpLocalHostPicker));
        }
        if sftp.right.host_picker.open {
            return Some(FocusId::Cursor(CursorTarget::SftpRemoteHostPicker));
        }
        if sftp.left.pane.attempt.state == SshConnectionState::WaitingForPassword {
            return Some(FocusId::SftpPassword {
                pane: SftpPane::Local,
            });
        }
        if sftp.right.pane.attempt.state == SshConnectionState::WaitingForPassword {
            return Some(FocusId::SftpPassword {
                pane: SftpPane::Remote,
            });
        }
        return crate::cursor::sftp_file_target(app).map(FocusId::Cursor);
    }

    if matches!(scope, FocusScope::Vault(_)) {
        return crate::cursor::vault_target(app).map(FocusId::Cursor);
    }

    if let FocusScope::Draft(draft_id) = scope {
        return match app
            .terminal
            .drafts
            .get(&draft_id)
            .map(|draft| draft.ssh.attempt.state)
        {
            Some(SshConnectionState::Idle) => Some(FocusId::NewTab(draft_id)),
            Some(SshConnectionState::WaitingForPassword) => Some(FocusId::SshPassword),
            _ => None,
        };
    }

    None
}

fn desired_keyboard_layers(app: &App, scope: FocusScope) -> Vec<FocusId> {
    let mut layers = Vec::new();

    if let Some(owner) = active_editor(app).map(FocusId::Editor)
        && owner_belongs_to_scope(owner, scope)
    {
        layers.push(owner);
    }
    if let Some(owner) = app
        .active_disconnected_session_id()
        .map(FocusId::Disconnected)
        && owner_belongs_to_scope(owner, scope)
    {
        layers.push(owner);
    }
    if let Some(owner) = active_search(app).map(FocusId::Search)
        && owner_belongs_to_scope(owner, scope)
    {
        layers.push(owner);
    }
    if crate::context_menu::is_open(app) {
        layers.push(FocusId::ContextMenu);
    }
    if let Some(target) = app.active_dropdown {
        layers.push(FocusId::Dropdown(target));
    }
    if let Some(dialog) = active_dialog(app) {
        layers.push(FocusId::Dialog(dialog));
    }

    layers.sort_by_key(|owner| owner.layer());

    layers
}

fn owner_belongs_to_scope(owner: FocusId, scope: FocusScope) -> bool {
    match owner {
        FocusId::Dialog(_) => true,
        FocusId::Cursor(target) if is_right_sidebar(target) => {
            matches!(scope, FocusScope::Terminal(_))
        }
        FocusId::Cursor(
            CursorTarget::SftpLocalFiles
            | CursorTarget::SftpRemoteFiles
            | CursorTarget::SftpLocalHostPicker
            | CursorTarget::SftpRemoteHostPicker,
        ) => scope == FocusScope::Sftp,
        FocusId::Cursor(_) => matches!(scope, FocusScope::Vault(_)),
        FocusId::Terminal(session_id) | FocusId::Disconnected(session_id) => {
            scope == FocusScope::Terminal(session_id)
        }
        FocusId::NewTab(draft_id) => scope == FocusScope::Draft(draft_id),
        FocusId::SshPassword => matches!(scope, FocusScope::Draft(_)),
        FocusId::SftpPassword { .. } => scope == FocusScope::Sftp,
        FocusId::Editor(editor) => editor_scope(editor) == scope,
        FocusId::Search(search) => search_scope(search) == scope,
        FocusId::ContextMenu | FocusId::Dropdown(_) => true,
    }
}

fn editor_scope(editor: EditorId) -> FocusScope {
    match editor {
        EditorId::Host => FocusScope::Vault(NavItem::Hosts),
        EditorId::KeychainKey | EditorId::KeychainIdentity => FocusScope::Vault(NavItem::Keychain),
        EditorId::PortForward => FocusScope::Vault(NavItem::PortForwarding),
        EditorId::Snippet | EditorId::SnippetPackage => FocusScope::Vault(NavItem::Snippets),
        EditorId::KnownHostDetails => FocusScope::Vault(NavItem::KnownHosts),
        EditorId::SftpPreview => FocusScope::Sftp,
    }
}

fn search_scope(search: SearchId) -> FocusScope {
    match search {
        SearchId::Terminal(session_id)
        | SearchId::Spotlight(session_id)
        | SearchId::TabRename(session_id) => FocusScope::Terminal(session_id),
        SearchId::SftpFilter { .. } | SearchId::SftpPath { .. } => FocusScope::Sftp,
    }
}

fn active_editor(app: &App) -> Option<EditorId> {
    if app.active_tab == ActiveTab::Workspace(Workspace::Sftp) {
        return app
            .sftp
            .state
            .preview
            .as_ref()
            .map(|_| EditorId::SftpPreview);
    }
    if app.active_tab != ActiveTab::Workspace(Workspace::Vaults) {
        return None;
    }

    if app.active_nav == crate::core::data::NavItem::Hosts && app.hosts.form.visible {
        Some(EditorId::Host)
    } else if app.active_nav == crate::core::data::NavItem::Keychain && app.keychain.form.visible {
        Some(EditorId::KeychainKey)
    } else if app.active_nav == crate::core::data::NavItem::Keychain
        && app.keychain.identity_form.visible
    {
        Some(EditorId::KeychainIdentity)
    } else if app.active_nav == crate::core::data::NavItem::PortForwarding
        && app.port_forwards.form.visible
    {
        Some(EditorId::PortForward)
    } else if app.active_nav == crate::core::data::NavItem::Snippets && app.snippets.form.visible {
        Some(EditorId::Snippet)
    } else if app.active_nav == crate::core::data::NavItem::Snippets
        && app.snippets.package_form.visible
    {
        Some(EditorId::SnippetPackage)
    } else if app.active_nav == crate::core::data::NavItem::KnownHosts
        && app.known_hosts.detail_open
    {
        Some(EditorId::KnownHostDetails)
    } else {
        None
    }
}

fn active_dialog(app: &App) -> Option<DialogId> {
    crate::overlay::active(app)
}

fn active_search(app: &App) -> Option<SearchId> {
    if let Some(session_id) = app.terminal.editing_tab {
        return Some(SearchId::TabRename(session_id));
    }

    if let Some(session_id) = app.active_terminal_session_id() {
        let session = app
            .terminal
            .sessions
            .iter()
            .find(|session| session.id == session_id)?;
        if session.spotlight.is_some() {
            return Some(SearchId::Spotlight(session_id));
        }
        if session.show_search {
            return Some(SearchId::Terminal(session_id));
        }
    }

    if app.active_tab == ActiveTab::Workspace(Workspace::Sftp) {
        let sftp = &app.sftp.state;
        let pane = sftp.active_pane;
        if sftp.pane(pane).path_edit.is_some() {
            return Some(SearchId::SftpPath { pane });
        }
        if sftp.pane(pane).filter_open {
            return Some(SearchId::SftpFilter { pane });
        }
    }

    None
}

fn focus_top(app: &mut App) -> Task<Message> {
    match app.focus.top() {
        Some(FocusId::Cursor(
            CursorTarget::SftpLocalHostPicker | CursorTarget::SftpRemoteHostPicker,
        )) => operation::focus(Id::new(crate::sftp::view::SFTP_HOST_PICKER_INPUT_ID)),
        Some(FocusId::Cursor(target)) => operation::focus(Id::from(target.id())),
        Some(FocusId::Terminal(session_id)) => {
            crate::terminal::update::request_terminal_focus(app, session_id)
        }
        Some(FocusId::NewTab(draft_id)) => {
            operation::focus(Id::from(format!("new_tab_input_{draft_id}")))
        }
        Some(FocusId::SshPassword) => {
            operation::focus(Id::new(crate::ssh_connect::view::SSH_PASSWORD_INPUT_ID))
        }
        Some(FocusId::SftpPassword {
            pane: SftpPane::Local,
        }) => operation::focus(Id::new(crate::sftp::view::SFTP_LOCAL_PASSWORD_INPUT_ID)),
        Some(FocusId::SftpPassword {
            pane: SftpPane::Remote,
        }) => operation::focus(Id::new(crate::sftp::view::SFTP_REMOTE_PASSWORD_INPUT_ID)),
        Some(FocusId::Dropdown(target)) => operation::focus(Id::new(target.input_id())),
        Some(FocusId::Search(search)) => operation::focus(search.id()),
        Some(FocusId::Dialog(dialog)) => dialog.focus(),
        Some(FocusId::ContextMenu | FocusId::Disconnected(_) | FocusId::Editor(_)) | None => {
            operation::unfocus()
        }
    }
}

impl SearchId {
    fn id(self) -> Id {
        match self {
            Self::Terminal(session_id) => Id::from(format!("search_input_{session_id}")),
            Self::Spotlight(_) => Id::new(crate::update::SPOTLIGHT_INPUT_ID),
            Self::TabRename(session_id) => Id::from(format!("tab_rename_input_{session_id}")),
            Self::SftpFilter {
                pane: SftpPane::Local,
            } => Id::new("sftp_local_filter"),
            Self::SftpFilter {
                pane: SftpPane::Remote,
            } => Id::new("sftp_remote_filter"),
            Self::SftpPath {
                pane: SftpPane::Local,
            } => Id::new("sftp_path_input_local"),
            Self::SftpPath {
                pane: SftpPane::Remote,
            } => Id::new("sftp_path_input_remote"),
        }
    }
}

fn is_right_sidebar(target: CursorTarget) -> bool {
    matches!(
        target,
        CursorTarget::RightSidebarSnippets
            | CursorTarget::RightSidebarCommands
            | CursorTarget::RightSidebarTheme
    )
}

fn cursor_target_for_id(id: &Id) -> Option<CursorTarget> {
    [
        CursorTarget::Hosts,
        CursorTarget::Keychain,
        CursorTarget::PortForwards,
        CursorTarget::Snippets,
        CursorTarget::KnownHosts,
        CursorTarget::Workspaces,
        CursorTarget::RightSidebarSnippets,
        CursorTarget::RightSidebarCommands,
        CursorTarget::RightSidebarTheme,
        CursorTarget::SftpLocalFiles,
        CursorTarget::SftpRemoteFiles,
        CursorTarget::SftpLocalHostPicker,
        CursorTarget::SftpRemoteHostPicker,
    ]
    .into_iter()
    .find(|target| Id::from(target.id()) == *id)
}

#[cfg(test)]
mod tests {
    use super::{
        DialogId, EditorId, FocusFrame, FocusId, FocusScope, FocusStack, KeyRoute, KeyboardLayer,
        SearchId, adopt_widget, handle_key, sync,
    };
    use crate::app::{CommandFrequencyPrompt, SessionId, Workspace};
    use crate::core::cursor_focus::CursorTarget;
    use crate::core::data::{FileEntryType, HostId, NavItem};
    use crate::sftp::{SftpPane, SftpRenamePending};
    use crate::terminal::state::CommandInfo;

    fn passes_global_shortcut(app: &mut crate::app::App) -> bool {
        matches!(
            handle_key(
                app,
                &iced::keyboard::Key::Character("l".into()),
                iced::keyboard::Modifiers::COMMAND,
            ),
            KeyRoute::PassThrough
        )
    }

    fn frame(base: FocusId, layers: Vec<FocusId>) -> FocusFrame {
        FocusFrame {
            scope: FocusScope::Vault(NavItem::Hosts),
            base: Some(base),
            layers,
        }
    }

    #[test]
    fn opening_an_overlay_pushes_it_onto_the_focus_stack() {
        let mut stack = FocusStack::new();
        stack.push(FocusId::Cursor(CursorTarget::Hosts));
        stack.push(FocusId::ContextMenu);

        assert_eq!(stack.base(), Some(FocusId::Cursor(CursorTarget::Hosts)));
        assert_eq!(stack.top(), Some(FocusId::ContextMenu));
    }

    #[test]
    fn replacing_an_overlay_keeps_the_page_owner_beneath_it() {
        let mut stack = FocusStack::new();
        stack.push(FocusId::Cursor(CursorTarget::Hosts));
        stack.push(FocusId::ContextMenu);

        stack.replace_top(FocusId::Editor(EditorId::Host));

        assert_eq!(stack.base(), Some(FocusId::Cursor(CursorTarget::Hosts)));
        assert_eq!(stack.top(), Some(FocusId::Editor(EditorId::Host)));
    }

    #[test]
    fn popping_an_overlay_restores_the_previous_owner() {
        let mut stack = FocusStack::new();
        stack.push(FocusId::Cursor(CursorTarget::Hosts));
        stack.push(FocusId::ContextMenu);

        assert_eq!(stack.pop(), Some(FocusId::ContextMenu));
        assert_eq!(stack.top(), Some(FocusId::Cursor(CursorTarget::Hosts)));
    }

    #[test]
    fn reconciling_the_same_path_does_not_request_focus_again() {
        let mut stack = FocusStack::new();
        let base = FocusId::Cursor(CursorTarget::Hosts);
        let overlays = vec![FocusId::ContextMenu];

        assert!(stack.reconcile(&frame(base, overlays.clone())));
        assert_eq!(stack.scope(), FocusScope::Vault(NavItem::Hosts));
        let owners = stack.owners.clone();

        assert!(!stack.reconcile(&frame(base, overlays)));
        assert_eq!(stack.owners, owners);
    }

    #[test]
    fn changing_scope_discards_transient_owners_before_rebuilding() {
        let mut stack = FocusStack::new();
        let _ = stack.reconcile(&frame(
            FocusId::Cursor(CursorTarget::Hosts),
            vec![FocusId::ContextMenu],
        ));

        assert!(stack.reconcile(&FocusFrame {
            scope: FocusScope::Vault(NavItem::Keychain),
            base: Some(FocusId::Cursor(CursorTarget::Keychain)),
            layers: Vec::new(),
        }));

        assert_eq!(stack.owners, vec![FocusId::Cursor(CursorTarget::Keychain)]);
        assert_eq!(stack.scope(), FocusScope::Vault(NavItem::Keychain));
    }

    #[test]
    fn changing_the_base_below_an_overlay_keeps_the_input_owner() {
        let mut stack = FocusStack::new();
        let dialog = FocusId::Dialog(DialogId::HostDelete);
        let _ = stack.reconcile(&frame(FocusId::Cursor(CursorTarget::Hosts), vec![dialog]));

        assert!(!stack.reconcile(&FocusFrame {
            scope: FocusScope::Vault(NavItem::Keychain),
            base: Some(FocusId::Cursor(CursorTarget::Keychain)),
            layers: vec![dialog],
        }));
        assert_eq!(
            stack.owners,
            vec![FocusId::Cursor(CursorTarget::Keychain), dialog]
        );
    }

    #[test]
    fn replacing_an_overlay_at_the_same_depth_requests_focus() {
        let mut stack = FocusStack::new();
        let base = FocusId::Cursor(CursorTarget::Hosts);
        let _ = stack.reconcile(&frame(base, vec![FocusId::ContextMenu]));

        assert!(stack.reconcile(&frame(base, vec![FocusId::Dialog(DialogId::HostDelete)],)));
        assert_eq!(
            stack.owners,
            vec![base, FocusId::Dialog(DialogId::HostDelete)]
        );
    }

    #[test]
    fn shortening_the_overlay_path_prunes_hidden_owners() {
        let mut stack = FocusStack::new();
        let base = FocusId::Cursor(CursorTarget::Hosts);
        let dialog = FocusId::Dialog(DialogId::HostDelete);
        let dropdown = FocusId::Dropdown(crate::core::dropdown::DropdownTarget::HostsTagFilter);
        let _ = stack.reconcile(&frame(base, vec![dialog, dropdown]));

        assert!(stack.reconcile(&frame(base, vec![dialog])));
        assert_eq!(stack.owners, vec![base, dialog]);
    }

    #[test]
    fn removing_every_owner_clears_the_stack() {
        let mut stack = FocusStack::new();
        let _ = stack.reconcile(&frame(
            FocusId::Cursor(CursorTarget::Hosts),
            vec![FocusId::ContextMenu],
        ));

        assert!(stack.reconcile(&FocusFrame {
            scope: FocusScope::Window,
            base: None,
            layers: Vec::new(),
        }));
        assert!(stack.owners.is_empty());
        assert_eq!(stack.top(), None);
    }

    #[test]
    fn a_pointer_focus_observation_adopts_the_cursor_owner() {
        let mut app = crate::core::test_support::app();
        let id = iced::widget::Id::from(CursorTarget::Hosts.id());

        let _ = adopt_widget(&mut app, &id);

        assert_eq!(app.focus.top(), Some(FocusId::Cursor(CursorTarget::Hosts)));
    }

    #[test]
    fn inline_search_stays_below_modals_but_spotlight_stays_above_them() {
        let inline_search = FocusId::Search(SearchId::SftpFilter {
            pane: SftpPane::Local,
        });
        let dialog = FocusId::Dialog(DialogId::SftpRename);
        let spotlight = FocusId::Search(SearchId::Spotlight(SessionId(1)));

        assert!(inline_search.layer() < dialog.layer());
        assert!(dialog.layer() < spotlight.layer());
        assert_eq!(inline_search.layer(), KeyboardLayer::Search);
        assert_eq!(dialog.layer(), KeyboardLayer::Dialog);
        assert_eq!(spotlight.layer(), KeyboardLayer::Spotlight);
    }

    #[test]
    fn a_modal_dialog_takes_focus_from_an_open_sftp_filter() {
        let mut app = crate::core::test_support::app();
        app.active_tab = crate::app::ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.state.left.pane.filter_open = true;
        app.sftp.state.rename_pending = Some(SftpRenamePending {
            pane: SftpPane::Local,
            directory: String::from("/tmp"),
            old_name: String::from("old.txt"),
            file_type: FileEntryType::File,
            new_name: String::from("old.txt"),
            in_progress: false,
        });

        let _ = sync(&mut app);

        assert_eq!(app.focus.top(), Some(FocusId::Dialog(DialogId::SftpRename)));
    }

    #[test]
    fn a_modal_dialog_takes_focus_from_an_open_dropdown() {
        let mut app = crate::core::test_support::app();
        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::HostsTagFilter);
        app.show_quit_confirm = true;

        let _ = sync(&mut app);

        assert_eq!(app.focus.top(), Some(FocusId::Dialog(DialogId::Quit)));
    }

    #[test]
    fn changing_the_modal_kind_changes_the_focus_owner() {
        let mut app = crate::core::test_support::app();

        app.show_quit_confirm = true;
        let _ = sync(&mut app);
        assert_eq!(app.focus.top(), Some(FocusId::Dialog(DialogId::Quit)));

        app.show_quit_confirm = false;
        app.show_crash_dialog = true;
        let _ = sync(&mut app);
        assert_eq!(app.focus.top(), Some(FocusId::Dialog(DialogId::Crash)));

        app.show_crash_dialog = false;
        app.hosts.delete_pending = Some(HostId(1));
        let _ = sync(&mut app);
        assert_eq!(app.focus.top(), Some(FocusId::Dialog(DialogId::HostDelete)));
    }

    #[test]
    fn a_command_info_dialog_only_owns_focus_for_its_active_session() {
        let mut app = crate::core::test_support::app();
        let tab = app.allocate_tab_id(crate::terminal::TabKind::Session(SessionId(1)));
        app.activate_tab(tab);
        app.terminal.command_info = Some(CommandInfo {
            session_id: SessionId(2),
            index: 0,
        });

        let _ = sync(&mut app);

        assert_eq!(app.focus.top(), Some(FocusId::Terminal(SessionId(1))));
        assert!(
            passes_global_shortcut(&mut app),
            "a dialog belonging to another session must not block the active terminal"
        );
    }

    #[test]
    fn a_modal_dialog_explicitly_consumes_global_shortcuts() {
        let mut app = crate::core::test_support::app();
        app.hosts.delete_pending = Some(HostId(1));
        let _ = sync(&mut app);

        assert_eq!(app.focus.top(), Some(FocusId::Dialog(DialogId::HostDelete)));
        assert!(!passes_global_shortcut(&mut app));
    }

    #[test]
    fn a_command_info_dialog_without_a_rendered_entry_does_not_own_focus() {
        let mut app = crate::core::test_support::app();
        let tab = app.allocate_tab_id(crate::terminal::TabKind::Session(SessionId(1)));
        app.activate_tab(tab);
        app.terminal.command_info = Some(CommandInfo {
            session_id: SessionId(1),
            index: 0,
        });

        let _ = sync(&mut app);

        assert_eq!(app.focus.top(), Some(FocusId::Terminal(SessionId(1))));
        assert!(passes_global_shortcut(&mut app));
    }

    #[test]
    fn a_page_editor_only_owns_focus_while_its_page_is_active() {
        let mut app = crate::core::test_support::app();
        app.active_nav = crate::core::data::NavItem::Keychain;
        app.hosts.form.visible = true;

        let _ = sync(&mut app);

        assert_eq!(
            app.focus.top(),
            Some(FocusId::Cursor(CursorTarget::Keychain))
        );
        assert!(
            passes_global_shortcut(&mut app),
            "a hidden editor from another page must not block navigation"
        );
    }

    #[test]
    fn switching_terminal_tabs_closes_transient_panels() {
        let mut app = crate::core::test_support::app();
        let first = app.allocate_tab_id(crate::terminal::TabKind::Session(SessionId(1)));
        let second = app.allocate_tab_id(crate::terminal::TabKind::Session(SessionId(2)));
        app.activate_tab(first);
        app.hosts.form.visible = true;
        app.terminal.command_info = Some(CommandInfo {
            session_id: SessionId(1),
            index: 0,
        });

        app.activate_tab(second);

        assert!(!app.hosts.form.visible);
        assert_eq!(app.terminal.command_info, None);
    }

    #[test]
    fn the_command_frequency_prompt_does_not_steal_focus_from_an_inline_search() {
        let mut app = crate::core::test_support::app();
        app.active_tab = crate::app::ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.state.left.pane.filter_open = true;
        app.command_frequency_prompt = Some(CommandFrequencyPrompt {
            command: String::from("git status"),
            count: 3,
        });

        let _ = sync(&mut app);

        assert_eq!(
            app.focus.top(),
            Some(FocusId::Search(SearchId::SftpFilter {
                pane: SftpPane::Local
            }))
        );
    }

    #[test]
    fn the_command_frequency_prompt_does_not_steal_focus_from_the_terminal() {
        let mut app = crate::core::test_support::app();
        let tab = app.allocate_tab_id(crate::terminal::TabKind::Session(SessionId(7)));
        app.activate_tab(tab);
        let _ = sync(&mut app);
        assert_eq!(app.focus.top(), Some(FocusId::Terminal(SessionId(7))));

        app.command_frequency_prompt = Some(CommandFrequencyPrompt {
            command: String::from("git status"),
            count: 3,
        });
        let _ = sync(&mut app);

        assert_eq!(
            app.focus.top(),
            Some(FocusId::Terminal(SessionId(7))),
            "the passive prompt must leave terminal input focused"
        );
        assert!(
            passes_global_shortcut(&mut app),
            "the prompt must not swallow terminal keystrokes"
        );
    }
}
