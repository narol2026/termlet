//! Reusable confirm-dialog building blocks.
//!
//! Confirms render on the [`crate::ui::message_dialog`] shell (header / body /
//! separator / footer, zoom-in): [`message_confirm`] for title + body +
//! [Cancel, Confirm], [`delete_confirm_dialog`] for the common "Delete X?"
//! pattern, and [`accent_outline_button`] / [`cancel_button`] /
//! [`danger_outline_button`] / [`confirm_button`] as the button helpers.

use iced::widget::{button, column, text};
use iced::{Background, Border, Color, Element, Theme, mouse};

use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::control_padding;
use crate::ui::message_dialog;
use rust_i18n::t;

/// Standard "Cancel" button used in confirm dialogs.
pub fn cancel_button(msg: Message, p: &ThemePalette) -> Element<'static, Message> {
    cancel_button_with_label(t_static(t!("settings.cancel")), msg, p)
}

/// Cancel-style button with a custom label (e.g. "Close" instead of "Cancel").
pub fn cancel_button_with_label(
    label: &str,
    msg: Message,
    p: &ThemePalette,
) -> Element<'static, Message> {
    let button_bg = p.button_bg;
    let button_hover = p.button_hover;
    let button_border = p.button_border;
    let button_border_hover = p.button_border_hover;
    let text_pri = p.text_pri;
    button(text(label.to_string()).size(12).color(text_pri))
        .on_press(msg)
        .padding(control_padding(12.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    button_hover
                } else {
                    button_bg
                })),
                text_color: text_pri,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if hovered {
                        button_border_hover
                    } else {
                        button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}

/// Danger-outlined button for destructive choices that should stay secondary.
///
/// The neutral fill keeps it below a solid primary action, while the red label
/// and border make the consequence explicit. Hovering adds a faint red wash.
pub fn danger_outline_button(
    label: &str,
    msg: Message,
    p: &ThemePalette,
) -> Element<'static, Message> {
    outline_button(label, msg, p.danger, p)
}

/// Accent-outlined button for safe actions that should stay secondary.
///
/// The neutral fill keeps it below a solid primary action, while the accent
/// label and border make the action discoverable. Hovering adds a faint accent
/// wash.
pub fn accent_outline_button(
    label: &str,
    msg: Message,
    p: &ThemePalette,
) -> Element<'static, Message> {
    outline_button(label, msg, p.accent, p)
}

fn outline_button(
    label: &str,
    msg: Message,
    color: Color,
    p: &ThemePalette,
) -> Element<'static, Message> {
    let background = p.button_bg;
    button(text(label.to_string()).size(12).color(color))
        .on_press(msg)
        .padding(control_padding(12.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    Color { a: 0.16, ..color }
                } else {
                    background
                })),
                text_color: color,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color,
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}

/// Colored action button (danger, accent, etc.) used in confirm dialogs.
///
/// When `disabled` is true the `on_press` callback is omitted and the button
/// renders with a dimmed appearance.
pub fn confirm_button(
    label: &str,
    msg: Message,
    color: Color,
    text_color: Color,
    disabled: bool,
) -> Element<'static, Message> {
    let bg = color;
    let fg = text_color;
    let mut btn = button(text(label.to_string()).size(12).color(fg))
        .padding(control_padding(12.0, 18.0))
        .style(move |_: &Theme, status| {
            if disabled {
                let disabled_bg = Color { a: 0.35, ..bg };
                return button::Style {
                    background: Some(Background::Color(disabled_bg)),
                    text_color: fg,
                    border: Border {
                        radius: 6.0.into(),
                        width: 1.0,
                        color: disabled_bg,
                    },
                    ..button::Style::default()
                };
            }
            let hovered = matches!(status, button::Status::Hovered);
            let light_bg = (bg.r + bg.g + bg.b) / 3.0 > 0.85;
            let bg_color = if hovered {
                if light_bg {
                    Color {
                        r: (bg.r - 0.04).max(0.0),
                        g: (bg.g - 0.04).max(0.0),
                        b: (bg.b - 0.04).max(0.0),
                        a: 1.0,
                    }
                } else {
                    Color {
                        r: (bg.r + 0.06).min(1.0),
                        g: (bg.g + 0.06).min(1.0),
                        b: (bg.b + 0.06).min(1.0),
                        a: 1.0,
                    }
                }
            } else {
                bg
            };
            button::Style {
                background: Some(Background::Color(bg_color)),
                text_color: fg,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: bg_color,
                },
                ..button::Style::default()
            }
        });
    if !disabled {
        btn = btn.on_press(msg);
    }
    btn.cursor_interaction(mouse::Interaction::default()).into()
}

/// Simple confirm dialog on the message-dialog shell: bold title, one dim body
/// line, and [Cancel, Confirm] actions. `confirm_color` is typically
/// `p.danger`. Pass `backdrop_on_press: None` for a non-dismissible backdrop
/// (e.g. the quit dialog).
#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn message_confirm(
    title: impl Into<String>,
    body_text: impl Into<String>,
    confirm_label: impl Into<String>,
    confirm_msg: Message,
    cancel_msg: &Message,
    cancel_label: Option<String>,
    confirm_color: Color,
    backdrop_on_press: Option<Message>,
    p: &ThemePalette,
    anim: f32,
) -> Element<'static, Message> {
    let confirm_label = confirm_label.into();
    let cancel_btn = match cancel_label {
        Some(label) => cancel_button_with_label(&label, cancel_msg.clone(), p),
        None => cancel_button(cancel_msg.clone(), p),
    };
    let actions: Vec<Element<'static, Message>> = vec![
        cancel_btn,
        confirm_button(
            confirm_label.as_str(),
            confirm_msg,
            confirm_color,
            Color::WHITE,
            false,
        ),
    ];

    message_dialog::message_dialog(
        title,
        Some(body_text),
        column![].into(),
        actions,
        backdrop_on_press.unwrap_or(Message::Noop),
        360.0,
        p,
        anim,
    )
}

/// The most common confirm: "Delete X?" with a danger-colored Delete button and
/// dismiss-on-backdrop-click.
pub fn delete_confirm_dialog(
    title: impl Into<String>,
    body_text: impl Into<String>,
    cancel_msg: Message,
    confirm_msg: Message,
    p: &ThemePalette,
    anim: f32,
) -> Element<'static, Message> {
    message_confirm(
        title,
        body_text,
        t_static(t!("dialog.delete")),
        confirm_msg,
        &cancel_msg.clone(),
        None,
        p.danger,
        Some(cancel_msg),
        p,
        anim,
    )
}
