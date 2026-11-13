//! Theme palette preview icon.

// Rust guideline compliant 2026-10-09

use iced::widget::{column, container, row};
use iced::{Background, Color, Element, Length, Theme};

use crate::core::message::Message;
use crate::core::theme::ThemeMode;

/// Four-color quadrant preview icon showing a theme's color scheme.
/// For [`ThemeMode::Auto`] the left half uses the dark palette and the
/// right half uses the light palette, so the user can see both at a glance.
pub fn theme_preview_icon<'a>(mode: ThemeMode) -> Element<'a, Message> {
    fn quadrant<'a>(color: Color, size: f32) -> Element<'a, Message> {
        container(row![])
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(color)),
                ..container::Style::default()
            })
            .into()
    }
    let size = 12.0;
    let gap = 1.0;
    let half = (size - gap) / 2.0;

    if mode == ThemeMode::Auto {
        // Split preview: left column = dark, right column = light.
        let dp = crate::core::theme::dark_palette();
        let lp = crate::core::theme::light_palette();
        let top_row =
            row![quadrant(dp.sidebar_bg, half), quadrant(lp.sidebar_bg, half),].spacing(gap);
        let bottom_row = row![quadrant(dp.accent, half), quadrant(lp.accent, half),].spacing(gap);
        return container(column![top_row, bottom_row].spacing(gap))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into();
    }

    let p = crate::core::theme::palette(mode);

    // 2×2 grid: sidebar_bg | text_pri
    //           accent     | terminal_bg
    let top_row = row![quadrant(p.sidebar_bg, half), quadrant(p.text_pri, half),].spacing(gap);
    let bottom_row = row![quadrant(p.accent, half), quadrant(p.terminal_bg, half),].spacing(gap);

    container(column![top_row, bottom_row].spacing(gap))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}
