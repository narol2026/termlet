//! Headless end-to-end tests. Compiled only under `cfg(test)`.
//!
//! These drive the *real* app: the real widget tree, the real messages, the
//! real update handlers, and — in the [`Emulator`] case — the real
//! subscriptions and tasks. `iced_test` renders through the software renderer
//! and injects synthetic events, so this runs in a plain `cargo test` with no
//! window, display or GPU.
//!
//! Two levels are used here:
//!
//! - [`simulator`] builds one frame of an interface and lets a test click, type
//!   and scroll through it. Nothing runs as a side effect — the messages it
//!   produces are handed back to `update` explicitly, keeping the loop
//!   deterministic.
//! - [`Emulator`] runs the whole [`iced::Program`] — boot, subscriptions and
//!   tasks included — against an injected initial state, so tests observe the
//!   app as the runtime really drives it. [`Instruction`]s run via [`run`] and
//!   [`settle`].
//!
//! Both rely on [`crate::core::test_support`] for the in-memory database and
//! the scratch data directory, so a test run never touches `~/.termlet/`.
//!
//! What these cover is the *wiring*: a real widget produces the message, a real
//! handler receives it, the next frame is built from the result. Fixtures only
//! stand in where a user or a machine would be (a vault, an open form, a live
//! terminal), and input the shell delivers outside the widget tree — an ignored
//! key, the pointer position when a menu opens — is handed over as the very
//! message the matching subscription produces.

// Rust guideline compliant 2026-10-09

use iced::Task;
use iced::futures::StreamExt;
use iced::futures::channel::mpsc;
use iced_test::emulator::{Emulator, Event, Mode};
use iced_test::instruction::{Interaction, Mouse, Target};
use iced_test::selector::Bounded;
use iced_test::{Instruction, Selector, Simulator, program, simulator};

use crate::app::{ActiveTab, App, Workspace};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{FileEntry, FileEntryType, HostId, NavItem};
use crate::core::message::Message;
use crate::core::test_support::{app, app_with_saved_hosts, app_with_startup_task, drain, host};
use crate::core::theme::ThemeMode;
use crate::hosts::view::VAULTS_HOSTS_SEARCH_ID;

/// One rendered frame of the real app, together with the simulator driving it.
type Ui<'a> = Simulator<'a, Message, iced::Theme, iced::Renderer>;

/// Render a frame of the app exactly as the view layer builds it.
fn frame(app: &App) -> Ui<'_> {
    simulator(crate::views::view(app))
}

/// Render a frame, let `interact` drive it, and collect every message the
/// widgets produced.
///
/// The messages come back instead of being applied here: a frame borrows the
/// state it was built from, so it has to be gone before the state may change.
fn collect(app: &App, interact: impl FnOnce(&mut Ui<'_>)) -> Vec<Message> {
    let mut ui = frame(app);
    interact(&mut ui);
    ui.into_messages().collect()
}

/// Hand messages to the real update handler, in order.
fn dispatch(app: &mut App, messages: impl IntoIterator<Item = Message>) {
    for message in messages {
        let _ = crate::update::update(app, message);
    }
}

/// Click the widget found by `selector`, then run the messages it produced.
fn click<S>(app: &mut App, selector: S)
where
    S: Selector + Send,
    S::Output: Bounded + Clone + Send + Sync + 'static,
{
    let description = selector.description();
    let in_closure = description.clone();
    let messages = collect(app, move |ui| {
        ui.click(selector)
            .unwrap_or_else(|error| panic!("no clickable widget matching {in_closure}: {error}"));
    });

    assert!(
        !messages.is_empty(),
        "clicking {description} produced no message"
    );
    dispatch(app, messages);
}

/// Click into the input with the given widget id and type `text` into it.
///
/// The keystrokes go through the real text input: each one comes back as the
/// widget's own `on_input`, which is the message the app stores as the query.
fn type_into(app: &mut App, id: &'static str, text: &str) {
    let messages = collect(app, |ui| {
        ui.click(iced::widget::Id::from(id))
            .unwrap_or_else(|error| panic!("no clickable input {id:?}: {error}"));
        ui.typewrite(text);
    });

    dispatch(app, messages);
}

/// Right-click the widget showing exactly `text`.
///
/// The widget tree gets the button events; the position of the pointer reaches
/// the app as the same [`Message::CursorPositionChanged`] its mouse
/// subscription produces, which is what a context menu reads when it opens.
fn right_click(app: &mut App, text: &str) {
    let mut aimed_at = None;
    let messages = collect(app, |ui| {
        let target = ui
            .find(text)
            .unwrap_or_else(|error| panic!("no widget showing {text:?}: {error}"));
        let position = target
            .visible_bounds()
            .expect("the widget is laid out")
            .center();
        aimed_at = Some(position);

        ui.point_at(position);
        ui.simulate([
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Right,
            )),
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Right,
            )),
        ]);
    });

    // The pointer has to have been recorded before the menu opens, or it lands
    // wherever the last recorded position was.
    dispatch(
        app,
        [Message::CursorPositionChanged(
            aimed_at.expect("the pointer was aimed at the widget"),
        )],
    );
    dispatch(app, messages);
}

/// Press a key the way the shell delivers it: the widget tree sees it first,
/// and when nothing claims it the app-level keyboard handler gets the same
/// [`Message::TermEvent`] its keyboard subscription produces.
fn press_key(app: &mut App, key: iced::keyboard::Key) {
    let status = {
        let mut ui = frame(app);
        ui.tap_key(key.clone())
    };

    if status == iced::event::Status::Ignored {
        let event = simulator::press_key(key, None);
        let drained = drain(crate::update::update(app, Message::TermEvent(event)));
        dispatch(app, drained.messages);
    }
}

/// Whether a frame renders a widget showing exactly `text`.
fn shows(app: &App, text: &str) -> bool {
    let mut ui = frame(app);
    ui.find(text).is_ok()
}

/// The centre of the widget showing exactly `text`, in window coordinates.
fn centre_of(app: &App, text: &str) -> iced::Point {
    let mut ui = frame(app);
    let target = ui
        .find(text)
        .unwrap_or_else(|error| panic!("no widget showing {text:?}: {error}"));

    target
        .visible_bounds()
        .expect("the widget is laid out")
        .center()
}

/// Click at a point in the window, the way the shell delivers a pointer press.
///
/// Unlike [`click`] this aims at coordinates rather than at a widget, so the
/// event also reaches the overlays on top of the widget tree.
fn click_at(app: &mut App, position: iced::Point) {
    let messages = collect(app, |ui| {
        ui.point_at(position);
        ui.simulate(simulator::click());
    });

    dispatch(app, messages);
}

/// Drive the runtime until the work it was just given has settled.
///
/// A failed instruction is a test failure: the runtime hands back the step it
/// could not carry out — an expectation whose text is not on screen, or a
/// click that found nothing to click.
fn settle<P: program::Program + 'static>(
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
) {
    let mut failed = None;

    let settled = iced::futures::executor::block_on(async {
        while let Some(event) = receiver.next().await {
            match event {
                Event::Ready => return true,
                Event::Action(action) => emulator.perform(program, action),
                Event::Failed(instruction) => {
                    failed = Some(instruction);
                    return false;
                }
            }
        }
        false
    });

    if let Some(instruction) = failed {
        panic!("the runtime could not run this step: {instruction}");
    }
    assert!(settled, "the runtime never reported that it was done");
}

/// Run one instruction and wait for the runtime to settle again.
fn run<P: program::Program + 'static>(
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
    step: &str,
) {
    emulator.run(program, Instruction::parse(step).expect("the step parses"));
    settle(emulator, program, receiver);
}

#[test]
fn clicking_another_host_card_switches_the_open_form_to_that_host() {
    let mut app = app();
    app.hosts.list = vec![
        host(1, "web-tower", "10.0.0.1"),
        host(2, "db-primary", "10.0.0.2"),
    ];
    app.hosts.grid_view = false;

    // The form is open on the first host, as if the user had started editing it.
    app.hosts.form.visible = true;
    app.hosts.form.editing_id = Some(HostId(1));
    app.hosts.form.name = "web-tower".into();
    app.hosts.form.hostname = "10.0.0.1".into();
    app.hosts.form.username = "root".into();
    app.hosts.form.port = "22".into();

    // The click is resolved by the real card widget, so this also proves the
    // card is hit-testable while the form panel is open.
    click(&mut app, "db-primary");

    assert_eq!(app.hosts.form.editing_id, Some(HostId(2)));
    assert_eq!(app.hosts.form.name, "db-primary");
    assert_eq!(app.hosts.form.hostname, "10.0.0.2");
}

#[test]
fn clicking_a_vault_card_selects_it_without_opening_the_form() {
    let mut app = app();
    app.hosts.list = vec![host(1, "web-tower", "10.0.0.1")];
    app.hosts.grid_view = false;
    app.hosts.form.visible = true;

    // With the form open the click switches it; the point here is that the
    // message came from the card and not from a test helper.
    click(&mut app, "web-tower");

    assert_eq!(app.hosts.form.editing_id, Some(HostId(1)));
    assert_eq!(app.hosts.form.port, "22");
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(1)));
    assert!(
        app.hosts.form.original.is_some(),
        "the form has a baseline to diff"
    );
}

#[test]
fn clicking_a_cursor_area_requests_keyboard_focus() {
    let mut app = app();
    app.hosts.list = vec![host(1, "web-tower", "10.0.0.1")];
    app.hosts.grid_view = false;

    let messages = collect(&app, |ui| {
        let _ = ui.click("web-tower").expect("the host card is clickable");
    });

    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::CursorFocus(CursorTarget::Hosts))),
        "clicking inside a cursor area focuses it for keyboard navigation"
    );
}

#[test]
fn clicking_a_host_card_puts_the_collection_on_the_focus_stack() {
    let program = iced::application(
        || (app(), Task::none()),
        crate::update::update,
        crate::views::view,
    )
    .title("termlet")
    .font(lucide_icons::LUCIDE_FONT_BYTES)
    .subscription(App::subscription);
    let preset = iced::Preset::new("cursor-activation", || {
        app_with_saved_hosts(&[host(1, "web-tower", "10.0.0.1")])
    });
    let (sender, mut receiver) = mpsc::channel(1);
    let mut emulator = Emulator::with_preset(
        sender,
        &program,
        Mode::Zen,
        iced::Size::new(1280.0, 720.0),
        Some(&preset),
    );

    settle(&mut emulator, &program, &mut receiver);
    run(
        &mut emulator,
        &program,
        &mut receiver,
        "expect \"web-tower\"",
    );
    run(
        &mut emulator,
        &program,
        &mut receiver,
        "click \"web-tower\"",
    );
    let (app, _window) = emulator.into_state();
    assert_eq!(
        app.focus.top(),
        Some(crate::focus::FocusId::Cursor(CursorTarget::Hosts))
    );
    assert_eq!(app.hosts.cursor.selected_key(), Some(&HostId(1)));
}

#[test]
fn typing_in_the_vault_search_hides_the_cards_that_do_not_match() {
    let mut app = app();
    app.hosts.list = vec![
        host(1, "web-tower", "10.0.0.1"),
        host(2, "db-primary", "10.0.0.2"),
    ];
    app.hosts.grid_view = false;

    assert!(shows(&app, "web-tower") && shows(&app, "db-primary"));

    type_into(&mut app, VAULTS_HOSTS_SEARCH_ID, "db");

    assert_eq!(app.hosts.search_query, "db");
    assert!(
        shows(&app, "db-primary"),
        "the card that matches the query stays"
    );
    assert!(
        !shows(&app, "web-tower"),
        "the card that does not match is filtered out of the next frame"
    );
}

#[test]
fn the_sidebar_nav_switches_which_page_is_rendered() {
    let mut app = app();

    // The boot log is in the app's state from the start; it is the Logs page
    // that puts it on screen.
    assert!(!shows(&app, "Termlet started"));

    click(&mut app, NavItem::Logs.label());

    assert_eq!(app.active_nav, NavItem::Logs);
    assert!(
        shows(&app, "Termlet started"),
        "the logs page lists the entry the app logged while booting"
    );

    click(&mut app, NavItem::Keychain.label());
    assert_eq!(app.active_nav, NavItem::Keychain);
    assert!(
        !shows(&app, "Termlet started"),
        "switching away takes the logs page down"
    );
}

#[test]
fn the_topbar_tabs_switch_the_workspace_behind_them() {
    let mut app = app();
    assert!(shows(&app, "NEW HOST"), "the vault workspace is up");

    click(&mut app, "SFTP");

    assert_eq!(app.active_tab, ActiveTab::Workspace(Workspace::Sftp));
    assert!(
        !shows(&app, "NEW HOST"),
        "the vault toolbar went away with the workspace"
    );

    click(&mut app, "Vaults");

    assert_eq!(app.active_tab, ActiveTab::Workspace(Workspace::Vaults));
    assert!(shows(&app, "NEW HOST"), "the vault workspace is back");
}

#[test]
fn clicking_the_sftp_filter_focuses_it_before_typing() {
    let program = iced::application(
        || (app(), Task::none()),
        crate::update::update,
        crate::views::view,
    )
    .title("termlet")
    .subscription(App::subscription);
    let preset = iced::Preset::new("sftp-filter-focus", || {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        (app, Task::none())
    });
    let (sender, mut receiver) = mpsc::channel(1);
    let mut emulator = Emulator::with_preset(
        sender,
        &program,
        Mode::Zen,
        iced::Size::new(1280.0, 720.0),
        Some(&preset),
    );

    settle(&mut emulator, &program, &mut receiver);

    let filter_icon = char::from(lucide_icons::Icon::Filter).to_string();
    emulator.run(
        &program,
        Instruction::Interact(Interaction::Mouse(Mouse::Click {
            button: iced::mouse::Button::Left,
            target: Some(Target::Text(filter_icon)),
        })),
    );
    settle(&mut emulator, &program, &mut receiver);
    run(&mut emulator, &program, &mut receiver, "type \"txt\"");

    let (app, _window) = emulator.into_state();
    assert_eq!(app.sftp.state.left.pane.filter, "txt");
}

#[test]
fn external_drag_names_the_panel_it_would_drop_into() {
    let mut app = app();
    app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
    // This is the Windows setup that exposed the blank overlay: Auto resolves
    // to the light palette, whose surface is the overlay background.
    app.preferences.theme_mode = ThemeMode::Auto;
    app.preferences.system_dark_mode = false;
    app.sftp.state.external_drag_paths = vec![std::path::PathBuf::from("file.txt")];

    app.sftp.state.external_drop_target = Some(crate::sftp::SftpExternalDropTarget::Local);
    assert!(shows(&app, "Drop to Local"));

    app.sftp.state.external_drop_target = Some(crate::sftp::SftpExternalDropTarget::Remote);
    app.sftp.state.right.pane.connected = true;
    assert!(shows(&app, "Upload to Remote"));

    app.sftp.state.right.pane.connected = false;
    assert!(shows(&app, "Remote not connected"));
}

#[test]
fn queued_transfers_render_below_the_active_transfer() {
    let mut app = app();
    app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
    let transfer_host = host(1, "web-tower", "10.0.0.1");

    app.sftp.state.transfer_queue = vec![crate::sftp::SftpQueuedTransfer {
        file_name: "queued-file.txt".into(),
        direction: crate::sftp::SftpTransferDirection::Download,
        total_bytes: 10,
        total_files: 1,
        target: crate::sftp::SftpTransferTarget::Local {
            directory: "/tmp".into(),
            name: "queued-file.txt".into(),
        },
        kind: crate::sftp::SftpQueuedTransferKind::DownloadFile {
            host: transfer_host,
            remote_path: "/srv/queued-file.txt".into(),
            local_path: "/tmp/queued-file.txt".into(),
            remote_size: 10,
        },
    }];
    app.sftp.state.transfer_progress = Some(std::sync::Arc::new(std::sync::Mutex::new(
        crate::sftp::SftpTransferProgress {
            file_name: "active-file.txt".into(),
            direction: crate::sftp::SftpTransferDirection::Upload,
            transferred_bytes: 50,
            total_bytes: 100,
            cancelled: false,
            current_file: "active-file.txt".into(),
            files_completed: 0,
            total_files: 1,
            resumed: false,
            start_time: None,
            speed: 0.0,
            last_speed_update: None,
        },
    )));

    let mut ui = frame(&app);
    let active = ui
        .find("Uploading active-file.txt")
        .expect("the active transfer is visible")
        .visible_bounds()
        .expect("the active transfer is laid out");
    let queued = ui
        .find("queued-file.txt")
        .expect("the queued transfer is visible")
        .visible_bounds()
        .expect("the queued transfer is laid out");

    assert!(
        queued.center().y > active.center().y,
        "the waiting row should be below the in-flight row"
    );
}

#[test]
fn transfer_strip_blocks_hover_from_file_rows_behind_it() {
    let mut app = app();
    app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
    app.sftp.state.left.pane.entries = (0..40)
        .map(|index| FileEntry {
            name: format!("file-{index:02}.txt"),
            file_type: FileEntryType::File,
            size: Some(10),
            modified: "2026-01-01 00:00".into(),
            modified_epoch: Some(1),
            permissions: "-rw-r--r--".into(),
            kind: "Text".into(),
        })
        .collect();
    app.sftp
        .state
        .transfer_results
        .record(crate::sftp::SftpTransferResult {
            file_name: "release.tar.gz".into(),
            status: crate::sftp::SftpTransferResultStatus::Failed,
            error: Some("Download incomplete: received 512 of 1024 bytes".into()),
        });

    let messages = collect(&app, |ui| {
        let bar = ui
            .find("release.tar.gz")
            .expect("the transfer result is visible")
            .visible_bounds()
            .expect("the transfer result is laid out")
            .center();
        ui.point_at(bar);
    });

    assert!(
        !messages.iter().any(|message| matches!(
            message,
            Message::Sftp(crate::sftp::message::Message::SftpRowHoverEnter { .. })
        )),
        "hovering the transfer strip must not activate a file row behind it"
    );
}

#[test]
fn multiple_failed_transfers_each_keep_a_status_bar() {
    let mut app = app();
    app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
    for (file_name, error) in [
        ("first.iso", "receipt failed"),
        ("second.iso", "connection reset"),
    ] {
        app.sftp
            .state
            .transfer_results
            .record(crate::sftp::SftpTransferResult {
                file_name: file_name.into(),
                status: crate::sftp::SftpTransferResultStatus::Failed,
                error: Some(error.into()),
            });
    }

    let mut ui = frame(&app);
    let first = ui
        .find("first.iso")
        .expect("the first failure is visible")
        .visible_bounds()
        .expect("the first failure is laid out");
    let second = ui
        .find("second.iso")
        .expect("the second failure is visible")
        .visible_bounds()
        .expect("the second failure is laid out");

    assert!(
        second.center().y > first.center().y,
        "each failed transfer should keep its own stacked status bar"
    );
}

#[test]
fn double_clicking_empty_topbar_space_requests_maximize() {
    let mut app = app();
    let anchor = centre_of(&app, "Vaults");
    // The top bar starts with 8px of padding, so this lands on its drag area
    // rather than on the Vaults button.
    let empty_gutter = iced::Point::new(4.0, anchor.y);

    let messages = collect(&app, |ui| {
        ui.point_at(empty_gutter);
        ui.simulate(simulator::click());
        ui.simulate(simulator::click());
    });

    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::WindowMaximize)),
        "the second press on the top bar must produce a maximize request"
    );

    dispatch(&mut app, messages);
}

#[test]
fn the_settings_picker_opens_on_its_trigger_and_closes_on_a_click_outside_it() {
    let mut app = app();
    let stored = app.preferences.theme_mode;

    click(&mut app, NavItem::Settings.label());

    // The trigger carries the current choice; opening the picker is the real
    // click on the real button.
    click(&mut app, stored.label());
    assert_eq!(
        app.active_dropdown,
        Some(crate::core::dropdown::DropdownTarget::SettingsTheme)
    );

    // The popup belongs to an overlay, so a press outside it has to be aimed
    // by coordinates. It closes the picker and is swallowed on the way — the
    // nav item under the pointer is not activated.
    let nav_item = centre_of(&app, NavItem::Logs.label());
    click_at(&mut app, nav_item);

    assert_eq!(app.active_dropdown, None);
    assert_eq!(
        app.active_nav,
        NavItem::Settings,
        "the page under the open picker does not see the click"
    );
    assert_eq!(app.preferences.theme_mode, stored);
}

#[test]
fn the_history_limit_dropdown_opens_on_its_trigger_and_closes_on_a_click_outside_it() {
    let mut app = app();
    click(&mut app, NavItem::Settings.label());

    // The trigger carries the limit in force. The option rows are overlay
    // content, which the headless selector does not reach — what a pick stores
    // is covered where the message is handled — so this is the trigger's own
    // wiring: it opens the list and an outside press closes it.
    click(&mut app, "1000");
    assert_eq!(
        app.active_dropdown,
        Some(crate::core::dropdown::DropdownTarget::SettingsHistoryLimit)
    );

    let nav_item = centre_of(&app, NavItem::Logs.label());
    click_at(&mut app, nav_item);
    assert_eq!(app.active_dropdown, None);
    assert_eq!(
        app.active_nav,
        NavItem::Settings,
        "the page under the open list does not see the click"
    );
    assert_eq!(app.preferences.command_history_limit, 1000);
}

#[test]
fn the_theme_dropdown_can_be_driven_from_the_keyboard() {
    let mut app = app();
    app.preferences.theme_mode = crate::core::theme::ThemeMode::Dark;
    click(&mut app, NavItem::Settings.label());
    click(&mut app, crate::core::theme::ThemeMode::Dark.label());

    press_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown),
    );
    press_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
    );

    assert_eq!(
        app.preferences.theme_mode,
        crate::core::theme::ThemeMode::Light
    );
    assert_eq!(app.active_dropdown, None);
}

#[test]
fn a_searchable_dropdown_walks_only_the_filtered_options() {
    let mut app = app();
    app.hosts.list = vec![
        host(1, "web", "10.0.0.1"),
        host(2, "db-primary", "10.0.0.2"),
    ];

    click(&mut app, "NEW HOST");
    assert!(app.hosts.form.visible);
    app.hosts.form_anim = 1.0;
    click(
        &mut app,
        crate::core::i18n::t_static(rust_i18n::t!("hosts.no_jump_host")),
    );
    dispatch(
        &mut app,
        [Message::Hosts(
            crate::hosts::message::Message::FormJumpSearch("db".into()),
        )],
    );
    press_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown),
    );
    press_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter),
    );

    assert_eq!(app.hosts.form.jump_host_id, Some(HostId(2)));
    assert_eq!(app.active_dropdown, None);
}

#[test]
fn the_quit_dialog_is_cancelled_from_its_own_button() {
    let mut app = app();
    // Settled, as it is once the entrance animation has finished.
    app.confirm_dialog_anim = 1.0;

    // What the shell does when the window is closed: the app asks instead of
    // quitting on the spot.
    let _ = crate::update::update(&mut app, Message::WindowClose);
    assert!(app.show_quit_confirm);

    click(&mut app, "Cancel");

    assert!(!app.show_quit_confirm, "the app stays open");
}

#[test]
fn deleting_a_host_from_its_context_menu_asks_first_and_then_removes_it() {
    let mut app = app();
    app.hosts.list = vec![
        host(1, "web-tower", "10.0.0.1"),
        host(2, "db-primary", "10.0.0.2"),
    ];
    app.hosts.grid_view = false;

    right_click(&mut app, "web-tower");
    assert_eq!(
        app.hosts.context_menu.as_ref().map(|menu| menu.host_id),
        Some(HostId(1)),
        "the menu is about the card that was right-clicked"
    );

    // The menu entry opens the confirmation rather than deleting.
    click(&mut app, "Delete");
    assert_eq!(app.hosts.delete_pending, Some(HostId(1)));
    assert!(
        app.hosts.list.iter().any(|h| h.id == HostId(1)),
        "nothing is deleted before the confirmation is answered"
    );

    click(&mut app, "Delete");

    assert!(
        !app.hosts.list.iter().any(|h| h.id == HostId(1)),
        "the confirmed host is gone"
    );
    assert!(
        app.hosts.list.iter().any(|h| h.id == HostId(2)),
        "the host that was not picked stays"
    );
}

#[test]
fn escape_dismisses_the_modal_on_top_and_leaves_the_form_underneath_alone() {
    let mut app = app();
    app.hosts.list = vec![host(1, "web-tower", "10.0.0.1")];
    app.hosts.grid_view = false;

    // The toolbar button is how the form gets opened in the first place.
    click(&mut app, "NEW HOST");
    assert!(app.hosts.form.visible, "the new-host button opens the form");

    // Closing the window raises the quit confirmation over it.
    let _ = crate::update::update(&mut app, Message::WindowClose);
    assert!(app.show_quit_confirm);

    press_key(
        &mut app,
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
    );

    assert!(
        !app.show_quit_confirm,
        "the modal on top is what Escape takes down"
    );
    assert!(
        app.hosts.form.visible,
        "the form underneath is left alone by that press"
    );
}

#[test]
fn the_whole_app_boots_and_runs_headlessly() {
    // The same program `main` builds, except that the state comes from the test
    // fixture (in-memory database, scratch data directory).
    let program = iced::application(
        || (app(), Task::none()),
        crate::update::update,
        crate::views::view,
    )
    .title("termlet")
    .subscription(App::subscription);

    // The preset carries the app's own startup task, so the runtime really runs
    // the boot pipeline instead of an empty one.
    let preset = iced::Preset::new("headless", app_with_startup_task);
    let (sender, mut receiver) = mpsc::channel(1);
    let mut emulator = Emulator::with_preset(
        sender,
        &program,
        Mode::Zen,
        iced::Size::new(1280.0, 720.0),
        Some(&preset),
    );

    // `Zen` waits for the whole task chain, so this covers the real startup
    // pipeline: the bulk data load, the known-hosts parse and the SFTP local
    // listing, each of which comes back as a message.
    settle(&mut emulator, &program, &mut receiver);

    // A message sent through the runtime is handled by the real update
    // function, and the resulting state is observable.
    emulator.update(&program, Message::SetTheme(ThemeMode::Novel));

    let (app, _window) = emulator.into_state();
    assert_eq!(app.preferences.theme_mode, ThemeMode::Novel);
    assert!(
        !app.hosts_loading,
        "the startup data load never delivered its result"
    );
    assert!(
        app.logs
            .iter()
            .any(|log| log.message.contains("Termlet started")),
        "the app did not finish booting: {:?}",
        app.logs.iter().map(|log| &log.message).collect::<Vec<_>>()
    );
}

#[test]
fn the_headless_runtime_loads_the_saved_vault_and_follows_instructions() {
    // The same program `main` builds, except that the preset boots on a
    // database that already holds a vault.
    let program = iced::application(
        || (app(), Task::none()),
        crate::update::update,
        crate::views::view,
    )
    .title("termlet")
    .subscription(App::subscription);

    let preset = iced::Preset::new("saved-vault", || {
        app_with_saved_hosts(&[
            host(1, "web-tower", "10.0.0.1"),
            host(2, "db-primary", "10.0.0.2"),
        ])
    });

    let (sender, mut receiver) = mpsc::channel(1);
    let mut emulator = Emulator::with_preset(
        sender,
        &program,
        Mode::Zen,
        iced::Size::new(1280.0, 720.0),
        Some(&preset),
    );

    // Booting runs the real startup load, so whatever is on screen now came
    // back from the database.
    settle(&mut emulator, &program, &mut receiver);
    run(
        &mut emulator,
        &program,
        &mut receiver,
        "expect \"db-primary\"",
    );

    // An instruction is a step through the runtime like any other: the widget
    // tree produces the message, the handler takes it, and the next frame is
    // built from what the handler did.
    run(&mut emulator, &program, &mut receiver, "click \"Logs\"");
    run(
        &mut emulator,
        &program,
        &mut receiver,
        "expect \"Termlet started\"",
    );

    let (app, _window) = emulator.into_state();
    assert_eq!(app.active_nav, NavItem::Logs);
    assert_eq!(app.hosts.list.len(), 2, "the vault came back from storage");
}
