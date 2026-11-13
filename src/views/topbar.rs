use iced::widget::{
    Id, Space, Stack, button, container, mouse_area, row, scrollable, text, text_input,
};
use iced::{Background, Border, Color, Element, Gradient, Length, Padding, Theme, mouse};

use crate::app::{ActiveTab, App, SessionId, TabId, Workspace};
use crate::core::data::OsIcon;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::{self, ThemePalette};
use crate::core::widgets::{TooltipPosition, cursor_tooltip_label_at, scrollable_style};
use crate::terminal::{TabKind, TerminalSession};
use crate::ui::keyed_row::KeyedRow;
use crate::ui::native_titlebar;
use rust_i18n::t;

use super::{
    LUCIDE_FONT, MAX_TAB_WIDTH, MY_ICONS_FONT, TAB_HEIGHT, TAB_MIN_WIDTH, TAB_PADDING, tab_icon,
    view_os_icon,
};

/// Scrollable ID for the horizontal topbar tab strip.  Used to route wheel
/// events into horizontal scroll commands.
pub const TOP_TABS_SCROLL_ID: &str = "top_tabs_scroll";

// Reserve room for the native traffic lights in the full-size macOS content view.
// The width is owned by the module that lays the buttons out, so the gutter the
// top bar reserves and the buttons themselves can not drift apart.
#[cfg(target_os = "macos")]
const TOPBAR_LEFT_PADDING: f32 = crate::core::traffic_lights::TRAFFIC_LIGHTS_GUTTER;

#[cfg(target_os = "macos")]
const TOPBAR_LEFT_PADDING_FULLSCREEN: f32 = 10.0;

#[cfg(not(target_os = "macos"))]
const TOPBAR_LEFT_PADDING: f32 = 8.0;

/// Left padding for the top bar: the traffic-light gutter whenever the native
/// buttons are actually on screen, `fullscreen_padding` otherwise.
#[cfg(target_os = "macos")]
fn topbar_gutter(fullscreen_padding: f32) -> f32 {
    if crate::core::traffic_lights::traffic_lights_reserved() {
        TOPBAR_LEFT_PADDING
    } else {
        fullscreen_padding
    }
}

/// Chrome-style pill tab style with full rounded corners and a subtle outline.
fn chrome_tab_style(
    p: ThemePalette,
    is_active: bool,
    is_hovered: bool,
    is_reorder_target: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    let topbar_tab_active = p.topbar_tab_active;
    let surface = p.surface;
    let topbar_tab_inactive = p.topbar_tab_inactive;
    let accent = p.accent;
    let text_pri = p.text_pri;
    let text_sec = p.text_sec;
    let show_border = p.show_border;
    let border_color = p.border;
    move |_: &Theme, status| {
        let hovered = is_hovered || matches!(status, button::Status::Hovered);
        let pressed = matches!(status, button::Status::Pressed);
        let bg = if is_reorder_target {
            // Subtle accent tint to indicate valid reorder drop target
            Color { a: 0.18, ..accent }
        } else if is_active || hovered {
            topbar_tab_active
        } else if pressed {
            surface
        } else {
            topbar_tab_inactive
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: if is_active || hovered || is_reorder_target {
                text_pri
            } else {
                text_sec
            },
            border: Border {
                radius: 8.0.into(),
                width: if show_border { 1.0 } else { 0.0 },
                color: border_color,
            },
            ..button::Style::default()
        }
    }
}

/// Render a terminal-session tab, either standalone (`pane_count == None`) or
/// as a split's focused pane (`pane_count == Some(_)`). Split tabs use a grid
/// icon, a `(N)` label suffix, and no inline rename. `sid` is the displayed
/// session (standalone id, or focused-pane id for a split).
fn session_tab<'a>(
    app: &App,
    p: ThemePalette,
    tid: TabId,
    sid: SessionId,
    session: &'a TerminalSession,
    pane_count: Option<usize>,
    locked: bool,
) -> Element<'a, Message> {
    let is_split = pane_count.is_some();
    let is_active = app.active_tab == ActiveTab::Tab(tid);
    let is_hovered = app.terminal.hovered_terminal_id == Some(sid);
    let is_editing = !is_split && app.terminal.editing_tab == Some(sid);

    let close_btn: Element<'_, Message> = {
        let text_dim = p.text_dim;
        button(
            text(char::from(lucide_icons::Icon::X))
                .font(LUCIDE_FONT)
                .size(12)
                .line_height(1.0)
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0))
                .center()
                .color(text_dim),
        )
        .on_press(Message::Terminal(
            crate::terminal::message::Message::TermClose(sid),
        ))
        .padding(Padding::from([2.0, 2.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: hovered.then_some(Background::Color(p.tab_close_hover)),
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
    };

    let tab_icon_color = if is_active { p.accent } else { p.text_sec };
    // Status dots take priority over the leading icon: red = disconnected,
    // yellow = unstable, green = new output on an inactive tab. Split tabs
    // fall back to a grid icon rather than the OS icon.
    let os_icon: Element<'_, Message> = if session.disconnected {
        view_os_icon(
            OsIcon::Os(crate::core::fonts::my_icon::POINT),
            14.0 / 1.2,
            p.danger,
        )
    } else if session.connection_unstable {
        view_os_icon(
            OsIcon::Os(crate::core::fonts::my_icon::POINT),
            14.0 / 1.2,
            p.warning,
        )
    } else if !is_active && session.has_new_output {
        view_os_icon(
            OsIcon::Os(crate::core::fonts::my_icon::POINT),
            14.0 / 1.2,
            p.success,
        )
    } else if is_split {
        tab_icon(lucide_icons::Icon::LayoutGrid, tab_icon_color).into()
    } else {
        session
            .host_id
            .and_then(|id| app.hosts.list.iter().find(|h| h.id == id))
            .map_or_else(
                || tab_icon(lucide_icons::Icon::Terminal, tab_icon_color).into(),
                |h| view_os_icon(h.os_type.icon(), 14.0 / 1.2, tab_icon_color),
            )
    };

    // A locked tab shows the lock icon in place of the status/OS icon.
    let lead_icon: Element<'_, Message> = if locked {
        tab_icon(lucide_icons::Icon::Lock, tab_icon_color).into()
    } else {
        os_icon
    };

    let label = if is_split {
        format!("{} ({})", session.label, pane_count.unwrap_or(0))
    } else {
        session.label.clone()
    };
    let label_widget: Element<'_, Message> = container(
        text(label)
            .size(14)
            .color(if is_active { p.text_pri } else { p.text_sec })
            .wrapping(iced::widget::text::Wrapping::None)
            .ellipsis(iced::widget::text::Ellipsis::End),
    )
    .width(Length::Fill)
    .clip(true)
    .into();

    // Close button is shown when the tab is active, being renamed, or hovered.
    let show_close = is_active || is_editing || is_hovered;
    let tab_content: Element<'_, Message> = if is_editing {
        let text_pri = p.text_pri;
        let accent = p.accent;
        let edit_input: Element<'_, Message> = text_input(
            t_static(t!("placeholder.tab_name")),
            &app.terminal.editing_tab_text,
        )
        .id(Id::from(format!("tab_rename_input_{sid}")))
        .on_input(|v0| {
            Message::Terminal(crate::terminal::message::Message::TermRenameInputChanged(
                v0,
            ))
        })
        .on_submit(Message::Terminal(
            crate::terminal::message::Message::TermRenameSubmit(sid),
        ))
        .size(14)
        .width(Length::Fill)
        .padding(0)
        .style(move |_: &Theme, status| {
            let focused = matches!(status, text_input::Status::Focused { .. });
            text_input::Style {
                background: if focused {
                    Background::Color(Color {
                        r: accent.r,
                        g: accent.g,
                        b: accent.b,
                        a: 0.12,
                    })
                } else {
                    Background::Color(Color::TRANSPARENT)
                },
                border: Border {
                    radius: 4.0.into(),
                    color: Color::TRANSPARENT,
                    width: 0.0,
                },
                icon: text_pri,
                placeholder: text_pri,
                value: text_pri,
                selection: Color {
                    r: accent.r,
                    g: accent.g,
                    b: accent.b,
                    a: 0.3,
                },
            }
        })
        .into();
        row![lead_icon, edit_input, close_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    } else if show_close {
        row![lead_icon, label_widget, close_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        row![lead_icon, label_widget]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    };

    let tab_btn = mouse_area(
        button(
            container(tab_content)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(iced::Alignment::Center),
        )
        .padding(TAB_PADDING)
        .height(Length::Fixed(TAB_HEIGHT))
        .style(chrome_tab_style(
            p,
            is_active,
            is_hovered,
            app.terminal.drag_reorder_target == Some(sid),
        ))
        .width(Length::Fill)
        .cursor_interaction(mouse::Interaction::default()),
    )
    .on_right_press(Message::Terminal(
        crate::terminal::message::Message::TermContextMenuOpen(sid),
    ))
    .on_enter(Message::Terminal(
        crate::terminal::message::Message::TerminalTabHovered {
            session_id: sid,
            entered: true,
        },
    ))
    .on_exit(Message::Terminal(
        crate::terminal::message::Message::TerminalTabHovered {
            session_id: sid,
            entered: false,
        },
    ))
    .interaction(iced::mouse::Interaction::Idle);

    let draggable_tab = iced_drop::droppable(tab_btn)
        .drag_threshold(6.0)
        .drag_hide(false)
        .drag_center(true)
        .drag_opacity(0.6)
        .drag_border_radius(8.0)
        .on_press(Message::Hosts(crate::hosts::message::Message::SelectTab(
            tid,
        )))
        .on_drag(move |point, _bounds| {
            Message::Terminal(crate::terminal::message::Message::TerminalTabDragging {
                session_id: sid,
                point,
            })
        })
        .on_drop(move |point, _bounds| {
            Message::Terminal(crate::terminal::message::Message::TerminalTabDropped {
                session_id: sid,
                point,
            })
        })
        .on_cancel(Message::Terminal(
            crate::terminal::message::Message::TerminalTabDragCancelled,
        ));

    let tab_element: Element<'_, Message> = cursor_tooltip_label_at(
        draggable_tab,
        &session.label,
        p,
        TooltipPosition::Bottom,
        Some(350.0),
    )
    .into();

    let target_width = if is_active {
        (TAB_MIN_WIDTH + 40.0).min(MAX_TAB_WIDTH)
    } else {
        TAB_MIN_WIDTH
    };
    let tab_width = app
        .terminal
        .tab_width_anims
        .get(&tid)
        .copied()
        .unwrap_or(target_width);

    let tab_zone_id = Id::from(format!("tab_zone_{sid}"));
    container(tab_element)
        .width(Length::Fixed(tab_width))
        .clip(true)
        .id(tab_zone_id)
        .into()
}

pub fn view_topbar(app: &App) -> Element<'_, Message> {
    #[cfg(target_os = "windows")]
    crate::core::window_chrome::begin_frame();

    let p = theme::palette(app.effective_theme_mode());

    #[cfg(target_os = "macos")]
    let topbar_left_padding = topbar_gutter(TOPBAR_LEFT_PADDING_FULLSCREEN);

    #[cfg(not(target_os = "macos"))]
    let topbar_left_padding = TOPBAR_LEFT_PADDING;

    // "+" new tab button — placed inline after the last tab when tabs fit
    // without scrolling, or fixed to the right when tabs overflow.
    let add_btn = || -> Element<'_, Message> {
        let btn = button(
            container(
                text(char::from(lucide_icons::Icon::Plus))
                    .font(LUCIDE_FONT)
                    .size(16)
                    .color(p.text_sec),
            )
            .height(Length::Fill)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Hosts(crate::hosts::message::Message::OpenNewTab))
        .height(Length::Fixed(TAB_HEIGHT))
        .padding(Padding {
            top: 0.0,
            right: 11.0,
            bottom: 0.0,
            left: 11.0,
        })
        .style({
            let item_hover = p.item_hover;
            move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: hovered.then_some(Background::Color(item_hover)),
                    border: Border {
                        radius: 6.0.into(),
                        ..Border::default()
                    },
                    ..button::Style::default()
                }
            }
        })
        .cursor_interaction(mouse::Interaction::default());

        cursor_tooltip_label_at(
            btn,
            t_static(t!("topbar.new_tab")),
            p,
            TooltipPosition::Bottom,
            None,
        )
        .into()
    };

    let vaults_active = app.active_tab == ActiveTab::Workspace(Workspace::Vaults);
    let vaults_btn = button(
        container(
            row![
                tab_icon(
                    lucide_icons::Icon::Box,
                    if vaults_active { p.accent } else { p.text_sec }
                ),
                text(t!("topbar.vaults")).size(14).color(if vaults_active {
                    p.text_pri
                } else {
                    p.text_sec
                }),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .height(Length::Fill)
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::Hosts(
        crate::hosts::message::Message::SelectWorkspace(Workspace::Vaults),
    ))
    .cursor_interaction(mouse::Interaction::default())
    .height(Length::Fixed(TAB_HEIGHT))
    .padding(TAB_PADDING)
    .style(chrome_tab_style(p, vaults_active, false, false));
    let vaults_btn: Element<'_, Message> = native_titlebar::client_zone(vaults_btn.into());

    let sftp_active = app.active_tab == ActiveTab::Workspace(Workspace::Sftp);
    let sftp_btn = button(
        container(
            row![
                tab_icon(
                    lucide_icons::Icon::HardDrive,
                    if sftp_active { p.accent } else { p.text_sec }
                ),
                text(t!("topbar.sftp")).size(14).color(if sftp_active {
                    p.text_pri
                } else {
                    p.text_sec
                }),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .height(Length::Fill)
        .align_y(iced::Alignment::Center),
    )
    .on_press(Message::Hosts(
        crate::hosts::message::Message::SelectWorkspace(Workspace::Sftp),
    ))
    .cursor_interaction(mouse::Interaction::default())
    .height(Length::Fixed(TAB_HEIGHT))
    .padding(TAB_PADDING)
    .style(chrome_tab_style(p, sftp_active, false, false));
    let sftp_btn: Element<'_, Message> = native_titlebar::client_zone(sftp_btn.into());

    // Scrollable row for terminal session and new-tab tabs. Keyed by tab id so
    // each tab's drag state survives live reordering during a drag.
    let mut tabs_scroll_row = KeyedRow::<Option<TabId>>::new()
        .spacing(6)
        .align_y(iced::Alignment::Center);

    // Sessions rendered inside split panes don't need their own tab, but keep
    // one focused split tab visible so users can return after opening another terminal tab.
    for tid in app.visible_tab_ids() {
        let Some(tab) = app.terminal.tab_registry.get(&tid) else {
            continue;
        };
        let kind = tab.kind;
        let locked = tab.locked;
        match kind {
            TabKind::Session(sid) => {
                let Some(session) = app.terminal.sessions.iter().find(|s| s.id == sid) else {
                    continue;
                };
                let tab_container = native_titlebar::scrolled_client_zone(session_tab(
                    app, p, tid, sid, session, None, locked,
                ));
                tabs_scroll_row = tabs_scroll_row.push(Some(tid), tab_container);
            }
            TabKind::Split(split_id) => {
                // A split tab represents its focused pane's session (for status,
                // hover, drag/drop, and close) but is labeled with a pane count.
                let (focused_sid, pane_count) =
                    app.terminal.splits.get(&split_id).map_or((None, 0), |s| {
                        let count = s.panes.len();
                        let sid = s.focus.and_then(|p| s.panes.get(p).copied());
                        (sid, count)
                    });
                let Some(focused_sid) = focused_sid else {
                    continue;
                };
                let Some(session) = app.terminal.sessions.iter().find(|s| s.id == focused_sid)
                else {
                    continue;
                };
                let tab_container = native_titlebar::scrolled_client_zone(session_tab(
                    app,
                    p,
                    tid,
                    focused_sid,
                    session,
                    Some(pane_count),
                    locked,
                ));
                tabs_scroll_row = tabs_scroll_row.push(Some(tid), tab_container);
            }
            TabKind::Draft(ntid) => {
                let is_active = app.active_tab == ActiveTab::Tab(tid);
                let is_hovered = app.terminal.hovered_new_tab_id == Some(ntid);
                let show_close = is_active || is_hovered;
                // Read host label from the per-tab SSH state.
                let connecting_label = app
                    .terminal
                    .drafts
                    .get(&ntid)
                    .and_then(|d| d.ssh.attempt.pending_host.as_ref())
                    .map(|h| {
                        if h.name.is_empty() {
                            h.hostname.clone()
                        } else {
                            h.name.clone()
                        }
                    });
                let raw_label = connecting_label.as_deref().map_or_else(
                    || t_static(t!("topbar.new_tab")).to_string(),
                    std::string::ToString::to_string,
                );

                let close_btn: Element<'_, Message> = {
                    let text_dim = p.text_dim;
                    button(
                        text(char::from(lucide_icons::Icon::X))
                            .font(LUCIDE_FONT)
                            .size(12)
                            .line_height(1.0)
                            .width(Length::Fixed(16.0))
                            .height(Length::Fixed(16.0))
                            .center()
                            .color(text_dim),
                    )
                    .on_press(Message::Hosts(crate::hosts::message::Message::CloseNewTab(
                        ntid,
                    )))
                    .padding(Padding::from([2.0, 2.0]))
                    .style(move |_: &Theme, status| {
                        let hovered = matches!(status, button::Status::Hovered);
                        button::Style {
                            background: hovered.then_some(Background::Color(p.tab_close_hover)),
                            border: Border {
                                radius: 4.0.into(),
                                ..Border::default()
                            },
                            ..button::Style::default()
                        }
                    })
                    .cursor_interaction(mouse::Interaction::default())
                    .into()
                };

                let tab_content: Element<'_, Message> = if show_close {
                    row![
                        container(
                            text(raw_label.clone())
                                .size(14)
                                .color(if is_active { p.text_pri } else { p.text_sec })
                                .wrapping(iced::widget::text::Wrapping::None)
                                .ellipsis(iced::widget::text::Ellipsis::End),
                        )
                        .width(Length::Fill)
                        .clip(true),
                        close_btn,
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center)
                    .into()
                } else {
                    container(
                        text(raw_label.clone())
                            .size(14)
                            .color(if is_active { p.text_pri } else { p.text_sec })
                            .wrapping(iced::widget::text::Wrapping::None)
                            .ellipsis(iced::widget::text::Ellipsis::End),
                    )
                    .width(Length::Fill)
                    .clip(true)
                    .into()
                };

                let tab_btn = mouse_area(
                    button(
                        container(tab_content)
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .align_y(iced::Alignment::Center),
                    )
                    .on_press(Message::Hosts(crate::hosts::message::Message::SelectTab(
                        tid,
                    )))
                    .cursor_interaction(mouse::Interaction::default())
                    .padding(TAB_PADDING)
                    .height(Length::Fixed(TAB_HEIGHT))
                    .style(chrome_tab_style(p, is_active, is_hovered, false))
                    .width(Length::Fill),
                )
                .on_enter(Message::Hosts(
                    crate::hosts::message::Message::NewTabHovered {
                        id: ntid,
                        entered: true,
                    },
                ))
                .on_exit(Message::Hosts(
                    crate::hosts::message::Message::NewTabHovered {
                        id: ntid,
                        entered: false,
                    },
                ))
                .interaction(iced::mouse::Interaction::Idle);

                let tab_element: Element<'_, Message> = cursor_tooltip_label_at(
                    tab_btn,
                    &raw_label,
                    p,
                    TooltipPosition::Bottom,
                    Some(350.0),
                )
                .into();

                let nt_target_width = if is_active {
                    (TAB_MIN_WIDTH + 40.0).min(MAX_TAB_WIDTH)
                } else {
                    TAB_MIN_WIDTH
                };
                let nt_tab_width = app
                    .terminal
                    .tab_width_anims
                    .get(&tid)
                    .copied()
                    .unwrap_or(nt_target_width);
                tabs_scroll_row = tabs_scroll_row.push(
                    Some(tid),
                    native_titlebar::scrolled_client_zone(
                        container(tab_element)
                            .width(Length::Fixed(nt_tab_width))
                            .clip(true)
                            .into(),
                    ),
                );
            }
        }
    }

    if !app.top_tabs_can_scroll {
        tabs_scroll_row =
            tabs_scroll_row.push(None, native_titlebar::scrolled_client_zone(add_btn()));
    }

    let wctl_btn = |icon: lucide_icons::Icon,
                    msg: Message,
                    hover_bg: Color,
                    hover_text: Color,
                    tooltip: &'static str|
     -> Element<'_, Message> {
        let text_dim = p.text_dim;
        let btn = button(text(char::from(icon)).font(LUCIDE_FONT).size(13))
            .on_press(msg)
            .padding(Padding::from([6.0, 10.0]))
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: hovered.then_some(Background::Color(hover_bg)),
                    border: Border {
                        radius: 6.0.into(),
                        width: 1.0,
                        color: if hovered {
                            hover_bg
                        } else {
                            Color::TRANSPARENT
                        },
                    },
                    text_color: if hovered { hover_text } else { text_dim },
                    ..button::Style::default()
                }
            })
            .cursor_interaction(mouse::Interaction::default());

        cursor_tooltip_label_at(btn, tooltip, p, TooltipPosition::Bottom, None).into()
    };

    let workspace_save_btn = wctl_btn(
        lucide_icons::Icon::Save,
        Message::WorkspaceSave,
        p.item_hover,
        p.text_pri,
        t_static(t!("topbar.tooltip_save_workspace")),
    );

    let sidebar_btn: Element<'_, Message> = {
        let text_dim = p.text_dim;
        let text_pri = p.text_pri;
        let btn = button(
            text(crate::core::fonts::my_icon::PANEL_RIGHT_OPEN)
                .font(MY_ICONS_FONT)
                .size(13),
        )
        .on_press(Message::ToggleRightSidebar)
        .padding(Padding::from([6.0, 10.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: hovered.then_some(Background::Color(p.item_hover)),
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if hovered {
                        p.item_hover
                    } else {
                        Color::TRANSPARENT
                    },
                },
                text_color: if hovered { text_pri } else { text_dim },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

        cursor_tooltip_label_at(
            btn,
            t_static(t!("topbar.tooltip_sidebar")),
            p,
            TooltipPosition::Bottom,
            None,
        )
        .into()
    };
    #[cfg(not(target_os = "macos"))]
    let minimize_btn = wctl_btn(
        lucide_icons::Icon::Minus,
        Message::WindowMinimize,
        p.item_hover,
        p.text_pri,
        t_static(t!("topbar.tooltip_minimize")),
    );
    #[cfg(not(target_os = "macos"))]
    let maximize_icon = if app.window.is_maximized {
        lucide_icons::Icon::Minimize2
    } else {
        lucide_icons::Icon::Square
    };
    #[cfg(not(target_os = "macos"))]
    let maximize_tooltip = if app.window.is_maximized {
        t_static(t!("topbar.tooltip_restore"))
    } else {
        t_static(t!("topbar.tooltip_maximize"))
    };
    #[cfg(not(target_os = "macos"))]
    let maximize_btn = native_titlebar::maximize_zone(wctl_btn(
        maximize_icon,
        Message::WindowMaximize,
        p.item_hover,
        p.text_pri,
        maximize_tooltip,
    ));
    #[cfg(not(target_os = "macos"))]
    let close_btn = wctl_btn(
        lucide_icons::Icon::X,
        Message::WindowClose,
        p.danger,
        Color::WHITE,
        t_static(t!("topbar.tooltip_close")),
    );

    // Horizontal scrollable area for terminal + new-tab tabs only.
    // Vaults, SFTP, and "+" stay fixed outside the scroll area.
    let scrollable_tabs = scrollable(container(tabs_scroll_row).width(Length::Shrink))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::hidden(),
        ))
        .style(move |_, status| scrollable_style(status, p))
        .id(iced::widget::Id::from(TOP_TABS_SCROLL_ID))
        .on_scroll(move |vp| Message::Scrolled(TOP_TABS_SCROLL_ID, vp))
        .width(Length::Fill);

    // Edge fade masks — gradient overlays at left/right edges to indicate
    // scrollable overflow. They are only shown when the content actually
    // overflows and the scroll position is not at the corresponding extreme.
    let edge_fade_width = 24.0;
    let fade_bg = p.top_bar;
    let left_fade = move |_: &Theme| container::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                .add_stop(0.0, fade_bg)
                .add_stop(1.0, Color { a: 0.0, ..fade_bg }),
        ))),
        ..container::Style::default()
    };
    let right_fade = move |_: &Theme| container::Style {
        background: Some(Background::Gradient(Gradient::Linear(
            iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                .add_stop(0.0, Color { a: 0.0, ..fade_bg })
                .add_stop(1.0, fade_bg),
        ))),
        ..container::Style::default()
    };

    let show_left_fade = app.top_tabs_can_scroll && app.top_tabs_scroll_offset > 0.01;
    let show_right_fade = app.top_tabs_can_scroll && app.top_tabs_scroll_offset < 0.99;

    let mut fade_children: Vec<Element<'_, Message>> = Vec::with_capacity(3);
    if show_left_fade {
        fade_children.push(
            container(
                Space::new()
                    .width(Length::Fixed(edge_fade_width))
                    .height(Length::Fill),
            )
            .width(Length::Fixed(edge_fade_width))
            .height(Length::Fill)
            .style(left_fade)
            .into(),
        );
    }
    if show_left_fade || show_right_fade {
        fade_children.push(Space::new().width(Length::Fill).height(Length::Fill).into());
    }
    if show_right_fade {
        fade_children.push(
            container(
                Space::new()
                    .width(Length::Fixed(edge_fade_width))
                    .height(Length::Fill),
            )
            .width(Length::Fixed(edge_fade_width))
            .height(Length::Fill)
            .style(right_fade)
            .into(),
        );
    }

    // Always wrap the scrollable in the Stack, even when no fade is shown:
    // iced stores the scrollable's offset by its position in the widget tree,
    // so conditionally adding/removing the wrapper would reset the scroll
    // position whenever tab mutations transiently clear `top_tabs_can_scroll`.
    // fade_children is empty when no fade is needed, and the empty overlay
    // row passes pointer events through to the scrollable below.
    let scrollable_with_fade: Element<'_, Message> = Stack::with_children(vec![
        mouse_area(scrollable_tabs)
            .on_scroll(Message::TopTabsWheel)
            .into(),
        row(fade_children)
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    ])
    .into();
    let scrollable_with_fade = native_titlebar::tab_strip_clip(scrollable_with_fade);

    let mut fixed_and_scroll_row = row![vaults_btn, sftp_btn, scrollable_with_fade]
        .spacing(6)
        .align_y(iced::Alignment::Center);

    // When tabs overflow, pin "+" to the right so it stays accessible.
    if app.top_tabs_can_scroll {
        fixed_and_scroll_row = fixed_and_scroll_row.push(native_titlebar::client_zone(add_btn()));
    }

    #[cfg(not(target_os = "macos"))]
    let trailing_controls: Element<'_, Message> = container({
        if app.active_terminal_session_id().is_some() {
            row![
                workspace_save_btn,
                sidebar_btn,
                minimize_btn,
                maximize_btn,
                close_btn
            ]
            .spacing(2)
            .align_y(iced::Alignment::Center)
        } else {
            row![minimize_btn, maximize_btn, close_btn]
                .spacing(2)
                .align_y(iced::Alignment::Center)
        }
    })
    .padding(Padding {
        top: 0.0,
        right: 6.0,
        bottom: 0.0,
        left: 0.0,
    })
    .align_y(iced::alignment::Vertical::Center)
    .into();

    #[cfg(target_os = "macos")]
    let trailing_controls: Element<'_, Message> = {
        let content: Element<'_, Message> = if app.active_terminal_session_id().is_some() {
            row![workspace_save_btn, sidebar_btn]
                .spacing(2)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            Space::new().into()
        };

        container(content)
            .padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 0.0,
            })
            .align_y(iced::alignment::Vertical::Center)
            .into()
    };

    let trailing_controls: Element<'_, Message> = native_titlebar::client_zone(trailing_controls);

    let drag_area = mouse_area(
        container(fixed_and_scroll_row.padding(Padding {
            top: 6.0,
            right: 4.0,
            bottom: 6.0,
            left: topbar_left_padding,
        }))
        .width(Length::Fill),
    )
    .on_double_click(Message::WindowMaximize);

    // Windows owns blank top-bar space through WM_NCHITTEST now. On the other
    // platforms iced still starts the drag on press.
    #[cfg(not(target_os = "windows"))]
    let drag_area = drag_area.on_press(Message::WindowDrag);

    let topbar_row = row![drag_area, trailing_controls]
        .align_y(iced::Alignment::Center)
        .height(Length::Shrink);

    let topbar_background = p.top_bar;

    let topbar: Element<'_, Message> = container(topbar_row)
        .width(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(topbar_background)),
            ..Default::default()
        })
        .into();

    native_titlebar::caption_zone(topbar)
}
