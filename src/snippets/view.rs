use iced::widget::{
    Space, column, container, mouse_area, responsive, row, sensor, text, text_editor,
};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme};

use crate::app::App;
use crate::core::collection::{
    CARD_HEIGHT, EMPTY_HEIGHT, ROW_GAP, SECTION_HEADER_GAP, SECTION_HEADER_HEIGHT,
};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{Snippet, SnippetId, SnippetPackageId};
use crate::core::dropdown::DropdownTarget;
use crate::core::forms::FormDraft;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::{self, ThemePalette};
use crate::core::widgets::{
    SelectOption, card_hover_btn, card_info, card_wrapper, collection_layout, collection_rows,
    form_actions, form_header, form_panel, icon_box, labeled_input, select_dropdown,
    styled_scrollable, toolbar_icon_btn, toolbar_outline_btn, toolbar_primary_btn,
};
use crate::ui::confirm_dialog::delete_confirm_dialog;
use crate::ui::cursor_area::CursorArea;
use crate::ui::slide::slide_panel_from_right;
use rust_i18n::t;

pub const SNIPPETS_SCROLL_ID: &str = "snippets_scroll";
pub(crate) const SNIPPETS_GRID_TOP_PADDING: f32 = 6.0;
pub(crate) const SNIPPETS_PACKAGE_GRID_BOTTOM_PADDING: f32 = 10.0;
pub(crate) const SNIPPETS_GRID_BOTTOM_PADDING: f32 = 20.0;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_snippets(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let add_bar = view_add_bar(app);
    let divider1 = divider(p);
    let packages_section = view_packages_section(app);
    let divider2 = divider(p);
    let snippets_section = view_snippets_section(app);

    let scroll_content = CursorArea::for_target(
        column![packages_section, divider2, snippets_section]
            .spacing(16)
            .width(Length::Fill),
        CursorTarget::Snippets,
    );

    let body = column![
        add_bar,
        divider1,
        styled_scrollable(
            scroll_content,
            p,
            app.is_scrollbar_visible(SNIPPETS_SCROLL_ID)
        )
        .id(iced::widget::Id::from(SNIPPETS_SCROLL_ID))
        .on_scroll(move |v| Message::Scrolled(SNIPPETS_SCROLL_ID, v))
        .height(Length::Fill),
    ]
    .spacing(0)
    .width(Length::Fill)
    .height(Length::Fill);

    let form_content: Element<'_, Message> = if app.snippets.package_form.visible {
        view_package_form(app)
    } else {
        view_snippet_form(app)
    };
    let form = slide_panel_from_right(
        form_content,
        380.0,
        app.snippets.form.visible || app.snippets.package_form.visible,
        app.snippets.form_anim,
    );

    let page: Element<'_, Message> = row![body, form].spacing(0).height(Length::Fill).into();

    mouse_area(page)
        .on_move(|v0| {
            Message::Snippets(crate::snippets::message::Message::SnippetViewCursorMoved(
                v0,
            ))
        })
        .into()
}

fn view_add_bar(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    // NEW SNIPPET — primary accent button
    let new_snippet_btn = toolbar_primary_btn(
        lucide_icons::Icon::Plus,
        t_static(t!("snippets.new_snippet")),
        Message::Snippets(crate::snippets::message::Message::SnippetShowForm),
        p,
    );

    // NEW PACKAGE — outline style (matches TERMINAL button)
    let new_package_btn = toolbar_outline_btn(
        lucide_icons::Icon::Package,
        t_static(t!("snippets.new_package")),
        Message::Snippets(crate::snippets::message::Message::SnippetPackageShowForm),
        p,
    );

    // View toggle button (list/grid)
    let view_icon = if app.snippets.grid_view {
        lucide_icons::Icon::List
    } else {
        lucide_icons::Icon::LayoutGrid
    };
    let view_btn = toolbar_icon_btn(
        view_icon,
        Message::Snippets(crate::snippets::message::Message::SnippetToggleView),
        p,
    );

    container(
        row![
            new_snippet_btn,
            new_package_btn,
            Space::new().width(Length::Fill),
            view_btn,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding::from([13.0, 20.0]))
    .width(Length::Fill)
    .into()
}

fn view_packages_section(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let header = container(
        row![
            text(t_static(t!("snippets.packages_title")))
                .size(14)
                .color(p.text_pri),
            Space::new().width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding {
        top: 8.0,
        right: 20.0,
        bottom: 0.0,
        left: 20.0,
    })
    .height(Length::Fixed(SECTION_HEADER_HEIGHT))
    .width(Length::Fill);

    if app.snippets.packages.is_empty() {
        let empty = container(
            text(t_static(t!("snippets.no_packages_yet")))
                .size(12)
                .color(p.text_dim),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding::from([20.0, 20.0]))
        .height(Length::Fixed(EMPTY_HEIGHT));
        return column![header, empty].spacing(4).into();
    }

    let grid = view_packages_grid(app);
    column![header, grid].spacing(SECTION_HEADER_GAP).into()
}

fn view_packages_grid(app: &App) -> Element<'_, Message> {
    let grid_view = app.snippets.grid_view;

    container(responsive(move |size| {
        let layout = collection_layout(size.width, grid_view, 320.0, 5);
        let grid = collection_rows(
            app.snippets
                .packages
                .iter()
                .map(|package| view_package_row(app, package, layout.item_width)),
            layout,
            CARD_HEIGHT,
            ROW_GAP,
        );
        let columns = layout.columns;

        sensor(grid)
            .on_resize(move |_| Message::CursorColumns(CursorTarget::Snippets, columns))
            .into()
    }))
    .padding(Padding {
        top: SNIPPETS_GRID_TOP_PADDING,
        right: 20.0,
        bottom: SNIPPETS_PACKAGE_GRID_BOTTOM_PADDING,
        left: 20.0,
    })
    .width(Length::Fill)
    .into()
}

fn view_package_row<'a>(
    app: &'a App,
    pkg: &'a crate::core::data::SnippetPackage,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let snippet_count = app
        .snippets
        .list
        .iter()
        .filter(|s| s.package_id == Some(pkg.id))
        .count();

    let icon = icon_box(
        text(char::from(lucide_icons::Icon::Package))
            .font(LUCIDE_FONT)
            .size(20)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let subtitle = if pkg.description.is_empty() {
        format!("{snippet_count} snippet(s)")
    } else {
        format!("{} — {} snippet(s)", pkg.description, snippet_count)
    };

    let hovered = app.snippets.hovered_package_id == Some(pkg.id);
    let selected = app.snippets.selected_package() == Some(pkg.id);
    let info = card_info(&pkg.name.clone(), subtitle, card_width, hovered, p);

    let actions: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::Snippets(crate::snippets::message::Message::SnippetPackageEdit(
                pkg.id,
            )),
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
        Message::Snippets(crate::snippets::message::Message::SnippetPackageHoverEnter(
            pkg.id,
        )),
        Message::Snippets(crate::snippets::message::Message::SnippetPackageHoverLeave(
            pkg.id,
        )),
        Some(Message::Snippets(
            crate::snippets::message::Message::SnippetPackageSelect(pkg.id),
        )),
        None,
        Some(Message::Snippets(
            crate::snippets::message::Message::SnippetPackageContextMenuOpen(pkg.id),
        )),
        p,
    )
}

fn view_snippets_section(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let header = container(
        row![
            text(t_static(t!("snippets.snippets_title")))
                .size(14)
                .color(p.text_pri),
            Space::new().width(Length::Fill),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding(Padding {
        top: 8.0,
        right: 20.0,
        bottom: 0.0,
        left: 20.0,
    })
    .height(Length::Fixed(SECTION_HEADER_HEIGHT))
    .width(Length::Fill);

    column![header, view_snippets_grid(app)]
        .spacing(SECTION_HEADER_GAP)
        .into()
}

fn view_snippets_grid(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    if app.snippets.list.is_empty() {
        return container(
            column![
                text(char::from(lucide_icons::Icon::Code))
                    .font(LUCIDE_FONT)
                    .size(40)
                    .color(p.text_dim),
                text(t_static(t!("snippets.no_snippets_yet")))
                    .size(14)
                    .color(p.text_pri),
                text(t_static(t!("snippets.click_new_snippet")))
                    .size(12)
                    .color(p.text_sec),
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding::from([40.0, 20.0]))
        .into();
    }

    let grid_view = app.snippets.grid_view;

    container(responsive(move |size| {
        let layout = collection_layout(size.width, grid_view, 320.0, 5);
        let grid = collection_rows(
            app.snippets
                .list
                .iter()
                .map(|snippet| view_snippet_row(app, snippet, layout.item_width)),
            layout,
            CARD_HEIGHT,
            ROW_GAP,
        );
        let columns = layout.columns;

        sensor(grid)
            .on_resize(move |_| Message::CursorColumns(CursorTarget::Snippets, columns))
            .into()
    }))
    .padding(Padding {
        top: SNIPPETS_GRID_TOP_PADDING,
        right: 20.0,
        bottom: SNIPPETS_GRID_BOTTOM_PADDING,
        left: 20.0,
    })
    .width(Length::Fill)
    .into()
}

fn view_snippet_row<'a>(
    app: &'a App,
    snippet: &'a Snippet,
    card_width: f32,
) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let id = snippet.id;

    let icon = icon_box(
        text(char::from(lucide_icons::Icon::Braces))
            .font(LUCIDE_FONT)
            .size(20)
            .color(Color::WHITE)
            .into(),
        p.accent,
    );

    let package_badge: Element<'_, Message> = if let Some(pid) = snippet.package_id {
        if let Some(pkg) = app.snippets.packages.iter().find(|p| p.id == pid) {
            container(
                text(pkg.name.clone())
                    .size(12)
                    .color(p.accent)
                    .ellipsis(text::Ellipsis::End)
                    .wrapping(text::Wrapping::None),
            )
            .padding(Padding::from([2.0, 6.0]))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(Color {
                    r: p.accent.r,
                    g: p.accent.g,
                    b: p.accent.b,
                    a: 0.12,
                })),
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..Default::default()
            })
            .into()
        } else {
            Space::new().width(0).height(0).into()
        }
    } else {
        Space::new().width(0).height(0).into()
    };

    let hovered = app.snippets.hovered_snippet_id == Some(id);
    let selected = app.snippets.selected_snippet() == Some(id);
    let info_width = card_width - 92.0 - if hovered { 28.0 } else { 0.0 };

    let desc_text: Element<'_, Message> = text(&snippet.description)
        .size(14.0)
        .color(p.text_pri)
        .width(Length::Fixed(info_width))
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None)
        .into();
    let desc_row = row![desc_text, package_badge]
        .spacing(8)
        .align_y(iced::Alignment::Center);

    let info = column![
        desc_row,
        text(&snippet.script)
            .size(11.0)
            .color(p.text_sec)
            .width(Length::Fixed(info_width))
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
    ]
    .spacing(2);

    let actions: Element<'_, Message> = if hovered {
        card_hover_btn(
            lucide_icons::Icon::Pencil,
            Message::Snippets(crate::snippets::message::Message::SnippetEdit(id)),
            p,
        )
    } else {
        Space::new().width(Length::Shrink).into()
    };

    let content = row![icon, info.width(Length::Fill), actions]
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
        Message::Snippets(crate::snippets::message::Message::SnippetHoverEnter(id)),
        Message::Snippets(crate::snippets::message::Message::SnippetHoverLeave(id)),
        Some(Message::Snippets(
            crate::snippets::message::Message::SnippetCopyScript(id),
        )),
        None,
        Some(Message::Snippets(
            crate::snippets::message::Message::SnippetContextMenuOpen(id),
        )),
        p,
    )
}

fn view_package_form(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let is_edit = app.snippets.package_form.editing_id.is_some();

    let header = form_header(
        if is_edit {
            t_static(t!("snippets.edit_package"))
        } else {
            t_static(t!("snippets.new_package"))
        }
        .to_string(),
        Message::Snippets(crate::snippets::message::Message::SnippetPackageHideForm),
        p,
    );

    let name_input = labeled_input(
        t_static(t!("form.name")),
        "e.g. Docker, Git, System",
        &app.snippets.package_form.name,
        &p,
        |v0| {
            Message::Snippets(crate::snippets::message::Message::SnippetPackageFormName(
                v0,
            ))
        },
    );

    let desc_input = labeled_input(
        t_static(t!("form.description")),
        "e.g. Common Docker maintenance commands",
        &app.snippets.package_form.description,
        &p,
        |v0| {
            Message::Snippets(crate::snippets::message::Message::SnippetPackageFormDescription(v0))
        },
    );

    let can_save = app.snippets.package_form.save_enabled();
    let delete_msg = app.snippets.package_form.editing_id.map(|v0| {
        Message::Snippets(crate::snippets::message::Message::SnippetPackageDeleteRequest(v0))
    });
    let actions = form_actions(
        if is_edit {
            t_static(t!("settings.save"))
        } else {
            t_static(t!("snippets.create_package"))
        },
        t_static(t!("settings.cancel")),
        can_save,
        Message::Snippets(crate::snippets::message::Message::SnippetPackageFormSubmit),
        Message::Snippets(crate::snippets::message::Message::SnippetPackageHideForm),
        delete_msg,
        p,
    );

    let form_content = column![header, name_input, desc_input, actions]
        .spacing(14)
        .padding(20);

    form_panel(form_content, 380.0, p)
}

fn view_snippet_form(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let is_edit = app.snippets.form.editing_id.is_some();

    let header = form_header(
        if is_edit {
            t_static(t!("snippets.edit_snippet"))
        } else {
            t_static(t!("snippets.new_snippet"))
        }
        .to_string(),
        Message::Snippets(crate::snippets::message::Message::SnippetHideForm),
        p,
    );

    // Package picker (optional — no package selected by default).
    let package_label = text(t_static(t!("snippets.package_optional_label")))
        .size(12)
        .color(p.text_sec);

    let current_label = app
        .snippets
        .form
        .package_id
        .and_then(|id| app.snippets.packages.iter().find(|pk| pk.id == id))
        .map_or(t_static(t!("snippets.package_none")), |pk| pk.name.as_str());

    let items = crate::cursor::snippet_package_ids(app);
    let focused = app.snippets.form.package_picker.active(&items).copied();
    let options: Vec<SelectOption> =
        if app.active_dropdown == Some(DropdownTarget::SnippetFormPackage) {
            items
                .into_iter()
                .filter_map(|id| {
                    let package = app.snippets.packages.iter().find(|pkg| pkg.id == id)?;
                    Some(SelectOption {
                        label: package.name.clone(),
                        subtitle: None,
                        active: app.snippets.form.package_id == Some(id),
                        focused: focused == Some(id),
                        on_select: Message::Snippets(
                            crate::snippets::message::Message::SnippetFormPackage(Some(id)),
                        ),
                        leading: None,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

    let package_picker = select_dropdown(
        current_label,
        None,
        options,
        app.active_dropdown == Some(DropdownTarget::SnippetFormPackage),
        DropdownTarget::SnippetFormPackage,
        Message::Snippets(crate::snippets::message::Message::SnippetFormTogglePackagePicker),
        Length::Fill,
        None,
        Some(t_static(t!("snippets.no_packages_yet"))),
        p,
    );

    let package_section = column![package_label, package_picker].spacing(4);

    let desc_input = labeled_input(
        t_static(t!("snippets.action_description")),
        t_static(t!("snippets.action_description_hint")),
        &app.snippets.form.description,
        &p,
        |v0| {
            Message::Snippets(crate::snippets::message::Message::SnippetFormDescription(
                v0,
            ))
        },
    );

    let script_label = text(t_static(t!("snippets.script_label")))
        .size(12)
        .color(p.text_sec);
    let script_input = text_editor(&app.snippets.form.script)
        .placeholder(t_static(t!("snippets.script_placeholder")))
        .on_action(|v0| Message::Snippets(crate::snippets::message::Message::SnippetFormScript(v0)))
        // Bare Enter = submit (global handler fires SnippetFormSubmit); swallow
        // it here so the editor doesn't also insert a newline. Shift+Enter keeps
        // newline for multi-line scripts.
        .key_binding(|press: text_editor::KeyPress| {
            if matches!(
                press.key,
                iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter)
            ) && press.modifiers.is_empty()
            {
                None
            } else {
                text_editor::Binding::from_key_press(press)
            }
        })
        .size(13)
        .padding(8)
        .height(Length::Fixed(120.0))
        .style(move |_: &Theme, _status| text_editor::Style {
            background: Background::Color(p.surface),
            border: Border {
                radius: 5.0.into(),
                color: p.border,
                width: 1.0,
            },
            placeholder: p.text_dim,
            value: p.text_pri,
            selection: p.accent,
        });
    let script_section = column![script_label, script_input].spacing(4);

    let can_save = app.snippets.form.save_enabled();
    let delete_msg =
        app.snippets.form.editing_id.map(|v0| {
            Message::Snippets(crate::snippets::message::Message::SnippetDeleteRequest(v0))
        });
    let actions = form_actions(
        if is_edit {
            t_static(t!("settings.save"))
        } else {
            t_static(t!("snippets.add_snippet"))
        },
        t_static(t!("settings.cancel")),
        can_save,
        Message::Snippets(crate::snippets::message::Message::SnippetFormSubmit),
        Message::Snippets(crate::snippets::message::Message::SnippetHideForm),
        delete_msg,
        p,
    );

    let form_content = column![header, package_section, desc_input, script_section, actions]
        .spacing(14)
        .padding(20);

    form_panel(form_content, 380.0, p)
}

fn divider(p: ThemePalette) -> Element<'static, Message> {
    container(column![])
        .width(Length::Fill)
        .height(1)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..container::Style::default()
        })
        .into()
}

pub fn view_snippet_delete_confirm(app: &App, p: theme::ThemePalette) -> Element<'static, Message> {
    let Some(id) = app.snippets.delete_pending else {
        return text("").into();
    };
    let name = app
        .snippets
        .list
        .iter()
        .find(|s| s.id == id)
        .map_or(t_static(t!("snippets.delete_snippet_fallback")), |s| {
            s.description.as_str()
        });
    let title = t!("snippets.delete_snippet_title");
    let body = t!("snippets.delete_snippet_confirm", name = name);
    delete_confirm_dialog(
        title,
        body,
        Message::Snippets(crate::snippets::message::Message::SnippetDeleteCancel),
        Message::Snippets(crate::snippets::message::Message::SnippetDelete(id)),
        &p,
        app.confirm_dialog_anim,
    )
}

pub fn view_snippet_package_delete_confirm(
    app: &App,
    p: theme::ThemePalette,
) -> Element<'static, Message> {
    let Some(id) = app.snippets.package_delete_pending else {
        return text("").into();
    };
    let name = app
        .snippets
        .packages
        .iter()
        .find(|p| p.id == id)
        .map_or(t_static(t!("snippets.delete_package_fallback")), |p| {
            p.name.as_str()
        });
    let title = t!("snippets.delete_package_title");
    let body = t!("snippets.delete_package_confirm", name = name);
    delete_confirm_dialog(
        title,
        body,
        Message::Snippets(crate::snippets::message::Message::SnippetPackageDeleteCancel),
        Message::Snippets(crate::snippets::message::Message::SnippetPackageDelete(id)),
        &p,
        app.confirm_dialog_anim,
    )
}

pub fn view_snippet_context_menu(
    menu: &crate::snippets::SnippetContextMenu,
    p: ThemePalette,
) -> Element<'_, Message> {
    const MENU_WIDTH: f32 = 140.0;
    use crate::core::context_menu::ContextMenuAction as Action;

    let actions = menu.actions();
    let focused = menu.cursor().selected_or_first(&actions).copied();
    let (id, position, is_snippet) = match menu {
        crate::snippets::SnippetContextMenu::Snippet { id, position, .. } => {
            (id.0, *position, true)
        }
        crate::snippets::SnippetContextMenu::Package { id, position, .. } => {
            (id.0, *position, false)
        }
    };

    crate::ui::overlay::context_menu_frame(
        position,
        move || {
            let edit_msg = if is_snippet {
                Message::Snippets(crate::snippets::message::Message::SnippetEdit(SnippetId(
                    id,
                )))
            } else {
                Message::Snippets(crate::snippets::message::Message::SnippetPackageEdit(
                    SnippetPackageId(id),
                ))
            };
            let delete_msg = if is_snippet {
                Message::Snippets(crate::snippets::message::Message::SnippetDeleteRequest(
                    SnippetId(id),
                ))
            } else {
                Message::Snippets(
                    crate::snippets::message::Message::SnippetPackageDeleteRequest(
                        SnippetPackageId(id),
                    ),
                )
            };

            let mut items: Vec<Element<'_, Message>> = vec![crate::core::widgets::menu_item(
                lucide_icons::Icon::Pencil,
                t_static(t!("context_menu.edit")),
                edit_msg,
                false,
                focused
                    == Some(if is_snippet {
                        Action::SnippetEdit
                    } else {
                        Action::SnippetPackageEdit
                    }),
                p,
            )];
            if is_snippet {
                items.push(crate::core::widgets::menu_item(
                    lucide_icons::Icon::Copy,
                    t_static(t!("context_menu.copy")),
                    Message::Snippets(crate::snippets::message::Message::SnippetCopyScript(
                        SnippetId(id),
                    )),
                    false,
                    focused == Some(Action::SnippetCopy),
                    p,
                ));
            }
            items.push(crate::core::widgets::menu_item(
                lucide_icons::Icon::Trash2,
                t_static(t!("dialog.delete")),
                delete_msg,
                true,
                focused
                    == Some(if is_snippet {
                        Action::SnippetDelete
                    } else {
                        Action::SnippetPackageDelete
                    }),
                p,
            ));
            items
        },
        MENU_WIDTH,
        Message::Snippets(crate::snippets::message::Message::SnippetContextMenuClose),
        p,
    )
}
