//! Per-pane split controls.

// Rust guideline compliant 2026-10-09

use iced::widget::pane_grid;
use iced::widget::{button, row, text};
use iced::{Background, Border, Element, Theme, mouse};

use crate::core::i18n::t_static;
use crate::core::ids::SplitId;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::{TooltipPosition, cursor_tooltip_label_at};
use crate::views::LUCIDE_FONT;
use rust_i18n::t;

pub(super) fn view_pane_controls<'a>(
    pane: pane_grid::Pane,
    total_panes: usize,
    split_id: SplitId,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    let btn_style = move |_: &Theme, status: button::Status| {
        let bg = match status {
            button::Status::Hovered => Some(Background::Color(p.item_hover)),
            _ => None,
        };
        button::Style {
            background: bg,
            border: Border {
                radius: 3.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    };

    let mut expand = button(
        text(char::from(lucide_icons::Icon::Expand))
            .font(LUCIDE_FONT)
            .size(11)
            .color(p.text_dim),
    )
    .padding(iced::Padding::from([2, 4]))
    .style(btn_style)
    .cursor_interaction(mouse::Interaction::default());

    let mut close = button(
        text(char::from(lucide_icons::Icon::X))
            .font(LUCIDE_FONT)
            .size(11)
            .color(p.text_dim),
    )
    .padding(iced::Padding::from([2, 4]))
    .style(btn_style)
    .cursor_interaction(mouse::Interaction::default());

    if total_panes > 1 {
        expand = expand.on_press(Message::Terminal(
            crate::terminal::message::Message::SplitUnsplit(split_id, pane),
        ));
        close = close.on_press(Message::Terminal(
            crate::terminal::message::Message::SplitClose(split_id, pane),
        ));
    }

    let expand: Element<'_, Message> = cursor_tooltip_label_at(
        expand,
        t_static(t!("terminal.split_restore_tooltip")),
        p,
        TooltipPosition::Bottom,
        None,
    )
    .into();
    let close: Element<'_, Message> = cursor_tooltip_label_at(
        close,
        t_static(t!("terminal.split_close_tooltip")),
        p,
        TooltipPosition::Bottom,
        None,
    )
    .into();

    row![expand, close]
        .spacing(2)
        .align_y(iced::Alignment::Center)
        .into()
}
