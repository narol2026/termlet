pub mod activity_log;
pub mod cache;
pub mod collection;
pub mod context_menu;
pub mod crash_reporter;
pub mod crypto;
pub mod cursor_focus;
pub mod data;
pub mod diagnostics;
pub mod dropdown;
pub mod focus_area;
pub mod fonts;
pub mod forms;
pub mod fuzzy;
pub mod grid_cursor;
pub mod i18n;
pub mod ids;
pub mod list_cursor;
pub mod locale;
pub mod message;
pub mod open;
pub mod paths;
pub mod platform;
pub mod process;
pub mod storage;
pub mod storage_service;
#[cfg(test)]
pub(crate) mod test_support;
pub mod theme;
pub mod time;
pub mod version;
#[cfg(target_os = "windows")]
mod windows_permissions;
// Windows-only: keeps the main window out of DWM until its first frame.
#[cfg(target_os = "windows")]
pub(crate) mod window_startup;
// Windows-only: native title-bar hit testing for the borderless window.
#[cfg(target_os = "windows")]
pub mod window_chrome;
// AppKit-only: native traffic lights in the full-size content view.
#[cfg(target_os = "macos")]
pub mod traffic_lights;
// AppKit-only: "New Window" item in the Dock's right-click menu.
#[cfg(target_os = "macos")]
pub mod dock_menu;
pub mod widgets;
