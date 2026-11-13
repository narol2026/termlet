use iced::widget::{
    Space, button, column, container, mouse_area, responsive, row, sensor, stack, text,
};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};

use crate::app::App;
use crate::core::collection::{CARD_HEIGHT, ROW_GAP};
use crate::core::context_menu::ContextMenuAction as Action;
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{PortForward, PortForwardKind};
use crate::core::dropdown::DropdownTarget;
use crate::core::forms::FormDraft;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::{self, ThemePalette};
use crate::core::widgets::{
    DropdownSearch, SelectOption, card_hover_btn, card_info, card_wrapper, collection_layout,
    collection_rows, control_padding, filter_input, form_actions, form_header, form_panel,
    icon_box, labeled_input, segment_btn, select_dropdown, styled_scrollable, toolbar_icon_btn,
    toolbar_primary_btn,
};
use crate::ui::confirm_dialog::delete_confirm_dialog;
use crate::ui::cursor_area::CursorArea;
use crate::ui::host_key_prompt::{TrustActions, dialog as host_key_dialog};
use crate::ui::loading_indicator::loading_indicator;
use crate::ui::message_dialog;
use crate::ui::password_prompt::{self, PasswordPrompt};
use crate::ui::slide::slide_panel_from_right;
use rust_i18n::t;

use super::state::PortForwardStatus;

pub const PORT_FORWARDS_SCROLL_ID: &str = "port_forwards_scroll";
pub(crate) const PORT_FORWARDS_GRID_TOP_PADDING: f32 = 4.0;
pub(crate) const PORT_FORWARD_PASSWORD_INPUT_ID: &str = "port_forward_password_input";

pub(crate) fn view_port_forward_key_confirm(app: &App, p: ThemePalette) -> Element<'_, Message> {
    let Some(pending) = app.port_forwards.trust.confirmation() else {
        return Space::new().into();
    };
    let Some(key) = &pending.attempt.pending_key_info else {
        return Space::new().into();
    };
    let id = pending.forward.id;
    let request = pending.request;
    let cancel =
        Message::PortForward(super::message::Message::PortForwardKeyCancel { id, request });
    host_key_dialog(
        key,
        Some(&pending.forward.name),
        TrustActions::new(
            t_static(t!("settings.cancel")),
            cancel,
            Message::PortForward(super::message::Message::PortForwardKeyConfirm {
                id,
                request,
                save: false,
            }),
            Message::PortForward(super::message::Message::PortForwardKeyConfirm {
                id,
                request,
                save: true,
            }),
        ),
        p,
        app.confirm_dialog_anim,
    )
}

pub(crate) fn view_port_forward_password(app: &App, p: ThemePalette) -> Element<'_, Message> {
    let Some(pending) = app.port_forwards.password.prompt() else {
        return Space::new().into();
    };
    let id = pending.forward.id;
    let request = pending.request;
    let cancel =
        Message::PortForward(super::message::Message::PortForwardPasswordCancel { id, request });
    let Some(host) = pending.attempt.pending_host.as_ref() else {
        return Space::new().into();
    };
    let target = format!("{}@{}:{}", host.username, host.hostname, host.port);
    let submit =
        Message::PortForward(super::message::Message::PortForwardPasswordSubmit { id, request });
    let prompt = PasswordPrompt {
        label: t_static(t!("ssh.password")),
        placeholder: t_static(t!("ssh.enter_password")),
        value: &pending.attempt.password_input,
        visible: pending.attempt.password_visible,
        input_id: PORT_FORWARD_PASSWORD_INPUT_ID,
        submit_label: t_static(t!("port_forward.connect")),
        submit,
        toggle: Message::PortForward(super::message::Message::PortForwardPasswordToggleVisible {
            id,
            request,
        }),
        cancel: cancel.clone(),
    };
    let input = password_prompt::field(&prompt, &p, move |value| {
        Message::PortForward(super::message::Message::PortForwardPasswordChanged {
            id,
            request,
            value: secrecy::SecretString::from(value),
        })
    });
    let actions = password_prompt::actions(&prompt, &p);
    message_dialog::message_dialog(
        t_static(t!("ssh.password_required")),
        Some(
            t!(
                "port_forward.password_body",
                name = pending.forward.name,
                target = target
            )
            .to_string(),
        ),
        input,
        vec![actions],
        cancel,
        420.0,
        &p,
        app.confirm_dialog_anim,
    )
}

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_port_forwards(app: &App) -> Element<'_, Message> {
    let toolbar = view_toolbar(app);
    let grid = view_grid(app);

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
            340.0,
            app.port_forwards.form.visible,
            app.port_forwards.form_anim,
        ),
    ]
    .spacing(0)
    .height(Length::Fill)
    .into();

    mouse_area(page)
        .on_move(|v0| {
            Message::PortForward(
                crate::port_forward::message::Message::PortForwardViewCursorMoved(v0),
            )
        })
        .into()
}

fn view_toolbar(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let search = filter_input(
        t_static(t!("port_forward.search")),
        &app.port_forwards.search_query,
        &p,
        |v0| {
            Message::PortForward(
                crate::port_forward::message::Message::PortForwardSearchChanged(v0),
            )
        },
    );

    let new_btn = toolbar_primary_btn(
        lucide_icons::Icon::Plus,
        t_static(t!("port_forward.new")),
        Message::PortForward(crate::port_forward::message::Message::PortForwardShowForm),
        p,
    );

    let view_icon = if app.port_forwards.grid_view {
        lucide_icons::Icon::List
    } else {
        lucide_icons::Icon::LayoutGrid
    };
    let view_btn = toolbar_icon_btn(
        view_icon,
        Message::PortForward(crate::port_forward::message::Message::PortForwardToggleView),
        p,
    );

    let action_row = row![new_btn, Space::new().width(Length::Fill), view_btn,]
        .spacing(8)
        .align_y(iced::Alignment::Center);

    container(column![search, action_row].spacing(10))
        .padding(Padding::from([13.0, 20.0]))
        .width(Length::Fill)
        .into()
}

fn view_grid(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let grid_view = app.port_forwards.grid_view;

    let forwards = app.port_forwards.filtered();

    if forwards.is_empty() {
        let msg = if app.port_forwards.search_query.is_empty() {
            t_static(t!("port_forward.empty"))
        } else {
            t_static(t!("port_forward.no_match"))
        };
        return container(text(msg).size(14).color(p.text_sec))
            .width(Length::Fill)
            .height(Length::Fill)
            .center(Length::Fill)
            .into();
    }

    styled_scrollable(
        CursorArea::for_target(
            container(responsive(move |size| {
                let layout = collection_layout(size.width, grid_view, 270.0, 5);
                let grid = collection_rows(
                    forwards.iter().map(|forward| {
                        if grid_view {
                            view_forward_card(app, forward, layout.item_width)
                        } else {
                            view_forward_row(app, forward, layout.item_width)
                        }
                    }),
                    layout,
                    CARD_HEIGHT,
                    ROW_GAP,
                );
                let columns = layout.columns;

                sensor(grid)
                    .on_resize(move |_| Message::CursorColumns(CursorTarget::PortForwards, columns))
                    .into()
            }))
            .padding(Padding {
                top: PORT_FORWARDS_GRID_TOP_PADDING,
                right: 20.0,
                bottom: 20.0,
                left: 20.0,
            })
            .width(Length::Fill),
            CursorTarget::PortForwards,
        ),
        p,
        app.is_scrollbar_visible(PORT_FORWARDS_SCROLL_ID),
    )
    .id(iced::widget::Id::from(PORT_FORWARDS_SCROLL_ID))
    .on_scroll(move |v| Message::Scrolled(PORT_FORWARDS_SCROLL_ID, v))
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn view_forward_card<'a>(
    app: &'a App,
    forward: &'a PortForward,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let hovered = app.port_forwards.hovered_id == Some(forward.id);
    view_forward_item(app, forward, hovered, p, card_width)
}

fn view_forward_row<'a>(
    app: &'a App,
    forward: &'a PortForward,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let hovered = app.port_forwards.hovered_id == Some(forward.id);
    view_forward_item(app, forward, hovered, p, card_width)
}

fn view_forward_item<'a>(
    app: &'a App,
    forward: &'a PortForward,
    hovered: bool,
    p: theme::ThemePalette,
    card_width: f32,
) -> Element<'a, Message> {
    let id = forward.id;
    let selected = app.port_forwards.cursor.selected_key() == Some(&id);
    let status = app.port_forwards.status_for(id);
    let running = status == PortForwardStatus::Running;
    let starting = status == PortForwardStatus::Starting;
    let stopping = status == PortForwardStatus::Stopping;
    let failed = status == PortForwardStatus::Failed;
    let active = running || starting;

    let kind_bg = kind_color(forward.kind, &p);
    let icon_bg = if running {
        kind_bg
    } else if starting || stopping {
        Color {
            r: kind_bg.r * 0.55 + p.text_dim.r * 0.45,
            g: kind_bg.g * 0.55 + p.text_dim.g * 0.45,
            b: kind_bg.b * 0.55 + p.text_dim.b * 0.45,
            a: 1.0,
        }
    } else if failed {
        p.danger
    } else {
        Color {
            r: p.text_dim.r * 0.55,
            g: p.text_dim.g * 0.55,
            b: p.text_dim.b * 0.55,
            a: 1.0,
        }
    };
    let kind_label = match forward.kind {
        PortForwardKind::Local => t_static(t!("port_forward_kind.local_short")),
        PortForwardKind::Remote => t_static(t!("port_forward_kind.remote_short")),
        PortForwardKind::Dynamic => t_static(t!("port_forward_kind.dynamic_short")),
    };

    let icon = icon_box(
        if starting || stopping {
            // Self-animating ring indicator marking that the connection (or
            // disconnection) is still being processed.
            let indicator: Element<'static, Message> = loading_indicator(18.0, Color::WHITE).into();
            indicator
        } else {
            text(kind_label).size(18).color(Color::WHITE).into()
        },
        icon_bg,
    );

    let subtitle = forward.human_summary();
    let info = card_info(&forward.name.clone(), subtitle, card_width, hovered, p);

    let actions: Element<'_, Message> = if hovered {
        row![card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::PortForward(crate::port_forward::message::Message::PortForwardEdit(id)),
            p
        )]
        .align_y(iced::Alignment::Center)
        .into()
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let content = row![icon, info, actions]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .padding(Padding::from([10.0, 12.0]));

    let active_bg = Color {
        r: (p.accent.r * 0.10 + p.card_bg.r * 0.90),
        g: (p.accent.g * 0.10 + p.card_bg.g * 0.90),
        b: (p.accent.b * 0.10 + p.card_bg.b * 0.90),
        a: 1.0,
    };

    let border_accent = if running {
        Some(p.accent)
    } else if failed {
        Some(p.danger)
    } else {
        None
    };

    card_wrapper(
        content.into(),
        hovered,
        selected,
        border_accent,
        active.then_some(active_bg),
        10.0,
        true,
        Message::PortForward(crate::port_forward::message::Message::PortForwardHoverEnter(id)),
        Message::PortForward(crate::port_forward::message::Message::PortForwardHoverLeave(id)),
        Some(Message::PortForward(
            crate::port_forward::message::Message::PortForwardSelect(id),
        )),
        Some(Message::PortForward(
            crate::port_forward::message::Message::PortForwardToggle(id),
        )),
        Some(Message::PortForward(
            crate::port_forward::message::Message::PortForwardContextMenuOpen(id),
        )),
        p,
    )
}

pub fn view_port_forward_context_menu<'a>(
    app: &'a App,
    menu: &'a crate::port_forward::PortForwardContextMenu,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    const MENU_WIDTH: f32 = 140.0;

    let id = menu.forward_id;
    let status = app.port_forwards.status_for(id);
    let focused = menu
        .cursor
        .selected_or_first(crate::port_forward::PortForwardContextMenu::actions())
        .copied();
    let running_or_starting = matches!(
        status,
        PortForwardStatus::Running | PortForwardStatus::Starting
    );

    crate::ui::overlay::context_menu_frame(
        menu.position,
        move || {
            let toggle_item = if running_or_starting {
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::CircleX,
                    t_static(t!("port_forward.disconnect")),
                    Message::PortForward(crate::port_forward::message::Message::PortForwardStop(
                        id,
                    )),
                    false,
                    focused == Some(Action::PortForwardToggle),
                    p,
                )
            } else {
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Play,
                    t_static(t!("port_forward.connect")),
                    Message::PortForward(crate::port_forward::message::Message::PortForwardStart(
                        id,
                    )),
                    false,
                    focused == Some(Action::PortForwardToggle),
                    p,
                )
            };

            vec![
                toggle_item,
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Pencil,
                    t_static(t!("context_menu.edit")),
                    Message::PortForward(crate::port_forward::message::Message::PortForwardEdit(
                        id,
                    )),
                    false,
                    focused == Some(Action::PortForwardEdit),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::ScrollText,
                    t_static(t!("port_forward.logs")),
                    Message::PortForward(
                        crate::port_forward::message::Message::PortForwardShowLogs(id),
                    ),
                    false,
                    focused == Some(Action::PortForwardLogs),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Copy,
                    t_static(t!("port_forward.copy")),
                    Message::PortForward(
                        crate::port_forward::message::Message::PortForwardDuplicate(id),
                    ),
                    false,
                    focused == Some(Action::PortForwardDuplicate),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Trash2,
                    t_static(t!("dialog.delete")),
                    Message::PortForward(
                        crate::port_forward::message::Message::PortForwardDeleteRequest(id),
                    ),
                    true,
                    focused == Some(Action::PortForwardDelete),
                    p,
                ),
            ]
        },
        MENU_WIDTH,
        Message::PortForward(crate::port_forward::message::Message::PortForwardContextMenuClose),
        p,
    )
}

fn view_form(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let title = if app.port_forwards.form.editing_id.is_some() {
        t_static(t!("port_forward.edit_forward"))
    } else {
        t_static(t!("port_forward.new_forward"))
    };
    let header = form_header(
        title.to_string(),
        Message::PortForward(crate::port_forward::message::Message::PortForwardHideForm),
        p,
    );

    let name_input = labeled_input(
        t_static(t!("port_forward.name_optional")),
        "e.g. db tunnel",
        &app.port_forwards.form.name,
        &p,
        |v0| {
            Message::PortForward(crate::port_forward::message::Message::PortForwardFormName(
                v0,
            ))
        },
    );

    let kind_label = text(t_static(t!("port_forward.type")))
        .size(12)
        .color(p.text_sec);
    let kind_btns = row(PortForwardKind::all().iter().map(|&k| {
        segment_btn(
            kind_label_text(k),
            app.port_forwards.form.kind == k,
            Message::PortForward(crate::port_forward::message::Message::PortForwardFormKind(
                k,
            )),
            p,
        )
    }))
    .spacing(6);
    let kind_section = column![kind_label, kind_btns].spacing(6);

    let host_section = view_host_section(app);

    let bind_input = labeled_input(
        match app.port_forwards.form.kind {
            PortForwardKind::Remote => t_static(t!("port_forward.bind_address_remote")),
            _ => t_static(t!("port_forward.bind_address")),
        },
        "127.0.0.1",
        &app.port_forwards.form.bind_address,
        &p,
        |v0| {
            Message::PortForward(
                crate::port_forward::message::Message::PortForwardFormBindAddress(v0),
            )
        },
    );
    let listen_input = labeled_input(
        t_static(t!("port_forward.listen_port")),
        "8080",
        &app.port_forwards.form.listen_port,
        &p,
        |v0| {
            Message::PortForward(
                crate::port_forward::message::Message::PortForwardFormListenPort(v0),
            )
        },
    );

    let needs_dest = app.port_forwards.form.kind != PortForwardKind::Dynamic;

    let can_save = app.port_forwards.form.save_enabled();

    let actions = form_actions(
        if app.port_forwards.form.editing_id.is_some() {
            t_static(t!("settings.save"))
        } else {
            t_static(t!("port_forward.add_forward"))
        },
        t_static(t!("settings.cancel")),
        can_save,
        Message::PortForward(crate::port_forward::message::Message::PortForwardFormSubmit),
        Message::PortForward(crate::port_forward::message::Message::PortForwardHideForm),
        app.port_forwards.form.editing_id.map(|editing_id| {
            Message::PortForward(
                crate::port_forward::message::Message::PortForwardDeleteRequest(editing_id),
            )
        }),
        p,
    );

    let mut form_content = column![
        header,
        name_input,
        kind_section,
        host_section,
        bind_input,
        listen_input,
    ]
    .spacing(14);

    if needs_dest {
        let dest_host_input = labeled_input(
            t_static(t!("port_forward.dest_host")),
            "127.0.0.1",
            &app.port_forwards.form.dest_host,
            &p,
            |v0| {
                Message::PortForward(
                    crate::port_forward::message::Message::PortForwardFormDestHost(v0),
                )
            },
        );
        let dest_port_input = labeled_input(
            t_static(t!("port_forward.dest_port")),
            "80",
            &app.port_forwards.form.dest_port,
            &p,
            |v0| {
                Message::PortForward(
                    crate::port_forward::message::Message::PortForwardFormDestPort(v0),
                )
            },
        );
        form_content = form_content.push(dest_host_input).push(dest_port_input);
    }

    form_content = form_content.push(actions).padding(20);

    form_panel(form_content, 340.0, p)
}

fn view_host_section(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let label = text(t_static(t!("port_forward.via_host")))
        .size(12)
        .color(p.text_sec);

    let current = app
        .port_forwards
        .form
        .host_id
        .and_then(|id| app.hosts.list.iter().find(|h| h.id == id));
    let current_label = match current {
        Some(host) => host.name.clone(),
        None => t_static(t!("port_forward.select_host")).to_string(),
    };

    let empty_state = if app.hosts.list.is_empty() {
        t_static(t!("port_forward.no_hosts"))
    } else {
        t_static(t!("hosts.no_match"))
    };

    let items = crate::cursor::port_forward_host_ids(app);
    let focused = app.port_forwards.form.host_picker.active(&items).copied();
    let options: Vec<SelectOption> = if app.active_dropdown == Some(DropdownTarget::PortForwardHost)
    {
        items
            .into_iter()
            .filter_map(|id| {
                let host = app.hosts.list.iter().find(|host| host.id == id)?;
                Some(SelectOption {
                    label: host.name.clone(),
                    subtitle: Some(format!("{}@{}:{}", host.username, host.hostname, host.port)),
                    active: app.port_forwards.form.host_id == Some(host.id),
                    focused: focused == Some(id),
                    on_select: Message::PortForward(
                        crate::port_forward::message::Message::PortForwardFormHost(id),
                    ),
                    leading: None,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let dropdown = select_dropdown(
        current_label,
        None,
        options,
        app.active_dropdown == Some(DropdownTarget::PortForwardHost),
        DropdownTarget::PortForwardHost,
        Message::PortForward(
            crate::port_forward::message::Message::PortForwardFormToggleHostPicker,
        ),
        Length::Fill,
        Some(DropdownSearch {
            placeholder: t_static(t!("port_forward.search_hosts")),
            query: app.port_forwards.form.host_picker.query(),
            on_input: |v0| {
                Message::PortForward(
                    crate::port_forward::message::Message::PortForwardFormHostSearch(v0),
                )
            },
        }),
        Some(empty_state),
        p,
    );

    column![label, dropdown].spacing(6).into()
}

fn kind_label_text(kind: PortForwardKind) -> String {
    match kind {
        PortForwardKind::Local => t!("port_forward_kind.local_label").to_string(),
        PortForwardKind::Remote => t!("port_forward_kind.remote_label").to_string(),
        PortForwardKind::Dynamic => t!("port_forward_kind.dynamic_label").to_string(),
    }
}

fn kind_color(kind: PortForwardKind, p: &ThemePalette) -> Color {
    match kind {
        PortForwardKind::Local => p.accent,
        PortForwardKind::Remote => p.success,
        PortForwardKind::Dynamic => p.purple,
    }
}

pub fn view_port_forward_log_modal(app: &App, p: theme::ThemePalette) -> Element<'_, Message> {
    let Some(id) = app
        .port_forwards
        .logs_open
        .then_some(app.port_forwards.cursor.selected_key().copied())
        .flatten()
    else {
        return text("").into();
    };
    let forward = app.port_forwards.list.iter().find(|f| f.id == id);
    let name = forward.map_or(t_static(t!("port_forward.logs_title")), |f| f.name.as_str());

    let runtime = app.port_forwards.runtime_for(id);
    let (log_text, has_logs) = if let Some(runtime) = runtime {
        if runtime.logs.is_empty() {
            (t!("port_forward.no_logs_yet").into_owned(), false)
        } else {
            (runtime.logs.join("\n"), true)
        }
    } else {
        (t!("port_forward.no_activity_yet").into_owned(), false)
    };

    let backdrop_color = p.backdrop;
    let backdrop = mouse_area(
        container(Space::new().width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(backdrop_color)),
                ..Default::default()
            }),
    )
    .on_press(Message::PortForward(
        crate::port_forward::message::Message::PortForwardHideLogs,
    ))
    .interaction(iced::mouse::Interaction::Idle);

    let close_btn = {
        let item_hover = p.item_hover;
        let surface = p.surface;
        let text_pri = p.text_pri;
        button(
            text(t_static(t!("port_forward.close")))
                .size(12)
                .color(text_pri),
        )
        .on_press(Message::PortForward(
            crate::port_forward::message::Message::PortForwardHideLogs,
        ))
        .padding(control_padding(12.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    item_hover
                } else {
                    surface
                })),
                text_color: text_pri,
                border: Border {
                    radius: 6.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
    };

    let copy_btn = {
        let accent = p.accent;
        let text_dim = p.text_dim;
        let has = has_logs;
        button(
            row![
                text(char::from(lucide_icons::Icon::Copy))
                    .font(LUCIDE_FONT)
                    .size(11)
                    .color(if has { accent } else { text_dim }),
                text(t_static(t!("port_forward.copy")))
                    .size(12)
                    .color(if has { accent } else { text_dim }),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press_maybe(has.then(|| {
            Message::PortForward(crate::port_forward::message::Message::PortForwardCopyLogs(
                id,
            ))
        }))
        .padding(control_padding(12.0, 18.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered && has {
                    Color { a: 0.16, ..accent }
                } else {
                    Color::TRANSPARENT
                })),
                text_color: if has { accent } else { text_dim },
                border: Border {
                    radius: 6.0.into(),
                    width: 1.0,
                    color: if has { accent } else { p.button_border },
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
    };

    let header = row![
        row![
            text(char::from(lucide_icons::Icon::ScrollText))
                .font(LUCIDE_FONT)
                .size(14)
                .color(p.text_sec),
            text(name).size(14).color(p.text_pri),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        Space::new().width(Length::Fill),
        text(
            t!(
                "port_forward.log_line_count",
                count = log_text.lines().count().to_string()
            )
            .to_string()
        )
        .size(11)
        .color(p.text_dim),
    ]
    .align_y(iced::Alignment::Center);

    let log_body = styled_scrollable(
        container(
            text(log_text)
                .size(12)
                .color(p.text_sec)
                .width(Length::Fill)
                .ellipsis(text::Ellipsis::End)
                .wrapping(text::Wrapping::None),
        )
        .padding(Padding::ZERO.right(12.0)),
        p,
        false,
    )
    // The forward keeps streaming logs while this modal is open — anchor the
    // scrollable to the bottom so new output stays visible.
    .anchor_bottom()
    .width(Length::Fill);

    let actions = row![copy_btn, Space::new().width(Length::Fill), close_btn]
        .spacing(10)
        .align_y(iced::Alignment::Center);

    let card = {
        let content_bg = p.content_bg;
        let border_color = p.border;
        mouse_area(
            container(
                column![header, log_body, actions]
                    .spacing(14)
                    .width(Length::Fill),
            )
            .padding(Padding::from([22.0, 24.0]))
            .width(Length::Fixed(620.0))
            .height(Length::Fixed(420.0))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(content_bg)),
                border: Border {
                    color: border_color,
                    width: 1.0,
                    radius: 10.0.into(),
                },
                shadow: iced::Shadow {
                    color: Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.35,
                    },
                    offset: iced::Vector { x: 0.0, y: 8.0 },
                    blur_radius: 32.0,
                },
                ..Default::default()
            }),
        )
        .interaction(iced::mouse::Interaction::Idle)
    };

    stack![backdrop, container(card).center(Length::Fill)]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub fn view_port_forward_delete_confirm(
    app: &App,
    p: theme::ThemePalette,
) -> Element<'static, Message> {
    let Some(id) = app.port_forwards.delete_pending else {
        return text("").into();
    };
    let name = app
        .port_forwards
        .list
        .iter()
        .find(|f| f.id == id)
        .map_or(t_static(t!("port_forward.delete_fallback")), |f| {
            f.name.as_str()
        });
    let body = t!("port_forward.delete_confirm_body", name = name);
    delete_confirm_dialog(
        t!("port_forward.delete_confirm_title"),
        body,
        Message::PortForward(crate::port_forward::message::Message::PortForwardDeleteCancel),
        Message::PortForward(crate::port_forward::message::Message::PortForwardDelete(id)),
        &p,
        app.confirm_dialog_anim,
    )
}
