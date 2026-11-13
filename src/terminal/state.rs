//! Terminal tabs, sessions, splits, and per-session UI state.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use iced::widget::pane_grid;
use iced::{Point, Rectangle};
use secrecy::SecretString;

use crate::app::command::PaletteCommand;
use crate::core::cache::CommandHistoryEntry;
use crate::core::context_menu::ContextMenuAction;
use crate::core::data::{Host, HostId, RightSidebarTab, SnippetId};
use crate::core::grid_cursor::GridDirection;
use crate::core::ids::{DraftId, SessionId, SplitId, TabId};
use crate::core::list_cursor::ListCursor;
use crate::core::theme::ThemeMode;
use crate::ssh_connect::SshConnectState;

/// What a dynamic terminal tab points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    Session(SessionId),
    Draft(DraftId),
    Split(SplitId),
}

/// A dynamic tab in the topbar.
///
/// `id` is its stable slot identity and survives a draft-to-session upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub kind: TabKind,
    /// When true, closing this tab requires user confirmation.
    ///
    /// This is runtime-only and never persisted. It applies to the whole tab.
    pub locked: bool,
}

/// A new-tab or SSH connection draft.
#[derive(Clone, Default)]
pub struct Draft {
    pub query: String,
    pub cursor: ListCursor<HostId>,
    pub ssh: SshConnectState,
}

#[derive(Debug, Clone)]
pub struct SidebarState {
    pub show: bool,
    pub anim: f32,
    pub closing: bool,
    pub active_tab: RightSidebarTab,
    pub(crate) focus_list_pending: Option<RightSidebarTab>,
    pub(crate) cursors: SidebarCursors,
    /// Live filter text for the Command History tab.
    pub commands_filter: String,
    pub commands_hovered: Option<usize>,
}

/// Independent selection state for each right-sidebar list.
///
/// Each cursor stores item identity rather than visible index so selection
/// survives filtering, reloads, and reordering.
#[derive(Debug, Clone, Default)]
pub(crate) struct SidebarCursors {
    snippets: ListCursor<SnippetId>,
    commands: ListCursor<String>,
    themes: ListCursor<ThemeMode>,
}

impl SidebarCursors {
    pub(crate) fn selected_snippet(&self) -> Option<&SnippetId> {
        self.snippets.selected_key()
    }

    pub(crate) fn selected_command(&self) -> Option<&String> {
        self.commands.selected_key()
    }

    pub(crate) fn selected_theme(&self) -> Option<&ThemeMode> {
        self.themes.selected_key()
    }

    pub(crate) fn select_snippet(&mut self, id: SnippetId) {
        self.snippets.select(id);
    }

    pub(crate) fn select_command(&mut self, command: &str) {
        self.commands.select(command.to_string());
    }

    pub(crate) fn select_theme(&mut self, mode: ThemeMode) {
        self.themes.select(mode);
    }

    pub(crate) fn clear_commands(&mut self) {
        self.commands.clear();
    }

    pub(crate) fn move_in(
        &mut self,
        tab: RightSidebarTab,
        snippets: &[SnippetId],
        commands: &[String],
        themes: &[ThemeMode],
        direction: GridDirection,
    ) {
        match tab {
            RightSidebarTab::Snippets => {
                move_list_cursor(&mut self.snippets, snippets, direction);
            }
            RightSidebarTab::Commands => {
                move_list_cursor(&mut self.commands, commands, direction);
            }
            RightSidebarTab::Theme => {
                move_list_cursor(&mut self.themes, themes, direction);
            }
        }
    }
}

fn move_list_cursor<T: Clone + Eq>(
    cursor: &mut ListCursor<T>,
    items: &[T],
    direction: GridDirection,
) {
    match direction {
        GridDirection::Up | GridDirection::Left => cursor.move_previous(items),
        GridDirection::Down | GridDirection::Right => cursor.move_next(items),
    }
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            show: false,
            anim: 0.0,
            closing: false,
            active_tab: RightSidebarTab::Commands,
            focus_list_pending: None,
            cursors: SidebarCursors::default(),
            commands_filter: String::new(),
            commands_hovered: None,
        }
    }
}

/// A split pane grid containing terminal session identifiers.
#[derive(Clone)]
pub struct Split {
    pub id: SplitId,
    pub panes: pane_grid::State<SessionId>,
    pub focus: Option<pane_grid::Pane>,
    pub previous_focus: Option<pane_grid::Pane>,
}

impl Split {
    /// Focus `pane`, preserving the directly previous pane for restoration.
    pub(crate) fn focus_pane(&mut self, pane: pane_grid::Pane) {
        if self.focus == Some(pane) {
            return;
        }
        self.previous_focus = self.focus;
        self.focus = Some(pane);
    }

    /// Apply a pane-grid drag while keeping focus on the dragged pane.
    pub(crate) fn apply_drag(&mut self, event: &pane_grid::DragEvent) -> pane_grid::Pane {
        let pane = match event {
            pane_grid::DragEvent::Dropped { pane, target } => {
                self.panes.drop(*pane, *target);
                *pane
            }
            pane_grid::DragEvent::Picked { pane } | pane_grid::DragEvent::Canceled { pane } => {
                *pane
            }
        };
        self.focus_pane(pane);
        pane
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DragPreview {
    pub grid: usize,
    pub bounds: Rectangle,
}

/// Delay between automatic reconnect attempts.
///
/// Ten seconds gives a network interface switch enough time to finish before
/// the next SSH handshake starts. Shortening it can overlap attempts while
/// DHCP and routing are still changing.
pub(crate) const AUTO_RECONNECT_DELAY: Duration = Duration::from_secs(10);

/// Number of automatic reconnect attempts allowed for one outage.
///
/// Six attempts at [`AUTO_RECONNECT_DELAY`] cover about one minute without
/// retrying indefinitely while the remote host or network remains unavailable.
pub(crate) const MAX_AUTO_RECONNECT_ATTEMPTS: u8 = 6;

/// Tracks automatic reconnect scheduling for one SSH terminal.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct AutoReconnectState {
    attempts: u8,
    generation: u64,
    active: bool,
}

impl AutoReconnectState {
    /// Start or continue an outage and return the next timer generation.
    ///
    /// A live connection starts a fresh six-attempt allowance. A failed
    /// reconnect keeps using the current outage's allowance.
    pub(crate) fn begin_outage(&mut self) -> Option<u64> {
        if !self.active {
            self.attempts = 0;
        }
        self.active = true;
        self.advance_generation();
        (self.attempts < MAX_AUTO_RECONNECT_ATTEMPTS).then_some(self.generation)
    }

    /// Reserve an automatic attempt if its timer belongs to the current outage.
    pub(crate) fn begin_attempt(&mut self, generation: u64) -> bool {
        if generation != self.generation
            || !self.active
            || self.attempts >= MAX_AUTO_RECONNECT_ATTEMPTS
        {
            return false;
        }
        self.attempts = self.attempts.saturating_add(1);
        true
    }

    /// Start a fresh outage cycle after the user requests a reconnect.
    pub(crate) fn reset_for_manual_attempt(&mut self) {
        self.attempts = 0;
        self.active = true;
        self.advance_generation();
    }

    /// Finish the current outage once the bridge reports a live connection.
    pub(crate) fn mark_connected(&mut self) {
        if !self.active {
            return;
        }
        self.attempts = 0;
        self.active = false;
        self.advance_generation();
    }

    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

pub struct TerminalSession {
    pub id: SessionId,
    #[allow(dead_code, reason = "carried on the session record; not read yet")]
    pub host_id: Option<HostId>,
    pub ssh_secrets: SshSessionSecrets,
    pub label: String,
    pub terminal: iced_term::Terminal,
    pub sidebar: SidebarState,
    pub has_new_output: bool,
    pub spotlight: Option<SpotlightState>,
    pub working_directory: Option<String>,
    pub working_directory_synced: bool,
    pub osc_integration_detected: bool,
    pub recent_commands: Vec<CommandHistoryEntry>,
    pub disconnected: bool,
    pub disconnect_dismissed: bool,
    pub disconnect_anim: f32,
    pub(crate) auto_reconnect: AutoReconnectState,
    pub connection_unstable: bool,
    pub ssh_health_path: Option<std::path::PathBuf>,
    pub term_scrollbar_generation: u64,
    pub search_query: String,
    pub search_matches_count: usize,
    pub search_index: usize,
    pub search_case_sensitive: bool,
    pub search_whole_word: bool,
    pub search_regex: bool,
    pub show_search: bool,
    pub search_anim: f32,
    pub search_closing: bool,
}

/// Secrets that are valid for one SSH terminal connection attempt.
#[derive(Clone, Default)]
pub struct SshSessionSecrets {
    pub key_passphrase: Option<SecretString>,
    pub jump_key_passphrase: Option<SecretString>,
    pub expected_fingerprint: Option<String>,
    pub jump_expected_fingerprint: Option<String>,
}

impl SshSessionSecrets {
    /// Capture the connection-scoped values currently held by the GUI hosts.
    pub fn from_hosts(target: &Host, jump: Option<&Host>) -> Self {
        Self {
            key_passphrase: target.key_passphrase.clone(),
            jump_key_passphrase: jump.and_then(|host| host.key_passphrase.clone()),
            expected_fingerprint: target.expected_fingerprint.clone(),
            jump_expected_fingerprint: jump.and_then(|host| host.expected_fingerprint.clone()),
        }
    }
}

impl fmt::Debug for SshSessionSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SshSessionSecrets")
            .field(
                "key_passphrase",
                &self.key_passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "jump_key_passphrase",
                &self.jump_key_passphrase.as_ref().map(|_| "<redacted>"),
            )
            .field("expected_fingerprint", &self.expected_fingerprint)
            .field("jump_expected_fingerprint", &self.jump_expected_fingerprint)
            .finish()
    }
}

/// Whether the blocking connection-lost dialog should cover this session.
pub(crate) fn disconnect_overlay_visible(session: &TerminalSession) -> bool {
    disconnect_notice_visible(session.disconnected, session.disconnect_dismissed)
}

pub(crate) fn disconnect_notice_visible(disconnected: bool, dismissed: bool) -> bool {
    disconnected && !dismissed
}

/// Whether OSC 7 / 633 shell integration can track this terminal's directory.
///
/// Windows local shells do not support the Unix hook scripts that provide this
/// integration. Windows SSH sessions still go through the remote-shell bridge,
/// so they retain the same integration as on Unix.
pub(crate) fn supports_osc_shell_integration(host_id: Option<HostId>) -> bool {
    !cfg!(windows) || host_id.is_some()
}

#[derive(Debug, Clone)]
pub struct SpotlightState {
    pub filter_text: String,
    pub cursor: ListCursor<String>,
    pub filtered_commands: Vec<PaletteCommand>,
    pub hovered_index: Option<usize>,
    pub anim: f32,
}

impl SpotlightState {
    pub(crate) fn new(filtered_commands: Vec<PaletteCommand>) -> Self {
        let mut state = Self {
            filter_text: String::new(),
            cursor: ListCursor::new(),
            filtered_commands,
            hovered_index: None,
            anim: 0.0,
        };
        state.reset_cursor();
        state
    }

    pub(crate) fn reset_cursor(&mut self) {
        self.cursor.clear();
        if let Some(first) = self.filtered_commands.first() {
            self.cursor.select(first.id.clone());
        }
    }

    pub(crate) fn command_ids(&self) -> Vec<String> {
        self.filtered_commands
            .iter()
            .map(|command| command.id.clone())
            .collect()
    }

    pub(crate) fn selected_command(&self) -> Option<&PaletteCommand> {
        if let Some(selected) = self.cursor.selected_key()
            && let Some(command) = self
                .filtered_commands
                .iter()
                .find(|command| command.id == *selected)
        {
            return Some(command);
        }
        self.filtered_commands.first()
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.cursor
            .selected_key()
            .and_then(|selected| {
                self.filtered_commands
                    .iter()
                    .position(|command| command.id == *selected)
            })
            .or_else(|| (!self.filtered_commands.is_empty()).then_some(0))
    }
}

#[derive(Debug, Clone)]
pub struct TerminalContextMenu {
    pub session_id: SessionId,
    pub position: Point,
    pub has_tabs_to_right: bool,
    pub is_split: bool,
    pub locked: bool,
    pub cursor: ListCursor<ContextMenuAction>,
}

/// What a close request targets.
#[derive(Debug, Clone)]
pub enum TerminalCloseTarget {
    Tab(SessionId),
    Pane(SplitId, pane_grid::Pane),
}

#[derive(Debug, Clone)]
pub struct PendingLockedClose {
    pub label: String,
    pub target: TerminalCloseTarget,
}

/// The command-history entry displayed in the metadata dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandInfo {
    pub session_id: SessionId,
    pub index: usize,
}

/// State owned by the terminal workspace.
pub struct TerminalDomain {
    pub tabs: Vec<TabId>,
    pub tab_registry: HashMap<TabId, Tab>,
    pub drafts: HashMap<DraftId, Draft>,
    pub next_tab_id: usize,
    pub next_draft_id: usize,
    pub sessions: Vec<TerminalSession>,
    pub splits: HashMap<SplitId, Split>,
    pub next_split_id: usize,
    pub drag_preview: Option<DragPreview>,
    pub dragging_terminal_tab: Option<SessionId>,
    pub drag_reorder_target: Option<SessionId>,
    pub drag_insert_before: bool,
    pub hovered_terminal_id: Option<SessionId>,
    pub hovered_new_tab_id: Option<DraftId>,
    pub editing_tab: Option<SessionId>,
    pub editing_tab_text: String,
    pub command_info: Option<CommandInfo>,
    pub next_session_id: usize,
    pub context_menu: Option<TerminalContextMenu>,
    pub tab_width_anims: HashMap<TabId, f32>,
}

impl TerminalDomain {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            tab_registry: HashMap::new(),
            drafts: HashMap::new(),
            next_tab_id: 1,
            next_draft_id: 1,
            sessions: Vec::new(),
            splits: HashMap::new(),
            next_split_id: 0,
            drag_preview: None,
            dragging_terminal_tab: None,
            drag_reorder_target: None,
            drag_insert_before: false,
            hovered_terminal_id: None,
            hovered_new_tab_id: None,
            editing_tab: None,
            editing_tab_text: String::new(),
            command_info: None,
            next_session_id: 1,
            context_menu: None,
            tab_width_anims: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focusing_another_pane_preserves_only_the_directly_previous_focus() {
        let (mut panes, first) = pane_grid::State::new(SessionId(1));
        let (second, _) = panes
            .split(pane_grid::Axis::Vertical, first, SessionId(2))
            .expect("two panes can be split");
        let mut split = Split {
            id: SplitId(1),
            panes,
            focus: Some(first),
            previous_focus: None,
        };

        split.focus_pane(second);
        assert_eq!(split.focus, Some(second));
        assert_eq!(split.previous_focus, Some(first));

        split.focus_pane(second);
        assert_eq!(
            split.previous_focus,
            Some(first),
            "refocusing the current pane must not overwrite its restoration target"
        );
    }

    #[test]
    fn local_osc_tracking_matches_platform_support() {
        assert_eq!(
            supports_osc_shell_integration(None),
            !cfg!(windows),
            "Windows local shells must not drive OSC-based tracking"
        );
    }

    #[test]
    fn ssh_terminals_keep_osc_tracking_on_every_platform() {
        assert!(
            supports_osc_shell_integration(Some(HostId(1))),
            "the remote-shell bridge supports OSC integration on Windows too"
        );
    }

    #[test]
    fn automatic_reconnect_stops_after_six_attempts() {
        let mut state = AutoReconnectState::default();
        let mut generation = state
            .begin_outage()
            .expect("the first connection loss schedules a reconnect");

        for attempt in 1..=MAX_AUTO_RECONNECT_ATTEMPTS {
            assert!(state.begin_attempt(generation));
            assert_eq!(state.attempts, attempt);
            if attempt < MAX_AUTO_RECONNECT_ATTEMPTS {
                generation = state
                    .begin_outage()
                    .expect("a failed reconnect schedules the next attempt");
            } else {
                assert_eq!(
                    state.begin_outage(),
                    None,
                    "the sixth failed reconnect exhausts the allowance"
                );
            }
        }

        assert!(!state.begin_attempt(generation));
    }

    #[test]
    fn a_live_connection_starts_a_fresh_reconnect_allowance() {
        let mut state = AutoReconnectState::default();
        let generation = state
            .begin_outage()
            .expect("an outage schedules an attempt");
        assert!(state.begin_attempt(generation));

        state.mark_connected();
        let generation = state
            .begin_outage()
            .expect("a later outage gets a fresh attempt");

        assert!(state.begin_attempt(generation));
        assert_eq!(state.attempts, 1);
    }

    #[test]
    fn a_manual_reconnect_invalidates_the_pending_automatic_timer() {
        let mut state = AutoReconnectState::default();
        let stale_generation = state
            .begin_outage()
            .expect("an outage schedules an attempt");

        state.reset_for_manual_attempt();

        assert!(
            !state.begin_attempt(stale_generation),
            "the old timer must not race a manual reconnect"
        );
        assert_eq!(state.attempts, 0);
    }
}
