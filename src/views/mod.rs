pub mod content;
pub mod logs;
pub mod new_tab;
pub mod right_sidebar;
pub mod settings;
pub mod sidebar;
pub mod topbar;
pub mod workspaces;

use iced::widget::{
    Space, Stack, button, column, container, mouse_area, row, scrollable, text, text_input,
};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};

use crate::core::context_menu::ContextMenuAction as Action;
use crate::core::data::OsIcon;
use crate::core::diagnostics::Level;

use crate::app::{ActiveTab, App, Workspace};
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::{self};
use crate::core::widgets::control_padding;
use crate::overlay::{self, Overlay};
#[cfg(test)]
use crate::sftp::SftpPane;
use crate::sftp::SftpTransferModal;
use crate::ssh_connect::SshConnectionState;
use crate::terminal::TabKind;
use crate::ui::confirm_dialog::{
    accent_outline_button, cancel_button_with_label, confirm_button, message_confirm,
};
use crate::ui::message_dialog;
use crate::ui::popup_notification::{bottom_right_popup_frame, popup_action_button, popup_header};
use rust_i18n::t;

/// Height of a navigation/list row: the vaults sidebar, the SFTP sidebar and
/// the right sidebar's item rows.
///
/// One of three independent height knobs — the others are [`TAB_HEIGHT`] (the
/// topbar tab strip) and [`crate::core::widgets::CONTROL_HEIGHT`] (toolbar
/// buttons, dropdown triggers and fields). They share a value where that reads
/// as one rhythm, but each can move on its own.
pub const ITEM_HEIGHT: f32 = 34.0;

use self::content::view_content;
use self::new_tab::view_new_tab;
use self::sidebar::view_sidebar;
use crate::hosts::transfer_view::view_host_transfer_dialog;
use crate::hosts::view::{view_host_context_menu, view_host_delete_confirm};
use crate::keychain::view::{
    view_identity_delete_confirm, view_keychain_context_menu, view_keychain_delete_confirm,
};
use crate::known_hosts::view::{view_known_host_context_menu, view_known_host_delete_confirm};
use crate::port_forward::view::{
    view_port_forward_context_menu, view_port_forward_delete_confirm,
    view_port_forward_key_confirm, view_port_forward_log_modal, view_port_forward_password,
};
use crate::sftp::view::{
    view_cancel_transfer_confirm as view_sftp_cancel_transfer_confirm,
    view_delete_confirm as view_sftp_delete_confirm,
    view_new_folder_dialog as view_sftp_new_folder_dialog,
    view_overwrite_confirm as view_sftp_overwrite_confirm,
    view_rename_dialog as view_sftp_rename_dialog, view_sftp,
};
use crate::snippets::view::{
    view_snippet_context_menu, view_snippet_delete_confirm, view_snippet_package_delete_confirm,
};
use crate::ssh_connect::view::view_ssh_connect;
use crate::terminal::view::{view_split_terminal, view_terminal};

pub const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub const MY_ICONS_FONT: Font = Font {
    family: iced::font::Family::Name("my-icons"),
    ..Font::DEFAULT
};

fn tab_icon(icon: lucide_icons::Icon, color: iced::Color) -> iced::widget::Text<'static> {
    text(char::from(icon))
        .font(LUCIDE_FONT)
        .size(14)
        .color(color)
}

pub fn view_os_icon<'a>(icon: OsIcon, size: f32, color: Color) -> Element<'a, Message> {
    let scaled = size * 1.2;
    match icon {
        OsIcon::Lucide(icon) => text(char::from(icon))
            .font(LUCIDE_FONT)
            .size(scaled)
            .color(color)
            .into(),
        OsIcon::Os(icon) => text(icon)
            .font(MY_ICONS_FONT)
            .size(scaled)
            .color(color)
            .into(),
    }
}

/// Height of a topbar tab: workspace tabs (Vaults / SFTP), terminal session
/// tabs, new-tab entries and the inline `+` button.
///
/// Deliberately a knob of its own: the tab strip can be tuned without moving
/// the sidebar items ([`ITEM_HEIGHT`]) or the toolbar controls
/// ([`crate::core::widgets::CONTROL_HEIGHT`]).
pub const TAB_HEIGHT: f32 = 34.0;

pub(crate) const MAX_TAB_WIDTH: f32 = 176.0;
pub(crate) const TAB_MIN_WIDTH: f32 = 142.0;
const TAB_H_PADDING: f32 = 10.0;
const TAB_PADDING: Padding = Padding {
    top: 0.0,
    right: TAB_H_PADDING,
    bottom: 0.0,
    left: TAB_H_PADDING,
};

/// Returns a zero-size, non-void placeholder for Stack layers.
/// This keeps widget tree structure stable across frames, preventing
/// iced from discarding scrollable state in the layer underneath.
fn empty_overlay<'a>() -> Element<'a, Message> {
    // Space::new() → {Shrink, Shrink} → non-void per Size::is_void().
    // Wrapping in mouse_area prevents accidental interaction.
    mouse_area(Space::new()).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    // Anything over 16 ms of rendering (one 60 fps frame) logs a warning.
    let _view_guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "VIEW",
        "view",
        std::time::Duration::from_millis(16),
    );

    let p = theme::palette(app.effective_theme_mode());

    let body: Element<'_, Message> = match app.active_tab {
        ActiveTab::Workspace(Workspace::Sftp) => view_sftp(app),
        ActiveTab::Workspace(Workspace::Vaults) => column![
            container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(
                move |_: &Theme| container::Style {
                    background: Some(Background::Color(p.border)),
                    ..container::Style::default()
                }
            ),
            row![view_sidebar(app), view_content(app)].height(Length::Fill),
        ]
        .into(),
        ActiveTab::Tab(tid) => match app.terminal.tab_registry.get(&tid).map(|t| t.kind) {
            Some(TabKind::Session(sid)) => view_terminal(app, sid),
            Some(TabKind::Split(split_id)) => view_split_terminal(app, split_id),
            Some(TabKind::Draft(did)) => match app.terminal.drafts.get(&did) {
                Some(draft) if draft.ssh.attempt.state != SshConnectionState::Idle => {
                    view_ssh_connect(&draft.ssh, app)
                }
                _ => view_new_tab(app, did),
            },
            None => empty_overlay(),
        },
    };

    let base_content = column![topbar::view_topbar(app), body];

    let bottom_right_popup =
        app.command_frequency_prompt
            .as_ref()
            .map_or_else(empty_overlay, |prompt| {
                view_command_frequency_prompt(
                    prompt,
                    app.command_frequency_prompt_anim,
                    app.command_frequency_prompt_seq,
                    &p,
                )
            });

    let active_overlay = overlay::active(app);
    let hosted_overlay = active_overlay
        .filter(|overlay| overlay.is_hosted())
        .map(|overlay| view_overlay(app, overlay, p));
    let base_content = crate::ui::modal::modal(base_content.into(), hosted_overlay);
    let popup_layers_visible = active_overlay.is_none();

    // Non-modal notifications and pointer popups remain separate layers. Modal
    // content is owned by `ModalHost`, so adding a dialog no longer means
    // reserving another numbered Stack slot.
    let children: Vec<Element<'_, Message>> = vec![
        base_content,
        if let Some(toast) = app.toast.as_ref() {
            let eased = crate::ui::helpers::ease_out_cubic(app.toast_anim);
            view_toast(&toast.message, &toast.kind, eased, &p)
        } else {
            empty_overlay()
        },
        bottom_right_popup,
        if popup_layers_visible && let Some(menu) = app.hosts.context_menu.as_ref() {
            view_host_context_menu(menu, p)
        } else {
            empty_overlay()
        },
        if popup_layers_visible && let Some(menu) = app.port_forwards.context_menu.as_ref() {
            view_port_forward_context_menu(app, menu, p)
        } else {
            empty_overlay()
        },
        if popup_layers_visible && let Some(menu) = app.snippets.context_menu.as_ref() {
            view_snippet_context_menu(menu, p)
        } else {
            empty_overlay()
        },
        if popup_layers_visible && let Some(menu) = app.keychain.context_menu.as_ref() {
            view_keychain_context_menu(menu, p)
        } else {
            empty_overlay()
        },
        if popup_layers_visible && let Some(menu) = app.known_hosts.context_menu.as_ref() {
            view_known_host_context_menu(menu, p)
        } else {
            empty_overlay()
        },
        if popup_layers_visible && let Some(menu) = app.terminal.context_menu.as_ref() {
            view_term_context_menu(menu, &p)
        } else {
            empty_overlay()
        },
        crate::terminal::view::view_spotlight_overlay(app),
    ];

    Stack::with_children(children)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn view_overlay(app: &App, overlay: Overlay, p: theme::ThemePalette) -> Element<'_, Message> {
    match overlay {
        Overlay::CommandInfo => right_sidebar::view_command_info_modal(app),
        Overlay::WorkspaceSave => view_workspace_save_dialog(app, &p),
        Overlay::WorkspaceSaveConflict => view_workspace_save_conflict_dialog(app, &p),
        Overlay::WorkspaceRename => view_workspace_rename_dialog(app, &p),
        Overlay::WorkspaceDelete => view_workspace_delete_confirm(app, &p),
        Overlay::WorkspaceImport => view_workspace_import_confirm(app, &p),
        Overlay::WorkspaceApply => {
            app.pending_apply_workspace
                .as_ref()
                .map_or_else(empty_overlay, |pending| {
                    view_workspace_apply_confirm(
                        pending,
                        app.visible_tab_ids().len(),
                        app.locked_terminal_tab_count(),
                        &p,
                        app.confirm_dialog_anim,
                    )
                })
        }
        Overlay::WorkspaceRecovery => app
            .workspace_recovery
            .pending
            .as_ref()
            .map_or_else(empty_overlay, |snapshot| {
                view_auto_restore_confirm(snapshot, &p, app.confirm_dialog_anim)
            }),
        Overlay::Crash => view_crash_dialog(&p, &app.crash_reports, false, app.confirm_dialog_anim),
        Overlay::Quit => {
            view_quit_confirm(&p, app.confirm_dialog_anim, app.locked_terminal_tab_count())
        }
        Overlay::LockedTabClose => app
            .pending_locked_close
            .as_ref()
            .map_or_else(empty_overlay, |pending| {
                view_locked_close_confirm(&p, app.confirm_dialog_anim, pending)
            }),
        Overlay::HostDelete => view_host_delete_confirm(app, p),
        Overlay::HostExportPassword | Overlay::HostImportPassword | Overlay::HostImportConfirm => {
            app.hosts
                .transfer_dialog
                .as_ref()
                .map_or_else(empty_overlay, |dialog| {
                    view_host_transfer_dialog(dialog, app, p)
                })
        }
        Overlay::PortForwardDelete => view_port_forward_delete_confirm(app, p),
        Overlay::PortForwardLogs => view_port_forward_log_modal(app, p),
        Overlay::PortForwardKeyConfirmation { .. } => view_port_forward_key_confirm(app, p),
        Overlay::PortForwardPassword { .. } => view_port_forward_password(app, p),
        Overlay::SnippetDelete => view_snippet_delete_confirm(app, p),
        Overlay::SnippetPackageDelete => view_snippet_package_delete_confirm(app, p),
        Overlay::KeychainDelete => view_keychain_delete_confirm(app, p),
        Overlay::IdentityDelete => view_identity_delete_confirm(app, p),
        Overlay::KnownHostDelete => view_known_host_delete_confirm(app, p),
        Overlay::SftpDelete => app
            .sftp
            .state
            .delete_confirm
            .as_ref()
            .map_or_else(empty_overlay, |pending| {
                view_sftp_delete_confirm(pending, p, app.confirm_dialog_anim)
            }),
        Overlay::SftpTransfer => {
            app.sftp
                .state
                .transfer_modal
                .as_ref()
                .map_or_else(empty_overlay, |modal| match modal {
                    SftpTransferModal::Discard { file_name, .. } => {
                        view_sftp_cancel_transfer_confirm(file_name, p, app.confirm_dialog_anim)
                    }
                    SftpTransferModal::Overwrite(pending) => view_sftp_overwrite_confirm(
                        pending,
                        p,
                        !app.sftp.state.transfer_queue.is_empty(),
                        app.confirm_dialog_anim,
                    ),
                })
        }
        Overlay::SftpRename => app
            .sftp
            .state
            .rename_pending
            .as_ref()
            .map_or_else(empty_overlay, |pending| {
                view_sftp_rename_dialog(pending, p, app.confirm_dialog_anim)
            }),
        Overlay::SftpNewFolder => app
            .sftp
            .state
            .new_folder_pending
            .as_ref()
            .map_or_else(empty_overlay, |pending| {
                view_sftp_new_folder_dialog(pending, p, app.confirm_dialog_anim)
            }),
        Overlay::SshKeyConfirmation | Overlay::SftpKeyConfirmation { .. } => empty_overlay(),
    }
}

fn view_term_context_menu<'a>(
    menu: &'a crate::terminal::TerminalContextMenu,
    p: &theme::ThemePalette,
) -> Element<'a, Message> {
    const MENU_WIDTH: f32 = 200.0;

    let session_id = menu.session_id;
    let has_tabs_to_right = menu.has_tabs_to_right;
    let is_split = menu.is_split;
    let palette = *p;
    let actions = menu.actions();
    let focused = menu.cursor.selected_or_first(&actions).copied();

    crate::ui::overlay::context_menu_frame(
        menu.position,
        move || {
            let menu_item = |action: Action,
                             icon: lucide_icons::Icon,
                             label: &'static str,
                             msg: Message,
                             destructive: bool| {
                crate::core::widgets::menu_item(
                    icon,
                    label,
                    msg,
                    destructive,
                    focused == Some(action),
                    palette,
                )
            };

            let lock_label = if menu.locked {
                t_static(t!("term_menu.unlock"))
            } else {
                t_static(t!("term_menu.lock"))
            };

            let mut items: Vec<Element<'_, Message>> = vec![
                menu_item(
                    Action::TerminalNewWindow,
                    lucide_icons::Icon::ExternalLink,
                    t_static(t!("term_menu.new_window")),
                    Message::Terminal(crate::terminal::message::Message::DuplicateInNewWindow(
                        session_id,
                    )),
                    false,
                ),
                menu_item(
                    Action::TerminalToggleLock,
                    if menu.locked {
                        lucide_icons::Icon::LockOpen
                    } else {
                        lucide_icons::Icon::Lock
                    },
                    lock_label,
                    Message::Terminal(crate::terminal::message::Message::TermToggleLock(
                        session_id,
                    )),
                    false,
                ),
                menu_item(
                    Action::TerminalClose,
                    lucide_icons::Icon::X,
                    t_static(t!("term_menu.close")),
                    Message::Terminal(crate::terminal::message::Message::TermClose(session_id)),
                    false,
                ),
                menu_item(
                    Action::TerminalCloseOthers,
                    lucide_icons::Icon::SquareX,
                    t_static(t!("term_menu.close_others")),
                    Message::Terminal(crate::terminal::message::Message::TermCloseOthers(
                        session_id,
                    )),
                    false,
                ),
            ];
            if !is_split {
                items.insert(
                    0,
                    menu_item(
                        Action::TerminalDuplicate,
                        lucide_icons::Icon::Copy,
                        t_static(t!("term_menu.duplicate")),
                        Message::Terminal(crate::terminal::message::Message::DuplicateSession(
                            session_id,
                        )),
                        false,
                    ),
                );
                items.insert(
                    2,
                    menu_item(
                        Action::TerminalRename,
                        lucide_icons::Icon::Pencil,
                        t_static(t!("term_menu.rename")),
                        Message::Terminal(crate::terminal::message::Message::TermRenameStart(
                            session_id,
                        )),
                        false,
                    ),
                );
            }
            if has_tabs_to_right {
                items.push(menu_item(
                    Action::TerminalCloseRight,
                    lucide_icons::Icon::PanelRightClose,
                    t_static(t!("term_menu.close_right")),
                    Message::Terminal(crate::terminal::message::Message::TermCloseRight(
                        session_id,
                    )),
                    true,
                ));
            }
            items
        },
        MENU_WIDTH,
        Message::Terminal(crate::terminal::message::Message::TermContextMenuClose),
        *p,
    )
}

fn view_toast<'a>(
    message: &str,
    kind: &crate::app::ToastKind,
    anim: f32,
    p: &theme::ThemePalette,
) -> Element<'a, Message> {
    let (icon, color) = match kind {
        crate::app::ToastKind::Info => (lucide_icons::Icon::Check, p.success),
        crate::app::ToastKind::Warning => (lucide_icons::Icon::AlertTriangle, p.warning),
        crate::app::ToastKind::Error => (lucide_icons::Icon::X, p.danger),
    };
    let card = container(
        row![
            text(char::from(icon))
                .font(LUCIDE_FONT)
                .size(14)
                .color(Color { a: anim, ..color }),
            text(message.to_string())
                .size(13)
                .color(Color {
                    a: anim,
                    ..p.text_pri
                })
                .width(Length::Shrink)
                .ellipsis(text::Ellipsis::End)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([12.0, 18.0]))
    .max_width(500)
    .style({
        let bg = p.sidebar_bg;
        let border = p.border;
        move |_: &Theme| container::Style {
            background: Some(Background::Color(Color {
                a: 0.98 * anim,
                ..bg
            })),
            border: Border {
                radius: 10.0.into(),
                width: 1.0,
                color: Color {
                    a: border.a * anim,
                    ..border
                },
            },
            shadow: iced::Shadow {
                color: Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.35 * anim,
                },
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        }
    });

    // Clipped to a fixed-height container and animated by top padding: at
    // anim=0 the card sits below the clip, and it rises into place as anim→1.
    let slide_offset = (1.0 - anim) * 52.0;
    container(
        iced::widget::column![
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
            container(container(card).padding(Padding {
                top: slide_offset,
                right: 0.0,
                bottom: 0.0,
                left: 0.0,
            }),)
            .clip(true)
            .height(Length::Fixed(44.0)),
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fixed(8.0)),
        ]
        .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Bottom-right popup suggesting the user save a frequently-executed command
/// as a snippet.
fn view_command_frequency_prompt<'a>(
    prompt: &'a crate::app::CommandFrequencyPrompt,
    anim: f32,
    seq: u64,
    p: &theme::ThemePalette,
) -> Element<'a, Message> {
    let eased = crate::ui::helpers::ease_out_cubic(anim);
    let title = t!("command_frequency.title", count = prompt.count).to_string();
    let display_cmd = crate::ui::helpers::truncate_display_text(&prompt.command, 40);

    let content = column![
        popup_header(
            lucide_icons::Icon::Zap,
            p.warning,
            &title,
            Message::CommandFrequencyDismiss(seq),
            p,
            anim,
        ),
        text(display_cmd)
            .size(13)
            .font(Font::MONOSPACE)
            .color(Color {
                a: eased,
                ..p.text_pri
            }),
        popup_action_button(
            Some(lucide_icons::Icon::Plus),
            t_static(t!("command_frequency.add")),
            Message::CommandFrequencyAddToSnippet {
                command: prompt.command.clone(),
            },
            p,
            anim,
        ),
    ]
    .spacing(10);

    bottom_right_popup_frame(content.into(), p, anim)
}

fn view_quit_confirm<'a>(
    p: &theme::ThemePalette,
    anim: f32,
    locked_count: usize,
) -> Element<'a, Message> {
    let body = if locked_count > 0 {
        t!("app.quit_body_locked", count = locked_count).to_string()
    } else {
        t_static(t!("app.quit_body")).to_string()
    };

    let quit_now_button = confirm_button(
        t_static(t!("app.quit_without_save")),
        Message::WindowCloseConfirm,
        p.danger,
        Color::WHITE,
        false,
    );

    let actions: Vec<Element<'static, Message>> = vec![
        cancel_button_with_label(
            t_static(t!("settings.cancel")),
            Message::WindowCloseCancel,
            p,
        ),
        quit_now_button,
    ];

    message_dialog::message_dialog(
        t_static(t!("app.quit_title")),
        Some(body),
        column![].into(),
        actions,
        Message::Noop,
        460.0,
        p,
        anim,
    )
}

fn view_workspace_apply_confirm<'a>(
    pending: &crate::workspace::PendingWorkspaceApply,
    current_count: usize,
    locked_count: usize,
    p: &theme::ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let body = t!(
        "workspace.apply_body",
        name = pending.name.as_str(),
        current = current_count,
        locked = locked_count,
        target = pending.snapshot.tabs.len()
    )
    .to_string();

    message_confirm(
        t_static(t!("workspace.apply_title")),
        body,
        t_static(t!("workspace.apply_confirm")),
        Message::WorkspaceApplyConfirm,
        &Message::WorkspaceApplyCancel,
        None,
        p.accent,
        Some(Message::WorkspaceApplyCancel),
        p,
        anim,
    )
}

fn view_auto_restore_confirm<'a>(
    snapshot: &crate::workspace::WorkspaceSnapshot,
    p: &theme::ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let body = t!(
        "workspace.auto_restore_body",
        tabs = snapshot.tabs.len(),
        sessions = snapshot.sessions.len()
    )
    .to_string();

    message_confirm(
        t_static(t!("workspace.auto_restore_title")),
        body,
        t_static(t!("workspace.auto_restore_confirm")),
        Message::WorkspaceRecovery(crate::workspace::recovery::Message::RestoreConfirm),
        &Message::WorkspaceRecovery(crate::workspace::recovery::Message::RestoreDismiss),
        Some(t_static(t!("workspace.auto_restore_dismiss")).to_string()),
        p.accent,
        Some(Message::WorkspaceRecovery(
            crate::workspace::recovery::Message::RestoreDismiss,
        )),
        p,
        anim,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
fn view_workspace_name_dialog<'a>(
    title: &'static str,
    value: &'a str,
    input_id: &'static str,
    on_input: fn(String) -> Message,
    confirm_msg: Message,
    cancel_msg: Message,
    confirm_label: &'static str,
    p: &theme::ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let palette = *p;
    let name_input = text_input(t_static(t!("workspace.name_placeholder")), value)
        .id(iced::widget::Id::new(input_id))
        .on_input(on_input)
        .padding(control_padding(13.0, 8.0))
        .size(13)
        .style(move |_, s| crate::core::widgets::input_style_with_palette(&palette, s));

    let cancel_btn = cancel_button_with_label(t_static(t!("settings.cancel")), cancel_msg, p);
    let confirm_btn = confirm_button(
        confirm_label,
        confirm_msg,
        p.accent,
        p.text_on_color,
        value.trim().is_empty(),
    );
    let actions: Vec<Element<'static, Message>> = vec![cancel_btn, confirm_btn];

    message_dialog::message_dialog(
        title,
        None::<String>,
        name_input.into(),
        actions,
        Message::Noop,
        360.0,
        p,
        anim,
    )
}

fn view_workspace_save_dialog<'a>(app: &'a App, p: &theme::ThemePalette) -> Element<'a, Message> {
    view_workspace_name_dialog(
        t_static(t!("workspace.save_title")),
        &app.workspace_manager.save_name,
        "workspace_save_name",
        Message::WorkspaceSaveName,
        Message::WorkspaceSaveConfirm,
        Message::WorkspaceSaveCancel,
        t_static(t!("workspace.save_confirm")),
        p,
        app.confirm_dialog_anim,
    )
}

fn view_workspace_save_conflict_dialog<'a>(
    app: &'a App,
    p: &theme::ThemePalette,
) -> Element<'a, Message> {
    let name = app
        .workspace_manager
        .save_conflict
        .as_deref()
        .unwrap_or_default();
    let body = t!("workspace.duplicate_body", name = name).to_string();

    let keep_duplicate_btn = cancel_button_with_label(
        t_static(t!("workspace.duplicate_keep")),
        Message::WorkspaceSaveKeepDuplicate,
        p,
    );
    let overwrite_btn = confirm_button(
        t_static(t!("workspace.duplicate_overwrite")),
        Message::WorkspaceSaveOverwrite,
        p.danger,
        Color::WHITE,
        false,
    );
    let actions: Vec<Element<'static, Message>> = vec![keep_duplicate_btn, overwrite_btn];

    message_dialog::message_dialog(
        t_static(t!("workspace.duplicate_title")),
        Some(body),
        column![].into(),
        actions,
        Message::WorkspaceSaveConflictCancel,
        420.0,
        p,
        app.confirm_dialog_anim,
    )
}

fn view_workspace_rename_dialog<'a>(app: &'a App, p: &theme::ThemePalette) -> Element<'a, Message> {
    view_workspace_name_dialog(
        t_static(t!("workspace.rename_title")),
        &app.workspace_manager.rename_name,
        "workspace_rename_name",
        Message::WorkspaceRenameInput,
        Message::WorkspaceRenameSubmit,
        Message::WorkspaceRenameCancel,
        t_static(t!("workspace.rename_confirm")),
        p,
        app.confirm_dialog_anim,
    )
}

fn view_workspace_delete_confirm<'a>(
    app: &'a App,
    p: &theme::ThemePalette,
) -> Element<'a, Message> {
    let Some(id) = app.workspace_manager.delete_id.as_ref() else {
        return empty_overlay();
    };
    let Some(saved) = app
        .workspace_manager
        .list
        .iter()
        .find(|saved| &saved.id == id)
    else {
        return empty_overlay();
    };

    message_confirm(
        t_static(t!("workspace.delete_title")),
        t!("workspace.delete_body", name = saved.name.as_str()).to_string(),
        t_static(t!("workspace.delete_confirm")),
        Message::WorkspaceDeleteConfirm,
        &Message::WorkspaceDeleteCancel,
        None,
        p.danger,
        Some(Message::WorkspaceDeleteCancel),
        p,
        app.confirm_dialog_anim,
    )
}

fn view_workspace_import_confirm<'a>(
    app: &'a App,
    p: &theme::ThemePalette,
) -> Element<'a, Message> {
    let Some(dialog) = app.workspace_manager.import_dialog.as_ref() else {
        return empty_overlay();
    };

    message_confirm(
        t_static(t!("workspace.import_confirm_title")),
        t!(
            "workspace.import_confirm_body",
            new = dialog.new_count,
            dup = dialog.dup_count
        )
        .to_string(),
        t_static(t!("workspace.import_confirm_submit")),
        Message::WorkspaceImportConfirm,
        &Message::WorkspaceImportCancel,
        None,
        p.accent,
        Some(Message::WorkspaceImportCancel),
        p,
        app.confirm_dialog_anim,
    )
}

fn view_locked_close_confirm<'a>(
    p: &theme::ThemePalette,
    anim: f32,
    pending: &crate::terminal::PendingLockedClose,
) -> Element<'a, Message> {
    message_confirm(
        t_static(t!("term_lock_close.title")),
        t!("term_lock_close.body", name = pending.label.as_str()).to_string(),
        t_static(t!("term_lock_close.confirm")),
        Message::Terminal(crate::terminal::message::Message::TermCloseLockedConfirm),
        &Message::Terminal(crate::terminal::message::Message::TermCloseLockedCancel),
        None,
        p.danger,
        None,
        p,
        anim,
    )
}

fn view_crash_dialog<'a>(
    p: &theme::ThemePalette,
    reports: &[(u64, String)],
    scrollbar_visible: bool,
    anim: f32,
) -> Element<'a, Message> {
    let mut report_text = String::new();
    for (i, (_, content)) in reports.iter().enumerate() {
        if i > 0 {
            report_text.push_str("\n\n═══════════════════════════════════\n\n");
        }
        report_text.push_str(content);
    }

    let count = reports.len();
    let title = if count == 1 {
        t!("app.crash_title").to_string()
    } else {
        t!("app.crash_title_count", count = count).to_string()
    };

    let header = column![
        text(char::from(lucide_icons::Icon::AlertTriangle))
            .font(LUCIDE_FONT)
            .size(28)
            .color(p.danger),
        text(title).size(15).color(p.text_pri),
        text(t!("app.crash_body")).size(12).color(p.text_sec),
        text(t!("app.crash_detail")).size(11).color(p.text_dim),
    ]
    .spacing(8)
    .width(Length::Fill)
    .align_x(Alignment::Center);

    let report_body = {
        let text_color = p.text_sec;
        let font_size = 11.0;
        let bar = if scrollbar_visible {
            scrollable::Scrollbar::new()
                .width(6)
                .scroller_width(6)
                .margin(4)
        } else {
            scrollable::Scrollbar::hidden()
        };
        scrollable(
            text(report_text)
                .size(font_size)
                .color(text_color)
                .font(Font::MONOSPACE),
        )
        .direction(scrollable::Direction::Vertical(bar))
        .height(Length::Fixed(320.0))
    };

    let copy_btn = accent_outline_button(t_static(t!("app.copy")), Message::CrashDialogCopy, p);

    let dismiss_btn = {
        let surface = p.surface;
        let item_hover = p.item_hover;
        let text_pri = p.text_pri;
        button(text(t!("app.dismiss")).size(12).color(text_pri))
            .on_press(Message::CrashDialogClose)
            .padding(control_padding(12.0, 18.0))
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: Some(Background::Color(if hovered {
                        item_hover
                    } else {
                        surface
                    })),
                    text_color: text_pri,
                    border: Border {
                        radius: 6.0.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                }
            })
            .cursor_interaction(mouse::Interaction::default())
    };

    let footer = row![Space::new().width(Length::Fill), copy_btn, dismiss_btn]
        .spacing(8)
        .align_y(Alignment::Center);

    message_dialog::message_dialog_frame(
        header.into(),
        report_body.into(),
        footer.into(),
        560.0,
        Message::Noop,
        p,
        anim,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{DraftId, SessionId, SplitId, Toast, ToastKind};
    use crate::core::data::{
        FileEntry, FileEntryType, HostId, KnownHost, LogCategory, NavItem, RightSidebarTab,
        Snippet, SnippetId, SnippetPackage, SnippetPackageId,
    };
    use crate::core::test_support::{app, host};
    use crate::core::theme::ThemeMode;
    use crate::hosts::state::{HostTransferDialog, HostTransferKind};
    use crate::sftp::{SftpTransferProgress, SftpTransferResult, SftpTransferResultStatus};
    use crate::ssh_connect::{SshConnectStage, SshConnectState};
    use crate::terminal::{Draft, SidebarState};
    use secrecy::SecretString;

    // Render smoke tests.
    //
    // `view()` builds the whole widget tree for a frame. Nothing here inspects
    // the result — an `Element` only means something once iced lays it out — so
    // what these cover is everything that runs before that: the panel, list and
    // dialog constructors, across the state combinations the app can actually
    // be in. A panic in any of them takes the window down on that frame, and
    // paths like "a selected row that is no longer in the list", "an empty list
    // next to an active filter" or "a missing timestamp" are exactly where that
    // happens.
    //
    // The terminal workspaces are out of reach here: their view needs a live
    // PTY session, which means spawning real shells. Everything else renders.

    /// Render one frame, so a panic surfaces as a test failure.
    fn render(app: &App) {
        let element = view(app);
        drop(element);
    }

    /// Render the right sidebar on its own, for the same reason: it only needs
    /// the sidebar's own state, not a live terminal behind it.
    fn render_sidebar(app: &App, tab: RightSidebarTab) {
        let sidebar = SidebarState {
            show: true,
            anim: 1.0,
            active_tab: tab,
            ..Default::default()
        };
        let element = crate::views::right_sidebar::view_right_sidebar(app, &sidebar);
        drop(element);
    }

    fn file_entry(name: &str, file_type: FileEntryType, size: Option<u64>) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            file_type,
            size,
            modified: "2026-01-01 00:00".into(),
            modified_epoch: Some(1),
            permissions: "-rw-r--r--".into(),
            kind: "Text".into(),
        }
    }

    #[test]
    fn the_vault_workspace_renders_empty_populated_and_filtered() {
        let mut app = app();
        render(&app);

        app.hosts.list = vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")];
        app.hosts.recent_ids = vec![HostId(2)];
        app.hosts.cursor.select(HostId(2));
        render(&app);

        app.hosts.grid_view = false;
        render(&app);

        // A filter that matches nothing renders the empty state next to an
        // active search box.
        app.hosts.search_query = "nothing matches this".into();
        render(&app);

        // A tag filter the user cleared leaves the list empty too.
        app.hosts.search_query.clear();
        app.hosts.tag_filter = crate::core::data::HostTagFilter::Tag("prod".into());
        render(&app);
    }

    #[test]
    fn the_vault_workspace_renders_its_dialogs() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];

        app.hosts.form.visible = true;
        app.hosts.form_anim = 1.0;
        render(&app);

        app.hosts.form_anim = 0.0;
        render(&app);

        app.hosts.form.visible = false;
        app.hosts.form_closing = true;
        render(&app);
        app.hosts.form_closing = false;

        app.hosts.delete_pending = Some(HostId(1));
        render(&app);
        app.hosts.delete_pending = None;

        for kind in [
            HostTransferKind::Export,
            HostTransferKind::ImportPassword,
            HostTransferKind::ImportConfirm,
        ] {
            app.hosts.transfer_dialog = Some(HostTransferDialog {
                kind,
                password: SecretString::from(String::new()),
                password_visible: false,
                busy: false,
                anim_t: 0.0,
                pending_path: None,
                pending_bytes: Vec::new(),
                pending_hosts: Vec::new(),
                new_count: 2,
                dup_count: 1,
                error: None,
            });
            render(&app);
        }

        // An export in progress and a failed import both have their own look.
        if let Some(dialog) = app.hosts.transfer_dialog.as_mut() {
            dialog.busy = true;
            dialog.anim_t = 0.5;
        }
        render(&app);
        if let Some(dialog) = app.hosts.transfer_dialog.as_mut() {
            dialog.busy = false;
            dialog.error = Some("wrong password".into());
        }
        render(&app);
    }

    #[test]
    fn the_new_tab_screen_renders_with_and_without_matches() {
        let mut app = app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];
        let draft = DraftId(1);
        app.terminal.drafts.insert(
            draft,
            Draft {
                query: String::new(),
                cursor: crate::core::list_cursor::ListCursor::new(),
                ssh: SshConnectState::default(),
            },
        );
        let tab = app.allocate_tab_id(TabKind::Draft(draft));
        app.active_tab = ActiveTab::Tab(tab);

        render(&app);

        app.terminal.drafts.get_mut(&draft).unwrap().query = "web".into();
        app.terminal
            .drafts
            .get_mut(&draft)
            .unwrap()
            .cursor
            .select(HostId(1));
        render(&app);

        app.terminal.drafts.get_mut(&draft).unwrap().query = "no such host".into();
        render(&app);

        // An app with no saved hosts offers only "open a local terminal".
        app.hosts.list.clear();
        render(&app);
    }

    #[test]
    fn the_ssh_connect_screens_render_in_every_connection_state() {
        let mut app = app();
        let draft = DraftId(1);
        app.terminal.drafts.insert(
            draft,
            Draft {
                query: String::new(),
                cursor: crate::core::list_cursor::ListCursor::new(),
                ssh: SshConnectState::default(),
            },
        );
        let tab = app.allocate_tab_id(TabKind::Draft(draft));
        app.active_tab = ActiveTab::Tab(tab);

        for state in [
            SshConnectionState::Connecting,
            SshConnectionState::WaitingForPassword,
            SshConnectionState::WaitingForKeyConfirmation,
            SshConnectionState::Connected,
            SshConnectionState::Failed,
        ] {
            let ssh = &mut app.terminal.drafts.get_mut(&draft).unwrap().ssh;
            ssh.attempt.state = state;
            ssh.attempt.pending_host = Some(host(1, "web", "10.0.0.1"));
            ssh.attempt.stage = SshConnectStage::Target;
            ssh.logs = vec!["connecting".into(), "key accepted".into()];
            render(&app);
        }

        // A jump host makes the password screen label the bastion instead, and
        // a pending key confirmation adds a fingerprint panel.
        let ssh = &mut app.terminal.drafts.get_mut(&draft).unwrap().ssh;
        ssh.attempt.state = SshConnectionState::WaitingForKeyConfirmation;
        ssh.attempt.jump_host = Some(host(2, "bastion", "10.0.0.2"));
        ssh.attempt.stage = SshConnectStage::Jump;
        ssh.attempt.pending_key_info = Some(crate::core::data::ServerKeyInfo {
            hostname: "bastion".into(),
            port: 22,
            key_type: "ssh-ed25519".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            fingerprint: "SHA256:abc".into(),
        });
        render(&app);
    }

    #[test]
    fn the_sftp_workspace_renders_its_panes_and_dialogs() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        render(&app);

        app.sftp.state.left.pane.entries = vec![
            file_entry("src", FileEntryType::Folder, None),
            file_entry("notes.txt", FileEntryType::File, Some(12)),
            file_entry("link", FileEntryType::Symlink, Some(0)),
        ];
        app.sftp.state.left.pane.selected = vec!["notes.txt".into()];
        app.sftp.state.left.pane.connected = true;
        app.sftp.state.left.pane.entries = app.sftp.state.left.pane.entries.clone();
        render(&app);

        app.sftp.state.left.pane.loading = true;
        render(&app);
        app.sftp.state.left.pane.loading = false;

        app.sftp.state.left.pane.error = Some("permission denied".into());
        render(&app);
        app.sftp.state.left.pane.error = None;

        // A path being edited, an open host picker and an open filter box.
        app.sftp.state.left.pane.path_edit = Some("/srv/app".into());
        app.sftp.state.left.pane.connected = true;
        app.sftp.state.right.host_picker.open = true;
        app.sftp.state.left.pane.filter = "txt".into();
        render(&app);

        app.sftp.sidebar_show = true;
        app.sftp.sidebar_anim = 1.0;
        render(&app);
    }

    #[test]
    fn the_sftp_workspace_renders_its_transfer_states() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.state.transfer_progress = Some(std::sync::Arc::new(std::sync::Mutex::new(
            SftpTransferProgress {
                file_name: "big.iso".into(),
                direction: crate::sftp::SftpTransferDirection::Download,
                transferred_bytes: 512,
                total_bytes: 1024,
                cancelled: false,
                current_file: "big.iso".into(),
                files_completed: 1,
                total_files: 2,
                resumed: false,
                start_time: Some(std::time::Instant::now()),
                speed: 0.0,
                last_speed_update: None,
            },
        )));
        render(&app);

        // A cancelled transfer reads differently from a running one.
        if let Some(progress) = app.sftp.state.transfer_progress.as_ref() {
            progress.lock().unwrap().cancelled = true;
        }
        render(&app);

        app.sftp.state.transfer_progress = None;
        for result in [
            SftpTransferResult {
                file_name: "big.iso".into(),
                status: SftpTransferResultStatus::Failed,
                error: Some("permission denied while writing the destination file".into()),
            },
            SftpTransferResult {
                file_name: "archive.zip".into(),
                status: SftpTransferResultStatus::Failed,
                error: Some("connection reset".into()),
            },
        ] {
            app.sftp.state.transfer_results.record(result);
        }
        render(&app);

        app.sftp.state.transfer_results.clear();
        for (status, error, file_name) in [
            (SftpTransferResultStatus::Succeeded, None, "small.txt"),
            (SftpTransferResultStatus::Cancelled, None, "dir"),
        ] {
            app.sftp.state.transfer_results.record(SftpTransferResult {
                file_name: file_name.into(),
                status,
                error,
            });
            render(&app);
        }
    }

    #[test]
    fn the_sftp_workspace_renders_its_file_operation_dialogs() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.state.left.pane.entries = vec![
            file_entry("notes.txt", FileEntryType::File, Some(12)),
            file_entry("src", FileEntryType::Folder, None),
        ];

        app.sftp.state.delete_confirm = Some(crate::sftp::SftpDeletePending {
            pane: SftpPane::Local,
            directory: "/srv/app".into(),
            names: vec!["notes.txt".into()],
            file_types: vec![FileEntryType::File],
            in_progress: false,
            remaining: 1,
            failures: Vec::new(),
        });
        render(&app);

        // A half-finished delete lists what has already failed.
        if let Some(pending) = app.sftp.state.delete_confirm.as_mut() {
            pending.in_progress = true;
            pending.failures = vec![("locked.txt".into(), "permission denied".into())];
            pending.names.push("locked.txt".into());
            pending.file_types.push(FileEntryType::File);
        }
        render(&app);
        app.sftp.state.delete_confirm = None;

        app.sftp.state.rename_pending = Some(crate::sftp::SftpRenamePending {
            pane: SftpPane::Local,
            directory: "/srv/app".into(),
            old_name: "notes.txt".into(),
            file_type: FileEntryType::File,
            new_name: "notes-2.txt".into(),
            in_progress: false,
        });
        render(&app);
        app.sftp.state.rename_pending = None;

        app.sftp.state.new_folder_pending = Some(crate::sftp::SftpNewFolderPending {
            pane: SftpPane::Local,
            directory: "/srv/app".into(),
            name: "new folder".into(),
            in_progress: false,
        });
        render(&app);
        app.sftp.state.new_folder_pending = None;

        // The overwrite prompt, with a single file and with a folder.
        let overwrite = |file_name: &str, is_folder: bool| crate::sftp::SftpOverwritePending {
            host: host(1, "web", "10.0.0.1"),
            file_name: file_name.into(),
            remote_path: "/srv/app".into(),
            source_path: "/tmp/app".into(),
            local_path: Some(std::path::PathBuf::from("/srv/app")),
            source_host: None,
            total_bytes: 2048,
            target: crate::sftp::SftpTransferTarget::Local {
                directory: "/srv/app".into(),
                name: file_name.into(),
            },
            in_progress: false,
            is_folder,
        };
        app.sftp.state.transfer_modal = Some(crate::sftp::SftpTransferModal::Overwrite(overwrite(
            "notes.txt",
            false,
        )));
        render(&app);

        app.sftp.state.transfer_modal = Some(crate::sftp::SftpTransferModal::Overwrite(overwrite(
            "src", true,
        )));
        render(&app);

        // The discard prompt can hold an overwrite decision behind it.
        app.sftp.state.transfer_modal = Some(crate::sftp::SftpTransferModal::Discard {
            file_name: "notes.txt".into(),
            queued_overwrite: Some(overwrite("notes.txt", false)),
        });
        render(&app);

        // The discard prompt without a file name behind it (an emptied queue).
        app.sftp.state.transfer_modal = Some(crate::sftp::SftpTransferModal::Discard {
            file_name: String::new(),
            queued_overwrite: None,
        });
        render(&app);
        app.sftp.state.transfer_modal = None;
    }

    #[test]
    fn the_sftp_workspace_renders_its_context_menus() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Sftp);
        app.sftp.state.left.pane.entries = vec![
            file_entry("notes.txt", FileEntryType::File, Some(12)),
            file_entry("src", FileEntryType::Folder, None),
        ];
        app.sftp.state.left.pane.selected = vec!["notes.txt".into()];

        for menu in [
            crate::sftp::SftpContextMenu {
                pane: SftpPane::Local,
                directory: "/srv/app".into(),
                name: "notes.txt".into(),
                file_type: FileEntryType::File,
                position: iced::Point::ORIGIN,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::SftpTransfer,
                    crate::core::context_menu::ContextMenuAction::SftpNewFolder,
                    crate::core::context_menu::ContextMenuAction::SftpRename,
                    crate::core::context_menu::ContextMenuAction::SftpDelete,
                ]),
            },
            crate::sftp::SftpContextMenu {
                pane: SftpPane::Local,
                directory: "/srv/app".into(),
                name: "src".into(),
                file_type: FileEntryType::Folder,
                position: iced::Point::ORIGIN,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::SftpTransfer,
                    crate::core::context_menu::ContextMenuAction::SftpNewFolder,
                    crate::core::context_menu::ContextMenuAction::SftpRename,
                    crate::core::context_menu::ContextMenuAction::SftpDelete,
                ]),
            },
            // A remote row with no selection: "download" is the only action.
            crate::sftp::SftpContextMenu {
                pane: SftpPane::Remote,
                directory: "/srv/app".into(),
                name: String::new(),
                file_type: FileEntryType::File,
                position: iced::Point::ORIGIN,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::SftpTransfer,
                    crate::core::context_menu::ContextMenuAction::SftpNewFolder,
                    crate::core::context_menu::ContextMenuAction::SftpRename,
                    crate::core::context_menu::ContextMenuAction::SftpDelete,
                ]),
            },
        ] {
            app.sftp.state.context_menu = Some(menu);
            render(&app);
        }
        app.sftp.state.context_menu = None;
    }

    #[test]
    fn every_vault_page_renders() {
        let mut app = app();
        app.active_tab = ActiveTab::Workspace(Workspace::Vaults);

        for nav in [
            NavItem::Hosts,
            NavItem::Keychain,
            NavItem::PortForwarding,
            NavItem::Snippets,
            NavItem::KnownHosts,
            NavItem::Logs,
            NavItem::Settings,
        ] {
            app.active_nav = nav;
            render(&app);
        }
    }

    #[test]
    fn the_key_chain_page_renders_its_lists_forms_and_dialogs() {
        let mut app = app();
        app.active_nav = NavItem::Keychain;
        render(&app);

        app.keychain.list = vec![
            crate::core::data::KeychainEntry {
                id: crate::core::data::KeychainEntryId(1),
                name: "github".into(),
                key_type: "ED25519".into(),
                fingerprint: "SHA256:abc".into(),
                public_key: "ssh-ed25519 AAAA".into(),
                path: "~/.ssh/github".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            },
            crate::core::data::KeychainEntry {
                id: crate::core::data::KeychainEntryId(2),
                name: "legacy".into(),
                key_type: "RSA".into(),
                fingerprint: String::new(),
                public_key: String::new(),
                path: String::new(),
                created_at: String::new(),
            },
        ];
        app.keychain
            .select_key(crate::core::data::KeychainEntryId(1));
        render(&app);

        app.keychain.grid_view = false;
        app.keychain.search_query = "nothing".into();
        render(&app);
        app.keychain.search_query.clear();

        app.keychain.form.visible = true;
        app.keychain.form.in_progress = true;
        render(&app);
        app.keychain.form.in_progress = false;

        app.keychain.identities = vec![crate::core::data::Identity {
            id: crate::core::data::IdentityId(1),
            label: "deploy".into(),
            username: "deploy".into(),
            password: SecretString::from("hunter2"),
            created_at: String::new(),
        }];
        app.keychain
            .select_identity(crate::core::data::IdentityId(1));
        app.keychain.identity_form.visible = true;
        render(&app);

        app.keychain.delete_pending = Some(crate::core::data::KeychainEntryId(1));
        render(&app);
    }

    #[test]
    fn the_port_forward_page_renders_its_rows_and_states() {
        let mut app = app();
        app.active_nav = NavItem::PortForwarding;
        app.hosts.list = vec![host(1, "bastion", "10.0.0.1")];
        render(&app);

        let forward =
            |id: u32, kind: crate::core::data::PortForwardKind| crate::core::data::PortForward {
                id: crate::core::data::PortForwardId(id),
                name: format!("forward {id}"),
                kind,
                host_id: HostId(1),
                bind_address: "127.0.0.1".into(),
                listen_port: 8080 + id as u16,
                dest_host: "10.0.0.9".into(),
                dest_port: 80,
            };
        app.port_forwards.list = vec![
            forward(1, crate::core::data::PortForwardKind::Local),
            forward(2, crate::core::data::PortForwardKind::Remote),
            forward(3, crate::core::data::PortForwardKind::Dynamic),
        ];
        app.port_forwards
            .cursor
            .select(crate::core::data::PortForwardId(1));
        app.port_forwards.runtime.insert(
            crate::core::data::PortForwardId(1),
            crate::port_forward::state::PortForwardRuntime {
                status: crate::port_forward::PortForwardStatus::Running,
                logs: vec!["listening on 127.0.0.1:8081".into()],
            },
        );
        app.port_forwards.runtime.insert(
            crate::core::data::PortForwardId(2),
            crate::port_forward::state::PortForwardRuntime {
                status: crate::port_forward::PortForwardStatus::Failed,
                logs: vec!["ssh: connection refused".into()],
            },
        );
        app.port_forwards
            .cursor
            .select(crate::core::data::PortForwardId(2));
        app.port_forwards.logs_open = true;
        render(&app);

        app.port_forwards.form.visible = true;
        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::PortForwardHost);
        render(&app);

        app.port_forwards.form.visible = false;
        app.port_forwards.delete_pending = Some(crate::core::data::PortForwardId(1));
        render(&app);
    }

    #[test]
    fn the_snippets_page_renders_packages_and_editors() {
        let mut app = app();
        app.active_nav = NavItem::Snippets;
        render(&app);

        app.snippets.packages = vec![SnippetPackage {
            id: SnippetPackageId(1),
            name: "ops".into(),
            description: "runbooks".into(),
        }];
        app.snippets.list = vec![
            Snippet {
                id: SnippetId(1),
                description: "deploy".into(),
                script: "git push".into(),
                package_id: Some(SnippetPackageId(1)),
                last_used_at: "1700000000000".into(),
            },
            Snippet {
                id: SnippetId(2),
                description: "loose".into(),
                script: "ls".into(),
                package_id: None,
                last_used_at: String::new(),
            },
        ];
        app.snippets
            .cursor
            .select(crate::snippets::SnippetSelection::Package(
                SnippetPackageId(1),
            ));
        render(&app);
        app.snippets
            .cursor
            .select(crate::snippets::SnippetSelection::Snippet(SnippetId(1)));
        render(&app);

        app.snippets.grid_view = false;
        render(&app);

        app.snippets.form.visible = true;
        app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::SnippetFormPackage);
        render(&app);

        app.snippets.package_form.visible = true;
        render(&app);

        app.snippets.delete_pending = Some(SnippetId(1));
        render(&app);
        app.snippets.package_delete_pending = Some(SnippetPackageId(1));
        render(&app);
        app.snippets.context_menu = Some(crate::snippets::SnippetContextMenu::Package {
            id: SnippetPackageId(1),
            position: iced::Point::ORIGIN,
            cursor: crate::context_menu::initial_cursor(&[
                crate::core::context_menu::ContextMenuAction::SnippetPackageEdit,
                crate::core::context_menu::ContextMenuAction::SnippetPackageDelete,
            ]),
        });
        render(&app);
    }

    #[test]
    fn the_known_hosts_page_renders_every_row_shape() {
        let mut app = app();
        app.active_nav = NavItem::KnownHosts;
        render(&app);

        app.known_hosts.entries = vec![
            KnownHost {
                line_number: 1,
                hostname: "example.com".into(),
                port: 22,
                key_type: "ssh-ed25519".into(),
                public_key: "ssh-ed25519 AAAA".into(),
                fingerprint: "SHA256:abc".into(),
                marker: None,
                raw_pattern: "example.com".into(),
            },
            KnownHost {
                line_number: 2,
                hostname: "(hashed)".into(),
                port: 2222,
                key_type: "ssh-rsa".into(),
                public_key: "ssh-rsa AAAA".into(),
                fingerprint: "SHA256:def".into(),
                marker: Some("@revoked".into()),
                raw_pattern: "|1|abc=|def=".into(),
            },
            KnownHost {
                line_number: 3,
                hostname: "authority".into(),
                port: 22,
                key_type: "ssh-ed25519".into(),
                public_key: "ssh-ed25519 CCCC".into(),
                fingerprint: String::new(),
                marker: Some("@cert-authority".into()),
                raw_pattern: "@cert-authority authority".into(),
            },
        ];
        app.known_hosts.cursor.select(1);
        render(&app);

        app.known_hosts.grid_view = false;
        render(&app);

        app.known_hosts.detail_open = true;
        app.known_hosts.detail_anim = 1.0;
        render(&app);

        app.known_hosts.detail_open = false;
        app.known_hosts.delete_pending = Some(1);
        render(&app);
    }

    #[test]
    fn the_workspaces_card_renders_its_cards_and_dialogs() {
        let mut app = app();
        app.active_nav = NavItem::Settings;
        render(&app);

        app.workspace_manager.list = vec![crate::workspace::SavedWorkspace {
            id: "prod".into(),
            name: "Production".into(),
            updated_at: 1_700_000_000,
            snapshot: crate::workspace::WorkspaceSnapshot {
                version: 1,
                tabs: Vec::new(),
                active_tab: None,
                sessions: Vec::new(),
                splits: Vec::new(),
            },
        }];
        app.workspace_manager.cursor.select("prod".into());
        render(&app);

        app.workspace_manager.hovered_id = Some("prod".into());
        render(&app);

        app.workspace_manager.save_pending = true;
        app.workspace_manager.save_name = "prod".into();
        render(&app);

        app.workspace_manager.save_pending = false;
        app.workspace_manager.save_conflict = Some("prod".into());
        render(&app);

        app.workspace_manager.save_conflict = None;
        app.workspace_manager.delete_id = Some("prod".into());
        render(&app);

        app.workspace_manager.delete_id = None;
        app.workspace_manager.rename_id = Some("prod".into());
        app.workspace_manager.rename_name = "Prod 2".into();
        render(&app);

        app.workspace_manager.rename_id = None;
        app.workspace_manager.import_dialog = Some(crate::workspace::WorkspaceImportDialog {
            pending_workspaces: Vec::new(),
            new_count: 2,
            dup_count: 1,
        });
        render(&app);
    }

    #[test]
    fn the_settings_page_renders_every_theme() {
        let mut app = app();
        app.active_nav = NavItem::Settings;

        for mode in [
            ThemeMode::Dark,
            ThemeMode::Light,
            ThemeMode::HighContrastDark,
            ThemeMode::HighContrastLight,
            ThemeMode::Dracula,
            ThemeMode::Novel,
            ThemeMode::Auto,
        ] {
            app.preferences.theme_mode = mode;
            render(&app);
        }

        for target in [
            crate::core::dropdown::DropdownTarget::SettingsTheme,
            crate::core::dropdown::DropdownTarget::SettingsLocale,
            crate::core::dropdown::DropdownTarget::SettingsDiagnostics,
            crate::core::dropdown::DropdownTarget::SettingsShellIntegration,
            crate::core::dropdown::DropdownTarget::SettingsHistoryLimit,
        ] {
            app.active_dropdown = Some(target);
            render(&app);
        }

        #[cfg(target_os = "windows")]
        {
            app.active_dropdown = Some(crate::core::dropdown::DropdownTarget::SettingsLocalShell);
            render(&app);
        }
    }

    #[test]
    fn the_logs_page_renders_entries_of_every_category() {
        let mut app = app();
        app.active_nav = NavItem::Logs;

        for category in [
            LogCategory::Host,
            LogCategory::Terminal,
            LogCategory::Sftp,
            LogCategory::PortForward,
            LogCategory::Snippet,
            LogCategory::KnownHost,
            LogCategory::Keychain,
            LogCategory::System,
        ] {
            app.add_log(category, "something happened");
        }
        render(&app);

        // A long log scrolls; the render path differs from the empty one.
        for index in 0..80 {
            app.add_log(LogCategory::System, format!("line {index}"));
        }
        render(&app);
    }

    #[test]
    fn the_topbar_renders_with_tabs_split_into_groups() {
        let mut app = app();
        let first = app.allocate_tab_id(TabKind::Session(SessionId(1)));
        let second = app.allocate_tab_id(TabKind::Draft(DraftId(1)));
        let third = app.allocate_tab_id(TabKind::Split(SplitId(1)));
        app.active_tab = ActiveTab::Tab(second);
        render(&app);

        // A tab strip that thinks it can scroll has its own chrome.
        app.top_tabs_can_scroll = true;
        app.top_tabs_scroll_offset = 0.5;
        app.top_tabs_scroll_absolute_x = 120.0;
        app.top_tabs_viewport_width = 400.0;
        render(&app);

        // Locked tabs show a lock badge.
        app.set_tab_locked_by_id(first, true);
        app.set_tab_locked_by_id(third, true);
        render(&app);
    }

    #[test]
    fn the_bottom_right_popups_render() {
        let mut app = app();

        app.toast = Some(Toast {
            id: 1,
            message: "saved".into(),
            kind: ToastKind::Info,
        });
        app.toast_anim = 1.0;
        render(&app);

        for kind in [ToastKind::Warning, ToastKind::Error] {
            app.toast.as_mut().unwrap().kind = kind;
            render(&app);
        }
        app.toast_closing = true;
        render(&app);
        app.toast = None;
        app.toast_closing = false;

        app.command_frequency_prompt = Some(crate::app::CommandFrequencyPrompt {
            command: "git status".into(),
            count: 12,
        });
        app.command_frequency_prompt_anim = 1.0;
        render(&app);
    }

    #[test]
    fn the_app_level_dialogs_render() {
        let mut app = app();

        app.show_quit_confirm = true;
        render(&app);
        app.show_quit_confirm = false;

        app.show_crash_dialog = true;
        app.crash_reports.push((1, "backtrace".into()));
        render(&app);
        app.show_crash_dialog = false;
        app.crash_reports.clear();

        app.show_apply_workspace_confirm = true;
        app.pending_apply_workspace = Some(crate::workspace::PendingWorkspaceApply {
            name: "prod".into(),
            snapshot: crate::workspace::WorkspaceSnapshot {
                version: 1,
                tabs: Vec::new(),
                active_tab: None,
                sessions: Vec::new(),
                splits: Vec::new(),
            },
        });
        render(&app);
        app.show_apply_workspace_confirm = false;
        app.pending_apply_workspace = None;

        app.pending_locked_close = Some(crate::terminal::PendingLockedClose {
            label: "web".into(),
            target: crate::terminal::TerminalCloseTarget::Tab(SessionId(1)),
        });
        render(&app);
        app.pending_locked_close = None;
    }

    #[test]
    fn the_right_sidebar_renders_every_tab() {
        let mut app = app();
        app.snippets.list = vec![Snippet {
            id: SnippetId(1),
            description: "deploy".into(),
            script: "git push".into(),
            package_id: None,
            last_used_at: "1700000000000".into(),
        }];

        for tab in [
            RightSidebarTab::Snippets,
            RightSidebarTab::Commands,
            RightSidebarTab::Theme,
        ] {
            render_sidebar(&app, tab);
        }
    }
}
