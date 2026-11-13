//! Shared scrollable styling.

// Rust guideline compliant 2026-10-09

use std::cell::Cell;

use iced::widget::scrollable;
use iced::{Background, Border, Element};

use crate::core::theme::ThemePalette;

use super::common::with_alpha;

thread_local! {
    pub static SCROLLBAR_HOVERED: Cell<bool> = const { Cell::new(false) };
}

pub(super) const SCROLLBAR_MARGIN: f32 = 2.0;
const SCROLLBAR_RADIUS: f32 = 3.0;
const SCROLLBAR_WIDTH: f32 = 6.0;

pub fn styled_scrollable<'a, M: 'a>(
    content: impl Into<Element<'a, M>>,
    p: ThemePalette,
    scrollbar_visible: bool,
) -> scrollable::Scrollable<'a, M> {
    let bar = if scrollbar_visible {
        scrollable::Scrollbar::new()
            .width(SCROLLBAR_WIDTH)
            .scroller_width(SCROLLBAR_WIDTH)
            .margin(SCROLLBAR_MARGIN)
    } else {
        scrollable::Scrollbar::hidden()
    };
    scrollable(content)
        .direction(scrollable::Direction::Vertical(bar))
        .style(move |_, status| scrollable_style(status, p))
}

pub fn scrollable_style(status: scrollable::Status, p: ThemePalette) -> scrollable::Style {
    let hovered = matches!(
        status,
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        }
    );
    let dragged = matches!(
        status,
        scrollable::Status::Dragged {
            is_vertical_scrollbar_dragged: true,
            ..
        }
    );

    if hovered || dragged {
        SCROLLBAR_HOVERED.set(true);
    }

    let track_color = with_alpha(p.text_dim, if hovered || dragged { 0.28 } else { 0.18 });
    let thumb_color = if dragged {
        with_alpha(p.text_pri, 0.82)
    } else if hovered {
        with_alpha(p.text_sec, 0.72)
    } else {
        with_alpha(p.text_dim, 0.64)
    };

    let rail = scrollable::Rail {
        background: Some(Background::Color(track_color)),
        border: Border {
            radius: SCROLLBAR_RADIUS.into(),
            ..Border::default()
        },
        scroller: scrollable::Scroller {
            background: Background::Color(thumb_color),
            border: Border {
                radius: SCROLLBAR_RADIUS.into(),
                ..Border::default()
            },
        },
    };

    scrollable::Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(p.surface),
            border: Border {
                radius: 999.0.into(),
                width: 1.0,
                color: p.border,
            },
            shadow: iced::Shadow::default(),
            icon: p.text_sec,
        },
    }
}
