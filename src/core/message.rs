// Rust guideline compliant 2026-10-09

use iced::event::Event;
use iced::{Point, Size};

use crate::app::{SessionId, StartupData};
use crate::core::data::{LocalShell, RightSidebarTab};
use crate::core::locale::Locale;
use crate::core::theme::ThemeMode;

#[derive(Debug, Clone)]
pub enum Message {
    CursorFocus(crate::core::cursor_focus::CursorTarget),
    CursorMove {
        target: crate::core::cursor_focus::CursorTarget,
        direction: crate::core::grid_cursor::GridDirection,
        extend: bool,
    },
    CursorColumns(crate::core::cursor_focus::CursorTarget, usize),
    /// Activate the item selected by the focused collection cursor.
    CursorActivate(crate::core::cursor_focus::CursorTarget),
    /// Observe the widget that owns focus after a pointer interaction.
    ObserveFocus,
    /// The stable id of the widget that currently owns focus.
    FocusedWidget(iced::widget::Id),
    Noop,
    Scrolled(&'static str, iced::widget::scrollable::Viewport),
    TermEvent(Event),
    StartupDataLoaded(StartupData),
    CopyLogs,
    AnimTick,
    ScrollbarVisibilityTimer,
    ShowToast(String),
    ShowWarningToast(String),
    ShowErrorToast(String),
    DismissToast(u64),
    CrashDialogClose,
    CrashDialogCopy,
    WindowClose,
    WindowCloseConfirm,
    WindowCloseCancel,
    /// Save the current terminal workspace without quitting.
    WorkspaceSave,
    /// Live text for the save-workspace name input.
    WorkspaceSaveName(String),
    WorkspaceSaveConfirm,
    WorkspaceSaveCancel,
    WorkspaceSaveOverwrite,
    WorkspaceSaveKeepDuplicate,
    /// Return from the duplicate-name prompt back to the name input.
    WorkspaceSaveConflictCancel,
    /// Apply a named workspace by id.
    WorkspaceApply(String),
    /// Applying closes the current terminals.
    WorkspaceApplyConfirm,
    WorkspaceApplyCancel,
    WorkspaceRenameStart(String),
    /// Live text for the rename input.
    WorkspaceRenameInput(String),
    WorkspaceRenameSubmit,
    WorkspaceRenameCancel,
    WorkspaceDeleteRequest(String),
    WorkspaceDeleteConfirm,
    WorkspaceDeleteCancel,
    WorkspaceSelect(String),
    WorkspaceHoverEnter(String),
    WorkspaceHoverLeave(String),
    WorkspaceTransferMenuToggle,
    /// Export all saved workspaces to a user-chosen JSON file.
    WorkspaceExportRequest,
    /// Result of the workspace export file write.
    WorkspaceExportSaved(Result<Option<std::path::PathBuf>, String>),
    WorkspaceImportRequest,
    /// Import file was picked (`None` = user cancelled).
    WorkspaceImportPicked(Option<std::path::PathBuf>),
    WorkspaceImportConfirm,
    WorkspaceImportCancel,
    /// Crash-recovery domain message (autosave tick, restore/dismiss prompt).
    WorkspaceRecovery(crate::workspace::recovery::Message),
    WindowMinimize,
    WindowMaximize,
    WindowToggleFullscreen,
    FullscreenChanged(bool),
    WindowDrag,
    CursorPositionChanged(Point),
    WindowPositionChanged(Option<Point>),
    WindowScaleFactorChanged(f32),
    WindowResized(Size),
    #[cfg(target_os = "windows")]
    SyncWindowChrome,
    #[cfg(target_os = "windows")]
    RevealWindowFirstFrame,
    ToggleRightSidebar,
    SwitchRightTab(RightSidebarTab),
    /// Focus a right-sidebar list after it has entered the widget tree.
    FocusRightSidebarTab(RightSidebarTab),
    /// Select a snippet row in the right sidebar without activating it.
    RightSidebarSelectSnippet(crate::core::data::SnippetId),
    /// Select a command row in the active terminal's history.
    RightSidebarSelectCommand(String),
    /// Copy one command from the active terminal session's command history
    /// (index into `TerminalSession::recent_commands`) to the clipboard.
    CommandHistoryCopy(usize),
    /// Show a modal with the start time of one command from the active
    /// terminal session's command history.
    CommandHistoryInfo(usize),
    CommandHistoryInfoClose,
    /// Execute one command from the active terminal session's command history
    /// immediately in the terminal (writes the command and submits it).
    CommandHistoryRun(usize),
    /// Live filter text for the Command History tab in the right sidebar.
    CommandHistoryFilter(String),
    /// Mouse entered/left a command history row (index into
    /// `TerminalSession::recent_commands`), driving the hover background.
    CommandHistoryHoverEnter(usize),
    CommandHistoryHoverLeave(usize),
    /// Show a bottom-right "used N times" prompt suggesting the command be
    /// saved as a snippet. Produced by the shell-integration command tracker.
    CommandFrequencyNotify {
        command: String,
        count: u32,
    },
    /// Dismiss the command-frequency prompt. `u64` is a generation guard so a
    /// stale auto-dismiss timer can't close a newer prompt.
    CommandFrequencyDismiss(u64),
    /// User clicked "Add to Snippets" on the command-frequency prompt.
    CommandFrequencyAddToSnippet {
        command: String,
    },
    SpotlightShow(SessionId),
    SpotlightCancel(SessionId),
    /// The `String` is the typed filter.
    SpotlightFilter(SessionId, String),
    SpotlightMoveUp(SessionId),
    SpotlightMoveDown(SessionId),
    SpotlightConfirm(SessionId),
    /// The `String` is the stable palette command ID.
    SpotlightPick(SessionId, String),
    SpotlightHover(SessionId, Option<usize>),
    SetTheme(ThemeMode),
    /// Periodic cross-window preference check. Only `theme_mode` is polled;
    /// other preferences intentionally remain local to the process.
    ThemePreferenceSyncCheck,
    ThemePreferenceSyncLoaded(Result<Option<ThemeMode>, String>),
    SystemThemeCheck,
    /// Background result of querying the OS colour-scheme preference.
    SystemThemeDetected(bool),
    SetTerminalFontSize(f32),
    /// Command-history limit picked from the Profiles dropdown. One of
    /// `COMMAND_HISTORY_LIMITS`, so the sidebar is never handed a ring size it
    /// cannot render.
    SetCommandHistoryLimit(usize),
    SettingsToggleCommandHistoryLimitPicker,
    /// Global toggle for OSC 7 / OSC 633 shell-hook injection (local + SSH).
    /// Defaults to on; persisted under the `shell_integration` preference.
    SetShellIntegration(bool),
    /// Shell executable used by new local terminals on Windows.
    SetLocalShell(LocalShell),
    SettingsToggleLocalShellPicker,
    SettingsToggleThemePicker,
    SetLocale(Locale),
    SettingsToggleLocalePicker,
    SettingsToggleShellIntegrationPicker,
    SetDiagnosticsLevel(crate::core::diagnostics::Level),
    SettingsToggleDiagnosticsPicker,
    /// Open the prefilled GitHub bug-report form in the default browser.
    OpenBugReport,
    /// Mouse wheel event over the topbar horizontal tab scroll area.
    /// Translates vertical wheel delta into horizontal scrolling.
    TopTabsWheel(iced::mouse::ScrollDelta),
    // Domain messages, owned by each feature module.
    Hosts(crate::hosts::Message),
    Keychain(crate::keychain::Message),
    KnownHosts(crate::known_hosts::Message),
    PortForward(crate::port_forward::Message),
    Sftp(crate::sftp::Message),
    Snippets(crate::snippets::Message),
    SshConnect(crate::ssh_connect::Message),
    Terminal(crate::terminal::Message),
}
