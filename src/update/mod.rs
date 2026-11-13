// Rust guideline compliant 2026-10-09

use iced::event::Event;
use iced::widget::operation::AbsoluteOffset;
use iced::widget::{Id, operation};
use iced::{Task, keyboard, window};
use iced_term::TerminalView;

use crate::app::keybindings::Action;
use crate::app::{ActiveTab, App, InitialAction, ScrollbarState, SessionId, Workspace};
use crate::core::data::{HostId, LogCategory, RightSidebarTab};
use crate::core::diagnostics::Level;
use crate::core::ids::SftpSessionId;
use crate::core::message::Message;
use crate::sftp::{SftpSession, SftpState};
use crate::ssh_connect::SshConnectionState;
use crate::terminal::{SidebarState, TabKind};
use rust_i18n::t;

mod animation;
mod global;
mod notifications;
mod settings;
mod sidebar;
#[path = "window_update.rs"]
mod window_update;
mod workspace;

/// Turn a storage write result into a UI task: a no-op on success, or an
/// error toast on failure.  Use this instead of discarding the result with
/// `let _ = ...` so a failed persistence is visible to the user.
pub(crate) fn toast_on_storage_error(result: Result<(), String>) -> Task<Message> {
    match result {
        Ok(()) => Task::none(),
        Err(err) => Task::done(Message::ShowErrorToast(err)),
    }
}

/// Generate the default name for a new workspace save by finding the highest
/// numeric suffix already in use and incrementing it. Existing names such as
/// "Workspace-2" or "工作区-5" are recognized; unrelated names are ignored.
fn default_workspace_name(app: &App) -> String {
    let next_number = app
        .workspace_manager
        .list
        .iter()
        .filter_map(|saved| {
            let suffix = saved.name.rsplit_once('-')?.1.trim();
            suffix.parse::<u64>().ok()
        })
        .max()
        .unwrap_or(0)
        + 1;
    t!("workspace.auto_name", number = next_number).to_string()
}

pub(crate) fn refresh_workspace_manager(app: &mut App) {
    app.workspace_manager.list = crate::workspace::list_workspaces();
}

/// Dedupe parsed export workspaces against the current list (and against each
/// other within the file), then show the import-confirm dialog. If nothing is
/// new, toast and close without showing the dialog.
fn present_workspace_import(
    app: &mut App,
    workspaces: Vec<crate::workspace::export::ExportWorkspace>,
) -> Task<Message> {
    let mut existing: std::collections::HashSet<String> = app
        .workspace_manager
        .list
        .iter()
        .map(|saved| saved.name.clone())
        .collect();

    let mut new_count = 0usize;
    let mut dup_count = 0usize;
    let mut pending = Vec::new();

    for ws in workspaces {
        if ws.name.trim().is_empty() {
            dup_count += 1;
            continue;
        }
        if !existing.insert(ws.name.clone()) {
            dup_count += 1;
            continue;
        }
        new_count += 1;
        pending.push(ws);
    }

    if new_count == 0 {
        let msg = t!("workspace.import_all_duplicates", n = dup_count).to_string();
        return Task::done(Message::ShowToast(msg));
    }

    app.workspace_manager.import_dialog = Some(crate::workspace::WorkspaceImportDialog {
        pending_workspaces: pending,
        new_count,
        dup_count,
    });
    Task::none()
}

fn workspace_transfer_file_dialog(title: String) -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .set_file_name(crate::workspace::export::WORKSPACE_EXPORT_FILE_NAME)
        .add_filter(
            crate::workspace::export::WORKSPACE_EXPORT_FILTER_NAME,
            &[crate::workspace::export::WORKSPACE_EXPORT_EXTENSION],
        )
}

fn begin_workspace_apply(
    app: &mut App,
    name: String,
    snapshot: crate::workspace::WorkspaceSnapshot,
) {
    app.show_apply_workspace_confirm = true;
    app.pending_apply_workspace = Some(crate::workspace::PendingWorkspaceApply { name, snapshot });
}

fn missing_ssh_host_count(
    snapshot: &crate::workspace::WorkspaceSnapshot,
    hosts: &[crate::core::data::Host],
) -> usize {
    let missing_hosts: std::collections::HashSet<HostId> = snapshot
        .sessions
        .iter()
        .filter_map(|session| match &session.kind {
            crate::workspace::SnapshotSessionKind::Ssh { host_id } => {
                let id = HostId(*host_id);
                (!hosts.iter().any(|host| host.id == id)).then_some(id)
            }
            crate::workspace::SnapshotSessionKind::Local { .. } => None,
        })
        .collect();
    missing_hosts.len()
}

/// Stop background port-forwarding processes and close the window.
fn close_app(app: &mut App) -> Task<Message> {
    crate::port_forward::update::stop_all(app);
    window::oldest().then(|id| {
        if let Some(id) = id {
            window::close(id)
        } else {
            Task::none()
        }
    })
}

/// Advance the terminal right-sidebar slide animation by one tick.
fn animate_right_sidebar(sidebar: &mut crate::terminal::SidebarState) {
    let step = crate::ui::helpers::SLIDE_PANEL_ANIM_STEP;

    if sidebar.closing {
        crate::ui::helpers::tick_slide_anim(&mut sidebar.anim, true, step);
        if sidebar.anim <= 0.0 {
            sidebar.show = false;
            sidebar.closing = false;
            sidebar.focus_list_pending = None;
        }
    } else if sidebar.show {
        crate::ui::helpers::tick_slide_anim(&mut sidebar.anim, false, step);
    }
}

/// Start opening a sidebar and request focus for its active list.
fn open_right_sidebar(sidebar: &mut SidebarState) {
    sidebar.show = true;
    sidebar.anim = 0.0;
    sidebar.closing = false;
    sidebar.focus_list_pending = Some(sidebar.active_tab);
}

/// Spawn the OS colour-scheme query on the blocking thread pool and deliver
/// the result as [`Message::SystemThemeDetected`].
fn system_theme_detect_task() -> Task<Message> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(crate::core::theme::detect_system_dark_mode)
                .await
                .unwrap_or(false)
        },
        Message::SystemThemeDetected,
    )
}

#[cfg(target_os = "macos")]
fn reposition_traffic_lights_now() -> Task<Message> {
    window::oldest().then(|id| {
        let Some(id) = id else {
            return Task::none();
        };

        window::run(id, |window| {
            crate::core::traffic_lights::sync(window);
        })
        .discard()
    })
}

/// Applies one app message and reconciles deferred focus restoration.
pub fn update(app: &mut App, msg: Message) -> Task<Message> {
    let update = update_inner(app, msg);
    Task::batch([update, crate::focus::sync(app)])
}

fn update_inner(app: &mut App, msg: Message) -> Task<Message> {
    // Anything over 16 ms (one frame at 60 fps) logs a warning; the guard spans
    // every return path out of this function.
    let _update_guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "update",
        std::time::Duration::from_millis(16),
    );
    // Tags log lines with the current frame id so timing guards correlate.
    let _frame_id = crate::core::diagnostics::next_frame_id();

    // Pings on every message rather than from AnimTick: that subscription is
    // conditional, so pinging only there would report false freezes whenever no
    // animation is running.
    crate::core::diagnostics::watchdog_ping();
    if let Some(gap) = crate::core::diagnostics::check_tick_gap() {
        // Formatting only happens on the rare gap, so the cost is negligible.
        crate::core::diagnostics::log(
            Level::Warn,
            "FREEZE",
            format!(
                "event-loop gap {:?} — main thread was blocked for at least {:.1}s",
                gap,
                gap.as_secs_f64(),
            ),
        );
    }

    app.needs_scroll_restore = false;
    let task = match msg {
        message @ (Message::CursorFocus(_)
        | Message::CursorMove { .. }
        | Message::CursorColumns(_, _)
        | Message::CursorActivate(_)
        | Message::ObserveFocus
        | Message::FocusedWidget(_)
        | Message::Scrolled(_, _)
        | Message::TopTabsWheel(_)
        | Message::StartupDataLoaded(_)
        | Message::ScrollbarVisibilityTimer) => global::update(app, message),

        Message::Hosts(inner) => crate::hosts::update::update(app, inner),

        Message::Sftp(inner) => crate::sftp::update::update(app, inner),

        Message::TermEvent(event) => {
            return handle_global_keyboard(app, &event);
        }
        Message::Terminal(inner) => crate::terminal::update::update(app, inner),

        Message::SshConnect(inner) => crate::ssh_connect::update::update(app, inner),

        Message::KnownHosts(inner) => crate::known_hosts::update::update(app, inner),

        Message::Keychain(inner) => crate::keychain::update::update(app, inner),

        Message::PortForward(inner) => crate::port_forward::update::update(app, inner),

        Message::Snippets(inner) => crate::snippets::update::update(app, inner),

        Message::AnimTick => animation::tick(app),

        message @ (Message::ShowToast(_)
        | Message::ShowWarningToast(_)
        | Message::ShowErrorToast(_)
        | Message::DismissToast(_)
        | Message::CommandFrequencyNotify { .. }
        | Message::CommandFrequencyDismiss(_)
        | Message::CommandFrequencyAddToSnippet { .. }) => notifications::update(app, message),

        message @ (Message::WindowClose
        | Message::WindowCloseConfirm
        | Message::WindowCloseCancel
        | Message::WindowMinimize
        | Message::WindowMaximize
        | Message::WindowToggleFullscreen
        | Message::FullscreenChanged(_)
        | Message::WindowDrag
        | Message::CursorPositionChanged(_)
        | Message::WindowPositionChanged(_)
        | Message::WindowScaleFactorChanged(_)
        | Message::WindowResized(_)) => window_update::update(app, &message),
        message @ (Message::WorkspaceSave
        | Message::WorkspaceSaveName(_)
        | Message::WorkspaceSaveConfirm
        | Message::WorkspaceSaveOverwrite
        | Message::WorkspaceSaveKeepDuplicate
        | Message::WorkspaceSaveConflictCancel
        | Message::WorkspaceSaveCancel
        | Message::WorkspaceApply(_)
        | Message::WorkspaceApplyConfirm
        | Message::WorkspaceApplyCancel
        | Message::WorkspaceRenameStart(_)
        | Message::WorkspaceRenameInput(_)
        | Message::WorkspaceRenameSubmit
        | Message::WorkspaceRenameCancel
        | Message::WorkspaceDeleteRequest(_)
        | Message::WorkspaceDeleteConfirm
        | Message::WorkspaceDeleteCancel
        | Message::WorkspaceSelect(_)
        | Message::WorkspaceHoverEnter(_)
        | Message::WorkspaceHoverLeave(_)
        | Message::WorkspaceTransferMenuToggle
        | Message::WorkspaceExportRequest
        | Message::WorkspaceExportSaved(_)
        | Message::WorkspaceImportRequest
        | Message::WorkspaceImportPicked(_)
        | Message::WorkspaceImportConfirm
        | Message::WorkspaceImportCancel
        | Message::WorkspaceRecovery(_)) => workspace::update(app, message),

        message @ (Message::CrashDialogClose
        | Message::CrashDialogCopy
        | Message::CopyLogs
        | Message::Noop) => global::update(app, message),
        message @ (Message::ToggleRightSidebar
        | Message::SwitchRightTab(_)
        | Message::FocusRightSidebarTab(_)
        | Message::RightSidebarSelectSnippet(_)
        | Message::RightSidebarSelectCommand(_)
        | Message::CommandHistoryCopy(_)
        | Message::CommandHistoryRun(_)
        | Message::CommandHistoryInfo(_)
        | Message::CommandHistoryInfoClose
        | Message::CommandHistoryFilter(_)
        | Message::CommandHistoryHoverEnter(_)
        | Message::CommandHistoryHoverLeave(_)
        | Message::SpotlightShow(_)
        | Message::SpotlightCancel(_)
        | Message::SpotlightFilter(_, _)
        | Message::SpotlightMoveUp(_)
        | Message::SpotlightMoveDown(_)
        | Message::SpotlightPick(_, _)
        | Message::SpotlightHover(_, _)
        | Message::SpotlightConfirm(_)) => sidebar::update(app, message),

        message @ (Message::SetTheme(_)
        | Message::ThemePreferenceSyncCheck
        | Message::ThemePreferenceSyncLoaded(_)
        | Message::SystemThemeCheck
        | Message::SystemThemeDetected(_)
        | Message::SetTerminalFontSize(_)
        | Message::SetCommandHistoryLimit(_)
        | Message::SettingsToggleCommandHistoryLimitPicker
        | Message::SettingsToggleThemePicker
        | Message::SetShellIntegration(_)
        | Message::SettingsToggleShellIntegrationPicker
        | Message::SetLocalShell(_)
        | Message::SettingsToggleLocalShellPicker
        | Message::SetDiagnosticsLevel(_)
        | Message::SettingsToggleDiagnosticsPicker
        | Message::OpenBugReport
        | Message::SetLocale(_)
        | Message::SettingsToggleLocalePicker) => settings::update(app, message),

        #[cfg(target_os = "windows")]
        message @ (Message::SyncWindowChrome | Message::RevealWindowFirstFrame) => {
            window_update::update(app, &message)
        }
    };
    if app.needs_scroll_restore {
        task.chain(restore_scrolls(app))
    } else {
        task
    }
}

pub(crate) fn restore_scrolls(app: &App) -> Task<Message> {
    let mut task = Task::none();
    for (&id, &y) in &app.scroll_offsets {
        if y > 0.0 {
            task = task.chain(operation::scroll_to(
                Id::from(id),
                AbsoluteOffset {
                    x: Some(0.0),
                    y: Some(y),
                },
            ));
        }
    }
    task
}

/// Compute the total pixel width of all visible tabs in the scrollable tab
/// strip (including inter-tab spacing).  Uses target widths so the result is
/// correct even before the next view re-render — callers don't depend on stale
/// viewport cache.
fn compute_tab_strip_content_width(app: &App) -> f32 {
    let mut width: f32 = 0.0;
    let mut count: usize = 0;

    for tid in app.visible_tab_ids() {
        let is_active = app.active_tab == ActiveTab::Tab(tid);

        let target_width = if is_active {
            (crate::views::TAB_MIN_WIDTH + 40.0).min(crate::views::MAX_TAB_WIDTH)
        } else {
            crate::views::TAB_MIN_WIDTH
        };
        width += target_width;
        count += 1;
    }

    if count > 0 {
        width += (count as f32 - 1.0) * 6.0;
    }
    width
}

/// Calculate the pixel offset of the active terminal/new-tab tab's left edge
/// within the topbar scrollable content area.  Returns `None` when no
/// scrollable tab is active (e.g. Vaults or SFTP is selected).
fn compute_active_tab_left(app: &App) -> Option<f32> {
    let active_tid = match app.active_tab {
        ActiveTab::Tab(tid) => tid,
        ActiveTab::Workspace(_) => return None,
    };

    let mut offset: f32 = 0.0;

    for tid in app.visible_tab_ids() {
        if tid == active_tid {
            return Some(offset);
        }

        offset += crate::views::TAB_MIN_WIDTH + 6.0; // 6.0 = row spacing
    }

    None
}

/// Generate a `scroll_to` task that brings the active topbar tab fully into
/// the visible area.  When the viewport width is unknown (the strip has never
/// overflowed), arms `top_tabs_pending_scroll` so subsequent Scrolled events
/// retry; the flag is cleared once the active tab is visible / not scrollable.
pub(crate) fn scroll_to_active_tab(app: &mut App) -> Task<Message> {
    let Some(tab_left) = compute_active_tab_left(app) else {
        // Vaults/SFTP tabs are not in the strip.
        app.top_tabs_pending_scroll = false;
        return Task::none();
    };

    let viewport_w = app.top_tabs_viewport_width;
    if viewport_w <= 0.0 {
        // No Scrolled event has fired yet, so the viewport width is unknown.
        // Defer until one arrives and re-attempts the scroll.
        app.top_tabs_pending_scroll = true;
        return Task::none();
    }

    // The cached viewport value lags a frame behind, so measure the strip.
    let content_w = compute_tab_strip_content_width(app);
    if content_w <= viewport_w + 1.0 {
        app.top_tabs_pending_scroll = false;
        return Task::none();
    }

    // Target width, not the animated one: `compute_tab_strip_content_width`
    // measures target widths too.
    let tab_width = (crate::views::TAB_MIN_WIDTH + 40.0).min(crate::views::MAX_TAB_WIDTH);

    let max_x = (content_w - viewport_w).max(0.0);
    let tab_right = tab_left + tab_width;

    // The absolute offset stays correct when the content has grown since the
    // last Scrolled event, unlike the relative `top_tabs_scroll_offset`.
    let current_left = app.top_tabs_scroll_absolute_x;
    let current_right = current_left + viewport_w;

    let target_x = if tab_right > current_right + 1.0 {
        // Align the tab's right edge with the viewport's, plus a small margin.
        tab_right - viewport_w + 12.0
    } else if tab_left < current_left - 1.0 {
        // Align the tab's left edge with the viewport's.
        tab_left - 12.0
    } else {
        app.top_tabs_pending_scroll = false;
        return Task::none();
    };

    // Stay armed: the resulting Scrolled retries with a fresh viewport (which
    // a pinned "+" narrows) until the tab is visible.
    let clamped_x = target_x.max(0.0).min(max_x);

    operation::scroll_to(
        Id::from(crate::views::topbar::TOP_TABS_SCROLL_ID),
        AbsoluteOffset {
            x: Some(clamped_x),
            y: None,
        },
    )
}

/// Stable id used to focus the search input at the top of the spotlight
/// command palette.
pub(crate) const SPOTLIGHT_INPUT_ID: &str = "__termlet_spotlight_input__";

/// Handle the built-in "Open in SFTP" spotlight action.
///
/// Closes the spotlight, creates a **new** SFTP tab, and if the terminal
/// session is connected to a host, auto-connects SFTP to that host at
/// the terminal's current working directory (reported by the shell via OSC 7).
fn open_sftp_from_spotlight(app: &mut App, session_id: SessionId) -> Task<Message> {
    app.add_log(
        LogCategory::Terminal,
        format!("OpenInSftp from spotlight: session_id={session_id}"),
    );

    if let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
    {
        session.spotlight = None;
    }

    // OSC 7 cache file first, then the session's cached working directory. The
    // latter is only trustworthy when reported at the current prompt: btop, vim,
    // ssh or a nested shell can change directory without telling the outer shell.
    let cwd_info = app
        .terminal
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .map_or((None, false), |s| {
            (
                extract_cwd_from_terminal(s.host_id, s.working_directory.as_deref()),
                s.working_directory_synced,
            )
        });
    let (remote_cwd, cwd_synced) = cwd_info;

    // Alert rather than silently opening an old or guessed path.
    if !cwd_synced || remote_cwd.is_none() {
        app.add_log(
            LogCategory::Terminal,
            format!(
                "OpenInSftp blocked: session_id={session_id}, cwd_synced={cwd_synced}, cwd={:?}",
                remote_cwd.as_deref()
            ),
        );

        let focus = app
            .terminal
            .sessions
            .iter()
            .find(|s| s.id == session_id)
            .map_or_else(Task::none, |s| {
                TerminalView::focus(s.terminal.widget_id().clone())
            });
        let cwd_display = remote_cwd.as_deref().unwrap_or("?");
        let error = Task::done(Message::ShowErrorToast(
            t!(
                "terminal.spotlight_open_sftp_cwd_unsynced",
                path = cwd_display
            )
            .to_string(),
        ));
        return Task::batch([focus, error]);
    }

    let host_id = app
        .terminal
        .sessions
        .iter()
        .find(|s| s.id == session_id)
        .and_then(|s| s.host_id);

    // A new session, so an existing tab is never overwritten.
    let new_id = SftpSessionId(app.sftp.next_session_id as u32);
    app.sftp.next_session_id += 1;
    let label = match (host_id, remote_cwd.as_ref()) {
        (Some(hid), _) => app
            .hosts
            .list
            .iter()
            .find(|h| h.id == hid)
            .map_or_else(|| format!("Session {new_id}"), |h| h.name.clone()),
        (None, Some(cwd)) => {
            // Expand a leading `~` so the label is the real directory name.
            let cwd = crate::core::paths::expand_home(cwd);
            std::path::Path::new(&cwd)
                .file_name()
                .map_or_else(|| cwd.clone(), |n| n.to_string_lossy().to_string())
        }
        _ => format!("Session {new_id}"),
    };
    app.sftp.sessions.push(SftpSession {
        id: new_id,
        label,
        state: SftpState::default(),
    });
    app.load_sftp_state(new_id);

    // Switching to the SFTP workspace also refreshes the local listing.
    let mut tasks: Vec<Task<Message>> = Vec::new();
    tasks.push(Task::done(Message::Hosts(
        crate::hosts::message::Message::SelectWorkspace(Workspace::Sftp),
    )));

    if let Some(session) = app.terminal.sessions.iter().find(|s| s.id == session_id) {
        let wid = session.terminal.widget_id().clone();
        tasks.push(TerminalView::focus(wid));
    }

    match (host_id, remote_cwd) {
        (Some(hid), Some(cwd)) => {
            let host = app.hosts.list.iter().find(|h| h.id == hid).cloned();
            if let Some(host) = host {
                let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
                app.add_log(
                    LogCategory::Sftp,
                    format!("SFTP connected to {target} at {cwd} (right pane)"),
                );

                app.sftp.state.right.pane.connected = true;
                app.sftp.state.right.pane.host_id = Some(hid);
                let mut attempt = crate::ssh_connect::ConnectionAttempt::new(host.clone(), None);
                attempt.state = SshConnectionState::Connecting;
                app.sftp.state.right.pane.attempt = attempt;
                app.sftp.state.right.pane.path.clone_from(&cwd);
                app.sftp.state.right.pane.history = vec![cwd.clone()];
                app.sftp.state.right.pane.history_idx = 0;
                app.sftp.state.right.pane.entries.clear();
                app.sftp.state.right.pane.loading = true;
                app.sftp.state.right.pane.error = None;
                app.sftp.state.right.host_picker.open = false;
                app.sftp.state.right.pane.filter.clear();
                app.sftp.state.right.pane.path_edit = None;
                app.sftp.state.right.pane.selected.clear();
                app.sftp.state.right.pane.last_clicked = None;
                app.sftp.state.right.pane.focused.clear();
                if let Some(session) = app
                    .sftp
                    .sessions
                    .iter_mut()
                    .find(|s| s.id == app.sftp.active_session)
                {
                    session.label.clone_from(&host.name);
                }
                tasks.push(crate::sftp::update::list_remote_pane(
                    app,
                    host,
                    crate::sftp::SftpPane::Remote,
                ));
            }
        }
        (Some(hid), _) => {
            // Unreachable now that a missing or unverified CWD is rejected
            // above; kept as a safe fall back to the host's home directory.
            tasks.push(Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpConnectHost(hid),
            )));
        }
        (None, Some(cwd)) => {
            tasks.push(Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpLocalNavigate(cwd),
            )));
        }
        _ => {
            tasks.push(Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpRefreshLocal,
            )));
        }
    }

    Task::batch(tasks)
}

/// Try to extract the current working directory from the terminal session.
///
/// Priority order:
/// 1. `cached_working_directory` — the in‑memory `TerminalSession::working_directory`
///    field, populated by OSC 7 `WorkingDirectory` events.
/// 2. OSC 7 cache file — for SSH sessions the bridge writes the remote CWD to a
///    per‑host file; we read it on demand.
fn extract_cwd_from_terminal(
    host_id: Option<HostId>,
    cached_working_directory: Option<&str>,
) -> Option<String> {
    // Priority 1: in‑memory cached working directory.
    if let Some(cwd) = cached_working_directory
        && !cwd.is_empty()
    {
        return Some(cwd.to_string());
    }

    // Priority 2: OSC 7 cache file written by the SSH bridge.
    if let Some(hid) = host_id {
        let cache_file = crate::core::paths::cwd_cache_file(hid);
        if cache_file.exists()
            && let Ok(cwd) = std::fs::read_to_string(&cache_file)
        {
            let cwd = cwd.trim().to_string();
            if !cwd.is_empty() {
                return Some(cwd);
            }
        }
    }

    None
}

/// Apply a sidebar mutation to every pane that shares the current sidebar.
fn update_active_sidebars(app: &mut App, mut update: impl FnMut(&mut SidebarState)) {
    if app.is_active_session_in_split() {
        for sid in app.active_split_pane_sessions() {
            if let Some(session) = app.terminal.sessions.iter_mut().find(|s| s.id == sid) {
                update(&mut session.sidebar);
            }
        }
    } else if let Some(sidebar) = app.sidebar_mut() {
        update(sidebar);
    }
}

/// Defer focus until the newly rendered list has entered the widget tree.
fn request_right_sidebar_focus(tab: RightSidebarTab) -> Task<Message> {
    Task::done(Message::FocusRightSidebarTab(tab))
}

/// Returns a pending list focus and clears it when the sidebar settles.
fn take_pending_right_sidebar_focus(sidebar: &mut SidebarState) -> Option<RightSidebarTab> {
    if !sidebar.show || sidebar.closing {
        return None;
    }

    let tab = sidebar.focus_list_pending?;
    if sidebar.anim >= 1.0 {
        sidebar.focus_list_pending = None;
    }
    Some(tab)
}

/// Record which command-history row the mouse is hovering over. The sidebar is
/// shared across every pane in a split, so the state is mirrored to all of
/// them, matching [`Message::CommandHistoryFilter`].
fn set_command_history_hover(app: &mut App, hovered: Option<usize>) {
    update_active_sidebars(app, |sidebar| sidebar.commands_hovered = hovered);
}

/// Open the right-sidebar snippet add form pre-filled with `command`, and make
/// sure the active terminal's sidebar is visible and switched to the Snippets
/// tab. Used by the "used N times" command prompt.
fn open_snippet_form_for_command(app: &mut App, command: &str) {
    app.snippets.right_show_form = true;
    app.snippets.right_editing_id = None;
    // Prefilled with the command, so the user can save without typing a name.
    app.snippets.right_description = command.to_string();
    app.snippets.right_script = iced::widget::text_editor::Content::with_text(command);
    app.snippets.right_package_id = None;
    crate::cursor::close_dropdown(
        app,
        crate::core::dropdown::DropdownTarget::SnippetSidebarPackage,
    );

    if app.is_active_session_in_split() {
        for sid in &app.active_split_pane_sessions() {
            if let Some(s) = app.terminal.sessions.iter_mut().find(|s| s.id == *sid) {
                s.sidebar.show = true;
                s.sidebar.anim = 0.0;
                s.sidebar.closing = false;
                s.sidebar.active_tab = crate::core::data::RightSidebarTab::Snippets;
            }
        }
    } else if let Some(sidebar) = app.sidebar_mut() {
        sidebar.show = true;
        sidebar.anim = 0.0;
        sidebar.closing = false;
        sidebar.active_tab = crate::core::data::RightSidebarTab::Snippets;
    }
}

/// Execute a palette command after it is picked/confirmed.
///
/// `Open in SFTP` delegates to its existing handler; every other command closes
/// the palette, re-focuses the terminal, and dispatches its own message.
fn execute_spotlight_command(
    app: &mut App,
    session_id: SessionId,
    kind: &crate::app::command::PaletteCommandKind,
) -> Task<Message> {
    use crate::app::command::PaletteCommandKind as K;

    if let K::OpenInSftp = kind {
        return open_sftp_from_spotlight(app, session_id);
    }

    let close = close_spotlight(app, session_id);

    let command: Task<Message> = match kind {
        K::NewTab => Task::done(Message::Hosts(crate::hosts::message::Message::OpenNewTab)),
        K::OpenLocalTerminal => crate::terminal::update::create_local_session(app),
        K::CloseTab => close_current_tab(app),
        K::OpenInSftp => unreachable!("handled above"),
    };

    Task::batch(vec![close, command])
}

/// Close the spotlight for `session_id` and return focus to its terminal.
fn close_spotlight(app: &mut App, session_id: SessionId) -> Task<Message> {
    if let Some(session) = app
        .terminal
        .sessions
        .iter_mut()
        .find(|s| s.id == session_id)
    {
        session.spotlight = None;
        return Task::none();
    }
    Task::none()
}

/// Moves the spotlight cursor through the visible command IDs.
fn spotlight_move_selection(popup: &mut crate::terminal::SpotlightState, forward: bool) {
    let command_ids = popup.command_ids();
    if forward {
        popup.cursor.move_next_wrapped(&command_ids);
    } else {
        popup.cursor.move_previous_wrapped(&command_ids);
    }
}

fn close_current_tab(app: &mut App) -> Task<Message> {
    if let ActiveTab::Tab(tid) = app.active_tab {
        match app.terminal.tab_registry.get(&tid).map(|t| t.kind) {
            Some(TabKind::Session(session_id)) => Task::done(Message::Terminal(
                crate::terminal::message::Message::TermClose(session_id),
            )),
            Some(TabKind::Split(split_id)) => {
                let focused = app.terminal.splits.get(&split_id).and_then(|s| s.focus);
                match focused {
                    Some(pane) => Task::done(Message::Terminal(
                        crate::terminal::message::Message::SplitClose(split_id, pane),
                    )),
                    None => Task::none(),
                }
            }
            Some(TabKind::Draft(id)) => Task::done(Message::Hosts(
                crate::hosts::message::Message::CloseNewTab(id),
            )),
            None => Task::none(),
        }
    } else {
        Task::none()
    }
}

fn handle_global_keyboard(app: &mut App, event: &Event) -> Task<Message> {
    // Feeds SFTP multi-select (Shift+Click, Ctrl+Click).
    if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = &event {
        app.sftp.state.keyboard_modifiers = *modifiers;
        return Task::none();
    }

    if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = &event {
        app.sftp.state.keyboard_modifiers = *modifiers;
        let (fullscreen_key, fullscreen_modifiers) = crate::app::keybindings::fullscreen_shortcut();
        if key == &fullscreen_key && *modifiers == fullscreen_modifiers {
            return Task::done(Message::WindowToggleFullscreen);
        }
        match crate::focus::handle_key(app, key, *modifiers) {
            crate::focus::KeyRoute::Handle(task) => return task,
            crate::focus::KeyRoute::PassThrough => {}
        }
        let is_f1 = matches!(key, keyboard::Key::Named(keyboard::key::Named::F1));
        let no_mods = modifiers.is_empty();

        // A full-screen application (vim, nano, htop, …) runs on the alternate
        // screen buffer, where keys must reach it unmodified: the local
        // shortcuts below are suppressed in that mode.
        let is_alt_screen = app
            .active_terminal_session_id()
            .and_then(|session_id| {
                app.terminal
                    .sessions
                    .iter()
                    .find(|s| s.id == session_id)
                    .map(|s| s.terminal.is_alt_screen())
            })
            .unwrap_or(false);

        // F1 opens the skill spotlight, except in alt-screen mode where TUI
        // applications (vim help, htop help) claim it.
        if is_f1 && no_mods {
            if is_alt_screen {
                // The widget bindings Block F1 in a full-screen app, so
                // forward it as ESC OP; this subscription is its only writer.
                if let Some(session_id) = app.active_terminal_session_id() {
                    return Task::done(Message::Terminal(
                        crate::terminal::message::Message::IcedTermEvent(
                            iced_term::Event::BackendCall(
                                u64::from(session_id.0),
                                iced_term::BackendCommand::Write(b"\x1bOP".to_vec()),
                            ),
                        ),
                    ));
                }
            } else if let Some(session_id) = app.active_terminal_session_id() {
                let should_open = app
                    .terminal
                    .sessions
                    .iter()
                    .find(|s| s.id == session_id)
                    .is_some_and(|s| s.spotlight.is_none());
                if should_open {
                    return Task::done(Message::SpotlightShow(session_id));
                }
            }
        }

        if !is_alt_screen && let Some(action) = app.keybindings.resolve(key, *modifiers).cloned() {
            if action == Action::ToggleFullscreen && *modifiers != fullscreen_modifiers {
                return Task::none();
            }
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "keybinding matched: action={action:?} key={key:?} modifiers={modifiers:?}"
                ),
            );
            return match action {
                Action::OpenLocalTerminal => crate::terminal::update::create_local_session(app),
                Action::ToggleSearch => {
                    // Already resolves the focused pane when the tab is a split.
                    let target_id = app.active_terminal_session_id();

                    if let Some(sid) = target_id {
                        // Route through the ShowSearch / SearchClose messages so
                        // the slide-down / slide-up animation plays on both the
                        // keybinding and the close button.
                        let show = app
                            .terminal
                            .sessions
                            .iter()
                            .find(|s| s.id == sid)
                            .is_some_and(|s| s.show_search);
                        return if show {
                            Task::done(Message::Terminal(
                                crate::terminal::message::Message::SearchClose(sid),
                            ))
                        } else {
                            Task::done(Message::Terminal(
                                crate::terminal::message::Message::ShowSearch(sid),
                            ))
                        };
                    }
                    Task::none()
                }
                Action::CloseCurrentTab => {
                    if let ActiveTab::Tab(tid) = app.active_tab {
                        match app.terminal.tab_registry.get(&tid).map(|t| t.kind) {
                            Some(TabKind::Session(session_id)) => {
                                // Route through TermClose so a locked tab prompts
                                // for confirmation instead of closing immediately.
                                Task::done(Message::Terminal(
                                    crate::terminal::message::Message::TermClose(session_id),
                                ))
                            }
                            Some(TabKind::Split(split_id)) => {
                                let focused =
                                    app.terminal.splits.get(&split_id).and_then(|s| s.focus);
                                match focused {
                                    Some(pane) => Task::done(Message::Terminal(
                                        crate::terminal::message::Message::SplitClose(
                                            split_id, pane,
                                        ),
                                    )),
                                    None => Task::none(),
                                }
                            }
                            Some(TabKind::Draft(id)) => Task::done(Message::Hosts(
                                crate::hosts::message::Message::CloseNewTab(id),
                            )),
                            None => Task::none(),
                        }
                    } else {
                        if !app.has_any_dialog_open() {
                            app.show_quit_confirm = true;
                        }
                        Task::none()
                    }
                }
                Action::NewTerminalTab => {
                    Task::done(Message::Hosts(crate::hosts::message::Message::OpenNewTab))
                }
                Action::DuplicateSession => {
                    if let Some(sid) = app.active_terminal_session_id() {
                        Task::done(Message::Terminal(
                            crate::terminal::message::Message::DuplicateSession(sid),
                        ))
                    } else {
                        Task::none()
                    }
                }
                Action::ToggleFullscreen => Task::done(Message::WindowToggleFullscreen),
                Action::IncreaseFontSize => Task::done(Message::SetTerminalFontSize(
                    app.preferences.terminal_font_size + 1.0,
                )),
                Action::DecreaseFontSize => Task::done(Message::SetTerminalFontSize(
                    app.preferences.terminal_font_size - 1.0,
                )),
            };
        } // end is_alt_screen guard
    }

    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::DraftId;
    use crate::core::cursor_focus::CursorTarget;
    use crate::core::data::NavItem;
    use crate::core::forms::FormDraft;
    use crate::core::grid_cursor::GridDirection;
    use crate::core::theme::ThemeMode;
    use crate::cursor::{
        activate as activate_cursor, activation as cursor_activation, scroll_right_sidebar_row,
    };
    use crate::sftp::SftpPane;
    use crate::snippets::SnippetSelection;
    use crate::workspace::{SnapshotSession, SnapshotSessionKind, WorkspaceSnapshot};

    fn snapshot_with(sessions: Vec<SnapshotSession>) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            version: 1,
            tabs: Vec::new(),
            active_tab: None,
            sessions,
            splits: Vec::new(),
        }
    }

    fn ssh_session(host_id: u32) -> SnapshotSession {
        SnapshotSession {
            kind: SnapshotSessionKind::Ssh { host_id },
            label_override: None,
        }
    }

    fn local_session() -> SnapshotSession {
        SnapshotSession {
            kind: SnapshotSessionKind::Local { start_dir: None },
            label_override: None,
        }
    }

    fn saved_host(id: u32) -> crate::core::data::Host {
        crate::core::data::Host {
            id: HostId(id),
            name: "web".into(),
            hostname: "example.com".into(),
            port: 22,
            protocol: crate::core::data::Protocol::Ssh,
            username: "root".into(),
            auth_type: crate::core::data::AuthType::Password,
            jump_host_id: None,
            identity_id: None,
            key_path: None,
            key_passphrase: None,
            password: None,
            os_type: crate::core::data::OsType::Unknown,
            tags: Vec::new(),
            expected_fingerprint: None,
        }
    }

    fn synced_focus(app: &mut App) -> Option<crate::focus::FocusId> {
        let _ = crate::focus::sync(app);
        app.focus.top()
    }

    #[test]
    fn restoring_a_workspace_counts_the_hosts_that_are_no_longer_saved() {
        let hosts = vec![saved_host(1), saved_host(3)];

        // Nothing missing: every SSH session points at a saved host, and local
        // sessions need no host at all.
        let present = snapshot_with(vec![ssh_session(1), ssh_session(3), local_session()]);
        assert_eq!(missing_ssh_host_count(&present, &hosts), 0);

        // Two missing hosts…
        let missing = snapshot_with(vec![ssh_session(1), ssh_session(2), ssh_session(4)]);
        assert_eq!(missing_ssh_host_count(&missing, &hosts), 2);

        // …counted once each, however many panes referenced them, so the
        // confirmation dialog reports hosts rather than panes.
        let repeated = snapshot_with(vec![
            ssh_session(2),
            ssh_session(2),
            ssh_session(2),
            local_session(),
        ]);
        assert_eq!(missing_ssh_host_count(&repeated, &hosts), 1);

        assert_eq!(
            missing_ssh_host_count(&snapshot_with(Vec::new()), &hosts),
            0
        );
        assert_eq!(
            missing_ssh_host_count(&snapshot_with(vec![ssh_session(1)]), &[]),
            1,
            "an empty vault makes every SSH session unrestorable"
        );
    }

    /// A palette popup with `count` commands in it.
    fn spotlight(count: usize) -> crate::terminal::SpotlightState {
        let mut state = crate::terminal::SpotlightState::new(
            crate::app::command::build_palette_commands(Some(HostId(1)))
                .into_iter()
                .take(count)
                .collect(),
        );
        state.anim = 1.0;
        state
    }

    #[test]
    fn the_spotlight_selection_wraps_at_both_ends() {
        let mut popup = spotlight(4);

        spotlight_move_selection(&mut popup, true);
        assert_eq!(popup.selected_index(), Some(1));
        spotlight_move_selection(&mut popup, true);
        assert_eq!(popup.selected_index(), Some(2));

        // Arrow-down on the last item wraps to the first rather than sticking.
        let last_id = popup.filtered_commands[3].id.clone();
        popup.cursor.select(last_id);
        spotlight_move_selection(&mut popup, true);
        assert_eq!(popup.selected_index(), Some(0));

        // Arrow-up on the first item wraps to the last.
        spotlight_move_selection(&mut popup, false);
        assert_eq!(popup.selected_index(), Some(3));
    }

    #[test]
    fn moving_the_selection_in_an_empty_spotlight_is_harmless() {
        // A filter that matches nothing must not leave the index pointing past
        // the (empty) list, which would panic on the next render.
        let mut empty = spotlight(0);
        spotlight_move_selection(&mut empty, true);
        assert_eq!(empty.selected_index(), None);
        spotlight_move_selection(&mut empty, false);
        assert_eq!(empty.selected_index(), None);

        let mut single = spotlight(1);
        spotlight_move_selection(&mut single, true);
        assert_eq!(single.selected_index(), Some(0));
        spotlight_move_selection(&mut single, false);
        assert_eq!(single.selected_index(), Some(0));
    }

    #[test]
    fn spotlight_selection_follows_its_command_when_results_reorder() {
        let mut popup = spotlight(4);
        let selected = popup.filtered_commands[1].id.clone();
        popup.cursor.select(selected.clone());
        popup.filtered_commands.reverse();

        assert_eq!(
            popup.selected_command().map(|command| command.id.as_str()),
            Some(selected.as_str())
        );
        assert_eq!(popup.selected_index(), Some(2));
    }

    #[test]
    fn resetting_a_filtered_spotlight_focuses_its_top_match() {
        let mut popup = spotlight(4);
        let last = popup.filtered_commands[3].id.clone();
        popup.cursor.select(last);

        popup.reset_cursor();

        assert_eq!(popup.selected_index(), Some(0));
    }

    #[test]
    fn a_cached_working_directory_wins_over_reading_it_back_from_disk() {
        assert_eq!(
            extract_cwd_from_terminal(None, Some("/srv/app")),
            Some("/srv/app".to_string())
        );

        // An empty cache is no cache: the lookup falls through to the host's
        // OSC 7 file (and here, to nothing at all).
        assert_eq!(extract_cwd_from_terminal(None, Some("")), None);
        assert_eq!(extract_cwd_from_terminal(None, None), None);

        // A host whose cache file was never written yields nothing rather than
        // a guessed path.
        assert_eq!(
            extract_cwd_from_terminal(Some(HostId(u32::MAX)), None),
            None
        );
    }

    // App-level message flows

    use crate::core::locale::Locale;
    use crate::core::test_support::app;
    use iced::Point;

    #[test]
    fn a_toast_is_shown_with_its_own_kind_and_is_logged() {
        for (message, expected) in [
            (
                Message::ShowToast("saved".into()),
                crate::app::ToastKind::Info,
            ),
            (
                Message::ShowWarningToast("exposed port".into()),
                crate::app::ToastKind::Warning,
            ),
            (
                Message::ShowErrorToast("failed".into()),
                crate::app::ToastKind::Error,
            ),
        ] {
            let mut app = app();
            let _ = update(&mut app, message);

            let toast = app.toast.as_ref().expect("a toast is on screen");
            assert_eq!(toast.kind, expected);
            assert_eq!(app.toast_anim, 0.0, "the toast slides in");
            assert!(!app.toast_closing);
            assert!(
                app.logs.iter().any(|log| log.message.contains("[Toast]")),
                "toasts leave a trace in the in-app log"
            );
        }
    }

    #[test]
    fn each_toast_carries_a_fresh_id_so_a_stale_timer_cannot_close_it() {
        let mut app = app();
        let _ = update(&mut app, Message::ShowToast("first".into()));
        let first = app.toast.as_ref().unwrap().id;

        let _ = update(&mut app, Message::ShowToast("second".into()));
        let second = app.toast.as_ref().unwrap().id;
        assert_ne!(first, second, "the app must not reuse toast ids");

        // The first toast's auto-dismiss timer fires late; it must not close
        // the toast that replaced it.
        let _ = update(&mut app, Message::DismissToast(first));
        assert!(!app.toast_closing, "a stale timer dismissed the new toast");
        assert_eq!(app.toast.as_ref().unwrap().message, "second");

        let _ = update(&mut app, Message::DismissToast(second));
        assert!(app.toast_closing);
    }

    #[test]
    fn the_command_frequency_prompt_ignores_a_stale_dismiss_timer() {
        let mut app = app();
        let _ = update(
            &mut app,
            Message::CommandFrequencyNotify {
                command: "git status".into(),
                count: 5,
            },
        );
        let prompt = app.command_frequency_prompt.as_ref().expect("a prompt");
        assert_eq!(prompt.command, "git status");
        assert_eq!(prompt.count, 5);
        let seq = app.command_frequency_prompt_seq;

        let _ = update(&mut app, Message::CommandFrequencyDismiss(seq));
        assert!(app.command_frequency_prompt_closing);

        // A second prompt gets a new generation; the first one's timer is spent.
        let _ = update(
            &mut app,
            Message::CommandFrequencyNotify {
                command: "make".into(),
                count: 3,
            },
        );
        let _ = update(&mut app, Message::CommandFrequencyDismiss(seq));
        assert!(
            !app.command_frequency_prompt_closing,
            "the old timer closed the newer prompt"
        );
    }

    #[test]
    fn adding_a_frequent_command_to_snippets_prefills_the_editor() {
        let mut app = app();
        let _ = update(
            &mut app,
            Message::CommandFrequencyNotify {
                command: "git status".into(),
                count: 9,
            },
        );

        let _ = update(
            &mut app,
            Message::CommandFrequencyAddToSnippet {
                command: "git status".into(),
            },
        );

        assert!(
            app.command_frequency_prompt_closing,
            "the prompt steps aside"
        );
        assert!(app.snippets.right_show_form, "the sidebar form opens");
        assert_eq!(app.snippets.right_description, "git status");
        assert_eq!(app.snippets.right_script.text(), "git status");
        assert_eq!(app.snippets.right_package_id, None);
    }

    #[test]
    fn choosing_a_theme_closes_the_picker_and_stores_the_choice() {
        let mut app = app();
        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::SettingsTheme);
        app.preferences.system_theme_detecting = true;

        let _ = update(&mut app, Message::SetTheme(ThemeMode::Novel));

        assert_eq!(app.preferences.theme_mode, ThemeMode::Novel);
        assert!(
            app.active_dropdown.is_none(),
            "the dropdown closes behind the choice"
        );
    }

    #[test]
    fn switching_to_auto_schedules_the_system_theme_lookup_once() {
        let mut app = app();
        app.preferences.system_theme_detecting = false;

        let _ = update(&mut app, Message::SetTheme(ThemeMode::Auto));
        assert!(
            app.preferences.system_theme_detecting,
            "the OS preference has to be re-read when Auto is chosen"
        );

        // A second switch while that lookup is still in flight must not queue
        // another subprocess.
        let _ = update(&mut app, Message::SetTheme(ThemeMode::Auto));
        assert!(app.preferences.system_theme_detecting);
    }

    #[test]
    fn a_detected_system_theme_lands_in_the_preferences() {
        let mut app = app();
        app.preferences.system_theme_detecting = true;

        let _ = update(&mut app, Message::SystemThemeDetected(true));

        assert!(app.preferences.system_dark_mode);
        assert!(
            !app.preferences.system_theme_detecting,
            "the lookup is finished"
        );
    }

    #[test]
    fn the_other_settings_are_applied_and_their_pickers_closed() {
        let mut app = app();
        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::SettingsLocale);

        let _ = update(&mut app, Message::SetLocale(Locale::ZhCN));
        assert_eq!(app.preferences.locale, Locale::ZhCN);
        assert_eq!(app.active_dropdown, None);

        let _ = update(&mut app, Message::SetTerminalFontSize(17.5));
        assert_eq!(app.preferences.terminal_font_size, 17.5);

        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::SettingsShellIntegration);
        let _ = update(&mut app, Message::SetShellIntegration(false));
        assert!(!app.preferences.shell_integration);
        assert_eq!(app.active_dropdown, None);

        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::SettingsLocalShell);
        let _ = update(
            &mut app,
            Message::SetLocalShell(crate::core::data::LocalShell::PowerShell7),
        );
        assert_eq!(
            app.preferences.local_shell,
            crate::core::data::LocalShell::PowerShell7
        );
        assert_eq!(app.active_dropdown, None);

        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::SettingsDiagnostics);
        let _ = update(
            &mut app,
            Message::SetDiagnosticsLevel(crate::core::diagnostics::Level::Debug),
        );
        assert_eq!(
            app.preferences.diagnostics_level,
            crate::core::diagnostics::Level::Debug
        );
        assert_eq!(app.active_dropdown, None);
    }

    #[test]
    fn picking_a_command_history_limit_stores_it_and_closes_the_picker() {
        let mut app = app();
        assert_eq!(
            app.preferences.command_history_limit,
            crate::core::data::COMMAND_HISTORY_DEFAULT_LIMIT
        );

        let _ = update(&mut app, Message::SettingsToggleCommandHistoryLimitPicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsHistoryLimit)
        );

        let _ = update(&mut app, Message::SetCommandHistoryLimit(2000));
        assert_eq!(app.preferences.command_history_limit, 2000);
        assert!(app.active_dropdown.is_none(), "choosing closes the list");
    }

    #[test]
    fn opening_a_settings_dropdown_closes_the_previous_one() {
        let mut app = app();

        let _ = update(&mut app, Message::SettingsToggleThemePicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsTheme)
        );
        let _ = update(&mut app, Message::SettingsToggleLocalePicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsLocale)
        );
        let _ = update(&mut app, Message::SettingsToggleDiagnosticsPicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsDiagnostics)
        );
        let _ = update(&mut app, Message::SettingsToggleShellIntegrationPicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsShellIntegration)
        );
        let _ = update(&mut app, Message::SettingsToggleLocalShellPicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsLocalShell)
        );
        let _ = update(&mut app, Message::SettingsToggleCommandHistoryLimitPicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsHistoryLimit)
        );

        let _ = update(&mut app, Message::SettingsToggleThemePicker);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::SettingsTheme)
        );
    }

    #[test]
    fn arrow_keys_walk_a_settings_dropdown_and_enter_confirms_it() {
        let mut app = app();
        app.preferences.theme_mode = ThemeMode::Dark;

        let opened = drain(update(&mut app, Message::SettingsToggleThemePicker));
        assert!(
            opened.other_actions > 0,
            "opening moves focus away from the underlying page"
        );
        press(
            &mut app,
            named(Named::ArrowDown),
            keyboard::Modifiers::empty(),
        );

        let picked = press(&mut app, named(Named::Enter), keyboard::Modifiers::empty());
        assert_answered(
            &picked,
            |message| matches!(message, Message::SetTheme(ThemeMode::Light)),
            "Enter activates the focused theme",
        );

        for message in picked.messages {
            let _ = update(&mut app, message);
        }
        assert_eq!(app.preferences.theme_mode, ThemeMode::Light);
        assert_eq!(app.active_dropdown, None);
    }

    #[test]
    fn escape_closes_a_dropdown_without_changing_its_value() {
        let mut app = app();
        app.preferences.theme_mode = ThemeMode::Dracula;

        let _ = update(&mut app, Message::SettingsToggleThemePicker);
        press(&mut app, named(Named::Escape), keyboard::Modifiers::empty());

        assert_eq!(app.active_dropdown, None);
        assert_eq!(app.preferences.theme_mode, ThemeMode::Dracula);
    }

    #[test]
    fn a_filtered_dropdown_activates_only_a_visible_option() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")];
        app.hosts.form.visible = true;

        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::FormToggleJumpPicker),
        );
        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::FormJumpSearch("db".into())),
        );
        let picked = press(&mut app, named(Named::Enter), keyboard::Modifiers::empty());

        assert_answered(
            &picked,
            |message| {
                matches!(
                    message,
                    Message::Hosts(crate::hosts::message::Message::FormSelectJump(
                        crate::core::data::HostId(2)
                    ))
                )
            },
            "Enter selects the only visible host",
        );
    }

    #[test]
    fn an_action_menu_can_be_confirmed_from_the_keyboard() {
        let mut app = app();

        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::HostTransferMenuToggle),
        );
        let picked = press(&mut app, named(Named::Enter), keyboard::Modifiers::empty());

        assert_answered(
            &picked,
            |message| {
                matches!(
                    message,
                    Message::Hosts(crate::hosts::message::Message::HostExportRequest)
                )
            },
            "Enter runs the first action",
        );
    }

    #[test]
    fn closing_the_window_asks_before_quitting_and_can_be_cancelled() {
        let mut app = app();

        let _ = update(&mut app, Message::WindowClose);
        assert!(app.show_quit_confirm);

        let _ = update(&mut app, Message::WindowCloseCancel);
        assert!(!app.show_quit_confirm, "quitting was cancelled");
    }

    #[test]
    fn a_modal_on_screen_takes_the_close_request_instead_of_the_quit_dialog() {
        let mut app = app();
        app.show_apply_workspace_confirm = true;

        let _ = update(&mut app, Message::WindowClose);

        assert!(
            !app.show_quit_confirm,
            "the open dialog owns the close request"
        );
    }

    #[test]
    fn dismissing_a_crash_report_clears_the_reports() {
        let mut app = app();
        app.show_crash_dialog = true;
        app.crash_reports.push((1, "panic backtrace".into()));

        let _ = update(&mut app, Message::CrashDialogClose);

        assert!(!app.show_crash_dialog);
        assert!(app.crash_reports.is_empty());
    }

    #[test]
    fn the_right_sidebar_messages_have_nothing_to_act_on_without_a_terminal() {
        let mut app = app();

        // No terminal session is active, so there is no sidebar to switch; the
        // messages have to be no-ops rather than a panic.
        let _ = update(&mut app, Message::SwitchRightTab(RightSidebarTab::Snippets));
        let _ = update(&mut app, Message::ToggleRightSidebar);
        let _ = update(
            &mut app,
            Message::CursorMove {
                target: CursorTarget::for_right_sidebar(RightSidebarTab::Commands),
                direction: GridDirection::Down,
                extend: false,
            },
        );
        let _ = update(
            &mut app,
            Message::RightSidebarSelectSnippet(crate::core::data::SnippetId(1)),
        );
        let _ = update(
            &mut app,
            Message::RightSidebarSelectCommand("cargo test".into()),
        );

        assert!(app.sidebar().is_none());
    }

    #[test]
    fn right_sidebar_cursor_scrolls_only_when_the_selected_row_is_outside_the_viewport() {
        use crate::views::right_sidebar::RIGHT_SIDEBAR_COMMANDS_SCROLL_ID;

        let mut app = app();
        app.scroll_offsets
            .insert(RIGHT_SIDEBAR_COMMANDS_SCROLL_ID, 0.0);
        app.scroll_viewport_heights
            .insert(RIGHT_SIDEBAR_COMMANDS_SCROLL_ID, 44.0);

        let visible = drain(scroll_right_sidebar_row(
            &app,
            RightSidebarTab::Commands,
            Some(0),
        ));
        assert_eq!(visible.other_actions, 0);

        let below = drain(scroll_right_sidebar_row(
            &app,
            RightSidebarTab::Commands,
            Some(2),
        ));
        assert_eq!(below.other_actions, 1);
    }

    #[test]
    fn switching_right_sidebar_tab_defers_focus_until_the_list_is_rendered() {
        let sidebar = SidebarState::default();

        assert_eq!(sidebar.active_tab, RightSidebarTab::Commands);
        let focus = drain(request_right_sidebar_focus(sidebar.active_tab));
        assert!(matches!(
            focus.messages.as_slice(),
            [Message::FocusRightSidebarTab(RightSidebarTab::Commands)]
        ));
        assert_eq!(focus.other_actions, 0);
    }

    #[test]
    fn a_sidebar_focus_request_survives_until_the_open_animation_finishes() {
        let mut sidebar = SidebarState {
            show: true,
            focus_list_pending: Some(RightSidebarTab::Commands),
            ..Default::default()
        };

        assert_eq!(
            take_pending_right_sidebar_focus(&mut sidebar),
            Some(RightSidebarTab::Commands)
        );
        assert!(sidebar.focus_list_pending.is_some());

        sidebar.anim = 1.0;
        assert_eq!(
            take_pending_right_sidebar_focus(&mut sidebar),
            Some(RightSidebarTab::Commands)
        );
        assert!(sidebar.focus_list_pending.is_none());

        sidebar.focus_list_pending = Some(RightSidebarTab::Commands);
        sidebar.closing = true;
        assert_eq!(take_pending_right_sidebar_focus(&mut sidebar), None);
    }

    #[test]
    fn activating_a_host_cursor_reuses_its_primary_action() {
        let mut app = app();
        app.hosts.list = vec![crate::core::test_support::host(7, "web", "web.example")];
        app.hosts.cursor.select(HostId(7));

        let activation = drain(activate_cursor(&app, CursorTarget::Hosts));

        assert!(matches!(
            activation.messages.as_slice(),
            [Message::Hosts(crate::hosts::message::Message::ConnectHost(
                HostId(7)
            ))]
        ));
        assert_eq!(activation.other_actions, 0);
    }

    #[test]
    fn a_filtered_out_host_is_not_activated() {
        let mut app = app();
        app.hosts.list = vec![
            crate::core::test_support::host(1, "web", "web.example"),
            crate::core::test_support::host(2, "db", "db.example"),
        ];
        app.hosts.cursor.select(HostId(1));
        app.hosts.search_query = "db".into();

        assert!(cursor_activation(&app, CursorTarget::Hosts).is_none());
    }

    #[test]
    fn window_geometry_events_are_recorded() {
        let mut app = app();

        let _ = update(
            &mut app,
            Message::WindowPositionChanged(Some(Point::new(10.0, 20.0))),
        );
        assert_eq!(app.window.position, Some(Point::new(10.0, 20.0)));

        let _ = update(&mut app, Message::WindowScaleFactorChanged(1.5));
        assert_eq!(app.window.scale_factor, 1.5);

        let _ = update(
            &mut app,
            Message::CursorPositionChanged(Point::new(3.0, 4.0)),
        );
        assert_eq!(app.window.cursor_pos, Point::new(3.0, 4.0));

        let _ = update(&mut app, Message::FullscreenChanged(true));
        assert!(
            app.window.is_maximized,
            "fullscreen drives the maximized flag"
        );
    }

    #[test]
    fn workspace_saving_is_refused_when_there_is_nothing_to_save() {
        let mut app = app();

        // A brand new app has no terminal sessions, so there is no workspace to
        // capture — the user gets told rather than an empty entry being written.
        let _ = update(&mut app, Message::WorkspaceSave);

        assert!(
            !app.workspace_manager.save_pending,
            "the save dialog must not open for an empty workspace"
        );
    }

    #[test]
    fn the_workspace_save_dialog_can_be_named_and_cancelled() {
        let mut app = app();
        app.workspace_manager.save_pending = true;

        let _ = update(&mut app, Message::WorkspaceSaveName("prod".into()));
        assert_eq!(app.workspace_manager.save_name, "prod");

        let _ = update(&mut app, Message::WorkspaceSaveCancel);
        assert!(!app.workspace_manager.save_pending);
    }

    #[test]
    fn the_workspace_transfer_menu_toggles_shut() {
        let mut app = app();

        let _ = update(&mut app, Message::WorkspaceTransferMenuToggle);
        assert_eq!(
            app.active_dropdown,
            Some(crate::core::dropdown::DropdownTarget::WorkspaceTransferMenu)
        );

        let _ = update(&mut app, Message::WorkspaceTransferMenuToggle);
        assert_eq!(app.active_dropdown, None);
    }

    #[test]
    fn hovering_a_workspace_marks_only_that_card() {
        let mut app = app();

        let _ = update(&mut app, Message::WorkspaceHoverEnter("a".into()));
        assert_eq!(app.workspace_manager.hovered_id.as_deref(), Some("a"));

        let _ = update(&mut app, Message::WorkspaceHoverEnter("b".into()));
        let _ = update(&mut app, Message::WorkspaceHoverLeave("a".into()));
        assert_eq!(
            app.workspace_manager.hovered_id.as_deref(),
            Some("b"),
            "a stale leave must not clear the hovered card"
        );

        let _ = update(&mut app, Message::WorkspaceHoverLeave("b".into()));
        assert_eq!(app.workspace_manager.hovered_id, None);
    }

    #[test]
    fn selecting_a_workspace_tracks_only_that_card() {
        let mut app = app();

        let _ = update(&mut app, Message::WorkspaceSelect("prod".into()));

        assert_eq!(
            app.workspace_manager
                .cursor
                .selected_key()
                .map(String::as_str),
            Some("prod")
        );
    }

    #[test]
    fn workspace_cursor_walks_the_saved_list() {
        let mut app = app();
        app.workspace_manager.list = vec![saved_workspace("one"), saved_workspace("two")];

        let _ = update(
            &mut app,
            Message::CursorMove {
                target: CursorTarget::Workspaces,
                direction: GridDirection::Down,
                extend: false,
            },
        );
        assert_eq!(
            app.workspace_manager
                .cursor
                .selected_key()
                .map(String::as_str),
            Some("one")
        );
        let _ = update(
            &mut app,
            Message::CursorMove {
                target: CursorTarget::Workspaces,
                direction: GridDirection::Down,
                extend: false,
            },
        );
        assert_eq!(
            app.workspace_manager
                .cursor
                .selected_key()
                .map(String::as_str),
            Some("two")
        );
    }

    #[test]
    fn cursor_activation_maps_each_collection_to_its_primary_action() {
        let mut app = app();

        app.hosts.list = vec![crate::core::test_support::host(1, "web", "web.example")];
        app.hosts.cursor.select(HostId(1));
        assert!(matches!(
            cursor_activation(&app, CursorTarget::Hosts),
            Some(Message::Hosts(crate::hosts::message::Message::ConnectHost(
                HostId(1)
            )))
        ));

        app.keychain.list = vec![crate::core::data::KeychainEntry {
            id: crate::core::data::KeychainEntryId(2),
            name: "deploy".into(),
            key_type: "ssh-ed25519".into(),
            fingerprint: "SHA256:test".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            path: "/tmp/deploy".into(),
            created_at: String::new(),
        }];
        app.keychain
            .cursor
            .select(crate::keychain::KeychainSelection::Key(
                crate::core::data::KeychainEntryId(2),
            ));
        assert!(matches!(
            cursor_activation(&app, CursorTarget::Keychain),
            Some(Message::Keychain(
                crate::keychain::message::Message::KeychainEdit(
                    crate::core::data::KeychainEntryId(2)
                )
            ))
        ));
        app.keychain.identities = vec![crate::core::data::Identity {
            id: crate::core::data::IdentityId(3),
            label: "prod".into(),
            username: "deploy".into(),
            password: secrecy::SecretString::from("secret"),
            created_at: String::new(),
        }];
        app.keychain
            .cursor
            .select(crate::keychain::KeychainSelection::Identity(
                crate::core::data::IdentityId(3),
            ));
        assert!(matches!(
            cursor_activation(&app, CursorTarget::Keychain),
            Some(Message::Keychain(
                crate::keychain::message::Message::IdentityEdit(crate::core::data::IdentityId(3))
            ))
        ));

        app.port_forwards.list = vec![crate::core::data::PortForward {
            id: crate::core::data::PortForwardId(4),
            name: "web".into(),
            kind: crate::core::data::PortForwardKind::Local,
            host_id: HostId(1),
            bind_address: "127.0.0.1".into(),
            listen_port: 8080,
            dest_host: "localhost".into(),
            dest_port: 80,
        }];
        app.port_forwards
            .cursor
            .select(crate::core::data::PortForwardId(4));
        assert!(matches!(
            cursor_activation(&app, CursorTarget::PortForwards),
            Some(Message::PortForward(
                crate::port_forward::message::Message::PortForwardToggle(
                    crate::core::data::PortForwardId(4)
                )
            ))
        ));

        app.snippets.list = vec![crate::core::data::Snippet {
            id: crate::core::data::SnippetId(5),
            description: "deploy".into(),
            script: "cargo test".into(),
            package_id: None,
            last_used_at: String::new(),
        }];
        app.snippets
            .cursor
            .select(SnippetSelection::Snippet(crate::core::data::SnippetId(5)));
        assert!(matches!(
            cursor_activation(&app, CursorTarget::Snippets),
            Some(Message::Snippets(
                crate::snippets::message::Message::SnippetCopyScript(crate::core::data::SnippetId(
                    5
                ))
            ))
        ));
        app.snippets.packages = vec![crate::core::data::SnippetPackage {
            id: crate::core::data::SnippetPackageId(6),
            name: "dev".into(),
            description: String::new(),
        }];
        app.snippets.cursor.select(SnippetSelection::Package(
            crate::core::data::SnippetPackageId(6),
        ));
        assert!(matches!(
            cursor_activation(&app, CursorTarget::Snippets),
            Some(Message::Snippets(
                crate::snippets::message::Message::SnippetPackageSelect(
                    crate::core::data::SnippetPackageId(6)
                )
            ))
        ));

        app.known_hosts.entries = vec![crate::core::data::KnownHost {
            line_number: 7,
            hostname: "web.example".into(),
            port: 22,
            key_type: "ssh-ed25519".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            fingerprint: "SHA256:test".into(),
            marker: None,
            raw_pattern: "web.example".into(),
        }];
        app.known_hosts.cursor.select(7);
        assert!(matches!(
            cursor_activation(&app, CursorTarget::KnownHosts),
            Some(Message::KnownHosts(
                crate::known_hosts::message::Message::KnownHostShowDetails(7)
            ))
        ));

        app.workspace_manager.list = vec![saved_workspace("prod")];
        app.workspace_manager.cursor.select("prod".into());
        assert!(matches!(
            cursor_activation(&app, CursorTarget::Workspaces),
            Some(Message::WorkspaceApply(ref id)) if id == "prod"
        ));
    }

    #[test]
    fn spotlight_actions_for_a_session_that_is_gone_are_ignored() {
        let mut app = app();

        // The palette belongs to a terminal session; without one there is
        // nothing to open it over, and no state may be created for a dead id.
        for message in [
            Message::SpotlightShow(SessionId(7)),
            Message::SpotlightFilter(SessionId(7), "new".into()),
            Message::SpotlightMoveDown(SessionId(7)),
            Message::SpotlightMoveUp(SessionId(7)),
            Message::SpotlightConfirm(SessionId(7)),
            Message::SpotlightCancel(SessionId(7)),
            Message::SpotlightHover(SessionId(7), Some(0)),
            Message::SpotlightPick(SessionId(7), "terminal:new_tab".into()),
        ] {
            let _ = update(&mut app, message);
        }

        assert!(
            app.terminal.sessions.iter().all(|s| s.spotlight.is_none()),
            "no palette state may be created for a session that does not exist"
        );
    }

    #[test]
    fn context_menu_arrow_keys_move_focus_and_enter_activates_it() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];

        let opened = drain(update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::HostsContextMenuOpen(
                crate::core::data::HostId(1),
            )),
        ));
        assert!(
            opened.other_actions > 0,
            "opening unfocuses the collection under the menu"
        );

        press(
            &mut app,
            named(Named::ArrowDown),
            keyboard::Modifiers::empty(),
        );
        let picked = press(&mut app, named(Named::Enter), keyboard::Modifiers::empty());

        assert_answered(
            &picked,
            |message| {
                matches!(
                    message,
                    Message::Hosts(crate::hosts::message::Message::DuplicateHost(
                        crate::core::data::HostId(1)
                    ))
                )
            },
            "Enter runs the focused host action",
        );
        for message in picked.messages {
            let _ = update(&mut app, message);
        }
        assert!(app.hosts.context_menu.is_none());
    }

    #[test]
    fn closing_a_host_editor_restores_the_vault_cursor_focus() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];

        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::HostsContextMenuOpen(
                crate::core::data::HostId(1),
            )),
        );
        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::EditHost(
                crate::core::data::HostId(1),
            )),
        );
        assert!(
            app.hosts.form.visible,
            "the edit action opens the host editor"
        );

        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::HideForm),
        );

        let mut focus_restored = false;
        for _ in 0..200 {
            let _ = update(&mut app, Message::AnimTick);
            focus_restored |=
                synced_focus(&mut app) == Some(crate::focus::FocusId::Cursor(CursorTarget::Hosts));
            if !app.hosts.form.visible {
                break;
            }
        }

        assert!(
            !app.hosts.form.visible,
            "the closing animation has finished"
        );
        assert!(
            focus_restored,
            "the host grid must take keyboard focus back after the editor closes"
        );
    }

    #[test]
    fn saving_a_host_returns_focus_to_the_vault_cursor() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];
        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::EditHost(
                crate::core::data::HostId(1),
            )),
        );
        assert!(app.hosts.form.visible);
        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::FormHostname(
                "10.0.0.9".into(),
            )),
        );
        assert!(
            app.hosts.form.save_enabled(),
            "the fixture host is saveable"
        );

        let save = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::SaveHost),
        );
        drop(save);

        assert!(
            !app.hosts.form.visible,
            "saving closes the editor immediately"
        );
        assert_eq!(
            app.focus.top(),
            Some(crate::focus::FocusId::Cursor(CursorTarget::Hosts)),
            "the host grid becomes the keyboard owner after saving"
        );
    }

    #[test]
    fn closing_a_context_menu_dialog_restores_the_vault_cursor_focus() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];

        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::HostsContextMenuOpen(
                crate::core::data::HostId(1),
            )),
        );
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::ContextMenu)
        );

        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::HostDeleteRequest(
                crate::core::data::HostId(1),
            )),
        );
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Dialog(
                crate::focus::DialogId::HostDelete
            ))
        );

        let _ = update(
            &mut app,
            Message::Hosts(crate::hosts::message::Message::HostDeleteCancel),
        );

        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Cursor(CursorTarget::Hosts))
        );
    }

    #[test]
    fn escape_closes_the_sftp_sidebar_context_menu() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.sidebar_context_menu = Some(crate::sftp::SftpSidebarContextMenu {
            session_id: crate::app::SftpSessionId(1),
            tab_index: 0,
            can_close: true,
            cursor: crate::context_menu::initial_cursor(&[
                crate::core::context_menu::ContextMenuAction::SftpSidebarDuplicate,
                crate::core::context_menu::ContextMenuAction::SftpSidebarClose,
            ]),
        });

        let closed = press(&mut app, named(Named::Escape), keyboard::Modifiers::empty());

        assert_answered(
            &closed,
            |message| {
                matches!(
                    message,
                    Message::Sftp(crate::sftp::message::Message::SftpSidebarContextMenuClose)
                )
            },
            "Escape closes the topmost SFTP sidebar menu",
        );
        for message in closed.messages {
            let _ = update(&mut app, message);
        }
        assert!(app.sftp.sidebar_context_menu.is_none());
    }

    // Tab strip geometry and focus
    //
    // The topbar measures the tabs it is about to draw, and every navigation
    // path funnels its focus request through the app focus stack. Both read the tab
    // registry and nothing else, so the registry is all these fixtures fill.

    /// Register a tab hosting a bare session id, the way a new session does.
    /// The session behind it is never needed — only the entry is.
    fn session_tab(app: &mut App, id: u32) -> crate::app::TabId {
        app.allocate_tab_id(TabKind::Session(SessionId(id)))
    }

    /// Register a new-tab draft in `state`, as an SSH connect attempt leaves it.
    fn draft_tab(app: &mut App, id: u32, state: SshConnectionState) -> crate::app::TabId {
        app.terminal.drafts.insert(
            DraftId(id),
            crate::terminal::Draft {
                query: String::new(),
                cursor: crate::core::list_cursor::ListCursor::new(),
                ssh: crate::ssh_connect::SshConnectState {
                    attempt: crate::ssh_connect::ConnectionAttempt {
                        state,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            },
        );
        app.allocate_tab_id(TabKind::Draft(DraftId(id)))
    }

    /// The width the strip gives the tab the user is looking at.
    fn active_tab_width() -> f32 {
        (crate::views::TAB_MIN_WIDTH + 40.0).min(crate::views::MAX_TAB_WIDTH)
    }

    #[test]
    fn the_tab_strip_width_adds_up_the_visible_tabs() {
        let mut app = app();
        assert_eq!(
            compute_tab_strip_content_width(&app),
            0.0,
            "no tabs, no strip"
        );

        let first = session_tab(&mut app, 1);
        app.activate_tab(first);
        assert_eq!(compute_tab_strip_content_width(&app), active_tab_width());

        // The active tab is wider than the rest, and neighbours are separated
        // by a fixed gap — the two things the per-tab loop has to add up.
        let second = session_tab(&mut app, 2);
        assert_eq!(
            compute_tab_strip_content_width(&app),
            active_tab_width() + crate::views::TAB_MIN_WIDTH + 6.0
        );

        // A closed tab must shrink the strip again, or the topbar keeps
        // scrolling to make room for something that is no longer drawn.
        app.remove_tab(second);
        assert_eq!(compute_tab_strip_content_width(&app), active_tab_width());
    }

    #[test]
    fn the_active_tab_offset_sums_only_the_tabs_before_it() {
        let mut app = app();
        let first = session_tab(&mut app, 1);
        let second = session_tab(&mut app, 2);
        session_tab(&mut app, 3);

        app.activate_tab(first);
        assert_eq!(compute_active_tab_left(&app), Some(0.0));

        // Only tabs to the left of the active one take space, at their
        // collapsed width.
        app.activate_tab(second);
        assert_eq!(
            compute_active_tab_left(&app),
            Some(crate::views::TAB_MIN_WIDTH + 6.0)
        );
    }

    #[test]
    fn there_is_no_tab_offset_without_a_tab_in_the_strip() {
        let mut app = app();
        // Vaults and SFTP are not inside the scrollable strip, so there is
        // nothing to scroll to.
        assert_eq!(compute_active_tab_left(&app), None);

        // An active tab that is no longer registered has no offset either:
        // this is what a stale id after a close looks like.
        let stale = app.allocate_tab_id(TabKind::Session(SessionId(9)));
        app.remove_tab(stale);
        app.activate_tab(stale);
        assert_eq!(compute_active_tab_left(&app), None);
    }

    #[test]
    fn focus_follows_what_the_active_tab_shows() {
        let mut app = app();
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Cursor(CursorTarget::Hosts))
        );

        let session = session_tab(&mut app, 7);
        app.activate_tab(session);
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Terminal(SessionId(7)))
        );

        // An idle draft focuses its search box…
        let idle = draft_tab(&mut app, 3, SshConnectionState::Idle);
        app.activate_tab(idle);
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::NewTab(DraftId(3)))
        );

        // …a draft waiting for a password focuses the password field…
        let password = draft_tab(&mut app, 4, SshConnectionState::WaitingForPassword);
        app.activate_tab(password);
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::SshPassword)
        );

        let confirmation = draft_tab(&mut app, 6, SshConnectionState::WaitingForKeyConfirmation);
        app.activate_tab(confirmation);
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Dialog(
                crate::focus::DialogId::SshKeyConfirmation
            ))
        );

        // …and every state with no interactive overlay focuses nothing at all.
        // These states are progress indicators or button-driven screens.
        for state in [
            SshConnectionState::Connecting,
            SshConnectionState::Connected,
            SshConnectionState::Failed,
        ] {
            let tab = draft_tab(&mut app, 5, state);
            app.activate_tab(tab);
            assert_eq!(synced_focus(&mut app), None, "{state:?} has no input");
        }
    }

    #[test]
    fn overlays_take_focus_precedence_over_the_active_view() {
        let mut app = app();
        app.hosts.context_menu = Some(crate::hosts::HostsContextMenu {
            host_id: HostId(1),
            position: Point::ORIGIN,
            cursor: crate::context_menu::initial_cursor(&[
                crate::core::context_menu::ContextMenuAction::HostEdit,
                crate::core::context_menu::ContextMenuAction::HostDuplicate,
                crate::core::context_menu::ContextMenuAction::HostDelete,
            ]),
        });
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::ContextMenu)
        );

        app.hosts.context_menu = None;
        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::HostsTagFilter);
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Dropdown(
                crate::core::dropdown::DropdownTarget::HostsTagFilter
            ))
        );
    }

    #[test]
    fn vault_pages_focus_their_cursor_unless_an_editor_is_open() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);

        let pages = [
            (NavItem::Hosts, CursorTarget::Hosts),
            (NavItem::Keychain, CursorTarget::Keychain),
            (NavItem::PortForwarding, CursorTarget::PortForwards),
            (NavItem::Snippets, CursorTarget::Snippets),
            (NavItem::KnownHosts, CursorTarget::KnownHosts),
            (NavItem::Settings, CursorTarget::Workspaces),
        ];
        for (page, target) in pages {
            app.active_nav = page;
            assert_eq!(
                synced_focus(&mut app),
                Some(crate::focus::FocusId::Cursor(target))
            );
        }

        app.active_nav = NavItem::Logs;
        assert_eq!(synced_focus(&mut app), None);

        app.active_nav = NavItem::Keychain;
        app.keychain.form.visible = true;
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Editor(
                crate::focus::EditorId::KeychainKey
            ))
        );
        app.keychain.form.visible = false;

        app.active_nav = NavItem::Hosts;
        app.hosts.delete_pending = Some(HostId(1));
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Dialog(
                crate::focus::DialogId::HostDelete
            ))
        );
    }

    #[test]
    fn the_sftp_pane_that_wants_a_password_takes_the_focus() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);

        app.sftp.state.left.pane.attempt.state = SshConnectionState::WaitingForPassword;
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::SftpPassword {
                pane: crate::sftp::SftpPane::Local
            })
        );

        // Only the prompt has an input to focus; a connection in progress must
        // fall back to the active file pane.
        app.sftp.state.left.pane.attempt.state = SshConnectionState::Connecting;
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::Cursor(CursorTarget::SftpLocalFiles))
        );

        app.sftp.state.active_pane = SftpPane::Remote;
        app.sftp.state.right.pane.attempt.state = SshConnectionState::WaitingForPassword;
        assert_eq!(
            synced_focus(&mut app),
            Some(crate::focus::FocusId::SftpPassword {
                pane: crate::sftp::SftpPane::Remote
            })
        );
    }

    #[test]
    fn a_new_workspace_is_named_after_the_highest_number_in_use() {
        let mut app = app();
        // The saved list is read from the data directory at boot, which the
        // whole test binary shares; what matters here is the arithmetic.
        app.workspace_manager.list.clear();
        assert_eq!(
            default_workspace_name(&app),
            auto_workspace_name(1),
            "the first save is number one"
        );

        // Numeric suffixes count whoever wrote them; names without one are
        // ignored instead of resetting the count to one.
        for name in ["workspace-2", "prod-7", "notes", "工作区-5"] {
            app.workspace_manager.list.push(saved_workspace(name));
        }
        assert_eq!(default_workspace_name(&app), auto_workspace_name(8));

        app.workspace_manager.list.push(saved_workspace("prod-9"));
        assert_eq!(
            default_workspace_name(&app),
            auto_workspace_name(10),
            "the suggestion only moves up, so two saves in a row differ"
        );
    }

    /// The name the dialog would suggest, in whatever locale the test runs in.
    fn auto_workspace_name(number: u64) -> String {
        t!("workspace.auto_name", number = number).to_string()
    }

    fn saved_workspace(name: &str) -> crate::workspace::SavedWorkspace {
        crate::workspace::SavedWorkspace {
            id: name.to_string(),
            name: name.to_string(),
            updated_at: 0,
            snapshot: snapshot_with(Vec::new()),
        }
    }

    // ---- the global keyboard handler ----
    //
    // Every shortcut the shell does not deliver to a focused widget lands in
    // `handle_global_keyboard`, which decides what the key means by looking at
    // the overlays currently open. The tests below press keys through the real
    // handler and read the message it answers with, which is the part of the
    // shortcut that no state assertion can see.

    use crate::core::data::FileEntryType;
    use crate::core::test_support::{drain, host};
    use iced::keyboard::key::Named;

    /// A key press carrying `modifiers`, shaped the way the shell delivers it.
    fn key_pressed(key: iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> Message {
        Message::TermEvent(iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: iced::keyboard::key::Physical::Unidentified(
                iced::keyboard::key::NativeCode::Unidentified,
            ),
            location: iced::keyboard::Location::Standard,
            modifiers,
            repeat: false,
            text: None,
        }))
    }

    /// Press a key through the real handler and read back what it answered.
    fn press(
        app: &mut App,
        key: iced::keyboard::Key,
        modifiers: iced::keyboard::Modifiers,
    ) -> crate::core::test_support::Drained {
        let _ = crate::focus::sync(app);
        drain(update(app, key_pressed(key, modifiers)))
    }

    fn named(name: Named) -> iced::keyboard::Key {
        iced::keyboard::Key::Named(name)
    }

    /// One row of a keyboard table: what the case is about, the state to set up
    /// before the key is pressed, and the message the handler owes in answer.
    type MessageCase = (&'static str, fn(&mut App), fn(&Message) -> bool);

    /// The same, for a case whose assertion reads the app rather than a message.
    type StateCase = (&'static str, fn(&mut App), fn(&App) -> bool);

    /// The same, for a case that varies the transfer prompt on screen.
    type ModalCase = (
        &'static str,
        crate::sftp::SftpTransferModal,
        fn(&Message) -> bool,
    );

    /// Assert the handler answered with exactly one message, the one `expects`
    /// accepts, and asked for nothing else.
    fn assert_answered(
        drained: &crate::core::test_support::Drained,
        expects: impl Fn(&Message) -> bool,
        what: &str,
    ) {
        assert_eq!(
            drained.messages.len(),
            1,
            "{what}: expected one message, got {:?}",
            drained.messages
        );
        assert!(
            expects(&drained.messages[0]),
            "{what}: answered with {:?}",
            drained.messages[0]
        );
        assert_eq!(drained.other_actions, 0, "{what}: asked for extra actions");
    }

    /// An empty workspace-manager import dialog.
    fn import_dialog() -> crate::workspace::WorkspaceImportDialog {
        crate::workspace::WorkspaceImportDialog {
            pending_workspaces: Vec::new(),
            new_count: 0,
            dup_count: 0,
        }
    }

    /// A rename prompt for the remote pane.
    fn rename_pending() -> crate::sftp::SftpRenamePending {
        crate::sftp::SftpRenamePending {
            pane: crate::sftp::SftpPane::Local,
            directory: "/tmp".to_string(),
            old_name: "notes.txt".to_string(),
            file_type: FileEntryType::File,
            new_name: "notes.txt".to_string(),
            in_progress: false,
        }
    }

    /// A new-folder prompt for the local pane.
    fn new_folder_pending() -> crate::sftp::SftpNewFolderPending {
        crate::sftp::SftpNewFolderPending {
            pane: crate::sftp::SftpPane::Local,
            directory: "/tmp".to_string(),
            name: String::new(),
            in_progress: false,
        }
    }

    /// A draft tab in place of the real new-tab form.
    fn with_draft(app: &mut App) -> DraftId {
        let id = DraftId(1);
        app.terminal.drafts.insert(
            id,
            crate::terminal::Draft {
                query: String::new(),
                cursor: crate::core::list_cursor::ListCursor::new(),
                ssh: crate::ssh_connect::SshConnectState::default(),
            },
        );
        let tab = app.allocate_tab_id(TabKind::Draft(id));
        app.active_tab = ActiveTab::Tab(tab);
        id
    }

    #[test]
    fn a_modifier_change_reaches_the_sftp_multi_select() {
        let mut app = app();
        let modifiers = iced::keyboard::Modifiers::SHIFT;
        let _ = crate::focus::sync(&mut app);

        let carried = drain(update(
            &mut app,
            Message::TermEvent(iced::Event::Keyboard(
                iced::keyboard::Event::ModifiersChanged(modifiers),
            )),
        ));

        assert!(carried.messages.is_empty());
        assert_eq!(carried.other_actions, 0);
        assert_eq!(
            app.sftp.state.keyboard_modifiers, modifiers,
            "Shift+Click multi-select reads the modifiers off the app"
        );
    }

    #[test]
    fn the_platform_fullscreen_shortcut_requests_window_fullscreen() {
        let mut app = app();
        let (key, modifiers) = crate::app::keybindings::fullscreen_shortcut();
        let pressed = press(&mut app, key, modifiers);

        assert_answered(
            &pressed,
            |message| matches!(message, Message::WindowToggleFullscreen),
            "the platform fullscreen shortcut",
        );
    }

    #[test]
    fn escape_dismisses_the_topmost_overlay() {
        let cases: Vec<MessageCase> = vec![
            (
                "workspace save",
                |app| app.workspace_manager.save_pending = true,
                |m| matches!(m, Message::WorkspaceSaveCancel),
            ),
            (
                "workspace save conflict",
                |app| {
                    app.workspace_manager.save_pending = true;
                    app.workspace_manager.save_conflict = Some("web".to_string());
                },
                |m| matches!(m, Message::WorkspaceSaveConflictCancel),
            ),
            (
                "workspace apply",
                |app| app.show_apply_workspace_confirm = true,
                |m| matches!(m, Message::WorkspaceApplyCancel),
            ),
            (
                "workspace delete",
                |app| app.workspace_manager.delete_id = Some("web".to_string()),
                |m| matches!(m, Message::WorkspaceDeleteCancel),
            ),
            (
                "workspace rename",
                |app| app.workspace_manager.rename_id = Some("web".to_string()),
                |m| matches!(m, Message::WorkspaceRenameCancel),
            ),
            (
                "workspace import",
                |app| app.workspace_manager.import_dialog = Some(import_dialog()),
                |m| matches!(m, Message::WorkspaceImportCancel),
            ),
            (
                "workspace recovery",
                |app| app.workspace_recovery.show_prompt = true,
                |m| {
                    matches!(
                        m,
                        Message::WorkspaceRecovery(
                            crate::workspace::recovery::Message::RestoreDismiss
                        )
                    )
                },
            ),
            (
                "host delete",
                |app| app.hosts.delete_pending = Some(HostId(7)),
                |m| {
                    matches!(
                        m,
                        Message::Hosts(crate::hosts::message::Message::HostDeleteCancel)
                    )
                },
            ),
            (
                "keychain delete",
                |app| app.keychain.delete_pending = Some(crate::core::data::KeychainEntryId(7)),
                |m| {
                    matches!(
                        m,
                        Message::Keychain(crate::keychain::message::Message::KeychainDeleteCancel)
                    )
                },
            ),
            (
                "identity delete",
                |app| app.keychain.identity_delete_pending = Some(crate::core::data::IdentityId(7)),
                |m| {
                    matches!(
                        m,
                        Message::Keychain(crate::keychain::message::Message::IdentityDeleteCancel)
                    )
                },
            ),
            (
                "known host delete",
                |app| app.known_hosts.delete_pending = Some(7),
                |m| {
                    matches!(
                        m,
                        Message::KnownHosts(
                            crate::known_hosts::message::Message::KnownHostDeleteCancel
                        )
                    )
                },
            ),
            (
                "port forward delete",
                |app| app.port_forwards.delete_pending = Some(crate::core::data::PortForwardId(7)),
                |m| {
                    matches!(
                        m,
                        Message::PortForward(
                            crate::port_forward::message::Message::PortForwardDeleteCancel
                        )
                    )
                },
            ),
            (
                "port forward logs",
                |app| {
                    app.port_forwards
                        .cursor
                        .select(crate::core::data::PortForwardId(7));
                    app.port_forwards.logs_open = true;
                },
                |m| {
                    matches!(
                        m,
                        Message::PortForward(
                            crate::port_forward::message::Message::PortForwardHideLogs
                        )
                    )
                },
            ),
            (
                "snippet delete",
                |app| app.snippets.delete_pending = Some(crate::core::data::SnippetId(7)),
                |m| {
                    matches!(
                        m,
                        Message::Snippets(crate::snippets::message::Message::SnippetDeleteCancel)
                    )
                },
            ),
            (
                "snippet package delete",
                |app| {
                    app.snippets.package_delete_pending =
                        Some(crate::core::data::SnippetPackageId(7));
                },
                |m| {
                    matches!(
                        m,
                        Message::Snippets(
                            crate::snippets::message::Message::SnippetPackageDeleteCancel
                        )
                    )
                },
            ),
            (
                "sftp rename",
                |app| app.sftp.state.rename_pending = Some(rename_pending()),
                |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpRenameCancel)
                    )
                },
            ),
            (
                "sftp new folder",
                |app| app.sftp.state.new_folder_pending = Some(new_folder_pending()),
                |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpNewFolderCancel)
                    )
                },
            ),
            (
                "hosts context menu",
                |app| {
                    app.hosts.context_menu = Some(crate::hosts::HostsContextMenu {
                        host_id: HostId(7),
                        position: Point::ORIGIN,
                        cursor: crate::context_menu::initial_cursor(&[
                            crate::core::context_menu::ContextMenuAction::HostEdit,
                            crate::core::context_menu::ContextMenuAction::HostDuplicate,
                            crate::core::context_menu::ContextMenuAction::HostDelete,
                        ]),
                    });
                },
                |m| {
                    matches!(
                        m,
                        Message::Hosts(crate::hosts::message::Message::HostsContextMenuClose)
                    )
                },
            ),
            (
                "keychain context menu",
                |app| {
                    app.keychain.context_menu = Some(crate::keychain::KeychainContextMenu::Key {
                        id: crate::core::data::KeychainEntryId(7),
                        position: Point::ORIGIN,
                        cursor: crate::context_menu::initial_cursor(&[
                            crate::core::context_menu::ContextMenuAction::KeychainEdit,
                            crate::core::context_menu::ContextMenuAction::KeychainDelete,
                        ]),
                    });
                },
                |m| {
                    matches!(
                        m,
                        Message::Keychain(
                            crate::keychain::message::Message::KeychainContextMenuClose
                        )
                    )
                },
            ),
            (
                "port forward context menu",
                |app| {
                    app.port_forwards.context_menu =
                        Some(crate::port_forward::PortForwardContextMenu {
                            forward_id: crate::core::data::PortForwardId(7),
                            position: Point::ORIGIN,
                            cursor: crate::context_menu::initial_cursor(&[
                                crate::core::context_menu::ContextMenuAction::PortForwardToggle,
                                crate::core::context_menu::ContextMenuAction::PortForwardEdit,
                                crate::core::context_menu::ContextMenuAction::PortForwardLogs,
                                crate::core::context_menu::ContextMenuAction::PortForwardDuplicate,
                                crate::core::context_menu::ContextMenuAction::PortForwardDelete,
                            ]),
                        });
                },
                |m| {
                    matches!(
                        m,
                        Message::PortForward(
                            crate::port_forward::message::Message::PortForwardContextMenuClose
                        )
                    )
                },
            ),
            (
                "snippet context menu",
                |app| {
                    app.snippets.context_menu =
                        Some(crate::snippets::SnippetContextMenu::Snippet {
                            id: crate::core::data::SnippetId(7),
                            position: Point::ORIGIN,
                            cursor: crate::context_menu::initial_cursor(&[
                                crate::core::context_menu::ContextMenuAction::SnippetEdit,
                                crate::core::context_menu::ContextMenuAction::SnippetCopy,
                                crate::core::context_menu::ContextMenuAction::SnippetDelete,
                            ]),
                        });
                },
                |m| {
                    matches!(
                        m,
                        Message::Snippets(
                            crate::snippets::message::Message::SnippetContextMenuClose
                        )
                    )
                },
            ),
            (
                "known host context menu",
                |app| {
                    app.known_hosts.context_menu = Some(crate::known_hosts::KnownHostContextMenu {
                        id: 7,
                        position: Point::ORIGIN,
                        cursor: crate::context_menu::initial_cursor(&[
                            crate::core::context_menu::ContextMenuAction::KnownHostDetails,
                            crate::core::context_menu::ContextMenuAction::KnownHostDelete,
                        ]),
                    });
                },
                |m| {
                    matches!(
                        m,
                        Message::KnownHosts(
                            crate::known_hosts::message::Message::KnownHostContextMenuClose
                        )
                    )
                },
            ),
            (
                "terminal context menu",
                |app| {
                    app.terminal.context_menu = Some(crate::terminal::TerminalContextMenu {
                        session_id: SessionId(1),
                        position: Point::ORIGIN,
                        has_tabs_to_right: false,
                        is_split: false,
                        locked: false,
                        cursor: crate::context_menu::initial_cursor(&[
                            crate::core::context_menu::ContextMenuAction::TerminalDuplicate,
                            crate::core::context_menu::ContextMenuAction::TerminalNewWindow,
                            crate::core::context_menu::ContextMenuAction::TerminalRename,
                            crate::core::context_menu::ContextMenuAction::TerminalToggleLock,
                            crate::core::context_menu::ContextMenuAction::TerminalClose,
                            crate::core::context_menu::ContextMenuAction::TerminalCloseOthers,
                        ]),
                    });
                },
                |m| {
                    matches!(
                        m,
                        Message::Terminal(crate::terminal::message::Message::TermContextMenuClose)
                    )
                },
            ),
            (
                "host form",
                |app| {
                    app.active_nav = NavItem::Hosts;
                    app.hosts.form.visible = true;
                },
                |m| matches!(m, Message::Hosts(crate::hosts::message::Message::HideForm)),
            ),
            (
                "identity form",
                |app| {
                    app.active_nav = NavItem::Keychain;
                    app.keychain.identity_form.visible = true;
                },
                |m| {
                    matches!(
                        m,
                        Message::Keychain(crate::keychain::message::Message::IdentityHideForm)
                    )
                },
            ),
        ];

        for (what, set_up, expects) in cases {
            let mut app = app();
            set_up(&mut app);

            let carried = press(
                &mut app,
                named(Named::Escape),
                iced::keyboard::Modifiers::default(),
            );

            assert_answered(&carried, expects, what);
        }
    }

    #[test]
    fn enter_confirms_the_topmost_overlay() {
        let cases: Vec<MessageCase> = vec![
            (
                "host delete",
                |app| app.hosts.delete_pending = Some(HostId(7)),
                |m| {
                    matches!(
                        m,
                        Message::Hosts(crate::hosts::message::Message::DeleteHost(id)) if *id == HostId(7)
                    )
                },
            ),
            (
                "keychain delete",
                |app| app.keychain.delete_pending = Some(crate::core::data::KeychainEntryId(7)),
                |m| {
                    matches!(
                        m,
                        Message::Keychain(crate::keychain::message::Message::KeychainDelete(id)) if *id == crate::core::data::KeychainEntryId(7)
                    )
                },
            ),
            (
                "snippet delete",
                |app| app.snippets.delete_pending = Some(crate::core::data::SnippetId(7)),
                |m| {
                    matches!(
                        m,
                        Message::Snippets(crate::snippets::message::Message::SnippetDelete(id)) if *id == crate::core::data::SnippetId(7)
                    )
                },
            ),
            (
                "port forward delete",
                |app| app.port_forwards.delete_pending = Some(crate::core::data::PortForwardId(7)),
                |m| {
                    matches!(
                        m,
                        Message::PortForward(
                            crate::port_forward::message::Message::PortForwardDelete(id)
                        ) if *id == crate::core::data::PortForwardId(7)
                    )
                },
            ),
            (
                "known host delete",
                |app| app.known_hosts.delete_pending = Some(7),
                |m| {
                    matches!(
                        m,
                        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostDelete(
                            7
                        ))
                    )
                },
            ),
            (
                "sftp rename",
                |app| app.sftp.state.rename_pending = Some(rename_pending()),
                |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpRenameConfirm)
                    )
                },
            ),
            (
                "sftp new folder",
                |app| app.sftp.state.new_folder_pending = Some(new_folder_pending()),
                |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpNewFolderConfirm)
                    )
                },
            ),
            (
                "sftp delete confirmation",
                |app| {
                    app.sftp.state.delete_confirm = Some(crate::sftp::SftpDeletePending {
                        pane: crate::sftp::SftpPane::Local,
                        directory: "/tmp".to_string(),
                        names: vec!["notes.txt".to_string()],
                        file_types: vec![FileEntryType::File],
                        in_progress: false,
                        remaining: 1,
                        failures: Vec::new(),
                    });
                },
                |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpDeleteConfirm)
                    )
                },
            ),
            (
                "host form",
                |app| {
                    app.active_nav = NavItem::Hosts;
                    app.hosts.form.visible = true;
                },
                |m| matches!(m, Message::Hosts(crate::hosts::message::Message::SaveHost)),
            ),
            (
                "keychain form",
                |app| {
                    app.active_nav = NavItem::Keychain;
                    app.keychain.form.visible = true;
                },
                |m| {
                    matches!(
                        m,
                        Message::Keychain(crate::keychain::message::Message::KeychainFormSubmit)
                    )
                },
            ),
            (
                "identity form",
                |app| {
                    app.active_nav = NavItem::Keychain;
                    app.keychain.identity_form.visible = true;
                },
                |m| {
                    matches!(
                        m,
                        Message::Keychain(crate::keychain::message::Message::IdentityFormSubmit)
                    )
                },
            ),
            (
                "port forward form",
                |app| {
                    app.active_nav = NavItem::PortForwarding;
                    app.port_forwards.form.visible = true;
                },
                |m| {
                    matches!(
                        m,
                        Message::PortForward(
                            crate::port_forward::message::Message::PortForwardFormSubmit
                        )
                    )
                },
            ),
            (
                "snippet form",
                |app| {
                    app.active_nav = NavItem::Snippets;
                    app.snippets.form.visible = true;
                },
                |m| {
                    matches!(
                        m,
                        Message::Snippets(crate::snippets::message::Message::SnippetFormSubmit)
                    )
                },
            ),
            (
                "snippet package form",
                |app| {
                    app.active_nav = NavItem::Snippets;
                    app.snippets.package_form.visible = true;
                },
                |m| {
                    matches!(
                        m,
                        Message::Snippets(
                            crate::snippets::message::Message::SnippetPackageFormSubmit
                        )
                    )
                },
            ),
        ];

        for (what, set_up, expects) in cases {
            let mut app = app();
            set_up(&mut app);

            let carried = press(
                &mut app,
                named(Named::Enter),
                iced::keyboard::Modifiers::default(),
            );

            assert_answered(&carried, expects, what);
        }
    }

    #[test]
    fn the_workspace_dialogs_answer_enter_with_their_own_confirmation() {
        let cases: Vec<MessageCase> = vec![
            (
                "save",
                |app| app.workspace_manager.save_pending = true,
                |m| matches!(m, Message::WorkspaceSaveConfirm),
            ),
            (
                "apply",
                |app| app.show_apply_workspace_confirm = true,
                |m| matches!(m, Message::WorkspaceApplyConfirm),
            ),
            (
                "delete",
                |app| app.workspace_manager.delete_id = Some("web".to_string()),
                |m| matches!(m, Message::WorkspaceDeleteConfirm),
            ),
            (
                "rename",
                |app| app.workspace_manager.rename_id = Some("web".to_string()),
                |m| matches!(m, Message::WorkspaceRenameSubmit),
            ),
            (
                "recovery",
                |app| app.workspace_recovery.show_prompt = true,
                |m| {
                    matches!(
                        m,
                        Message::WorkspaceRecovery(
                            crate::workspace::recovery::Message::RestoreConfirm
                        )
                    )
                },
            ),
            (
                "quit",
                |app| app.show_quit_confirm = true,
                |m| matches!(m, Message::WindowCloseConfirm),
            ),
            (
                "locked tab close",
                |app| {
                    app.pending_locked_close = Some(crate::terminal::PendingLockedClose {
                        label: "web".to_string(),
                        target: crate::terminal::TerminalCloseTarget::Tab(SessionId(1)),
                    });
                },
                |m| {
                    matches!(
                        m,
                        Message::Terminal(
                            crate::terminal::message::Message::TermCloseLockedConfirm
                        )
                    )
                },
            ),
        ];

        for (what, set_up, expects) in cases {
            let mut app = app();
            set_up(&mut app);

            let carried = press(
                &mut app,
                named(Named::Enter),
                iced::keyboard::Modifiers::default(),
            );

            assert_answered(&carried, expects, what);
        }
    }

    #[test]
    fn enter_leaves_a_pending_save_conflict_for_its_own_buttons() {
        let mut app = app();
        app.workspace_manager.save_pending = true;
        app.workspace_manager.save_conflict = Some("web".to_string());

        let carried = press(
            &mut app,
            named(Named::Enter),
            iced::keyboard::Modifiers::default(),
        );

        assert!(
            carried.messages.is_empty(),
            "the overwrite-but-keep-a-copy choice is not one Enter can make: {:?}",
            carried.messages
        );
        assert!(app.workspace_manager.save_pending, "the dialog stays open");
    }

    #[test]
    fn escape_answers_a_waiting_password_prompt_by_closing_the_connection() {
        let mut app = app();
        let draft = with_draft(&mut app);
        app.terminal
            .drafts
            .get_mut(&draft)
            .unwrap()
            .ssh
            .attempt
            .state = SshConnectionState::WaitingForPassword;

        let carried = press(
            &mut app,
            named(Named::Escape),
            iced::keyboard::Modifiers::default(),
        );

        assert_answered(
            &carried,
            |m| {
                matches!(
                    m,
                    Message::SshConnect(crate::ssh_connect::message::Message::SshClose)
                )
            },
            "password prompt",
        );
    }

    #[test]
    fn enter_accepts_a_host_key_that_is_waiting_for_confirmation() {
        let mut app = app();
        let draft = with_draft(&mut app);
        app.terminal
            .drafts
            .get_mut(&draft)
            .unwrap()
            .ssh
            .attempt
            .state = SshConnectionState::WaitingForKeyConfirmation;

        let carried = press(
            &mut app,
            named(Named::Enter),
            iced::keyboard::Modifiers::default(),
        );

        assert_answered(
            &carried,
            |m| {
                matches!(
                    m,
                    Message::SshConnect(crate::ssh_connect::message::Message::SshKeyConfirmAccept)
                )
            },
            "new-tab host key prompt",
        );
    }

    #[test]
    fn enter_accepts_the_sftp_host_key_and_prefers_the_local_pane() {
        let fresh = crate::core::test_support::app;

        for (pane, local) in [("local", true), ("remote", false)] {
            let mut app = fresh();
            app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
            if local {
                app.sftp.state.left.pane.attempt.state =
                    SshConnectionState::WaitingForKeyConfirmation;
            } else {
                app.sftp.state.right.pane.attempt.state =
                    SshConnectionState::WaitingForKeyConfirmation;
            }

            let carried = press(
                &mut app,
                named(Named::Enter),
                iced::keyboard::Modifiers::default(),
            );

            assert_answered(
                &carried,
                |m| {
                    let expected = if local {
                        crate::sftp::SftpPane::Local
                    } else {
                        crate::sftp::SftpPane::Remote
                    };
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpKeyConfirmAccept {
                            pane
                        }) if *pane == expected
                    )
                },
                pane,
            );
        }

        // Both panes waiting at once: the local one is drawn last, so it wins.
        let mut app = fresh();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.state.left.pane.attempt.state = SshConnectionState::WaitingForKeyConfirmation;
        app.sftp.state.right.pane.attempt.state = SshConnectionState::WaitingForKeyConfirmation;

        let carried = press(
            &mut app,
            named(Named::Enter),
            iced::keyboard::Modifiers::default(),
        );

        assert_answered(
            &carried,
            |m| {
                matches!(
                    m,
                    Message::Sftp(crate::sftp::message::Message::SftpKeyConfirmAccept {
                        pane: crate::sftp::SftpPane::Local
                    })
                )
            },
            "both panes waiting",
        );
    }

    #[test]
    fn a_transfer_prompt_is_answered_by_the_button_the_modal_shows() {
        let pending = crate::sftp::SftpOverwritePending {
            host: crate::core::test_support::host(1, "web", "10.0.0.1"),
            file_name: "notes.txt".to_string(),
            remote_path: "/srv/app/notes.txt".to_string(),
            source_path: "/srv/app/notes.txt".to_string(),
            local_path: None,
            source_host: None,
            total_bytes: 2048,
            target: crate::sftp::SftpTransferTarget::Local {
                directory: "/tmp/downloads".to_string(),
                name: "notes.txt".to_string(),
            },
            in_progress: false,
            is_folder: false,
        };

        let cases: Vec<ModalCase> = vec![
            (
                "discard",
                crate::sftp::SftpTransferModal::Discard {
                    file_name: "notes.txt".to_string(),
                    queued_overwrite: None,
                },
                |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpCancelTransferCancel)
                    )
                },
            ),
            (
                "overwrite",
                crate::sftp::SftpTransferModal::Overwrite(pending),
                |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpOverwriteCancel)
                    )
                },
            ),
        ];

        for (what, modal, expects) in cases {
            let mut app = crate::core::test_support::app();
            app.sftp.state.transfer_modal = Some(modal.clone());
            let carried = press(
                &mut app,
                named(Named::Escape),
                iced::keyboard::Modifiers::default(),
            );
            assert_answered(&carried, expects, what);

            let mut app = crate::core::test_support::app();
            app.sftp.state.transfer_modal = Some(modal);
            let carried = press(
                &mut app,
                named(Named::Enter),
                iced::keyboard::Modifiers::default(),
            );
            let expects_confirm: fn(&Message) -> bool = match what {
                "discard" => |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpCancelTransfer)
                    )
                },
                "overwrite" => |m| {
                    matches!(
                        m,
                        Message::Sftp(crate::sftp::message::Message::SftpOverwriteReplace)
                    )
                },
                _ => |_| false,
            };
            assert_answered(&carried, expects_confirm, what);
        }
    }

    #[test]
    fn escape_clears_an_open_sftp_overlay_without_a_round_trip() {
        let cases: Vec<StateCase> = vec![
            (
                "preview",
                |app| {
                    app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
                    app.sftp.state.preview = Some(crate::sftp::SftpPreview {
                        source: crate::sftp::SftpPreviewSource::Local,
                        directory: "/tmp".to_string(),
                        file_name: "notes.txt".to_string(),
                        location_label: "local".to_string(),
                        state: crate::sftp::SftpPreviewState::Loading,
                    });
                },
                |app| app.sftp.state.preview.is_none(),
            ),
            (
                "context menu",
                |app| {
                    app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
                    app.sftp.state.context_menu = None;
                },
                |app| app.sftp.state.context_menu.is_none(),
            ),
        ];

        for (what, set_up, cleared) in cases {
            let mut app = app();
            set_up(&mut app);
            let carried = press(
                &mut app,
                named(Named::Escape),
                iced::keyboard::Modifiers::default(),
            );

            assert!(
                carried.messages.is_empty(),
                "{what}: {:?}",
                carried.messages
            );
            assert!(cleared(&app), "{what} is still open");
        }
    }

    #[test]
    fn escape_plays_the_slide_out_animation_instead_of_closing_a_form_directly() {
        let cases: Vec<StateCase> = vec![
            (
                "keychain form",
                |app| {
                    app.active_nav = NavItem::Keychain;
                    app.keychain.form.visible = true;
                },
                |app| app.keychain.form_closing,
            ),
            (
                "port forward form",
                |app| {
                    app.active_nav = NavItem::PortForwarding;
                    app.port_forwards.form.visible = true;
                },
                |app| app.port_forwards.form_closing,
            ),
            (
                "snippet form",
                |app| {
                    app.active_nav = NavItem::Snippets;
                    app.snippets.form.visible = true;
                },
                |app| app.snippets.form_closing,
            ),
            (
                "snippet package form",
                |app| {
                    app.active_nav = NavItem::Snippets;
                    app.snippets.package_form.visible = true;
                },
                |app| app.snippets.form_closing,
            ),
            (
                "known host detail",
                |app| {
                    app.active_nav = NavItem::KnownHosts;
                    app.known_hosts.cursor.select(3);
                    app.known_hosts.detail_open = true;
                },
                |app| app.known_hosts.detail_closing,
            ),
        ];

        for (what, set_up, closing) in cases {
            let mut app = app();
            set_up(&mut app);
            let carried = press(
                &mut app,
                named(Named::Escape),
                iced::keyboard::Modifiers::default(),
            );

            assert!(
                carried.messages.is_empty(),
                "{what}: {:?}",
                carried.messages
            );
            assert!(closing(&app), "{what} was not animated shut");
        }
    }

    #[test]
    fn escape_drops_the_quit_and_crash_dialogs_in_place() {
        for (what, set_up, gone) in [
            (
                "quit",
                (|app: &mut App| app.show_quit_confirm = true) as fn(&mut App),
                (|app: &App| !app.show_quit_confirm) as fn(&App) -> bool,
            ),
            (
                "crash",
                |app: &mut App| app.show_crash_dialog = true,
                |app: &App| !app.show_crash_dialog,
            ),
            (
                "locked close",
                |app: &mut App| {
                    app.pending_locked_close = Some(crate::terminal::PendingLockedClose {
                        label: "web".to_string(),
                        target: crate::terminal::TerminalCloseTarget::Tab(SessionId(1)),
                    });
                },
                |app: &App| app.pending_locked_close.is_none(),
            ),
            (
                "host transfer dialog",
                |app: &mut App| {
                    app.hosts.transfer_dialog = Some(crate::hosts::state::HostTransferDialog {
                        kind: crate::hosts::state::HostTransferKind::Export,
                        password: secrecy::SecretString::from(String::new()),
                        password_visible: false,
                        busy: false,
                        anim_t: 0.0,
                        pending_path: None,
                        pending_bytes: Vec::new(),
                        pending_hosts: Vec::new(),
                        new_count: 0,
                        dup_count: 0,
                        error: None,
                    });
                },
                |app: &App| app.hosts.transfer_dialog.is_none(),
            ),
        ] {
            let mut app = app();
            set_up(&mut app);
            let carried = press(
                &mut app,
                named(Named::Escape),
                iced::keyboard::Modifiers::default(),
            );

            assert!(
                carried.messages.is_empty(),
                "{what}: {:?}",
                carried.messages
            );
            assert!(gone(&app), "{what} is still open");
        }
    }

    #[test]
    fn escape_with_no_overlay_open_is_ignored() {
        let mut app = app();

        let carried = press(
            &mut app,
            named(Named::Escape),
            iced::keyboard::Modifiers::default(),
        );

        assert!(carried.messages.is_empty());
        assert_eq!(carried.other_actions, 0);
    }

    #[test]
    fn escape_cancels_an_inline_tab_rename_before_anything_behind_it() {
        let mut app = app();
        let tab = app.allocate_tab_id(TabKind::Session(SessionId(1)));
        app.activate_tab(tab);
        app.terminal.editing_tab = Some(SessionId(1));
        app.terminal.editing_tab_text = "web".to_string();

        let carried = press(
            &mut app,
            named(Named::Escape),
            iced::keyboard::Modifiers::default(),
        );

        assert_answered(
            &carried,
            |m| {
                matches!(
                    m,
                    Message::Terminal(crate::terminal::message::Message::TermRenameCancel)
                )
            },
            "inline rename",
        );
    }

    #[test]
    fn tab_moves_focus_through_the_host_form_and_nowhere_else() {
        let fresh = crate::core::test_support::app;
        let mut app = fresh();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);
        app.hosts.form.visible = true;
        let _ = crate::focus::sync(&mut app);

        let forward = press(
            &mut app,
            named(Named::Tab),
            iced::keyboard::Modifiers::default(),
        );
        assert!(forward.messages.is_empty());
        assert_eq!(forward.other_actions, 1, "Tab steps to the next field");

        let backward = press(
            &mut app,
            named(Named::Tab),
            iced::keyboard::Modifiers::SHIFT,
        );
        assert!(backward.messages.is_empty());
        assert_eq!(backward.other_actions, 1, "Shift+Tab steps back");

        // With the form closed, Tab means nothing here: the terminal widget
        // keeps it.
        let mut app = fresh();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);
        let _ = crate::focus::sync(&mut app);
        let elsewhere = press(
            &mut app,
            named(Named::Tab),
            iced::keyboard::Modifiers::default(),
        );
        assert!(elsewhere.messages.is_empty());
        assert_eq!(elsewhere.other_actions, 0);
    }

    #[test]
    fn a_modal_blocks_tab_navigation_in_the_host_form() {
        let mut app = crate::core::test_support::app();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);
        app.hosts.form.visible = true;
        app.show_quit_confirm = true;

        let carried = press(
            &mut app,
            named(Named::Tab),
            iced::keyboard::Modifiers::default(),
        );

        assert!(carried.messages.is_empty());
        assert_eq!(
            carried.other_actions, 0,
            "the modal owns Tab instead of the form behind it"
        );
    }

    #[test]
    fn collection_keys_are_routed_by_the_focus_stack() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];
        app.hosts.cursor.select(HostId(1));
        let _ = update(&mut app, Message::CursorFocus(CursorTarget::Hosts));

        let activated = press(
            &mut app,
            named(Named::Enter),
            iced::keyboard::Modifiers::empty(),
        );

        assert_answered(
            &activated,
            |message| {
                matches!(
                    message,
                    Message::Hosts(crate::hosts::message::Message::ConnectHost(HostId(1)))
                )
            },
            "the stack routes Enter to the active collection",
        );
    }

    #[test]
    fn the_arrow_keys_walk_the_new_tab_search_results() {
        let mut app = app();
        with_draft(&mut app);

        let down = press(
            &mut app,
            named(Named::ArrowDown),
            iced::keyboard::Modifiers::default(),
        );
        assert_answered(
            &down,
            |m| {
                matches!(
                    m,
                    Message::Hosts(crate::hosts::message::Message::NewTabSelectDown)
                )
            },
            "ArrowDown",
        );

        let up = press(
            &mut app,
            named(Named::ArrowUp),
            iced::keyboard::Modifiers::default(),
        );
        assert_answered(
            &up,
            |m| {
                matches!(
                    m,
                    Message::Hosts(crate::hosts::message::Message::NewTabSelectUp)
                )
            },
            "ArrowUp",
        );
    }

    #[test]
    fn the_arrow_keys_walk_the_sftp_host_picker() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.state.right.host_picker.open = true;

        let down = press(
            &mut app,
            named(Named::ArrowDown),
            iced::keyboard::Modifiers::default(),
        );
        assert_answered(
            &down,
            |m| {
                matches!(
                    m,
                    Message::CursorMove {
                        target: CursorTarget::SftpRemoteHostPicker,
                        direction: GridDirection::Down,
                        extend: false,
                    }
                )
            },
            "SFTP host picker ArrowDown",
        );

        let up = press(
            &mut app,
            named(Named::ArrowUp),
            iced::keyboard::Modifiers::default(),
        );
        assert_answered(
            &up,
            |m| {
                matches!(
                    m,
                    Message::CursorMove {
                        target: CursorTarget::SftpRemoteHostPicker,
                        direction: GridDirection::Up,
                        extend: false,
                    }
                )
            },
            "SFTP host picker ArrowUp",
        );
    }

    #[test]
    fn the_sftp_host_picker_does_not_take_keys_from_other_workspaces() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);
        app.sftp.state.right.host_picker.open = true;

        let down = press(
            &mut app,
            named(Named::ArrowDown),
            iced::keyboard::Modifiers::default(),
        );

        assert!(
            down.messages.iter().all(|message| !matches!(
                message,
                Message::CursorMove {
                    target: CursorTarget::SftpLocalHostPicker | CursorTarget::SftpRemoteHostPicker,
                    ..
                }
            )),
            "an invisible picker must not consume arrow keys: {:?}",
            down.messages
        );
    }

    #[test]
    fn the_font_size_shortcuts_step_the_terminal_font() {
        let mut app = app();
        let start = app.preferences.terminal_font_size;

        let bigger = press(
            &mut app,
            iced::keyboard::Key::Character("=".into()),
            iced::keyboard::Modifiers::COMMAND,
        );
        assert_answered(
            &bigger,
            |m| matches!(m, Message::SetTerminalFontSize(size) if *size == start + 1.0),
            "zoom in",
        );

        let smaller = press(
            &mut app,
            iced::keyboard::Key::Character("-".into()),
            iced::keyboard::Modifiers::COMMAND,
        );
        assert_answered(
            &smaller,
            |m| matches!(m, Message::SetTerminalFontSize(size) if *size == start - 1.0),
            "zoom out",
        );
    }

    #[test]
    fn the_new_tab_shortcut_reaches_the_hosts_module() {
        let mut app = app();

        let carried = press(
            &mut app,
            iced::keyboard::Key::Character("n".into()),
            iced::keyboard::Modifiers::COMMAND,
        );

        assert_answered(
            &carried,
            |m| {
                matches!(
                    m,
                    Message::Hosts(crate::hosts::message::Message::OpenNewTab)
                )
            },
            "new tab",
        );
    }

    #[test]
    fn shortcuts_that_need_a_session_do_nothing_without_one() {
        for key in ["f", "d"] {
            let mut app = app();
            let carried = press(
                &mut app,
                iced::keyboard::Key::Character(key.into()),
                iced::keyboard::Modifiers::COMMAND,
            );

            assert!(
                carried.messages.is_empty(),
                "Ctrl+{key} has no session to act on: {:?}",
                carried.messages
            );
            assert_eq!(carried.other_actions, 0);
        }
    }

    #[test]
    fn closing_the_last_tab_asks_to_quit_and_a_dialog_blocks_it() {
        let fresh = crate::core::test_support::app;
        let mut app = fresh();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);

        let carried = press(
            &mut app,
            iced::keyboard::Key::Character("w".into()),
            iced::keyboard::Modifiers::COMMAND,
        );
        assert!(carried.messages.is_empty());
        assert!(
            app.show_quit_confirm,
            "closing the last tab closes the window"
        );

        // With something already modal on top, the shortcut must not stack a
        // second confirmation behind it.
        let mut app = fresh();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);
        app.workspace_manager.save_pending = true;

        let carried = press(
            &mut app,
            iced::keyboard::Key::Character("w".into()),
            iced::keyboard::Modifiers::COMMAND,
        );
        assert!(carried.messages.is_empty());
        assert!(!app.show_quit_confirm);
    }

    #[test]
    fn a_modifier_that_matches_no_binding_is_dropped() {
        let mut app = app();

        // Shift+Escape is not the Escape every overlay above listens for.
        let carried = press(
            &mut app,
            named(Named::Escape),
            iced::keyboard::Modifiers::SHIFT,
        );
        assert!(carried.messages.is_empty());
        assert!(app.terminal.drafts.is_empty());
    }

    #[test]
    fn a_session_working_directory_prefers_what_the_shell_just_reported() {
        crate::core::test_support::prepare_data_dir();
        let host_id = HostId(31);
        let cache = crate::core::paths::cwd_cache_file(host_id);
        std::fs::create_dir_all(cache.parent().unwrap()).expect("the cwd cache directory exists");

        // Nothing reported yet, and nothing cached: there is no directory to
        // offer, and guessing one would be worse than offering none.
        assert_eq!(extract_cwd_from_terminal(Some(host_id), None), None);
        assert_eq!(extract_cwd_from_terminal(None, None), None);

        // The bridge's cache file is the fallback for a session whose OSC 7
        // has not arrived yet.
        std::fs::write(&cache, "/srv/app\n").expect("write the cache file");
        assert_eq!(
            extract_cwd_from_terminal(Some(host_id), None),
            Some("/srv/app".to_string()),
            "the cached directory comes back trimmed"
        );

        assert_eq!(
            extract_cwd_from_terminal(Some(host_id), Some("/srv/other")),
            Some("/srv/other".to_string()),
            "what the session reports now wins over what was cached earlier"
        );

        // A session that has reported nothing yet falls through to the file
        // rather than treating an empty string as a directory.
        assert_eq!(
            extract_cwd_from_terminal(Some(host_id), Some("")),
            Some("/srv/app".to_string())
        );

        // A cache file that is empty or blank says nothing useful either.
        std::fs::write(&cache, "   \n").expect("blank the cache file");
        assert_eq!(extract_cwd_from_terminal(Some(host_id), None), None);

        // A local session has no host to key a cache file by.
        std::fs::write(&cache, "/srv/app\n").expect("write the cache file again");
        assert_eq!(extract_cwd_from_terminal(None, None), None);
    }
}
