// Rust guideline compliant 2026-10-09

use iced::widget::{
    Id, Space, button, column, container, mouse_area, row, rule, stack, text, text_input,
};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};

use crate::app::App;
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{HostId, OsIcon, ServerKeyInfo};
use crate::core::diagnostics::Level;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::{ThemePalette, palette};
use crate::core::widgets::{
    control_padding, cursor_tooltip_label, input_style_with_palette, styled_scrollable,
};
use crate::ui::cursor_area::CursorArea;
use crate::ui::host_key_prompt::{
    TrustActions, action_row as host_key_action_row, panel as host_key_panel,
};
use crate::ui::password_prompt::{self, PasswordPrompt};
use rust_i18n::t;
use secrecy::SecretString;

use crate::sftp::{SftpExternalDropTarget, SftpPane};
use crate::ssh_connect::SshConnectionState;

pub const LOCAL_DROP_ZONE_ID: &str = "sftp_local_drop_zone";
pub const REMOTE_DROP_ZONE_ID: &str = "sftp_remote_drop_zone";

pub const SFTP_LOCAL_FILE_LIST_ID: &str = "sftp_local_file_list";
pub const SFTP_LOCAL_REMOTE_FILE_LIST_ID: &str = "sftp_local_remote_file_list";
pub const SFTP_REMOTE_FILE_LIST_ID: &str = "sftp_remote_file_list";
pub const SFTP_HOST_PICKER_SCROLL_ID: &str = "sftp_host_picker_scroll";
pub const SFTP_HOST_PICKER_INPUT_ID: &str = "sftp_host_picker_input";
pub const SFTP_PREVIEW_SCROLL_ID: &str = "sftp_preview_scroll";
pub const SFTP_LOCAL_PASSWORD_INPUT_ID: &str = "sftp_local_password_input";
pub const SFTP_REMOTE_PASSWORD_INPUT_ID: &str = "sftp_remote_password_input";

/// Fixed host-row height used to keep keyboard selection inside the viewport.
pub(crate) const SFTP_HOST_PICKER_ROW_HEIGHT: f32 = 60.0;
/// Spacing between adjacent host rows.
pub(crate) const SFTP_HOST_PICKER_ROW_GAP: f32 = 4.0;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

/// Width budget per segment used only by the collapsed-breadcrumb fallback
/// (icon + separator + text) to decide how many tail segments to keep.
#[derive(Debug, Clone, Copy)]
struct ExternalDropFeedback {
    enabled: bool,
    icon: lucide_icons::Icon,
    title: &'static str,
    subtitle: &'static str,
}

#[derive(Debug, Clone, Copy)]
struct DropOverlayColors {
    background: Color,
    icon: Color,
    title: Color,
    subtitle: Color,
    border: Color,
}

fn external_drop_feedback(
    app: &App,
    target: SftpExternalDropTarget,
) -> Option<ExternalDropFeedback> {
    if app.sftp.state.external_drag_paths.is_empty() {
        return None;
    }

    if app.sftp.state.external_drop_target != Some(target) {
        return None;
    }

    Some(match target {
        SftpExternalDropTarget::Local if app.sftp.state.left.pane.connected => {
            ExternalDropFeedback {
                enabled: true,
                icon: lucide_icons::Icon::CloudUpload,
                title: t_static(t!("sftp.drop_upload_left")),
                subtitle: t_static(t!("sftp.drop_upload_left_sub")),
            }
        }
        SftpExternalDropTarget::Local => ExternalDropFeedback {
            enabled: true,
            icon: lucide_icons::Icon::HardDriveDownload,
            title: t_static(t!("sftp.drop_local")),
            subtitle: t_static(t!("sftp.drop_local_sub")),
        },
        SftpExternalDropTarget::Remote if app.sftp.state.right.pane.connected => {
            ExternalDropFeedback {
                enabled: true,
                icon: lucide_icons::Icon::CloudUpload,
                title: t_static(t!("sftp.drop_upload")),
                subtitle: t_static(t!("sftp.drop_upload_sub")),
            }
        }
        SftpExternalDropTarget::Remote => ExternalDropFeedback {
            enabled: false,
            icon: lucide_icons::Icon::ServerOff,
            title: t_static(t!("sftp.drop_no_remote")),
            subtitle: t_static(t!("sftp.drop_no_remote_sub")),
        },
    })
}

/// Pick overlay colours from the active theme.
///
/// The overlay covers the panel with `content_bg`, so its text must contrast
/// with that surface rather than with an assumed dark background. A fixed
/// white worked only in dark themes; in light themes (including Windows' Auto
/// mode) it made the whole drop target look blank.
fn external_drop_overlay_colors(enabled: bool, p: ThemePalette) -> DropOverlayColors {
    let background = Color {
        a: 0.95,
        ..p.content_bg
    };

    if enabled {
        DropOverlayColors {
            background,
            icon: p.accent,
            title: p.text_pri,
            subtitle: p.text_sec,
            border: p.accent,
        }
    } else {
        DropOverlayColors {
            background,
            icon: p.danger,
            title: p.text_pri,
            subtitle: p.text_sec,
            border: p.danger,
        }
    }
}

pub fn view_sftp(app: &App) -> Element<'_, Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "VIEW",
        "view_sftp",
        std::time::Duration::from_millis(16),
    );
    let p = palette(app.effective_theme_mode());
    let main: Element<'_, Message> = if let Some(preview) = app.sftp.state.preview.as_ref() {
        view_preview_fullscreen(preview, p, app.is_scrollbar_visible(SFTP_PREVIEW_SCROLL_ID))
    } else {
        let local = view_local_panel(app);
        let remote = view_remote_panel(app);

        // Cancel opposite panel's edit when clicking on the other panel.
        // Always wrap in mouse_area so the widget tree structure never changes
        // between frames — this prevents iced from discarding the scrollable
        // state of the file list underneath.
        let local: Element<'_, Message> = {
            let mut area = mouse_area(local);
            if app.sftp.state.right.pane.path_edit.is_some() {
                area = area.on_press(Message::Sftp(
                    crate::sftp::message::Message::SftpPathEditCancel(SftpPane::Remote),
                ));
            }
            area.into()
        };
        let remote: Element<'_, Message> = {
            let mut area = mouse_area(remote);
            if app.sftp.state.left.pane.path_edit.is_some() {
                area = area.on_press(Message::Sftp(
                    crate::sftp::message::Message::SftpPathEditCancel(SftpPane::Local),
                ));
            }
            area.into()
        };

        let divider = container(rule::vertical(0))
            .width(Length::Fixed(1.0))
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..Default::default()
            });

        row![local, divider, remote].height(Length::Fill).into()
    };

    let mut layers = stack![main];

    // Always include transfer container layer so the widget tree structure
    // never changes between frames — this prevents iced from discarding
    // the scrollable state of the file list underneath.
    let mut bottom_content = column![].spacing(0).width(Length::Fill);

    if let Some(progress) = app.sftp.state.transfer_progress.as_ref() {
        bottom_content =
            bottom_content.push(view_transfer_progress(progress, app.sftp.state.anim_t, p));
    }

    let transfer_results = &app.sftp.state.transfer_results;
    for (index, failure) in transfer_results.failures().iter().enumerate() {
        bottom_content = bottom_content.push(view_transfer_result(
            failure,
            Message::Sftp(crate::sftp::message::Message::SftpDismissTransferFailure(
                index,
            )),
            p,
        ));
    }

    if app.sftp.state.transfer_progress.is_none()
        && let Some(result) = transfer_results.latest()
    {
        bottom_content = bottom_content.push(view_transfer_result(
            result,
            Message::Sftp(crate::sftp::message::Message::SftpDismissTransferResult),
            p,
        ));
    }

    if !app.sftp.state.transfer_queue.is_empty() {
        bottom_content =
            bottom_content.push(view_queued_transfers(&app.sftp.state.transfer_queue, p));
    }

    // Treat the visible transfer strip as its own interaction layer. Without
    // this, file rows behind it still receive hover changes because the outer
    // bottom-aligned container spans the whole SFTP body.
    let transfer_strip = mouse_area(bottom_content)
        .on_press(Message::Noop)
        .interaction(mouse::Interaction::Idle);

    layers = layers.push(
        container(transfer_strip)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Bottom),
    );

    // Always include a context menu layer so the widget tree structure
    // never changes between frames — this prevents iced from discarding
    // the scrollable state of the file list underneath.
    layers = layers.push(if let Some(menu) = app.sftp.state.context_menu.as_ref() {
        view_context_menu(menu, p)
    } else {
        mouse_area(Space::new().width(Length::Shrink).height(Length::Shrink)).into()
    });

    let sftp_body: Element<'_, Message> = mouse_area(layers)
        .on_move(|v0| Message::Sftp(crate::sftp::message::Message::SftpViewCursorMoved(v0)))
        .into();

    let sidebar_divider = container(rule::vertical(0))
        .width(Length::Fixed(1.0))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..container::Style::default()
        });

    // Top border spanning the full width (sidebar + divider + body).
    let top_border = container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(
        move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..container::Style::default()
        },
    );

    let body_row = row![
        crate::sftp::sidebar_view::view_sftp_sidebar(app),
        sidebar_divider,
        sftp_body,
    ]
    .height(Length::Fill);

    let sftp_page = column![top_border, body_row].height(Length::Fill);

    // Sidebar tab context menu — positioned at the right edge of the
    // sidebar, vertically aligned with the right-clicked tab.
    let sidebar_menu_layer = if let Some(menu) = app.sftp.sidebar_context_menu.as_ref() {
        let sidebar_width = crate::sftp::sidebar_view::SIDEBAR_COLLAPSED_WIDTH
            + (crate::sftp::sidebar_view::SIDEBAR_EXPANDED_WIDTH
                - crate::sftp::sidebar_view::SIDEBAR_COLLAPSED_WIDTH)
                * crate::ui::helpers::ease_out_cubic(app.sftp.sidebar_anim);
        let scroll_offset = app
            .scroll_offsets
            .get(crate::sftp::sidebar_view::SFTP_SIDEBAR_SCROLL_ID)
            .copied()
            .unwrap_or(0.0);
        let x = sidebar_width + 4.0;
        let y = 1.0 // top border
            + 12.0 // sidebar top padding
            + menu.tab_index as f32 * (crate::views::ITEM_HEIGHT + 2.0)
            - scroll_offset;

        crate::sftp::sidebar_view::view_sidebar_context_menu_at(menu, p, x, y)
    } else {
        mouse_area(Space::new().width(Length::Shrink).height(Length::Shrink)).into()
    };

    stack![sftp_page, sidebar_menu_layer]
        .height(Length::Fill)
        .into()
}

fn host_header_button(
    icon: OsIcon,
    icon_bg: Color,
    label: String,
    fill: bool,
    on_press: Message,
    p: ThemePalette,
) -> Element<'static, Message> {
    let text_on_color = p.text_on_color;
    let button_hover = p.button_hover;
    let text_pri = p.text_pri;

    let mut label_text = text(label).size(15).color(text_pri);
    if fill {
        label_text = label_text
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None);
    }

    let mut btn = button(
        row![
            container(crate::views::view_os_icon(icon, 11.0, text_on_color))
                .width(20)
                .height(20)
                .center(20)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(icon_bg)),
                    border: Border {
                        radius: 5.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
            label_text,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .on_press(on_press)
    .padding(6)
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: hovered.then_some(Background::Color(button_hover)),
            text_color: text_pri,
            border: Border {
                radius: 8.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    if fill {
        btn = btn.width(Length::Fill);
    }

    btn.into()
}

fn open_terminal_button(p: ThemePalette, pane: SftpPane) -> Element<'static, Message> {
    icon_btn(
        lucide_icons::Icon::Terminal,
        p.text_sec,
        Message::Sftp(crate::sftp::message::Message::SftpOpenTerminal { pane }),
        p,
        t_static(t!("sftp.tooltip_open_terminal")),
    )
}

fn view_sftp_host_key_confirm_dialog(
    key_info: &ServerKeyInfo,
    p: ThemePalette,
) -> Element<'_, Message> {
    host_key_panel(key_info, None, p)
}

fn view_sftp_key_confirm_actions(pane: SftpPane, p: ThemePalette) -> Element<'static, Message> {
    let disconnect_msg = if pane == SftpPane::Local {
        Message::Sftp(crate::sftp::message::Message::SftpDisconnectLocalRemote)
    } else {
        Message::Sftp(crate::sftp::message::Message::SftpDisconnect)
    };

    host_key_action_row(
        TrustActions::new(
            t_static(t!("ssh.close")),
            disconnect_msg,
            Message::Sftp(crate::sftp::message::Message::SftpKeyConfirmContinue { pane }),
            Message::Sftp(crate::sftp::message::Message::SftpKeyConfirmAccept { pane }),
        ),
        p,
    )
}

fn sftp_lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

fn view_sftp_connection_indicator(
    state: SshConnectionState,
    anim_t: f32,
    p: ThemePalette,
) -> Element<'static, Message> {
    if state == SshConnectionState::WaitingForKeyConfirmation {
        let icon = |icon: lucide_icons::Icon, color: Color| -> Element<'static, Message> {
            container(
                text(char::from(icon))
                    .font(LUCIDE_FONT)
                    .size(24)
                    .color(color),
            )
            .width(Length::Fixed(36.0))
            .height(Length::Fixed(36.0))
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into()
        };

        let bar = |total_w: f32, color: Color| -> Element<'static, Message> {
            container(text(""))
                .width(Length::Fixed(total_w))
                .height(Length::Fixed(4.0))
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: Border {
                        radius: 2.0.into(),
                        color: Color::TRANSPARENT,
                        width: 0.0,
                    },
                    ..Default::default()
                })
                .into()
        };

        return row![
            icon(lucide_icons::Icon::Monitor, p.accent),
            bar(116.0, p.accent),
            icon(lucide_icons::Icon::Fingerprint, p.accent),
            bar(116.0, sftp_lerp_color(p.surface, p.text_dim, 0.35)),
            icon(lucide_icons::Icon::Server, p.text_dim),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center)
        .into();
    }

    let pulse = (anim_t * std::f32::consts::TAU).sin() * 0.25 + 0.75;

    let (start_icon, start_color) = match state {
        SshConnectionState::Idle => (lucide_icons::Icon::Monitor, p.text_dim),
        SshConnectionState::WaitingForPassword => (lucide_icons::Icon::Monitor, p.accent),
        SshConnectionState::Connecting => (
            lucide_icons::Icon::Monitor,
            sftp_lerp_color(
                Color {
                    r: p.accent.r * 0.45,
                    g: p.accent.g * 0.45,
                    b: p.accent.b * 0.45,
                    a: 1.0,
                },
                p.accent,
                pulse,
            ),
        ),
        SshConnectionState::Connected => (lucide_icons::Icon::Monitor, p.success),
        SshConnectionState::Failed => (lucide_icons::Icon::Monitor, p.text_dim),
        SshConnectionState::WaitingForKeyConfirmation => unreachable!(),
    };

    let (end_icon, end_color) = match state {
        SshConnectionState::Idle => (lucide_icons::Icon::Server, p.text_dim),
        SshConnectionState::WaitingForPassword => (lucide_icons::Icon::LockKeyhole, p.accent),
        SshConnectionState::Connecting => (
            lucide_icons::Icon::Server,
            sftp_lerp_color(
                Color {
                    r: p.accent.r * 0.3,
                    g: p.accent.g * 0.3,
                    b: p.accent.b * 0.3,
                    a: 1.0,
                },
                Color {
                    r: p.accent.r * 0.7,
                    g: p.accent.g * 0.7,
                    b: p.accent.b * 0.7,
                    a: 1.0,
                },
                1.0 - pulse,
            ),
        ),
        SshConnectionState::Connected => (lucide_icons::Icon::Server, p.success),
        SshConnectionState::Failed => (lucide_icons::Icon::Server, p.text_dim),
        SshConnectionState::WaitingForKeyConfirmation => unreachable!(),
    };

    let start = container(
        text(char::from(start_icon))
            .font(LUCIDE_FONT)
            .size(24)
            .color(start_color),
    )
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(36.0))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center);

    let progress_bar = crate::ssh_connect::view::view_progress_bar(state, anim_t, p, 284.0);

    let end = container(
        text(char::from(end_icon))
            .font(LUCIDE_FONT)
            .size(24)
            .color(end_color),
    )
    .width(Length::Fixed(36.0))
    .height(Length::Fixed(36.0))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center);

    row![start, progress_bar, end]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into()
}

fn view_sftp_status_text(state: SshConnectionState, p: ThemePalette) -> Element<'static, Message> {
    let (label, color): (String, Color) = match state {
        SshConnectionState::Idle => (t_static(t!("ssh.waiting_connect")).into(), p.text_dim),
        SshConnectionState::WaitingForPassword => {
            (t_static(t!("ssh.password_required")).into(), p.accent)
        }
        SshConnectionState::Connecting => (t_static(t!("ssh.connecting")).into(), p.accent),
        SshConnectionState::Connected => {
            (t_static(t!("ssh.connection_established")).into(), p.success)
        }
        SshConnectionState::Failed => (t_static(t!("ssh.connection_failed")).into(), p.danger),
        SshConnectionState::WaitingForKeyConfirmation => {
            (t_static(t!("ssh.host_key_required")).into(), p.accent)
        }
    };

    container(
        text(label)
            .size(18)
            .color(color)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fixed(380.0))
    .into()
}

fn view_sftp_failed_actions(
    pane: SftpPane,
    host_id: HostId,
    p: ThemePalette,
) -> Element<'static, Message> {
    let retry_msg = if pane == SftpPane::Local {
        Message::Sftp(crate::sftp::message::Message::SftpConnectLocalHost(host_id))
    } else {
        Message::Sftp(crate::sftp::message::Message::SftpConnectHost(host_id))
    };
    let disconnect_msg = if pane == SftpPane::Local {
        Message::Sftp(crate::sftp::message::Message::SftpDisconnectLocalRemote)
    } else {
        Message::Sftp(crate::sftp::message::Message::SftpDisconnect)
    };

    let retry_btn = button(
        row![
            text(char::from(lucide_icons::Icon::RefreshCw))
                .font(LUCIDE_FONT)
                .size(14)
                .color(p.text_on_color),
            text(t_static(t!("sftp.retry")))
                .size(14)
                .color(p.text_on_color),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .on_press(retry_msg)
    .padding(control_padding(14.0, 18.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                p.accent_hover
            } else {
                p.accent
            })),
            text_color: p.text_on_color,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let disconnect_btn = button(
        row![
            text(char::from(lucide_icons::Icon::Unplug))
                .font(LUCIDE_FONT)
                .size(14)
                .color(p.danger),
            text(t_static(t!("sftp.disconnect")))
                .size(14)
                .color(p.danger),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .on_press(disconnect_msg)
    .padding(control_padding(14.0, 18.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                p.surface
            } else {
                p.content_bg
            })),
            text_color: p.danger,
            border: Border {
                color: if hovered { p.danger } else { p.border },
                width: 1.0,
                radius: 6.0.into(),
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    row![retry_btn, disconnect_btn]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into()
}

fn view_sftp_connection_state(
    app: &App,
    pane: SftpPane,
    connect_state: SshConnectionState,
    host_icon: OsIcon,
    icon_bg: Color,
    host_name: String,
    p: ThemePalette,
) -> Element<'_, Message> {
    let host_name = if pane == SftpPane::Local {
        if app.sftp.state.left.pane.attempt.jump_stage() {
            app.sftp
                .state
                .left
                .pane
                .attempt
                .jump_host
                .as_ref()
                .map_or(host_name, |host| host.name.clone())
        } else {
            host_name
        }
    } else if app.sftp.state.right.pane.attempt.jump_stage() {
        app.sftp
            .state
            .right
            .pane
            .attempt
            .jump_host
            .as_ref()
            .map(|host| host.name.clone())
            .unwrap_or(host_name)
    } else {
        host_name
    };

    let minimal_header_left = button(
        row![
            container(crate::views::view_os_icon(host_icon, 15.0, p.text_on_color))
                .width(28)
                .height(28)
                .center(28)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(icon_bg)),
                    border: Border {
                        radius: 5.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
            text(host_name)
                .size(20)
                .color(p.text_pri)
                .ellipsis(text::Ellipsis::End)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding(6)
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: hovered.then_some(Background::Color(p.button_hover)),
            text_color: p.text_pri,
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let indicator = view_sftp_connection_indicator(connect_state, app.sftp.state.anim_t, p);
    let status_text = view_sftp_status_text(connect_state, p);
    let indicator_section = container(
        column![indicator, status_text]
            .spacing(16)
            .align_x(iced::Alignment::Center),
    );

    let mut connect_content = column![]
        .spacing(8)
        .align_x(iced::Alignment::Center)
        .push(minimal_header_left)
        .push(indicator_section);

    if connect_state == SshConnectionState::WaitingForPassword {
        let (password_input, password_visible) = if pane == SftpPane::Local {
            (
                &app.sftp.state.left.pane.attempt.password_input,
                app.sftp.state.left.pane.attempt.password_visible,
            )
        } else {
            (
                &app.sftp.state.right.pane.attempt.password_input,
                app.sftp.state.right.pane.attempt.password_visible,
            )
        };
        let prompt = PasswordPrompt {
            label: t_static(t!("ssh.password")),
            placeholder: t_static(t!("ssh.enter_password")),
            value: password_input,
            visible: password_visible,
            input_id: if pane == SftpPane::Local {
                SFTP_LOCAL_PASSWORD_INPUT_ID
            } else {
                SFTP_REMOTE_PASSWORD_INPUT_ID
            },
            submit_label: t_static(t!("ssh.connect")),
            submit: Message::Sftp(crate::sftp::message::Message::SftpPasswordSubmit { pane }),
            toggle: Message::Sftp(crate::sftp::message::Message::SftpTogglePasswordVisible {
                pane,
            }),
            cancel: Message::Sftp(if pane == SftpPane::Local {
                crate::sftp::message::Message::SftpDisconnectLocalRemote
            } else {
                crate::sftp::message::Message::SftpDisconnect
            }),
        };
        let password_panel =
            password_prompt::panel(&prompt, Length::Fixed(380.0), &p, move |value| {
                Message::Sftp(crate::sftp::message::Message::SftpPasswordChanged {
                    pane,
                    password: SecretString::from(value),
                })
            });
        let actions = password_prompt::actions(&prompt, &p);
        let password_content = container(
            column![password_panel, actions]
                .spacing(24)
                .align_x(iced::Alignment::Center),
        )
        .width(Length::Fixed(380.0));
        connect_content = connect_content.push(password_content);
    } else if connect_state == SshConnectionState::WaitingForKeyConfirmation {
        let key_info = if pane == SftpPane::Local {
            app.sftp.state.left.pane.attempt.pending_key_info.as_ref()
        } else {
            app.sftp.state.right.pane.attempt.pending_key_info.as_ref()
        };
        if let Some(key_info) = key_info {
            let confirm = view_sftp_host_key_confirm_dialog(key_info, p);
            let actions = view_sftp_key_confirm_actions(pane, p);
            let key_content = container(
                column![confirm, actions]
                    .spacing(24)
                    .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill);
            connect_content = connect_content.push(key_content);
        } else {
            let error = text(t_static(t!("ssh.key_info_unavailable")))
                .size(13)
                .color(p.danger);
            connect_content = connect_content.push(error);
        }
    } else if connect_state == SshConnectionState::Failed {
        let error_text = if pane == SftpPane::Local {
            app.sftp.state.left.pane.error.clone()
        } else {
            app.sftp.state.right.pane.error.clone()
        }
        .unwrap_or_else(|| t!("sftp.error.connection_failed").to_string());
        let error = text(error_text)
            .size(13)
            .color(p.danger)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .wrapping(text::Wrapping::WordOrGlyph);
        let host_id = if pane == SftpPane::Local {
            app.sftp.state.left.pane.host_id.unwrap_or(HostId(0))
        } else {
            app.sftp.state.right.pane.host_id.unwrap_or(HostId(0))
        };
        let actions = view_sftp_failed_actions(pane, host_id, p);
        connect_content = connect_content.push(
            column![error, actions]
                .spacing(12)
                .align_x(iced::Alignment::Center),
        );
    }

    let connect_content = container(connect_content).width(Length::Fixed(380.0));
    container(connect_content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .into()
}

fn view_local_panel(app: &App) -> Element<'_, Message> {
    let p = palette(app.effective_theme_mode());
    if app.sftp.state.left.host_picker.open {
        return view_host_picker(app, SftpPane::Local);
    }
    if app.sftp.state.left.pane.connected {
        return view_left_remote_connected(app);
    }

    let mut header = row![host_header_button(
        OsIcon::Lucide(lucide_icons::Icon::HardDrive),
        p.accent,
        t!("sftp.location_local").to_string(),
        false,
        Message::Sftp(crate::sftp::message::Message::SftpShowLocalHostPicker),
        p,
    ),]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    if app.sftp.state.left.pane.filter_open || app.sftp.state.left.pane.filter_anim > 0.01 {
        let w = (220.0 * app.sftp.state.left.pane.filter_anim).max(1.0);
        header = header.push(Space::new().width(Length::Fill));
        header = header.push(
            container(
                text_input(
                    t_static(t!("placeholder.filter_files")),
                    &app.sftp.state.left.pane.filter,
                )
                .on_input(|v0| Message::Sftp(crate::sftp::message::Message::SftpLocalFilter(v0)))
                .id(iced::widget::Id::from("sftp_local_filter"))
                .padding(Padding::from([3, 8]))
                .size(13)
                .width(Length::Fill)
                .style(move |_t, s| input_style_with_palette(&p, s)),
            )
            .width(Length::Fixed(w))
            .clip(true),
        );
    } else {
        header = header.push(Space::new().width(Length::Fill));
    }
    let header = header
        .push(icon_btn(
            lucide_icons::Icon::Filter,
            if app.sftp.state.left.pane.filter_open || !app.sftp.state.left.pane.filter.is_empty() {
                p.accent
            } else {
                p.text_sec
            },
            Message::Sftp(crate::sftp::message::Message::SftpToggleFilter(
                SftpPane::Local,
            )),
            p,
            t_static(t!("sftp.tooltip_filter")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::Eye,
            if app.sftp.state.left.pane.show_hidden {
                p.accent
            } else {
                p.text_sec
            },
            Message::Sftp(crate::sftp::message::Message::SftpToggleHidden(
                SftpPane::Local,
            )),
            p,
            t_static(t!("sftp.tooltip_show_hidden")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::RefreshCw,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpRefreshLocal),
            p,
            t_static(t!("sftp.tooltip_refresh")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::FolderPlus,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpNewFolderRequest {
                pane: SftpPane::Local,
                directory: app.sftp.state.left.pane.path.clone(),
            }),
            p,
            t_static(t!("sftp.tooltip_new_folder")),
        ));

    let can_back = app.sftp.state.left.pane.history_idx > 0;
    let can_forward = app.sftp.state.left.pane.history_idx
        < app.sftp.state.left.pane.history.len().saturating_sub(1);
    let breadcrumb = view_path_bar(
        &app.sftp.state.left.pane.path,
        SftpPane::Local,
        app.sftp.state.left.pane.path_edit.as_deref(),
        can_back,
        can_forward,
        p,
    );

    let file_table = view_file_table(
        &app.sftp.state.left.pane.entries,
        &app.sftp.state.left.pane.filter,
        SftpPane::Local,
        &app.sftp.state.left.pane.path,
        app.sftp.state.left.pane.selected.as_slice(),
        app.sftp.state.left.pane.hovered.as_deref(),
        app.sftp.state.left.pane.sort_column,
        app.sftp.state.left.pane.sort_direction,
        SFTP_LOCAL_FILE_LIST_ID,
        p,
        app.is_scrollbar_visible(SFTP_LOCAL_FILE_LIST_ID),
    );

    drop_zone_panel(
        LOCAL_DROP_ZONE_ID,
        column![
            container(header).padding(Padding {
                left: 12.0,
                right: 12.0,
                top: 0.0,
                bottom: 0.0
            }),
            column![
                container(breadcrumb).padding(Padding {
                    left: 12.0,
                    right: 12.0,
                    top: 0.0,
                    bottom: 0.0
                }),
                file_table
            ]
            .spacing(0)
        ]
        .spacing(8)
        .height(Length::Fill),
        p,
        external_drop_feedback(app, SftpExternalDropTarget::Local),
    )
}

fn view_left_remote_connected(app: &App) -> Element<'_, Message> {
    let p = palette(app.effective_theme_mode());
    let host = app
        .sftp
        .state
        .left
        .pane
        .host_id
        .and_then(|id| app.hosts.list.iter().find(|h| h.id == id));

    let host_name = host.map_or_else(
        || t!("sftp.location_source_remote").to_string(),
        |h| h.name.clone(),
    );

    let host_icon = host.map_or(OsIcon::Lucide(lucide_icons::Icon::Monitor), |h| {
        h.os_type.icon()
    });

    let icon_bg = host.map_or(p.accent, |h| h.os_type.icon_bg());

    let header_left = host_header_button(
        host_icon,
        icon_bg,
        host_name.clone(),
        true,
        Message::Sftp(crate::sftp::message::Message::SftpShowLocalHostPicker),
        p,
    );

    let mut header = row![header_left]
        .spacing(6)
        .align_y(iced::Alignment::Center);

    if app.sftp.state.left.pane.filter_open || app.sftp.state.left.pane.filter_anim > 0.01 {
        let w = (220.0 * app.sftp.state.left.pane.filter_anim).max(1.0);
        header = header.push(
            container(
                text_input(
                    t_static(t!("placeholder.filter_files")),
                    &app.sftp.state.left.pane.filter,
                )
                .on_input(|v0| Message::Sftp(crate::sftp::message::Message::SftpLocalFilter(v0)))
                .id(iced::widget::Id::from("sftp_local_filter"))
                .padding(Padding::from([3, 8]))
                .size(13)
                .width(Length::Fill)
                .style(move |_t, s| input_style_with_palette(&p, s)),
            )
            .width(Length::Fixed(w))
            .clip(true),
        );
    }

    let header = header
        .push(icon_btn(
            lucide_icons::Icon::Filter,
            if app.sftp.state.left.pane.filter_open || !app.sftp.state.left.pane.filter.is_empty() {
                p.accent
            } else {
                p.text_sec
            },
            Message::Sftp(crate::sftp::message::Message::SftpToggleFilter(
                SftpPane::Local,
            )),
            p,
            t_static(t!("sftp.tooltip_filter")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::Eye,
            if app.sftp.state.left.pane.show_hidden {
                p.accent
            } else {
                p.text_sec
            },
            Message::Sftp(crate::sftp::message::Message::SftpToggleHidden(
                SftpPane::Local,
            )),
            p,
            t_static(t!("sftp.tooltip_show_hidden")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::RefreshCw,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpRefreshLocal),
            p,
            t_static(t!("sftp.tooltip_refresh")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::FolderPlus,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpNewFolderRequest {
                pane: SftpPane::Local,
                directory: app.sftp.state.left.pane.path.clone(),
            }),
            p,
            t_static(t!("sftp.tooltip_new_folder")),
        ))
        .push(open_terminal_button(p, SftpPane::Local))
        .push({
            let btn = button(
                text(char::from(lucide_icons::Icon::Unplug))
                    .font(LUCIDE_FONT)
                    .size(14)
                    .color(p.danger),
            )
            .on_press(Message::Sftp(
                crate::sftp::message::Message::SftpDisconnectLocalRemote,
            ))
            .padding(6)
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: hovered.then_some(Background::Color(p.surface)),
                    border: Border {
                        radius: 4.0.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                }
            })
            .cursor_interaction(mouse::Interaction::default());
            cursor_tooltip_label(btn, t_static(t!("sftp.tooltip_disconnect")), p, None)
        });

    let can_back = app.sftp.state.left.pane.history_idx > 0 && !app.sftp.state.left.pane.loading;
    let can_forward = app.sftp.state.left.pane.history_idx
        < app.sftp.state.left.pane.history.len().saturating_sub(1)
        && !app.sftp.state.left.pane.loading;
    let breadcrumb = view_path_bar(
        &app.sftp.state.left.pane.path,
        SftpPane::Local,
        app.sftp.state.left.pane.path_edit.as_deref(),
        can_back,
        can_forward,
        p,
    );

    // Connection indicator during the initial connect.
    let connect_state = app.sftp.state.left.pane.attempt.state;
    if !matches!(
        connect_state,
        SshConnectionState::Idle | SshConnectionState::Connected
    ) && (app.sftp.state.left.pane.loading
        || connect_state == SshConnectionState::Failed
        || connect_state == SshConnectionState::WaitingForPassword
        || connect_state == SshConnectionState::WaitingForKeyConfirmation)
    {
        let connect_body = view_sftp_connection_state(
            app,
            SftpPane::Local,
            connect_state,
            host_icon,
            icon_bg,
            host_name.clone(),
            p,
        );
        return drop_zone_panel(
            LOCAL_DROP_ZONE_ID,
            connect_body,
            p,
            external_drop_feedback(app, SftpExternalDropTarget::Local),
        );
    }

    let status: Option<Element<'_, Message>> = if app.sftp.state.left.pane.loading {
        None
    } else {
        app.sftp.state.left.pane.error.clone().map(|status| {
            container(
                text(status)
                    .size(13)
                    .color(p.danger)
                    .ellipsis(text::Ellipsis::End)
                    .wrapping(text::Wrapping::None),
            )
            .padding(Padding::from([0.0, 0.0]))
            .width(Length::Fill)
            .into()
        })
    };

    let file_table = view_file_table(
        &app.sftp.state.left.pane.entries,
        &app.sftp.state.left.pane.filter,
        SftpPane::Local,
        &app.sftp.state.left.pane.path,
        app.sftp.state.left.pane.selected.as_slice(),
        app.sftp.state.left.pane.hovered.as_deref(),
        app.sftp.state.left.pane.sort_column,
        app.sftp.state.left.pane.sort_direction,
        SFTP_LOCAL_REMOTE_FILE_LIST_ID,
        p,
        app.is_scrollbar_visible(SFTP_LOCAL_REMOTE_FILE_LIST_ID),
    );

    let file_area: Element<'_, Message> = if app.sftp.state.left.pane.loading {
        iced::widget::stack![file_table, view_loading_overlay(p)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        file_table
    };

    let mut file_section = column![container(breadcrumb).padding(Padding {
        left: 12.0,
        right: 12.0,
        top: 0.0,
        bottom: 0.0
    })]
    .spacing(0);
    if let Some(status) = status {
        file_section = file_section.push(container(status).padding(Padding {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0,
        }));
    }
    file_section = file_section.push(file_area);

    let body = column![
        container(header).padding(Padding {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0
        }),
        file_section
    ]
    .spacing(8)
    .height(Length::Fill);

    drop_zone_panel(
        LOCAL_DROP_ZONE_ID,
        body,
        p,
        external_drop_feedback(app, SftpExternalDropTarget::Local),
    )
}

fn view_remote_panel(app: &App) -> Element<'_, Message> {
    if app.sftp.state.right.host_picker.open {
        return view_host_picker(app, SftpPane::Remote);
    }
    if !app.sftp.state.right.pane.connected {
        return view_remote_disconnected(app);
    }
    view_remote_connected(app)
}

fn view_remote_disconnected(app: &App) -> Element<'_, Message> {
    let p = palette(app.effective_theme_mode());
    let icon = text(char::from(lucide_icons::Icon::Folder))
        .font(LUCIDE_FONT)
        .size(50)
        .color(p.text_dim);

    let title = text(t_static(t!("sftp.connect_to_host")))
        .size(18)
        .color(p.text_pri);

    let subtitle = text(t_static(t!("sftp.connect_to_host_hint")))
        .size(14)
        .color(p.text_sec);

    let connect_btn = {
        let accent = p.accent;
        button(
            row![
                text(t_static(t!("sftp.select_host_btn")))
                    .size(12)
                    .color(Color::WHITE),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Sftp(
            crate::sftp::message::Message::SftpShowHostPicker,
        ))
        .padding(control_padding(12.0, 14.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    Color {
                        r: (accent.r + 0.04).min(1.0),
                        g: (accent.g + 0.04).min(1.0),
                        b: (accent.b + 0.04).min(1.0),
                        a: accent.a,
                    }
                } else {
                    accent
                })),
                text_color: Color::WHITE,
                border: Border {
                    radius: 5.0.into(),
                    ..Default::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
    };

    let card = container(
        column![icon, title, subtitle, connect_btn]
            .spacing(12)
            .align_x(iced::Alignment::Center),
    )
    .padding(40)
    .style(move |_: &Theme| container::Style {
        border: Border {
            radius: 12.0.into(),
            ..Border::default()
        },
        ..Default::default()
    });

    let content = container(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill);

    drop_zone_panel(
        REMOTE_DROP_ZONE_ID,
        content,
        p,
        external_drop_feedback(app, SftpExternalDropTarget::Remote),
    )
}

fn view_remote_connected(app: &App) -> Element<'_, Message> {
    let p = palette(app.effective_theme_mode());
    let host = app
        .sftp
        .state
        .right
        .pane
        .host_id
        .and_then(|id| app.hosts.list.iter().find(|h| h.id == id));

    let host_name = host.map_or_else(
        || t!("sftp.location_remote").to_string(),
        |h| h.name.clone(),
    );

    let host_icon = host.map_or(OsIcon::Lucide(lucide_icons::Icon::Monitor), |h| {
        h.os_type.icon()
    });

    let icon_bg = host.map_or(p.accent, |h| h.os_type.icon_bg());

    let header_left = host_header_button(
        host_icon,
        icon_bg,
        host_name.clone(),
        true,
        Message::Sftp(crate::sftp::message::Message::SftpShowHostPicker),
        p,
    );

    let mut header = row![header_left]
        .spacing(6)
        .align_y(iced::Alignment::Center);

    if app.sftp.state.right.pane.filter_open || app.sftp.state.right.pane.filter_anim > 0.01 {
        let w = (220.0 * app.sftp.state.right.pane.filter_anim).max(1.0);
        header = header.push(
            container(
                text_input(
                    t_static(t!("placeholder.filter_files")),
                    &app.sftp.state.right.pane.filter,
                )
                .on_input(|v0| Message::Sftp(crate::sftp::message::Message::SftpRemoteFilter(v0)))
                .id(iced::widget::Id::from("sftp_remote_filter"))
                .padding(Padding::from([3, 8]))
                .size(13)
                .width(Length::Fill)
                .style(move |_t, s| input_style_with_palette(&p, s)),
            )
            .width(Length::Fixed(w))
            .clip(true),
        );
    }

    let header = header
        .push(icon_btn(
            lucide_icons::Icon::Filter,
            if app.sftp.state.right.pane.filter_open || !app.sftp.state.right.pane.filter.is_empty()
            {
                p.accent
            } else {
                p.text_sec
            },
            Message::Sftp(crate::sftp::message::Message::SftpToggleFilter(
                SftpPane::Remote,
            )),
            p,
            t_static(t!("sftp.tooltip_filter")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::Eye,
            if app.sftp.state.right.pane.show_hidden {
                p.accent
            } else {
                p.text_sec
            },
            Message::Sftp(crate::sftp::message::Message::SftpToggleHidden(
                SftpPane::Remote,
            )),
            p,
            t_static(t!("sftp.tooltip_show_hidden")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::RefreshCw,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpRefreshRemote),
            p,
            t_static(t!("sftp.tooltip_refresh")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::Upload,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpUploadSelected),
            p,
            t_static(t!("sftp.tooltip_upload")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::Download,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpDownloadSelected),
            p,
            t_static(t!("sftp.tooltip_download")),
        ))
        .push(icon_btn(
            lucide_icons::Icon::FolderPlus,
            p.text_sec,
            Message::Sftp(crate::sftp::message::Message::SftpNewFolderRequest {
                pane: SftpPane::Remote,
                directory: app.sftp.state.right.pane.path.clone(),
            }),
            p,
            t_static(t!("sftp.tooltip_new_folder")),
        ))
        .push(open_terminal_button(p, SftpPane::Remote))
        .push({
            let btn = button(
                text(char::from(lucide_icons::Icon::Unplug))
                    .font(LUCIDE_FONT)
                    .size(14)
                    .color(p.danger),
            )
            .on_press(Message::Sftp(crate::sftp::message::Message::SftpDisconnect))
            .padding(6)
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: hovered.then_some(Background::Color(p.surface)),
                    border: Border {
                        radius: 4.0.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                }
            })
            .cursor_interaction(mouse::Interaction::default());
            cursor_tooltip_label(btn, t_static(t!("sftp.tooltip_disconnect")), p, None)
        });

    let can_back = app.sftp.state.right.pane.history_idx > 0 && !app.sftp.state.right.pane.loading;
    let can_forward = app.sftp.state.right.pane.history_idx
        < app.sftp.state.right.pane.history.len().saturating_sub(1)
        && !app.sftp.state.right.pane.loading;
    let breadcrumb = view_path_bar(
        &app.sftp.state.right.pane.path,
        SftpPane::Remote,
        app.sftp.state.right.pane.path_edit.as_deref(),
        can_back,
        can_forward,
        p,
    );

    // Connection indicator during the initial connect.
    let connect_state = app.sftp.state.right.pane.attempt.state;
    if !matches!(
        connect_state,
        SshConnectionState::Idle | SshConnectionState::Connected
    ) && (app.sftp.state.right.pane.loading
        || connect_state == SshConnectionState::Failed
        || connect_state == SshConnectionState::WaitingForPassword
        || connect_state == SshConnectionState::WaitingForKeyConfirmation)
    {
        let connect_body = view_sftp_connection_state(
            app,
            SftpPane::Remote,
            connect_state,
            host_icon,
            icon_bg,
            host_name.clone(),
            p,
        );
        return drop_zone_panel(
            REMOTE_DROP_ZONE_ID,
            connect_body,
            p,
            external_drop_feedback(app, SftpExternalDropTarget::Remote),
        );
    }

    let status: Option<Element<'_, Message>> =
        if app.sftp.state.transfer_status.is_some() || app.sftp.state.right.pane.loading {
            None
        } else {
            app.sftp.state.right.pane.error.clone().map(|status| {
                container(
                    text(status)
                        .size(13)
                        .color(p.danger)
                        .ellipsis(text::Ellipsis::End)
                        .wrapping(text::Wrapping::None),
                )
                .padding(Padding::from([0.0, 0.0]))
                .width(Length::Fill)
                .into()
            })
        };

    let file_table = view_file_table(
        &app.sftp.state.right.pane.entries,
        &app.sftp.state.right.pane.filter,
        SftpPane::Remote,
        &app.sftp.state.right.pane.path,
        app.sftp.state.right.pane.selected.as_slice(),
        app.sftp.state.right.pane.hovered.as_deref(),
        app.sftp.state.right.pane.sort_column,
        app.sftp.state.right.pane.sort_direction,
        SFTP_REMOTE_FILE_LIST_ID,
        p,
        app.is_scrollbar_visible(SFTP_REMOTE_FILE_LIST_ID),
    );

    let file_area: Element<'_, Message> = if app.sftp.state.right.pane.loading {
        iced::widget::stack![file_table, view_loading_overlay(p)]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        file_table
    };

    let mut file_section = column![container(breadcrumb).padding(Padding {
        left: 12.0,
        right: 12.0,
        top: 0.0,
        bottom: 0.0
    })]
    .spacing(0);
    if let Some(status) = status {
        file_section = file_section.push(container(status).padding(Padding {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0,
        }));
    }
    file_section = file_section.push(file_area);

    let body = column![
        container(header).padding(Padding {
            left: 12.0,
            right: 12.0,
            top: 0.0,
            bottom: 0.0
        }),
        file_section
    ]
    .spacing(8)
    .height(Length::Fill);

    drop_zone_panel(
        REMOTE_DROP_ZONE_ID,
        body,
        p,
        external_drop_feedback(app, SftpExternalDropTarget::Remote),
    )
}

fn view_host_picker(app: &App, pane: SftpPane) -> Element<'_, Message> {
    let p = palette(app.effective_theme_mode());
    let header = row![
        text(t_static(t!("sftp.select_host")))
            .size(16)
            .color(p.text_pri),
        Space::new().width(Length::Fill),
        icon_btn(
            lucide_icons::Icon::X,
            p.text_sec,
            if pane == SftpPane::Local {
                Message::Sftp(crate::sftp::message::Message::SftpHideLocalHostPicker)
            } else {
                Message::Sftp(crate::sftp::message::Message::SftpHideHostPicker)
            },
            p,
            "",
        ),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let picker = app.sftp.state.host_picker(pane);
    let host_list = crate::sftp::host_picker::visible_hosts(app, pane);
    let active = picker.cursor.selected_key().copied();

    let host_rows: Vec<Element<'_, Message>> = host_list
        .into_iter()
        .map(|h| {
            let id = h.id;
            let is_selected = active == Some(id);
            let connect_msg = if pane == SftpPane::Local {
                Message::Sftp(crate::sftp::message::Message::SftpConnectLocalHost(id))
            } else {
                Message::Sftp(crate::sftp::message::Message::SftpConnectHost(id))
            };
            let icon_bg = h.os_type.icon_bg();
            let content = row![
                container(crate::views::view_os_icon(
                    h.os_type.icon(),
                    14.0,
                    p.text_on_color
                ))
                .width(24)
                .height(24)
                .center(24)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(icon_bg)),
                    border: Border {
                        radius: 5.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                }),
                column![
                    text(h.name.clone())
                        .size(15)
                        .color(p.text_pri)
                        .width(Length::Fill)
                        .ellipsis(text::Ellipsis::End)
                        .wrapping(text::Wrapping::None),
                    text(format!("{}@{}:{}", h.username, h.hostname, h.port))
                        .size(13)
                        .color(p.text_sec)
                        .width(Length::Fill)
                        .ellipsis(text::Ellipsis::End)
                        .wrapping(text::Wrapping::None),
                ]
                .spacing(2),
            ]
            .spacing(12)
            .align_y(iced::Alignment::Center);

            button(content)
                .on_press(connect_msg)
                .width(Length::Fill)
                .height(Length::Fixed(SFTP_HOST_PICKER_ROW_HEIGHT))
                .padding(Padding::from([10.0, 12.0]))
                .style(move |_: &Theme, status| {
                    let hovered = matches!(status, button::Status::Hovered);
                    button::Style {
                        background: Some(Background::Color(if hovered || is_selected {
                            p.item_hover
                        } else {
                            p.surface
                        })),
                        text_color: p.text_pri,
                        border: Border {
                            radius: 6.0.into(),
                            ..Border::default()
                        },
                        ..button::Style::default()
                    }
                })
                .cursor_interaction(mouse::Interaction::default())
                .into()
        })
        .collect();

    let host_count = host_rows.len();
    let total_count = app.hosts.list.len();

    let subtitle = if picker.filter.is_empty() {
        text(t_static(t!("sftp.choose_saved_host")))
            .size(14)
            .color(p.text_sec)
    } else {
        text(
            t!(
                "sftp.host_picker_subtitle",
                count = host_count.to_string(),
                total = total_count.to_string(),
                query = picker.filter.as_str()
            )
            .to_string(),
        )
        .size(14)
        .color(p.text_sec)
    };

    let search_input = text_input(t_static(t!("hosts.search")), &picker.filter)
        .id(Id::new(SFTP_HOST_PICKER_INPUT_ID))
        .on_input(|v0| Message::Sftp(crate::sftp::message::Message::SftpHostPickerFilter(v0)))
        .on_submit(Message::Sftp(
            crate::sftp::message::Message::SftpHostPickerConnect,
        ))
        .padding(8)
        .size(14)
        .width(Length::Fill)
        .style(move |_t, s| input_style_with_palette(&p, s));

    let host_list: Element<'_, Message> = if host_rows.is_empty() && !picker.filter.is_empty() {
        container(
            text(t_static(t!("sftp.no_hosts_match")))
                .size(14)
                .color(p.text_sec),
        )
        .padding(20)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .into()
    } else {
        column(host_rows).spacing(SFTP_HOST_PICKER_ROW_GAP).into()
    };

    drop_zone_panel(
        if pane == SftpPane::Local {
            LOCAL_DROP_ZONE_ID
        } else {
            REMOTE_DROP_ZONE_ID
        },
        column![
            container(header).padding(Padding {
                left: 12.0,
                right: 12.0,
                top: 0.0,
                bottom: 0.0
            }),
            container(search_input).padding(Padding {
                left: 12.0,
                right: 12.0,
                top: 0.0,
                bottom: 0.0
            }),
            container(subtitle).padding(Padding {
                left: 12.0,
                right: 12.0,
                top: 0.0,
                bottom: 0.0
            }),
            styled_scrollable(
                CursorArea::for_target(
                    container(host_list).padding(Padding {
                        left: 12.0,
                        right: 12.0,
                        top: 0.0,
                        bottom: 0.0,
                    }),
                    if pane == SftpPane::Local {
                        CursorTarget::SftpLocalHostPicker
                    } else {
                        CursorTarget::SftpRemoteHostPicker
                    },
                ),
                p,
                app.is_scrollbar_visible(SFTP_HOST_PICKER_SCROLL_ID),
            )
            .id(iced::widget::Id::from(SFTP_HOST_PICKER_SCROLL_ID))
            .on_scroll(move |v| Message::Scrolled(SFTP_HOST_PICKER_SCROLL_ID, v))
            .height(Length::Fill)
        ]
        .spacing(12)
        .height(Length::Fill),
        p,
        external_drop_feedback(
            app,
            if pane == SftpPane::Local {
                SftpExternalDropTarget::Local
            } else {
                SftpExternalDropTarget::Remote
            },
        ),
    )
}

fn icon_btn<'a>(
    icon: lucide_icons::Icon,
    color: Color,
    msg: Message,
    p: ThemePalette,
    tooltip: &str,
) -> Element<'a, Message> {
    let btn = button(text(char::from(icon)).font(LUCIDE_FONT).size(14))
        .on_press(msg)
        .padding(Padding::from([4.0, 6.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.item_hover
                } else {
                    p.surface
                })),
                text_color: if hovered { p.text_pri } else { color },
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    if tooltip.is_empty() {
        btn.into()
    } else {
        cursor_tooltip_label(btn, tooltip, p, None).into()
    }
}

fn icon_nav_btn<'a>(
    icon: lucide_icons::Icon,
    enabled: bool,
    msg: Message,
    p: ThemePalette,
) -> Element<'a, Message> {
    let color = if enabled { p.text_sec } else { p.text_dim };
    let mut btn = button(text(char::from(icon)).font(LUCIDE_FONT).size(14))
        .padding(Padding::from([4.0, 6.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered) && enabled;
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.item_hover
                } else {
                    p.surface
                })),
                text_color: if hovered { p.text_pri } else { color },
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());
    if enabled {
        btn = btn.on_press(msg);
    }
    btn.into()
}

fn drop_zone_panel<'a>(
    id: &'static str,
    content: impl Into<Element<'a, Message>>,
    p: ThemePalette,
    feedback: Option<ExternalDropFeedback>,
) -> Element<'a, Message> {
    let padded_content = container(content).padding(Padding {
        top: 12.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    });

    let body = column![padded_content]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);

    let mut layers = stack![body].width(Length::Fill).height(Length::Fill);
    if let Some(feedback) = feedback {
        layers = layers.push(view_external_drop_overlay(feedback, p));
    }

    container(layers)
        .id(iced::widget::Id::from(id))
        .width(Length::FillPortion(1))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.content_bg)),
            ..Default::default()
        })
        .into()
}

fn view_external_drop_overlay<'a>(
    feedback: ExternalDropFeedback,
    p: ThemePalette,
) -> Element<'a, Message> {
    let colors = external_drop_overlay_colors(feedback.enabled, p);

    let content = column![
        text(char::from(feedback.icon))
            .font(LUCIDE_FONT)
            .size(34)
            .color(colors.icon),
        text(feedback.title).size(18).color(colors.title),
        text(feedback.subtitle).size(13).color(colors.subtitle),
    ]
    .spacing(8)
    .align_x(iced::Alignment::Center);

    let overlay = container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(colors.background)),
            border: Border {
                color: colors.border,
                width: 2.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });

    mouse_area(overlay)
        .interaction(if feedback.enabled {
            iced::mouse::Interaction::Copy
        } else {
            iced::mouse::Interaction::NotAllowed
        })
        .into()
}

fn view_loading_overlay<'a>(p: ThemePalette) -> Element<'a, Message> {
    let indicator: Element<'static, Message> =
        crate::ui::loading_indicator::loading_indicator(28.0, p.accent).into();

    let overlay = container(indicator)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(Color {
                r: p.content_bg.r,
                g: p.content_bg.g,
                b: p.content_bg.b,
                a: 0.55,
            })),
            ..Default::default()
        });

    // Swallow all mouse interaction so the underlying file list doesn't
    // respond to clicks/hover while loading.
    mouse_area(overlay)
        .on_press(Message::Noop)
        .interaction(iced::mouse::Interaction::Progress)
        .into()
}

mod dialogs;
mod file_table;
mod path_bar;
mod preview;
mod transfer;

pub use dialogs::{
    view_cancel_transfer_confirm, view_context_menu, view_delete_confirm, view_new_folder_dialog,
    view_overwrite_confirm, view_rename_dialog,
};
use file_table::view_file_table;
use path_bar::view_path_bar;
use preview::view_preview_fullscreen;
use transfer::{view_queued_transfers, view_transfer_progress, view_transfer_result};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data::FileEntryType;
    use crate::core::theme::ThemeMode;

    /// WCAG 2.1 relative luminance.
    fn luminance(color: Color) -> f32 {
        fn linear(channel: f32) -> f32 {
            if channel <= 0.03928 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    fn contrast_ratio(a: Color, b: Color) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn file_list_reaches_the_bottom_of_the_panel() {
        let mut app = crate::core::test_support::app();
        app.sftp.state.left.pane.path = "/tmp".to_string();
        app.sftp.state.left.pane.entries = (0..80)
            .map(|index| crate::core::data::FileEntry {
                name: format!("file-{index:02}.txt"),
                file_type: FileEntryType::File,
                size: Some(index),
                modified: String::new(),
                modified_epoch: None,
                permissions: String::new(),
                kind: String::new(),
            })
            .collect();

        let mut ui = iced_test::simulator(view_sftp(&app));
        let list = ui
            .find(Id::from(SFTP_LOCAL_FILE_LIST_ID))
            .expect("file list is rendered")
            .visible_bounds()
            .expect("file list is laid out");
        let panel = ui
            .find(Id::from(LOCAL_DROP_ZONE_ID))
            .expect("local panel is rendered")
            .visible_bounds()
            .expect("local panel is laid out");
        assert!(
            (list.y + list.height - panel.y - panel.height).abs() <= 1.0,
            "the file list should reach the bottom of its panel: list={list:?}, panel={panel:?}"
        );
    }

    #[test]
    fn the_connection_indicator_colour_lands_on_both_endpoints() {
        let from = Color::from_rgb8(0x10, 0x20, 0x30);
        let to = Color::from_rgb8(0x90, 0xA0, 0xB0);

        // t = 0 is the retry colour, t = 1 the connected one, so the animated
        // indicator cannot get stuck part-way between the two states.
        assert_eq!(sftp_lerp_color(from, to, 0.0), from);
        assert_eq!(sftp_lerp_color(from, to, 1.0), to);

        // Mid-animation stays opaque: a translucent indicator over the panel
        // would pick up whatever is behind it.
        let mid = sftp_lerp_color(from, to, 0.5);
        assert_eq!(mid.a, 1.0);
        assert!(mid.r > from.r && mid.r < to.r, "the blend is not a jump");
        assert!(mid.g > from.g && mid.g < to.g);
        assert!(mid.b > from.b && mid.b < to.b);
    }

    #[test]
    fn external_drop_feedback_stays_readable_on_every_theme() {
        let modes = [
            ThemeMode::Dark,
            ThemeMode::Light,
            ThemeMode::HighContrastDark,
            ThemeMode::HighContrastLight,
            ThemeMode::Dracula,
            ThemeMode::Novel,
        ];

        for mode in modes {
            let p = palette(mode);
            for enabled in [true, false] {
                // The overlay draws on `content_bg`, so every foreground has
                // to contrast with that surface in both the enabled and the
                // "remote not connected" state.
                let colors = external_drop_overlay_colors(enabled, p);

                let title = contrast_ratio(colors.title, p.content_bg);
                let subtitle = contrast_ratio(colors.subtitle, p.content_bg);
                let icon = contrast_ratio(colors.icon, p.content_bg);

                assert!(
                    title >= 4.5,
                    "{mode:?} drop title (enabled={enabled}) is {title:.2}:1"
                );
                assert!(
                    subtitle >= 4.5,
                    "{mode:?} drop subtitle (enabled={enabled}) is {subtitle:.2}:1"
                );
                assert!(
                    icon >= 3.0,
                    "{mode:?} drop icon (enabled={enabled}) is {icon:.2}:1"
                );
                assert_eq!(colors.background.a, 0.95);
            }
        }
    }
}
