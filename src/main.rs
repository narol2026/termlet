#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

// Rust guideline compliant 2026-10-09

use std::process::ExitCode;

use iced::Size;
use iced::window;
use termlet::gui::{self, Gui, InitialAction};

const ICON_BYTES: &[u8] = include_bytes!("../assets/app-icons/generic/termlet.png");

/// Window size the app opens at, and the smallest size it may be dragged down
/// to. The layout is designed for this size, so this is also the floor: any
/// smaller and the vault sidebar, a terminal pane and the right sidebar get
/// squeezed into unreadable slivers.
const WINDOW_SIZE: Size = Size::new(1280.0, 720.0);

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let first_arg = args.next();

    if let Err(error) = termlet::ensure_data_dir() {
        eprintln!("termlet: failed to secure ~/.termlet: {error}");
        return ExitCode::FAILURE;
    }

    termlet::init_crash_reporter();

    // Set default locale before App construction; the persisted preference
    // will be applied inside App::new().
    rust_i18n::set_locale("en");

    // Subprocess mode: when launched as a PTY child by the terminal tab, run
    // the in-process russh bridge instead of starting the GUI.
    let mut initial_action = InitialAction::None;
    if let Some(first) = first_arg {
        if first == "--ssh-bridge" {
            #[cfg(not(windows))]
            {
                let id = args.next().unwrap_or_default();
                return termlet::run_ssh_bridge(&id);
            }
            #[cfg(windows)]
            {
                eprintln!("termlet: --ssh-bridge is provided by termlet-bridge.exe");
                return ExitCode::from(2);
            }
        } else if first == "--quick-ssh" {
            if let Some(id_str) = args.next()
                && let Ok(id) = id_str.parse::<u32>()
            {
                initial_action = InitialAction::QuickSsh(id);
            }
        } else if first == "--new-window" {
            initial_action = InitialAction::NewWindow;
        } else if first == "--quick-local" {
            initial_action = InitialAction::QuickLocal;
        }
    }

    let icon = window::icon::from_file_data(ICON_BYTES, None).ok();
    let app = iced::application(move || Gui::new(initial_action), gui::update, gui::view)
        .title(gui::title())
        .theme(gui::theme)
        .subscription(Gui::subscription)
        .font(lucide_icons::LUCIDE_FONT_BYTES)
        .font(gui::MY_ICONS_FONT_BYTES)
        .font(gui::SOURCE_CODE_PRO_REGULAR)
        .font(gui::SOURCE_CODE_PRO_BOLD)
        .font(gui::SOURCE_CODE_PRO_ITALIC)
        .font(gui::SOURCE_CODE_PRO_BOLD_ITALIC);

    let result = app
        .window(window::Settings {
            icon,
            size: WINDOW_SIZE,
            min_size: Some(WINDOW_SIZE),
            decorations: cfg!(target_os = "macos"),
            position: window::Position::Centered,
            // Windows keeps the window cloaked until the first frame has been
            // presented; otherwise DWM can briefly composite an unpainted
            // borderless client area when the executable starts directly.
            visible: cfg!(not(target_os = "windows")),
            exit_on_close_request: false,
            platform_specific: window::settings::PlatformSpecific {
                #[cfg(target_os = "linux")]
                application_id: String::from("termlet"),
                #[cfg(target_os = "macos")]
                title_hidden: true,
                #[cfg(target_os = "macos")]
                titlebar_transparent: true,
                #[cfg(target_os = "macos")]
                fullsize_content_view: true,
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                corner_radius: Some(10.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .run();

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("termlet: {error}");
            ExitCode::FAILURE
        }
    }
}
