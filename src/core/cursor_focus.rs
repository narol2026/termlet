//! Stable focus targets for keyboard-driven collection cursors.

use crate::core::data::RightSidebarTab;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorTarget {
    Hosts,
    Keychain,
    PortForwards,
    Snippets,
    KnownHosts,
    Workspaces,
    RightSidebarSnippets,
    RightSidebarCommands,
    RightSidebarTheme,
    SftpLocalFiles,
    SftpRemoteFiles,
    SftpLocalHostPicker,
    SftpRemoteHostPicker,
}

impl CursorTarget {
    /// Returns the focus target for a right-sidebar list.
    #[must_use]
    pub const fn for_right_sidebar(tab: RightSidebarTab) -> Self {
        match tab {
            RightSidebarTab::Snippets => Self::RightSidebarSnippets,
            RightSidebarTab::Commands => Self::RightSidebarCommands,
            RightSidebarTab::Theme => Self::RightSidebarTheme,
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Hosts => "cursor_hosts",
            Self::Keychain => "cursor_keychain",
            Self::PortForwards => "cursor_port_forwards",
            Self::Snippets => "cursor_snippets",
            Self::KnownHosts => "cursor_known_hosts",
            Self::Workspaces => "cursor_workspaces",
            Self::RightSidebarSnippets => "cursor_right_sidebar_snippets",
            Self::RightSidebarCommands => "cursor_right_sidebar_commands",
            Self::RightSidebarTheme => "cursor_right_sidebar_theme",
            Self::SftpLocalFiles => "cursor_sftp_local_files",
            Self::SftpRemoteFiles => "cursor_sftp_remote_files",
            Self::SftpLocalHostPicker => "cursor_sftp_local_host_picker",
            Self::SftpRemoteHostPicker => "cursor_sftp_remote_host_picker",
        }
    }
}
