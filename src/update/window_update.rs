//! Window lifecycle, geometry, and dialog-close updates.

// Rust guideline compliant 2026-10-09

use iced::Task;
use iced::window;

#[cfg(target_os = "macos")]
use super::reposition_traffic_lights_now;
use super::{App, Message, close_app};

pub(super) fn update(app: &mut App, msg: &Message) -> Task<Message> {
    match msg {
        Message::WindowClose => {
            if !app.has_any_dialog_open() {
                app.show_quit_confirm = true;
            }
            Task::none()
        }
        Message::WindowCloseConfirm => close_app(app),
        Message::WindowCloseCancel => {
            app.show_quit_confirm = false;
            Task::none()
        }
        Message::WindowMinimize => window::oldest().then(|id| {
            if let Some(id) = id {
                window::minimize(id, true)
            } else {
                Task::none()
            }
        }),
        Message::WindowMaximize => {
            if cfg!(target_os = "macos") {
                // macOS "maximize" is native fullscreen: from here the AppKit
                // observer owns the traffic lights, reacting to the
                // `WillEnterFullScreen` notification this toggle posts.
                toggle_fullscreen_mode()
            } else {
                maximize_or_restore()
            }
        }
        Message::WindowToggleFullscreen => toggle_fullscreen_mode(),
        Message::FullscreenChanged(fullscreen) => {
            app.window.is_maximized = *fullscreen;
            Task::none()
        }
        Message::WindowDrag => window::oldest().then(|id| {
            if let Some(id) = id {
                window::drag(id)
            } else {
                Task::none()
            }
        }),
        Message::CursorPositionChanged(pos) => {
            app.window.cursor_pos = *pos;
            crate::sftp::update::external_drag_cursor_moved(app, *pos)
        }
        Message::WindowPositionChanged(position) => {
            app.window.position = *position;
            Task::none()
        }
        Message::WindowScaleFactorChanged(scale_factor) => {
            app.window.scale_factor = *scale_factor;
            Task::none()
        }
        Message::WindowResized(_size) => {
            app.sftp.state.context_menu = None;
            // iced only corrects the tab strip's overflow state on Scrolled
            // events, which it does not fire when content stops overflowing, so
            // a resize that makes tabs fit would leave stale edge fades. Clear
            // the flag; the next Scrolled event sets it back if they still don't.
            app.top_tabs_can_scroll = false;
            // X11 window managers can move the window during maximize without
            // Iced firing Window::Moved, leaving window_position stale — and
            // external_file_drag_point needs a real one to map screen to logical
            // coordinates. macOS and Windows report moves reliably, so they skip
            // the query.
            if cfg!(all(unix, not(target_os = "macos"))) {
                return window::oldest().then(|id| {
                    if let Some(id) = id {
                        window::position(id)
                            .then(|pos| Task::done(Message::WindowPositionChanged(pos)))
                    } else {
                        Task::none()
                    }
                });
            }

            #[cfg(target_os = "macos")]
            {
                return window::oldest().then(|id| {
                    if let Some(id) = id {
                        window::mode(id).then(|mode| {
                            // Only feeds the app's window state; whether the
                            // title bar is ours to lay out is decided in `sync`.
                            Task::done(Message::FullscreenChanged(mode == window::Mode::Fullscreen))
                                .chain(reposition_traffic_lights_now())
                        })
                    } else {
                        Task::none()
                    }
                });
            }

            #[cfg(target_os = "windows")]
            {
                // Native maximize/restore (double-click or Win+Arrow) changes
                // the window without going through `Message::WindowMaximize`.
                // Re-read the real state so the top-bar icon stays in sync.
                window::oldest().then(|id| {
                    if let Some(id) = id {
                        window::is_maximized(id)
                            .then(|maximized| Task::done(Message::FullscreenChanged(maximized)))
                    } else {
                        Task::none()
                    }
                })
            }

            #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
            Task::none()
        }
        #[cfg(target_os = "windows")]
        Message::SyncWindowChrome => {
            if app.window.window_prepared {
                return Task::none();
            }
            app.window.window_prepared = true;
            let frame_presented = app.window.first_frame_presented;

            window::oldest().then(move |id| {
                if let Some(id) = id {
                    window::run(id, move |window| {
                        crate::core::window_chrome::sync(window);
                        crate::core::window_startup::show_cloaked(window);
                        if frame_presented {
                            crate::core::window_startup::reveal_first_frame(window);
                        }
                    })
                    .discard()
                } else {
                    Task::none()
                }
            })
        }
        #[cfg(target_os = "windows")]
        Message::RevealWindowFirstFrame => {
            if app.window.first_frame_presented {
                return Task::none();
            }
            app.window.first_frame_presented = true;
            if !app.window.window_prepared {
                return Task::none();
            }

            window::oldest().then(|id| {
                if let Some(id) = id {
                    window::run(id, |window| {
                        crate::core::window_startup::reveal_first_frame(window);
                    })
                    .discard()
                } else {
                    Task::none()
                }
            })
        }
        _ => unreachable!("window handler received another message"),
    }
}

fn toggle_fullscreen_mode() -> Task<Message> {
    window::oldest().then(|id| {
        if let Some(id) = id {
            window::mode(id).then(move |mode| {
                let new_mode = next_fullscreen_mode(mode);
                let is_fullscreen = new_mode == window::Mode::Fullscreen;
                window::set_mode(id, new_mode)
                    .chain(Task::done(Message::FullscreenChanged(is_fullscreen)))
            })
        } else {
            Task::none()
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaximizeAction {
    ExitFullscreen,
    ToggleMaximized,
}

fn maximize_action(mode: window::Mode) -> MaximizeAction {
    if mode == window::Mode::Fullscreen {
        MaximizeAction::ExitFullscreen
    } else {
        MaximizeAction::ToggleMaximized
    }
}

fn maximize_or_restore() -> Task<Message> {
    window::oldest().then(|id| {
        if let Some(id) = id {
            window::mode(id).then(move |mode| match maximize_action(mode) {
                MaximizeAction::ExitFullscreen => window::set_mode(id, window::Mode::Windowed)
                    .chain(Task::done(Message::FullscreenChanged(false))),
                MaximizeAction::ToggleMaximized => {
                    window::is_maximized(id).then(move |actually_maximized| {
                        let new_state = !actually_maximized;
                        window::maximize(id, new_state)
                            .chain(Task::done(Message::FullscreenChanged(new_state)))
                    })
                }
            })
        } else {
            Task::none()
        }
    })
}

fn next_fullscreen_mode(mode: window::Mode) -> window::Mode {
    match mode {
        window::Mode::Fullscreen => window::Mode::Windowed,
        _ => window::Mode::Fullscreen,
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    #[test]
    fn fullscreen_mode_toggles_between_windowed_and_fullscreen() {
        assert_eq!(
            next_fullscreen_mode(window::Mode::Windowed),
            window::Mode::Fullscreen
        );
        assert_eq!(
            next_fullscreen_mode(window::Mode::Fullscreen),
            window::Mode::Windowed
        );
    }

    #[test]
    fn the_maximize_button_exits_fullscreen() {
        assert_eq!(
            maximize_action(window::Mode::Fullscreen),
            MaximizeAction::ExitFullscreen
        );
        assert_eq!(
            maximize_action(window::Mode::Windowed),
            MaximizeAction::ToggleMaximized
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use crate::core::test_support::app;

    #[test]
    fn a_first_frame_before_window_preparation_is_remembered() {
        let mut app = app();
        assert!(!app.window.window_prepared);
        assert!(!app.window.first_frame_presented);

        let _task = update(&mut app, &Message::RevealWindowFirstFrame);
        assert!(app.window.first_frame_presented);
        assert!(
            !app.window.window_prepared,
            "the frame alone must not claim the window was shown"
        );

        let _task = update(&mut app, &Message::SyncWindowChrome);
        assert!(app.window.window_prepared);
    }
}
