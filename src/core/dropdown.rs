//! Shared state and identities for keyboard-driven dropdowns.

// Rust guideline compliant 2026-10-09

use crate::core::list_cursor::ListCursor;

/// Vertical gap between rendered dropdown rows.
pub(crate) const ROW_GAP: f32 = 4.0;

/// Owns a dropdown's live filter and keyboard selection.
///
/// The selected value remains in the owning domain. This state tracks the
/// transient filter and the option that keyboard navigation is currently on,
/// using the option's stable identity rather than its filtered index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DropdownState<T> {
    query: String,
    cursor: ListCursor<T>,
}

impl<T> Default for DropdownState<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> DropdownState<T> {
    /// Creates an empty dropdown state.
    pub(crate) fn new() -> Self {
        Self {
            query: String::new(),
            cursor: ListCursor::new(),
        }
    }

    /// Returns the active filter text.
    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    /// Replaces the filter text and clears stale keyboard focus.
    pub(crate) fn set_query(&mut self, query: String) {
        self.query = query;
        self.cursor.clear();
    }

    /// Clears all transient dropdown state.
    pub(crate) fn reset(&mut self) {
        self.query.clear();
        self.cursor.clear();
    }
}

impl<T: Clone + Eq> DropdownState<T> {
    /// Starts a session on the current value or the first visible option.
    pub(crate) fn begin(&mut self, current: Option<T>, items: &[T]) {
        self.query.clear();
        self.cursor.clear();
        if items.is_empty() {
            return;
        }

        if current.as_ref().is_some_and(|item| items.contains(item)) {
            if let Some(current) = current {
                self.cursor.select(current);
            }
        } else if let Some(first) = items.first() {
            self.cursor.select(first.clone());
        }
    }

    /// Returns the focused option, falling back to the first visible option.
    pub(crate) fn active<'a>(&self, items: &'a [T]) -> Option<&'a T> {
        self.cursor.selected_in(items).or_else(|| items.first())
    }

    /// Returns the focused option's visible index.
    pub(crate) fn active_index(&self, items: &[T]) -> Option<usize> {
        self.cursor
            .selected_index(items)
            .or_else(|| (!items.is_empty()).then_some(0))
    }

    /// Moves keyboard focus to the previous visible option.
    pub(crate) fn move_previous(&mut self, items: &[T]) {
        self.cursor.move_previous(items);
    }

    /// Moves keyboard focus to the next visible option.
    pub(crate) fn move_next(&mut self, items: &[T]) {
        self.cursor.move_next(items);
    }
}

/// Identifies one dropdown in the shared keyboard-routing table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropdownTarget {
    HostFormKeychain,
    HostFormJump,
    HostFormIdentity,
    HostFormTag,
    PortForwardHost,
    SnippetFormPackage,
    SnippetSidebarPackage,
    HostsTagFilter,
    HostsTransferMenu,
    WorkspaceTransferMenu,
    SettingsTheme,
    SettingsLocale,
    SettingsShellIntegration,
    SettingsLocalShell,
    SettingsHistoryLimit,
    SettingsDiagnostics,
}

impl DropdownTarget {
    /// Returns the stable identity used by scroll state and focus operations.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::HostFormKeychain => "dropdown_host_form_keychain",
            Self::HostFormJump => "dropdown_host_form_jump",
            Self::HostFormIdentity => "dropdown_host_form_identity",
            Self::HostFormTag => "dropdown_host_form_tag",
            Self::PortForwardHost => "dropdown_port_forward_host",
            Self::SnippetFormPackage => "dropdown_snippet_form_package",
            Self::SnippetSidebarPackage => "dropdown_snippet_sidebar_package",
            Self::HostsTagFilter => "dropdown_hosts_tag_filter",
            Self::HostsTransferMenu => "dropdown_hosts_transfer_menu",
            Self::WorkspaceTransferMenu => "dropdown_workspace_transfer_menu",
            Self::SettingsTheme => "dropdown_settings_theme",
            Self::SettingsLocale => "dropdown_settings_locale",
            Self::SettingsShellIntegration => "dropdown_settings_shell_integration",
            Self::SettingsLocalShell => "dropdown_settings_local_shell",
            Self::SettingsHistoryLimit => "dropdown_settings_history_limit",
            Self::SettingsDiagnostics => "dropdown_settings_diagnostics",
        }
    }

    /// Returns the search input's stable focus identity.
    pub(crate) const fn input_id(self) -> &'static str {
        match self {
            Self::HostFormKeychain => "dropdown_host_form_keychain_input",
            Self::HostFormJump => "dropdown_host_form_jump_input",
            Self::HostFormIdentity => "dropdown_host_form_identity_input",
            Self::HostFormTag => "dropdown_host_form_tag_input",
            Self::PortForwardHost => "dropdown_port_forward_host_input",
            Self::SnippetFormPackage
            | Self::SnippetSidebarPackage
            | Self::HostsTagFilter
            | Self::HostsTransferMenu
            | Self::WorkspaceTransferMenu
            | Self::SettingsTheme
            | Self::SettingsLocale
            | Self::SettingsShellIntegration
            | Self::SettingsLocalShell
            | Self::SettingsHistoryLimit
            | Self::SettingsDiagnostics => self.id(),
        }
    }

    /// Returns the option-list scroll identity.
    pub(crate) const fn scroll_id(self) -> &'static str {
        match self {
            Self::HostFormKeychain => "dropdown_host_form_keychain_scroll",
            Self::HostFormJump => "dropdown_host_form_jump_scroll",
            Self::HostFormIdentity => "dropdown_host_form_identity_scroll",
            Self::HostFormTag => "dropdown_host_form_tag_scroll",
            Self::PortForwardHost => "dropdown_port_forward_host_scroll",
            Self::SnippetFormPackage => "dropdown_snippet_form_package_scroll",
            Self::SnippetSidebarPackage => "dropdown_snippet_sidebar_package_scroll",
            Self::HostsTagFilter => "dropdown_hosts_tag_filter_scroll",
            Self::HostsTransferMenu => "dropdown_hosts_transfer_menu_scroll",
            Self::WorkspaceTransferMenu => "dropdown_workspace_transfer_menu_scroll",
            Self::SettingsTheme => "dropdown_settings_theme_scroll",
            Self::SettingsLocale => "dropdown_settings_locale_scroll",
            Self::SettingsShellIntegration => "dropdown_settings_shell_integration_scroll",
            Self::SettingsLocalShell => "dropdown_settings_local_shell_scroll",
            Self::SettingsHistoryLimit => "dropdown_settings_history_limit_scroll",
            Self::SettingsDiagnostics => "dropdown_settings_diagnostics_scroll",
        }
    }

    /// Returns the fixed height of one rendered option row.
    pub(crate) const fn row_height(self) -> f32 {
        // Each row has 8 px of vertical padding per side. A single 13 pt line
        // adds about 17 px; a second 12 pt line plus its 1 px gap adds 17 px.
        match self {
            Self::HostFormKeychain | Self::HostFormJump | Self::PortForwardHost => 50.0,
            Self::HostFormIdentity
            | Self::HostFormTag
            | Self::SnippetFormPackage
            | Self::SnippetSidebarPackage
            | Self::HostsTagFilter
            | Self::HostsTransferMenu
            | Self::WorkspaceTransferMenu
            | Self::SettingsTheme
            | Self::SettingsLocale
            | Self::SettingsShellIntegration
            | Self::SettingsLocalShell
            | Self::SettingsHistoryLimit
            | Self::SettingsDiagnostics => 33.0,
        }
    }
}

/// Identifies one entry in an Export/Import action dropdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferMenuAction {
    Export,
    Import,
}

impl TransferMenuAction {
    /// Returns the actions in their displayed order.
    pub(crate) const fn all() -> &'static [Self] {
        &[Self::Export, Self::Import]
    }
}

#[cfg(test)]
mod tests {
    use super::DropdownState;

    #[test]
    fn beginning_selects_the_current_value() {
        let mut state = DropdownState::new();

        state.begin(Some("beta"), &["alpha", "beta", "gamma"]);

        assert_eq!(state.active(&["alpha", "beta", "gamma"]), Some(&"beta"));
    }

    #[test]
    fn beginning_falls_back_to_the_first_visible_value() {
        let mut state = DropdownState::new();

        state.begin(Some("hidden"), &["alpha", "beta"]);

        assert_eq!(state.active(&["alpha", "beta"]), Some(&"alpha"));
    }

    #[test]
    fn changing_the_query_clears_stale_focus() {
        let mut state = DropdownState::new();
        state.begin(Some("beta"), &["alpha", "beta", "gamma"]);

        state.set_query("gam".into());

        assert_eq!(state.active(&["gamma"]), Some(&"gamma"));
        assert_eq!(state.active_index(&["gamma"]), Some(0));
    }

    #[test]
    fn movement_uses_visible_order() {
        let mut state = DropdownState::new();
        state.begin(Some("beta"), &["alpha", "beta", "gamma"]);

        state.move_next(&["gamma", "beta", "alpha"]);
        assert_eq!(state.active(&["gamma", "beta", "alpha"]), Some(&"alpha"));

        state.move_previous(&["gamma", "beta", "alpha"]);
        assert_eq!(state.active(&["gamma", "beta", "alpha"]), Some(&"beta"));
    }
}
