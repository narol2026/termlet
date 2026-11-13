use iced::widget::{
    Space, button, column, container, mouse_area, row, text, text_editor, text_input,
};
use iced::{Background, Border, Color, Element, Font, Length, Padding, Theme, mouse};

use crate::app::App;
use crate::core::cache::CommandHistoryEntry;
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{RightSidebarTab, Snippet};
use crate::core::diagnostics::Level;
use crate::core::dropdown::DropdownTarget;
use crate::core::i18n::t_static;
use crate::core::message::Message;
use crate::core::theme::{self, ThemeMode};
use crate::core::widgets::{
    SelectOption, TooltipPosition, card_wrapper, control_padding, cursor_tooltip,
    cursor_tooltip_label_at, filter_input, input_style_with_palette, select_dropdown,
    styled_scrollable,
};
use crate::terminal::SidebarState;
use crate::ui::cursor_area::CursorArea;
use rust_i18n::t;

pub const RIGHT_SIDEBAR_SNIPPETS_SCROLL_ID: &str = "right_sidebar_snippets_scroll";
pub const RIGHT_SIDEBAR_COMMANDS_SCROLL_ID: &str = "right_sidebar_commands_scroll";
pub const RIGHT_SIDEBAR_THEME_SCROLL_ID: &str = "right_sidebar_theme_scroll";
pub const RIGHT_SIDEBAR_WIDTH: f32 = 281.0;

// Fixed row extents and the adjacent gaps let the update layer keep the
// keyboard selection visible without measuring the widget tree again. The
// snippet height fits two text lines, the action strip, and card padding; the
// command height fits its one-line card and inner padding.
pub(crate) const RIGHT_SIDEBAR_SNIPPET_ROW_HEIGHT: f32 = 92.0;
pub(crate) const RIGHT_SIDEBAR_SNIPPET_GAP: f32 = 6.0;
pub(crate) const RIGHT_SIDEBAR_COMMAND_ROW_HEIGHT: f32 = 38.0;
pub(crate) const RIGHT_SIDEBAR_COMMAND_GAP: f32 = 6.0;
pub(crate) const RIGHT_SIDEBAR_THEME_GAP: f32 = 4.0;

/// Width cap for the command-history tooltip: wide enough for a real command
/// line, narrow enough to stay beside the sidebar instead of spanning the
/// window.
const COMMAND_TOOLTIP_MAX_WIDTH: f32 = 360.0;

const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub fn view_right_sidebar<'a>(app: &'a App, sidebar: &'a SidebarState) -> Element<'a, Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "VIEW",
        "view_right_sidebar",
        std::time::Duration::from_millis(16),
    );
    let p = theme::palette(app.effective_theme_mode());

    let tabs = RightSidebarTab::all().iter().map(|&tab| {
        let active = sidebar.active_tab == tab;
        let tab_btn = button(
            text(char::from(tab.icon()))
                .font(LUCIDE_FONT)
                .size(15)
                .color(if active { p.text_pri } else { p.text_sec }),
        )
        .on_press(Message::SwitchRightTab(tab))
        .padding(Padding::from([6.0, 10.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            let pressed = matches!(status, button::Status::Pressed);
            let bg = if active || pressed {
                p.nav_active
            } else if hovered {
                p.item_hover
            } else {
                return button::Style::default();
            };
            button::Style {
                background: Some(Background::Color(bg)),
                text_color: if active || hovered {
                    p.text_pri
                } else {
                    p.text_sec
                },
                border: Border {
                    radius: 6.0.into(),
                    width: if p.show_border { 1.0 } else { 0.0 },
                    color: p.border,
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

        let tab_element: Element<'_, Message> =
            cursor_tooltip_label_at(tab_btn, tab.label(), p, TooltipPosition::Bottom, None).into();

        container(tab_element).into()
    });

    let tab_bar = row(tabs).spacing(8).width(Length::Fill);

    let tab_content: Element<'_, Message> = match sidebar.active_tab {
        RightSidebarTab::Snippets => view_snippets_panel(app, sidebar),
        RightSidebarTab::Commands => view_commands_panel(app, sidebar),
        RightSidebarTab::Theme => view_theme_panel(app, sidebar),
    };

    let edge_divider =
        container(column![])
            .width(1)
            .height(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..container::Style::default()
            });

    let tab_divider =
        container(column![])
            .width(Length::Fill)
            .height(1)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..container::Style::default()
            });

    container(row![
        edge_divider,
        column![tab_bar, tab_divider, tab_content]
            .spacing(12)
            .padding(16)
    ])
    .width(RIGHT_SIDEBAR_WIDTH)
    .height(Length::Fill)
    .style(move |_theme: &Theme| container::Style {
        background: Some(Background::Color(p.sidebar_bg)),
        ..container::Style::default()
    })
    .into()
}

fn sidebar_cursor_area<'a>(
    tab: RightSidebarTab,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    CursorArea::for_target(content, CursorTarget::for_right_sidebar(tab)).into()
}

fn view_theme_panel<'a>(app: &'a App, sidebar: &'a SidebarState) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let font_size_row = {
        let label = text(t_static(t!("right_sidebar.font_size")))
            .size(13)
            .color(p.text_sec);

        let dec_btn = button(
            text(char::from(lucide_icons::Icon::Minus))
                .font(LUCIDE_FONT)
                .size(13)
                .color(p.text_dim),
        )
        .on_press(Message::SetTerminalFontSize(
            app.preferences.terminal_font_size - 1.0,
        ))
        .padding(Padding::from([4.0, 8.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: None,
                text_color: if hovered { p.text_pri } else { p.text_dim },
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

        let size_display = text(format!("{:.0}", app.preferences.terminal_font_size))
            .size(13)
            .color(p.text_pri);

        let inc_btn = button(
            text(char::from(lucide_icons::Icon::Plus))
                .font(LUCIDE_FONT)
                .size(13)
                .color(p.text_dim),
        )
        .on_press(Message::SetTerminalFontSize(
            app.preferences.terminal_font_size + 1.0,
        ))
        .padding(Padding::from([4.0, 8.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: None,
                text_color: if hovered { p.text_pri } else { p.text_dim },
                border: Border {
                    radius: 4.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

        row![label, dec_btn, size_display, inc_btn]
            .spacing(4)
            .align_y(iced::alignment::Vertical::Center)
    };

    let divider =
        container(column![])
            .width(Length::Fill)
            .height(1)
            .style(move |_theme: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..container::Style::default()
            });

    let list = column(ThemeMode::all().iter().map(|&mode| {
        let selected = sidebar.cursors.selected_theme() == Some(&mode);
        view_theme_row(app.preferences.theme_mode, mode, selected)
    }))
    .spacing(RIGHT_SIDEBAR_THEME_GAP);

    let list = styled_scrollable(
        sidebar_cursor_area(RightSidebarTab::Theme, list),
        p,
        app.is_scrollbar_visible(RIGHT_SIDEBAR_THEME_SCROLL_ID),
    )
    .id(iced::widget::Id::from(RIGHT_SIDEBAR_THEME_SCROLL_ID))
    .on_scroll(move |viewport| Message::Scrolled(RIGHT_SIDEBAR_THEME_SCROLL_ID, viewport))
    .height(Length::Fill);

    column![font_size_row, divider, list]
        .spacing(12)
        .height(Length::Fill)
        .into()
}

fn view_theme_row<'a>(current: ThemeMode, mode: ThemeMode, selected: bool) -> Element<'a, Message> {
    let p = theme::palette(current);
    let active = current == mode;

    let icon = crate::core::widgets::theme_preview_icon(mode);

    let label = text(mode.label())
        .size(13)
        .color(if active { p.text_pri } else { p.text_sec });

    let check = if active {
        text(char::from(lucide_icons::Icon::Check))
            .font(LUCIDE_FONT)
            .size(14)
            .color(p.accent)
    } else {
        text("").size(14)
    };

    let row_content = row![icon, label]
        .spacing(6)
        .align_y(iced::alignment::Vertical::Center)
        .width(Length::Fill);

    let padded = row![row_content, check]
        .align_y(iced::alignment::Vertical::Center)
        .spacing(8)
        .height(Length::Fill)
        .padding(Padding::from([0.0, 12.0]));

    let btn = button(padded)
        .on_press(Message::SetTheme(mode))
        .width(Length::Fill)
        .height(Length::Fixed(crate::views::ITEM_HEIGHT))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            let bg = if selected {
                p.item_hover
            } else if active {
                p.item_active
            } else if hovered {
                p.item_hover
            } else {
                p.surface
            };
            button::Style {
                background: Some(Background::Color(bg)),
                border: if selected || active {
                    Border {
                        color: p.accent,
                        width: 1.0,
                        radius: 8.0.into(),
                    }
                } else if p.show_border {
                    Border {
                        color: p.border,
                        width: 1.0,
                        radius: 8.0.into(),
                    }
                } else {
                    Border {
                        radius: 8.0.into(),
                        ..Border::default()
                    }
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default());

    container(btn).into()
}

fn view_snippets_panel<'a>(app: &'a App, sidebar: &'a SidebarState) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());
    let has_active_terminal = app.active_terminal_session_id().is_some();

    let add_section: Element<'_, Message> = if app.snippets.right_show_form {
        let desc_input = text_input(
            t_static(t!("sidebar.description")),
            &app.snippets.right_description,
        )
        .on_input(|v0| {
            Message::Snippets(
                crate::snippets::message::Message::SnippetRightSidebarFormDescription(v0),
            )
        })
        .padding(control_padding(13.0, 8.0))
        .size(13)
        .style(move |_, s| input_style_with_palette(&p, s));

        let script_input = text_editor(&app.snippets.right_script)
            .placeholder(t_static(t!("sidebar.script")))
            .on_action(|v0| {
                Message::Snippets(
                    crate::snippets::message::Message::SnippetRightSidebarFormScript(v0),
                )
            })
            .size(13)
            .padding(8)
            .height(Length::Fixed(100.0))
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

        // Package picker for the sidebar add-form.
        let current_label = app
            .snippets
            .right_package_id
            .and_then(|id| app.snippets.packages.iter().find(|pk| pk.id == id))
            .map_or(t_static(t!("sidebar.package_none")), |pk| pk.name.as_str());

        let items = crate::cursor::snippet_package_ids(app);
        let focused = app.snippets.right_package_picker.active(&items).copied();
        let options: Vec<SelectOption> =
            if app.active_dropdown == Some(DropdownTarget::SnippetSidebarPackage) {
                items
                    .into_iter()
                    .filter_map(|id| {
                        let package = app.snippets.packages.iter().find(|pkg| pkg.id == id)?;
                        Some(SelectOption {
                            label: package.name.clone(),
                            subtitle: None,
                            active: app.snippets.right_package_id == Some(id),
                            focused: focused == Some(id),
                            on_select: Message::Snippets(
                                crate::snippets::message::Message::SnippetRightSidebarFormPackage(
                                    Some(id),
                                ),
                            ),
                            leading: None,
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };

        let pkg_picker = select_dropdown(
            current_label,
            None,
            options,
            app.active_dropdown == Some(DropdownTarget::SnippetSidebarPackage),
            DropdownTarget::SnippetSidebarPackage,
            Message::Snippets(
                crate::snippets::message::Message::SnippetRightSidebarTogglePackagePicker,
            ),
            Length::Fill,
            None,
            Some(t_static(t!("snippets.no_packages_yet"))),
            p,
        );

        let can_add = !app.snippets.right_description.trim().is_empty()
            && !app.snippets.right_script.text().trim().is_empty();

        let is_editing = app.snippets.right_editing_id.is_some();
        let submit_btn = {
            let button_bg = p.button_bg;
            let button_hover = p.button_hover;
            let button_border = p.button_border;
            let button_border_hover = p.button_border_hover;
            let text_dim = p.text_dim;
            let btn_text = if is_editing {
                t_static(t!("settings.save"))
            } else {
                t_static(t!("sidebar.add"))
            };
            let label = text(btn_text)
                .size(12)
                .color(if can_add { p.text_sec } else { text_dim });
            let mut btn = button(label)
                .padding(Padding::from([7.0, 14.0]))
                .style(move |_: &Theme, status| {
                    if !can_add {
                        return button::Style {
                            background: Some(Background::Color(Color {
                                r: button_bg.r,
                                g: button_bg.g,
                                b: button_bg.b,
                                a: button_bg.a * 0.35,
                            })),
                            text_color: text_dim,
                            border: Border {
                                radius: 5.0.into(),
                                width: 1.0,
                                color: button_border,
                            },
                            ..button::Style::default()
                        };
                    }
                    let hovered = matches!(status, button::Status::Hovered);
                    button::Style {
                        background: Some(Background::Color(if hovered {
                            button_hover
                        } else {
                            button_bg
                        })),
                        text_color: if hovered { p.text_pri } else { p.text_sec },
                        border: Border {
                            radius: 5.0.into(),
                            width: 1.0,
                            color: if hovered {
                                button_border_hover
                            } else {
                                button_border
                            },
                        },
                        ..button::Style::default()
                    }
                })
                .cursor_interaction(mouse::Interaction::default());
            if can_add {
                btn = btn.on_press(Message::Snippets(
                    crate::snippets::message::Message::SnippetRightSidebarFormSubmit,
                ));
            }
            btn
        };

        let cancel_btn = {
            let text_sec = p.text_sec;
            let button_bg = p.button_bg;
            let button_hover = p.button_hover;
            button(
                text(t_static(t!("settings.cancel")))
                    .size(12)
                    .color(text_sec),
            )
            .on_press(Message::Snippets(
                crate::snippets::message::Message::SnippetRightSidebarCancelAdd,
            ))
            .padding(Padding::from([7.0, 14.0]))
            .style(move |_: &Theme, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: Some(Background::Color(if hovered {
                        button_hover
                    } else {
                        button_bg
                    })),
                    text_color: if hovered { p.text_pri } else { text_sec },
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
            .cursor_interaction(mouse::Interaction::default())
        };

        column![
            desc_input,
            script_input,
            pkg_picker,
            row![Space::new().width(Length::Fill), cancel_btn, submit_btn].spacing(6),
        ]
        .spacing(6)
        .into()
    } else {
        let accent = p.accent;
        let item_hover = p.item_hover;
        button(
            row![
                text(char::from(lucide_icons::Icon::Plus))
                    .font(LUCIDE_FONT)
                    .size(12)
                    .color(accent),
                text(t_static(t!("right_sidebar.add_snippet")))
                    .size(12)
                    .color(accent),
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Snippets(
            crate::snippets::message::Message::SnippetRightSidebarShowAdd,
        ))
        .padding(Padding::from([6.0, 12.0]))
        .style(move |_: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if hovered {
                    item_hover
                } else {
                    Color::TRANSPARENT
                })),
                text_color: accent,
                border: Border {
                    radius: 5.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        })
        .cursor_interaction(mouse::Interaction::default())
        .into()
    };

    let divider = container(column![])
        .width(Length::Fill)
        .height(1)
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.border)),
            ..container::Style::default()
        });

    let list: Element<'_, Message> = if app.snippets.list.is_empty() {
        container(
            text(t_static(t!("right_sidebar.no_snippets")))
                .size(12)
                .color(p.text_dim),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(12)
        .into()
    } else {
        let rows: Vec<Element<'_, Message>> = app
            .snippets
            .list
            .iter()
            .map(|snippet| {
                let selected = sidebar.cursors.selected_snippet() == Some(&snippet.id);
                view_snippet_row(app, snippet, selected, has_active_terminal, p)
            })
            .collect();

        styled_scrollable(
            sidebar_cursor_area(
                RightSidebarTab::Snippets,
                container(
                    column(rows)
                        .spacing(RIGHT_SIDEBAR_SNIPPET_GAP)
                        .width(Length::Fill),
                ),
            ),
            p,
            app.is_scrollbar_visible(RIGHT_SIDEBAR_SNIPPETS_SCROLL_ID),
        )
        .id(iced::widget::Id::from(RIGHT_SIDEBAR_SNIPPETS_SCROLL_ID))
        .on_scroll(move |v| Message::Scrolled(RIGHT_SIDEBAR_SNIPPETS_SCROLL_ID, v))
        .height(Length::Fill)
        .into()
    };

    column![add_section, divider, list]
        .spacing(8)
        .height(Length::Fill)
        .into()
}

fn view_snippet_row<'a>(
    app: &'a App,
    snippet: &'a Snippet,
    selected: bool,
    has_terminal: bool,
    p: theme::ThemePalette,
) -> Element<'a, Message> {
    let id = snippet.id;

    let pkg_badge: Element<'_, Message> = if let Some(pid) = snippet.package_id {
        if let Some(pkg) = app.snippets.packages.iter().find(|pk| pk.id == pid) {
            container(
                text(pkg.name.clone())
                    .size(9)
                    .color(p.accent)
                    .ellipsis(text::Ellipsis::End)
                    .wrapping(text::Wrapping::None),
            )
            .padding(Padding::from([1.0, 5.0]))
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(Color {
                    r: p.accent.r,
                    g: p.accent.g,
                    b: p.accent.b,
                    a: 0.12,
                })),
                border: Border {
                    radius: 3.0.into(),
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

    let desc_row = row![
        text(snippet.description.clone())
            .size(12)
            .color(p.text_pri)
            .width(Length::Fill)
            .ellipsis(text::Ellipsis::End)
            .wrapping(text::Wrapping::None),
        pkg_badge,
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    let script_text = text(snippet.script.clone())
        .size(12)
        .color(p.text_sec)
        .width(Length::Fill)
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None);

    let mut action_row = row![].spacing(4).align_y(iced::Alignment::Center);

    if has_terminal {
        let run_btn = compact_icon_btn(
            lucide_icons::Icon::Play,
            p.success,
            false,
            Message::Snippets(crate::snippets::message::Message::SnippetRunScript(id)),
            &p,
        );
        let paste_btn = compact_icon_btn(
            lucide_icons::Icon::ClipboardPaste,
            p.text_sec,
            false,
            Message::Snippets(crate::snippets::message::Message::SnippetPasteScript(id)),
            &p,
        );
        action_row = action_row.push(run_btn).push(paste_btn);
    }

    let edit_btn = compact_icon_btn(
        lucide_icons::Icon::Pencil,
        p.text_sec,
        false,
        Message::Snippets(crate::snippets::message::Message::SnippetRightSidebarEdit(
            id,
        )),
        &p,
    );

    let delete_btn = compact_icon_btn(
        lucide_icons::Icon::Trash2,
        p.danger,
        false,
        Message::Snippets(crate::snippets::message::Message::SnippetDeleteRequest(id)),
        &p,
    );
    action_row = action_row
        .push(Space::new().width(Length::Fill))
        .push(edit_btn)
        .push(delete_btn);

    let action_divider =
        container(column![])
            .width(Length::Fill)
            .height(1)
            .style(move |_: &Theme| container::Style {
                background: Some(Background::Color(p.border)),
                ..container::Style::default()
            });

    let action_row = container(action_row).padding(Padding {
        top: 4.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    });

    let content = column![desc_row, script_text, action_divider, action_row].spacing(4);

    let card = container(content)
        .padding(10)
        .width(Length::Fill)
        .height(Length::Fixed(RIGHT_SIDEBAR_SNIPPET_ROW_HEIGHT))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(if selected {
                p.card_hover
            } else {
                p.card_bg
            })),
            border: Border {
                radius: 6.0.into(),
                width: if selected || p.show_border { 1.0 } else { 0.0 },
                color: if selected { p.accent } else { p.border },
            },
            ..Default::default()
        });

    mouse_area(card)
        .on_press(Message::RightSidebarSelectSnippet(id))
        .into()
}

/// Returns visible command indices in reverse-chronological display order.
pub(crate) fn visible_command_indices(entries: &[CommandHistoryEntry], filter: &str) -> Vec<usize> {
    let query = filter.trim().to_lowercase();
    entries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, entry)| query.is_empty() || entry.command.to_lowercase().contains(&query))
        .map(|(index, _)| index)
        .collect()
}

fn view_commands_panel<'a>(app: &'a App, sidebar: &'a SidebarState) -> Element<'a, Message> {
    let p = theme::palette(app.effective_theme_mode());

    // The sidebar is per-terminal; read the command history of the active session.
    let recent_commands: &[CommandHistoryEntry] = app
        .active_terminal_session_id()
        .and_then(|session_id| {
            app.terminal
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .map(|session| session.recent_commands.as_slice())
        })
        .unwrap_or(&[]);

    let search_input = filter_input(
        t_static(t!("right_sidebar.search_commands")),
        &sidebar.commands_filter,
        &p,
        Message::CommandHistoryFilter,
    );

    let visible = visible_command_indices(recent_commands, &sidebar.commands_filter);

    let list: Element<'_, Message> = if recent_commands.is_empty() {
        container(
            text(t_static(t!("right_sidebar.no_commands")))
                .size(12)
                .color(p.text_dim),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(12)
        .into()
    } else if visible.is_empty() {
        container(
            text(t_static(t!("right_sidebar.no_matching_commands")))
                .size(12)
                .color(p.text_dim),
        )
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(12)
        .into()
    } else {
        // Newest command first; keep the original index for row actions.
        let rows = visible
            .iter()
            .map(|&index| {
                let selected =
                    sidebar.cursors.selected_command() == Some(&recent_commands[index].command);
                view_command_row(
                    &recent_commands[index],
                    index,
                    selected,
                    sidebar.commands_hovered,
                    p,
                )
            })
            .collect::<Vec<_>>();

        styled_scrollable(
            sidebar_cursor_area(
                RightSidebarTab::Commands,
                container(
                    column(rows)
                        .spacing(RIGHT_SIDEBAR_COMMAND_GAP)
                        .width(Length::Fill),
                ),
            ),
            p,
            app.is_scrollbar_visible(RIGHT_SIDEBAR_COMMANDS_SCROLL_ID),
        )
        .id(iced::widget::Id::from(RIGHT_SIDEBAR_COMMANDS_SCROLL_ID))
        .on_scroll(move |v| Message::Scrolled(RIGHT_SIDEBAR_COMMANDS_SCROLL_ID, v))
        .height(Length::Fill)
        .into()
    };

    column![search_input, list]
        .spacing(10)
        .height(Length::Fill)
        .into()
}

fn view_command_row(
    entry: &CommandHistoryEntry,
    idx: usize,
    selected: bool,
    commands_hovered: Option<usize>,
    p: theme::ThemePalette,
) -> Element<'static, Message> {
    let hovered = commands_hovered == Some(idx);
    let cmd = &entry.command;

    let run_btn = compact_icon_btn(
        lucide_icons::Icon::Play,
        p.success,
        false,
        Message::CommandHistoryRun(idx),
        &p,
    );

    let copy_btn = compact_icon_btn(
        lucide_icons::Icon::Copy,
        p.text_sec,
        false,
        Message::CommandHistoryCopy(idx),
        &p,
    );

    let info_btn = compact_icon_btn(
        lucide_icons::Icon::Info,
        p.text_sec,
        false,
        Message::CommandHistoryInfo(idx),
        &p,
    );

    // Run, copy, and info are grouped at a tighter 2px gap; the command text stays
    // 6px away from the group.
    let actions = row![run_btn, copy_btn, info_btn]
        .spacing(2)
        .align_y(iced::Alignment::Center);

    // A pasted script stores as one multi-line string; a Text widget breaks
    // lines on every `\n` no matter the wrapping mode, so collapse to the first
    // line and mark the rest with an ellipsis.
    let display = if cmd.contains('\n') {
        format!("{}…", cmd.lines().next().unwrap_or_default())
    } else {
        cmd.clone()
    };

    // The row is a clipped single line, so hovering the text is how the whole
    // command is read: the tooltip wraps instead of truncating and keeps the
    // newlines of a pasted script.
    let display_text: Element<'static, Message> = text(display)
        .size(11)
        .color(p.text_pri)
        .width(Length::Fill)
        .ellipsis(text::Ellipsis::End)
        .wrapping(text::Wrapping::None)
        .into();

    let content = row![
        cursor_tooltip(display_text, command_tooltip(cmd, p)),
        actions
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .padding(Padding::from([2.0, 6.0]));

    let card = card_wrapper(
        content.into(),
        hovered,
        selected,
        None,
        None,
        6.0,
        true,
        Message::CommandHistoryHoverEnter(idx),
        Message::CommandHistoryHoverLeave(idx),
        Some(Message::RightSidebarSelectCommand(cmd.clone())),
        None,
        None,
        p,
    );

    container(card)
        .width(Length::Fill)
        .height(Length::Fixed(RIGHT_SIDEBAR_COMMAND_ROW_HEIGHT))
        .into()
}

/// The hover popup for a history row: the command in full, monospaced, wrapped
/// at [`COMMAND_TOOLTIP_MAX_WIDTH`] so a long line or a pasted script stays
/// readable.
fn command_tooltip(cmd: &str, p: theme::ThemePalette) -> Element<'static, Message> {
    container(
        text(cmd.to_string())
            .size(12)
            .font(Font::MONOSPACE)
            .color(p.text_pri)
            .wrapping(text::Wrapping::WordOrGlyph),
    )
    .padding(Padding::from([6.0, 10.0]))
    .max_width(COMMAND_TOOLTIP_MAX_WIDTH)
    .style(move |_: &Theme| container::Style {
        background: Some(Background::Color(p.tooltip_bg)),
        text_color: Some(p.text_pri),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn view_command_info_modal(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let Some(entry) = app.active_command_info() else {
        return mouse_area(Space::new()).into();
    };

    let command_label = text(t_static(t!("right_sidebar.command_info_command")))
        .size(11)
        .color(p.text_sec);
    let command_value = text(&entry.command)
        .size(12)
        .color(p.text_pri)
        .width(Length::Fill)
        .wrapping(text::Wrapping::WordOrGlyph);

    let executed_at_label = text(t_static(t!("right_sidebar.command_info_executed_at")))
        .size(11)
        .color(p.text_sec);
    let executed_at_value = text(crate::core::time::format_timestamp(entry.executed_at))
        .size(12)
        .color(p.text_pri)
        .width(Length::Fill);

    let content = column![
        command_label,
        command_value,
        Space::new().height(Length::Fixed(10.0)),
        executed_at_label,
        executed_at_value,
    ]
    .spacing(4)
    .width(Length::Fill);

    let actions: Vec<Element<'static, Message>> =
        vec![crate::ui::confirm_dialog::cancel_button_with_label(
            t_static(t!("app.dismiss")),
            Message::CommandHistoryInfoClose,
            &p,
        )];

    crate::ui::message_dialog::message_dialog(
        t_static(t!("right_sidebar.command_info_title")),
        None::<String>,
        content.into(),
        actions,
        Message::CommandHistoryInfoClose,
        360.0,
        &p,
        app.confirm_dialog_anim,
    )
}

fn compact_icon_btn<'a>(
    icon: lucide_icons::Icon,
    color: Color,
    _disabled: bool,
    on_press: Message,
    p: &theme::ThemePalette,
) -> Element<'a, Message> {
    let palette = *p;
    button(
        text(char::from(icon))
            .font(LUCIDE_FONT)
            .size(12)
            .color(color),
    )
    .on_press(on_press)
    .padding(Padding::from([4.0, 6.0]))
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered);
        // One notch lighter than `card_hover` so the icon still stands out on
        // cards that themselves turn `card_hover` when the mouse is over them.
        let hover_bg = Color {
            r: (palette.card_hover.r + 0.06).min(1.0),
            g: (palette.card_hover.g + 0.06).min(1.0),
            b: (palette.card_hover.b + 0.06).min(1.0),
            a: 1.0,
        };
        button::Style {
            background: Some(Background::Color(if hovered {
                hover_bg
            } else {
                palette.surface
            })),
            text_color: color,
            border: Border {
                radius: 4.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .cursor_interaction(mouse::Interaction::default())
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ids::SnippetId;
    use std::time::Duration;

    /// A row clips the command to its first line, so the tooltip is where the
    /// rest of it is read: hovering the text has to show the whole command,
    /// newlines and all, without the popup running off the window beside the
    /// right-edge sidebar.
    #[test]
    fn hovering_the_command_text_opens_a_tooltip_with_the_whole_command() {
        let p = theme::palette(ThemeMode::Dark);
        let command = "cargo test --all-features\ncargo clippy --all-targets";
        let entry = CommandHistoryEntry {
            command: command.to_string(),
            executed_at: 0,
        };

        // The sidebar sits against the right edge of the window, and the window
        // is the simulator's default 1024×768.
        let mut ui = iced_test::simulator(row![
            Space::new().width(Length::Fill),
            container(view_command_row(&entry, 0, false, None, p))
                .width(RIGHT_SIDEBAR_WIDTH)
                .padding(10),
        ]);

        // The clipped one-line row is all there is before the pointer arrives.
        let clipped = format!("{}…", command.lines().next().unwrap_or_default());
        assert!(
            ui.find(clipped.as_str()).is_ok(),
            "the row shows the first line of the command"
        );
        assert!(
            ui.find(command).is_err(),
            "there is no tooltip until the text is hovered"
        );

        let text = ui
            .find(clipped.as_str())
            .expect("the command text is laid out");
        let position = text
            .visible_bounds()
            .expect("the command text has bounds")
            .center();

        ui.point_at(position);
        ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved { position })]);

        // The tooltip opens on the redraw its hover delay asks for, which a
        // real run reaches through the runtime's event loop.
        std::thread::sleep(Duration::from_millis(700));
        let _ = ui.snapshot(&Theme::Dark).expect("the frame renders");

        let tooltip = ui
            .find(command)
            .expect("hovering the command text shows the whole command");
        // Compared with a tolerance: the two rectangles are computed through
        // different layout passes, so the last bit of the float can differ.
        let bounds = tooltip.bounds();
        let visible = tooltip.visible_bounds().expect("the popup is on screen");
        assert!(
            (visible.x - bounds.x).abs() < 0.5
                && (visible.y - bounds.y).abs() < 0.5
                && (visible.width - bounds.width).abs() < 0.5,
            "the popup stays inside the window next to the right-edge sidebar: \
             {visible:?} is not {bounds:?}"
        );
        assert!(
            tooltip.bounds().width <= COMMAND_TOOLTIP_MAX_WIDTH,
            "the popup is capped at a readable width instead of stretching"
        );
        // Two lines of the tooltip's font against the row's single clipped
        // line: the newline is rendered, not swallowed.
        assert!(
            tooltip.bounds().height > text.bounds().height,
            "the command's second line is rendered as a line of its own"
        );
    }

    #[test]
    fn visible_commands_are_newest_first_and_match_ignoring_case() {
        let entries = [
            CommandHistoryEntry {
                command: "git status".into(),
                executed_at: 1,
            },
            CommandHistoryEntry {
                command: "cargo TEST".into(),
                executed_at: 2,
            },
            CommandHistoryEntry {
                command: "cargo clippy".into(),
                executed_at: 3,
            },
        ];

        assert_eq!(visible_command_indices(&entries, ""), vec![2, 1, 0]);
        assert_eq!(visible_command_indices(&entries, " test "), vec![1]);
    }

    #[test]
    fn clicking_a_snippet_requests_sidebar_focus_and_selection() {
        let mut app = crate::core::test_support::app();
        app.snippets.list.push(Snippet {
            id: SnippetId(1),
            description: "deploy".into(),
            script: "git push".into(),
            package_id: None,
            last_used_at: "1700000000000".into(),
        });
        let sidebar = SidebarState {
            show: true,
            anim: 1.0,
            active_tab: RightSidebarTab::Snippets,
            ..Default::default()
        };

        let messages = {
            let mut ui = iced_test::simulator(view_right_sidebar(&app, &sidebar));
            ui.click("deploy").expect("the snippet row is clickable");
            ui.into_messages().collect::<Vec<_>>()
        };

        assert!(messages.iter().any(|message| matches!(
            message,
            Message::CursorFocus(CursorTarget::RightSidebarSnippets)
        )));
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::RightSidebarSelectSnippet(SnippetId(1))))
        );
    }

    #[test]
    fn right_sidebar_tabs_have_distinct_focus_targets() {
        let targets = [
            CursorTarget::for_right_sidebar(RightSidebarTab::Snippets),
            CursorTarget::for_right_sidebar(RightSidebarTab::Commands),
            CursorTarget::for_right_sidebar(RightSidebarTab::Theme),
        ];

        assert_eq!(targets[0].id(), "cursor_right_sidebar_snippets");
        assert_eq!(targets[1].id(), "cursor_right_sidebar_commands");
        assert_eq!(targets[2].id(), "cursor_right_sidebar_theme");
    }

    #[test]
    fn command_history_panel_omits_the_title_and_limit_summary() {
        let app = crate::core::test_support::app();
        let sidebar = SidebarState {
            show: true,
            anim: 1.0,
            active_tab: RightSidebarTab::Commands,
            ..Default::default()
        };
        let title = t!("right_sidebar.commands").to_string();
        let limit = format!("0/{}", app.preferences.command_history_limit);
        let mut ui = iced_test::simulator(view_right_sidebar(&app, &sidebar));

        ui.find(title.as_str())
            .expect_err("the panel title is removed");
        ui.find(limit.as_str())
            .expect_err("the history limit summary is removed");
    }
}
