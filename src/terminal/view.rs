// Rust guideline compliant 2026-10-09

use iced::widget::{
    Id, Space, button, center, column, container, mouse_area, responsive, row, stack, text,
    text_input,
};
use iced::{Background, Border, Color, Element, Length, Padding, Theme, Vector, mouse};
use iced_term::TerminalView;

use crate::app::App;
use crate::core::data::OsIcon;
use crate::core::diagnostics::Level;
use crate::core::i18n::t_static;
use crate::core::ids::SessionId;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::input_style_with_palette;
use crate::terminal::{TerminalSession, state::disconnect_overlay_visible};
use crate::ui::confirm_dialog::{cancel_button_with_label, confirm_button, danger_outline_button};
use crate::ui::message_dialog;
use crate::ui::slide::{slide_panel_from_right, slide_wrapper};
use rust_i18n::t;

use crate::views::right_sidebar::{RIGHT_SIDEBAR_WIDTH, view_right_sidebar};
use crate::views::{LUCIDE_FONT, view_os_icon};

mod controls;
mod divider;
mod preview;
mod split;

use controls::view_pane_controls;
use divider::PaneDividerDots;
use preview::split_preview_wrapper;
pub use split::view_split_terminal;

pub(super) const TERMINAL_PADDING: u16 = 8;
/// Small inset keeps the scrollbar legible without pulling it off the pane edge.
pub(super) const SPLIT_TERMINAL_RIGHT_PADDING: f32 = 4.0;
/// Gap between the disconnect notice and the terminal container on every side.
const DISCONNECT_BANNER_MARGIN: f32 = 6.0;
/// Breathing room between the disconnect notice content and its rounded background.
const DISCONNECT_BANNER_PADDING: f32 = 8.0;
/// Matches the rounded terminal overlays while keeping the compact banner soft.
const DISCONNECT_BANNER_RADIUS: f32 = 8.0;

pub const TERMINAL_DROP_ZONE_ID: &str = "terminal_drop_zone";

/// Thin 1px horizontal border used above terminal and split content.
pub(super) fn horizontal_border(p: theme::ThemePalette) -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fixed(1.0)))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..container::Style::default()
        })
        .into()
}

/// Optional diagnostic watermark shown at the bottom of a terminal once OSC 7
/// or OSC 633 shell-integration markers have been observed.
pub(super) fn osc_watermark_overlay(
    enabled: bool,
    p: theme::ThemePalette,
) -> Element<'static, Message> {
    if !enabled {
        return Space::new().width(0).height(0).into();
    }

    let watermark_color = Color {
        a: 0.3,
        ..p.text_dim
    };
    let watermark = row![
        text("OSC7").size(11).color(watermark_color),
        text("·").size(11).color(watermark_color),
        text("OSC633").size(11).color(watermark_color),
    ]
    .spacing(6);

    container(watermark)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(Padding {
            top: 0.0,
            right: f32::from(TERMINAL_PADDING) + 6.0,
            bottom: f32::from(TERMINAL_PADDING),
            left: 0.0,
        })
        .into()
}

/// Render a standalone terminal tab (a `TabKind::Session`). A session is never
/// simultaneously a standalone tab and a split pane; split tabs are rendered by
/// [`view_split_terminal`].
pub fn view_terminal(app: &App, session_id: SessionId) -> Element<'_, Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "VIEW",
        "view_terminal",
        std::time::Duration::from_millis(16),
    );
    let p = theme::palette(app.effective_theme_mode());

    let top = horizontal_border(p);

    let Some(session) = app.terminal.sessions.iter().find(|s| s.id == session_id) else {
        return column![
            top,
            container(center(
                text(t_static(t!("terminal.session_not_found")))
                    .color(p.text_sec)
                    .size(14)
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.content_bg)),
                ..Default::default()
            }),
        ]
        .into();
    };

    let terminal: Element<'_, Message> = stack![
        container(
            TerminalView::show(&session.terminal)
                .map(|v0| Message::Terminal(crate::terminal::message::Message::IcedTermEvent(v0)))
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(iced::Padding {
            top: f32::from(TERMINAL_PADDING),
            right: 0.0,
            bottom: f32::from(TERMINAL_PADDING),
            left: f32::from(TERMINAL_PADDING),
        })
        .id(iced::widget::Id::from(TERMINAL_DROP_ZONE_ID))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.terminal_bg)),
            ..Default::default()
        }),
        osc_watermark_overlay(
            app.preferences.osc_watermark_enabled && session.osc_integration_detected,
            p,
        ),
    ]
    .into();

    // Chrome-style search bar overlaid at the top-right of the terminal.
    // Slides down into view on open and up out of view on close.
    let search_overlay: Element<'_, Message> = if session.show_search {
        // A `responsive` wrapper hands the find bar the real pane width so it
        // can shrink instead of overflowing when the terminal is narrow.
        responsive(move |size| view_search_overlay(session, p, session.search_anim, size.width))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    } else {
        Space::new().width(0).height(0).into()
    };

    let terminal_with_search: Element<'_, Message> =
        if session.disconnected && session.disconnect_dismissed {
            container(column![
                disconnect_status_bar(session_id, p, app.preferences.terminal_font_size),
                stack![terminal, search_overlay],
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &Theme| terminal_body_style(p))
            .into()
        } else {
            stack![terminal, search_overlay].into()
        };

    let body: Element<'_, Message> = if session.sidebar.show || session.sidebar.anim > 0.0 {
        row![
            terminal_with_search,
            slide_panel_from_right(
                view_right_sidebar(app, &session.sidebar),
                RIGHT_SIDEBAR_WIDTH,
                true,
                session.sidebar.anim,
            ),
        ]
        .spacing(0)
        .height(Length::Fill)
        .into()
    } else {
        terminal_with_search
    };

    // Disconnect overlay: shown when the SSH connection drops, asking the user
    // to keep the terminal for review, reconnect, or close it.
    let body = disconnect_overlay(app, session_id, body);
    let body = split_preview_wrapper(body, app.terminal.drag_preview.as_ref());

    column![top, body].into()
}

// Disconnect overlay

/// Compact, non-blocking replacement for the connection-lost dialog after the
/// user dismisses it. It keeps the disconnect actions reachable without
/// covering the terminal output that the user may still want to copy.
pub(super) fn disconnect_status_bar(
    session_id: SessionId,
    p: theme::ThemePalette,
    terminal_font_size: f32,
) -> Element<'static, Message> {
    let reconnect = Message::Terminal(crate::terminal::message::Message::DisconnectedReconnect(
        session_id,
    ));
    let close = Message::Terminal(crate::terminal::message::Message::DisconnectedClose(
        session_id,
    ));

    let status = row![
        view_os_icon(
            OsIcon::Os(crate::core::fonts::my_icon::POINT),
            (terminal_font_size * 0.55).max(8.0),
            p.danger,
        ),
        text(t_static(t!("terminal.disconnect_banner")))
            .size(terminal_font_size)
            .color(p.text_sec)
            .width(Length::Fill)
            .ellipsis(iced::widget::text::Ellipsis::End),
        disconnect_bar_button(
            t_static(t!("terminal.disconnect_close")),
            close,
            DisconnectButtonKind::Danger,
            p,
            terminal_font_size,
        ),
        disconnect_bar_button(
            t_static(t!("terminal.reconnect")),
            reconnect,
            DisconnectButtonKind::Primary,
            p,
            terminal_font_size,
        ),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    let banner = container(status)
        .width(Length::Fill)
        .padding(Padding::from(DISCONNECT_BANNER_PADDING))
        .style(move |_: &Theme| disconnect_banner_style(p));

    // Keep this frame transparent because pane-grid borders are painted before
    // child content; a full-width fill here would cover them in split view.
    container(banner)
        .width(Length::Fill)
        .padding(disconnect_banner_margin())
        .into()
}

fn disconnect_banner_margin() -> Padding {
    Padding::from(DISCONNECT_BANNER_MARGIN)
}

fn terminal_body_style(p: theme::ThemePalette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.terminal_bg)),
        ..container::Style::default()
    }
}

fn disconnect_banner_style(p: theme::ThemePalette) -> container::Style {
    container::Style {
        background: Some(Background::Color(p.terminal_notice_bg)),
        border: Border {
            radius: DISCONNECT_BANNER_RADIUS.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

#[derive(Clone, Copy)]
enum DisconnectButtonKind {
    Primary,
    Danger,
}

fn disconnect_bar_button(
    label: &'static str,
    msg: Message,
    kind: DisconnectButtonKind,
    p: theme::ThemePalette,
    terminal_font_size: f32,
) -> Element<'static, Message> {
    let text_color = match kind {
        DisconnectButtonKind::Primary => Color::WHITE,
        DisconnectButtonKind::Danger => p.danger,
    };
    let vertical_padding = (terminal_font_size * 0.22).clamp(3.0, 6.0);
    let horizontal_padding = (terminal_font_size * 0.57).clamp(8.0, 14.0);

    button(
        text(label.to_string())
            .size(terminal_font_size)
            .color(text_color),
    )
    .on_press(msg)
    .padding(Padding {
        top: vertical_padding,
        right: horizontal_padding,
        bottom: vertical_padding,
        left: horizontal_padding,
    })
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        let background = match kind {
            DisconnectButtonKind::Primary => {
                if hovered {
                    p.accent_hover
                } else {
                    p.accent
                }
            }
            DisconnectButtonKind::Danger => {
                if hovered {
                    Color {
                        a: 0.16,
                        ..p.danger
                    }
                } else {
                    p.button_bg
                }
            }
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                width: match kind {
                    DisconnectButtonKind::Primary => 0.0,
                    DisconnectButtonKind::Danger => 1.0,
                },
                color: match kind {
                    DisconnectButtonKind::Primary => background,
                    DisconnectButtonKind::Danger => p.danger,
                },
                radius: 4.0.into(),
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default())
    .into()
}

pub(super) fn disconnect_overlay<'a>(
    app: &App,
    session_id: SessionId,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(session) = app.terminal.sessions.iter().find(|s| s.id == session_id) else {
        return content;
    };
    if !disconnect_overlay_visible(session) {
        return content;
    }

    let p = theme::palette(app.effective_theme_mode());

    let title = t!(
        "terminal.connection_lost_label",
        label = session.label.as_str()
    )
    .to_string();

    let dismiss_msg = Message::Terminal(crate::terminal::message::Message::DisconnectedDismiss(
        session_id,
    ));
    let close_msg = Message::Terminal(crate::terminal::message::Message::DisconnectedClose(
        session_id,
    ));
    let reconnect_msg = Message::Terminal(
        crate::terminal::message::Message::DisconnectedReconnect(session_id),
    );

    let leading_actions = vec![cancel_button_with_label(
        t_static(t!("terminal.disconnect_dismiss")),
        dismiss_msg.clone(),
        &p,
    )];
    let trailing_actions = vec![
        danger_outline_button(t_static(t!("terminal.disconnect_close")), close_msg, &p),
        confirm_button(
            t_static(t!("terminal.reconnect")),
            reconnect_msg,
            p.accent,
            Color::WHITE,
            false,
        ),
    ];

    let dialog = message_dialog::message_dialog_with_leading_actions(
        title,
        Some(t_static(t!("terminal.connection_dropped"))),
        column![].into(),
        leading_actions,
        trailing_actions,
        dismiss_msg,
        420.0,
        &p,
        session.disconnect_anim,
    );

    iced::widget::stack([content, dialog])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// Spotlight overlay (Alfred-style, centered on the app window)
// Rendered as a top-level overlay in the main view's Stack so the popup
// is centered relative to the entire application window, not the terminal.

pub(crate) fn view_spotlight_overlay(app: &App) -> Element<'_, Message> {
    let (session_id, session) = match app.terminal.sessions.iter().find(|s| s.spotlight.is_some()) {
        Some(s) => (s.id, s),
        None => {
            return Space::new()
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        }
    };

    let Some(popup) = &session.spotlight else {
        return Space::new()
            .width(Length::Shrink)
            .height(Length::Shrink)
            .into();
    };

    let p = theme::palette(app.effective_theme_mode());

    // Shared backdrop (Alfred / Spotlight style)
    // A MouseArea that fills the layer. Clicking it dismisses the spotlight
    // and captures the event so nothing falls through to the terminal.
    let backdrop = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.15))),
                ..container::Style::default()
            }),
    )
    .on_press(Message::SpotlightCancel(session_id))
    .interaction(mouse::Interaction::Idle);

    // Build the popup card fresh on each layout pass (responsive's closure is
    // `Fn`, so it cannot consume a pre-built Element).
    let build_card = move || {
        let placeholder = t_static(t!("terminal.spotlight_placeholder"));
        let input: Element<'_, Message> = text_input(placeholder, &popup.filter_text)
            .on_input(move |s| Message::SpotlightFilter(session_id, s))
            .id(Id::new(crate::update::SPOTLIGHT_INPUT_ID))
            .padding(8)
            .size(14)
            .width(Length::Fill)
            .style(move |_, status| input_style_with_palette(&p, status))
            .into();

        let separator = spotlight_divider(p);

        let list: Element<'_, Message> = if popup.filtered_commands.is_empty() {
            container(
                text(t_static(t!("terminal.spotlight_no_results")))
                    .size(13)
                    .color(p.text_dim),
            )
            .width(Length::Fill)
            .padding(iced::Padding {
                left: 6.0,
                right: 6.0,
                top: 12.0,
                bottom: 12.0,
            })
            .into()
        } else {
            let mut rows: Vec<Element<'_, Message>> = Vec::new();
            let selected_index = popup.selected_index();

            for (idx, cmd) in popup.filtered_commands.iter().enumerate() {
                let is_selected = selected_index == Some(idx);
                let is_hovered = popup.hovered_index == Some(idx);
                let sid = session_id;
                let command_id = cmd.id.clone();

                let row_bg = if is_selected {
                    Color {
                        a: 0.15,
                        ..p.accent
                    }
                } else if is_hovered {
                    Color {
                        a: 0.08,
                        ..p.accent
                    }
                } else {
                    Color::TRANSPARENT
                };

                let name_color = if is_selected { p.accent } else { p.text_pri };

                let icon = text(char::from(cmd.icon))
                    .font(LUCIDE_FONT)
                    .size(14)
                    .color(if is_selected { p.accent } else { p.text_sec });

                let row_content = row![
                    icon,
                    Space::new().width(Length::Fixed(6.0)),
                    text(cmd.label.clone()).size(14).color(name_color),
                    Space::new().width(Length::Fixed(8.0)),
                    text(cmd.description.clone())
                        .size(13)
                        .color(p.text_dim)
                        .width(Length::Fill)
                        .ellipsis(text::Ellipsis::End)
                        .wrapping(text::Wrapping::None),
                    Space::new().width(Length::Fixed(8.0)),
                    text(cmd.category.label()).size(11).color(p.text_dim),
                ]
                .align_y(iced::Alignment::Center);

                let row = button(
                    container(row_content)
                        .padding(iced::Padding {
                            left: 6.0,
                            right: 6.0,
                            top: 5.0,
                            bottom: 5.0,
                        })
                        .width(Length::Fill)
                        .style(move |_: &Theme| container::Style {
                            background: Some(Background::Color(row_bg)),
                            border: Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                            ..container::Style::default()
                        }),
                )
                .on_press(Message::SpotlightPick(sid, command_id))
                .style(iced::widget::button::text)
                .padding(0)
                .width(Length::Fill)
                .cursor_interaction(mouse::Interaction::default());

                let row = mouse_area(row)
                    .on_enter(Message::SpotlightHover(sid, Some(idx)))
                    .on_exit(Message::SpotlightHover(sid, None));

                rows.push(row.into());
            }

            column(rows).spacing(2).width(Length::Fill).into()
        };

        let body = column![input, separator, list].spacing(8);
        let inner: Element<'_, Message> = body.into();
        let popup_w = 552.0;
        let pad_h = 12.0;
        let shadow_alpha = 0.35;
        let shadow_offset_y = 4.0;

        container(inner)
            .padding(iced::Padding {
                left: pad_h,
                top: pad_h,
                right: pad_h,
                bottom: pad_h,
            })
            .width(Length::Fixed(popup_w))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.card_bg)),
                border: Border {
                    color: p.border,
                    width: 1.0,
                    radius: 8.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color::from_rgba(0.0, 0.0, 0.0, shadow_alpha),
                    offset: Vector::new(0.0, shadow_offset_y),
                    blur_radius: 16.0,
                },
                ..container::Style::default()
            })
    };

    // Slide-down entrance animation: the card drops in from the top edge
    // (ease-out), anchored below the topbar — VSCode's F1 palette look. The
    // card stays horizontally centered; `slide_wrapper` shifts it up by
    // (1 - eased) × height so it slides down to rest as `anim` advances.
    let eased = crate::ui::helpers::ease_out_cubic(popup.anim);
    let top_anchored = responsive(move |_size| {
        // Wrap the card in a MouseArea that captures clicks on it (no-op) so
        // they neither dismiss the spotlight nor fall through to the terminal.
        // Only the backdrop dismisses.
        let card = mouse_area(build_card())
            .on_press(Message::Noop)
            .interaction(mouse::Interaction::Idle);
        container(slide_wrapper(card.into(), eased))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Top)
            .padding(iced::Padding {
                left: 0.0,
                right: 0.0,
                top: 60.0,
                bottom: 0.0,
            })
            .clip(true)
            .into()
    });

    // Stack: backdrop fills the entire window, popup card floats on top.
    // Clicks outside the card land on the backdrop MouseArea and dismiss the
    // spotlight; clicks on the card are absorbed by the card's MouseArea.
    iced::widget::stack([backdrop.into(), top_anchored.into()])
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Thin 1px divider used to separate the spotlight's input and list sections.
fn spotlight_divider(p: theme::ThemePalette) -> Element<'static, Message> {
    container(Space::new())
        .height(Length::Fixed(1.0))
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(Color {
                a: 0.12,
                ..p.text_dim
            })),
            ..container::Style::default()
        })
        .into()
}

// Terminal search bar (Chrome-style find bar)
// Floats at the top-right corner of the terminal area.

const SEARCH_BAR_MAX_WIDTH: f32 = 360.0;
/// The find bar never shrinks below this, even in the narrowest split pane.
const SEARCH_BAR_MIN_WIDTH: f32 = 120.0;
/// Gaps kept between the floating bar and the pane edges it hovers over.
const SEARCH_BAR_LEFT_GAP: f32 = 8.0;
const SEARCH_BAR_RIGHT_GAP: f32 = TERMINAL_PADDING as f32 + 6.0;
/// Pane-width breakpoints below which lower-priority chrome folds away. The
/// order of sacrifice is mode toggles → match counter → prev/next, so the
/// query field (and the close button) survive in every pane size.
const SEARCH_BAR_TOGGLES_BREAKPOINT: f32 = 360.0;
const SEARCH_BAR_COUNTER_BREAKPOINT: f32 = 260.0;
const SEARCH_BAR_NAV_BREAKPOINT: f32 = 180.0;

/// Width the floating find bar takes inside `available_width`, reserving the
/// side gaps so the card never touches — or overflows — the pane edges.
fn search_bar_width(available_width: f32) -> f32 {
    let usable = (available_width - SEARCH_BAR_LEFT_GAP - SEARCH_BAR_RIGHT_GAP).max(0.0);
    let preferred = usable.clamp(SEARCH_BAR_MIN_WIDTH, SEARCH_BAR_MAX_WIDTH);
    // Never overflow the pane: if even the floor width does not fit, shrink.
    preferred.min(usable)
}

/// How much chrome the find bar can afford at a given width. Narrow split
/// panes shed lower-priority controls first so the query field stays usable.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchBarLayout {
    /// Full bar: mode toggles, match counter, prev/next and close.
    Full,
    /// Mode toggles fold away; counter, prev/next and close remain.
    Compact,
    /// Mode toggles and match counter fold away; prev/next and close remain.
    Narrow,
    /// Below 180px only the query field and the close button remain.
    Minimal,
}

impl SearchBarLayout {
    fn for_width(width: f32) -> Self {
        if width >= SEARCH_BAR_TOGGLES_BREAKPOINT {
            Self::Full
        } else if width >= SEARCH_BAR_COUNTER_BREAKPOINT {
            Self::Compact
        } else if width >= SEARCH_BAR_NAV_BREAKPOINT {
            Self::Narrow
        } else {
            Self::Minimal
        }
    }

    fn shows_toggles(self) -> bool {
        matches!(self, Self::Full)
    }

    fn shows_counter(self) -> bool {
        matches!(self, Self::Full | Self::Compact)
    }

    fn shows_nav(self) -> bool {
        !matches!(self, Self::Minimal)
    }

    fn shows_input_icon(self) -> bool {
        !matches!(self, Self::Minimal)
    }
}

/// Floating find bar anchored to the top-right of a terminal area, sized to
/// the space actually available so it degrades gracefully inside narrow panes.
pub(super) fn view_search_overlay(
    session: &TerminalSession,
    p: theme::ThemePalette,
    anim: f32,
    available_width: f32,
) -> Element<'_, Message> {
    let eased = crate::ui::helpers::ease_out_cubic(anim);

    container(slide_wrapper(
        view_search_bar(session, p, available_width),
        eased,
    ))
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(iced::alignment::Horizontal::Right)
    .align_y(iced::alignment::Vertical::Top)
    .padding(Padding {
        top: f32::from(TERMINAL_PADDING),
        right: SEARCH_BAR_RIGHT_GAP,
        bottom: 0.0,
        left: SEARCH_BAR_LEFT_GAP,
    })
    .clip(true)
    .into()
}

pub(crate) fn view_search_bar(
    session: &TerminalSession,
    p: theme::ThemePalette,
    available_width: f32,
) -> Element<'_, Message> {
    let session_id = session.id;
    // Chrome tier and physical width are both driven by the space the pane
    // actually offers, so the bar degrades as the pane narrows.
    let layout = SearchBarLayout::for_width(available_width);
    let bar_width = search_bar_width(available_width);
    let nav_visible = layout.shows_nav() && !session.search_query.is_empty();

    let icon_button =
        |icon: lucide_icons::Icon, size: f32, padding: [f32; 2], active: bool, msg: Message| {
            mouse_area(
                button(
                    container(
                        text(char::from(icon))
                            .font(LUCIDE_FONT)
                            .size(size)
                            .color(p.text_dim),
                    )
                    .center_x(Length::Shrink)
                    .center_y(Length::Shrink),
                )
                .on_press(msg)
                .padding(Padding::from(padding))
                .style(move |_: &Theme, status| {
                    let hovered = matches!(status, button::Status::Hovered);
                    button::Style {
                        background: hovered.then_some(Background::Color(p.item_hover)),
                        text_color: if hovered { p.text_pri } else { p.text_dim },
                        border: Border {
                            color: if active { p.accent } else { Color::TRANSPARENT },
                            width: if active { 1.0 } else { 0.0 },
                            radius: 3.0.into(),
                        },
                        ..button::Style::default()
                    }
                })
                .cursor_interaction(mouse::Interaction::None),
            )
            .interaction(mouse::Interaction::Idle)
        };

    // Reserve room for the toggle overlay only when the toggles are shown;
    // otherwise the field reclaims that space for the query text.
    let input_right_padding = if layout.shows_toggles() { 78.0 } else { 8.0 };

    let field = text_input(
        t_static(t!("placeholder.find_terminal")),
        &session.search_query,
    )
    .id(iced::widget::Id::from(format!("search_input_{session_id}")))
    .on_input(move |s| {
        Message::Terminal(crate::terminal::message::Message::TerminalSearch(
            session_id, s,
        ))
    })
    .on_submit(Message::Terminal(
        crate::terminal::message::Message::SearchNext(session_id),
    ))
    .padding(Padding {
        top: 5.0,
        right: input_right_padding,
        bottom: 5.0,
        left: 8.0,
    })
    .size(13)
    .width(Length::Fill)
    .style(move |_, status| input_style_with_palette(&p, status));

    // The leading search glyph costs room the minimal bar needs for typing.
    let field = if layout.shows_input_icon() {
        field.icon(iced::widget::text_input::Icon {
            font: LUCIDE_FONT,
            code_point: char::from(lucide_icons::Icon::Search),
            size: Some(iced::Pixels(13.0)),
            spacing: 6.0,
            side: iced::widget::text_input::Side::Left,
        })
    } else {
        field
    };

    let search_input: Element<'_, Message> = if layout.shows_toggles() {
        let toggles = container(
            row![
                icon_button(
                    lucide_icons::Icon::CaseSensitive,
                    13.0,
                    [2.0, 4.0],
                    session.search_case_sensitive,
                    Message::Terminal(crate::terminal::message::Message::SearchToggleCase(
                        session_id,
                    ))
                ),
                icon_button(
                    lucide_icons::Icon::WholeWord,
                    13.0,
                    [2.0, 4.0],
                    session.search_whole_word,
                    Message::Terminal(crate::terminal::message::Message::SearchToggleWholeWord(
                        session_id,
                    ))
                ),
                icon_button(
                    lucide_icons::Icon::Regex,
                    13.0,
                    [2.0, 4.0],
                    session.search_regex,
                    Message::Terminal(crate::terminal::message::Message::SearchToggleRegex(
                        session_id,
                    ))
                ),
            ]
            .spacing(2),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Center)
        .padding(Padding {
            top: 0.0,
            right: 7.0,
            bottom: 0.0,
            left: 0.0,
        });

        container(stack![field, toggles]).width(Length::Fill).into()
    } else {
        field.into()
    };

    // Build the row incrementally so hidden controls leave no gap behind.
    let mut bar_content = row![search_input]
        .spacing(6)
        .align_y(iced::alignment::Vertical::Center);

    if nav_visible {
        let mut nav = row![].spacing(2).align_y(iced::alignment::Vertical::Center);

        if layout.shows_counter() {
            let current = if session.search_matches_count > 0 {
                session.search_index + 1
            } else {
                0
            };
            nav = nav.push(
                text(format!("{}/{}", current, session.search_matches_count))
                    .size(13)
                    .color(p.text_dim),
            );
        }

        nav = nav.push(icon_button(
            lucide_icons::Icon::ChevronUp,
            13.0,
            [3.0, 5.0],
            false,
            Message::Terminal(crate::terminal::message::Message::SearchPrev(session_id)),
        ));
        nav = nav.push(icon_button(
            lucide_icons::Icon::ChevronDown,
            13.0,
            [3.0, 5.0],
            false,
            Message::Terminal(crate::terminal::message::Message::SearchNext(session_id)),
        ));

        bar_content = bar_content.push(nav);
    }

    bar_content = bar_content.push(icon_button(
        lucide_icons::Icon::X,
        13.0,
        [3.0, 5.0],
        false,
        Message::Terminal(crate::terminal::message::Message::SearchClose(session_id)),
    ));

    let bar_content = bar_content.padding(Padding {
        top: 4.0,
        right: 4.0,
        bottom: 4.0,
        left: 4.0,
    });

    container(bar_content)
        .width(Length::Fixed(bar_width))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.card_bg)),
            border: Border {
                radius: 8.0.into(),
                color: p.border,
                width: 1.0,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
                offset: Vector::new(0.0, 2.0),
                blur_radius: 12.0,
            },
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnect_banner_uses_a_rounded_background_without_a_border() {
        let palette = theme::dark_palette();
        let style = disconnect_banner_style(palette);

        assert_eq!(
            style.background,
            Some(Background::Color(palette.terminal_notice_bg))
        );
        assert_eq!(
            style.border.radius,
            DISCONNECT_BANNER_RADIUS.into(),
            "the notice should have rounded corners"
        );
        assert_eq!(
            style.border.width, 0.0,
            "the notice should rely on its background instead of a border"
        );
        assert_eq!(
            style.border.color,
            Color::TRANSPARENT,
            "the border color should not affect the notice"
        );
    }

    #[test]
    fn disconnect_banner_has_a_six_pixel_outer_margin() {
        let margin = disconnect_banner_margin();

        assert_eq!(margin.top, 6.0);
        assert_eq!(margin.right, 6.0);
        assert_eq!(margin.bottom, 6.0);
        assert_eq!(margin.left, 6.0);
    }

    #[test]
    fn standalone_terminal_body_matches_the_terminal_background() {
        let palette = theme::dark_palette();
        let style = terminal_body_style(palette);

        assert_eq!(
            style.background,
            Some(Background::Color(palette.terminal_bg))
        );
    }

    #[test]
    fn search_bar_width_clamps_to_bounds_and_never_overflows() {
        // Wide panes cap at the full bar width.
        assert_eq!(search_bar_width(1200.0), SEARCH_BAR_MAX_WIDTH);
        // Mid panes track the available space minus the side gaps.
        assert_eq!(search_bar_width(300.0), 300.0 - 22.0);
        // Panes narrower than the floor shrink instead of overflowing.
        assert_eq!(search_bar_width(80.0), 80.0 - 22.0);
    }

    #[test]
    fn layout_sheds_chrome_as_the_pane_narrows() {
        // Full width keeps the mode toggles.
        let full = SearchBarLayout::for_width(SEARCH_BAR_MAX_WIDTH);
        assert!(full.shows_toggles() && full.shows_counter() && full.shows_nav());

        // Below the toggle breakpoint the toggles fold, counter/prev-next stay.
        let compact = SearchBarLayout::for_width(300.0);
        assert!(!compact.shows_toggles() && compact.shows_counter() && compact.shows_nav());

        // Below the counter breakpoint only prev/next (and close) remain.
        let narrow = SearchBarLayout::for_width(200.0);
        assert!(!narrow.shows_toggles() && !narrow.shows_counter() && narrow.shows_nav());

        // Below 180px only the query field and the close button fit.
        let minimal = SearchBarLayout::for_width(SEARCH_BAR_NAV_BREAKPOINT - 1.0);
        assert!(!minimal.shows_toggles() && !minimal.shows_counter() && !minimal.shows_nav());
        assert!(!minimal.shows_input_icon());
    }
}
