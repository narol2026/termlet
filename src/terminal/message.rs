use iced::widget::Id;
use iced::widget::pane_grid;
use iced::{Point, Rectangle, Size};

use crate::app::{SessionId, SplitId};
use crate::core::data::HostId;

#[derive(Debug, Clone)]
pub enum Message {
    IcedTermEvent(iced_term::Event),
    /// Deferred terminal focus, dispatched one frame after a navigation that
    /// changes the active tab (tab switch, tab close, …). A split pane's
    /// terminal is only materialized during layout, so it must not be focused
    /// in the same frame the tab changes.
    FocusTerminalWidget(SessionId),
    TermClose(SessionId),
    TermToggleLock(SessionId),
    TermCloseLockedConfirm,
    TermCloseLockedCancel,
    TermCloseOthers(SessionId),
    TermCloseRight(SessionId),
    TermRenameStart(SessionId),
    TermRenameInputChanged(String),
    TermRenameSubmit(SessionId),
    TermRenameCancel,
    TermRenameBlurCheck,
    TermContextMenuOpen(SessionId),
    TermContextMenuClose,
    DisconnectedReconnect(SessionId),
    /// Timer-driven reconnect for the current SSH outage.
    DisconnectedAutoReconnect {
        session_id: SessionId,
        generation: u64,
    },
    /// Hide the blocking connection-lost dialog without dropping the dead
    /// terminal, so its scrollback can still be selected and copied.
    DisconnectedDismiss(SessionId),
    DisconnectedClose(SessionId),
    DuplicateSession(SessionId),
    DuplicateInNewWindow(SessionId),
    StartupOpenSsh(HostId),
    StartupOpenLocal,
    RestoreWorkspace(Box<crate::workspace::WorkspaceSnapshot>),
    TerminalTabDropped {
        session_id: SessionId,
        point: Point,
    },
    TerminalDropZonesFound {
        session_id: SessionId,
        zones: Vec<(Id, Rectangle)>,
        point: Point,
    },
    TerminalTabDragging {
        session_id: SessionId,
        point: Point,
    },
    TerminalTabDragCancelled,
    TerminalTabHovered {
        session_id: SessionId,
        entered: bool,
    },
    TerminalDragPreview {
        zones: Vec<(Id, Rectangle)>,
        point: Point,
    },
    /// `split_id` is the unique identifier of the split group (see `Split::id`).
    /// It disambiguates Pane IDs that overlap across separate `pane_grid::State` instances.
    SplitClicked(SplitId, pane_grid::Pane),
    SplitDragged(SplitId, pane_grid::DragEvent),
    /// Divider drag. Carries the grid's inner size so the update path can clamp
    /// the new ratio against the per-axis pane minimums, which the widget's
    /// scalar `min_size` cannot express.
    SplitResized(SplitId, pane_grid::ResizeEvent, Size),
    SplitClose(SplitId, pane_grid::Pane),
    SplitUnsplit(SplitId, pane_grid::Pane),
    /// One-shot timer to hide a terminal scrollbar.  Carries the `session_id`
    /// and the scroll generation at the time the timer was started; if the
    /// generation has advanced by the time this fires the message is a no-op.
    HideTerminalScrollbar(SessionId, u64),
    /// Periodic poll (1 s) of each SSH terminal session's bridge health-status
    /// file; drives the topbar's yellow "connection unstable" dot.
    SshHealthTick,
    /// Chrome-style search bar input in the active terminal.
    TerminalSearch(SessionId, String),
    SearchNext(SessionId),
    SearchPrev(SessionId),
    SearchToggleCase(SessionId),
    SearchToggleWholeWord(SessionId),
    SearchToggleRegex(SessionId),
    SearchClose(SessionId),
    ShowSearch(SessionId),
}
