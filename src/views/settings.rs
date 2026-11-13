// Rust guideline compliant 2026-10-09

use iced::widget::{Space, column, container, row, text};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

use crate::app::App;
use crate::app::keybindings::{Action, format_binding};
use crate::core::dropdown::DropdownTarget;
use crate::core::i18n::t_static;
use crate::core::locale::Locale;
use crate::core::message::Message;
use crate::core::theme::{self, ThemeMode};
use crate::core::widgets::{styled_btn, styled_scrollable};
use crate::views::workspaces::view_workspaces_card;
use rust_i18n::t;
use std::borrow::Cow;

pub const SETTINGS_SCROLL_ID: &str = "settings_scroll";
pub(crate) const SETTINGS_CONTENT_TOP_PADDING: f32 = 24.0;
pub(crate) const SETTINGS_SECTION_GAP: f32 = 20.0;
pub(crate) const SETTINGS_HEADER_HEIGHT: f32 = 26.0;

pub fn view_settings(app: &App) -> Element<'_, Message> {
    let p = theme::palette(app.effective_theme_mode());

    let header = container(
        text(t!("profiles.title").to_string())
            .size(20)
            .color(p.text_pri),
    )
    .height(Length::Fixed(SETTINGS_HEADER_HEIGHT));

    let section_title = |label: &str| text(label.to_string()).size(13).color(p.text_pri);

    let workspaces_card = view_workspaces_card(app, p);

    let theme_options = ThemeMode::all()
        .iter()
        .map(|&mode| crate::core::widgets::SelectOption {
            label: mode.label().to_string(),
            subtitle: None,
            active: app.preferences.theme_mode == mode,
            focused: app.preferences.theme_picker.active(ThemeMode::all()) == Some(&mode),
            on_select: Message::SetTheme(mode),
            leading: Some(crate::core::widgets::theme_preview_icon(mode)),
        })
        .collect();

    let theme_dropdown = crate::core::widgets::select_dropdown(
        app.preferences.theme_mode.label(),
        Some(crate::core::widgets::theme_preview_icon(
            app.preferences.theme_mode,
        )),
        theme_options,
        app.active_dropdown == Some(DropdownTarget::SettingsTheme),
        DropdownTarget::SettingsTheme,
        Message::SettingsToggleThemePicker,
        Length::Fill,
        None,
        None,
        p,
    );

    let theme_section = column![
        text(t!("settings.theme").to_string())
            .size(11)
            .color(p.text_dim),
        theme_dropdown,
    ]
    .spacing(6);

    let locale_options = Locale::ALL
        .iter()
        .map(|&loc| crate::core::widgets::SelectOption {
            label: loc.label().to_string(),
            subtitle: None,
            active: app.preferences.locale == loc,
            focused: app.preferences.locale_picker.active(Locale::ALL) == Some(&loc),
            on_select: Message::SetLocale(loc),
            leading: None,
        })
        .collect();

    let locale_dropdown = crate::core::widgets::select_dropdown(
        app.preferences.locale.label(),
        None,
        locale_options,
        app.active_dropdown == Some(DropdownTarget::SettingsLocale),
        DropdownTarget::SettingsLocale,
        Message::SettingsToggleLocalePicker,
        Length::Fill,
        None,
        None,
        p,
    );

    let locale_section = column![
        text(t!("settings.language").to_string())
            .size(11)
            .color(p.text_dim),
        locale_dropdown,
    ]
    .spacing(6);

    let appearance_card = container(
        column![
            section_title(t_static(t!("settings.appearance"))),
            text(t!("settings.appearance_hint").to_string())
                .size(11)
                .color(p.text_dim),
            theme_section,
            locale_section,
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: None,
        border: Border {
            radius: 10.0.into(),
            color: p.border,
            width: 1.0,
        },
        ..container::Style::default()
    });

    #[cfg(target_os = "windows")]
    let local_shell_options = crate::core::data::LocalShell::windows_options()
        .iter()
        .copied()
        .map(|shell| crate::core::widgets::SelectOption {
            label: shell.label().to_string(),
            subtitle: None,
            active: app.preferences.local_shell == shell,
            focused: app
                .preferences
                .local_shell_picker
                .active(crate::core::data::LocalShell::windows_options())
                == Some(&shell),
            on_select: Message::SetLocalShell(shell),
            leading: None,
        })
        .collect();

    #[cfg(target_os = "windows")]
    let local_shell_dropdown = crate::core::widgets::select_dropdown(
        app.preferences.local_shell.label(),
        None,
        local_shell_options,
        app.active_dropdown == Some(DropdownTarget::SettingsLocalShell),
        DropdownTarget::SettingsLocalShell,
        Message::SettingsToggleLocalShellPicker,
        Length::Fill,
        None,
        None,
        p,
    );

    #[cfg(target_os = "windows")]
    let local_shell_card = container(
        column![
            section_title(t_static(t!("settings.local_shell_title"))),
            text(t!("settings.local_shell_hint").to_string())
                .size(11)
                .color(p.text_dim)
                .wrapping(text::Wrapping::Word),
            local_shell_dropdown,
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: None,
        border: Border {
            radius: 10.0.into(),
            color: p.border,
            width: 1.0,
        },
        ..container::Style::default()
    });

    let shell_integration_options = vec![
        crate::core::widgets::SelectOption {
            label: t!("settings.shell_integration_on").into_owned(),
            subtitle: None,
            active: app.preferences.shell_integration,
            focused: app
                .preferences
                .shell_integration_picker
                .active(&[true, false])
                == Some(&true),
            on_select: Message::SetShellIntegration(true),
            leading: None,
        },
        crate::core::widgets::SelectOption {
            label: t!("settings.shell_integration_off").into_owned(),
            subtitle: None,
            active: !app.preferences.shell_integration,
            focused: app
                .preferences
                .shell_integration_picker
                .active(&[true, false])
                == Some(&false),
            on_select: Message::SetShellIntegration(false),
            leading: None,
        },
    ];
    let shell_integration_label = if app.preferences.shell_integration {
        t!("settings.shell_integration_on")
    } else {
        t!("settings.shell_integration_off")
    };
    let shell_integration_dropdown = crate::core::widgets::select_dropdown(
        shell_integration_label,
        None,
        shell_integration_options,
        app.active_dropdown == Some(DropdownTarget::SettingsShellIntegration),
        DropdownTarget::SettingsShellIntegration,
        Message::SettingsToggleShellIntegrationPicker,
        Length::Fill,
        None,
        None,
        p,
    );

    let privacy_card = container(
        column![
            section_title(t_static(t!("settings.privacy_title"))),
            text(t!("settings.shell_integration_title").to_string())
                .size(11)
                .color(p.text_sec),
            shell_integration_dropdown,
            text(t!("settings.shell_integration_hint").to_string())
                .size(11)
                .color(p.text_dim)
                .wrapping(text::Wrapping::Word),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: None,
        border: Border {
            radius: 10.0.into(),
            color: p.border,
            width: 1.0,
        },
        ..container::Style::default()
    });

    // The ring size is picked rather than typed: it bounds what the sidebar
    // keeps rendering, and a fixed list is what keeps the choice inside that.
    let history_limit_options = crate::core::data::COMMAND_HISTORY_LIMITS
        .iter()
        .map(|&limit| crate::core::widgets::SelectOption {
            label: limit.to_string(),
            subtitle: None,
            active: app.preferences.command_history_limit == limit,
            focused: app
                .preferences
                .command_history_limit_picker
                .active(&crate::core::data::COMMAND_HISTORY_LIMITS)
                == Some(&limit),
            on_select: Message::SetCommandHistoryLimit(limit),
            leading: None,
        })
        .collect();

    let history_limit_dropdown = crate::core::widgets::select_dropdown(
        app.preferences.command_history_limit.to_string(),
        None,
        history_limit_options,
        app.active_dropdown == Some(DropdownTarget::SettingsHistoryLimit),
        DropdownTarget::SettingsHistoryLimit,
        Message::SettingsToggleCommandHistoryLimitPicker,
        Length::Fill,
        None,
        None,
        p,
    );

    let history_card = container(
        column![
            section_title(t_static(t!("settings.history_title"))),
            text(t!("settings.history_limit").to_string())
                .size(11)
                .color(p.text_sec),
            history_limit_dropdown,
            text(t!("settings.history_hint").to_string())
                .size(11)
                .color(p.text_dim)
                .wrapping(text::Wrapping::Word),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: None,
        border: Border {
            radius: 10.0.into(),
            color: p.border,
            width: 1.0,
        },
        ..container::Style::default()
    });

    let diagnostics_levels = crate::cursor::settings_diagnostics_ids();

    let diagnostics_options = diagnostics_levels
        .iter()
        .map(|&level| crate::core::widgets::SelectOption {
            label: diagnostics_level_label(level).into_owned(),
            subtitle: None,
            active: app.preferences.diagnostics_level == level,
            focused: app
                .preferences
                .diagnostics_picker
                .active(diagnostics_levels)
                == Some(&level),
            on_select: Message::SetDiagnosticsLevel(level),
            leading: None,
        })
        .collect();

    let diagnostics_dropdown = crate::core::widgets::select_dropdown(
        diagnostics_level_label(app.preferences.diagnostics_level),
        None,
        diagnostics_options,
        app.active_dropdown == Some(DropdownTarget::SettingsDiagnostics),
        DropdownTarget::SettingsDiagnostics,
        Message::SettingsToggleDiagnosticsPicker,
        Length::Fill,
        None,
        None,
        p,
    );

    let diagnostics_card = container(
        column![
            section_title(t_static(t!("settings.diagnostics_title"))),
            text(t!("settings.diagnostics_hint").to_string())
                .size(11)
                .color(p.text_dim)
                .wrapping(text::Wrapping::Word),
            diagnostics_dropdown,
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: None,
        border: Border {
            radius: 10.0.into(),
            color: p.border,
            width: 1.0,
        },
        ..container::Style::default()
    });

    let shortcut_rows: Vec<Element<'_, Message>> = app
        .keybindings
        .bindings()
        .iter()
        .map(|(key, modifiers, action)| {
            let label = shortcut_action_label(action).to_string();
            let combo = format_binding(key, *modifiers);
            view_shortcut_row(label, combo, p)
        })
        .collect();

    let shortcuts_card = container(
        column![
            section_title(t_static(t!("settings.shortcuts"))),
            text(t!("settings.shortcuts_hint").to_string())
                .size(11)
                .color(p.text_dim),
            column(shortcut_rows).spacing(8),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: None,
        border: Border {
            radius: 10.0.into(),
            color: p.border,
            width: 1.0,
        },
        ..container::Style::default()
    });

    let feedback_btn = styled_btn(
        t_static(t!("settings.feedback_report_bug")),
        Some(Message::OpenBugReport),
        p.accent,
        Color::WHITE,
        p,
    );

    let about_card = container(
        column![
            section_title(t_static(t!("settings.about_title"))),
            text(t!("settings.about_hint").to_string())
                .size(11)
                .color(p.text_dim)
                .wrapping(text::Wrapping::Word),
            row![
                text(t!("settings.feedback_title").to_string())
                    .size(12)
                    .color(p.text_sec),
                Space::new().width(Length::Fill),
                feedback_btn,
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
            row![
                text(t!("settings.about_version").to_string())
                    .size(12)
                    .color(p.text_sec),
                Space::new().width(Length::Fill),
                text(crate::core::version::display())
                    .size(12)
                    .color(p.text_pri),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center),
        ]
        .spacing(12),
    )
    .padding(20)
    .width(Length::Fill)
    .style(move |_: &Theme| container::Style {
        background: None,
        border: Border {
            radius: 10.0.into(),
            color: p.border,
            width: 1.0,
        },
        ..container::Style::default()
    });

    let mut sections: Vec<Element<'_, Message>> = vec![
        header.into(),
        workspaces_card,
        appearance_card.into(),
        privacy_card.into(),
        history_card.into(),
    ];
    #[cfg(target_os = "windows")]
    sections.push(local_shell_card.into());
    sections.extend([
        diagnostics_card.into(),
        shortcuts_card.into(),
        about_card.into(),
    ]);
    let content = column(sections).spacing(SETTINGS_SECTION_GAP);

    let scroll_visible = app
        .scrollbar_states
        .get(SETTINGS_SCROLL_ID)
        .is_some_and(|s| s.visible);

    let scroll = styled_scrollable(
        container(content)
            .padding(Padding {
                top: SETTINGS_CONTENT_TOP_PADDING,
                right: 32.0,
                bottom: 24.0,
                left: 32.0,
            })
            .width(Length::Fill),
        p,
        scroll_visible,
    )
    .id(iced::widget::Id::from(SETTINGS_SCROLL_ID))
    .on_scroll(|v| Message::Scrolled(SETTINGS_SCROLL_ID, v))
    .height(Length::Fill)
    .width(Length::Fill);

    let scroll_container = container(scroll).width(Length::Fill).height(Length::Fill);

    scroll_container.into()
}

fn diagnostics_level_label(level: crate::core::diagnostics::Level) -> Cow<'static, str> {
    match level {
        crate::core::diagnostics::Level::Error => t!("settings.diagnostics_level_error"),
        crate::core::diagnostics::Level::Warn => t!("settings.diagnostics_level_warn"),
        crate::core::diagnostics::Level::Info => t!("settings.diagnostics_level_info"),
        crate::core::diagnostics::Level::Debug => t!("settings.diagnostics_level_debug"),
        crate::core::diagnostics::Level::Trace => t!("settings.diagnostics_level_trace"),
    }
}

fn shortcut_action_label(action: &Action) -> Cow<'static, str> {
    match action {
        Action::OpenLocalTerminal => t!("settings.shortcut.open_local_terminal"),
        Action::NewTerminalTab => t!("settings.shortcut.new_terminal_tab"),
        Action::ToggleSearch => t!("settings.shortcut.toggle_search"),
        Action::CloseCurrentTab => t!("settings.shortcut.close_current_tab"),
        Action::DuplicateSession => t!("settings.shortcut.duplicate_session"),
        Action::ToggleFullscreen => t!("settings.shortcut.toggle_fullscreen"),
        Action::IncreaseFontSize => t!("settings.shortcut.increase_font_size"),
        Action::DecreaseFontSize => t!("settings.shortcut.decrease_font_size"),
    }
}

fn view_shortcut_row<'a>(
    label: String,
    combo: String,
    p: crate::core::theme::ThemePalette,
) -> Element<'a, Message> {
    let keycap = container(text(combo).size(11).color(p.text_pri))
        .padding(Padding::from([3.0, 8.0]))
        .style(move |_: &Theme| container::Style {
            background: Some(Background::Color(p.surface)),
            border: Border {
                radius: 5.0.into(),
                color: p.border,
                width: 1.0,
            },
            ..container::Style::default()
        });

    row![
        text(label).size(12).color(p.text_sec),
        Space::new().width(Length::Fill),
        keycap,
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center)
    .into()
}
