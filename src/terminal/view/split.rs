//! Split-tab rendering and per-pane layout.

// Rust guideline compliant 2026-10-09

use iced::widget::pane_grid::{self, PaneGrid};
use iced::widget::{Space, canvas, column, container, responsive, row, stack, text};
use iced::{Background, Border, Element, Length, Padding, Size, Theme};
use iced_term::TerminalView;

use super::{
    PaneDividerDots, SPLIT_TERMINAL_RIGHT_PADDING, TERMINAL_PADDING, disconnect_overlay,
    disconnect_status_bar, horizontal_border, osc_watermark_overlay, split_preview_wrapper,
    view_pane_controls, view_search_overlay,
};
use crate::app::App;
use crate::core::ids::{SessionId, SplitId};
use crate::core::message::Message;
use crate::core::theme;
use crate::terminal::Split;
use crate::terminal::split_layout::{CONTAINER_PADDING, PANE_RADIUS, PANE_SPACING, grid_min_size};
use crate::terminal::state::disconnect_overlay_visible;
use crate::ui::slide::slide_panel_from_right;
use crate::views::LUCIDE_FONT;
use crate::views::right_sidebar::{RIGHT_SIDEBAR_WIDTH, view_right_sidebar};

/// Render a split tab (a `TabKind::Split`): the pane grid plus the same top
/// border, drag-preview wrapper, and shared sidebar as a standalone terminal.
pub fn view_split_terminal(app: &App, split_id: SplitId) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let top = horizontal_border(p);

    let Some(split) = app.terminal.splits.get(&split_id) else {
        return column![top, Space::new().width(Length::Fill).height(Length::Fill)].into();
    };

    let body: Element<'_, Message> = view_split(app, split, p);
    let body = split_preview_wrapper(body, app.terminal.drag_preview.as_ref());

    // The sidebar is shared across every pane in the split; render it whenever
    // any pane has it open, so it doesn't flicker when switching focus.
    let sidebar = split.panes.iter().find_map(|(_, sid)| {
        app.terminal
            .sessions
            .iter()
            .find(|s| s.id == *sid && (s.sidebar.show || s.sidebar.anim > 0.0))
            .map(|s| &s.sidebar)
    });
    let body = if let Some(sidebar) = sidebar {
        row![
            body,
            slide_panel_from_right(
                view_right_sidebar(app, sidebar),
                RIGHT_SIDEBAR_WIDTH,
                true,
                sidebar.anim
            ),
        ]
        .spacing(0)
        .height(Length::Fill)
        .into()
    } else {
        body
    };

    let body = split_disconnect_overlay(app, split_id, body);

    column![top, body].into()
}

// Split disconnect overlay
// For split tabs, connection-lost dialogs are rendered once per disconnected
// session but centered over the whole split area (not inside individual
// panes). Multiple dialogs stack on top of each other, with the focused
// pane's dialog last so it is the visible top card. A dismissed dialog leaves
// that pane's compact status bar in place.
fn split_disconnect_overlay<'a>(
    app: &'a App,
    split_id: SplitId,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(split) = app.terminal.splits.get(&split_id) else {
        return content;
    };

    let mut disconnected: Vec<SessionId> = split
        .panes
        .iter()
        .filter_map(|(_, sid)| {
            app.terminal
                .sessions
                .iter()
                .find(|s| s.id == *sid && disconnect_overlay_visible(s))
                .map(|_| *sid)
        })
        .collect();

    if disconnected.is_empty() {
        return content;
    }

    // Put the focused disconnected pane last so its dialog renders on top.
    if let Some(focused) = split.focus.and_then(|pane| split.panes.get(pane).copied())
        && let Some(pos) = disconnected.iter().position(|sid| *sid == focused)
    {
        let focused = disconnected.remove(pos);
        disconnected.push(focused);
    }

    let mut body = content;
    for sid in disconnected {
        body = disconnect_overlay(app, sid, body);
    }
    body
}

fn view_split<'a>(app: &'a App, split: &'a Split, p: theme::ThemePalette) -> Element<'a, Message> {
    let total_panes = split.panes.len();
    let focused_pane = split.focus;

    responsive(move |available| {
        let inner_size = Size::new(
            (available.width - CONTAINER_PADDING * 2.0).max(0.0),
            (available.height - CONTAINER_PADDING * 2.0).max(0.0),
        );
        let min_size = grid_min_size(split, inner_size);

        let pane_grid = PaneGrid::new(&split.panes, move |id, sid, _| {
            let Some(session) = app.terminal.sessions.iter().find(|s| s.id == *sid) else {
                return pane_grid::Content::new(responsive(move |_| {
                    container(Space::new())
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(move |_: &Theme| container::Style {
                            background: Some(Background::Color(p.content_bg)),
                            ..Default::default()
                        })
                        .into()
                }));
            };

            let is_focused = Some(id) == focused_pane;

            let title_bar_text = p.text_dim;
            let title_icon = text(char::from(lucide_icons::Icon::Terminal))
                .font(LUCIDE_FONT)
                .size(12)
                .color(title_bar_text);
            let title_label = text(&session.label)
                .size(12)
                .color(title_bar_text)
                .ellipsis(iced::widget::text::Ellipsis::End);
            let title_bar = pane_grid::TitleBar::new(
                container(
                    row![title_icon, title_label]
                        .spacing(4)
                        .align_y(iced::Alignment::Center),
                )
                .padding(iced::Padding {
                    top: 2.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                }),
            )
            .controls(view_pane_controls(id, total_panes, split.id, p))
            .padding(Padding {
                top: 6.0,
                right: 8.0,
                bottom: 4.0,
                left: 8.0,
            })
            .style(move |_: &Theme| container::Style {
                background: None,
                ..Default::default()
            });

            let p_border = p.split_focus_border;
            let p_border_dim = p.border;
            let p_bg = p.terminal_bg;
            pane_grid::Content::new(responsive(move |pane_size| {
                let term: Element<'_, Message> =
                    container(TerminalView::show(&session.terminal).map(|v0| {
                        Message::Terminal(crate::terminal::message::Message::IcedTermEvent(v0))
                    }))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .padding(iced::Padding {
                        top: f32::from(TERMINAL_PADDING),
                        right: SPLIT_TERMINAL_RIGHT_PADDING,
                        bottom: f32::from(TERMINAL_PADDING),
                        left: f32::from(TERMINAL_PADDING),
                    })
                    .into();
                let term: Element<'_, Message> = stack![
                    term,
                    osc_watermark_overlay(
                        app.preferences.osc_watermark_enabled && session.osc_integration_detected,
                        p,
                    ),
                ]
                .into();
                // Chrome-style search bar overlay for split panes. It sizes
                // itself to the pane width, so a crowded grid keeps a usable
                // (if trimmed) find bar instead of a clipped one.
                let term_with_search: Element<'_, Message> = if session.show_search {
                    stack![
                        term,
                        view_search_overlay(session, p, session.search_anim, pane_size.width),
                    ]
                    .into()
                } else {
                    term
                };
                if session.disconnected && session.disconnect_dismissed {
                    column![
                        disconnect_status_bar(session.id, p, app.preferences.terminal_font_size),
                        term_with_search,
                    ]
                    .height(Length::Fill)
                    .into()
                } else {
                    term_with_search
                }
            }))
            .title_bar(title_bar)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p_bg)),
                border: Border {
                    width: 1.0,
                    color: if is_focused { p_border } else { p_border_dim },
                    radius: PANE_RADIUS.into(),
                },
                ..Default::default()
            })
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .spacing(PANE_SPACING)
        .min_size(min_size)
        .on_click(move |pane| {
            Message::Terminal(crate::terminal::message::Message::SplitClicked(
                split.id, pane,
            ))
        })
        .on_drag(move |event| {
            Message::Terminal(crate::terminal::message::Message::SplitDragged(
                split.id, event,
            ))
        })
        .on_resize(8, move |event| {
            Message::Terminal(crate::terminal::message::Message::SplitResized(
                split.id, event, inner_size,
            ))
        })
        .style(move |theme: &Theme| {
            let mut style = pane_grid::default(theme);
            style.hovered_split.color = p.split_line;
            style.picked_split.color = p.split_line;
            style
        });
        let dots = canvas::Canvas::new(PaneDividerDots {
            node: split.panes.layout().clone(),
            min_size,
            color: p.text_dim,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let pane_grid = stack![pane_grid, dots]
            .width(Length::Fill)
            .height(Length::Fill);

        container(pane_grid)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(CONTAINER_PADDING)
            .id(iced::widget::Id::from(super::TERMINAL_DROP_ZONE_ID))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.content_bg)),
                ..Default::default()
            })
            .into()
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
