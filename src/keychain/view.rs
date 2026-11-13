use iced::widget::{Space, button, column, container, mouse_area, responsive, row, sensor, text};
use iced::{Alignment, Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};
use secrecy::SecretString;

use crate::app::App;
use crate::core::collection::{
    CARD_HEIGHT, EMPTY_HEIGHT, ROW_GAP, SECTION_HEADER_GAP, SECTION_HEADER_HEIGHT,
};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{Identity, IdentityId, KeyType, KeychainEntryId};
use crate::core::forms::FormDraft;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::{
    card_hover_btn, card_info, card_wrapper, collection_layout, collection_rows, control_padding,
    filter_input, form_actions, form_actions_with_save, form_header, form_panel, icon_box,
    labeled_input, labeled_password_input, labeled_readonly, styled_btn, styled_scrollable,
    toolbar_icon_btn, toolbar_outline_btn, toolbar_primary_btn,
};
use crate::ui::confirm_dialog::delete_confirm_dialog;
use crate::ui::cursor_area::CursorArea;
use crate::ui::slide::slide_panel_from_right;
use rust_i18n::t;

pub const KEYCHAIN_SCROLL_ID: &str = "keychain_scroll";
pub(crate) const KEYCHAIN_GRID_TOP_PADDING: f32 = 6.0;
pub(crate) const KEYCHAIN_KEY_GRID_BOTTOM_PADDING: f32 = 10.0;
pub(crate) const KEYCHAIN_IDENTITIES_GRID_BOTTOM_PADDING: f32 = 20.0;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_keychain(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let toolbar = view_toolbar(app);

    let keys_section = view_keys_section(app);
    let divider1 = divider(p);
    let identities_section = view_identities_section(app);

    let scroll_content = CursorArea::for_target(
        column![keys_section, divider1, identities_section]
            .spacing(16)
            .width(Length::Fill),
        CursorTarget::Keychain,
    );

    let body = column![
        toolbar,
        divider(p),
        styled_scrollable(
            scroll_content,
            p,
            app.is_scrollbar_visible(KEYCHAIN_SCROLL_ID)
        )
        .id(iced::widget::Id::from(KEYCHAIN_SCROLL_ID))
        .on_scroll(move |v| Message::Scrolled(KEYCHAIN_SCROLL_ID, v))
        .height(Length::Fill),
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill);

    let form_content: Element<'_, Message> = if app.keychain.identity_form.visible {
        view_identity_form(app)
    } else {
        view_form(app)
    };
    let form = slide_panel_from_right(
        form_content,
        340.0,
        app.keychain.form.visible || app.keychain.identity_form.visible,
        app.keychain.form_anim,
    );

    let page: Element<'_, Message> = row![body, form].spacing(0).height(Length::Fill).into();

    mouse_area(page)
        .on_move(|v0| {
            Message::Keychain(crate::keychain::message::Message::KeychainViewCursorMoved(
                v0,
            ))
        })
        .into()
}

fn view_toolbar(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let search = filter_input(
        t_static(t!("keychain.search_keys")),
        &app.keychain.search_query,
        &p,
        |v0| Message::Keychain(crate::keychain::message::Message::KeychainSearchChanged(v0)),
    );

    let new_btn = toolbar_primary_btn(
        lucide_icons::Icon::Plus,
        t_static(t!("keychain.generate_key")),
        Message::Keychain(crate::keychain::message::Message::KeychainShowForm),
        p,
    );

    let identity_btn = toolbar_outline_btn(
        lucide_icons::Icon::User,
        t_static(t!("keychain.new_identity")),
        Message::Keychain(crate::keychain::message::Message::IdentityShowForm),
        p,
    );

    let view_icon = if app.keychain.grid_view {
        lucide_icons::Icon::List
    } else {
        lucide_icons::Icon::LayoutGrid
    };
    let view_btn = toolbar_icon_btn(
        view_icon,
        Message::Keychain(crate::keychain::message::Message::KeychainToggleView),
        p,
    );

    let action_row = row![
        new_btn,
        identity_btn,
        Space::new().width(Length::Fill),
        view_btn,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    container(column![search, action_row].spacing(10))
        .padding(Padding::from([13.0, 20.0]))
        .width(Length::Fill)
        .into()
}

fn view_keys_section(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let header = container(
        row![
            text(t_static(t!("keychain.keys_title")))
                .size(14)
                .color(p.text_pri),
            Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 8.0,
        right: 20.0,
        bottom: 0.0,
        left: 20.0,
    })
    .height(Length::Fixed(SECTION_HEADER_HEIGHT))
    .width(Length::Fill);

    if app.keychain.list.is_empty() {
        let empty = container(
            text(t_static(t!("keychain.no_keys_yet")))
                .size(12)
                .color(p.text_dim),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding::from([20.0, 20.0]))
        .height(Length::Fixed(EMPTY_HEIGHT));
        return column![header, empty].spacing(4).into();
    }

    let grid = view_keys_grid(app);
    column![header, grid].spacing(SECTION_HEADER_GAP).into()
}

fn view_keys_grid(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let filtered = app.keychain.filtered_keys();
    let grid_view = app.keychain.grid_view;

    if filtered.is_empty() && !app.keychain.list.is_empty() {
        return container(
            text(t_static(t!("keychain.no_keys_match")))
                .size(12)
                .color(p.text_dim),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding::from([20.0, 20.0]))
        .into();
    }

    container(responsive(move |size| {
        let layout = collection_layout(size.width, grid_view, 280.0, 5);
        let grid = collection_rows(
            filtered.iter().map(|entry| {
                if grid_view {
                    view_keychain_card(app, entry, layout.item_width)
                } else {
                    view_keychain_row(app, entry, layout.item_width)
                }
            }),
            layout,
            CARD_HEIGHT,
            ROW_GAP,
        );
        let columns = layout.columns;

        sensor(grid)
            .on_resize(move |_| Message::CursorColumns(CursorTarget::Keychain, columns))
            .into()
    }))
    .padding(Padding {
        top: KEYCHAIN_GRID_TOP_PADDING,
        right: 20.0,
        bottom: KEYCHAIN_KEY_GRID_BOTTOM_PADDING,
        left: 20.0,
    })
    .width(Length::Fill)
    .into()
}

fn view_keychain_card<'a>(
    app: &'a App,
    entry: &'a crate::core::data::KeychainEntry,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let hovered = app.keychain.hovered_id == Some(entry.id);
    let selected = app.keychain.selected_key() == Some(entry.id);
    let id = entry.id;

    let icon = icon_box(
        text(char::from(lucide_icons::Icon::KeyRound))
            .font(LUCIDE_FONT)
            .size(18)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let info = card_info(
        &entry.name.clone(),
        entry.path.clone(),
        card_width,
        hovered,
        p,
    );

    let actions: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::Keychain(crate::keychain::message::Message::KeychainEdit(id)),
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
        Message::Keychain(crate::keychain::message::Message::KeychainHoverEnter(id)),
        Message::Keychain(crate::keychain::message::Message::KeychainHoverLeave(id)),
        Some(Message::Keychain(
            crate::keychain::message::Message::KeychainSelect(id),
        )),
        None,
        Some(Message::Keychain(
            crate::keychain::message::Message::KeychainContextMenuOpen(id),
        )),
        p,
    )
}

fn view_keychain_row<'a>(
    app: &'a App,
    entry: &'a crate::core::data::KeychainEntry,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let hovered = app.keychain.hovered_id == Some(entry.id);
    let selected = app.keychain.selected_key() == Some(entry.id);
    let id = entry.id;

    let key_icon = icon_box(
        text(char::from(lucide_icons::Icon::KeyRound))
            .font(LUCIDE_FONT)
            .size(18)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let info = card_info(
        &entry.name.clone(),
        entry.path.clone(),
        card_width,
        hovered,
        p,
    );

    let edit_btn: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::Keychain(crate::keychain::message::Message::KeychainEdit(id)),
            p,
        )
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let row_content = row![key_icon, info, edit_btn,]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(Padding::from([10.0, 12.0]));

    card_wrapper(
        row_content.into(),
        hovered,
        selected,
        None,
        None,
        8.0,
        false,
        Message::Keychain(crate::keychain::message::Message::KeychainHoverEnter(id)),
        Message::Keychain(crate::keychain::message::Message::KeychainHoverLeave(id)),
        Some(Message::Keychain(
            crate::keychain::message::Message::KeychainSelect(id),
        )),
        Some(Message::Keychain(
            crate::keychain::message::Message::KeychainEdit(id),
        )),
        Some(Message::Keychain(
            crate::keychain::message::Message::KeychainContextMenuOpen(id),
        )),
        p,
    )
}

fn view_identities_section(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let header = container(
        row![
            text(t_static(t!("keychain.identities_title")))
                .size(14)
                .color(p.text_pri),
            Space::new().width(Length::Fill),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 8.0,
        right: 20.0,
        bottom: 0.0,
        left: 20.0,
    })
    .height(Length::Fixed(SECTION_HEADER_HEIGHT))
    .width(Length::Fill);

    if app.keychain.identities.is_empty() {
        let empty = container(
            text(t_static(t!("keychain.no_identities_yet")))
                .size(12)
                .color(p.text_dim),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding::from([20.0, 20.0]))
        .height(Length::Fixed(EMPTY_HEIGHT));
        return column![header, empty].spacing(4).into();
    }

    let grid = view_identities_grid(app);
    column![header, grid].spacing(SECTION_HEADER_GAP).into()
}

fn view_identities_grid(app: &App) -> Element<'_, Message> {
    let grid_view = app.keychain.grid_view;

    container(responsive(move |size| {
        let layout = collection_layout(size.width, grid_view, 280.0, 5);
        let grid = collection_rows(
            app.keychain.identities.iter().map(|ident| {
                if grid_view {
                    view_identity_card(app, ident, layout.item_width)
                } else {
                    view_identity_row(app, ident, layout.item_width)
                }
            }),
            layout,
            CARD_HEIGHT,
            ROW_GAP,
        );
        let columns = layout.columns;

        sensor(grid)
            .on_resize(move |_| Message::CursorColumns(CursorTarget::Keychain, columns))
            .into()
    }))
    .padding(Padding {
        top: KEYCHAIN_GRID_TOP_PADDING,
        right: 20.0,
        bottom: KEYCHAIN_IDENTITIES_GRID_BOTTOM_PADDING,
        left: 20.0,
    })
    .width(Length::Fill)
    .into()
}

fn view_identity_card<'a>(
    app: &'a App,
    entry: &'a Identity,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let hovered = app.keychain.hovered_identity_id == Some(entry.id);
    let selected = app.keychain.selected_identity() == Some(entry.id);
    let id = entry.id;

    let icon = icon_box(
        text(char::from(lucide_icons::Icon::User))
            .font(LUCIDE_FONT)
            .size(18)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let subtitle = if entry.username.is_empty() {
        t!("keychain.no_username").to_string()
    } else {
        entry.username.clone()
    };
    let info = card_info(&entry.label.clone(), subtitle, card_width, hovered, p);

    let actions: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::Keychain(crate::keychain::message::Message::IdentityEdit(id)),
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
        Message::Keychain(crate::keychain::message::Message::IdentityHoverEnter(id)),
        Message::Keychain(crate::keychain::message::Message::IdentityHoverLeave(id)),
        Some(Message::Keychain(
            crate::keychain::message::Message::IdentitySelect(id),
        )),
        None,
        Some(Message::Keychain(
            crate::keychain::message::Message::IdentityContextMenuOpen(id),
        )),
        p,
    )
}

fn view_identity_row<'a>(
    app: &'a App,
    entry: &'a Identity,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let hovered = app.keychain.hovered_identity_id == Some(entry.id);
    let selected = app.keychain.selected_identity() == Some(entry.id);
    let id = entry.id;

    let key_icon = icon_box(
        text(char::from(lucide_icons::Icon::User))
            .font(LUCIDE_FONT)
            .size(18)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let subtitle = if entry.username.is_empty() {
        t!("keychain.no_username").to_string()
    } else {
        entry.username.clone()
    };
    let info = card_info(&entry.label.clone(), subtitle, card_width, hovered, p);

    let edit_btn: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::Keychain(crate::keychain::message::Message::IdentityEdit(id)),
            p,
        )
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let row_content = row![key_icon, info, edit_btn]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(Padding::from([10.0, 12.0]));

    card_wrapper(
        row_content.into(),
        hovered,
        selected,
        None,
        None,
        8.0,
        false,
        Message::Keychain(crate::keychain::message::Message::IdentityHoverEnter(id)),
        Message::Keychain(crate::keychain::message::Message::IdentityHoverLeave(id)),
        Some(Message::Keychain(
            crate::keychain::message::Message::IdentitySelect(id),
        )),
        Some(Message::Keychain(
            crate::keychain::message::Message::IdentityEdit(id),
        )),
        Some(Message::Keychain(
            crate::keychain::message::Message::IdentityContextMenuOpen(id),
        )),
        p,
    )
}

fn view_identity_form(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let is_edit = app.keychain.identity_form.editing_id.is_some();

    let title = if is_edit {
        t_static(t!("keychain.edit_identity"))
    } else {
        t_static(t!("keychain.new_identity_title"))
    };
    let header = form_header(
        title.to_string(),
        Message::Keychain(crate::keychain::message::Message::IdentityHideForm),
        p,
    );

    let label_input = labeled_input(
        t_static(t!("keychain.label")),
        t_static(t!("keychain.label")),
        &app.keychain.identity_form.label,
        &p,
        |v0| Message::Keychain(crate::keychain::message::Message::IdentityFormLabel(v0)),
    );

    let username_input = labeled_input(
        t_static(t!("keychain.username")),
        t_static(t!("keychain.username")),
        &app.keychain.identity_form.username,
        &p,
        |v0| Message::Keychain(crate::keychain::message::Message::IdentityFormUsername(v0)),
    );

    let password_input = labeled_password_input(
        t_static(t!("form.password")),
        t_static(t!("form.password")),
        &app.keychain.identity_form.password,
        &p,
        |s| {
            Message::Keychain(crate::keychain::message::Message::IdentityFormPassword(
                SecretString::from(s),
            ))
        },
        !app.keychain.identity_form.password_visible,
        Message::Keychain(crate::keychain::message::Message::IdentityFormTogglePasswordVisible),
    );

    let can_save = app.keychain.identity_form.save_enabled();

    let actions = form_actions(
        if is_edit {
            t_static(t!("keychain.save"))
        } else {
            t_static(t!("keychain.create_identity"))
        },
        t_static(t!("settings.cancel")),
        can_save,
        Message::Keychain(crate::keychain::message::Message::IdentityFormSubmit),
        Message::Keychain(crate::keychain::message::Message::IdentityHideForm),
        app.keychain.identity_form.editing_id.map(|editing_id| {
            Message::Keychain(crate::keychain::message::Message::IdentityDeleteRequest(
                editing_id,
            ))
        }),
        p,
    );

    let form_content = column![header, label_input, username_input, password_input]
        .spacing(14)
        .push(actions)
        .padding(20);

    form_panel(form_content, 340.0, p)
}

pub fn view_identity_delete_confirm(
    app: &App,
    p: theme::ThemePalette,
) -> Element<'static, Message> {
    let Some(id) = app.keychain.identity_delete_pending else {
        return text("").into();
    };
    let name = app
        .keychain
        .identities
        .iter()
        .find(|i| i.id == id)
        .map_or(t_static(t!("keychain.delete_identity_fallback")), |i| {
            i.label.as_str()
        });
    let body = t!("keychain.delete_identity_confirm", name = name);
    delete_confirm_dialog(
        t!("keychain.delete_identity_title"),
        body,
        Message::Keychain(crate::keychain::message::Message::IdentityDeleteCancel),
        Message::Keychain(crate::keychain::message::Message::IdentityDelete(id)),
        &p,
        app.confirm_dialog_anim,
    )
}

fn divider(p: theme::ThemePalette) -> Element<'static, Message> {
    container(column![])
        .width(Length::Fill)
        .height(1)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..container::Style::default()
        })
        .into()
}

fn view_form(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let is_edit = app.keychain.form.editing_id.is_some();

    let title = if is_edit {
        t_static(t!("keychain.edit_key"))
    } else {
        t_static(t!("keychain.generate_key_title"))
    };
    let header = form_header(
        title.to_string(),
        Message::Keychain(crate::keychain::message::Message::KeychainHideForm),
        p,
    );

    let name_input = labeled_input(
        t_static(t!("keychain.name")),
        "e.g. My GitHub Key",
        &app.keychain.form.name,
        &p,
        |v0| Message::Keychain(crate::keychain::message::Message::KeychainFormName(v0)),
    );

    let path_input = labeled_input(
        t_static(t!("keychain.key_path")),
        "e.g. ~/.ssh/id_ed25519",
        &app.keychain.form.path,
        &p,
        |v0| Message::Keychain(crate::keychain::message::Message::KeychainFormPath(v0)),
    );

    let can_save = app.keychain.form.save_enabled();

    let save_btn: Element<'static, Message> = if app.keychain.form.in_progress {
        // Self-animating ring indicator marking that key generation is in flight.
        let indicator: Element<'static, Message> =
            crate::ui::loading_indicator::loading_indicator(14.0, Color::WHITE).into();
        button(
            row![
                indicator,
                text(t_static(t!("keychain.generating")))
                    .size(14)
                    .color(Color::WHITE),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding(control_padding(14.0, 14.0))
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(Background::Color(p.accent)),
            text_color: Color::WHITE,
            border: Border {
                radius: 5.0.into(),
                width: 1.0,
                color: p.accent,
            },
            ..button::Style::default()
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
    } else {
        styled_btn(
            if is_edit {
                t_static(t!("settings.save"))
            } else {
                t_static(t!("keychain.generate_save"))
            },
            can_save.then_some(Message::Keychain(
                crate::keychain::message::Message::KeychainFormSubmit,
            )),
            p.accent,
            Color::WHITE,
            p,
        )
    };
    let actions = form_actions_with_save(
        save_btn,
        t_static(t!("settings.cancel")),
        Message::Keychain(crate::keychain::message::Message::KeychainHideForm),
        app.keychain.form.editing_id.map(|editing_id| {
            Message::Keychain(crate::keychain::message::Message::KeychainDeleteRequest(
                editing_id,
            ))
        }),
        p,
    );

    if is_edit {
        let key_type_label = labeled_readonly(
            t_static(t!("keychain.key_type_label")),
            app.keychain.form.key_type.clone(),
            &p,
        );
        let fingerprint_input = labeled_input(
            t_static(t!("keychain.fingerprint")),
            "SHA256:...",
            &app.keychain.form.fingerprint,
            &p,
            |v0| {
                Message::Keychain(crate::keychain::message::Message::KeychainFormFingerprint(
                    v0,
                ))
            },
        );
        let public_key_input = labeled_input(
            t_static(t!("keychain.public_key")),
            "ssh-ed25519 AAAA...",
            &app.keychain.form.public_key,
            &p,
            |v0| Message::Keychain(crate::keychain::message::Message::KeychainFormPublicKey(v0)),
        );

        let form_content = column![
            header,
            name_input,
            key_type_label,
            fingerprint_input,
            public_key_input,
            path_input,
        ]
        .spacing(14)
        .push(actions)
        .padding(20);

        form_panel(form_content, 340.0, p)
    } else {
        let key_type_picker = view_key_type_picker(app, p);
        let passphrase_input = labeled_password_input(
            t_static(t!("keychain.passphrase")),
            t_static(t!("keychain.passphrase_hint")),
            &app.keychain.form.passphrase,
            &p,
            |s| {
                Message::Keychain(crate::keychain::message::Message::KeychainFormPassphrase(
                    SecretString::from(s),
                ))
            },
            !app.keychain.form.passphrase_visible,
            Message::Keychain(crate::keychain::message::Message::KeychainFormTogglePasswordVisible),
        );

        let form_content = column![
            header,
            name_input,
            key_type_picker,
            path_input,
            passphrase_input,
        ]
        .spacing(14)
        .push(actions)
        .padding(20);

        form_panel(form_content, 340.0, p)
    }
}

#[allow(
    clippy::needless_update,
    reason = "every field is written out so the call site reads whole"
)]
fn view_key_type_picker(app: &App, p: theme::ThemePalette) -> Element<'_, Message> {
    let selected = app.keychain.form.selected_key_type;
    let mut btn_row = row![].spacing(6);

    for kt in KeyType::all() {
        let is_selected = selected == Some(kt);
        let bg = if is_selected { p.accent } else { p.surface };
        let fg = if is_selected {
            Color::WHITE
        } else {
            p.text_sec
        };

        let picker_btn = button(text(kt.label()).size(12).color(fg))
            .on_press(Message::Keychain(
                crate::keychain::message::Message::KeychainFormKeyType(kt),
            ))
            .padding(control_padding(12.0, 10.0))
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered);
                let bg = if is_selected {
                    bg
                } else if hovered {
                    p.item_hover
                } else {
                    bg
                };
                button::Style {
                    background: Some(Background::Color(bg)),
                    text_color: fg,
                    border: Border {
                        radius: 5.0.into(),
                        width: 1.0,
                        color: if is_selected {
                            Color::TRANSPARENT
                        } else {
                            p.border
                        },
                    },
                    ..button::Style::default()
                }
            })
            .cursor_interaction(mouse::Interaction::default());

        btn_row = btn_row.push(picker_btn);
    }

    column![
        text(t_static(t!("keychain.key_type_label")).to_string())
            .size(12)
            .color(p.text_sec),
        btn_row,
    ]
    .spacing(4)
    .into()
}

pub fn view_keychain_delete_confirm(
    app: &App,
    p: theme::ThemePalette,
) -> Element<'static, Message> {
    let Some(id) = app.keychain.delete_pending else {
        return text("").into();
    };
    let name = app
        .keychain
        .list
        .iter()
        .find(|e| e.id == id)
        .map_or(t_static(t!("keychain.delete_key_fallback")), |e| {
            e.name.as_str()
        });
    let body = t!("keychain.delete_key_confirm", name = name);
    delete_confirm_dialog(
        t!("keychain.delete_key_title"),
        body,
        Message::Keychain(crate::keychain::message::Message::KeychainDeleteCancel),
        Message::Keychain(crate::keychain::message::Message::KeychainDelete(id)),
        &p,
        app.confirm_dialog_anim,
    )
}

pub fn view_keychain_context_menu(
    menu: &crate::keychain::KeychainContextMenu,
    p: theme::ThemePalette,
) -> Element<'_, Message> {
    const MENU_WIDTH: f32 = 140.0;
    use crate::core::context_menu::ContextMenuAction as Action;

    let actions = menu.actions();
    let focused = menu.cursor().selected_or_first(actions).copied();
    let (id, position, is_key) = match menu {
        crate::keychain::KeychainContextMenu::Key { id, position, .. } => (id.0, *position, true),
        crate::keychain::KeychainContextMenu::Identity { id, position, .. } => {
            (id.0, *position, false)
        }
    };

    let close_msg = if is_key {
        Message::Keychain(crate::keychain::message::Message::KeychainContextMenuClose)
    } else {
        Message::Keychain(crate::keychain::message::Message::IdentityContextMenuClose)
    };

    crate::ui::overlay::context_menu_frame(
        position,
        move || {
            let edit_msg = if is_key {
                Message::Keychain(crate::keychain::message::Message::KeychainEdit(
                    KeychainEntryId(id),
                ))
            } else {
                Message::Keychain(crate::keychain::message::Message::IdentityEdit(IdentityId(
                    id,
                )))
            };
            let delete_msg = if is_key {
                Message::Keychain(crate::keychain::message::Message::KeychainDeleteRequest(
                    KeychainEntryId(id),
                ))
            } else {
                Message::Keychain(crate::keychain::message::Message::IdentityDeleteRequest(
                    IdentityId(id),
                ))
            };
            vec![
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Pencil,
                    t_static(t!("context_menu.edit")),
                    edit_msg,
                    false,
                    focused
                        == Some(if is_key {
                            Action::KeychainEdit
                        } else {
                            Action::IdentityEdit
                        }),
                    p,
                ),
                crate::core::widgets::menu_item(
                    lucide_icons::Icon::Trash2,
                    t_static(t!("dialog.delete")),
                    delete_msg,
                    true,
                    focused
                        == Some(if is_key {
                            Action::KeychainDelete
                        } else {
                            Action::IdentityDelete
                        }),
                    p,
                ),
            ]
        },
        MENU_WIDTH,
        close_msg,
        p,
    )
}
