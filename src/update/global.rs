//! Cursor, scroll, startup, and clipboard updates.

use std::fmt::Write as _;

use iced::Task;
use iced::mouse;
use iced::widget::operation::AbsoluteOffset;
use iced::widget::{Id, operation};

use super::{App, InitialAction, Message, ScrollbarState, scroll_to_active_tab};
use crate::core::data::LogCategory;
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::CursorFocus(target) => {
            crate::focus::focus_base(app, crate::focus::FocusId::Cursor(target))
        }
        Message::CursorMove {
            target,
            direction,
            extend,
        } => crate::cursor::move_cursor_with_extend(app, target, direction, extend),
        Message::CursorColumns(target, columns) => crate::cursor::set_columns(app, target, columns),
        Message::CursorActivate(target) => crate::cursor::activate(app, target),
        Message::ObserveFocus => {
            iced_runtime::task::widget(iced::advanced::widget::operation::focusable::find_focused())
                .map(Message::FocusedWidget)
        }
        Message::FocusedWidget(id) => crate::focus::adopt_widget(app, &id),
        Message::Scrolled(id, viewport) => {
            app.scroll_offsets.insert(id, viewport.absolute_offset().y);
            app.scroll_viewport_heights
                .insert(id, viewport.bounds().height);
            // Tab-strip scroll state drives the edge fade masks in the top bar.
            let mut deferred_tab_scroll = false;
            if id == crate::views::topbar::TOP_TABS_SCROLL_ID {
                app.top_tabs_scroll_offset = viewport.relative_offset().x;
                app.top_tabs_scroll_absolute_x = viewport.absolute_offset().x;
                app.top_tabs_can_scroll =
                    viewport.content_bounds().width > viewport.bounds().width + 1.0;
                app.top_tabs_viewport_width = viewport.bounds().width;
                if app.top_tabs_pending_scroll {
                    // Retry with this fresh viewport; the flag stays armed until
                    // the tab shows.
                    deferred_tab_scroll = true;
                }
            }
            let state = app
                .scrollbar_states
                .entry(id)
                .or_insert_with(ScrollbarState::new);
            state.visible = true;
            state.last_active = std::time::Instant::now();
            if deferred_tab_scroll {
                scroll_to_active_tab(app)
            } else {
                Task::none()
            }
        }
        Message::TopTabsWheel(scroll_delta) => {
            // The tab strip scrolls horizontally only, so the built-in wheel
            // handler drops the y-component; intercept the event and scroll by
            // the combined delta instead.
            let delta_x = match scroll_delta {
                mouse::ScrollDelta::Lines { x, y } => -(x + y) * 60.0,
                mouse::ScrollDelta::Pixels { x, y } => -(x + y),
            };
            operation::scroll_by(
                Id::from(crate::views::topbar::TOP_TABS_SCROLL_ID),
                AbsoluteOffset { x: delta_x, y: 0.0 },
            )
        }
        Message::StartupDataLoaded(data) => {
            app.hosts.list = data.hosts;
            app.hosts.recent_ids = data.recent_ids;
            app.hosts_loading = false;
            app.port_forwards.list = data.port_forwards;
            app.snippets.list = data.snippets;
            app.snippets.packages = data.snippet_packages;
            app.keychain.list = data.keychain_entries;
            app.keychain.identities = data.identities;
            for err in data.errors {
                app.add_log(LogCategory::System, format!("Startup storage error: {err}"));
            }
            // Offered only after the host list loads, so `restore_workspace`
            // can resolve SSH sessions.
            crate::workspace::recovery::hosts_loaded(app);
            let keychain_fallback_task = if crate::core::crypto::keyring_fell_back() {
                Task::done(Message::ShowWarningToast(
                    t!("crypto.keychain_fallback").to_string(),
                ))
            } else {
                Task::none()
            };
            let initial = std::mem::replace(&mut app.pending_initial_action, InitialAction::None);
            let initial_task = match initial {
                InitialAction::None | InitialAction::NewWindow => Task::none(),
                InitialAction::QuickSsh(host_id) => Task::done(Message::Terminal(
                    crate::terminal::message::Message::StartupOpenSsh(host_id),
                )),
                InitialAction::QuickLocal => Task::done(Message::Terminal(
                    crate::terminal::message::Message::StartupOpenLocal,
                )),
            };
            Task::batch([initial_task, keychain_fallback_task])
        }
        Message::ScrollbarVisibilityTimer => {
            let hovered = crate::core::widgets::SCROLLBAR_HOVERED.get();
            let now = std::time::Instant::now();
            for state in app.scrollbar_states.values_mut() {
                if state.visible
                    && !hovered
                    && now.duration_since(state.last_active)
                        > std::time::Duration::from_millis(1500)
                {
                    state.visible = false;
                }
            }
            crate::core::widgets::SCROLLBAR_HOVERED.set(false);
            Task::none()
        }
        Message::CrashDialogClose => {
            app.show_crash_dialog = false;
            app.crash_reports.clear();
            // Surface the recovery prompt only after the crash report has been
            // dismissed so the two modals never stack.
            crate::workspace::recovery::crash_dialog_closed(app);
            Task::none()
        }
        Message::CrashDialogCopy => {
            let mut text = String::new();
            for (i, (_, content)) in app.crash_reports.iter().enumerate() {
                if i > 0 {
                    text.push_str("\n\n═══════════════════════════════════\n\n");
                }
                text.push_str(content);
            }
            if text.is_empty() {
                Task::none()
            } else {
                Task::batch([
                    iced::clipboard::write(text),
                    Task::done(Message::ShowToast(t!("app.report_copied").to_string())),
                ])
            }
        }
        Message::CopyLogs => {
            let mut text = String::new();
            for entry in &app.logs {
                let _ = writeln!(
                    text,
                    "{} [{}] {}",
                    crate::core::time::format_timestamp(entry.timestamp),
                    entry.category.label(),
                    entry.message
                );
            }
            if text.is_empty() {
                Task::none()
            } else {
                Task::batch([
                    iced::clipboard::write(text),
                    Task::done(Message::ShowToast(t!("app.logs_copied").to_string())),
                ])
            }
        }
        Message::Noop => Task::none(),
        _ => unreachable!("global handler received another message"),
    }
}
