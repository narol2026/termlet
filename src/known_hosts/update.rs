use super::message::Message as DomainMessage;
use crate::app::App;
use crate::core::data::LogCategory;
use crate::core::diagnostics::Level;
use crate::core::message::Message;
use crate::ssh::known_hosts_file;
use rust_i18n::t;

use crate::update::restore_scrolls;

use super::state::KnownHostContextMenu;

/// Load `~/.ssh/known_hosts` off the UI thread and deliver the parsed list.
pub fn reload_known_hosts_task() -> iced::Task<Message> {
    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(known_hosts_file::load_all)
                .await
                .unwrap_or_else(|join_err| {
                    Err(format!("known_hosts load thread panicked: {join_err}"))
                })
        },
        |v0| Message::KnownHosts(crate::known_hosts::message::Message::KnownHostsLoaded(v0)),
    )
}

pub fn update(app: &mut App, msg: DomainMessage) -> iced::Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "known_hosts::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        DomainMessage::KnownHostsLoaded(result) => {
            match result {
                Ok(list) => app.known_hosts.entries = list,
                Err(err) => {
                    crate::core::diagnostics::log(Level::Warn, "KNOWN-HOSTS", &err);
                    app.add_log(LogCategory::KnownHost, err);
                }
            }
            iced::Task::none()
        }
        DomainMessage::KnownHostDeleteRequest(id) => {
            app.known_hosts.context_menu = None;
            app.known_hosts.delete_pending = Some(id);
            restore_scrolls(app)
        }
        DomainMessage::KnownHostDeleteCancel => {
            app.known_hosts.delete_pending = None;
            restore_scrolls(app)
        }
        DomainMessage::KnownHostDelete(id) => {
            let entry = app
                .known_hosts
                .entries
                .iter()
                .find(|kh| kh.line_number == id)
                .cloned();
            let Some(entry) = entry else {
                app.known_hosts.delete_pending = None;
                return iced::Task::none();
            };
            let hostname = entry.hostname.clone();

            // For hashed entries we can't derive the real hostname from the
            // displayed `(hashed)` label, so delete only that exact line. For
            // plain entries, remove every matching entry (including hashed
            // duplicates for the same host) in one operation.
            let result = if entry.hostname == "(hashed)" {
                known_hosts_file::delete_line(id).map(|()| vec![id])
            } else {
                known_hosts_file::delete_host(&entry.hostname, entry.port)
            };

            match result {
                Ok(deleted_lines) => {
                    app.known_hosts
                        .entries
                        .retain(|kh| !deleted_lines.contains(&kh.line_number));
                    if app
                        .known_hosts
                        .cursor
                        .selected_key()
                        .is_some_and(|selected| deleted_lines.contains(selected))
                    {
                        app.known_hosts.cursor.clear();
                        app.known_hosts.detail_open = false;
                        app.known_hosts.detail_closing = false;
                    }
                    app.known_hosts.delete_pending = None;
                    app.add_log(
                        LogCategory::KnownHost,
                        t!("known_hosts.deleted_log", hostname = &hostname).to_string(),
                    );
                    restore_scrolls(app)
                }
                Err(err) => {
                    app.known_hosts.delete_pending = None;
                    app.add_log(
                        LogCategory::KnownHost,
                        format!("failed to delete known_hosts line {id}: {err}"),
                    );
                    iced::Task::batch([
                        iced::Task::done(Message::ShowErrorToast(err)),
                        restore_scrolls(app),
                    ])
                }
            }
        }
        DomainMessage::KnownHostSearchChanged(query) => {
            app.known_hosts.search = query;
            iced::Task::none()
        }
        DomainMessage::KnownHostToggleView => {
            app.known_hosts.grid_view = !app.known_hosts.grid_view;
            let value = if app.known_hosts.grid_view {
                "true"
            } else {
                "false"
            };
            app.storage_write_task(move |s| s.save_preference("known_hosts_grid_view", value))
        }
        DomainMessage::KnownHostHoverEnter(id) => {
            app.known_hosts.hovered_id = Some(id);
            iced::Task::none()
        }
        DomainMessage::KnownHostSelect(id) => {
            activate_known_host(app, id);
            iced::Task::none()
        }
        DomainMessage::KnownHostHoverLeave(id) => {
            if app.known_hosts.hovered_id == Some(id) {
                app.known_hosts.hovered_id = None;
            }
            iced::Task::none()
        }
        DomainMessage::KnownHostContextMenuOpen(idx) => {
            activate_known_host(app, idx);
            app.known_hosts.context_menu = Some(KnownHostContextMenu {
                id: idx,
                position: app.window.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::KnownHostDetails,
                    crate::core::context_menu::ContextMenuAction::KnownHostDelete,
                ]),
            });
            iced::Task::none()
        }
        DomainMessage::KnownHostContextMenuClose => {
            app.known_hosts.context_menu = None;
            iced::Task::none()
        }
        DomainMessage::KnownHostShowDetails(id) => {
            activate_known_host(app, id);
            app.known_hosts.context_menu = None;
            let was_visible = app.known_hosts.detail_open && !app.known_hosts.detail_closing;
            app.known_hosts.detail_open = true;
            app.known_hosts.detail_anim = if was_visible { 1.0 } else { 0.0 };
            app.known_hosts.detail_closing = false;
            iced::Task::none()
        }
        DomainMessage::KnownHostHideDetails => {
            app.known_hosts.detail_closing = true;
            iced::Task::none()
        }
        DomainMessage::KnownHostViewCursorMoved(pos) => {
            app.known_hosts.cursor_pos = pos;
            iced::Task::none()
        }
    }
}

fn activate_known_host(app: &mut App, id: usize) {
    app.known_hosts.cursor.select(id);
    if app.known_hosts.detail_open {
        app.known_hosts.detail_anim = 1.0;
        app.known_hosts.detail_closing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::KnownHost;
    use crate::core::grid_cursor::GridDirection;
    use iced::Point;

    // Only the non-destructive messages are exercised here: `KnownHostDelete`
    // rewrites the user's real `~/.ssh/known_hosts` file.

    fn entry(line: usize, hostname: &str) -> KnownHost {
        KnownHost {
            line_number: line,
            hostname: hostname.to_string(),
            port: 22,
            key_type: "ssh-ed25519".to_string(),
            public_key: "ssh-ed25519 AAAA".to_string(),
            fingerprint: "SHA256:abc".to_string(),
            marker: None,
            raw_pattern: hostname.to_string(),
        }
    }

    #[test]
    fn a_loaded_file_fills_the_workspace_and_a_failure_is_logged() {
        let mut app = crate::core::test_support::app();
        let logs_before = app.logs.len();

        let _ = update(
            &mut app,
            DomainMessage::KnownHostsLoaded(Ok(vec![
                entry(1, "example.com"),
                entry(2, "(hashed)"),
            ])),
        );
        assert_eq!(app.known_hosts.entries.len(), 2);
        assert_eq!(app.known_hosts.entries[1].hostname, "(hashed)");

        let _ = update(
            &mut app,
            DomainMessage::KnownHostsLoaded(Err("permission denied".into())),
        );
        assert!(
            app.logs.len() > logs_before,
            "an unreadable known_hosts file has to be visible"
        );
        assert!(
            app.logs
                .iter()
                .any(|log| log.message.contains("permission denied"))
        );
        assert_eq!(
            app.known_hosts.entries.len(),
            2,
            "a failed reload keeps the entries already on screen"
        );
    }

    #[test]
    fn selecting_a_known_host_is_tracked() {
        let mut app = crate::core::test_support::app();

        let _ = update(&mut app, DomainMessage::KnownHostSelect(4));

        assert_eq!(app.known_hosts.cursor.selected_key(), Some(&4));
    }

    #[test]
    fn cursor_walks_the_visible_known_hosts() {
        let mut app = crate::core::test_support::app();
        app.known_hosts.entries = vec![entry(1, "one"), entry(2, "two")];

        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::KnownHosts,
            GridDirection::Down,
        );
        assert_eq!(app.known_hosts.cursor.selected_key(), Some(&1));
        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::KnownHosts,
            GridDirection::Down,
        );
        assert_eq!(app.known_hosts.cursor.selected_key(), Some(&2));
    }

    #[test]
    fn deleting_asks_first_and_cancelling_changes_nothing() {
        let mut app = crate::core::test_support::app();
        app.known_hosts.entries = vec![entry(1, "example.com")];
        app.known_hosts.entries.push(entry(2, "other.example"));

        let _ = update(&mut app, DomainMessage::KnownHostDeleteRequest(1));
        assert_eq!(app.known_hosts.delete_pending, Some(1));

        let _ = update(&mut app, DomainMessage::KnownHostDeleteCancel);
        assert_eq!(app.known_hosts.delete_pending, None);
        assert_eq!(app.known_hosts.entries.len(), 2);
    }

    #[test]
    fn deleting_a_row_that_is_already_gone_is_harmless() {
        let mut app = crate::core::test_support::app();
        app.known_hosts.delete_pending = Some(99);

        // No entry matches, so this returns before touching the file.
        let _ = update(&mut app, DomainMessage::KnownHostDelete(99));

        assert_eq!(app.known_hosts.delete_pending, None);
        assert!(app.known_hosts.entries.is_empty());
    }

    #[test]
    fn searching_records_the_query_the_view_filters_by() {
        let mut app = crate::core::test_support::app();

        let _ = update(
            &mut app,
            DomainMessage::KnownHostSearchChanged("example".into()),
        );

        assert_eq!(app.known_hosts.search, "example");
    }

    #[test]
    fn the_details_panel_opens_animates_and_closes() {
        let mut app = crate::core::test_support::app();
        app.known_hosts.context_menu = Some(KnownHostContextMenu {
            id: 1,
            position: Point::ORIGIN,
            cursor: crate::context_menu::initial_cursor(&[
                crate::core::context_menu::ContextMenuAction::KnownHostDetails,
                crate::core::context_menu::ContextMenuAction::KnownHostDelete,
            ]),
        });

        let _ = update(&mut app, DomainMessage::KnownHostShowDetails(1));
        assert_eq!(app.known_hosts.cursor.selected_key(), Some(&1));
        assert!(app.known_hosts.detail_open);
        assert_eq!(app.known_hosts.detail_anim, 0.0, "it slides in");
        assert!(
            app.known_hosts.context_menu.is_none(),
            "the menu closes behind the panel"
        );

        app.known_hosts.detail_anim = 1.0;
        let _ = update(&mut app, DomainMessage::KnownHostShowDetails(2));
        assert_eq!(app.known_hosts.cursor.selected_key(), Some(&2));
        assert_eq!(
            app.known_hosts.detail_anim, 1.0,
            "switching entries must not replay the slide"
        );

        let _ = update(&mut app, DomainMessage::KnownHostSelect(3));
        assert_eq!(app.known_hosts.cursor.selected_key(), Some(&3));
        assert!(
            app.known_hosts.detail_open,
            "an open detail panel follows the selected row"
        );

        let _ = update(&mut app, DomainMessage::KnownHostHideDetails);
        assert!(app.known_hosts.detail_closing);
    }

    #[test]
    fn hover_events_track_the_row_under_the_cursor() {
        let mut app = crate::core::test_support::app();

        let _ = update(&mut app, DomainMessage::KnownHostHoverEnter(3));
        assert_eq!(app.known_hosts.hovered_id, Some(3));

        // A leave for a row that is no longer hovered must not clear the
        // current one.
        let _ = update(&mut app, DomainMessage::KnownHostHoverEnter(4));
        let _ = update(&mut app, DomainMessage::KnownHostHoverLeave(3));
        assert_eq!(app.known_hosts.hovered_id, Some(4));

        let _ = update(&mut app, DomainMessage::KnownHostHoverLeave(4));
        assert_eq!(app.known_hosts.hovered_id, None);
    }

    #[test]
    fn the_context_menu_follows_the_pointer_and_closes() {
        let mut app = crate::core::test_support::app();
        let position = Point::new(120.0, 340.0);
        let _ = update(&mut app, DomainMessage::KnownHostViewCursorMoved(position));
        assert_eq!(
            app.known_hosts.cursor_pos, position,
            "the workspace tracks the pointer for its own hover hit-testing"
        );

        // The menu itself is placed from the window-wide cursor position.
        app.window.cursor_pos = position;
        let _ = update(&mut app, DomainMessage::KnownHostContextMenuOpen(7));
        let menu = app.known_hosts.context_menu.as_ref().expect("a menu");
        assert_eq!(menu.id, 7);
        assert_eq!(
            menu.position, position,
            "the menu opens under the pointer, not at the origin"
        );

        let _ = update(&mut app, DomainMessage::KnownHostContextMenuClose);
        assert!(app.known_hosts.context_menu.is_none());
    }

    #[test]
    fn toggling_the_layout_flips_between_grid_and_list() {
        let mut app = crate::core::test_support::app();
        let was_grid = app.known_hosts.grid_view;

        let _ = update(&mut app, DomainMessage::KnownHostToggleView);
        assert_ne!(app.known_hosts.grid_view, was_grid);

        let _ = update(&mut app, DomainMessage::KnownHostToggleView);
        assert_eq!(app.known_hosts.grid_view, was_grid);
    }
}
