use iced::widget::{Space, button, column, container, mouse_area, row, text};
use iced::{
    Alignment, Background, Border, Color, Element, Font, Length, Padding, Point, Theme, mouse,
};

use crate::app::App;
use crate::core::context_menu::ContextMenuAction as Action;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::{TooltipPosition, cursor_tooltip_label_at, styled_scrollable};
use rust_i18n::t;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub const SFTP_SIDEBAR_SCROLL_ID: &str = "sftp_sidebar_scroll";

pub const SIDEBAR_EXPANDED_WIDTH: f32 = 192.0;
pub const SIDEBAR_COLLAPSED_WIDTH: f32 = 60.0;

fn centered_sidebar_icon<'a>(icon: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(icon)
        .width(Length::Fixed(crate::views::ITEM_HEIGHT))
        .height(Length::Fixed(crate::views::ITEM_HEIGHT))
        .center_x(crate::views::ITEM_HEIGHT)
        .center_y(crate::views::ITEM_HEIGHT)
        .into()
}

fn sidebar_item_button_style(
    p: theme::ThemePalette,
    is_active: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: if is_active {
                Some(Background::Color(p.nav_active))
            } else if hovered {
                Some(Background::Color(Color {
                    a: 0.5,
                    ..p.nav_active
                }))
            } else {
                None
            },
            text_color: if is_active || hovered {
                p.text_pri
            } else {
                p.text_sec
            },
            border: Border {
                radius: 8.0.into(),
                width: if p.show_border && (is_active || hovered) {
                    1.0
                } else {
                    0.0
                },
                color: p.border,
            },
            ..button::Style::default()
        }
    }
}

fn hover_item_button_style(
    p: theme::ThemePalette,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        button::Style {
            background: hovered.then_some(Background::Color(p.item_hover)),
            border: Border {
                radius: 8.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

pub fn view_sftp_sidebar(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let active_id = app.sftp.active_session;
    // Keep collapsed view during expand animation — text labels only appear
    // after the sidebar finishes sliding out.
    let expanding =
        app.sftp.sidebar_show && !app.sftp.sidebar_closing && app.sftp.sidebar_anim < 1.0;
    let collapsed = (!app.sftp.sidebar_show && !app.sftp.sidebar_closing) || expanding;

    // Show icon based on target state: if expanded (or expanding), show close icon;
    // if collapsed (or collapsing), show open icon.
    let toggle_icon = if app.sftp.sidebar_show && !app.sftp.sidebar_closing {
        lucide_icons::Icon::PanelRightClose
    } else {
        lucide_icons::Icon::PanelRightOpen
    };
    let centered_icon = centered_sidebar_icon(
        text(char::from(toggle_icon))
            .font(LUCIDE_FONT)
            .size(14)
            .color(p.text_sec),
    );

    let toggle_btn: Element<'_, Message> = button(centered_icon)
        .on_press(Message::Sftp(
            crate::sftp::message::Message::SftpToggleSidebar,
        ))
        .padding(0)
        .style(sidebar_item_button_style(p, false))
        .cursor_interaction(mouse::Interaction::default())
        .into();

    let session_entries: Vec<Element<'_, Message>> = app
        .sftp
        .sessions
        .iter()
        .map(|session| {
            let is_active = session.id == active_id;
            let can_close = app.sftp.sessions.len() > 1;

            // Show server icon when connected to a remote host, hard-drive for local
            let icon_kind =
                if session.state.right.pane.connected || session.state.left.pane.connected {
                    lucide_icons::Icon::Server
                } else {
                    lucide_icons::Icon::HardDrive
                };
            let icon_color = if is_active { p.text_pri } else { p.text_sec };
            let icon = container(
                text(char::from(icon_kind))
                    .font(LUCIDE_FONT)
                    .size(14)
                    .color(icon_color),
            )
            .width(18)
            .center_x(18);

            if collapsed {
                // Collapsed: icon-only button with tooltip
                let centered_icon = centered_sidebar_icon(icon);
                let btn = button(centered_icon)
                    .on_press(Message::Sftp(crate::sftp::message::Message::SftpSwitchTab(
                        session.id,
                    )))
                    .padding(0)
                    .cursor_interaction(mouse::Interaction::default())
                    .style(sidebar_item_button_style(p, is_active));
                let btn: Element<'_, Message> = mouse_area(btn)
                    .on_right_press(Message::Sftp(
                        crate::sftp::message::Message::SftpSidebarContextMenuOpen {
                            session_id: session.id,
                        },
                    ))
                    .into();
                cursor_tooltip_label_at(btn, &session.label, p, TooltipPosition::Right, Some(200.0))
                    .into()
            } else {
                // Expanded: icon + label + optional close
                let label = text(&session.label)
                    .size(14)
                    .color(icon_color)
                    .width(Length::Fill)
                    .ellipsis(text::Ellipsis::End)
                    .wrapping(text::Wrapping::None);

                let close_btn: Element<'_, Message> = if can_close {
                    button(
                        text(char::from(lucide_icons::Icon::X))
                            .font(LUCIDE_FONT)
                            .size(12)
                            .line_height(1.0)
                            .width(Length::Fixed(16.0))
                            .height(Length::Fixed(16.0))
                            .center()
                            .color(p.text_dim),
                    )
                    .on_press(Message::Sftp(crate::sftp::message::Message::SftpCloseTab(
                        session.id,
                    )))
                    .padding(Padding::from([2.0, 2.0]))
                    .style(move |_theme: &Theme, status| {
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
                    .into()
                } else {
                    Space::new()
                        .width(Length::Shrink)
                        .height(Length::Shrink)
                        .into()
                };

                let tab_btn = button(
                    container(
                        row![icon, label, close_btn]
                            .spacing(6)
                            .align_y(Alignment::Center),
                    )
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::Sftp(crate::sftp::message::Message::SftpSwitchTab(
                    session.id,
                )))
                .width(Length::Fill)
                .height(Length::Fixed(crate::views::ITEM_HEIGHT))
                .cursor_interaction(mouse::Interaction::default())
                .padding(Padding {
                    top: 0.0,
                    right: 8.0,
                    bottom: 0.0,
                    left: 12.0,
                })
                .style(sidebar_item_button_style(p, is_active));

                let tab_btn_element = mouse_area(tab_btn).on_right_press(Message::Sftp(
                    crate::sftp::message::Message::SftpSidebarContextMenuOpen {
                        session_id: session.id,
                    },
                ));
                cursor_tooltip_label_at(
                    tab_btn_element,
                    &session.label,
                    p,
                    TooltipPosition::Right,
                    Some(200.0),
                )
                .into()
            }
        })
        .collect();

    let session_list = column(session_entries).spacing(2);

    let add_btn: Element<'_, Message> = if collapsed {
        let plus_icon = text(char::from(lucide_icons::Icon::Plus))
            .font(LUCIDE_FONT)
            .size(16)
            .color(p.text_sec);
        let centered_icon = centered_sidebar_icon(plus_icon);
        let btn = button(centered_icon)
            .on_press(Message::Sftp(crate::sftp::message::Message::SftpNewTab))
            .padding(0)
            .style(hover_item_button_style(p))
            .cursor_interaction(mouse::Interaction::default());
        cursor_tooltip_label_at(
            btn,
            t_static(t!("sftp.new_sftp")),
            p,
            TooltipPosition::Right,
            Some(200.0),
        )
        .into()
    } else {
        button(
            container(
                row![
                    text(char::from(lucide_icons::Icon::Plus))
                        .font(LUCIDE_FONT)
                        .size(16)
                        .color(p.text_sec),
                    text(t_static(t!("sftp.new_sftp")))
                        .size(13)
                        .color(p.text_sec),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Alignment::Center),
        )
        .on_press(Message::Sftp(crate::sftp::message::Message::SftpNewTab))
        .width(Length::Fill)
        .height(Length::Fixed(crate::views::ITEM_HEIGHT))
        .padding(Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 12.0,
        })
        .style(hover_item_button_style(p))
        .cursor_interaction(mouse::Interaction::default())
        .into()
    };

    let eased = crate::ui::helpers::ease_out_cubic(app.sftp.sidebar_anim);
    let sidebar_width =
        SIDEBAR_COLLAPSED_WIDTH + (SIDEBAR_EXPANDED_WIDTH - SIDEBAR_COLLAPSED_WIDTH) * eased;

    let scrollable_content: Element<'_, Message> = if collapsed {
        column![session_list, add_btn]
            .spacing(4)
            .align_x(Alignment::Center)
            .into()
    } else {
        column![session_list, add_btn].spacing(4).into()
    };

    let scrollbar_visible = app.is_scrollbar_visible(SFTP_SIDEBAR_SCROLL_ID);

    let inner = if collapsed {
        column![
            styled_scrollable(scrollable_content, p, scrollbar_visible)
                .id(iced::widget::Id::new(SFTP_SIDEBAR_SCROLL_ID))
                .on_scroll(move |v| Message::Scrolled(SFTP_SIDEBAR_SCROLL_ID, v))
                .height(Length::Fill),
            toggle_btn,
        ]
        .spacing(4)
        .align_x(Alignment::Center)
    } else {
        column![
            styled_scrollable(scrollable_content, p, scrollbar_visible)
                .id(iced::widget::Id::new(SFTP_SIDEBAR_SCROLL_ID))
                .on_scroll(move |v| Message::Scrolled(SFTP_SIDEBAR_SCROLL_ID, v))
                .height(Length::Fill),
            row![Space::new().width(Length::Fill), toggle_btn],
        ]
        .spacing(4)
    };

    container(inner)
        .width(sidebar_width)
        .clip(true)
        .padding(Padding {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        })
        .height(Length::Fill)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(p.sidebar_bg)),
            ..container::Style::default()
        })
        .into()
}

/// Build the sidebar tab context menu overlay at the given position.
pub fn view_sidebar_context_menu_at(
    menu: &crate::sftp::SftpSidebarContextMenu,
    p: crate::core::theme::ThemePalette,
    x: f32,
    y: f32,
) -> Element<'_, Message> {
    const MENU_WIDTH: f32 = 120.0;

    let session_id = menu.session_id;
    let can_close = menu.can_close;
    let palette = p;
    let actions = menu.actions();
    let focused = menu.cursor.selected_or_first(&actions).copied();

    crate::ui::overlay::context_menu_frame(
        Point::new(x, y),
        move || {
            let menu_item = |action: Action,
                             icon: lucide_icons::Icon,
                             label: &'static str,
                             msg: Message,
                             destructive: bool| {
                crate::core::widgets::menu_item(
                    icon,
                    label,
                    msg,
                    destructive,
                    focused == Some(action),
                    palette,
                )
            };

            let mut items: Vec<Element<'_, Message>> = vec![menu_item(
                Action::SftpSidebarDuplicate,
                lucide_icons::Icon::Copy,
                t_static(t!("sftp.duplicate")),
                Message::Sftp(crate::sftp::message::Message::SftpDuplicateTab(session_id)),
                false,
            )];
            if can_close {
                items.push(menu_item(
                    Action::SftpSidebarClose,
                    lucide_icons::Icon::X,
                    t_static(t!("term_menu.close")),
                    Message::Sftp(crate::sftp::message::Message::SftpCloseTab(session_id)),
                    true,
                ));
            }
            items
        },
        MENU_WIDTH,
        Message::Sftp(crate::sftp::message::Message::SftpSidebarContextMenuClose),
        p,
    )
}
