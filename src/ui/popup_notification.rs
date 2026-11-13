//! Reusable bottom-right popup notification building blocks.
//!
//! Mirrors the `confirm_dialog` approach: [`bottom_right_popup_frame`] is the
//! slide-in card shell, [`popup_header`] the icon + title + dismiss row and
//! [`popup_action_button`] the accent action, so every lightweight notification
//! composes the pattern instead of copying it. Each block takes the raw `anim`
//! (0..1, ticked by [`crate::ui::helpers::tick_slide_anim`]) and eases it
//! internally, so callers pass un-eased progress.

use iced::widget::{Space, button, container, mouse_area, row, text};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};

use crate::core::message::Message;
use crate::core::theme::ThemePalette;
use crate::core::widgets::control_padding;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

/// Width of the clipped slide-in container.  Must comfortably fit the card
/// plus its internal padding; the clip animates 0 → this width to reveal the
/// card from the right edge.
const FRAME_WIDTH: f32 = 380.0;

/// The bottom-right popup card shell: sidebar background, rounded border,
/// soft shadow, and a slide-from-the-right entrance driven by `anim`.
///
/// `anim` is the raw tick progress (0 = hidden, 1 = fully visible).  Pass the
/// card's inner content (header / body / action rows) as `content`.
pub fn bottom_right_popup_frame<'a>(
    content: Element<'a, Message>,
    p: &ThemePalette,
    anim: f32,
) -> Element<'a, Message> {
    let eased = crate::ui::helpers::ease_out_cubic(anim);
    let bg = p.sidebar_bg;
    let border = p.border;

    let card = mouse_area(
        container(content)
            .padding(Padding::from([14.0, 18.0]))
            .max_width(360)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.98 * eased,
                    ..bg
                })),
                border: Border {
                    radius: 10.0.into(),
                    width: 1.0,
                    color: border,
                },
                shadow: iced::Shadow {
                    color: Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.35 * eased,
                    },
                    offset: iced::Vector::new(0.0, 4.0),
                    blur_radius: 16.0,
                },
                ..Default::default()
            }),
    )
    .interaction(mouse::Interaction::Idle);

    // Slide-from-right entrance/exit: the card keeps its intrinsic width while
    // the outer container animates its clip width, so text never reflows as
    // the card collapses. This mirrors the host-form right-side panel.
    let card = crate::ui::slide::fixed_width_wrapper(card.into());

    // At anim=0 the clip width is 0 (hidden); as anim→1 it grows to
    // FRAME_WIDTH, revealing the card from the right edge. Closing runs the
    // same animation in reverse.
    let animated_width = FRAME_WIDTH * eased;
    let card = container(card)
        .width(Length::Fixed(animated_width))
        .clip(true);

    container(
        iced::widget::column![
            iced::widget::Space::new()
                .width(Length::Fill)
                .height(Length::Fill),
            container(card).padding(Padding {
                top: 0.0,
                right: 0.0,
                bottom: 16.0,
                left: 0.0,
            }),
        ]
        .align_x(Alignment::End)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Standard popup header row: lucide icon + title, a spacer pushing the
/// dismiss button to the right edge.
pub fn popup_header(
    icon: lucide_icons::Icon,
    icon_color: Color,
    title: &str,
    dismiss_msg: Message,
    p: &ThemePalette,
    anim: f32,
) -> Element<'static, Message> {
    let eased = crate::ui::helpers::ease_out_cubic(anim);
    let text_sec = p.text_sec;
    let button_hover = p.button_hover;

    row![
        text(char::from(icon))
            .font(LUCIDE_FONT)
            .size(14)
            .color(Color {
                a: eased,
                ..icon_color
            }),
        text(title.to_string()).size(12).color(Color {
            a: eased,
            ..text_sec
        }),
        Space::new().width(Length::Fill).height(Length::Fixed(1.0)),
        button(
            text(char::from(lucide_icons::Icon::X))
                .font(LUCIDE_FONT)
                .size(12)
                .color(Color {
                    a: eased,
                    ..text_sec
                }),
        )
        .on_press(dismiss_msg)
        .padding(Padding::from([4.0, 6.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: hovered.then_some(Background::Color(Color {
                    a: eased,
                    ..button_hover
                })),
                text_color: Color {
                    a: eased,
                    ..text_sec
                },
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default()),
    ]
    .align_y(Alignment::Center)
    .spacing(6)
    .into()
}

/// Accent-colored action button for a popup notification (e.g. "Enter
/// directory").  `icon` is optional — pass `None` for a text-only button.
/// Sized to [`crate::core::widgets::CONTROL_HEIGHT`] like every other control.
pub fn popup_action_button(
    icon: Option<lucide_icons::Icon>,
    label: &str,
    msg: Message,
    p: &ThemePalette,
    anim: f32,
) -> Element<'static, Message> {
    let eased = crate::ui::helpers::ease_out_cubic(anim);
    let surface = p.surface;
    let accent = p.accent;

    let mut children: Vec<Element<'static, Message>> = Vec::with_capacity(2);
    if let Some(ic) = icon {
        children.push(
            text(char::from(ic))
                .font(LUCIDE_FONT)
                .size(13)
                .color(Color {
                    a: eased,
                    ..surface
                })
                .into(),
        );
    }
    children.push(
        text(label.to_string())
            .size(13)
            .color(Color {
                a: eased,
                ..surface
            })
            .into(),
    );
    let content = iced::widget::Row::with_children(children)
        .spacing(6)
        .align_y(Alignment::Center);

    button(content)
        .on_press(msg)
        .padding(control_padding(13.0, 14.0))
        .style(move |_: &Theme, status| {
            let base_color = Color { a: eased, ..accent };
            let hover_color = Color {
                a: (eased * 0.9),
                ..accent
            };
            button::Style {
                background: Some(Background::Color(match status {
                    button::Status::Hovered => hover_color,
                    _ => base_color,
                })),
                text_color: Color {
                    a: eased,
                    ..surface
                },
                border: Border {
                    radius: 6.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}
