//! Context-menu row widgets.

// Rust guideline compliant 2026-10-09

use iced::widget::{button, container, row, text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme, mouse};

use crate::core::message::Message;
use crate::core::theme::ThemePalette;

use super::common::LUCIDE_FONT;

/// Build a single menu item button for context menus.
///
/// Consists of a lucide icon + text label. When `destructive` is true,
/// both icon and label are rendered in the danger color and the hover
/// background uses a translucent danger tint. `focused` marks the row selected
/// by keyboard navigation.
pub fn menu_item(
    icon: lucide_icons::Icon,
    label: &str,
    msg: Message,
    destructive: bool,
    focused: bool,
    p: ThemePalette,
) -> Element<'static, Message> {
    let icon_color = if destructive { p.danger } else { p.text_sec };
    let text_color = if destructive { p.danger } else { p.text_pri };

    button(
        row![
            container(
                text(char::from(icon))
                    .font(LUCIDE_FONT)
                    .size(15)
                    .color(icon_color),
            )
            .width(18)
            .center_x(18),
            text(label.to_string()).size(14).color(text_color),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .on_press(msg)
    .width(Length::Fill)
    .padding(Padding::from([7.0, 10.0]))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let hover_bg = if destructive {
            Color {
                r: p.danger.r,
                g: p.danger.g,
                b: p.danger.b,
                a: 0.18,
            }
        } else {
            p.item_hover
        };
        button::Style {
            background: (hovered || focused).then_some(Background::Color(hover_bg)),
            text_color,
            border: Border {
                radius: 5.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default())
    .into()
}
