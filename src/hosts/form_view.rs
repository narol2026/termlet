use iced::widget::{button, column, row, text};
use iced::{Background, Border, Element, Font, Length, Theme, mouse};
use secrecy::SecretString;

use crate::app::App;
use crate::core::data::AuthType;
use crate::core::dropdown::DropdownTarget;
use crate::core::forms::FormDraft;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme;
use crate::core::widgets::{
    DropdownSearch, SelectOption, control_padding, form_actions, form_header, form_panel,
    labeled_input, labeled_password_input, segment_btn, select_dropdown, tag_chip,
    tag_select_option,
};
use rust_i18n::t;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_form(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let title = if app.hosts.form.editing_id.is_some() {
        t_static(t!("hosts.edit_host"))
    } else {
        t_static(t!("hosts.new_host"))
    };
    let header = form_header(
        title.to_string(),
        Message::Hosts(crate::hosts::message::Message::HideForm),
        p,
    );

    let name_input = labeled_input(
        t_static(t!("form.name")),
        "e.g. My Server",
        &app.hosts.form.name,
        &p,
        |v0| Message::Hosts(crate::hosts::message::Message::FormName(v0)),
    );
    let host_input = labeled_input(
        t_static(t!("form.hostname")),
        "e.g. 192.168.1.100",
        &app.hosts.form.hostname,
        &p,
        |v0| Message::Hosts(crate::hosts::message::Message::FormHostname(v0)),
    );
    let port_input = labeled_input(
        t_static(t!("form.port")),
        "22",
        &app.hosts.form.port,
        &p,
        |v0| Message::Hosts(crate::hosts::message::Message::FormPort(v0)),
    );
    // Selecting an identity supplies the username and password, so hide the
    // manual fields to make it clear they are no longer in play.
    let identity_selected = app.hosts.form.selected_identity_id.is_some();
    let user_input = if identity_selected {
        None
    } else {
        Some(labeled_input(
            t_static(t!("form.username")),
            "e.g. root",
            &app.hosts.form.username,
            &p,
            |v0| Message::Hosts(crate::hosts::message::Message::FormUsername(v0)),
        ))
    };
    let key_path_input = (app.hosts.form.auth == AuthType::Key).then(|| {
        labeled_input(
            t_static(t!("form.private_key")),
            "e.g. ~/.ssh/id_ed25519",
            &app.hosts.form.key_path,
            &p,
            |v0| Message::Hosts(crate::hosts::message::Message::FormKeyPath(v0)),
        )
    });
    let keychain_picker =
        (app.hosts.form.auth == AuthType::Key).then(|| view_keychain_section(app));

    let password_input =
        (app.hosts.form.auth == AuthType::Password && !identity_selected).then(|| {
            labeled_password_input(
                t_static(t!("form.password")),
                t_static(t!("form.saved_ssh_password")),
                &app.hosts.form.password,
                &p,
                |s| {
                    Message::Hosts(crate::hosts::message::Message::FormPassword(
                        SecretString::from(s),
                    ))
                },
                !app.hosts.form.password_visible,
                Message::Hosts(crate::hosts::message::Message::FormTogglePasswordVisible),
            )
        });

    let identity_section = view_identity_section(app);

    let auth_label = text(t_static(t!("hosts.auth_type")))
        .size(12)
        .color(p.text_sec);
    let auth_btns = row(AuthType::all().iter().map(|&at| {
        segment_btn(
            at.to_string(),
            app.hosts.form.auth == at,
            Message::Hosts(crate::hosts::message::Message::FormAuth(at)),
            p,
        )
    }))
    .spacing(6);

    let auth_section = column![auth_label, auth_btns].spacing(6);
    let jump_section = view_jump_section(app, p);

    let can_save = app.hosts.form.save_enabled();
    let actions = form_actions(
        t_static(t!("settings.save")),
        t_static(t!("settings.cancel")),
        can_save,
        Message::Hosts(crate::hosts::message::Message::SaveHost),
        Message::Hosts(crate::hosts::message::Message::HideForm),
        app.hosts.form.editing_id.map(|editing_id| {
            Message::Hosts(crate::hosts::message::Message::HostDeleteRequest(
                editing_id,
            ))
        }),
        p,
    );

    let mut form_content = column![
        header,
        name_input,
        host_input,
        port_input,
        auth_section,
        jump_section
    ]
    .spacing(14);

    if let Some(user_input) = user_input {
        form_content = form_content.push(user_input);
    }
    if let Some(key_path_input) = key_path_input {
        form_content = form_content.push(key_path_input);
    }
    if let Some(kp) = keychain_picker {
        form_content = form_content.push(kp);
    }
    if app.hosts.form.auth == AuthType::Password {
        if let Some(password_input) = password_input {
            form_content = form_content.push(password_input);
        }
        form_content = form_content.push(identity_section);
    }

    form_content = form_content.push(view_tags_section(app, p));

    form_content = form_content.push(actions);

    form_content = form_content.padding(20);

    form_panel(form_content, 320.0, p)
}

fn view_keychain_section(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let label = text(t_static(t!("form.from_keychain")))
        .size(12)
        .color(p.text_sec);

    // A keychain entry is "selected" when its path matches the current key
    // path draft, so editing a saved host highlights the key it points at.
    let current = app
        .keychain
        .list
        .iter()
        .find(|entry| entry.path == app.hosts.form.key_path);
    let current_label = match current {
        Some(entry) => entry.name.clone(),
        None => t_static(t!("hosts.select_key")).to_string(),
    };

    let empty_state = if app.keychain.list.is_empty() {
        t_static(t!("form.no_keys_hint"))
    } else {
        t_static(t!("keychain.no_keys_match"))
    };

    let items = crate::cursor::host_form_keychain_ids(app);
    let focused = app.hosts.form.keychain_picker.active(&items).copied();
    let options: Vec<SelectOption> = if app.active_dropdown
        == Some(DropdownTarget::HostFormKeychain)
    {
        items
            .into_iter()
            .filter_map(|id| {
                let entry = app.keychain.list.iter().find(|entry| entry.id == id)?;
                Some(SelectOption {
                    label: entry.name.clone(),
                    subtitle: Some(entry.path.clone()),
                    active: entry.path == app.hosts.form.key_path,
                    focused: focused == Some(id),
                    on_select: Message::Hosts(crate::hosts::message::Message::FormFillKeyPath(id)),
                    leading: Some(
                        text(char::from(lucide_icons::Icon::KeyRound))
                            .font(LUCIDE_FONT)
                            .size(12)
                            .color(p.accent)
                            .into(),
                    ),
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
        app.active_dropdown == Some(DropdownTarget::HostFormKeychain),
        DropdownTarget::HostFormKeychain,
        Message::Hosts(crate::hosts::message::Message::FormToggleKeychainPicker),
        Length::Fill,
        Some(DropdownSearch {
            placeholder: t_static(t!("keychain.search_keys")),
            query: app.hosts.form.keychain_picker.query(),
            on_input: |v0| Message::Hosts(crate::hosts::message::Message::FormKeychainSearch(v0)),
        }),
        Some(empty_state),
        p,
    );

    column![label, dropdown].spacing(6).into()
}

fn view_jump_section(app: &App, p: theme::ThemePalette) -> Element<'_, Message> {
    let label = text(t_static(t!("hosts.jump_host")))
        .size(12)
        .color(p.text_sec);

    let current = app
        .hosts
        .list
        .iter()
        .find(|h| Some(h.id) == app.hosts.form.jump_host_id);
    let current_label = match current {
        Some(host) => host.name.clone(),
        None => t_static(t!("hosts.no_jump_host")).to_string(),
    };

    let items = crate::cursor::host_form_jump_ids(app);
    let focused = app.hosts.form.jump_picker.active(&items).copied();
    let options: Vec<SelectOption> = if app.active_dropdown == Some(DropdownTarget::HostFormJump) {
        items
            .into_iter()
            .filter_map(|id| {
                let host = app.hosts.list.iter().find(|host| host.id == id)?;
                Some(SelectOption {
                    label: host.name.clone(),
                    subtitle: Some(format!("{}@{}:{}", host.username, host.hostname, host.port)),
                    active: Some(host.id) == app.hosts.form.jump_host_id,
                    focused: focused == Some(id),
                    on_select: Message::Hosts(crate::hosts::message::Message::FormSelectJump(id)),
                    leading: Some(
                        text(char::from(lucide_icons::Icon::Network))
                            .font(LUCIDE_FONT)
                            .size(12)
                            .color(p.accent)
                            .into(),
                    ),
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
        app.active_dropdown == Some(DropdownTarget::HostFormJump),
        DropdownTarget::HostFormJump,
        Message::Hosts(crate::hosts::message::Message::FormToggleJumpPicker),
        Length::Fill,
        Some(DropdownSearch {
            placeholder: t_static(t!("hosts.search_jump_hosts")),
            query: app.hosts.form.jump_picker.query(),
            on_input: |v0| Message::Hosts(crate::hosts::message::Message::FormJumpSearch(v0)),
        }),
        Some(t_static(t!("hosts.no_jump_host_options"))),
        p,
    );

    let picker: Element<'_, Message> = if app.hosts.form.jump_host_id.is_some() {
        let clear_btn = button(
            text(char::from(lucide_icons::Icon::X))
                .font(LUCIDE_FONT)
                .size(12)
                .color(p.text_sec),
        )
        .on_press(Message::Hosts(
            crate::hosts::message::Message::FormClearJump,
        ))
        .padding(control_padding(12.0, 10.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.item_hover
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
        .cursor_interaction(mouse::Interaction::Pointer);

        row![dropdown, clear_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        dropdown
    };

    column![label, picker].spacing(6).into()
}

fn view_identity_section(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let label = text(t_static(t!("hosts.from_identities")))
        .size(12)
        .color(p.text_sec);

    let current = app
        .keychain
        .identities
        .iter()
        .find(|i| Some(i.id) == app.hosts.form.selected_identity_id);
    let current_label = match current {
        Some(id) => id.label.clone(),
        None => t_static(t!("hosts.select_identity")).to_string(),
    };

    let empty_state = if app.keychain.identities.is_empty() {
        t_static(t!("hosts.no_identities"))
    } else {
        t_static(t!("hosts.no_identity_match"))
    };

    let items = crate::cursor::host_form_identity_ids(app);
    let focused = app.hosts.form.identity_picker.active(&items).copied();
    let options: Vec<SelectOption> =
        if app.active_dropdown == Some(DropdownTarget::HostFormIdentity) {
            items
                .into_iter()
                .filter_map(|id| {
                    let identity = app.keychain.identities.iter().find(|item| item.id == id)?;
                    Some(SelectOption {
                        label: identity.label.clone(),
                        subtitle: None,
                        active: Some(id) == app.hosts.form.selected_identity_id,
                        focused: focused == Some(id),
                        on_select: Message::Hosts(
                            crate::hosts::message::Message::FormSelectIdentity(id),
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
        app.active_dropdown == Some(DropdownTarget::HostFormIdentity),
        DropdownTarget::HostFormIdentity,
        Message::Hosts(crate::hosts::message::Message::FormToggleIdentityPicker),
        Length::Fill,
        Some(DropdownSearch {
            placeholder: t_static(t!("hosts.search_identities")),
            query: app.hosts.form.identity_picker.query(),
            on_input: |v0| Message::Hosts(crate::hosts::message::Message::FormIdentitySearch(v0)),
        }),
        Some(empty_state),
        p,
    );

    // Once an identity is selected the username/password inputs are hidden, so
    // offer an explicit clear control to return to manual entry.
    let picker: Element<'_, Message> = if app.hosts.form.selected_identity_id.is_some() {
        let clear_btn = button(
            text(char::from(lucide_icons::Icon::X))
                .font(LUCIDE_FONT)
                .size(12)
                .color(p.text_sec),
        )
        .on_press(Message::Hosts(
            crate::hosts::message::Message::FormClearIdentity,
        ))
        .padding(control_padding(12.0, 10.0))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    p.item_hover
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
        .cursor_interaction(mouse::Interaction::Pointer);

        row![dropdown, clear_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .into()
    } else {
        dropdown
    };

    column![label, picker].spacing(6).into()
}

/// Host-form tag editor: removable chips for the chosen tags plus a dropdown
/// picker that lists existing tags to select and can create a brand-new one.
fn view_tags_section(app: &App, p: theme::ThemePalette) -> Element<'_, Message> {
    let label = text(t_static(t!("hosts.tags"))).size(12).color(p.text_sec);

    let chips: Element<'_, Message> = if app.hosts.form.tags.is_empty() {
        text(t_static(t!("hosts.no_tags")))
            .size(11)
            .color(p.text_dim)
            .into()
    } else {
        row(app
            .hosts
            .form
            .tags
            .iter()
            .map(|name| {
                tag_chip(
                    name.clone(),
                    Message::Hosts(crate::hosts::message::Message::FormTagRemove(name.clone())),
                    p,
                )
            })
            .collect::<Vec<_>>())
        .spacing(4)
        .width(Length::Fill)
        .wrap()
        .into()
    };

    // Build the picker options. A persistent "new tag" row leads the list so
    // creation is always discoverable: it prompts to type when the search box
    // is empty and turns into "Create \"x\"" once a brand-new name is typed.
    let items = crate::cursor::host_form_tag_ids(app);
    let focused = app.hosts.form.tag_picker.active(&items).cloned();
    let options: Vec<SelectOption> = if app.active_dropdown == Some(DropdownTarget::HostFormTag) {
        items
            .into_iter()
            .map(|item| {
                let is_focused = focused.as_ref() == Some(&item);
                match item {
                    crate::hosts::TagPickerItem::Create(candidate) => SelectOption {
                        label: if candidate.is_empty() {
                            t_static(t!("hosts.new_tag")).to_string()
                        } else {
                            t!("hosts.create_tag", name = candidate.as_str()).to_string()
                        },
                        subtitle: None,
                        active: false,
                        focused: is_focused,
                        on_select: Message::Hosts(crate::hosts::message::Message::FormTagCreate),
                        leading: Some(
                            text(char::from(lucide_icons::Icon::Plus))
                                .font(LUCIDE_FONT)
                                .size(12)
                                .color(p.text_dim)
                                .into(),
                        ),
                    },
                    crate::hosts::TagPickerItem::Existing(tag) => {
                        let mut option = tag_select_option(
                            &tag,
                            false,
                            Message::Hosts(crate::hosts::message::Message::FormTagAdd(tag.clone())),
                        );
                        option.focused = is_focused;
                        option
                    }
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let leading = Some(
        text(char::from(lucide_icons::Icon::Plus))
            .font(LUCIDE_FONT)
            .size(12)
            .color(p.text_sec)
            .into(),
    );

    let dropdown = select_dropdown(
        t_static(t!("hosts.add_tag")),
        leading,
        options,
        app.active_dropdown == Some(DropdownTarget::HostFormTag),
        DropdownTarget::HostFormTag,
        Message::Hosts(crate::hosts::message::Message::FormTagPickerToggle),
        Length::Fill,
        Some(DropdownSearch {
            placeholder: t_static(t!("hosts.search_tags")),
            query: app.hosts.form.tag_picker.query(),
            on_input: |v0| Message::Hosts(crate::hosts::message::Message::FormTagSearch(v0)),
        }),
        Some(t_static(t!("hosts.no_tags_to_add"))),
        p,
    );

    column![label, chips, dropdown].spacing(6).into()
}
