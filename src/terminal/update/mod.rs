// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use iced::Task;
use iced::widget::Id;
#[cfg(test)]
use iced::widget::pane_grid;
use iced_term::TerminalView;

use super::message::Message as DomainMessage;
#[cfg(test)]
use crate::app::SplitId;
use crate::app::{App, SessionId, TabId};
use crate::core::data::{Host, HostId, LogCategory};
use crate::core::diagnostics::Level;
use crate::core::message::Message;
use crate::core::theme;
#[cfg(test)]
use crate::terminal::Split;
use crate::terminal::split_layout::clamp_resize;
use crate::terminal::{TabKind, TerminalSession};
use crate::update::scroll_to_active_tab;
use rust_i18n::t;

mod drag;
mod events;
mod launch;
mod search;
mod sessions;
mod split_ops;
mod splits;
mod tabs;

pub(crate) use split_ops::allocate_tab_at;
use split_ops::{
    close_split_group, close_split_pane, close_split_pane_by_session, create_split,
    focus_split_on_session, focus_split_pane, split_existing_pane, unsplit_keep_pane,
};

pub use launch::{create_local_session, create_local_session_entity_with_dir};

use launch::{create_session_entity, ssh_terminal_settings};

fn search_options(session: &TerminalSession) -> iced_term::SearchOptions {
    iced_term::SearchOptions {
        case_sensitive: session.search_case_sensitive,
        whole_word: session.search_whole_word,
        regex: session.search_regex,
    }
}

/// Re-run the active terminal search with the current query and options.
/// Used when one of the find-bar toggles changes while a query is already
/// present, so the highlight/count update immediately.
fn rerun_terminal_search(session: &mut TerminalSession) {
    if session.search_query.is_empty() {
        session.search_matches_count = 0;
        session.search_index = 0;
        session.terminal.clear_selection();
        return;
    }

    let options = search_options(session);
    let (matches, idx) = session
        .terminal
        .search_jump(&session.search_query, 0, options);
    session.search_matches_count = matches.len();
    session.search_index = idx.unwrap_or(0);
}

fn toggle_search_option(
    app: &mut App,
    session_id: SessionId,
    toggle: impl FnOnce(&mut TerminalSession),
) {
    if let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
    {
        toggle(session);
        rerun_terminal_search(session);
    }
}

/// Description of one raw backend call, for the diagnostics file.
///
/// `Write` is summarised by its length: spelled out in full, the trace would
/// hold every byte the user typed, passwords included.
fn backend_call_desc(cmd: &iced_term::BackendCommand) -> String {
    match cmd {
        iced_term::BackendCommand::Write(data) => format!("Write({} bytes)", data.len()),
        other => format!("{other:?}"),
    }
}

/// Write one raw terminal backend call to the diagnostics file.
///
/// This traffic — a line per keystroke, per pty chunk, per mouse hover — is a
/// trace, not an activity: the lines are near-equivalent in value, they carry
/// the remote paths and command lines the terminal is working with, and the
/// Logs page holds only its newest 200 entries, so a page full of them evicts
/// the host, SFTP and keychain events the page exists to show.  None of it
/// reaches the page, and the file sees it only at Trace level.
fn trace_backend_call(
    session_id: SessionId,
    label: &str,
    host_id: Option<HostId>,
    cmd: &iced_term::BackendCommand,
) {
    if !crate::core::diagnostics::enabled_at(Level::Trace) {
        return;
    }
    crate::core::diagnostics::log(
        Level::Trace,
        "TERM",
        format!(
            "BackendCall session={session_id} (\"{label}\") host_id={host_id:?}: {}",
            backend_call_desc(cmd)
        ),
    );
}

/// Line the Logs page shows for a command the shell announced over OSC 633.
fn command_run_line(session_id: SessionId, command: &str) -> String {
    format!("session {session_id}: command \"{command}\"")
}

/// Line for a command that came back with a non-zero status. `command` is the
/// command the shell announced for that run, and is `None` when the status
/// arrived without one — a `Ctrl-C` from a shell-side hook, typically.
fn command_exit_line(session_id: SessionId, code: i32, command: Option<&str>) -> String {
    match command {
        Some(command) => {
            format!("session {session_id}: command exited with code {code}: \"{command}\"")
        }
        None => format!("session {session_id}: command exited with code {code}"),
    }
}

/// Line for a change of directory. Shells that emit OSC 7 on every prompt
/// repeat the directory they already reported, so the caller only fires on an
/// actual move.
fn cwd_change_line(session_id: SessionId, previous: Option<&str>, dir: &str) -> String {
    match previous {
        Some(previous) => {
            format!("session {session_id}: working directory {previous} -> {dir}")
        }
        None => format!("session {session_id}: working directory {dir}"),
    }
}

pub fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "terminal::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        message @ (DomainMessage::FocusTerminalWidget(_)
        | DomainMessage::DisconnectedReconnect(_)
        | DomainMessage::DisconnectedAutoReconnect { .. }
        | DomainMessage::DisconnectedDismiss(_)
        | DomainMessage::DisconnectedClose(_)) => sessions::update(app, message),
        message
        @ (DomainMessage::IcedTermEvent(_) | DomainMessage::HideTerminalScrollbar(_, _)) => {
            events::update(app, message)
        }

        message @ (DomainMessage::TermClose(_)
        | DomainMessage::TermToggleLock(_)
        | DomainMessage::TermCloseLockedConfirm
        | DomainMessage::TermCloseLockedCancel
        | DomainMessage::DuplicateSession(_)
        | DomainMessage::DuplicateInNewWindow(_)
        | DomainMessage::TermContextMenuOpen(_)
        | DomainMessage::TermContextMenuClose
        | DomainMessage::TermCloseOthers(_)
        | DomainMessage::TermCloseRight(_)
        | DomainMessage::TermRenameStart(_)
        | DomainMessage::TermRenameInputChanged(_)
        | DomainMessage::TermRenameSubmit(_)
        | DomainMessage::TermRenameCancel
        | DomainMessage::TermRenameBlurCheck) => tabs::update(app, message),

        message @ (DomainMessage::StartupOpenSsh(_)
        | DomainMessage::StartupOpenLocal
        | DomainMessage::RestoreWorkspace(_)) => sessions::update(app, message),

        message @ (DomainMessage::TerminalTabDragCancelled
        | DomainMessage::TerminalTabHovered { .. }
        | DomainMessage::TerminalTabDropped { .. }
        | DomainMessage::TerminalTabDragging { .. }
        | DomainMessage::TerminalDragPreview { .. }
        | DomainMessage::TerminalDropZonesFound { .. }) => drag::update(app, message),

        message @ (DomainMessage::SplitClicked(_, _)
        | DomainMessage::SplitDragged(_, _)
        | DomainMessage::SplitResized(_, _, _)
        | DomainMessage::SplitClose(_, _)
        | DomainMessage::SplitUnsplit(_, _)) => splits::update(app, &message),

        message @ DomainMessage::SshHealthTick => sessions::update(app, message),
        message @ (DomainMessage::TerminalSearch(_, _)
        | DomainMessage::SearchNext(_)
        | DomainMessage::SearchPrev(_)
        | DomainMessage::SearchToggleCase(_)
        | DomainMessage::SearchToggleWholeWord(_)
        | DomainMessage::SearchToggleRegex(_)
        | DomainMessage::SearchClose(_)
        | DomainMessage::ShowSearch(_)) => search::update(app, message),
    }
}

pub(crate) fn create_ssh_session_entity(
    app: &mut App,
    host: &Host,
    secrets: &crate::terminal::SshSessionSecrets,
) -> Result<(SessionId, Task<Message>), String> {
    let session_id = SessionId(app.terminal.next_session_id as u32);

    let target = format!("{}@{}:{}", host.username, host.hostname, host.port);

    let label = if host.name.is_empty() {
        host.hostname.clone()
    } else {
        host.name.clone()
    };
    // Per-session SSH health-status file. Session ids restart at 0 on every
    // app launch, so clear any file left by a previous run for this id — the
    // bridge will write a fresh "stable" marker once it is up.
    let health_path = ssh_health_path(session_id);
    let term_palette = theme::term_palette(app.effective_theme_mode());
    let settings = ssh_terminal_settings(
        host,
        secrets,
        term_palette,
        app.preferences.terminal_font_size,
        Some(&health_path),
        app.preferences.shell_integration,
    )?;

    app.terminal.next_session_id += 1;
    let _ = std::fs::remove_file(&health_path);
    app.add_log(
        LogCategory::Terminal,
        format!("SSH session opened to {target}"),
    );

    Ok(create_session_entity(
        app,
        session_id,
        Some(host.id),
        label,
        settings,
        Some(health_path),
        secrets.clone(),
    ))
}

pub fn create_ssh_session(
    app: &mut App,
    host: &Host,
    secrets: &crate::terminal::SshSessionSecrets,
) -> Result<(SessionId, Task<Message>), String> {
    let (session_id, mut task) = create_ssh_session_entity(app, host, secrets)?;

    if app
        .terminal
        .sessions
        .iter()
        .any(|session| session.id == session_id)
    {
        let (_, attach_task) = attach_session_tab_at(app, session_id, usize::MAX);
        task = Task::batch([task, attach_task]);
    }

    Ok((session_id, task))
}

/// Report that the current executable cannot launch the SSH bridge.
///
/// The technical cause is logged for diagnostics. The user sees a stable,
/// localized instruction to restart Termlet.
pub(crate) fn ssh_bridge_unavailable_task(app: &mut App, error: &str) -> Task<Message> {
    app.add_log(
        LogCategory::Terminal,
        format!("SSH bridge unavailable: {error}"),
    );
    Task::done(Message::ShowErrorToast(
        t!("ssh.bridge_restart_required").to_string(),
    ))
}

/// Path of the per-session SSH health-status file the ssh-bridge subprocess
/// writes into and the GUI polls to grade the link (see
/// `TerminalSession::connection_unstable`). Derived from the session id so both
/// the env var handed to the bridge and the value stored on the session can use
/// one shared rule without drifting.
fn ssh_health_path(session_id: SessionId) -> PathBuf {
    crate::core::paths::config_dir().join(format!("ssh_health_{session_id}"))
}

/// Poll every SSH session's bridge health-status file and update its
/// `connection_unstable` flag, which drives the topbar's yellow dot. Runs once
/// per second (see the `SshHealthTick` subscription). Sessions without a health
/// file (local terminals, or an external-`ssh` fallback) stay stable.
pub fn poll_ssh_health(app: &mut App) {
    for session in &mut app.terminal.sessions {
        let Some(path) = session.ssh_health_path.as_ref() else {
            continue;
        };
        // Missing / unreadable file → treated as stable: the bridge writes the
        // file as soon as it starts, so absence only happens before startup or
        // after a close. A live "stable" marker also ends the reconnect outage;
        // the bridge writes it only after authentication, PTY allocation, and
        // remote-shell startup have all succeeded.
        let health = std::fs::read_to_string(path).ok();
        let health = health.as_deref().map(str::trim);
        session.connection_unstable = health == Some("unstable");
        if health == Some("stable") && !session.disconnected {
            session.auto_reconnect.mark_connected();
        }
    }
}

/// Attach a freshly-created session to a terminal tab at `position`.
///
/// The session itself must already exist in `app.terminal.sessions`. This is
/// the single tab-attach path used by normal local/SSH creation; workspace
/// restore intentionally bypasses it and builds its own tab/split layout.
pub(crate) fn attach_session_tab_at(
    app: &mut App,
    session_id: SessionId,
    position: usize,
) -> (TabId, Task<Message>) {
    let tab_id = allocate_tab_at(app, TabKind::Session(session_id), position);
    app.activate_tab(tab_id);

    // Deferred one frame; see `request_terminal_focus`.
    let focus = request_terminal_focus(app, session_id);
    let scroll = scroll_to_active_tab(app);
    (tab_id, Task::batch([focus, scroll]))
}

/// Apply the saved label to an already-created terminal session.
pub(crate) fn apply_restored_session_label(
    app: &mut App,
    session_id: SessionId,
    label_override: Option<&str>,
) {
    let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    else {
        return;
    };

    if let Some(label) = label_override.filter(|label| !label.is_empty()) {
        session.label = label.to_string();
    }
}

/// The session id a terminal tab's drag zone is keyed by: its own session for a
/// standalone tab, or the split's focused pane session for a split tab.
fn tab_representative_session(app: &App, tid: TabId) -> Option<SessionId> {
    match app.terminal.tab_registry.get(&tid).map(|t| t.kind) {
        Some(TabKind::Session(sid)) => Some(sid),
        Some(TabKind::Split(split_id)) => app
            .terminal
            .splits
            .get(&split_id)
            .and_then(|s| s.focus.and_then(|pane| s.panes.get(pane).copied())),
        _ => None,
    }
}

/// The drop-zone ids of every visible terminal tab, keyed by the tab's
/// representative session (standalone session id or a split's focused pane).
fn tab_zone_ids(app: &App) -> Vec<Id> {
    app.terminal
        .tabs
        .iter()
        .filter_map(|&tid| tab_representative_session(app, tid))
        .map(|sid| Id::from(format!("tab_zone_{sid}")))
        .collect()
}

/// Move a terminal tab before or after another terminal tab in the unified
/// `tabs` order. Does nothing if either session is not found or if they map to
/// the same tab.
fn reorder_terminal_tab(
    app: &mut App,
    dragged_sid: SessionId,
    target_sid: SessionId,
    insert_before: bool,
) -> Task<Message> {
    let Some(dragged_tid) = app.tab_id_for_terminal_session(dragged_sid) else {
        return Task::none();
    };
    let Some(target_tid) = app.tab_id_for_terminal_session(target_sid) else {
        return Task::none();
    };
    if dragged_tid == target_tid {
        return Task::none();
    }

    let Some(from_pos) = app.terminal.tabs.iter().position(|&t| t == dragged_tid) else {
        return Task::none();
    };
    let tab = app.terminal.tabs.remove(from_pos);

    let Some(target_pos) = app.terminal.tabs.iter().position(|&t| t == target_tid) else {
        // The target tab is gone (it disappeared between the two lookups);
        // put the dragged tab back where it came from and leave the order
        // alone rather than panicking mid-drag.
        app.terminal.tabs.insert(from_pos, tab);
        return Task::none();
    };
    let insert_pos = if insert_before {
        target_pos
    } else {
        target_pos + 1
    };
    app.terminal.tabs.insert(insert_pos, tab);

    app.activate_tab(dragged_tid);

    app.add_log(
        LogCategory::Terminal,
        format!("Reordered terminal tab (session_id={dragged_sid}, position={insert_pos})"),
    );

    Task::none()
}

/// Remove a session and its ssh-bridge health file.
///
/// Returns the closed session's label for logging. Does not touch tabs or
/// split panes; callers remove the entity's reference around this.
fn remove_session(app: &mut App, session_id: SessionId) -> Option<String> {
    let session = app.terminal.sessions.iter().find(|s| s.id == session_id)?;
    let label = session.label.clone();
    let health_path = session.ssh_health_path.clone();
    app.terminal.sessions.retain(|s| s.id != session_id);
    if let Some(path) = health_path {
        let _ = std::fs::remove_file(&path);
    }
    Some(label)
}

/// Close a standalone terminal session: remove its entity, its `TabKind::Session`
/// tab, and focus the neighboring tab. Split-pane sessions are closed through
/// the split functions instead (they have no tab of their own).
pub(crate) fn close_session(app: &mut App, session_id: SessionId) -> Task<Message> {
    let tab_id = app.tab_id_for_session(session_id);
    let was_active = app.active_terminal_session_id() == Some(session_id);

    let Some(label) = remove_session(app, session_id) else {
        app.add_log(
            LogCategory::Terminal,
            format!("close_session: session {session_id} not found"),
        );
        return Task::none();
    };

    app.add_log(
        LogCategory::Terminal,
        format!("Closed terminal \"{label}\" (session_id={session_id})"),
    );

    if let Some(tid) = tab_id {
        app.activate_neighbor_after_close(tid);
        app.remove_tab(tid);
    }

    let mut tasks: Vec<Task<Message>> = Vec::new();
    if was_active {
        app.seed_tab_width_anims();
        let scroll = scroll_to_active_tab(app);
        tasks.push(scroll);
    }
    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks)
    }
}

/// Close the tab hosting `session_id`: a whole split group when the session
/// lives inside a split, otherwise the standalone session.
/// Close a terminal tab or a single pane, honoring the hosting tab's lock. A
/// locked tab is not closed immediately: the close is deferred behind the
/// locked-close confirm dialog and `Task::none()` is returned.
fn close_terminal_guarded(
    app: &mut App,
    target: crate::terminal::TerminalCloseTarget,
) -> Task<Message> {
    let locked = match &target {
        crate::terminal::TerminalCloseTarget::Tab(session_id) => app.tab_is_locked(*session_id),
        crate::terminal::TerminalCloseTarget::Pane(split_id, _) => app
            .tab_id_for_split(*split_id)
            .is_some_and(|tid| app.tab_is_locked_by_id(tid)),
    };

    if locked {
        if app.pending_locked_close.is_none() {
            let label = terminal_close_label(app, &target);
            app.pending_locked_close = Some(crate::terminal::PendingLockedClose { label, target });
        }
        Task::none()
    } else {
        close_terminal_target(app, &target)
    }
}

/// Run a close target immediately, with no lock check.
fn close_terminal_target(
    app: &mut App,
    target: &crate::terminal::TerminalCloseTarget,
) -> Task<Message> {
    match target {
        crate::terminal::TerminalCloseTarget::Tab(session_id) => {
            close_terminal_tab(app, *session_id)
        }
        crate::terminal::TerminalCloseTarget::Pane(split_id, pane) => {
            close_split_pane(app, *split_id, *pane)
        }
    }
}

/// The session label shown in the locked-close dialog for a close target.
fn terminal_close_label(app: &App, target: &crate::terminal::TerminalCloseTarget) -> String {
    let session_id = match target {
        crate::terminal::TerminalCloseTarget::Tab(session_id) => Some(*session_id),
        crate::terminal::TerminalCloseTarget::Pane(split_id, pane) => app
            .terminal
            .splits
            .get(split_id)
            .and_then(|s| s.panes.get(*pane).copied()),
    };
    session_id
        .and_then(|sid| app.terminal.sessions.iter().find(|s| s.id == sid))
        .map(|s| s.label.clone())
        .unwrap_or_default()
}

pub(crate) fn close_terminal_tab(app: &mut App, session_id: SessionId) -> Task<Message> {
    if app.terminal.editing_tab == Some(session_id) {
        app.terminal.editing_tab = None;
        app.terminal.editing_tab_text = String::new();
    }
    match app.split_for_session(session_id) {
        Some(split_id) => close_split_group(app, split_id),
        None => close_session(app, session_id),
    }
}

/// Close a terminal tab (session or split) by its tab id. Drafts are closed by
/// their own message handler.
fn close_terminal_tab_id(app: &mut App, tid: TabId) -> Task<Message> {
    match app.terminal.tab_registry.get(&tid).map(|t| t.kind) {
        Some(TabKind::Session(sid)) => close_terminal_tab(app, sid),
        Some(TabKind::Split(split_id)) => close_split_group(app, split_id),
        _ => Task::none(),
    }
}

/// Close every dynamic terminal tab (sessions, split groups, and drafts) so a
/// workspace can be applied into a clean state. This is intentionally a bulk
/// operation that bypasses per-tab lock confirmations: the caller must have
/// already shown the workspace-apply confirmation, which reports locked tabs.
pub(crate) fn close_all_terminal_tabs_for_workspace_apply(app: &mut App) {
    app.terminal.context_menu = None;
    app.terminal.editing_tab = None;
    app.terminal.editing_tab_text = String::new();
    app.terminal.drag_preview = None;
    app.terminal.dragging_terminal_tab = None;
    app.terminal.drag_reorder_target = None;
    app.terminal.drag_insert_before = false;
    app.terminal.hovered_terminal_id = None;
    app.terminal.hovered_new_tab_id = None;
    app.terminal.tab_width_anims.clear();
    app.pending_locked_close = None;

    let tab_ids = app.terminal.tabs.clone();
    for tid in tab_ids {
        let kind = app.terminal.tab_registry.get(&tid).map(|tab| tab.kind);
        match kind {
            Some(TabKind::Session(sid)) => {
                let _ = close_terminal_tab(app, sid);
            }
            Some(TabKind::Split(split_id)) => {
                let _ = close_split_group(app, split_id);
            }
            Some(TabKind::Draft(draft_id)) => {
                app.add_log(
                    LogCategory::Terminal,
                    format!("Closed draft tab while applying workspace (draft_id={draft_id})"),
                );
                if let Some(draft_tab_id) = app.tab_id_for_draft(draft_id) {
                    app.remove_tab(draft_tab_id);
                }
                app.terminal.drafts.remove(&draft_id);
            }
            None => {}
        }
    }

    app.active_tab = crate::app::ActiveTab::Workspace(crate::app::Workspace::Vaults);
    app.seed_tab_width_anims();
}

pub(crate) fn focus_session(app: &mut App, session_id: SessionId) -> Task<Message> {
    // Resolve the tab that references this session: its own `TabKind::Session`
    // tab, or the split tab hosting it (updating that split's focused pane).
    let tab_id = if let Some(tid) = app.tab_id_for_session(session_id) {
        tid
    } else if let Some(split_id) = app.split_for_session(session_id) {
        focus_split_on_session(app, split_id, session_id);
        match app.tab_id_for_split(split_id) {
            Some(tid) => tid,
            None => return Task::none(),
        }
    } else {
        return Task::none();
    };

    app.seed_tab_width_anims();
    app.activate_tab(tab_id);

    let scroll = scroll_to_active_tab(app);

    // Re-apply the active search query to the newly focused terminal so
    // that the search result counter reflects the correct pane grid when
    // switching focus between split panes.
    if let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
    {
        let search_query = session.search_query.clone();
        if !search_query.is_empty() {
            let options = search_options(session);
            let (matches, idx) = session.terminal.search_jump(&search_query, 0, options);
            session.search_matches_count = matches.len();
            session.search_index = idx.unwrap_or(0);
        }
    }

    // Focus the terminal one frame later: this session's tab was just
    // activated above, and a split pane's terminal is only materialized during
    // the layout that follows — the same deferred convention as terminal
    // owners in the app focus stack.
    request_terminal_focus(app, session_id).chain(scroll)
}

/// Request keyboard focus for a terminal session. The **only** way to focus a
/// terminal whose tab/layout may be new; there is deliberately no public
/// "focus immediately" variant for callers to choose between.
///
/// Clears the session's "new output" badge now and defers the actual focus to
/// the next frame (via [`|v0| Message::Terminal(crate::terminal::message::Message::FocusTerminalWidget(v0))`]). Navigation swaps
/// `active_tab` in the same update that requests focus, and a split pane's
/// terminal is only materialized during the subsequent layout — so focusing
/// immediately would traverse a tree that does not yet contain the widget.
/// Text inputs (draft search, SSH password) are laid out directly and still
/// receive their focus request in the same frame from [`crate::focus`].
///
/// The only `TerminalView::focus` calls left in the app are re-focusing a
/// terminal that has never left the rendered tree (spotlight dismissal, font
/// change) — the terminal is provably on screen there. Any *new* code that
/// focuses a terminal must go through this entry point.
pub(crate) fn request_terminal_focus(app: &mut App, session_id: SessionId) -> Task<Message> {
    if let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
    {
        session.has_new_output = false;
    }
    Task::done(Message::Terminal(
        crate::terminal::message::Message::FocusTerminalWidget(session_id),
    ))
}

/// Apply a previously requested terminal focus. Private: only reachable via
/// the deferred [`|v0| Message::Terminal(crate::terminal::message::Message::FocusTerminalWidget(v0))`], one frame after the target
/// tab became active and its terminal was materialized.
///
/// Stale-request guard: the focus only lands if `session_id` is *still* the
/// active terminal when the deferred message runs. Between the request and this
/// frame the user may have navigated to another tab, closed the session, or
/// focused a different split pane — an unconditional `operation::focus` would
/// silently no-op against an unrendered widget (harmless) or, worse, steal
/// focus from whatever the user just clicked. Only the current keyboard target
/// may be focused, so both failure modes drop the stale request.
fn apply_terminal_focus(app: &mut App, session_id: SessionId) -> Task<Message> {
    if app.active_terminal_session_id() != Some(session_id) {
        return Task::none();
    }
    let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
    else {
        return Task::none();
    };
    let widget_id = session.terminal.widget_id().clone();
    TerminalView::focus(widget_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ActiveTab;
    use crate::terminal::split::{SplitDirection, pane_for_session};

    // ---- split groups ----
    //
    // A split is a `pane_grid` of session ids plus the tab that shows it, and
    // nothing here reaches the PTY: the sessions are only ever referred to by
    // id, so a test can build the grouping without spawning a shell. What is
    // pinned below is the bookkeeping — which tab survives, which sessions are
    // closed, where the split lands in the tab order.

    /// Nested pane configuration holding `sessions` left to right.
    fn configuration(sessions: &[SessionId]) -> pane_grid::Configuration<SessionId> {
        match sessions {
            [only] => pane_grid::Configuration::Pane(*only),
            [first, rest @ ..] => pane_grid::Configuration::Split {
                axis: pane_grid::Axis::Vertical,
                ratio: 0.5,
                a: Box::new(pane_grid::Configuration::Pane(*first)),
                b: Box::new(configuration(rest)),
            },
            [] => panic!("a split needs at least one session"),
        }
    }

    /// A split over `sessions` with its own tab, in the order `create_split`
    /// would leave them.
    fn with_split(app: &mut App, sessions: &[SessionId]) -> SplitId {
        let panes = pane_grid::State::with_configuration(configuration(sessions));
        let focus = pane_for_session(&panes, sessions[0]);
        let id = app.allocate_split_id();
        app.terminal.splits.insert(
            id,
            Split {
                id,
                panes,
                focus,
                previous_focus: None,
            },
        );
        app.allocate_tab_id(TabKind::Split(id));
        id
    }

    /// A standalone session tab. The session itself is never created: nothing
    /// in the grouping code looks it up.
    fn with_session_tab(app: &mut App, session: SessionId) -> TabId {
        app.allocate_tab_id(TabKind::Session(session))
    }

    fn sessions_in(app: &App, split_id: SplitId) -> Vec<SessionId> {
        let mut ids: Vec<SessionId> = app
            .terminal
            .splits
            .get(&split_id)
            .expect("the split still exists")
            .panes
            .iter()
            .map(|(_, session)| *session)
            .collect();
        ids.sort();
        ids
    }

    fn pane_of(app: &App, split_id: SplitId, session: SessionId) -> pane_grid::Pane {
        app.terminal
            .splits
            .get(&split_id)
            .expect("the split still exists")
            .panes
            .iter()
            .find_map(|(pane, sid)| (*sid == session).then_some(*pane))
            .expect("the session is in the split")
    }

    #[test]
    fn splitting_takes_the_targets_slot_and_keeps_the_dragged_side_focused() {
        let mut app = crate::core::test_support::app();
        let target = SessionId(1);
        let dragged = SessionId(2);
        with_session_tab(&mut app, target);
        let dragged_tab = with_session_tab(&mut app, dragged);

        let _ = create_split(&mut app, target, dragged, SplitDirection::Right);

        assert_eq!(app.terminal.tabs.len(), 1, "two tabs became one split tab");
        assert_ne!(app.terminal.tabs[0], dragged_tab);
        let TabKind::Split(split_id) = app.terminal.tab_registry[&app.terminal.tabs[0]].kind else {
            panic!("the surviving tab shows the split");
        };
        assert_eq!(sessions_in(&app, split_id), vec![target, dragged]);
        assert_eq!(
            app.terminal.splits[&split_id].focus,
            Some(pane_of(&app, split_id, dragged)),
            "the tab the user dropped keeps the focus"
        );
        assert!(app.terminal.splits[&split_id].previous_focus.is_none());
    }

    #[test]
    fn splitting_onto_a_pane_moves_the_whole_dragged_group_in() {
        let mut app = crate::core::test_support::app();
        let dropped = SessionId(1);
        let grouped = [SessionId(2), SessionId(3)];
        let dropped_tab = with_session_tab(&mut app, dropped);
        let target_split = with_split(&mut app, &grouped);
        let target_pane = pane_of(&app, target_split, grouped[1]);

        let _ = split_existing_pane(
            &mut app,
            target_split,
            target_pane,
            dropped,
            SplitDirection::Top,
        );

        assert_eq!(app.terminal.splits.len(), 1, "the drop reused the split");
        assert!(
            !app.terminal.tabs.contains(&dropped_tab),
            "the dropped session's own tab is gone: it lives in the split now"
        );
        assert_eq!(
            sessions_in(&app, target_split),
            vec![SessionId(1), SessionId(2), SessionId(3)],
            "the dropped session joined the group without displacing anyone"
        );
        assert_eq!(
            app.terminal.splits[&target_split]
                .focus
                .and_then(|pane| app.terminal.splits[&target_split].panes.get(pane).copied()),
            Some(dropped),
            "the arrival takes the focus so the user can keep typing"
        );
    }

    #[test]
    fn dropping_a_group_onto_its_own_split_only_moves_the_focus() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2), SessionId(3)];
        let split_id = with_split(&mut app, &sessions);
        let first = pane_of(&app, split_id, sessions[0]);

        let _ = split_existing_pane(
            &mut app,
            split_id,
            first,
            sessions[2],
            SplitDirection::Right,
        );

        assert_eq!(
            sessions_in(&app, split_id),
            sessions.to_vec(),
            "a drag that lands inside its own group must not tear it apart"
        );
        assert_eq!(
            app.terminal.splits[&split_id].focus,
            Some(first),
            "the drop is treated as a plain focus change onto the pane it landed on"
        );
    }

    #[test]
    fn dropping_a_pane_onto_itself_only_moves_the_focus() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2)];
        let split_id = with_split(&mut app, &sessions);
        let second = pane_of(&app, split_id, sessions[1]);

        let _ = split_existing_pane(
            &mut app,
            split_id,
            second,
            sessions[1],
            SplitDirection::Left,
        );

        assert_eq!(sessions_in(&app, split_id), sessions.to_vec());
        assert_eq!(app.terminal.splits[&split_id].focus, Some(second));
    }

    #[test]
    fn a_drop_on_a_pane_that_is_gone_changes_nothing() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2)];
        let split_id = with_split(&mut app, &sessions);
        let stale = pane_of(&app, split_id, sessions[1]);
        let _ = close_split_pane(&mut app, split_id, stale);
        let before = app.terminal.tabs.clone();

        let _ = split_existing_pane(
            &mut app,
            split_id,
            stale,
            SessionId(9),
            SplitDirection::Left,
        );

        assert_eq!(app.terminal.tabs, before);
        assert!(!app.terminal.splits.contains_key(&split_id));
    }

    #[test]
    fn closing_a_pane_of_three_leaves_the_split_standing() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2), SessionId(3)];
        let split_id = with_split(&mut app, &sessions);
        let doomed = pane_of(&app, split_id, sessions[0]);

        let _ = close_split_pane(&mut app, split_id, doomed);

        assert!(app.terminal.splits.contains_key(&split_id));
        assert_eq!(sessions_in(&app, split_id), vec![sessions[1], sessions[2]]);
        assert_eq!(
            app.terminal.splits[&split_id].previous_focus, None,
            "the focus moves to a surviving pane, not into history"
        );
        assert!(app.tab_id_for_split(split_id).is_some());
    }

    #[test]
    fn closing_a_pane_of_two_dissolves_the_split_into_a_standalone_tab() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2)];
        let split_id = with_split(&mut app, &sessions);
        let doomed = pane_of(&app, split_id, sessions[0]);

        let _ = close_split_pane(&mut app, split_id, doomed);

        assert!(!app.terminal.splits.contains_key(&split_id));
        assert_eq!(
            app.terminal.tabs.len(),
            1,
            "the split tab is replaced by the survivor's own tab"
        );
        assert_eq!(
            app.terminal.tab_registry[&app.terminal.tabs[0]].kind,
            TabKind::Session(sessions[1])
        );
    }

    #[test]
    fn a_dissolved_split_carries_its_lock_onto_the_survivor() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2)];
        let split_id = with_split(&mut app, &sessions);
        let split_tab = app.tab_id_for_split(split_id).unwrap();
        app.set_tab_locked_by_id(split_tab, true);
        let doomed = pane_of(&app, split_id, sessions[0]);

        let _ = close_split_pane(&mut app, split_id, doomed);

        let survivor = app
            .tab_id_for_session(sessions[1])
            .expect("the survivor has a tab");
        assert!(
            app.tab_is_locked_by_id(survivor),
            "closing one pane must not quietly unlock a locked terminal"
        );
    }

    #[test]
    fn unsplitting_promotes_the_extracted_pane_and_its_sibling() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2)];
        let split_id = with_split(&mut app, &sessions);
        let kept = pane_of(&app, split_id, sessions[1]);

        let _ = unsplit_keep_pane(&mut app, split_id, kept);

        assert!(!app.terminal.splits.contains_key(&split_id));
        assert_eq!(
            app.terminal.tabs.len(),
            2,
            "both panes survive as their own tabs"
        );
        assert_eq!(
            app.terminal.tab_registry[&app.terminal.tabs[0]].kind,
            TabKind::Session(sessions[0]),
            "the sibling takes the slot the split tab occupied"
        );
        assert_eq!(
            app.terminal.tab_registry[&app.terminal.tabs[1]].kind,
            TabKind::Session(sessions[1]),
            "the pane the user kept lands in the next slot, and keeps the focus"
        );
        assert_eq!(
            app.active_tab,
            ActiveTab::Tab(app.terminal.tabs[1]),
            "focus follows the extracted pane"
        );
    }

    #[test]
    fn unsplitting_a_group_of_three_keeps_the_rest_in_a_split() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2), SessionId(3)];
        let split_id = with_split(&mut app, &sessions);
        let kept = pane_of(&app, split_id, sessions[2]);

        let _ = unsplit_keep_pane(&mut app, split_id, kept);

        assert!(
            app.terminal.splits.contains_key(&split_id),
            "two panes are still a split"
        );
        assert_eq!(sessions_in(&app, split_id), vec![sessions[0], sessions[1]]);
        assert_eq!(
            app.terminal.tabs.len(),
            2,
            "the extracted pane gets its own tab beside the split"
        );
    }

    #[test]
    fn closing_a_split_group_takes_every_pane_with_it() {
        let mut app = crate::core::test_support::app();
        let sessions = [SessionId(1), SessionId(2), SessionId(3)];
        let split_id = with_split(&mut app, &sessions);
        let neighbour = with_session_tab(&mut app, SessionId(9));
        app.activate_tab(app.tab_id_for_split(split_id).unwrap());

        let _ = close_split_group(&mut app, split_id);

        assert!(app.terminal.splits.is_empty());
        assert_eq!(app.terminal.tabs, vec![neighbour]);
        assert_eq!(
            app.active_tab,
            ActiveTab::Tab(neighbour),
            "the tab next to the closed group takes over"
        );
    }

    #[test]
    fn closing_a_split_that_is_already_gone_is_harmless() {
        let mut app = crate::core::test_support::app();
        let stale_split = with_split(&mut app, &[SessionId(1), SessionId(2)]);
        let stale_pane = pane_of(&app, stale_split, SessionId(1));
        app.terminal.splits.remove(&stale_split);
        let before = app.terminal.tabs.clone();

        let _ = close_split_group(&mut app, stale_split);
        let _ = unsplit_keep_pane(&mut app, stale_split, stale_pane);
        let _ = close_split_pane(&mut app, stale_split, stale_pane);

        assert_eq!(app.terminal.tabs, before);
    }

    #[test]
    fn a_tabs_representative_session_is_its_focused_pane() {
        let mut app = crate::core::test_support::app();
        let session_tab = with_session_tab(&mut app, SessionId(4));
        let split_id = with_split(&mut app, &[SessionId(1), SessionId(2)]);
        let split_tab = app.tab_id_for_split(split_id).unwrap();
        let draft_tab = app.allocate_tab_id(TabKind::Draft(crate::app::DraftId(1)));

        assert_eq!(
            tab_representative_session(&app, session_tab),
            Some(SessionId(4))
        );
        assert_eq!(
            tab_representative_session(&app, split_tab),
            Some(SessionId(1)),
            "a split stands for whichever pane holds the focus"
        );
        assert_eq!(
            tab_representative_session(&app, draft_tab),
            None,
            "a draft tab has no terminal behind it"
        );

        // A split with nothing focused reports nothing rather than guessing.
        app.terminal.splits.get_mut(&split_id).unwrap().focus = None;
        assert_eq!(tab_representative_session(&app, split_tab), None);
    }

    #[test]
    fn attaching_a_session_puts_its_tab_where_it_was_asked_for() {
        let mut app = crate::core::test_support::app();
        let first = with_session_tab(&mut app, SessionId(1));
        let last = with_session_tab(&mut app, SessionId(2));

        let (tab, _) = attach_session_tab_at(&mut app, SessionId(3), 1);

        assert_eq!(
            app.terminal.tabs,
            vec![first, tab, last],
            "the tab slides in at the position instead of always appending"
        );
        assert_eq!(
            app.terminal.tab_registry[&tab].kind,
            TabKind::Session(SessionId(3))
        );
        assert_eq!(
            app.active_tab,
            ActiveTab::Tab(tab),
            "the tab that was just attached is the one the user is looking at"
        );
    }

    #[test]
    fn restoring_a_label_for_a_session_that_is_gone_changes_nothing() {
        let mut app = crate::core::test_support::app();

        apply_restored_session_label(&mut app, SessionId(404), Some("web"));

        assert!(app.terminal.sessions.is_empty());
        assert!(app.terminal.tabs.is_empty());
    }

    #[test]
    fn the_backend_trace_summarises_written_bytes_instead_of_quoting_them() {
        let desc = backend_call_desc(&iced_term::BackendCommand::Write(b"hunter2".to_vec()));

        assert_eq!(desc, "Write(7 bytes)");
        assert!(
            !desc.contains("hunter2"),
            "typed text must not reach the diagnostics file"
        );

        // Everything else keeps its `Debug` shape: the trace is read when the
        // terminal integration misbehaves, and the details are the point.
        assert_eq!(
            backend_call_desc(&iced_term::BackendCommand::Resize(None, None)),
            "Resize(None, None)"
        );
    }

    #[test]
    fn a_command_and_its_exit_code_both_name_what_ran() {
        assert_eq!(
            command_run_line(SessionId(7), "cd ./server/"),
            "session 7: command \"cd ./server/\""
        );
        assert_eq!(
            command_exit_line(SessionId(7), 130, Some("cd ./server/")),
            "session 7: command exited with code 130: \"cd ./server/\""
        );
        // A shell can report a status without naming the command.
        assert_eq!(
            command_exit_line(SessionId(7), 1, None),
            "session 7: command exited with code 1"
        );
    }

    #[test]
    fn a_directory_line_shows_both_ends_of_a_move() {
        assert_eq!(
            cwd_change_line(SessionId(2), Some("/home/u"), "/srv/app"),
            "session 2: working directory /home/u -> /srv/app"
        );
        assert_eq!(
            cwd_change_line(SessionId(2), None, "/srv/app"),
            "session 2: working directory /srv/app"
        );
    }
}
