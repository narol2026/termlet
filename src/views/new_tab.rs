use iced::widget::{button, column, container, text};
use iced::{Background, Border, Element, Length, Padding, Theme, mouse};

use crate::app::{App, DraftId};
use crate::core::data::HostId;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::search_input;
use rust_i18n::t;

pub fn view_new_tab(app: &App, id: DraftId) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let query = app
        .terminal
        .drafts
        .get(&id)
        .map_or("", |d| d.query.as_str());
    let input_id = format!("new_tab_input_{id}");
    let search = search_input(
        t_static(t!("new_tab.placeholder")),
        query,
        &p,
        Some(iced::widget::Id::from(input_id)),
        |v0| Message::Hosts(crate::hosts::message::Message::NewTabQueryChanged(v0)),
        Message::Hosts(crate::hosts::message::Message::NewTabConnect),
    );

    let hosts = new_tab_hosts(app, query);
    let active = app
        .terminal
        .drafts
        .get(&id)
        .and_then(|draft| draft.cursor.selected_key().copied());
    let recent_section = view_connections(app, query, hosts, active);

    let search_area = search;

    container(
        column![
            // Top border only
            container(text(""))
                .width(Length::Fill)
                .height(1)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(p.border)),
                    ..Default::default()
                }),
            container(
                column![search_area, recent_section]
                    .spacing(20)
                    .width(Length::Fill),
            )
            .padding(Padding::from([24.0, 28.0])),
        ]
        .spacing(0)
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(p.content_bg)),
        ..Default::default()
    })
    .into()
}

fn divider(p: theme::ThemePalette) -> Element<'static, Message> {
    container(text(""))
        .width(Length::Fill)
        .height(1)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..Default::default()
        })
        .into()
}

/// The ordered host list shown in a new-tab window for a given query.
/// Shared by the view (rendering) and the update handlers (ArrowUp/ArrowDown
/// selection + Enter), so keyboard navigation and the visual list stay in sync.
pub fn new_tab_hosts<'a>(app: &'a App, query: &str) -> Vec<&'a crate::core::data::Host> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        let recent = app.hosts.recent();
        if recent.is_empty() {
            app.hosts.sorted_by_recency()
        } else {
            recent
        }
    } else {
        let mut filtered = app
            .hosts
            .list
            .iter()
            .filter(|h| {
                h.name.to_lowercase().contains(&q)
                    || h.hostname.to_lowercase().contains(&q)
                    || h.username.to_lowercase().contains(&q)
            })
            .collect::<Vec<_>>();
        app.hosts.sort_by_recency(&mut filtered);
        filtered
    }
}

fn view_connections<'a>(
    app: &'a App,
    query: &'a str,
    hosts: Vec<&'a crate::core::data::Host>,
    active: Option<HostId>,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let q = query.trim().to_lowercase();

    let section_label: &str = if q.is_empty() {
        if app.hosts.recent().is_empty() {
            t_static(t!("new_tab.all_connections"))
        } else {
            t_static(t!("new_tab.recent_connections"))
        }
    } else {
        t_static(t!("new_tab.search_results"))
    };

    let mut col = column![].spacing(12).width(Length::Fill);

    // Saved hosts section
    if !hosts.is_empty() || !q.is_empty() {
        let mut host_col = column![text(section_label).size(14).color(p.text_sec)].spacing(12);

        if hosts.is_empty() {
            host_col = host_col.push(
                container(
                    text(t_static(t!("new_tab.no_matching_hosts")))
                        .size(12)
                        .color(p.text_sec),
                )
                .padding(Padding::from([12.0, 14.0]))
                .width(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    background: Some(Background::Color(p.card_bg)),
                    border: Border {
                        radius: 6.0.into(),
                        color: p.border,
                        width: 1.0,
                    },
                    ..Default::default()
                }),
            );
        } else {
            let count = hosts.len();
            let mut list = column![].width(Length::Fill);

            for (i, host) in hosts.into_iter().enumerate() {
                let id = host.id;
                let radius: iced::border::Radius = if count == 1 {
                    6.0.into()
                } else if i == 0 {
                    iced::border::Radius {
                        top_left: 6.0,
                        top_right: 6.0,
                        bottom_right: 0.0,
                        bottom_left: 0.0,
                    }
                } else if i == count - 1 {
                    iced::border::Radius {
                        top_left: 0.0,
                        top_right: 0.0,
                        bottom_right: 6.0,
                        bottom_left: 6.0,
                    }
                } else {
                    0.0.into()
                };

                let is_selected = active == Some(host.id);

                let row_btn = button(
                    column![
                        text(host.name.clone())
                            .size(13)
                            .color(p.text_pri)
                            .width(Length::Fill)
                            .ellipsis(text::Ellipsis::End)
                            .wrapping(text::Wrapping::None),
                        text(format!("{}@{}", host.username, host.hostname))
                            .size(11)
                            .color(p.text_sec)
                            .width(Length::Fill)
                            .ellipsis(text::Ellipsis::End)
                            .wrapping(text::Wrapping::None),
                    ]
                    .spacing(2),
                )
                .on_press(Message::Hosts(crate::hosts::message::Message::ConnectHost(
                    id,
                )))
                .padding(Padding::from([10.0, 14.0]))
                .width(Length::Fill)
                .style(move |_: &Theme, status| {
                    let hovered = matches!(status, button::Status::Hovered);
                    let highlighted = hovered || is_selected;
                    button::Style {
                        background: Some(Background::Color(if highlighted {
                            p.item_hover
                        } else {
                            p.surface
                        })),
                        text_color: p.text_pri,
                        border: Border {
                            radius,
                            ..Border::default()
                        },
                        ..button::Style::default()
                    }
                })
                .cursor_interaction(mouse::Interaction::default());

                list = list.push(row_btn);
                if i < count - 1 {
                    list = list.push(divider(p));
                }
            }

            host_col = host_col.push(list);
        }

        col = col.push(container(host_col).width(Length::Fill).padding(12).style(
            move |_: &Theme| container::Style {
                background: Some(Background::Color(p.card_bg)),
                border: Border {
                    radius: 8.0.into(),
                    color: p.border,
                    width: 1.0,
                },
                ..Default::default()
            },
        ));
    } else {
        // Empty state: no query, no hosts at all
        let mut host_col = column![text(section_label).size(14).color(p.text_sec)].spacing(12);
        host_col = host_col.push(
            container(
                text(t_static(t!("new_tab.no_hosts_configured")))
                    .size(12)
                    .color(p.text_sec),
            )
            .padding(Padding::from([12.0, 14.0]))
            .width(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.card_bg)),
                border: Border {
                    radius: 6.0.into(),
                    color: p.border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        );
        col = col.push(host_col);
    }

    container(col).width(Length::Fill).into()
}
