use iced::widget::{Space, button, column, container, row, text, text_editor};
use iced::{Background, Border, Color, Element, Length, Padding, Theme, mouse};

use crate::core::data::format_size;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::{control_padding, styled_scrollable};
use rust_i18n::t;

use crate::sftp::{SftpPreview, SftpPreviewState};

use super::{LUCIDE_FONT, SFTP_PREVIEW_SCROLL_ID, view_loading_overlay};

pub(super) fn view_preview_fullscreen(
    preview: &SftpPreview,
    p: ThemePalette,
    scrollbar_visible: bool,
) -> Element<'_, Message> {
    let close_btn = button(
        text(char::from(lucide_icons::Icon::X))
            .font(LUCIDE_FONT)
            .size(16)
            .color(p.text_sec),
    )
    .on_press(Message::Sftp(
        crate::sftp::message::Message::SftpPreviewClose,
    ))
    .padding(control_padding(16.0, 8.0))
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
    .cursor_interaction(mouse::Interaction::default());

    let file_icon = text(char::from(lucide_icons::Icon::FileText))
        .font(LUCIDE_FONT)
        .size(16)
        .color(p.accent);

    let file_name = text(&preview.file_name).size(15).color(p.text_pri);

    let info_label = match &preview.state {
        SftpPreviewState::Loading => t!("sftp.loading").to_string(),
        SftpPreviewState::Failed(_) => t!("sftp.error_label").to_string(),
        SftpPreviewState::Ready {
            total_bytes: Some(total),
            ..
        } => format!("{}  ·  {}", preview.location_label, format_size(*total)),
        SftpPreviewState::Ready { .. } => preview.location_label.clone(),
    };

    let top_bar = container(
        row![
            close_btn,
            Space::new().width(Length::Fixed(4.0)),
            file_icon,
            Space::new().width(Length::Fixed(2.0)),
            file_name,
            Space::new().width(Length::Fill),
            text(info_label)
                .size(13)
                .color(p.text_sec)
                .ellipsis(text::Ellipsis::End)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(0)
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding::from([6.0, 12.0]))
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(p.surface)),
        ..Default::default()
    });

    let top_bar_divider =
        container(Space::new())
            .width(Length::Fill)
            .height(1)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..Default::default()
            });

    let body: Element<'_, Message> = match &preview.state {
        SftpPreviewState::Loading => container(view_loading_overlay(p))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        SftpPreviewState::Failed(error) => container(
            column![
                text(char::from(lucide_icons::Icon::AlertTriangle))
                    .font(LUCIDE_FONT)
                    .size(26)
                    .color(p.danger),
                text(t_static(t!("sftp.cannot_preview")))
                    .size(16)
                    .color(p.text_pri),
                text(error.clone())
                    .size(14)
                    .color(p.text_sec)
                    .wrapping(text::Wrapping::WordOrGlyph),
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into(),
        SftpPreviewState::Ready { content, .. } => styled_scrollable(
            container(
                text_editor(content)
                    .size(14)
                    .on_action(|v0| {
                        Message::Sftp(crate::sftp::message::Message::SftpPreviewAction(v0))
                    })
                    .style(move |_: &Theme, _status| text_editor::Style {
                        background: Background::Color(Color::TRANSPARENT),
                        border: Border::default(),
                        placeholder: p.text_sec,
                        value: p.text_pri,
                        selection: p.accent,
                    }),
            )
            .padding(Padding::from([12.0, 16.0]))
            .width(Length::Fill),
            p,
            scrollbar_visible,
        )
        .id(iced::widget::Id::from(SFTP_PREVIEW_SCROLL_ID))
        .on_scroll(move |v| Message::Scrolled(SFTP_PREVIEW_SCROLL_ID, v))
        .height(Length::Fill)
        .into(),
    };

    let content = column![
        top_bar,
        top_bar_divider,
        container(body).width(Length::Fill).height(Length::Fill),
    ]
    .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.content_bg)),
            ..Default::default()
        })
        .into()
}
