use iced::Point;
use secrecy::{ExposeSecret, SecretString};

use super::animation::tick_animations;
use super::crud::next_copy_name;
use super::form::{add_form_tag, key_path_for_auth, password_for_auth};
use super::new_tab::resolve_new_tab_host;
use super::transfer::present_import_result;
use super::{DomainMessage, update};
use crate::app::{App, DraftId};
use crate::core::data::{AuthType, Host, HostId, HostTagFilter, NavItem};
use crate::core::forms::FormDraft;
use crate::core::grid_cursor::GridDirection;
use crate::core::test_support::{app, drain, host};
use crate::hosts::state::{HostTransferDialog, HostTransferKind};
use crate::terminal::TabKind;

#[test]
fn only_the_auth_types_own_credential_is_persisted() {
    // Key auth stores a key path and nothing else.
    assert_eq!(
        key_path_for_auth(AuthType::Key, "~/.ssh/id_ed25519"),
        Some("~/.ssh/id_ed25519".to_string())
    );
    assert_eq!(key_path_for_auth(AuthType::Key, ""), None);
    assert_eq!(
        key_path_for_auth(AuthType::Password, "~/.ssh/id_ed25519"),
        None
    );

    let password = SecretString::from("hunter2");
    assert!(password_for_auth(AuthType::Password, &password).is_some());
    assert!(password_for_auth(AuthType::Key, &password).is_none());
    assert!(password_for_auth(AuthType::Password, &SecretString::from(String::new())).is_none());
}

#[test]
fn copy_names_count_up_instead_of_colliding() {
    assert_eq!(next_copy_name("prod"), "prod Copy");
    assert_eq!(next_copy_name("prod Copy"), "prod Copy 2");
    assert_eq!(next_copy_name("prod Copy 7"), "prod Copy 8");
    assert_eq!(next_copy_name("prod Copy west"), "prod Copy west Copy");
    assert_eq!(next_copy_name("prod Copy 1x"), "prod Copy 1x Copy");
}

fn app_with_hosts(hosts: Vec<Host>) -> App {
    let mut app = app();
    app.hosts.list = hosts;
    app
}

fn move_cursor(app: &mut App, direction: GridDirection) {
    let _ = crate::cursor::move_cursor(
        app,
        crate::core::cursor_focus::CursorTarget::Hosts,
        direction,
    );
}

#[test]
fn opening_the_new_host_form_clears_it_and_adopts_the_active_tag_filter() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    app.hosts.cursor.select(HostId(1));
    app.hosts.form.name = "left over from last time".into();
    app.hosts.tag_filter = HostTagFilter::Tag("prod".into());

    let _ = update(&mut app, DomainMessage::ShowForm);

    assert!(app.hosts.form.visible && !app.hosts.form_closing);
    assert_eq!(app.hosts.form_anim, 0.0, "the panel slides in");
    assert!(app.hosts.form.name.is_empty(), "a new host starts blank");
    assert_eq!(app.hosts.form.editing_id, None);
    assert_eq!(app.hosts.cursor.selected_key(), None);
    assert_eq!(
        app.hosts.form.tags,
        ["prod"],
        "a host added while a tag filter is active joins that tag"
    );
}

#[test]
fn opening_a_searchable_dropdown_focuses_its_filter() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    app.hosts.form.visible = true;

    let opened = drain(update(&mut app, DomainMessage::FormToggleJumpPicker));

    assert_eq!(
        app.active_dropdown,
        Some(crate::core::dropdown::DropdownTarget::HostFormJump)
    );
    assert!(
        opened.other_actions > 0,
        "the popup's filter takes focus when it opens"
    );
}

#[test]
fn reopening_the_form_while_it_is_visible_does_not_replay_the_slide() {
    let mut app = app_with_hosts(Vec::new());
    let _ = update(&mut app, DomainMessage::ShowForm);
    app.hosts.form_anim = 1.0;

    let _ = update(&mut app, DomainMessage::ShowForm);

    assert_eq!(
        app.hosts.form_anim, 1.0,
        "an open panel must not slide in again"
    );
}

#[test]
fn the_port_field_keeps_only_digits_and_refuses_ports_that_cannot_be_bound() {
    let mut app = app_with_hosts(Vec::new());
    let _ = update(&mut app, DomainMessage::ShowForm);

    let _ = update(&mut app, DomainMessage::FormPort("8o0a8".into()));
    assert_eq!(
        app.hosts.form.port, "808",
        "letters are dropped as they are typed"
    );

    let _ = update(&mut app, DomainMessage::FormPort("0".into()));
    assert_eq!(app.hosts.form.port, "808", "port 0 is not a port");

    let _ = update(&mut app, DomainMessage::FormPort("70000".into()));
    assert_eq!(app.hosts.form.port, "808", "anything past 65535 is refused");

    let _ = update(&mut app, DomainMessage::FormPort(String::new()));
    assert_eq!(
        app.hosts.form.port, "",
        "the field can be cleared to retype"
    );

    let _ = update(&mut app, DomainMessage::FormPort("65535".into()));
    assert_eq!(
        app.hosts.form.port, "65535",
        "65535 is the last usable port"
    );
}

#[test]
fn editing_a_host_fills_the_form_and_remembers_what_was_saved() {
    let saved = host(1, "web", "10.0.0.1");
    let mut app = app_with_hosts(vec![saved]);

    let _ = update(&mut app, DomainMessage::EditHost(HostId(1)));

    assert!(app.hosts.form.visible && !app.hosts.form_closing);
    assert_eq!(app.hosts.form.editing_id, Some(HostId(1)));
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(1)));
    assert_eq!(app.hosts.form.name, "web");
    assert_eq!(app.hosts.form.hostname, "10.0.0.1");
    assert_eq!(app.hosts.form.port, "22");
    assert_eq!(app.hosts.form.auth, AuthType::Password);
    assert_eq!(
        app.hosts.form.password.expose_secret(),
        "hunter2",
        "the stored password is offered for editing"
    );
    assert!(
        app.hosts.form.original.is_some(),
        "the form tracks its baseline"
    );
    assert!(!app.hosts.form.is_dirty(), "nothing has changed yet");
}

#[test]
fn asking_for_the_host_that_is_already_open_leaves_the_edits_alone() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    let _ = update(&mut app, DomainMessage::EditHost(HostId(1)));
    let _ = update(
        &mut app,
        DomainMessage::FormHostname("edited.example".into()),
    );

    let _ = update(&mut app, DomainMessage::EditHost(HostId(1)));

    assert_eq!(app.hosts.form.hostname, "edited.example");
}

#[test]
fn editing_a_host_that_is_gone_opens_nothing() {
    let mut app = app_with_hosts(Vec::new());
    let _ = update(&mut app, DomainMessage::ShowForm);

    let _ = update(&mut app, DomainMessage::EditHost(HostId(404)));

    assert!(app.hosts.form.editing_id.is_none());
    assert!(app.hosts.form.name.is_empty());
}

#[test]
fn saving_an_edited_host_updates_the_vault_in_place() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")]);
    let _ = update(&mut app, DomainMessage::EditHost(HostId(1)));
    let _ = update(&mut app, DomainMessage::FormHostname("10.9.9.9".into()));
    let _ = update(&mut app, DomainMessage::FormPort("2222".into()));
    let _ = update(&mut app, DomainMessage::SaveHost);

    let saved = app
        .hosts
        .list
        .iter()
        .find(|host| host.id == HostId(1))
        .unwrap();
    assert_eq!(saved.hostname, "10.9.9.9");
    assert_eq!(saved.port, 2222);
    assert_eq!(app.hosts.list.len(), 2, "editing does not add a host");
    assert_eq!(
        app.hosts.list[0].id,
        HostId(1),
        "the edited host keeps its place in the list"
    );

    assert!(!app.hosts.form.visible, "the panel closes on save");
    assert!(app.hosts.form.editing_id.is_none(), "the form is reset");
    assert_eq!(
        app.hosts.recent_ids.first(),
        Some(&HostId(1)),
        "a just-edited host counts as recently used"
    );
}

#[test]
fn an_incomplete_form_is_not_saved() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    let _ = update(&mut app, DomainMessage::ShowForm);
    let _ = update(&mut app, DomainMessage::FormName("half typed".into()));

    let _ = update(&mut app, DomainMessage::SaveHost);

    assert!(
        app.hosts.form.visible,
        "the panel stays open so the user can finish"
    );
    assert!(app.hosts.list.iter().all(|host| host.name != "half typed"));
}

#[test]
fn deleting_a_host_clears_everything_that_pointed_at_it() {
    let mut second = host(2, "db", "10.0.0.2");
    second.jump_host_id = Some(HostId(1));
    let mut app = app_with_hosts(vec![host(1, "bastion", "10.0.0.1"), second]);
    app.hosts.recent_ids = vec![HostId(1), HostId(2)];
    app.hosts.delete_pending = Some(HostId(1));

    let _ = update(&mut app, DomainMessage::DeleteHost(HostId(1)));

    assert_eq!(app.hosts.list.len(), 1);
    assert_eq!(
        app.hosts.list[0].jump_host_id, None,
        "a dangling jump host would break the surviving host's connection"
    );
    assert_eq!(app.hosts.recent_ids, [HostId(2)]);
    assert_eq!(app.hosts.delete_pending, None, "the confirm closes itself");
}

#[test]
fn deleting_the_host_open_in_the_form_closes_the_form() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    let _ = update(&mut app, DomainMessage::EditHost(HostId(1)));

    let _ = update(&mut app, DomainMessage::DeleteHost(HostId(1)));

    assert!(!app.hosts.form.visible);
    assert!(app.hosts.form.editing_id.is_none());
    assert!(
        app.needs_scroll_restore,
        "the list scrolls back to where it was"
    );
}

#[test]
fn a_host_created_by_the_form_lands_on_top_and_counts_as_recent() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);

    let _ = update(
        &mut app,
        DomainMessage::HostCreated {
            host: host(2, "db", "10.0.0.2"),
        },
    );

    assert_eq!(app.hosts.list.len(), 2);
    assert_eq!(app.hosts.list[0].id, HostId(2), "the new host is on top");
    assert_eq!(app.hosts.recent_ids.first(), Some(&HostId(2)));
}

#[test]
fn hovering_the_list_marks_one_card_and_clearing_it_does_not_steal_the_next() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);

    let _ = update(&mut app, DomainMessage::HostHoverEnter(HostId(1)));
    assert_eq!(app.hosts.hovered_id, Some(HostId(1)));
    let _ = update(&mut app, DomainMessage::HostHoverEnter(HostId(2)));
    let _ = update(&mut app, DomainMessage::HostHoverLeave(HostId(1)));
    assert_eq!(app.hosts.hovered_id, Some(HostId(2)));
    let _ = update(&mut app, DomainMessage::HostHoverLeave(HostId(2)));
    assert_eq!(app.hosts.hovered_id, None);
}

#[test]
fn clicking_hosts_moves_the_single_selection() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")]);

    let _ = update(&mut app, DomainMessage::SelectHost(HostId(1)));
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(1)));
    let _ = update(&mut app, DomainMessage::SelectHost(HostId(2)));
    assert_eq!(
        app.hosts.cursor.selected_key(),
        Some(&HostId(2)),
        "a second click moves the selection instead of adding to it"
    );
}

#[test]
fn the_vault_cursor_follows_its_host_across_a_recency_reorder() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")]);
    app.hosts.cursor.select(HostId(1));
    app.hosts.record_recent(HostId(2));
    let visible: Vec<HostId> = app.hosts.filtered().iter().map(|host| host.id).collect();

    assert_eq!(visible, vec![HostId(2), HostId(1)]);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(1)));
}

#[test]
fn the_vault_cursor_walks_the_visible_hosts() {
    let mut app = app_with_hosts(vec![
        host(1, "web", "10.0.0.1"),
        host(2, "db", "10.0.0.2"),
        host(3, "scratch", "127.0.0.1"),
    ]);
    app.hosts.recent_ids = vec![HostId(2)];

    move_cursor(&mut app, GridDirection::Down);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(2)));
    move_cursor(&mut app, GridDirection::Down);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(1)));
    move_cursor(&mut app, GridDirection::Up);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(2)));

    app.hosts.search_query = "scratch".into();
    move_cursor(&mut app, GridDirection::Up);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(3)));
}

#[test]
fn the_grid_cursor_moves_by_rows_and_stays_inside_a_row() {
    let mut app = app_with_hosts(
        (1..=7)
            .map(|id| host(id, &format!("host-{id}"), "10.0.0.1"))
            .collect(),
    );
    app.hosts.grid_columns = 3;

    move_cursor(&mut app, GridDirection::Down);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(1)));
    move_cursor(&mut app, GridDirection::Down);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(4)));
    move_cursor(&mut app, GridDirection::Right);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(5)));
    move_cursor(&mut app, GridDirection::Up);
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(2)));
}

#[test]
fn deleting_the_selected_host_clears_the_selection() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    let _ = update(&mut app, DomainMessage::SelectHost(HostId(1)));

    let _ = update(&mut app, DomainMessage::DeleteHost(HostId(1)));

    assert_eq!(app.hosts.cursor.selected_key(), None);
}

#[test]
fn a_host_sitting_under_an_open_toolbar_menu_does_not_highlight() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::HostsTransferMenu);

    let _ = update(&mut app, DomainMessage::HostHoverEnter(HostId(1)));

    assert_eq!(app.hosts.hovered_id, None);
}

#[test]
fn toggling_the_layout_flips_between_grid_and_list() {
    let mut app = app_with_hosts(Vec::new());
    let was_grid = app.hosts.grid_view;

    let _ = update(&mut app, DomainMessage::ToggleView);
    assert_ne!(app.hosts.grid_view, was_grid);
    let _ = update(&mut app, DomainMessage::ToggleView);
    assert_eq!(app.hosts.grid_view, was_grid);
}

#[test]
fn the_delete_confirmation_is_opened_and_cancelled_by_its_own_messages() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    app.hosts.context_menu = Some(crate::hosts::HostsContextMenu {
        host_id: HostId(1),
        position: Point::ORIGIN,
        cursor: crate::context_menu::initial_cursor(&[
            crate::core::context_menu::ContextMenuAction::HostEdit,
            crate::core::context_menu::ContextMenuAction::HostDuplicate,
            crate::core::context_menu::ContextMenuAction::HostDelete,
        ]),
    });

    let _ = update(&mut app, DomainMessage::HostDeleteRequest(HostId(1)));
    assert_eq!(app.hosts.delete_pending, Some(HostId(1)));
    assert!(
        app.hosts.context_menu.is_none(),
        "the menu closes behind it"
    );
    let _ = update(&mut app, DomainMessage::HostDeleteCancel);
    assert_eq!(app.hosts.delete_pending, None);
    assert_eq!(app.hosts.list.len(), 1, "cancelling deletes nothing");
}

#[test]
fn switching_workspaces_closes_the_host_toolbar_menus() {
    let mut app = app_with_hosts(Vec::new());
    app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::HostsTransferMenu);

    let _ = update(&mut app, DomainMessage::Nav(NavItem::Settings));

    assert_eq!(app.active_dropdown, None);
    assert_eq!(app.active_nav, NavItem::Settings);
}

#[test]
fn navigation_closes_page_panels_and_command_info() {
    let mut app = app_with_hosts(Vec::new());
    app.hosts.form.visible = true;
    app.keychain.identity_form.visible = true;
    app.port_forwards.form.visible = true;
    app.snippets.package_form.visible = true;
    app.known_hosts.detail_open = true;
    app.terminal.command_info = Some(crate::terminal::state::CommandInfo {
        session_id: crate::app::SessionId(1),
        index: 0,
    });

    let _ = update(&mut app, DomainMessage::Nav(NavItem::Settings));

    assert!(!app.hosts.form.visible);
    assert!(!app.keychain.identity_form.visible);
    assert!(!app.port_forwards.form.visible);
    assert!(!app.snippets.package_form.visible);
    assert!(!app.known_hosts.detail_open);
    assert_eq!(app.terminal.command_info, None);
}

fn with_new_tab_query(app: &mut App, query: &str) {
    let id = DraftId(1);
    app.terminal.drafts.insert(
        id,
        crate::terminal::Draft {
            query: query.to_string(),
            cursor: crate::core::list_cursor::ListCursor::new(),
            ssh: crate::ssh_connect::SshConnectState::default(),
        },
    );
    let tab = app.allocate_tab_id(TabKind::Draft(id));
    app.activate_tab(tab);
}

#[test]
fn the_new_tab_cursor_walks_the_visible_hosts() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")]);
    with_new_tab_query(&mut app, "");
    let draft = DraftId(1);

    let _ = update(&mut app, DomainMessage::NewTabSelectDown);
    assert_eq!(
        app.terminal
            .drafts
            .get(&draft)
            .unwrap()
            .cursor
            .selected_key(),
        Some(&HostId(1))
    );
    let _ = update(&mut app, DomainMessage::NewTabSelectDown);
    assert_eq!(
        app.terminal
            .drafts
            .get(&draft)
            .unwrap()
            .cursor
            .selected_key(),
        Some(&HostId(2))
    );
    let _ = update(&mut app, DomainMessage::NewTabSelectDown);
    assert_eq!(
        app.terminal
            .drafts
            .get(&draft)
            .unwrap()
            .cursor
            .selected_key(),
        Some(&HostId(2)),
        "the cursor stops at the last host"
    );
}

#[test]
fn arrow_up_from_no_selection_starts_at_the_last_visible_host() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")]);
    with_new_tab_query(&mut app, "");
    let draft = DraftId(1);

    let _ = update(&mut app, DomainMessage::NewTabSelectUp);

    assert_eq!(
        app.terminal
            .drafts
            .get(&draft)
            .unwrap()
            .cursor
            .selected_key(),
        Some(&HostId(2))
    );
}

#[test]
fn changing_the_new_tab_query_clears_the_cursor() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")]);
    with_new_tab_query(&mut app, "db");
    let draft = DraftId(1);
    let _ = update(&mut app, DomainMessage::NewTabSelectDown);

    let _ = update(&mut app, DomainMessage::NewTabQueryChanged("web".into()));

    assert_eq!(
        app.terminal
            .drafts
            .get(&draft)
            .unwrap()
            .cursor
            .selected_key(),
        None
    );
}

#[test]
fn enter_without_moving_connects_the_first_visible_host() {
    let mut app = app_with_hosts(vec![
        host(1, "prod-db", "10.0.0.1"),
        host(2, "db", "10.0.0.2"),
    ]);
    app.hosts.recent_ids = vec![HostId(2), HostId(1)];
    with_new_tab_query(&mut app, "db");
    let draft = DraftId(1);

    let _ = update(&mut app, DomainMessage::NewTabConnect);

    assert_eq!(
        app.terminal
            .drafts
            .get(&draft)
            .unwrap()
            .ssh
            .attempt
            .pending_host
            .as_ref()
            .map(|host| host.id),
        Some(HostId(2)),
        "Enter connects to the row shown first, not an item hidden by list order"
    );
}

#[test]
fn the_new_tab_search_picks_the_first_host_that_matches_what_was_typed() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")]);
    with_new_tab_query(&mut app, "db");

    let picked = resolve_new_tab_host(&mut app).expect("the query matches the db host");

    assert_eq!(picked.id, HostId(2));
    assert_eq!(
        app.hosts.recent_ids.first().copied(),
        Some(HostId(2)),
        "connecting from the search box counts as recent use"
    );
}

#[test]
fn the_new_tab_search_matches_any_field_and_ignores_case_and_padding() {
    let mut app = app_with_hosts(vec![host(1, "web", "Example.COM")]);
    with_new_tab_query(&mut app, "  example.com  ");
    assert_eq!(
        resolve_new_tab_host(&mut app).map(|host| host.id),
        Some(HostId(1))
    );

    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    with_new_tab_query(&mut app, "root");
    assert_eq!(
        resolve_new_tab_host(&mut app).map(|host| host.id),
        Some(HostId(1)),
        "the username is searched too"
    );
}

#[test]
fn the_new_tab_search_answers_nothing_without_a_draft_or_a_match() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    with_new_tab_query(&mut app, "nothing matches this");
    assert!(resolve_new_tab_host(&mut app).is_none());
    assert!(
        app.hosts.recent_ids.is_empty(),
        "a query that matched nothing is not recorded as recent use"
    );

    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    assert!(resolve_new_tab_host(&mut app).is_none());
}

fn exported(name: &str, hostname: &str) -> crate::hosts::export::ExportHost {
    crate::hosts::export::ExportHost {
        name: name.to_string(),
        hostname: hostname.to_string(),
        port: 22,
        username: "root".to_string(),
        auth_type: "Password".to_string(),
        password: None,
        key_path: None,
        jump_host_name: None,
        os_type: "Unknown".to_string(),
        tags: Vec::new(),
    }
}

#[test]
fn an_import_of_nothing_new_just_reports_how_many_collided() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);
    app.hosts.transfer_dialog = Some(HostTransferDialog::new_import_confirm(Vec::new(), 0, 0));

    let _ = present_import_result(
        &mut app,
        vec![
            exported("web under another name", "10.0.0.1"),
            exported("web again", "10.0.0.1"),
        ],
    );

    assert!(
        app.hosts.transfer_dialog.is_none(),
        "with nothing left to confirm, the dialog closes"
    );
}

#[test]
fn an_import_that_names_one_endpoint_twice_only_queues_it_once() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);

    let _ = present_import_result(
        &mut app,
        vec![
            exported("staging", "10.0.0.7"),
            exported("staging under another name", "10.0.0.7"),
        ],
    );

    let dialog = app
        .hosts
        .transfer_dialog
        .as_ref()
        .expect("a new host means a confirmation");
    assert_eq!(dialog.new_count, 1);
    assert_eq!(dialog.dup_count, 1);
    assert_eq!(
        dialog.pending_hosts.len(),
        1,
        "the repeated endpoint is counted as a duplicate"
    );
}

#[test]
fn an_import_with_something_new_asks_for_confirmation_with_the_counts() {
    let mut app = app_with_hosts(vec![host(1, "web", "10.0.0.1")]);

    let _ = present_import_result(
        &mut app,
        vec![
            exported("web again", "10.0.0.1"),
            exported("staging", "10.0.0.7"),
        ],
    );

    let dialog = app
        .hosts
        .transfer_dialog
        .as_ref()
        .expect("a new host means a confirmation");
    assert_eq!(dialog.kind, HostTransferKind::ImportConfirm);
    assert_eq!(dialog.new_count, 1);
    assert_eq!(dialog.dup_count, 1);
    assert_eq!(dialog.pending_hosts.len(), 1, "only the new one is queued");
    assert_eq!(dialog.pending_hosts[0].hostname, "10.0.0.7");
}

#[test]
fn the_form_slide_animation_runs_to_its_ends() {
    let mut app = app_with_hosts(Vec::new());
    app.hosts.form.visible = true;
    app.hosts.form_anim = 0.0;

    tick_animations(&mut app);
    assert!(
        app.hosts.form_anim > 0.0 && app.hosts.form.visible,
        "an open form slides in rather than appearing at once"
    );

    app.hosts.form_closing = true;
    let mut steps = 0;
    while app.hosts.form.visible && steps < 200 {
        tick_animations(&mut app);
        steps += 1;
    }

    assert!(
        !app.hosts.form.visible,
        "the form closes once it is out of sight"
    );
    assert!(!app.hosts.form_closing);
    assert_eq!(app.hosts.form_anim, 0.0);
    assert!(app.needs_scroll_restore);
}

#[test]
fn a_busy_transfer_dialog_animates_and_an_idle_one_does_not() {
    let mut app = app_with_hosts(Vec::new());
    app.hosts.transfer_dialog = Some(HostTransferDialog::new_import_confirm(Vec::new(), 0, 0));

    tick_animations(&mut app);
    let idle = app.hosts.transfer_dialog.as_ref().unwrap().anim_t;
    assert_eq!(idle, 0.0, "a dialog that is not working does not shimmer");

    app.hosts.transfer_dialog.as_mut().unwrap().busy = true;
    tick_animations(&mut app);
    assert!(
        app.hosts.transfer_dialog.as_ref().unwrap().anim_t > 0.0,
        "the shimmer advances while work is in flight"
    );
}

#[test]
fn form_tags_are_trimmed_and_not_repeated() {
    let mut app = app_with_hosts(Vec::new());

    assert!(add_form_tag(&mut app, "  prod  "));
    assert_eq!(app.hosts.form.tags, ["prod"]);
    assert!(!add_form_tag(&mut app, "prod"));
    assert_eq!(app.hosts.form.tags.len(), 1);
    assert!(!add_form_tag(&mut app, "   "));
    assert_eq!(app.hosts.form.tags, ["prod"]);
    assert!(add_form_tag(&mut app, "Prod"));
}
