//! Application preference and theme updates.

use iced::Task;
use iced_term::TerminalView;

use super::{App, Message, system_theme_detect_task, update_active_sidebars};
use crate::core::data::LogCategory;
use crate::core::theme::ThemeMode;

pub(super) fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::SetTheme(mode) => {
            update_active_sidebars(app, |sidebar| sidebar.cursors.select_theme(mode));
            app.preferences.theme_mode = mode;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SettingsTheme,
            );
            let mode_str = mode.to_str().to_string();
            let save = app.storage_write_task(move |s| s.save_preference("theme_mode", &mode_str));
            // Auto re-reads the OS preference off the UI thread.
            let detect_task = if mode == ThemeMode::Auto && !app.preferences.system_theme_detecting
            {
                app.preferences.system_theme_detecting = true;
                system_theme_detect_task()
            } else {
                Task::none()
            };
            let term_palette = crate::core::theme::term_palette(app.effective_theme_mode());
            for session in &mut app.terminal.sessions {
                session
                    .terminal
                    .handle(iced_term::Command::ChangeTheme(Box::new(
                        term_palette.clone(),
                    )));
            }
            Task::batch([detect_task, save])
        }
        Message::ThemePreferenceSyncCheck => app.storage_task(
            |s| {
                s.load_preference("theme_mode")
                    .map(|value| value.and_then(|v| ThemeMode::from_str(&v)))
            },
            Message::ThemePreferenceSyncLoaded,
        ),
        Message::ThemePreferenceSyncLoaded(result) => {
            // Ignore transient read errors here. The next poll will retry, and
            // logging every interval would be noisy.
            let persisted = result.unwrap_or_default();

            if let Some(mode) = persisted
                && mode != app.preferences.theme_mode
            {
                app.preferences.theme_mode = mode;
                let term_palette = crate::core::theme::term_palette(app.effective_theme_mode());
                for session in &mut app.terminal.sessions {
                    session
                        .terminal
                        .handle(iced_term::Command::ChangeTheme(Box::new(
                            term_palette.clone(),
                        )));
                }
                if mode == ThemeMode::Auto && !app.preferences.system_theme_detecting {
                    app.preferences.system_theme_detecting = true;
                    return system_theme_detect_task();
                }
            }
            Task::none()
        }
        Message::SystemThemeCheck => {
            if app.preferences.system_theme_detecting {
                return Task::none();
            }
            app.preferences.system_theme_detecting = true;
            system_theme_detect_task()
        }
        Message::SystemThemeDetected(new_dark) => {
            app.preferences.system_theme_detecting = false;
            if new_dark != app.preferences.system_dark_mode {
                app.preferences.system_dark_mode = new_dark;
                if app.preferences.theme_mode == ThemeMode::Auto {
                    let term_palette = crate::core::theme::term_palette(app.effective_theme_mode());
                    for session in &mut app.terminal.sessions {
                        session
                            .terminal
                            .handle(iced_term::Command::ChangeTheme(Box::new(
                                term_palette.clone(),
                            )));
                    }
                }
            }
            Task::none()
        }
        Message::SetTerminalFontSize(size) => {
            let clamped = size.clamp(8.0, 32.0);
            app.preferences.terminal_font_size = clamped;
            let clamped_str = clamped.to_string();
            let save = app
                .storage_write_task(move |s| s.save_preference("terminal_font_size", &clamped_str));
            let font_settings = iced_term::settings::FontSettings {
                size: clamped,
                ..Default::default()
            };
            for session in &mut app.terminal.sessions {
                session
                    .terminal
                    .handle(iced_term::Command::ChangeFont(font_settings.clone()));
            }
            let focus = if let Some(session) = app.active_session() {
                TerminalView::focus(session.terminal.widget_id().clone())
            } else {
                Task::none()
            };
            Task::batch([save, focus])
        }
        Message::SetCommandHistoryLimit(limit) => {
            app.preferences.command_history_limit = limit;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SettingsHistoryLimit,
            );
            let stored = limit.to_string();
            app.storage_write_task(move |s| s.save_preference("command_history_limit", &stored))
        }
        Message::SettingsToggleCommandHistoryLimitPicker => crate::cursor::toggle_dropdown(
            app,
            crate::core::dropdown::DropdownTarget::SettingsHistoryLimit,
        ),
        Message::SettingsToggleThemePicker => crate::cursor::toggle_dropdown(
            app,
            crate::core::dropdown::DropdownTarget::SettingsTheme,
        ),
        Message::SetShellIntegration(enabled) => {
            app.preferences.shell_integration = enabled;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SettingsShellIntegration,
            );
            let save = app.storage_write_task(move |s| {
                s.save_preference("shell_integration", if enabled { "true" } else { "false" })
            });
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "shell integration (OSC 7/633) {} for new local/SSH terminals",
                    if enabled { "enabled" } else { "disabled" }
                ),
            );
            save
        }
        Message::SettingsToggleShellIntegrationPicker => crate::cursor::toggle_dropdown(
            app,
            crate::core::dropdown::DropdownTarget::SettingsShellIntegration,
        ),
        Message::SetLocalShell(shell) => {
            app.preferences.local_shell = shell;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SettingsLocalShell,
            );
            let stored = shell.to_str().to_string();
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "local shell set to {} for new Windows terminals",
                    shell.to_str()
                ),
            );
            app.storage_write_task(move |s| s.save_preference("local_shell", &stored))
        }
        Message::SettingsToggleLocalShellPicker => crate::cursor::toggle_dropdown(
            app,
            crate::core::dropdown::DropdownTarget::SettingsLocalShell,
        ),
        Message::SetDiagnosticsLevel(level) => {
            app.preferences.diagnostics_level = level;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SettingsDiagnostics,
            );
            crate::core::diagnostics::set_level(level);
            let level_str = level.as_str().to_string();
            app.storage_write_task(move |s| s.save_preference("diagnostics_level", &level_str))
        }
        Message::SettingsToggleDiagnosticsPicker => crate::cursor::toggle_dropdown(
            app,
            crate::core::dropdown::DropdownTarget::SettingsDiagnostics,
        ),
        Message::OpenBugReport => {
            let version = crate::core::version::display();
            let url = crate::core::open::bug_report_url(&version);
            crate::core::open::open_url(&url);
            Task::none()
        }
        Message::SetLocale(locale) => {
            app.preferences.locale = locale;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SettingsLocale,
            );
            rust_i18n::set_locale(locale.to_str());
            let locale_str = locale.to_str().to_string();
            app.storage_write_task(move |s| s.save_preference("locale", &locale_str))
        }
        Message::SettingsToggleLocalePicker => crate::cursor::toggle_dropdown(
            app,
            crate::core::dropdown::DropdownTarget::SettingsLocale,
        ),
        _ => unreachable!("settings handler received another message"),
    }
}
