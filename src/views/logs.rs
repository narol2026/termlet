use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};

use crate::app::App;
use crate::core::data::LogCategory;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::time::format_timestamp;
use crate::core::widgets::styled_scrollable;
use rust_i18n::t;

pub const LOGS_SCROLL_ID: &str = "logs_scroll";

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

fn category_badge_color(cat: LogCategory) -> Color {
    match cat {
        LogCategory::Host => Color {
            r: 0.380,
            g: 0.620,
            b: 0.980,
            a: 1.0,
        },
        LogCategory::Terminal => Color {
            r: 0.220,
            g: 0.820,
            b: 0.510,
            a: 1.0,
        },
        LogCategory::Sftp => Color {
            r: 0.980,
            g: 0.710,
            b: 0.220,
            a: 1.0,
        },
        LogCategory::PortForward => Color {
            r: 0.710,
            g: 0.450,
            b: 0.980,
            a: 1.0,
        },
        LogCategory::Snippet => Color {
            r: 0.980,
            g: 0.450,
            b: 0.620,
            a: 1.0,
        },
        LogCategory::KnownHost => Color {
            r: 0.420,
            g: 0.820,
            b: 0.910,
            a: 1.0,
        },
        LogCategory::Keychain => Color {
            r: 0.620,
            g: 0.820,
            b: 0.510,
            a: 1.0,
        },
        LogCategory::System => Color {
            r: 0.439,
            g: 0.502,
            b: 0.565,
            a: 1.0,
        },
    }
}

pub fn view_logs(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let copy_btn: Element<'_, Message> = if app.logs.is_empty() {
        Space::new().width(0).height(0).into()
    } else {
        button(
            text(char::from(lucide_icons::Icon::Copy))
                .font(LUCIDE_FONT)
                .size(14)
                .color(p.text_sec),
        )
        .on_press(Message::CopyLogs)
        .padding(Padding::from([4.0, 8.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.item_hover
                } else {
                    Color::TRANSPARENT
                })),
                text_color: if hovered { p.text_pri } else { p.text_sec },
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
    };

    let header = container(
        row![
            text(t_static(t!("logs.title"))).size(16).color(p.text_pri),
            Space::new().width(Length::Fill),
            copy_btn,
            text(t!("logs.entries", count = app.logs.len().to_string()))
                .size(12)
                .color(p.text_dim),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([16.0, 20.0]))
    .width(Length::Fill);

    let body: Element<'_, Message> = if app.logs.is_empty() {
        let icon = text(char::from(lucide_icons::Icon::ScrollText))
            .font(LUCIDE_FONT)
            .size(40)
            .color(p.text_dim);

        let label = text(t_static(t!("logs.empty"))).size(16).color(p.text_sec);

        let hint = text(t_static(t!("logs.empty_hint")))
            .size(12)
            .color(p.text_dim);

        container(
            column![icon, label, hint]
                .spacing(8)
                .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    } else {
        let mut log_col = column![].spacing(0);
        for (i, entry) in app.logs.iter().rev().enumerate() {
            let badge_color = category_badge_color(entry.category);

            let badge = container(text(entry.category.label()).size(12).color(Color::WHITE))
                .padding(Padding::from([2.0, 8.0]))
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(badge_color)),
                    border: Border {
                        radius: 4.0.into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                });

            let time_text = text(format_timestamp(entry.timestamp))
                .size(11)
                .color(p.text_dim)
                .font(Font::MONOSPACE);

            let msg_text = text(&entry.message)
                .size(13)
                .color(p.text_pri)
                .width(Length::Fill)
                .ellipsis(text::Ellipsis::End)
                .wrapping(text::Wrapping::None);

            let bg_color = if i % 2 == 0 {
                p.content_bg
            } else {
                p.stripe_bg
            };

            log_col = log_col.push(
                container(
                    row![
                        time_text,
                        Space::new().width(12),
                        badge,
                        Space::new().width(10),
                        msg_text,
                    ]
                    .spacing(0)
                    .align_y(Alignment::Start),
                )
                .width(Length::Fill)
                .padding(Padding::from([8.0, 20.0]))
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(bg_color)),
                    ..container::Style::default()
                }),
            );
        }
        styled_scrollable(log_col, p, app.is_scrollbar_visible(LOGS_SCROLL_ID))
            .id(iced::widget::Id::from(LOGS_SCROLL_ID))
            .on_scroll(move |vp| Message::Scrolled(LOGS_SCROLL_ID, vp))
            .into()
    };

    let page = column![header, body]
        .spacing(0)
        .width(Length::Fill)
        .height(Length::Fill);

    container(page)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
