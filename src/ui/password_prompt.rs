//! Password fields and actions shared by every credential prompt.

// Rust guideline compliant 2026-10-09

use iced::widget::{button, column, container, row, text};
use iced::{Background, Border, Color, Element, Length, Theme, mouse};
use secrecy::{ExposeSecret, SecretString};

use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::{control_padding, password_input_row};
use rust_i18n::t;

/// Domain-neutral data and messages for one password prompt.
pub(crate) struct PasswordPrompt<'a> {
    pub label: &'a str,
    pub placeholder: &'a str,
    pub value: &'a SecretString,
    pub visible: bool,
    pub input_id: &'static str,
    pub submit_label: &'a str,
    pub submit: Message,
    pub toggle: Message,
    pub cancel: Message,
}

/// Render the labelled password field.
pub(crate) fn field<'a>(
    prompt: &PasswordPrompt<'a>,
    p: &ThemePalette,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(prompt.label.to_string()).size(12).color(p.text_sec),
        password_input_row(
            prompt.placeholder,
            prompt.value,
            !prompt.visible,
            p,
            Some(iced::widget::Id::new(prompt.input_id)),
            on_input,
            Some(prompt.submit.clone()),
            prompt.toggle.clone(),
        ),
    ]
    .spacing(6)
    .into()
}

/// Render the field inside the card used by full-page connection flows.
pub(crate) fn panel<'a>(
    prompt: &PasswordPrompt<'a>,
    width: Length,
    p: &ThemePalette,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let palette = *p;
    container(field(prompt, p, on_input))
        .padding(16)
        .width(width)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(palette.card_bg)),
            border: Border {
                radius: 8.0.into(),
                color: palette.border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// Render cancel and connect actions for a password prompt.
pub(crate) fn actions(prompt: &PasswordPrompt<'_>, p: &ThemePalette) -> Element<'static, Message> {
    let enabled = !prompt.value.expose_secret().is_empty();
    let palette = *p;
    let accent = p.accent;
    let connect_btn = button(
        text(prompt.submit_label.to_string())
            .size(13)
            .color(Color::WHITE),
    )
    .on_press_maybe(enabled.then(|| prompt.submit.clone()))
    .padding(control_padding(13.0, 14.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let background = if enabled {
            if hovered {
                Color {
                    r: (accent.r + 0.04).min(1.0),
                    g: (accent.g + 0.04).min(1.0),
                    b: (accent.b + 0.04).min(1.0),
                    a: accent.a,
                }
            } else {
                accent
            }
        } else {
            Color { a: 0.45, ..accent }
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: Color::WHITE,
            border: Border {
                radius: 6.0.into(),
                color: accent,
                width: 1.0,
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let cancel_label = t_static(t!("settings.cancel"));
    let cancel_btn = button(text(cancel_label.to_string()).size(13).color(p.text_pri))
        .on_press(prompt.cancel.clone())
        .padding(control_padding(13.0, 14.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    palette.button_hover
                } else {
                    palette.button_bg
                })),
                text_color: palette.text_pri,
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if hovered {
                        palette.button_border_hover
                    } else {
                        palette.button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    row![cancel_btn, connect_btn].spacing(12).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt(value: &SecretString) -> PasswordPrompt<'_> {
        PasswordPrompt {
            label: "Password",
            placeholder: "Enter password",
            value,
            visible: false,
            input_id: "shared_password_prompt_test",
            submit_label: "Connect",
            submit: Message::Noop,
            toggle: Message::Noop,
            cancel: Message::WindowClose,
        }
    }

    #[test]
    fn actions_route_cancel_and_submit() {
        let value = SecretString::from("secret");
        let prompt = prompt(&value);
        let mut ui = iced_test::simulator(actions(&prompt, &crate::core::theme::dark_palette()));

        ui.click("Cancel").expect("cancel is clickable");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(matches!(messages.as_slice(), [Message::WindowClose]));

        let mut ui = iced_test::simulator(actions(&prompt, &crate::core::theme::dark_palette()));
        ui.click("Connect").expect("connect is clickable");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(matches!(messages.as_slice(), [Message::Noop]));
    }

    #[test]
    fn empty_password_disables_submit() {
        let value = SecretString::from(String::new());
        let prompt = prompt(&value);
        let mut ui = iced_test::simulator(actions(&prompt, &crate::core::theme::dark_palette()));

        ui.click("Connect")
            .expect("the disabled action is still rendered");
        assert!(ui.into_messages().next().is_none());
    }
}
