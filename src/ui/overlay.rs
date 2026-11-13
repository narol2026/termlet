use iced::widget::{Space, container, mouse_area, stack};
use iced::{Background, Border, Element, Length, Padding, Point, Theme, mouse};

use crate::core::message::Message;
use crate::core::theme::ThemePalette;

const EDGE_PADDING: f32 = 6.0;

/// Build a context menu overlay frame.
///
/// Takes a closure that builds the menu items on each invocation, then wraps
/// them in a positioned card with a transparent backdrop that dismisses on
/// click. Edge-aware positioning flips the menu left/up when it would
/// overflow the window bounds.
pub fn context_menu_frame<'a, F>(
    cursor: Point,
    build_items: F,
    menu_width: f32,
    close_msg: Message,
    p: ThemePalette,
) -> Element<'a, Message>
where
    F: Fn() -> Vec<Element<'a, Message>> + 'a,
{
    iced::widget::responsive(move |size| {
        let items = build_items();
        let item_count = items.len() as f32;
        let menu_height = item_count * 30.0 + 10.0; // 30px per item + chrome

        let backdrop = mouse_area(
            container(Space::new().width(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(close_msg.clone())
        .on_right_press(close_msg.clone())
        .interaction(mouse::Interaction::Idle);

        let menu_card = container(iced::widget::Column::with_children(items).spacing(2))
            .padding(Padding::from([4.0, 4.0]))
            .width(Length::Fixed(menu_width))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.card_bg)),
                border: Border {
                    color: p.border,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow {
                    color: p.menu_shadow,
                    offset: iced::Vector::new(0.0, 6.0),
                    blur_radius: 20.0,
                },
                ..Default::default()
            });

        let max_x = (size.width - menu_width - EDGE_PADDING).max(0.0);
        let max_y = (size.height - menu_height - EDGE_PADDING).max(0.0);
        let x = if cursor.x + menu_width + EDGE_PADDING > size.width {
            (cursor.x - menu_width).max(EDGE_PADDING)
        } else {
            cursor.x.max(EDGE_PADDING)
        }
        .min(max_x);
        let y = if cursor.y + menu_height + EDGE_PADDING > size.height {
            (cursor.y - menu_height).max(EDGE_PADDING)
        } else {
            cursor.y.max(EDGE_PADDING)
        }
        .min(max_y);

        let anchor = container(menu_card).padding(Padding {
            top: y,
            left: x,
            right: 0.0,
            bottom: 0.0,
        });

        stack![backdrop, anchor].into()
    })
    .into()
}
