use iced::widget::{Space, column, container, mouse_area, responsive, row, sensor, text};
use iced::{Color, Element, Font, Length, Padding};

use crate::app::App;
use crate::core::collection::{CARD_HEIGHT, ROW_GAP};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{Host, HostTagFilter};
use crate::core::dropdown::{DropdownTarget, TransferMenuAction};
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::{
    ActionMenuItem, SelectOption, action_menu_dropdown, card_hover_btn, card_info, card_wrapper,
    collection_layout, collection_rows, icon_box, search_input, select_dropdown, styled_scrollable,
    tag_select_option, text_width_estimate, toolbar_icon_btn, toolbar_outline_btn,
    toolbar_primary_btn,
};
use crate::ui::confirm_dialog::delete_confirm_dialog;
use crate::ui::cursor_area::CursorArea;
use crate::ui::slide::slide_panel_from_right;
use rust_i18n::t;

pub const VAULTS_HOSTS_SCROLL_ID: &str = "vaults_hosts_scroll";
pub const VAULTS_HOSTS_SEARCH_ID: &str = "vaults_hosts_search";
pub(crate) const HOSTS_GRID_TOP_PADDING: f32 = 4.0;

use super::form_view::view_form;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_hosts_page(app: &App) -> Element<'_, Message> {
    let toolbar = view_toolbar(app);
    let grid = view_host_grid(app);

    let left_content = column![
        toolbar,
        container(grid).width(Length::Fill).height(Length::Fill)
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill);

    let page: Element<'_, Message> = row![
        left_content,
        slide_panel_from_right(
            view_form(app),
            320.0,
            app.hosts.form.visible,
            app.hosts.form_anim
        ),
    ]
    .spacing(0)
    .height(Length::Fill)
    .into();

    mouse_area(page)
        .on_move(|v0| Message::Hosts(crate::hosts::message::Message::HostsViewCursorMoved(v0)))
        .into()
}

fn view_toolbar(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let search = search_input(
        t_static(t!("hosts.search")),
        &app.hosts.search_query,
        &p,
        Some(iced::widget::Id::from(VAULTS_HOSTS_SEARCH_ID)),
        |v0| Message::Hosts(crate::hosts::message::Message::SearchChanged(v0)),
        Message::Noop,
    );

    let search_row = search;

    let new_btn = toolbar_primary_btn(
        lucide_icons::Icon::Plus,
        t_static(t!("hosts.new_host")),
        Message::Hosts(crate::hosts::message::Message::ShowForm),
        p,
    );
    let term_btn = toolbar_outline_btn(
        lucide_icons::Icon::Terminal,
        t_static(t!("hosts.terminal")),
        Message::Hosts(crate::hosts::message::Message::OpenLocalTerminal),
        p,
    );
    // The Export/Import pair is folded into a single dropdown.
    let transfer_btn = view_transfer_dropdown(app, p);
    let view_icon = if app.hosts.grid_view {
        lucide_icons::Icon::List
    } else {
        lucide_icons::Icon::LayoutGrid
    };
    let view_btn = toolbar_icon_btn(
        view_icon,
        Message::Hosts(crate::hosts::message::Message::ToggleView),
        p,
    );

    let action_row = row![
        new_btn,
        term_btn,
        transfer_btn,
        Space::new().width(Length::Fill),
        view_tag_filter(app, p),
        view_btn
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    container(column![search_row, action_row].spacing(10))
        .padding(Padding::from([13.0, 20.0]))
        .width(Length::Fill)
        .into()
}

/// Toolbar dropdown that folds host Export / Import into a single control.
/// Rendered through the shared `select_dropdown` in action-menu mode so its
/// trigger and popup rows match the host tag filter (and every other dropdown).
fn view_transfer_dropdown(app: &App, p: theme::ThemePalette) -> Element<'static, Message> {
    action_menu_dropdown(
        t_static(t!("hosts.transfer_menu_btn")),
        app.active_dropdown == Some(DropdownTarget::HostsTransferMenu),
        (app.active_dropdown == Some(DropdownTarget::HostsTransferMenu))
            .then(|| {
                app.hosts
                    .transfer_menu
                    .active_index(TransferMenuAction::all())
            })
            .flatten(),
        DropdownTarget::HostsTransferMenu,
        Message::Hosts(crate::hosts::message::Message::HostTransferMenuToggle),
        vec![
            ActionMenuItem {
                icon: lucide_icons::Icon::Download,
                label: t_static(t!("hosts.export_btn")).to_string(),
                on_select: Message::Hosts(crate::hosts::message::Message::HostExportRequest),
            },
            ActionMenuItem {
                icon: lucide_icons::Icon::Upload,
                label: t_static(t!("hosts.import_btn")).to_string(),
                on_select: Message::Hosts(crate::hosts::message::Message::HostImportRequest),
            },
        ],
        p,
    )
}

/// Toolbar dropdown that filters the host list to a single tag (or All),
/// shown next to the list/card view toggle.
fn view_tag_filter(app: &App, p: theme::ThemePalette) -> Element<'static, Message> {
    const CHROME: f32 = 76.0; // leading dot/icon + check + chevron + spacing + padding

    const MAX_WIDTH: f32 = 240.0;
    let all = t_static(t!("hosts.filter_all"));
    let filter = &app.hosts.tag_filter;

    let current: String = match filter {
        HostTagFilter::All => all.to_string(),
        HostTagFilter::Tag(name) => name.clone(),
    };

    let tags = app.hosts.all_tags();

    // Size the trigger (and thus the anchored popup) to the widest option so
    // the button doesn't jump between tags and long tags aren't ellipsized.
    let max_label_w = tags
        .iter()
        .map(|t| text_width_estimate(t, 7.0))
        .fold(text_width_estimate(all, 7.0), f32::max);
    let trigger_width = Length::Fixed((max_label_w + CHROME).min(MAX_WIDTH));

    let items = crate::cursor::host_tag_filter_ids(app);
    let focused = app.hosts.tag_filter_picker.active(&items).cloned();
    let options = items
        .into_iter()
        .map(|item| {
            let is_focused = focused.as_ref() == Some(&item);
            let is_active = *filter == item;
            match item {
                HostTagFilter::All => SelectOption {
                    label: all.to_string(),
                    subtitle: None,
                    active: is_active,
                    focused: is_focused,
                    on_select: Message::Hosts(crate::hosts::message::Message::HostTagFilterSelect(
                        HostTagFilter::All,
                    )),
                    leading: None,
                },
                HostTagFilter::Tag(tag) => {
                    let mut option = tag_select_option(
                        &tag,
                        is_active,
                        Message::Hosts(crate::hosts::message::Message::HostTagFilterSelect(
                            HostTagFilter::Tag(tag.clone()),
                        )),
                    );
                    option.focused = is_focused;
                    option
                }
            }
        })
        .collect();

    let leading = Some(
        text(char::from(lucide_icons::Icon::Tag))
            .font(LUCIDE_FONT)
            .size(13)
            .color(p.text_sec)
            .into(),
    );

    select_dropdown(
        &current,
        leading,
        options,
        app.active_dropdown == Some(DropdownTarget::HostsTagFilter),
        DropdownTarget::HostsTagFilter,
        Message::Hosts(crate::hosts::message::Message::HostTagFilterToggle),
        trigger_width,
        None,
        None,
        p,
    )
}

fn view_host_grid(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let hosts = app.hosts.filtered();

    if hosts.is_empty() {
        if app.hosts_loading {
            let spinner: Element<'static, Message> =
                crate::ui::loading_indicator::loading_indicator(28.0, p.accent).into();
            return container(spinner)
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into();
        }
        let msg = if !app.hosts.search_query.is_empty() {
            t_static(t!("hosts.no_match"))
        } else if !matches!(app.hosts.tag_filter, HostTagFilter::All) {
            t_static(t!("hosts.filter_no_match"))
        } else {
            t_static(t!("hosts.empty"))
        };
        return container(text(msg).size(14).color(p.text_sec))
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into();
    }

    let grid_view = app.hosts.grid_view;

    styled_scrollable(
        CursorArea::for_target(
            container(responsive(move |size| {
                let layout = collection_layout(size.width, grid_view, 320.0, 6);
                let grid = collection_rows(
                    hosts.iter().map(|host| {
                        let hovered = app.hosts.hovered_id == Some(host.id);
                        let selected = app.hosts.cursor.selected_key() == Some(&host.id);
                        if grid_view {
                            view_host_card(host, hovered, selected, layout.item_width, p)
                        } else {
                            view_host_list_item(host, hovered, selected, layout.item_width, p)
                        }
                    }),
                    layout,
                    CARD_HEIGHT,
                    ROW_GAP,
                );
                let columns = layout.columns;

                sensor(grid)
                    .on_resize(move |_| Message::CursorColumns(CursorTarget::Hosts, columns))
                    .into()
            }))
            .padding(Padding {
                top: HOSTS_GRID_TOP_PADDING,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            })
            .width(Length::Fill),
            CursorTarget::Hosts,
        ),
        p,
        app.is_scrollbar_visible(VAULTS_HOSTS_SCROLL_ID),
    )
    .id(iced::widget::Id::from(VAULTS_HOSTS_SCROLL_ID))
    .on_scroll(move |v| Message::Scrolled(VAULTS_HOSTS_SCROLL_ID, v))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_host_card<'a>(
    host: &Host,
    hovered: bool,
    selected: bool,
    card_width: f32,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    view_host_item(host, hovered, selected, card_width, p)
}

fn view_host_list_item<'a>(
    host: &Host,
    hovered: bool,
    selected: bool,
    card_width: f32,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    view_host_item(host, hovered, selected, card_width, p)
}

fn view_host_item<'a>(
    host: &Host,
    hovered: bool,
    selected: bool,
    card_width: f32,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    let id = host.id;

    let icon = icon_box(
        crate::views::view_os_icon(host.os_type.icon(), 20.0, Color::WHITE),
        host.os_type.icon_bg(),
    );

    let subtitle = format!("{}@{}", host.username, host.hostname);
    let info = card_info(&host.name.clone(), subtitle, card_width, hovered, p);

    let actions: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::Hosts(crate::hosts::message::Message::EditHost(id)),
            p,
        )
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let content = row![icon, info, actions]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([10.0, 12.0]));

    card_wrapper(
        content.into(),
        hovered,
        selected,
        None,
        None,
        10.0,
        true,
        Message::Hosts(crate::hosts::message::Message::HostHoverEnter(id)),
        Message::Hosts(crate::hosts::message::Message::HostHoverLeave(id)),
        Some(Message::Hosts(crate::hosts::message::Message::SelectHost(
            id,
        ))),
        Some(Message::Hosts(crate::hosts::message::Message::ConnectHost(
            id,
        ))),
        Some(Message::Hosts(
            crate::hosts::message::Message::HostsContextMenuOpen(id),
        )),
        p,
    )
}

pub fn view_host_delete_confirm(app: &App, p: theme::ThemePalette) -> Element<'static, Message> {
    let Some(host_id) = app.hosts.delete_pending else {
        return text("").into();
    };
    let host_name = app
        .hosts
        .list
        .iter()
        .find(|h| h.id == host_id)
        .map_or(t_static(t!("hosts.delete_fallback")), |h| h.name.as_str());
    let body = t!("hosts.delete_confirm_body", name = host_name);
    delete_confirm_dialog(
        t!("hosts.delete_confirm_title"),
        body,
        Message::Hosts(crate::hosts::message::Message::HostDeleteCancel),
        Message::Hosts(crate::hosts::message::Message::DeleteHost(host_id)),
        &p,
        app.confirm_dialog_anim,
    )
}

pub fn view_host_context_menu(
    menu: &crate::hosts::HostsContextMenu,
    p: theme::ThemePalette,
) -> Element<'_, Message> {
    use crate::core::context_menu::ContextMenuAction as Action;

    let focused = menu
        .cursor
        .selected_or_first(crate::hosts::HostsContextMenu::actions())
        .copied();
    crate::ui::overlay::context_menu_frame(
        menu.position,
        move || {
            vec![
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Pencil,
                    t_static(t!("context_menu.edit")),
                    Message::Hosts(crate::hosts::message::Message::EditHost(menu.host_id)),
                    false,
                    focused == Some(Action::HostEdit),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Copy,
                    t_static(t!("context_menu.copy")),
                    Message::Hosts(crate::hosts::message::Message::DuplicateHost(menu.host_id)),
                    false,
                    focused == Some(Action::HostDuplicate),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Trash2,
                    t_static(t!("dialog.delete")),
                    Message::Hosts(crate::hosts::message::Message::HostDeleteRequest(
                        menu.host_id,
                    )),
                    true,
                    focused == Some(Action::HostDelete),
                    p,
                ),
            ]
        },
        140.0,
        Message::Hosts(crate::hosts::message::Message::HostsContextMenuClose),
        p,
    )
}
