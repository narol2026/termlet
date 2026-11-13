//! Stable action identities for keyboard-driven context menus.

// Rust guideline compliant 2026-10-09

/// Identifies one action in a context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContextMenuAction {
    HostEdit,
    HostDuplicate,
    HostDelete,
    TerminalDuplicate,
    TerminalNewWindow,
    TerminalRename,
    TerminalToggleLock,
    TerminalClose,
    TerminalCloseOthers,
    TerminalCloseRight,
    SftpTransfer,
    SftpNewFolder,
    SftpRename,
    SftpDelete,
    SftpSidebarDuplicate,
    SftpSidebarClose,
    SnippetEdit,
    SnippetCopy,
    SnippetDelete,
    SnippetPackageEdit,
    SnippetPackageDelete,
    KeychainEdit,
    KeychainDelete,
    IdentityEdit,
    IdentityDelete,
    PortForwardToggle,
    PortForwardEdit,
    PortForwardLogs,
    PortForwardDuplicate,
    PortForwardDelete,
    KnownHostDetails,
    KnownHostDelete,
}
