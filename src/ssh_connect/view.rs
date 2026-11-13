use iced::widget::{Space, button, column, container, row, text};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme, alignment, mouse};
use secrecy::SecretString;

use crate::app::App;
use crate::core::data::AuthType;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::{self, ThemePalette};
use crate::core::widgets::styled_scrollable;
use crate::hosts::form_view::view_form;
use crate::ssh_connect::SshConnectionState;
use crate::ui::host_key_prompt::{
    TrustActions, action_row as host_key_action_row, panel as host_key_panel,
};
use crate::ui::password_prompt::{self, PasswordPrompt};
use crate::ui::slide::slide_panel_from_right;
use rust_i18n::t;

pub const SSH_CONNECT_LOGS_SCROLL_ID: &str = "ssh_connect_logs_scroll";
pub const SSH_PASSWORD_INPUT_ID: &str = "ssh_password_input";

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_ssh_connect<'a>(
    state: &'a crate::ssh_connect::SshConnectState,
    app: &'a App,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let s = state;
    let host = if s.attempt.jump_host.is_some()
        && s.attempt.stage == crate::ssh_connect::SshConnectStage::Jump
    {
        s.attempt.jump_host.as_ref()
    } else {
        s.attempt.pending_host.as_ref()
    };
    let logs: &[String] = &s.logs;
    let anim_t = s.anim_t;
    let is_key_passphrase = host.is_some_and(|host| host.auth_type == AuthType::Key);

    let title = host.map_or(t_static(t!("ssh_connect.title")), |host| host.name.as_str());
    let subtitle = host.map_or_else(
        || t_static(t!("hosts.no_host_selected")).to_string(),
        |host| format!("SSH {}@{}:{}", host.username, host.hostname, host.port),
    );

    let title = text(title)
        .size(32)
        .color(p.text_pri)
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None);
    let subtitle = text(subtitle)
        .size(16)
        .color(p.text_sec)
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None);
    let title_block = container(column![title, subtitle].spacing(4)).width(Length::Fixed(560.0));

    let connection_indicator = view_connection_indicator(s.attempt.state, anim_t, p);
    let status_text = view_status_text(s.attempt.state, anim_t, p, is_key_passphrase);

    // The password panel and key-confirmation dialog render only in their
    // respective connection states; otherwise fall back to the logs view.
    let main_content: Element<'_, Message> = if s.attempt.state
        == SshConnectionState::WaitingForKeyConfirmation
    {
        let confirm = view_host_key_confirm_dialog(s, p);
        let actions = view_key_confirm_actions(p);

        connect_page(
            p,
            title_block,
            container(connection_indicator).width(Length::Fixed(560.0)),
            status_text,
            confirm,
            container(actions).width(Length::Fixed(560.0)),
        )
    } else if s.attempt.state == SshConnectionState::WaitingForPassword {
        let prompt = PasswordPrompt {
            label: if is_key_passphrase {
                t_static(t!("ssh.key_passphrase"))
            } else {
                t_static(t!("ssh.password"))
            },
            placeholder: if is_key_passphrase {
                t_static(t!("ssh.enter_key_passphrase"))
            } else {
                t_static(t!("ssh.enter_password"))
            },
            value: &s.attempt.password_input,
            visible: s.attempt.password_visible,
            input_id: SSH_PASSWORD_INPUT_ID,
            submit_label: t_static(t!("ssh.connect")),
            submit: Message::SshConnect(crate::ssh_connect::message::Message::SshPasswordSubmit),
            toggle: Message::SshConnect(
                crate::ssh_connect::message::Message::SshTogglePasswordVisible,
            ),
            cancel: Message::SshConnect(crate::ssh_connect::message::Message::SshClose),
        };
        let password_panel = password_prompt::panel(&prompt, Length::Fill, &p, |value| {
            Message::SshConnect(crate::ssh_connect::message::Message::SshPasswordChanged(
                SecretString::from(value),
            ))
        });
        let actions = password_prompt::actions(&prompt, &p);

        connect_page(
            p,
            title_block,
            connection_indicator,
            status_text,
            password_panel,
            actions,
        )
    } else {
        let logs_panel = view_logs_panel(
            p,
            s.attempt.state,
            host,
            logs,
            app.is_scrollbar_visible(SSH_CONNECT_LOGS_SCROLL_ID),
        );
        let actions = if s.attempt.state == SshConnectionState::Failed {
            row![
                styled_retry_btn(p),
                styled_edit_host_btn(p),
                styled_close_btn(p)
            ]
            .spacing(12)
            .into()
        } else {
            styled_close_btn(p)
        };

        connect_page(
            p,
            title_block,
            connection_indicator,
            status_text,
            logs_panel,
            actions,
        )
    };

    let top_border = container(text(""))
        .width(Length::Fill)
        .height(1)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..Default::default()
        });

    column![
        top_border,
        row![
            main_content,
            slide_panel_from_right(
                view_form(app),
                320.0,
                app.hosts.form.visible,
                app.hosts.form_anim
            ),
        ]
        .spacing(0)
        .height(Length::Fill),
    ]
    .spacing(0)
    .height(Length::Fill)
    .into()
}

/// Shared page scaffold for the SSH connect screen. Every connection state
/// renders the same title / indicator / status stack and differs only in the
/// central widget (`content`) and its action buttons (`actions`).
fn connect_page<'a>(
    p: ThemePalette,
    title_block: impl Into<Element<'a, Message>>,
    connection_indicator: impl Into<Element<'a, Message>>,
    status_text: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    actions: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    let page = column![
        title_block.into(),
        container(text("")).height(40),
        connection_indicator.into(),
        container(text("")).height(12),
        status_text.into(),
        container(text("")).height(24),
        container(content.into()).width(Length::Fixed(560.0)),
        container(text("")).height(24),
        actions.into(),
    ]
    .spacing(0)
    .align_x(iced::Alignment::Center);

    container(
        container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::Alignment::Center)
            .padding(Padding::from([32.0, 48.0])),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(p.content_bg)),
        ..Default::default()
    })
    .into()
}

fn view_host_key_confirm_dialog(
    state: &crate::ssh_connect::SshConnectState,
    p: ThemePalette,
) -> Element<'_, Message> {
    let Some(key_info) = &state.attempt.pending_key_info else {
        return container(
            text(t_static(t!("ssh.key_info_unavailable")))
                .size(13)
                .color(p.text_dim),
        )
        .into();
    };
    host_key_panel(key_info, None, p)
}

fn view_key_confirm_actions(p: ThemePalette) -> Element<'static, Message> {
    host_key_action_row(
        TrustActions::new(
            t_static(t!("ssh.close")),
            Message::SshConnect(crate::ssh_connect::message::Message::SshClose),
            Message::SshConnect(crate::ssh_connect::message::Message::SshKeyConfirmContinue),
            Message::SshConnect(crate::ssh_connect::message::Message::SshKeyConfirmAccept),
        ),
        p,
    )
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

pub fn view_connection_indicator(
    state: SshConnectionState,
    anim_t: f32,
    p: ThemePalette,
) -> Element<'static, Message> {
    // Three-icon layout for host key confirmation
    if state == SshConnectionState::WaitingForKeyConfirmation {
        let icon = |icon: lucide_icons::Icon, color: Color| -> Element<'static, Message> {
            container(
                text(char::from(icon))
                    .font(LUCIDE_FONT)
                    .size(28)
                    .color(color),
            )
            .width(Length::Fixed(44.0))
            .height(Length::Fixed(44.0))
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
            bar(180.0, p.accent),
            icon(lucide_icons::Icon::Fingerprint, p.accent),
            bar(180.0, lerp_color(p.surface, p.text_dim, 0.35)),
            icon(lucide_icons::Icon::Server, p.text_dim),
        ]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into();
    }

    // Two-icon layout for all other states
    let pulse = (anim_t * std::f32::consts::TAU).sin() * 0.25 + 0.75;

    let (start_icon, start_color) = match state {
        SshConnectionState::Idle => (lucide_icons::Icon::Monitor, p.text_dim),
        SshConnectionState::WaitingForPassword => (lucide_icons::Icon::Monitor, p.accent),
        SshConnectionState::Connecting => (
            lucide_icons::Icon::Monitor,
            lerp_color(
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
            lerp_color(
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
            .size(28)
            .color(start_color),
    )
    .width(Length::Fixed(44.0))
    .height(Length::Fixed(44.0))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center);

    let progress_bar = view_progress_bar(state, anim_t, p, 280.0);

    let end = container(
        text(char::from(end_icon))
            .font(LUCIDE_FONT)
            .size(28)
            .color(end_color),
    )
    .width(Length::Fixed(44.0))
    .height(Length::Fixed(44.0))
    .align_x(iced::Alignment::Center)
    .align_y(iced::Alignment::Center);

    row![start, progress_bar, end]
        .spacing(16)
        .align_y(iced::Alignment::Center)
        .into()
}

pub fn view_progress_bar(
    state: SshConnectionState,
    anim_t: f32,
    p: ThemePalette,
    total_w: f32,
) -> Element<'static, Message> {
    let bar_h = 4.0_f32;
    let radius = Border {
        radius: 2.0.into(),
        color: Color::TRANSPARENT,
        width: 0.0,
    };

    match state {
        SshConnectionState::WaitingForPassword => container(text(""))
            .width(Length::Fixed(total_w))
            .height(Length::Fixed(bar_h))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(lerp_color(p.surface, p.accent, 0.35))),
                border: Border {
                    radius: 2.0.into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                ..Default::default()
            })
            .into(),
        SshConnectionState::Connecting => {
            // Shimmer: a bright segment scanning left→right, looping every ~0.7s
            let shimmer_w = 64.0_f32;
            let travel = total_w + shimmer_w;
            let pos = (anim_t * (travel / 0.7)) % travel - shimmer_w;

            let left_w = pos.clamp(0.0, total_w);
            let shim_start = pos.max(0.0_f32);
            let shim_end = (pos + shimmer_w).clamp(0.0, total_w);
            let shim_w = (shim_end - shim_start).max(0.0);
            let right_w = (total_w - shim_end).max(0.0);

            let track = lerp_color(p.surface, p.accent, 0.22);

            let seg = |w: f32, color: Color| -> Element<'static, Message> {
                container(text(""))
                    .width(Length::Fixed(w.max(0.0)))
                    .height(Length::Fixed(bar_h))
                    .style(move |_: &Theme| container::Style {
                        background: Some(Background::Color(color)),
                        border: radius,
                        ..Default::default()
                    })
                    .into()
            };

            row![
                seg(left_w, track),
                seg(shim_w, p.accent),
                seg(right_w, track),
            ]
            .spacing(0)
            .into()
        }
        SshConnectionState::Connected => container(text(""))
            .width(Length::Fixed(total_w))
            .height(Length::Fixed(bar_h))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.success)),
                border: Border {
                    radius: 2.0.into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                ..Default::default()
            })
            .into(),
        SshConnectionState::Failed => container(text(""))
            .width(Length::Fixed(total_w))
            .height(Length::Fixed(bar_h))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.danger)),
                border: Border {
                    radius: 2.0.into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                ..Default::default()
            })
            .into(),
        SshConnectionState::WaitingForKeyConfirmation => container(text(""))
            .width(Length::Fixed(total_w))
            .height(Length::Fixed(bar_h))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(lerp_color(p.surface, p.accent, 0.35))),
                border: Border {
                    radius: 2.0.into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                ..Default::default()
            })
            .into(),
        SshConnectionState::Idle => container(text(""))
            .width(Length::Fixed(total_w))
            .height(Length::Fixed(bar_h))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.surface)),
                border: Border {
                    radius: 2.0.into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                ..Default::default()
            })
            .into(),
    }
}

pub fn view_status_text(
    state: SshConnectionState,
    _anim_t: f32,
    p: ThemePalette,
    key_passphrase: bool,
) -> Element<'static, Message> {
    let (label, color): (String, Color) = match state {
        SshConnectionState::Idle => (t_static(t!("ssh.waiting_connect")).into(), p.text_dim),
        SshConnectionState::WaitingForPassword => {
            if key_passphrase {
                (t_static(t!("ssh.key_passphrase_required")).into(), p.accent)
            } else {
                (t_static(t!("ssh.password_required")).into(), p.accent)
            }
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
            .align_x(alignment::Horizontal::Center)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
    )
    .width(Length::Fixed(560.0))
    .into()
}

pub fn view_logs_panel(
    p: ThemePalette,
    state: SshConnectionState,
    host: Option<&crate::core::data::Host>,
    logs: &[String],
    scrollbar_visible: bool,
) -> Element<'static, Message> {
    let target = host.map_or_else(
        || t_static(t!("ssh_connect.remote_host")).to_string(),
        |host| format!("{}@{}:{}", host.username, host.hostname, host.port),
    );

    let copy_btn: Element<'_, Message> = if logs.is_empty() {
        Space::new().width(0).height(0).into()
    } else {
        button(
            text(char::from(lucide_icons::Icon::Copy))
                .font(LUCIDE_FONT)
                .size(13)
                .color(p.text_sec),
        )
        .on_press(Message::SshConnect(
            crate::ssh_connect::message::Message::CopySshLogs,
        ))
        .padding(6)
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.button_hover
                } else {
                    p.button_bg
                })),
                text_color: if hovered { p.text_pri } else { p.text_sec },
                border: Border {
                    radius: 4.0.into(),
                    color: p.border,
                    width: 1.0,
                },
                ..Default::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
    };

    let header = row![
        container(
            text(char::from(lucide_icons::Icon::ScrollText))
                .font(LUCIDE_FONT)
                .size(14)
                .color(p.accent),
        )
        .width(16)
        .center_x(16),
        Space::new().width(Length::Fill),
        copy_btn,
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let fallback_logs = match state {
        SshConnectionState::Idle | SshConnectionState::WaitingForPassword => {
            vec![t!("ssh.waiting_for_connection").to_string()]
        }
        SshConnectionState::Connecting => vec![
            t!("ssh.connecting_to", target = target).to_string(),
            t!("ssh.checking_auth").to_string(),
        ],
        SshConnectionState::Connected => vec![
            t!("ssh.auth_success").to_string(),
            t!("ssh.opening_session").to_string(),
        ],
        SshConnectionState::Failed => vec![
            t!("ssh.auth_failed").to_string(),
            t!("ssh.check_creds").to_string(),
        ],
        SshConnectionState::WaitingForKeyConfirmation => vec![
            t!("ssh.host_key_required").to_string(),
            t!("ssh.review_key").to_string(),
        ],
    };
    let logs_content: Vec<String> = if logs.is_empty() {
        fallback_logs
    } else {
        logs.to_vec()
    };

    let log_lines: Vec<Element<'_, Message>> = logs_content
        .iter()
        .enumerate()
        .map(|(index, log)| {
            let color = if index == logs_content.len() - 1 {
                match state {
                    SshConnectionState::Connected => p.success,
                    SshConnectionState::Failed => p.danger,
                    _ => p.text_sec,
                }
            } else {
                p.text_sec
            };
            log_row(index + 1, log.clone(), color, p)
        })
        .collect();

    let mut logs_scrollable = styled_scrollable(
        container(column(log_lines).spacing(0)).padding(Padding::ZERO.right(12.0)),
        p,
        scrollbar_visible,
    )
    .id(iced::widget::Id::from(SSH_CONNECT_LOGS_SCROLL_ID))
    .on_scroll(move |v| Message::Scrolled(SSH_CONNECT_LOGS_SCROLL_ID, v));

    // When the connection fails, new log output is appended in a batch — anchor
    // the scrollable to the bottom so the most recent (failure) lines are
    // always visible instead of being hidden above the fold.
    if state == SshConnectionState::Failed {
        logs_scrollable = logs_scrollable.anchor_bottom();
    }

    let body = container(logs_scrollable)
        .width(Length::Fill)
        .height(Length::Fixed(184.0))
        .padding(Padding {
            top: 12.0,
            right: 0.0,
            bottom: 12.0,
            left: 8.0,
        })
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.surface)),
            border: Border {
                radius: 6.0.into(),
                color: p.border,
                width: 1.0,
            },
            ..Default::default()
        });

    panel_container(p, column![header, body].spacing(8))
}

fn panel_container<'a>(
    p: ThemePalette,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(content)
        .padding(12)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.card_bg)),
            border: Border {
                radius: 8.0.into(),
                color: p.border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

fn log_row(index: usize, line: String, color: Color, p: ThemePalette) -> Element<'static, Message> {
    row![
        container(text(format!("{index:02}")).size(11).color(p.text_dim))
            .width(Length::Fixed(28.0))
            .padding(Padding::from([2.0, 0.0])),
        text(line)
            .size(12)
            .color(color)
            .width(Length::Fill)
            .wrapping(text::Wrapping::WordOrGlyph),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Start)
    .padding(Padding::from([2.0, 8.0]))
    .into()
}

fn styled_retry_btn(p: ThemePalette) -> Element<'static, Message> {
    button(
        text(t_static(t!("ssh.retry")))
            .size(13)
            .color(p.text_on_color),
    )
    .on_press(Message::SshConnect(
        crate::ssh_connect::message::Message::SshRetry,
    ))
    .padding(Padding::from([10.0, 24.0]))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                Color {
                    r: p.accent.r * 0.85,
                    g: p.accent.g * 0.85,
                    b: p.accent.b * 0.85,
                    a: 1.0,
                }
            } else {
                p.accent
            })),
            text_color: p.text_on_color,
            border: Border {
                radius: 6.0.into(),
                color: p.accent,
                width: 1.0,
            },
            ..Default::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default())
    .into()
}

fn styled_edit_host_btn(p: ThemePalette) -> Element<'static, Message> {
    button(
        row![
            text(char::from(lucide_icons::Icon::Pencil))
                .font(LUCIDE_FONT)
                .size(12)
                .color(p.text_pri),
            text(t_static(t!("ssh.edit_host")))
                .size(12)
                .color(p.text_pri),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::SshConnect(
        crate::ssh_connect::message::Message::SshEditHost,
    ))
    .padding(Padding::from([10.0, 14.0]))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                p.button_hover
            } else {
                p.button_bg
            })),
            text_color: p.text_pri,
            border: Border {
                radius: 5.0.into(),
                width: 1.0,
                color: if hovered {
                    p.button_border_hover
                } else {
                    p.button_border
                },
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default())
    .into()
}

fn styled_close_btn(p: ThemePalette) -> Element<'static, Message> {
    button(text(t_static(t!("ssh.close"))).size(12).color(p.text_pri))
        .on_press(Message::SshConnect(
            crate::ssh_connect::message::Message::SshClose,
        ))
        .padding(Padding::from([10.0, 14.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.button_hover
                } else {
                    p.button_bg
                })),
                text_color: p.text_pri,
                border: Border {
                    radius: 5.0.into(),
                    width: 1.0,
                    color: if hovered {
                        p.button_border_hover
                    } else {
                        p.button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}
