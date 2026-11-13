use super::lifecycle::{check_system_port_available, delete_forwards_for_host};
use super::ui::{default_forward_name, is_non_loopback_bind, next_copy_name};
use super::worker::bind_address_for_listen;
use super::{DomainMessage, update};
use crate::app::App;
use crate::core::data::{HostId, PortForward, PortForwardId, PortForwardKind};
use crate::core::forms::FormDraft;
use crate::core::grid_cursor::GridDirection;
use crate::core::test_support::{app, host};
use crate::port_forward::state::PortForwardStatus;

#[test]
fn an_empty_bind_address_means_loopback() {
    // The form leaves the field empty for "just the local machine"; the
    // listen address must not be built as ":8080", which binds every interface.
    assert_eq!(bind_address_for_listen(""), "127.0.0.1");
    assert_eq!(bind_address_for_listen("   "), "127.0.0.1");
    assert_eq!(bind_address_for_listen(" 0.0.0.0 "), "0.0.0.0");
    assert_eq!(bind_address_for_listen("::1"), "::1");
}

#[test]
fn loopback_binds_are_not_treated_as_exposed() {
    for loopback in ["", "  ", "127.0.0.1", "localhost", "::1", "[::1]"] {
        assert!(
            !is_non_loopback_bind(loopback),
            "{loopback:?} is reachable only from this machine"
        );
    }
    for exposed in ["0.0.0.0", "::", "*", "192.168.1.10"] {
        assert!(
            is_non_loopback_bind(exposed),
            "{exposed:?} exposes the forward beyond this machine"
        );
    }
}

#[test]
fn suggested_names_describe_the_forward_and_fall_back_to_a_wildcard() {
    assert_eq!(
        default_forward_name(
            PortForwardKind::Local,
            "127.0.0.1",
            8080,
            "db.internal",
            5432
        ),
        "127.0.0.1:8080 → db.internal:5432"
    );
    assert_eq!(
        default_forward_name(PortForwardKind::Remote, "0.0.0.0", 80, "localhost", 8080),
        "0.0.0.0:80 → localhost:8080"
    );
    assert_eq!(
        default_forward_name(PortForwardKind::Dynamic, "127.0.0.1", 1080, "", 0),
        "127.0.0.1:1080"
    );
    assert_eq!(
        default_forward_name(PortForwardKind::Dynamic, "  ", 1080, "", 0),
        "*:1080",
        "an unset bind address is shown as a wildcard"
    );
}

#[test]
fn copy_names_count_up_instead_of_colliding() {
    assert_eq!(next_copy_name("web"), "web Copy");
    assert_eq!(next_copy_name("web Copy"), "web Copy 2");
    assert_eq!(next_copy_name("web Copy 2"), "web Copy 3");
    assert_eq!(next_copy_name("web Copy 41"), "web Copy 42");
    assert_eq!(next_copy_name("web Copy draft"), "web Copy draft Copy");
    assert_eq!(next_copy_name("Copy"), "Copy Copy");
}

#[test]
fn a_port_already_in_use_is_reported_with_a_reason() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = listener.local_addr().unwrap().to_string();
    let reason = check_system_port_available(&taken).expect("a bound port must be unavailable");
    assert!(
        reason.contains(&taken),
        "the reason should name the address it could not bind: {reason}"
    );
    drop(listener);
    assert!(check_system_port_available(&taken).is_none());
}

#[test]
fn a_malformed_listen_address_is_reported_rather_than_panicking() {
    let reason = check_system_port_available("not-an-address:8080")
        .expect("an unusable address must be reported as a conflict");
    assert!(reason.contains("not-an-address:8080"));
}

fn forward(id: u32, name: &str, kind: PortForwardKind) -> PortForward {
    PortForward {
        id: PortForwardId(id),
        name: name.to_string(),
        kind,
        host_id: HostId(1),
        bind_address: "127.0.0.1".into(),
        listen_port: 8080,
        dest_host: "10.0.0.9".into(),
        dest_port: 80,
    }
}

fn app_with_host() -> App {
    let mut app = app();
    app.hosts.list = vec![host(1, "bastion", "10.0.0.1")];
    app
}

#[test]
fn the_new_forward_form_defaults_to_the_first_saved_host() {
    let mut app = app_with_host();
    let _ = update(&mut app, DomainMessage::PortForwardShowForm);

    assert!(app.port_forwards.form.visible);
    assert_eq!(app.port_forwards.form.host_id, Some(HostId(1)));
    assert_eq!(app.port_forwards.form.bind_address, "127.0.0.1");
    assert_eq!(app.port_forwards.form.listen_port, "8080");
}

#[test]
fn the_new_forward_form_opens_with_no_host_when_the_vault_is_empty() {
    let mut app = app();
    let _ = update(&mut app, DomainMessage::PortForwardShowForm);

    assert!(app.port_forwards.form.visible);
    assert_eq!(app.port_forwards.form.host_id, None);
}

#[test]
fn editing_a_forward_loads_it_into_the_form() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];

    let _ = update(&mut app, DomainMessage::PortForwardEdit(PortForwardId(1)));

    assert!(app.port_forwards.form.visible);
    assert_eq!(app.port_forwards.form.editing_id, Some(PortForwardId(1)));
    assert_eq!(app.port_forwards.form.name, "web tunnel");
    assert_eq!(app.port_forwards.form.listen_port, "8080");
    assert_eq!(app.port_forwards.form.dest_host, "10.0.0.9");
    assert_eq!(app.port_forwards.form.dest_port, "80");
    assert!(app.port_forwards.form.original.is_some());
    assert!(!app.port_forwards.form.is_dirty());
}

#[test]
fn a_forward_without_a_name_is_named_after_its_endpoints() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    let _ = update(&mut app, DomainMessage::PortForwardEdit(PortForwardId(1)));
    let _ = update(&mut app, DomainMessage::PortForwardFormName(String::new()));
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormBindAddress("0.0.0.0".into()),
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormDestHost("db.internal".into()),
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormDestPort("5432".into()),
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormListenPort("15432".into()),
    );
    let _ = update(&mut app, DomainMessage::PortForwardFormSubmit);

    assert_eq!(
        app.port_forwards.list[0].name, "0.0.0.0:15432 → db.internal:5432",
        "the generated name describes the tunnel it opens"
    );
    assert_eq!(app.port_forwards.list[0].listen_port, 15432);
    assert_eq!(app.port_forwards.list[0].dest_port, 5432);
    assert!(!app.port_forwards.form.visible, "the form closes on save");
}

#[test]
fn an_incomplete_forward_form_is_not_submitted() {
    let mut app = app_with_host();
    let _ = update(&mut app, DomainMessage::PortForwardShowForm);
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormDestPort(String::new()),
    );
    let _ = update(&mut app, DomainMessage::PortForwardFormSubmit);

    assert!(app.port_forwards.form.visible);
    assert!(app.port_forwards.list.is_empty());
}

#[test]
fn a_dynamic_forward_needs_no_destination() {
    let mut app = app_with_host();
    let _ = update(&mut app, DomainMessage::PortForwardShowForm);
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormKind(PortForwardKind::Dynamic),
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormDestHost(String::new()),
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormDestPort(String::new()),
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardFormListenPort("1080".into()),
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardCreated {
            forward: {
                let mut socks = forward(9, "socks", PortForwardKind::Dynamic);
                socks.listen_port = 1080;
                socks.dest_host = String::new();
                socks.dest_port = 0;
                socks
            },
        },
    );

    assert_eq!(app.port_forwards.list.len(), 1);
    assert_eq!(app.port_forwards.list[0].dest_host, "");
}

#[test]
fn deleting_a_forward_clears_every_view_that_was_pointing_at_it() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    app.port_forwards.cursor.select(PortForwardId(1));
    app.port_forwards.logs_open = true;
    let _ = update(&mut app, DomainMessage::PortForwardEdit(PortForwardId(1)));
    let _ = update(
        &mut app,
        DomainMessage::PortForwardDeleteRequest(PortForwardId(1)),
    );
    assert_eq!(app.port_forwards.delete_pending, Some(PortForwardId(1)));
    let _ = update(&mut app, DomainMessage::PortForwardDeleteCancel);
    assert_eq!(app.port_forwards.delete_pending, None);
    assert_eq!(app.port_forwards.list.len(), 1);

    let _ = update(&mut app, DomainMessage::PortForwardDelete(PortForwardId(1)));

    assert!(app.port_forwards.list.is_empty());
    assert_eq!(app.port_forwards.cursor.selected_key(), None);
    assert!(!app.port_forwards.logs_open);
    assert!(!app.port_forwards.form.visible);
}

#[test]
fn selecting_a_forward_tracks_the_visible_card() {
    let mut app = app_with_host();
    let _ = update(&mut app, DomainMessage::PortForwardSelect(PortForwardId(1)));
    assert_eq!(
        app.port_forwards.cursor.selected_key(),
        Some(&PortForwardId(1))
    );
    let _ = update(&mut app, DomainMessage::PortForwardSelect(PortForwardId(2)));
    assert_eq!(
        app.port_forwards.cursor.selected_key(),
        Some(&PortForwardId(2))
    );
}

#[test]
fn selection_rebinds_the_open_editor_and_logs() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![
        forward(1, "one", PortForwardKind::Local),
        forward(2, "two", PortForwardKind::Remote),
    ];
    let _ = update(&mut app, DomainMessage::PortForwardEdit(PortForwardId(1)));
    assert!(app.port_forwards.form.visible);
    assert_eq!(app.port_forwards.form.editing_id, Some(PortForwardId(1)));
    let _ = update(&mut app, DomainMessage::PortForwardSelect(PortForwardId(2)));
    assert_eq!(
        app.port_forwards.cursor.selected_key(),
        Some(&PortForwardId(2))
    );
    assert_eq!(app.port_forwards.form.editing_id, Some(PortForwardId(2)));
    let _ = update(
        &mut app,
        DomainMessage::PortForwardShowLogs(PortForwardId(2)),
    );
    assert!(app.port_forwards.logs_open);
    let _ = update(&mut app, DomainMessage::PortForwardSelect(PortForwardId(1)));
    assert_eq!(
        app.port_forwards.cursor.selected_key(),
        Some(&PortForwardId(1))
    );
    assert!(app.port_forwards.logs_open);
}

#[test]
fn cursor_walks_the_visible_forwards() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![
        forward(1, "one", PortForwardKind::Local),
        forward(2, "two", PortForwardKind::Remote),
    ];

    let _ = crate::cursor::move_cursor(
        &mut app,
        crate::core::cursor_focus::CursorTarget::PortForwards,
        GridDirection::Down,
    );
    assert_eq!(
        app.port_forwards.cursor.selected_key(),
        Some(&PortForwardId(1))
    );
    let _ = crate::cursor::move_cursor(
        &mut app,
        crate::core::cursor_focus::CursorTarget::PortForwards,
        GridDirection::Down,
    );
    assert_eq!(
        app.port_forwards.cursor.selected_key(),
        Some(&PortForwardId(2))
    );
}

#[test]
fn stopping_a_forward_that_is_not_running_does_nothing() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    let _ = update(&mut app, DomainMessage::PortForwardStop(PortForwardId(1)));

    assert_eq!(
        app.port_forwards.status_for(PortForwardId(1)),
        PortForwardStatus::Stopped
    );
    assert_eq!(app.port_forwards.list.len(), 1);
}

#[test]
fn starting_a_forward_for_a_host_that_is_gone_fails_loudly() {
    let mut app = app();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    let _ = update(&mut app, DomainMessage::PortForwardStart(PortForwardId(1)));

    assert_eq!(
        app.port_forwards.status_for(PortForwardId(1)),
        PortForwardStatus::Failed
    );
    let runtime = app.port_forwards.runtime_for(PortForwardId(1)).unwrap();
    assert!(
        runtime
            .logs
            .iter()
            .any(|line| line.contains("host not found"))
    );
}

#[test]
fn starting_a_forward_on_a_port_that_is_already_taken_fails_before_spawning() {
    let mut app = app_with_host();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = listener.local_addr().unwrap().port();
    let mut forward = forward(1, "web tunnel", PortForwardKind::Local);
    forward.listen_port = taken;
    app.port_forwards.list = vec![forward];
    let _ = update(&mut app, DomainMessage::PortForwardStart(PortForwardId(1)));

    assert_eq!(
        app.port_forwards.status_for(PortForwardId(1)),
        PortForwardStatus::Failed
    );
    let runtime = app.port_forwards.runtime_for(PortForwardId(1)).unwrap();
    assert!(runtime.logs.iter().any(|line| line.contains("Cannot bind")));
}

#[test]
fn starting_a_forward_that_is_already_running_is_ignored() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    app.port_forwards.runtime.insert(
        PortForwardId(1),
        crate::port_forward::state::PortForwardRuntime {
            status: PortForwardStatus::Running,
            logs: vec!["already up".into()],
        },
    );
    let _ = update(&mut app, DomainMessage::PortForwardStart(PortForwardId(1)));

    assert_eq!(
        app.port_forwards.status_for(PortForwardId(1)),
        PortForwardStatus::Running
    );
    assert_eq!(
        app.port_forwards
            .runtime_for(PortForwardId(1))
            .unwrap()
            .logs,
        ["already up"]
    );
}

#[test]
fn the_reported_result_decides_whether_the_forward_is_live() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    let _ = update(
        &mut app,
        DomainMessage::PortForwardStarted {
            id: PortForwardId(1),
            success: false,
            logs: vec!["ssh: connection refused".into()],
        },
    );
    assert_eq!(
        app.port_forwards.status_for(PortForwardId(1)),
        PortForwardStatus::Failed
    );
    assert_eq!(
        app.port_forwards
            .runtime_for(PortForwardId(1))
            .unwrap()
            .logs,
        ["ssh: connection refused"]
    );
    let _ = update(
        &mut app,
        DomainMessage::PortForwardStarted {
            id: PortForwardId(1),
            success: true,
            logs: vec!["listening".into()],
        },
    );
    assert_eq!(
        app.port_forwards.status_for(PortForwardId(1)),
        PortForwardStatus::Running
    );
}

#[test]
fn hover_and_context_menu_events_track_the_row_they_came_from() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    let _ = update(
        &mut app,
        DomainMessage::PortForwardHoverEnter(PortForwardId(1)),
    );
    assert_eq!(app.port_forwards.hovered_id, Some(PortForwardId(1)));
    let _ = update(
        &mut app,
        DomainMessage::PortForwardHoverLeave(PortForwardId(1)),
    );
    assert_eq!(app.port_forwards.hovered_id, None);
    let _ = update(
        &mut app,
        DomainMessage::PortForwardContextMenuOpen(PortForwardId(1)),
    );
    assert_eq!(
        app.port_forwards
            .context_menu
            .as_ref()
            .map(|menu| menu.forward_id),
        Some(PortForwardId(1))
    );
    let _ = update(&mut app, DomainMessage::PortForwardContextMenuClose);
    assert!(app.port_forwards.context_menu.is_none());
}

#[test]
fn a_forward_for_a_deleted_host_is_removed_with_it() {
    let mut app = app_with_host();
    app.port_forwards.list = vec![forward(1, "web tunnel", PortForwardKind::Local)];
    delete_forwards_for_host(&mut app, HostId(1));

    assert!(app.port_forwards.list.is_empty());
}

#[test]
fn forwards_for_other_hosts_survive_a_deletion() {
    let mut app = app_with_host();
    app.hosts.list.push({
        let mut other = host(2, "db", "10.0.0.2");
        other.id = HostId(2);
        other
    });
    let mut other_forward = forward(2, "db tunnel", PortForwardKind::Local);
    other_forward.host_id = HostId(2);
    app.port_forwards.list = vec![
        forward(1, "web tunnel", PortForwardKind::Local),
        other_forward,
    ];
    delete_forwards_for_host(&mut app, HostId(1));

    assert_eq!(app.port_forwards.list.len(), 1);
    assert_eq!(app.port_forwards.list[0].host_id, HostId(2));
}
