//! Shared form panels, headers, actions, and read-only fields.

// Rust guideline compliant 2026-10-09

use iced::widget::{Space, button, column, container, row, text};
use iced::{Background, Border, Element, Font, Length, Theme, mouse};
use rust_i18n::t;

use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;

use super::buttons::{secondary_btn, styled_btn};
use super::common::LUCIDE_FONT;
use super::metrics::control_padding;
use super::scroll::styled_scrollable;

/// Shared right-side form panel: a 1px left divider plus a scrollable content
/// area with the standard sidebar background.
pub fn form_panel<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    width: f32,
    p: ThemePalette,
) -> Element<'a, M> {
    let border_color = p.border;

    row![
        container(Space::new())
            .width(1)
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(border_color)),
                ..Default::default()
            }),
        container(styled_scrollable(content, p, false))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.sidebar_bg)),
                ..Default::default()
            }),
    ]
    .width(width)
    .height(Length::Fill)
    .into()
}

/// X close button used in the top-right of every form / side-panel header
/// (Hosts, Keychain, Port-Forward, Known-Hosts, Snippets).
/// No border — a hover background is the only affordance, so the button reads
/// as clickable without competing with the form's title. Sized to
/// [`crate::core::widgets::CONTROL_HEIGHT`] like every other control inside the
/// form.
pub fn form_close_btn<'a>(on_press: Message, p: ThemePalette) -> Element<'a, Message> {
    button(
        text(char::from(lucide_icons::Icon::X))
            .font(LUCIDE_FONT)
            .size(12)
            .color(p.text_sec),
    )
    .on_press(on_press)
    .padding(control_padding(12.0, 8.0))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: hovered.then_some(Background::Color(p.item_hover)),
            text_color: if hovered { p.text_pri } else { p.text_sec },
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::Pointer)
    .into()
}

/// Shared form header: a title and the standard top-right close button. Forms
/// and side panels should use this instead of assembling the row themselves so
/// the title size, colour, and close affordance stay identical everywhere.
pub fn form_header<'a>(title: String, close_msg: Message, p: ThemePalette) -> Element<'a, Message> {
    let close_btn = form_close_btn(close_msg, p);

    row![
        text(title)
            .size(16)
            .color(p.text_pri)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
        close_btn,
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

/// Shared form action row: Save / Cancel on the left and an optional Delete on
/// the right. This keeps every editor panel aligned with the same button sizes,
/// spacing, and enabled-state handling.
pub fn form_actions<'a>(
    save_label: &'a str,
    cancel_label: &'a str,
    can_save: bool,
    save_msg: Message,
    cancel_msg: Message,
    delete_msg: Option<Message>,
    p: ThemePalette,
) -> Element<'a, Message> {
    let save_btn = styled_btn(
        save_label,
        can_save.then_some(save_msg),
        p.accent,
        iced::Color::WHITE,
        p,
    );
    form_actions_with_save(save_btn, cancel_label, cancel_msg, delete_msg, p)
}

/// Variant of [`form_actions`] for forms whose primary button needs custom
/// content (for example a loading indicator while key generation runs).
pub fn form_actions_with_save<'a>(
    save_btn: Element<'a, Message>,
    cancel_label: &'a str,
    cancel_msg: Message,
    delete_msg: Option<Message>,
    p: ThemePalette,
) -> Element<'a, Message> {
    let cancel_btn = secondary_btn(cancel_label, cancel_msg, p);

    let delete_btn = delete_msg.map(|msg| {
        styled_btn(
            t_static(t!("dialog.delete")),
            Some(msg),
            p.danger,
            iced::Color::WHITE,
            p,
        )
    });

    if let Some(btn) = delete_btn {
        row![save_btn, cancel_btn, Space::new().width(Length::Fill), btn]
            .spacing(8)
            .into()
    } else {
        row![save_btn, cancel_btn].spacing(8).into()
    }
}

/// Read-only form value styled like [`crate::core::widgets::labeled_input`]
/// (same label treatment and width), but rendered as text instead of an
/// editable field. Used by detail panels that share the host form layout
/// without accepting input.
pub fn labeled_readonly<'a>(label: &str, value: String, p: &ThemePalette) -> Element<'a, Message> {
    let palette = *p;
    column![
        text(label.to_string())
            .size(12)
            .color(palette.text_sec)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
        text(value)
            .size(13)
            .color(palette.text_pri)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(4)
    .into()
}

/// Monospace variant of [`labeled_readonly`] for keys and fingerprints.
pub fn labeled_readonly_mono<'a>(
    label: &str,
    value: String,
    p: &ThemePalette,
) -> Element<'a, Message> {
    let palette = *p;
    column![
        text(label.to_string())
            .size(12)
            .color(palette.text_sec)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
        text(value)
            .size(11)
            .color(palette.text_sec)
            .font(Font::MONOSPACE)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(4)
    .into()
}
