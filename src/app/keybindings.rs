// Rust guideline compliant 2026-10-09

use iced::keyboard;

/// Platform-appropriate shortcut modifier.
///
/// This mirrors `iced::keyboard::Modifiers::COMMAND`, which is **⌘ (Cmd/LOGO)**
/// on macOS and **Ctrl** on Linux / Windows.  We re-export it as a module-level
/// constant so the intent is clear at every call site.
const SHORTCUT_MOD: keyboard::Modifiers = keyboard::Modifiers::COMMAND;

/// Return the platform's fullscreen toggle shortcut.
///
/// Windows and Linux use `F11`. macOS follows the native convention of
/// `Ctrl+Cmd+F`, leaving `F11` available for Show Desktop.
pub fn fullscreen_shortcut() -> (keyboard::Key, keyboard::Modifiers) {
    #[cfg(target_os = "macos")]
    {
        (
            keyboard::Key::Character("f".into()),
            keyboard::Modifiers::CTRL | keyboard::Modifiers::COMMAND,
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        (
            keyboard::Key::Named(keyboard::key::Named::F11),
            keyboard::Modifiers::empty(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenLocalTerminal,
    NewTerminalTab,
    ToggleSearch,
    CloseCurrentTab,
    DuplicateSession,
    ToggleFullscreen,
    IncreaseFontSize,
    DecreaseFontSize,
}

pub struct KeybindingManager {
    bindings: Vec<(keyboard::Key, keyboard::Modifiers, Action)>,
}

impl KeybindingManager {
    pub fn new() -> Self {
        let mut mgr = Self {
            bindings: Vec::new(),
        };
        mgr.register(
            keyboard::Key::Character("l".into()),
            SHORTCUT_MOD,
            Action::OpenLocalTerminal,
        );
        mgr.register(
            keyboard::Key::Character("n".into()),
            SHORTCUT_MOD,
            Action::NewTerminalTab,
        );
        mgr.register(
            keyboard::Key::Character("f".into()),
            SHORTCUT_MOD,
            Action::ToggleSearch,
        );
        mgr.register(
            keyboard::Key::Character("w".into()),
            SHORTCUT_MOD,
            Action::CloseCurrentTab,
        );
        mgr.register(
            keyboard::Key::Character("d".into()),
            SHORTCUT_MOD,
            Action::DuplicateSession,
        );
        let (fullscreen_key, fullscreen_modifiers) = fullscreen_shortcut();
        mgr.register(
            fullscreen_key,
            fullscreen_modifiers,
            Action::ToggleFullscreen,
        );
        mgr.register(
            keyboard::Key::Character("=".into()),
            SHORTCUT_MOD,
            Action::IncreaseFontSize,
        );
        mgr.register(
            keyboard::Key::Character("+".into()),
            SHORTCUT_MOD,
            Action::IncreaseFontSize,
        );
        mgr.register(
            keyboard::Key::Character("-".into()),
            SHORTCUT_MOD,
            Action::DecreaseFontSize,
        );
        mgr
    }

    pub fn register(&mut self, key: keyboard::Key, modifiers: keyboard::Modifiers, action: Action) {
        self.bindings.push((key, modifiers, action));
    }

    pub fn resolve(&self, key: &keyboard::Key, modifiers: keyboard::Modifiers) -> Option<&Action> {
        self.bindings.iter().find_map(|(k, m, action)| {
            let modifier_matches = if m.is_empty() {
                modifiers.is_empty()
            } else {
                modifiers.contains(*m)
            };
            (k == key && modifier_matches).then_some(action)
        })
    }

    /// All registered bindings, for display in the Settings screen.
    pub fn bindings(&self) -> &[(keyboard::Key, keyboard::Modifiers, Action)] {
        &self.bindings
    }
}

/// Render a shortcut as a platform-aware string, e.g. `⌘L` or `Ctrl+1`.
pub fn format_binding(key: &keyboard::Key, modifiers: keyboard::Modifiers) -> String {
    let mut parts: Vec<String> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if modifiers.control() {
            parts.push("⌃".into());
        }
        if modifiers.logo() {
            parts.push("⌘".into());
        }
        if modifiers.alt() {
            parts.push("⌥".into());
        }
        if modifiers.shift() {
            parts.push("⇧".into());
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        if modifiers.control() {
            parts.push("Ctrl".into());
        }
        if modifiers.logo() {
            parts.push("Super".into());
        }
        if modifiers.alt() {
            parts.push("Alt".into());
        }
        if modifiers.shift() {
            parts.push("Shift".into());
        }
    }
    parts.push(key_label(key));
    parts.join("+")
}

fn key_label(key: &keyboard::Key) -> String {
    match key {
        keyboard::Key::Character(c) if c.chars().count() == 1 => c.to_uppercase(),
        keyboard::Key::Character(c) => c.to_string(),
        keyboard::Key::Named(n) => match n {
            keyboard::key::Named::Space => "Space".to_string(),
            keyboard::key::Named::Enter => "Enter".to_string(),
            keyboard::key::Named::Tab => "Tab".to_string(),
            keyboard::key::Named::Backspace => "Backspace".to_string(),
            keyboard::key::Named::Delete => "Delete".to_string(),
            keyboard::key::Named::Escape => "Esc".to_string(),
            _ => format!("{n:?}"),
        },
        keyboard::Key::Unidentified => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fullscreen_shortcut_matches_the_platform() {
        let bindings = KeybindingManager::new();
        let (key, modifiers) = fullscreen_shortcut();

        assert_eq!(
            bindings.resolve(&key, modifiers),
            Some(&Action::ToggleFullscreen)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_leaves_f11_available_for_show_desktop() {
        let bindings = KeybindingManager::new();
        let f11 = keyboard::Key::Named(keyboard::key::Named::F11);

        assert_eq!(bindings.resolve(&f11, keyboard::Modifiers::empty()), None);
        assert_eq!(
            format_binding(
                &keyboard::Key::Character("f".into()),
                keyboard::Modifiers::CTRL | keyboard::Modifiers::COMMAND,
            ),
            "⌃+⌘+F"
        );
    }
}
