//! Search, filter, labeled, and secure text inputs.

// Rust guideline compliant 2026-10-09

use iced::widget::{button, column, container, row, text, text_input};
use iced::{Background, Border, Color, Element, Length, Theme, mouse};
use secrecy::{ExposeSecret, SecretString};

use crate::core::message::Message;
use crate::core::theme::ThemePalette;

use super::common::LUCIDE_FONT;
use super::metrics::control_padding;

pub fn input_style_with_palette(p: &ThemePalette, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: if p.show_border {
            Background::Color(Color::TRANSPARENT)
        } else {
            Background::Color(p.surface)
        },
        border: Border {
            radius: 6.0.into(),
            color: if focused { p.accent } else { p.border },
            width: 1.0,
        },
        icon: p.text_sec,
        placeholder: p.text_dim,
        value: p.text_pri,
        selection: Color {
            r: p.accent.r,
            g: p.accent.g,
            b: p.accent.b,
            a: 0.3,
        },
    }
}

/// Shared host-filtering search box (Vaults hosts toolbar + new-tab).
pub fn search_input<'a>(
    placeholder: &str,
    value: &str,
    p: &ThemePalette,
    id: Option<iced::widget::Id>,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
) -> Element<'a, Message> {
    let palette = *p;
    let mut input = text_input(placeholder, value)
        .on_input(on_input)
        .on_submit(on_submit)
        .padding(control_padding(13.0, 8.0))
        .size(13)
        .width(Length::Fill)
        .style(move |_, s| input_style_with_palette(&palette, s));
    if let Some(id) = id {
        input = input.id(id);
    }
    input.into()
}

/// Live filter box: [`search_input`] without a submit action, for the list
/// searches that only narrow what is already on screen (Enter does nothing).
pub fn filter_input<'a>(
    placeholder: &str,
    value: &str,
    p: &ThemePalette,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    search_input(placeholder, value, p, None, on_input, Message::Noop)
}

pub fn labeled_input<'a>(
    label: &str,
    placeholder: &str,
    value: &str,
    p: &ThemePalette,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    labeled_input_inner(label, placeholder, value, p, on_input, false)
}

fn labeled_input_inner<'a>(
    label: &str,
    placeholder: &str,
    value: &str,
    p: &ThemePalette,
    on_input: impl Fn(String) -> Message + 'a,
    disabled: bool,
) -> Element<'a, Message> {
    let palette = *p;
    column![
        text(label.to_string())
            .size(12)
            .color(palette.text_sec)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
        text_input(placeholder, value)
            .on_input(move |s| if disabled { Message::Noop } else { on_input(s) })
            .padding(control_padding(13.0, 8.0))
            .size(13)
            .style(move |_, s| input_style_with_palette(&palette, s)),
    ]
    .spacing(4)
    .into()
}

pub fn labeled_password_input<'a>(
    label: &str,
    placeholder: &str,
    value: &SecretString,
    p: &ThemePalette,
    on_input: impl Fn(String) -> Message + 'a,
    secure: bool,
    on_toggle: Message,
) -> Element<'a, Message> {
    labeled_password_input_with_id(
        label,
        placeholder,
        value,
        p,
        on_input,
        secure,
        on_toggle,
        None,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn labeled_password_input_with_id<'a>(
    label: &str,
    placeholder: &str,
    value: &SecretString,
    p: &ThemePalette,
    on_input: impl Fn(String) -> Message + 'a,
    secure: bool,
    on_toggle: Message,
    id: Option<iced::widget::Id>,
) -> Element<'a, Message> {
    let palette = *p;
    column![
        text(label.to_string())
            .size(12)
            .color(palette.text_sec)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
        password_input_row(placeholder, value, secure, p, id, on_input, None, on_toggle),
    ]
    .spacing(4)
    .into()
}

/// Secure text field with its eye toggle: the body shared by every password and
/// key-passphrase field (Vaults forms, SSH connect, SFTP connect).
///
/// `on_submit` is optional — forms that submit through the global Enter handler
/// pass `None`, while the connect prompts submit on Enter themselves.
#[expect(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn password_input_row<'a>(
    placeholder: &str,
    value: &SecretString,
    secure: bool,
    p: &ThemePalette,
    id: Option<iced::widget::Id>,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Option<Message>,
    on_toggle: Message,
) -> Element<'a, Message> {
    let palette = *p;
    let visibility_icon = if secure {
        lucide_icons::Icon::Eye
    } else {
        lucide_icons::Icon::EyeOff
    };
    let toggle_btn = button(
        text(char::from(visibility_icon))
            .font(LUCIDE_FONT)
            .size(13)
            .color(palette.text_sec),
    )
    .on_press(on_toggle)
    .padding(control_padding(13.0, 10.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: if hovered {
                Some(Background::Color(palette.item_hover))
            } else {
                Some(Background::Color(palette.surface))
            },
            text_color: palette.text_sec,
            border: Border {
                radius: 6.0.into(),
                color: palette.border,
                width: 1.0,
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default());

    let mut input = text_input(placeholder, value.expose_secret())
        .secure(secure)
        .on_input(on_input)
        .padding(control_padding(13.0, 8.0))
        .size(13)
        .style(move |_, s| input_style_with_palette(&palette, s));
    if let Some(id) = id {
        input = input.id(id);
    }
    if let Some(msg) = on_submit {
        input = input.on_submit(msg);
    }

    row![container(input).width(Length::Fill), toggle_btn]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_inputs_are_outlined_with_the_accent_colour() {
        let dark = crate::core::theme::palette(crate::core::theme::ThemeMode::Dark);

        let focused =
            input_style_with_palette(&dark, text_input::Status::Focused { is_hovered: false });
        let blurred = input_style_with_palette(&dark, text_input::Status::Active);

        assert_eq!(focused.border.color, dark.accent, "focus is visible");
        assert_eq!(blurred.border.color, dark.border);
        assert_eq!(focused.value, dark.text_pri);
        assert_eq!(focused.placeholder, dark.text_dim);
        assert_eq!(
            focused.selection.a, 0.3,
            "a selection must show the text underneath"
        );
    }

    #[test]
    fn themes_that_draw_their_own_borders_leave_the_input_fill_transparent() {
        use crate::core::theme::ThemeMode;

        // Light and high-contrast themes set `show_border`, so the input keeps
        // the panel's own fill instead of painting a second surface on top.
        let light = crate::core::theme::palette(ThemeMode::Light);
        assert!(light.show_border);
        let styled = input_style_with_palette(&light, text_input::Status::Active);
        assert_eq!(styled.background, Background::Color(Color::TRANSPARENT));

        // Dark themes have no border, so the fill is what separates the field.
        let dark = crate::core::theme::palette(ThemeMode::Dark);
        assert!(!dark.show_border);
        let styled = input_style_with_palette(&dark, text_input::Status::Active);
        assert_eq!(styled.background, Background::Color(dark.surface));
    }
}
