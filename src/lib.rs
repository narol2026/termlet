//! Shared application code for the Termlet GUI and SSH bridge binaries.

mod app;
mod context_menu;
mod core;
mod cursor;
#[cfg(test)]
mod e2e;
mod focus;
mod hosts;
mod keychain;
mod known_hosts;
mod overlay;
mod port_forward;
mod sftp;
mod snippets;
mod ssh;
mod ssh_connect;
mod terminal;
mod ui;
mod update;
mod views;
mod workspace;

use std::process::ExitCode;

use rust_i18n::{i18n, t};

i18n!("locales", fallback = "en");

pub use core::message::Message;

/// Ensure the application's private data directory exists.
///
/// # Errors
///
/// Returns the underlying filesystem error when the directory cannot be
/// created or its permissions cannot be restricted.
pub fn ensure_data_dir() -> Result<(), String> {
    core::paths::ensure_termlet_dir_private()
}

/// Install the crash reporter shared by the GUI and bridge processes.
pub fn init_crash_reporter() {
    core::crash_reporter::init();
}

/// Run the in-process SSH bridge used by terminal tabs.
#[must_use]
pub fn run_ssh_bridge(host_id: &str) -> ExitCode {
    ssh::bridge::run(host_id)
}

/// Narrow public facade for the GUI entry point.
///
/// The application's internal state intentionally stays private to this crate;
/// the `termlet` binary only needs to construct the Elm-style application and
/// pass its update/view/subscription functions to iced.
pub mod gui {
    use iced::{Element, Subscription, Task, Theme};

    use super::{
        app,
        core::{data::HostId, i18n::t_static, theme::ThemeMode},
        t, update, views,
    };

    pub use super::core::fonts::{
        MY_ICONS_FONT_BYTES, SOURCE_CODE_PRO_BOLD, SOURCE_CODE_PRO_BOLD_ITALIC,
        SOURCE_CODE_PRO_ITALIC, SOURCE_CODE_PRO_REGULAR,
    };

    /// Initial action requested on the command line.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InitialAction {
        None,
        NewWindow,
        QuickSsh(u32),
        QuickLocal,
    }

    impl From<InitialAction> for app::InitialAction {
        fn from(action: InitialAction) -> Self {
            match action {
                InitialAction::None => Self::None,
                InitialAction::NewWindow => Self::NewWindow,
                InitialAction::QuickSsh(id) => Self::QuickSsh(HostId(id)),
                InitialAction::QuickLocal => Self::QuickLocal,
            }
        }
    }

    /// Opaque owner of the GUI application state.
    pub struct Gui(app::App);

    impl std::fmt::Debug for Gui {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Gui").finish_non_exhaustive()
        }
    }

    impl Gui {
        /// Construct the GUI and its startup task.
        pub fn new(action: InitialAction) -> (Self, Task<super::Message>) {
            let (app, task) = app::App::new(action.into());
            (Self(app), task)
        }

        /// Subscribe to application and window events.
        pub fn subscription(&self) -> Subscription<super::Message> {
            self.0.subscription()
        }
    }

    /// Apply one GUI message.
    pub fn update(gui: &mut Gui, message: super::Message) -> Task<super::Message> {
        update::update(&mut gui.0, message)
    }

    /// Render the current GUI state.
    pub fn view(gui: &Gui) -> Element<'_, super::Message> {
        views::view(&gui.0)
    }

    /// Resolve iced's base theme from the application's selected theme.
    pub fn theme(gui: &Gui) -> Theme {
        match gui.0.effective_theme_mode() {
            ThemeMode::Dark | ThemeMode::HighContrastDark | ThemeMode::Dracula => Theme::Dark,
            ThemeMode::Light | ThemeMode::HighContrastLight | ThemeMode::Novel => Theme::Light,
            ThemeMode::Auto => unreachable!("Auto resolved by effective_theme_mode"),
        }
    }

    /// Localized window title.
    #[must_use]
    pub fn title() -> &'static str {
        t_static(t!("app.title"))
    }
}
