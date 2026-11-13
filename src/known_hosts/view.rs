use iced::widget::{Space, column, container, mouse_area, responsive, row, sensor, text};
use iced::{Alignment, Color, Element, Font, Length, Padding};

use crate::app::App;
use crate::core::collection::{CARD_HEIGHT, ROW_GAP};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{Host, KnownHost};
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::{
    card_hover_btn, card_info, card_wrapper, collection_layout, collection_rows, filter_input,
    form_header, form_panel, icon_box, labeled_readonly, labeled_readonly_mono, styled_btn,
    styled_scrollable, toolbar_icon_btn,
};
use crate::ui::confirm_dialog::delete_confirm_dialog;
use crate::ui::cursor_area::CursorArea;
use crate::ui::slide::slide_panel_from_right;
use rust_i18n::t;

pub const KNOWN_HOSTS_SCROLL_ID: &str = "known_hosts_scroll";
pub(crate) const KNOWN_HOSTS_GRID_TOP_PADDING: f32 = 4.0;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_known_hosts(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let search = filter_input(
        t_static(t!("known_hosts.search")),
        &app.known_hosts.search,
        &p,
        |v0| Message::KnownHosts(crate::known_hosts::message::Message::KnownHostSearchChanged(v0)),
    );

    let view_icon = if app.known_hosts.grid_view {
        lucide_icons::Icon::List
    } else {
        lucide_icons::Icon::LayoutGrid
    };
    let view_btn = toolbar_icon_btn(
        view_icon,
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostToggleView),
        p,
    );

    let toolbar = container(
        row![search, Space::new().width(8), view_btn]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding(Padding::from([13.0, 20.0]))
    .width(Length::Fill);

    let body = view_known_host_grid(app);

    let left_content = column![
        toolbar,
        container(body).width(Length::Fill).height(Length::Fill)
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill);

    let page: Element<'_, Message> = if app.known_hosts.detail_open
        && let Some(&detail_id) = app.known_hosts.cursor.selected_key()
    {
        let kh = app
            .known_hosts
            .entries
            .iter()
            .find(|k| k.line_number == detail_id);
        if let Some(kh) = kh {
            let matched = app.hosts.list.iter().find(|h| {
                h.hostname.to_lowercase() == kh.hostname.to_lowercase() && h.port == kh.port
            });
            row![
                left_content,
                slide_panel_from_right(
                    view_known_host_detail_panel(kh, matched, app),
                    320.0,
                    true,
                    app.known_hosts.detail_anim,
                ),
            ]
            .spacing(0)
            .height(Length::Fill)
            .into()
        } else {
            left_content.into()
        }
    } else {
        left_content.into()
    };

    mouse_area(page)
        .on_move(|v0| {
            Message::KnownHosts(crate::known_hosts::message::Message::KnownHostViewCursorMoved(v0))
        })
        .into()
}

fn view_known_host_grid(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let filtered = app.known_hosts.filtered(&app.hosts.list);

    if filtered.is_empty() {
        let msg = if app.known_hosts.search.is_empty() {
            t_static(t!("known_hosts.empty"))
        } else {
            t_static(t!("known_hosts.no_match"))
        };
        return container(text(msg).size(14).color(p.text_sec))
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into();
    }

    let grid_view = app.known_hosts.grid_view;
    let hovered_id = app.known_hosts.hovered_id;

    styled_scrollable(
        CursorArea::for_target(
            container(responsive(move |size| {
                let layout = collection_layout(size.width, grid_view, 280.0, 6);
                let grid = collection_rows(
                    filtered.iter().map(|(known_host, matched)| {
                        let hovered = hovered_id == Some(known_host.line_number);
                        let selected =
                            app.known_hosts.cursor.selected_key() == Some(&known_host.line_number);
                        if grid_view {
                            view_known_host_card(
                                known_host,
                                *matched,
                                hovered,
                                selected,
                                layout.item_width,
                                p,
                            )
                        } else {
                            view_known_host_list_item(
                                known_host,
                                *matched,
                                hovered,
                                selected,
                                layout.item_width,
                                p,
                            )
                        }
                    }),
                    layout,
                    CARD_HEIGHT,
                    ROW_GAP,
                );
                let columns = layout.columns;

                sensor(grid)
                    .on_resize(move |_| Message::CursorColumns(CursorTarget::KnownHosts, columns))
                    .into()
            }))
            .padding(Padding {
                top: KNOWN_HOSTS_GRID_TOP_PADDING,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            })
            .width(Length::Fill),
            CursorTarget::KnownHosts,
        ),
        p,
        app.is_scrollbar_visible(KNOWN_HOSTS_SCROLL_ID),
    )
    .id(iced::widget::Id::from(KNOWN_HOSTS_SCROLL_ID))
    .on_scroll(move |v| Message::Scrolled(KNOWN_HOSTS_SCROLL_ID, v))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_known_host_card<'a>(
    kh: &'a KnownHost,
    matched: Option<&'a Host>,
    hovered: bool,
    selected: bool,
    card_width: f32,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    let id = kh.line_number;

    let icon = icon_box(
        text(char::from(lucide_icons::Icon::Fingerprint))
            .font(LUCIDE_FONT)
            .size(20)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let line1 = if let Some(host) = matched {
        host.name.clone()
    } else {
        kh.hostname.clone()
    };
    let line2 = format!("{}:{}", kh.hostname, kh.port);
    let info = card_info(&line1, line2, card_width, hovered, p);

    let actions: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Info,
            Message::KnownHosts(crate::known_hosts::message::Message::KnownHostShowDetails(
                id,
            )),
            p,
        )
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let content = row![icon, info, actions]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(Padding::from([10.0, 12.0]));

    card_wrapper(
        content.into(),
        hovered,
        selected,
        None,
        None,
        10.0,
        true,
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostHoverEnter(
            id,
        )),
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostHoverLeave(
            id,
        )),
        Some(Message::KnownHosts(
            crate::known_hosts::message::Message::KnownHostSelect(id),
        )),
        None,
        Some(Message::KnownHosts(
            crate::known_hosts::message::Message::KnownHostContextMenuOpen(id),
        )),
        p,
    )
}

fn view_known_host_list_item<'a>(
    kh: &'a KnownHost,
    matched: Option<&'a Host>,
    hovered: bool,
    selected: bool,
    card_width: f32,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    let id = kh.line_number;
    let info_width = card_width - 84.0 - if hovered { 28.0 } else { 0.0 };

    let name_str = if let Some(host) = matched {
        host.name.clone()
    } else {
        kh.hostname.clone()
    };

    let name: Element<'_, Message> = text(name_str)
        .size(14.0)
        .color(p.text_pri)
        .width(Length::Fixed(info_width))
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None)
        .into();

    let key_icon = icon_box(
        text(char::from(lucide_icons::Icon::Fingerprint))
            .font(LUCIDE_FONT)
            .size(20)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let details: Element<'_, Message> = text(format!("{}:{}", kh.hostname, kh.port))
        .size(12.0)
        .color(p.text_sec)
        .width(Length::Fixed(info_width))
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None)
        .into();

    let info_col = column![name, details].spacing(3);

    let details_btn: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Info,
            Message::KnownHosts(crate::known_hosts::message::Message::KnownHostShowDetails(
                kh.line_number,
            )),
            p,
        )
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let row_content = row![
        key_icon,
        Space::new().width(12),
        info_col,
        Space::new().width(Length::Fill),
        details_btn,
    ]
    .align_y(Alignment::Center)
    .padding(Padding::from([10.0, 14.0]))
    .width(Length::Fill);

    card_wrapper(
        row_content.into(),
        hovered,
        selected,
        None,
        None,
        8.0,
        false,
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostHoverEnter(
            id,
        )),
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostHoverLeave(
            id,
        )),
        Some(Message::KnownHosts(
            crate::known_hosts::message::Message::KnownHostSelect(id),
        )),
        None,
        Some(Message::KnownHosts(
            crate::known_hosts::message::Message::KnownHostContextMenuOpen(id),
        )),
        p,
    )
}

pub fn view_known_host_delete_confirm(
    app: &App,
    p: theme::ThemePalette,
) -> Element<'static, Message> {
    let Some(id) = app.known_hosts.delete_pending else {
        return text("").into();
    };
    let label = app
        .known_hosts
        .entries
        .iter()
        .find(|k| k.line_number == id)
        .map_or(t_static(t!("known_hosts.delete_fallback")), |k| &k.hostname);
    let body = t!("known_hosts.delete_confirm_body", name = label);
    delete_confirm_dialog(
        t_static(t!("known_hosts.delete_confirm_title")),
        body,
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostDeleteCancel),
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostDelete(id)),
        &p,
        app.confirm_dialog_anim,
    )
}

pub fn view_known_host_context_menu(
    menu: &crate::known_hosts::KnownHostContextMenu,
    p: theme::ThemePalette,
) -> Element<'_, Message> {
    const MENU_WIDTH: f32 = 140.0;
    use crate::core::context_menu::ContextMenuAction as Action;

    let focused = menu
        .cursor
        .selected_or_first(crate::known_hosts::KnownHostContextMenu::actions())
        .copied();

    crate::ui::overlay::context_menu_frame(
        menu.position,
        move || {
            vec![
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Info,
                    t_static(t!("known_hosts.details")),
                    Message::KnownHosts(
                        crate::known_hosts::message::Message::KnownHostShowDetails(menu.id),
                    ),
                    false,
                    focused == Some(Action::KnownHostDetails),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Trash2,
                    t_static(t!("dialog.delete")),
                    Message::KnownHosts(
                        crate::known_hosts::message::Message::KnownHostDeleteRequest(menu.id),
                    ),
                    true,
                    focused == Some(Action::KnownHostDelete),
                    p,
                ),
            ]
        },
        MENU_WIDTH,
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostContextMenuClose),
        p,
    )
}

fn view_known_host_detail_panel<'a>(
    kh: &'a KnownHost,
    matched: Option<&'a Host>,
    app: &'a App,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let title_text = if let Some(host) = matched {
        host.name.clone()
    } else {
        kh.hostname.clone()
    };
    let header = form_header(
        title_text,
        Message::KnownHosts(crate::known_hosts::message::Message::KnownHostHideDetails),
        p,
    );

    let port_str = kh.port.to_string();
    let line_number_str = kh.line_number.to_string();

    let mut content = column![
        header,
        labeled_readonly(
            t_static(t!("known_hosts.hostname_label")),
            kh.hostname.clone(),
            &p
        ),
        labeled_readonly(t_static(t!("known_hosts.port_label")), port_str, &p),
        labeled_readonly(
            t_static(t!("known_hosts.key_type_label")),
            kh.key_type.clone(),
            &p
        ),
        labeled_readonly_mono(
            t_static(t!("known_hosts.fingerprint_label")),
            kh.fingerprint.clone(),
            &p
        ),
    ]
    .spacing(14);

    content = content.push(labeled_readonly_mono(
        t_static(t!("known_hosts.public_key_label")),
        kh.public_key.clone(),
        &p,
    ));
    content = content.push(labeled_readonly(
        t_static(t!("known_hosts.line_number_label")),
        line_number_str,
        &p,
    ));
    content = content.push(labeled_readonly(
        t_static(t!("known_hosts.pattern_label")),
        kh.raw_pattern.clone(),
        &p,
    ));

    if let Some(ref marker) = kh.marker {
        content = content.push(labeled_readonly(
            t_static(t!("known_hosts.marker_label")),
            marker.clone(),
            &p,
        ));
    }

    let delete_btn = styled_btn(
        t_static(t!("dialog.delete")),
        Some(Message::KnownHosts(
            crate::known_hosts::message::Message::KnownHostDeleteRequest(kh.line_number),
        )),
        p.danger,
        Color::WHITE,
        p,
    );

    let spacer = Space::new().height(Length::Fill);

    let panel_content = column![content, spacer, delete_btn].spacing(10).padding(20);

    form_panel(panel_content, 320.0, p)
}
