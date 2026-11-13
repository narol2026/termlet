//! Shared button variants for toolbars, forms, and segmented controls.

// Rust guideline compliant 2026-10-09

use iced::widget::{button, row, text};
use iced::{Background, Border, Color, Element, Length, Theme, mouse};

use crate::core::message::Message;
use crate::core::theme::ThemePalette;

use super::common::LUCIDE_FONT;
use super::metrics::{CONTROL_HEIGHT, control_padding};

/// Accent primary button used for Save / Delete actions across all forms.
/// Sized to [`CONTROL_HEIGHT`] so the action row lines up with the fields and
/// dropdowns above it.
pub fn styled_btn<'a>(
    label: &str,
    on_press: Option<Message>,
    bg: Color,
    fg: Color,
    p: ThemePalette,
) -> Element<'a, Message> {
    styled_btn_with_padding(label, on_press, bg, fg, 14.0, 14.0, p)
}

/// Colored action button with a caller-chosen label size and horizontal
/// padding, always exactly [`CONTROL_HEIGHT`] tall.
///
/// The vertical padding is derived from `size` rather than passed in, so a
/// button can never drift away from the controls around it.
pub fn styled_btn_with_padding<'a>(
    label: &str,
    on_press: Option<Message>,
    bg: Color,
    fg: Color,
    size: f32,
    px: f32,
    p: ThemePalette,
) -> Element<'a, Message> {
    let disabled = on_press.is_none();

    let mut btn =
        button(
            text(label.to_string())
                .size(size)
                .color(if disabled { p.text_dim } else { fg }),
        )
        .padding(control_padding(size, px))
        .height(Length::Fixed(CONTROL_HEIGHT));
    btn = btn
        .style(move |_theme: &Theme, status| {
            if disabled {
                let disabled_bg = Color {
                    r: bg.r,
                    g: bg.g,
                    b: bg.b,
                    a: bg.a * 0.35,
                };
                return button::Style {
                    background: Some(Background::Color(disabled_bg)),
                    text_color: p.text_dim,
                    border: Border {
                        radius: 5.0.into(),
                        width: 1.0,
                        color: disabled_bg,
                    },
                    ..button::Style::default()
                };
            }

            let hovered = matches!(status, button::Status::Hovered);
            let light_bg = (bg.r + bg.g + bg.b) / 3.0 > 0.85;
            let bg_color = if hovered {
                if light_bg {
                    Color {
                        r: (bg.r - 0.04).max(0.0),
                        g: (bg.g - 0.04).max(0.0),
                        b: (bg.b - 0.04).max(0.0),
                        a: bg.a,
                    }
                } else {
                    Color {
                        r: (bg.r + 0.04).min(1.0),
                        g: (bg.g + 0.04).min(1.0),
                        b: (bg.b + 0.04).min(1.0),
                        a: bg.a,
                    }
                }
            } else {
                bg
            };
            button::Style {
                background: Some(Background::Color(bg_color)),
                text_color: if hovered && fg == p.text_sec {
                    p.text_pri
                } else {
                    fg
                },
                border: Border {
                    radius: 5.0.into(),
                    width: 1.0,
                    color: bg_color,
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }

    btn.into()
}

// Vaults toolbar buttons
//
// Hosts, Keychain, Known-Hosts, Snippets and Port-Forward all show the same
// three controls: an accent primary action, an outline secondary action and an
// icon-only view toggle. They are built here so every panel paints and measures
// them identically — all three are [`CONTROL_HEIGHT`] tall.

/// Icon + label row shared by the primary and outline toolbar buttons.
fn toolbar_btn_content(
    icon: lucide_icons::Icon,
    label: &str,
    color: Color,
) -> Element<'static, Message> {
    row![
        text(char::from(icon))
            .font(LUCIDE_FONT)
            .size(12)
            .color(color),
        text(label.to_string()).size(13).color(color),
    ]
    .spacing(4)
    .align_y(iced::Alignment::Center)
    .into()
}

/// Accent primary action of a toolbar ("New host", "Generate key", …).
pub fn toolbar_primary_btn<'a>(
    icon: lucide_icons::Icon,
    label: &str,
    on_press: Message,
    p: ThemePalette,
) -> Element<'a, Message> {
    let accent = p.accent;
    button(toolbar_btn_content(icon, label, Color::WHITE))
        .on_press(on_press)
        .padding(control_padding(13.0, 14.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    Color {
                        r: (accent.r + 0.04).min(1.0),
                        g: (accent.g + 0.04).min(1.0),
                        b: (accent.b + 0.04).min(1.0),
                        a: accent.a,
                    }
                } else {
                    accent
                })),
                text_color: Color::WHITE,
                border: Border {
                    radius: 5.0.into(),
                    ..Default::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}

/// Outline secondary action of a toolbar ("Terminal", "New identity", …).
pub fn toolbar_outline_btn<'a>(
    icon: lucide_icons::Icon,
    label: &str,
    on_press: Message,
    p: ThemePalette,
) -> Element<'a, Message> {
    button(toolbar_btn_content(icon, label, p.text_pri))
        .on_press(on_press)
        .padding(control_padding(13.0, 14.0))
        .style(move |_theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.button_hover
                } else {
                    p.button_bg
                })),
                text_color: p.text_pri,
                border: Border {
                    radius: 5.0.into(),
                    width: 1.0,
                    color: if hovered {
                        p.button_border_hover
                    } else {
                        p.button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}

/// Icon-only toolbar control, used for the list / grid view toggle.
pub fn toolbar_icon_btn<'a>(
    icon: lucide_icons::Icon,
    on_press: Message,
    p: ThemePalette,
) -> Element<'a, Message> {
    button(
        text(char::from(icon))
            .font(LUCIDE_FONT)
            .size(14)
            .color(p.text_sec),
    )
    .on_press(on_press)
    .padding(control_padding(14.0, 10.0))
    .style(move |_theme: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: Some(Background::Color(if hovered {
                p.button_hover
            } else {
                p.button_bg
            })),
            text_color: if hovered { p.text_pri } else { p.text_sec },
            border: Border {
                radius: 5.0.into(),
                width: 1.0,
                color: if hovered {
                    p.button_border_hover
                } else {
                    p.button_border
                },
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default())
    .into()
}

/// Selected / unselected pill for the small option rows inside forms, such as
/// the host form's "Auth type" or the port-forward "Forward kind".
pub fn segment_btn<'a>(
    label: impl Into<String>,
    active: bool,
    on_press: Message,
    p: ThemePalette,
) -> Element<'a, Message> {
    button(
        text(label.into())
            .size(12)
            .color(if active { Color::WHITE } else { p.text_sec }),
    )
    .on_press(on_press)
    .padding(control_padding(12.0, 12.0))
    .style(move |_: &Theme, _status| button::Style {
        background: Some(Background::Color(if active { p.accent } else { p.surface })),
        text_color: if active { Color::WHITE } else { p.text_sec },
        border: Border {
            radius: 4.0.into(),
            width: 1.0,
            color: if active { Color::TRANSPARENT } else { p.border },
        },
        ..button::Style::default()
    })
    .cursor_interaction(mouse::Interaction::default())
    .into()
}

/// Secondary/outline button used for Cancel actions across all forms.
/// Matches `styled_btn` ([`CONTROL_HEIGHT`], text 14, radius 5.0) so
/// Save / Cancel / Delete buttons sit side-by-side with identical dimensions.
pub fn secondary_btn<'a>(label: &str, on_press: Message, p: ThemePalette) -> Element<'a, Message> {
    button(text(label.to_string()).size(14))
        .on_press(on_press)
        .padding(control_padding(14.0, 14.0))
        .height(Length::Fixed(CONTROL_HEIGHT))
        .style(move |_theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.button_hover
                } else {
                    p.button_bg
                })),
                text_color: if hovered { p.text_pri } else { p.text_sec },
                border: Border {
                    radius: 5.0.into(),
                    width: 1.0,
                    color: if hovered {
                        p.button_border_hover
                    } else {
                        p.button_border
                    },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
}
