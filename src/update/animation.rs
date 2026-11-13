//! Per-frame animation and polling updates.

use iced::Task;

use super::{ActiveTab, App, Message, SshConnectionState};
use super::{animate_right_sidebar, take_pending_right_sidebar_focus};
use crate::core::cursor_focus::CursorTarget;

pub(super) fn tick(app: &mut App) -> Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        crate::core::diagnostics::Level::Debug,
        "ANIM",
        "AnimTick",
        std::time::Duration::from_millis(8),
    );
    // Heartbeat every ~5 s so gaps in the diagnostic log reveal freezes.
    {
        let frame = crate::core::diagnostics::next_frame_id();
        if frame.is_multiple_of(300) {
            crate::core::diagnostics::log(
                crate::core::diagnostics::Level::Trace,
                "HEARTBEAT",
                format!("frame={frame}"),
            );
        }
    }
    let mut task = Task::none();
    crate::hosts::update::tick_animations(app);
    for split in app.terminal.splits.values() {
        for (_, sid) in split.panes.iter() {
            if let Some(session) = app.terminal.sessions.iter_mut().find(|s| s.id == *sid) {
                animate_right_sidebar(&mut session.sidebar);
            }
        }
    }
    // A session outside any split grid animates its own sidebar.
    if !app.is_active_session_in_split()
        && let Some(sidebar) = app.sidebar_mut()
    {
        animate_right_sidebar(sidebar);
    }
    let sidebar_focus = app.sidebar_mut().and_then(take_pending_right_sidebar_focus);
    if let Some(tab) = sidebar_focus {
        task = task.chain(crate::focus::focus_base(
            app,
            crate::focus::FocusId::Cursor(CursorTarget::for_right_sidebar(tab)),
        ));
    }
    if app.sftp.sidebar_closing {
        crate::ui::helpers::tick_slide_anim(&mut app.sftp.sidebar_anim, true, 0.08);
        if app.sftp.sidebar_anim <= 0.0 {
            app.sftp.sidebar_show = false;
            app.sftp.sidebar_closing = false;
        }
    } else if app.sftp.sidebar_show {
        crate::ui::helpers::tick_slide_anim(&mut app.sftp.sidebar_anim, false, 0.08);
    }
    if app.has_any_dialog_open() {
        crate::ui::helpers::tick_slide_anim(&mut app.confirm_dialog_anim, false, 0.16);
    } else {
        app.confirm_dialog_anim = 0.0;
    }
    for session in &mut app.terminal.sessions {
        if let Some(popup) = &mut session.spotlight {
            crate::ui::helpers::tick_slide_anim(&mut popup.anim, false, 0.12);
        }
    }
    for session in &mut app.terminal.sessions {
        if crate::terminal::state::disconnect_overlay_visible(session) {
            crate::ui::helpers::tick_slide_anim(&mut session.disconnect_anim, false, 0.16);
        } else if session.disconnect_anim > 0.0 {
            session.disconnect_anim = 0.0;
        }
    }
    for session in &mut app.terminal.sessions {
        if session.search_closing {
            crate::ui::helpers::tick_slide_anim(&mut session.search_anim, true, 0.16);
            if session.search_anim <= 0.0 {
                session.show_search = false;
                session.search_closing = false;
                session.search_matches_count = 0;
                session.search_index = 0;
                session.terminal.clear_selection();
            }
        } else if session.show_search {
            crate::ui::helpers::tick_slide_anim(&mut session.search_anim, false, 0.16);
        }
    }
    if app.keychain.form_closing {
        crate::ui::helpers::tick_slide_anim(
            &mut app.keychain.form_anim,
            true,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
        if app.keychain.form_anim <= 0.0 {
            app.keychain.form.visible = false;
            app.keychain.identity_form.visible = false;
            app.keychain.form_closing = false;
            app.keychain.form.reset();
            app.needs_scroll_restore = true;
        }
    } else if app.keychain.form.visible || app.keychain.identity_form.visible {
        crate::ui::helpers::tick_slide_anim(
            &mut app.keychain.form_anim,
            false,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
    }
    if app.port_forwards.form_closing {
        crate::ui::helpers::tick_slide_anim(
            &mut app.port_forwards.form_anim,
            true,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
        if app.port_forwards.form_anim <= 0.0 {
            app.port_forwards.form.visible = false;
            app.port_forwards.form_closing = false;
            app.port_forwards.form.reset();
            app.needs_scroll_restore = true;
        }
    } else if app.port_forwards.form.visible {
        crate::ui::helpers::tick_slide_anim(
            &mut app.port_forwards.form_anim,
            false,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
    }
    if app.known_hosts.detail_closing {
        crate::ui::helpers::tick_slide_anim(
            &mut app.known_hosts.detail_anim,
            true,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
        if app.known_hosts.detail_anim <= 0.0 {
            app.known_hosts.detail_open = false;
            app.known_hosts.detail_closing = false;
        }
    } else if app.known_hosts.detail_open {
        crate::ui::helpers::tick_slide_anim(
            &mut app.known_hosts.detail_anim,
            false,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
    }
    if app.snippets.form_closing {
        crate::ui::helpers::tick_slide_anim(
            &mut app.snippets.form_anim,
            true,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
        if app.snippets.form_anim <= 0.0 {
            app.snippets.form.visible = false;
            app.snippets.package_form.visible = false;
            app.snippets.form_closing = false;
            app.snippets.form.reset();
            app.snippets.package_form.reset();
            app.needs_scroll_restore = true;
        }
    } else if app.snippets.form.visible || app.snippets.package_form.visible {
        crate::ui::helpers::tick_slide_anim(
            &mut app.snippets.form_anim,
            false,
            crate::ui::helpers::SLIDE_PANEL_ANIM_STEP,
        );
    }
    if app.toast_closing {
        crate::ui::helpers::tick_slide_anim(&mut app.toast_anim, true, 0.08);
        if app.toast_anim <= 0.0 {
            app.toast = None;
            app.toast_closing = false;
        }
    } else if app.toast.is_some() {
        crate::ui::helpers::tick_slide_anim(&mut app.toast_anim, false, 0.08);
    }
    if app.command_frequency_prompt_closing {
        crate::ui::helpers::tick_slide_anim(&mut app.command_frequency_prompt_anim, true, 0.08);
        if app.command_frequency_prompt_anim <= 0.0 {
            app.command_frequency_prompt = None;
            app.command_frequency_prompt_closing = false;
        }
    } else if app.command_frequency_prompt.is_some() {
        crate::ui::helpers::tick_slide_anim(&mut app.command_frequency_prompt_anim, false, 0.08);
    }
    for tid in app.visible_tab_ids() {
        let is_active = app.active_tab == ActiveTab::Tab(tid);
        let target = if is_active {
            (crate::views::TAB_MIN_WIDTH + 40.0).min(crate::views::MAX_TAB_WIDTH)
        } else {
            crate::views::TAB_MIN_WIDTH
        };
        let current = app.terminal.tab_width_anims.entry(tid).or_insert(target);
        let diff = target - *current;
        if diff.abs() < 0.5 {
            *current = target;
        } else {
            *current += diff * 0.44;
        }
    }
    app.terminal
        .tab_width_anims
        .retain(|&tid, _| app.terminal.tab_registry.contains_key(&tid));
    for draft in app.terminal.drafts.values_mut() {
        if draft.ssh.attempt.state == SshConnectionState::Connecting {
            draft.ssh.anim_t += 0.016;
        }
    }
    if app.sftp.state.right.pane.loading
        || app.sftp.state.left.pane.loading
        || app.sftp.state.transfer_progress.is_some()
        || app
            .sftp
            .state
            .preview
            .as_ref()
            .is_some_and(|p| matches!(p.state, crate::sftp::SftpPreviewState::Loading))
    {
        app.sftp.state.anim_t += 0.016;
    }
    if !app.sftp.state.external_drag_paths.is_empty() {
        // External drags do not reliably produce CursorMoved events
        // (Windows OLE runs its own drag loop), so poll the real
        // pointer while a drop is in flight.
        let point = crate::sftp::update::external_file_drag_point(app);
        task = task.chain(crate::sftp::update::external_drag_cursor_moved(app, point));
    }
    let step = 0.18;
    let local_target = if app.sftp.state.left.pane.filter_open {
        1.0
    } else {
        0.0
    };
    if app.sftp.state.left.pane.filter_anim != local_target {
        if app.sftp.state.left.pane.filter_anim < local_target {
            app.sftp.state.left.pane.filter_anim =
                (app.sftp.state.left.pane.filter_anim + step).min(local_target);
        } else {
            app.sftp.state.left.pane.filter_anim =
                (app.sftp.state.left.pane.filter_anim - step).max(local_target);
        }
    }
    let remote_target = if app.sftp.state.right.pane.filter_open {
        1.0
    } else {
        0.0
    };
    if app.sftp.state.right.pane.filter_anim != remote_target {
        if app.sftp.state.right.pane.filter_anim < remote_target {
            app.sftp.state.right.pane.filter_anim =
                (app.sftp.state.right.pane.filter_anim + step).min(remote_target);
        } else {
            app.sftp.state.right.pane.filter_anim =
                (app.sftp.state.right.pane.filter_anim - step).max(remote_target);
        }
    }
    task
}
