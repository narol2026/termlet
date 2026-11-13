//! Collection card, tag, and icon-box widgets.

// Rust guideline compliant 2026-10-09

use iced::widget::{Space, button, column, container, mouse_area, row, text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme, mouse};

use crate::core::data::tag_color;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;

use super::common::LUCIDE_FONT;
use super::dropdown::SelectOption;

pub fn icon_box(icon: Element<'_, Message>, bg: Color) -> Element<'_, Message> {
    container(icon)
        .width(44)
        .height(44)
        .center(44)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: 10.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// A small filled circle in the tag's deterministic color.
fn tag_dot(color: Color) -> Element<'static, Message> {
    tag_dot_sized(color, 7.0)
}

/// Like [`tag_dot`] but with an explicit diameter.
fn tag_dot_sized(color: Color, size: f32) -> Element<'static, Message> {
    container(Space::new())
        .width(size)
        .height(size)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: 999.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

pub fn tag_chip(label: String, on_remove: Message, p: ThemePalette) -> Element<'static, Message> {
    // Cap a single tag's text so an absurdly long name truncates with "…"
    // instead of overflowing the form sidebar; short tags keep natural width.
    const MAX_LABEL_WIDTH: f32 = 200.0;

    button(
        row![
            tag_dot_sized(tag_color(&label), 8.0),
            container(
                text(label)
                    .size(12)
                    .color(p.text_sec)
                    .wrapping(text::Wrapping::None)
                    .ellipsis(text::Ellipsis::End),
            )
            .max_width(MAX_LABEL_WIDTH),
            text(char::from(lucide_icons::Icon::X))
                .font(LUCIDE_FONT)
                .size(11)
                .color(p.text_dim),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .on_press(on_remove)
    .padding(Padding::from([5.0, 9.0]))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                p.item_hover
            } else {
                p.surface
            })),
            text_color: p.text_pri,
            border: Border {
                radius: 4.0.into(),
                color: p.border,
                width: 1.0,
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::Pointer)
    .into()
}

/// A dropdown option for a tag: a color-coded dot plus the tag name. Shared by
/// the host-list tag filter and the host-form tag picker so tags render the
/// same everywhere.
pub fn tag_select_option(label: &str, active: bool, on_select: Message) -> SelectOption {
    SelectOption {
        label: label.to_string(),
        subtitle: None,
        active,
        focused: false,
        on_select,
        leading: Some(tag_dot(tag_color(label))),
    }
}

pub fn card_info(
    line1: &str,
    line2: String,
    card_width: f32,
    has_action: bool,
    p: ThemePalette,
) -> Element<'static, Message> {
    let max_width = card_width - 92.0 - if has_action { 28.0 } else { 0.0 };
    // Normalize newlines to spaces so multi-line descriptions don't break card layout
    let line1 = line1.replace('\n', " ");
    column![
        text(line1)
            .size(14.0)
            .color(p.text_pri)
            .width(Length::Fixed(max_width))
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
        text(line2)
            .size(11.0)
            .color(p.text_sec)
            .width(Length::Fixed(max_width))
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(2)
    .width(Length::Fill)
    .into()
}

pub fn card_hover_btn(
    icon: lucide_icons::Icon,
    on_press: Message,
    p: ThemePalette,
) -> Element<'static, Message> {
    button(text(char::from(icon)).font(LUCIDE_FONT).size(14))
        .on_press(on_press)
        .padding(Padding::from([4.0, 6.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            let btn_hover = Color {
                r: (p.card_hover.r + 0.06).min(1.0),
                g: (p.card_hover.g + 0.06).min(1.0),
                b: (p.card_hover.b + 0.06).min(1.0),
                a: 1.0,
            };
            button::Style {
                background: Some(Background::Color(if hovered {
                    btn_hover
                } else {
                    Color::TRANSPARENT
                })),
                text_color: if hovered { p.accent } else { p.text_sec },
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn card_wrapper(
    content: Element<'_, Message>,
    hovered: bool,
    selected: bool,
    border_accent: Option<Color>,
    active_bg: Option<Color>,
    border_radius: f32,
    hover_bg: bool,
    on_enter: Message,
    on_exit: Message,
    on_press: Option<Message>,
    on_double_click: Option<Message>,
    on_right_press: Option<Message>,
    p: ThemePalette,
) -> Element<'_, Message> {
    card_wrapper_with_padding(
        content,
        hovered,
        selected,
        border_accent,
        active_bg,
        border_radius,
        hover_bg,
        on_enter,
        on_exit,
        on_press,
        on_double_click,
        on_right_press,
        Padding::from([5.0, 10.0]),
        p,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn card_wrapper_with_padding(
    content: Element<'_, Message>,
    hovered: bool,
    selected: bool,
    border_accent: Option<Color>,
    active_bg: Option<Color>,
    border_radius: f32,
    hover_bg: bool,
    on_enter: Message,
    on_exit: Message,
    on_press: Option<Message>,
    on_double_click: Option<Message>,
    on_right_press: Option<Message>,
    padding: Padding,
    p: ThemePalette,
) -> Element<'_, Message> {
    let card_button = button(content)
        .padding(padding)
        .width(Length::Fill)
        .style(move |_theme: &Theme, status| {
            let pressed = matches!(status, button::Status::Pressed);
            let (bg, border_color, border_width) = card_colors(
                pressed,
                hovered,
                selected,
                hover_bg,
                active_bg,
                border_accent,
                p,
            );

            button::Style {
                background: Some(Background::Color(bg)),
                text_color: p.text_pri,
                border: Border {
                    color: border_color,
                    width: border_width,
                    radius: border_radius.into(),
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    let mut area = mouse_area(card_button).on_enter(on_enter).on_exit(on_exit);

    if let Some(msg) = on_press {
        area = area.on_press(msg);
    }
    if let Some(msg) = on_double_click {
        area = area.on_double_click(msg);
    }
    if let Some(msg) = on_right_press {
        area = area.on_right_press(msg);
    }

    area.into()
}

/// Selected cards reuse the hover fill and add an accent border; on themes
/// without card borders that outline is what keeps the selection legible.
fn card_colors(
    pressed: bool,
    hovered: bool,
    selected: bool,
    hover_bg: bool,
    active_bg: Option<Color>,
    border_accent: Option<Color>,
    p: ThemePalette,
) -> (Color, Color, f32) {
    let bg = if pressed || (hovered && hover_bg) || selected {
        p.card_hover
    } else if let Some(active_bg) = active_bg {
        active_bg
    } else {
        p.card_bg
    };

    let (border_color, border_width) = if let Some(accent) = border_accent {
        (accent, 1.0)
    } else if selected {
        (p.accent, 1.0)
    } else if p.show_border {
        (p.border, 1.0)
    } else {
        (Color::TRANSPARENT, 0.0)
    };

    (bg, border_color, border_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_cards_reuse_the_hover_fill_and_add_a_border() {
        let p = crate::core::theme::palette(crate::core::theme::ThemeMode::Dark);

        let (bg, border, width) = card_colors(false, false, true, true, None, None, p);
        assert_eq!(bg, p.card_hover);
        assert_eq!(border, p.accent);
        assert_eq!(width, 1.0);

        let (list_bg, _, _) = card_colors(false, false, true, false, None, None, p);
        assert_eq!(
            list_bg, p.card_hover,
            "selected rows use the same fill even when hover does not"
        );
    }
}
