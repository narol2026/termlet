//! Reusable message-dialog shell.
//!
//! A message dialog is a centered card laid out as header / body / separator /
//! footer, the separator spanning the full card width rather than the padded
//! section width. The backdrop is a fixed semi-transparent black and the card
//! uses `p.card_bg`, a 1px border (radius 8) and a soft shadow; `anim` drives a
//! zoom-in entrance (0.0 = 80% scale, 1.0 = at rest), ticked by
//! [`crate::ui::helpers::tick_slide_anim`].
//!
//! [`message_dialog`] covers the common title / body / actions shape;
//! [`message_dialog_frame`] composes the shell with custom sections.

use iced::widget::{Space, column, container, mouse_area, row, stack, text};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Theme, mouse};

use crate::core::message::Message;
use crate::core::theme::ThemePalette;

/// Horizontal (and outer vertical) padding applied to each section inside the
/// card. The separator is not padded, so it runs edge-to-edge.
const SECTION_PADDING: f32 = 20.0;

/// Vertical gap between adjacent sections (header → body, body → footer).
/// Kept separate from [`SECTION_PADDING`] so sections don't stack two full
/// paddings against each other.
const SECTION_GAP: f32 = 12.0;

/// Backdrop + centered card shell: header / body / separator / footer.
pub fn message_dialog_frame<'a>(
    header: Element<'a, Message>,
    body: Element<'a, Message>,
    footer: Element<'a, Message>,
    width: f32,
    backdrop_on_press: Message,
    p: &ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let eased = crate::ui::helpers::ease_out_cubic(anim);

    let backdrop_color = Color::from_rgba(0.0, 0.0, 0.0, 0.15);
    let backdrop = mouse_area(
        container(Space::new().width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(backdrop_color)),
                ..Default::default()
            }),
    )
    .on_press(backdrop_on_press)
    .interaction(mouse::Interaction::Idle);

    // Separator spans the full card width (not inset by section padding).
    let separator_color = p.border;
    let separator = container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(
        move |_: &Theme| container::Style {
            background: Some(Background::Color(separator_color)),
            ..container::Style::default()
        },
    );

    let content_bg = p.card_bg;
    let border_color = p.border;
    let shadow_color = p.card_shadow;
    let card = container(column![
        container(header).padding(Padding {
            top: SECTION_PADDING,
            right: SECTION_PADDING,
            bottom: 0.0,
            left: SECTION_PADDING,
        }),
        container(body).padding(Padding {
            top: SECTION_GAP,
            right: SECTION_PADDING,
            bottom: SECTION_GAP,
            left: SECTION_PADDING,
        }),
        separator,
        container(footer).padding(Padding {
            top: SECTION_GAP,
            right: SECTION_PADDING,
            bottom: SECTION_PADDING,
            left: SECTION_PADDING,
        }),
    ])
    .width(Length::Fixed(width))
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(content_bg)),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: iced::Shadow {
            color: shadow_color,
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..Default::default()
    });

    // Absorb clicks on the card so they neither dismiss the dialog nor fall
    // through to the backdrop.
    let card = mouse_area(card)
        .on_press(Message::Noop)
        .interaction(mouse::Interaction::Idle);

    let centered = container(crate::ui::scale_wrapper::scale_wrapper(
        card.into(),
        0.8 + 0.2 * eased,
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Center)
    .align_y(iced::alignment::Vertical::Center);

    stack![backdrop, centered]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// A full message dialog: bold title, optional dim body line, rich content,
/// and right-aligned action buttons.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn message_dialog<'a>(
    title: impl Into<String>,
    body_text: Option<impl Into<String>>,
    content: Element<'a, Message>,
    actions: Vec<Element<'a, Message>>,
    backdrop_on_press: Message,
    width: f32,
    p: &ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    message_dialog_with_leading_actions(
        title,
        body_text,
        content,
        Vec::new(),
        actions,
        backdrop_on_press,
        width,
        p,
        anim,
    )
}

/// Like [`message_dialog`], but lets the caller pin some actions to the left
/// edge of the footer. The remaining actions stay right-aligned, matching the
/// usual primary/secondary action grouping.
#[allow(
    clippy::too_many_arguments,
    reason = "the parameter list mirrors the layout this widget builds"
)]
pub fn message_dialog_with_leading_actions<'a>(
    title: impl Into<String>,
    body_text: Option<impl Into<String>>,
    content: Element<'a, Message>,
    leading_actions: Vec<Element<'a, Message>>,
    trailing_actions: Vec<Element<'a, Message>>,
    backdrop_on_press: Message,
    width: f32,
    p: &ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let header = text(title.into())
        .size(15)
        .color(p.text_pri)
        .width(Length::Fill)
        .ellipsis(iced::widget::text::Ellipsis::End)
        .into();

    let mut body_col = column![].spacing(10).width(Length::Fill);
    if let Some(body) = body_text {
        body_col = body_col.push(
            text(body.into())
                .size(13)
                .color(p.text_dim)
                .width(Length::Fill)
                .wrapping(text::Wrapping::WordOrGlyph),
        );
    }
    body_col = body_col.push(content);
    let body: Element<'a, Message> = body_col.into();

    let mut footer = row![].spacing(8).align_y(Alignment::Center);
    for action in leading_actions {
        footer = footer.push(action);
    }
    footer = footer.push(Space::new().width(Length::Fill));
    for action in trailing_actions {
        footer = footer.push(action);
    }
    let footer: Element<'a, Message> = footer.into();

    message_dialog_frame(header, body, footer, width, backdrop_on_press, p, anim)
}
