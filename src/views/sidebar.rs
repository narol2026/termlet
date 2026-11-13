use iced::widget::{Space, button, column, container, row, text};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};

use crate::app::App;
use crate::core::data::NavItem;
use crate::core::diagnostics::Level;
use crate::core::message::Message;
use crate::core::theme::{self};

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_sidebar(app: &App) -> Element<'_, Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "VIEW",
        "view_sidebar",
        std::time::Duration::from_millis(16),
    );
    let p = theme::palette(app.effective_theme_mode());

    let build_nav_button = |item: NavItem, p: theme::ThemePalette| {
        let active = app.active_nav == item;
        let icon = container(
            text(char::from(item.icon()))
                .font(LUCIDE_FONT)
                .size(14)
                .color(if active { p.text_pri } else { p.text_sec }),
        )
        .width(18)
        .center_x(18);
        let label_row = row![
            icon,
            text(item.label())
                .size(14)
                .color(if active { p.text_pri } else { p.text_sec })
                .width(Length::Fill)
                .ellipsis(text::Ellipsis::End)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center);

        button(
            container(label_row)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Hosts(crate::hosts::message::Message::Nav(item)))
        .cursor_interaction(mouse::Interaction::default())
        .width(Length::Fill)
        .height(Length::Fixed(crate::views::ITEM_HEIGHT))
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 12.0,
        })
        .style(move |_theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: if active {
                    Some(Background::Color(p.nav_active))
                } else if hovered {
                    Some(Background::Color(Color {
                        a: 0.5,
                        ..p.nav_active
                    }))
                } else {
                    None
                },
                text_color: if active || hovered {
                    p.text_pri
                } else {
                    p.text_sec
                },
                border: Border {
                    radius: 8.0.into(),
                    width: if p.show_border && (active || hovered) {
                        1.0
                    } else {
                        0.0
                    },
                    color: p.border,
                },
                ..button::Style::default()
            }
        })
        .into()
    };

    let main_items = NavItem::all().iter().map(|&item| build_nav_button(item, p));
    let main_nav = column(main_items).spacing(2);

    let nav = main_nav;

    let sidebar_content = column![nav, Space::new().height(Length::Fill)];

    let divider =
        container(column![])
            .width(1)
            .height(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..container::Style::default()
            });

    container(row![
        container(sidebar_content)
            .width(192)
            .padding(Padding {
                top: 12.0,
                right: 12.0,
                bottom: 0.0,
                left: 12.0,
            })
            .height(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(p.sidebar_bg)),
                ..Default::default()
            }),
        divider,
    ])
    .height(Length::Fill)
    .into()
}
