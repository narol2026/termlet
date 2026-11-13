//! Active application modal selection and commands.
//!
//! Modal state stays with the page or domain that owns it. This module resolves
//! the single modal that currently has input priority and turns Enter, Escape,
//! and focus requests into commands. Rendering is delegated to
//! [`crate::views::view_overlay`].

// Rust guideline compliant 2026-10-09

use iced::Task;
use iced::widget::{Id, operation};

use crate::app::{ActiveTab, App, Workspace};
use crate::core::message::Message;
use crate::sftp::SftpPane;
use crate::ssh_connect::SshConnectionState;

/// Identifies the modal that currently owns input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overlay {
    CommandInfo,
    WorkspaceSave,
    WorkspaceSaveConflict,
    WorkspaceRename,
    WorkspaceDelete,
    WorkspaceImport,
    WorkspaceApply,
    WorkspaceRecovery,
    Crash,
    Quit,
    LockedTabClose,
    HostDelete,
    HostExportPassword,
    HostImportPassword,
    HostImportConfirm,
    PortForwardDelete,
    PortForwardLogs,
    PortForwardKeyConfirmation {
        id: crate::core::data::PortForwardId,
        request: u64,
    },
    PortForwardPassword {
        id: crate::core::data::PortForwardId,
        request: u64,
    },
    SnippetDelete,
    SnippetPackageDelete,
    KeychainDelete,
    IdentityDelete,
    KnownHostDelete,
    SftpDelete,
    SftpTransfer,
    SftpRename,
    SftpNewFolder,
    SshKeyConfirmation,
    SftpKeyConfirmation {
        pane: SftpPane,
    },
}

/// Resolves the modal with input priority.
///
/// The order keeps an in-progress connection or transfer prompt above page
/// state, then falls back to application-wide lifecycle dialogs. This is the
/// single source used by rendering, input routing, and the blocking-dialog
/// checks.
pub(crate) fn active(app: &App) -> Option<Overlay> {
    if let Some(dialog) = app.hosts.transfer_dialog.as_ref() {
        return Some(match dialog.kind {
            crate::hosts::state::HostTransferKind::Export => Overlay::HostExportPassword,
            crate::hosts::state::HostTransferKind::ImportPassword => Overlay::HostImportPassword,
            crate::hosts::state::HostTransferKind::ImportConfirm => Overlay::HostImportConfirm,
        });
    }
    if app.sftp.state.new_folder_pending.is_some() {
        return Some(Overlay::SftpNewFolder);
    }
    if app.sftp.state.rename_pending.is_some() {
        return Some(Overlay::SftpRename);
    }
    if app.sftp.state.transfer_modal.is_some() {
        return Some(Overlay::SftpTransfer);
    }
    if app.sftp.state.delete_confirm.is_some() {
        return Some(Overlay::SftpDelete);
    }
    if app.known_hosts.delete_pending.is_some() {
        return Some(Overlay::KnownHostDelete);
    }
    if app.keychain.identity_delete_pending.is_some() {
        return Some(Overlay::IdentityDelete);
    }
    if app.keychain.delete_pending.is_some() {
        return Some(Overlay::KeychainDelete);
    }
    if app.snippets.delete_pending.is_some() {
        return Some(Overlay::SnippetDelete);
    }
    if app.snippets.package_delete_pending.is_some() {
        return Some(Overlay::SnippetPackageDelete);
    }
    if let Some(pending) = app.port_forwards.trust.confirmation() {
        return Some(Overlay::PortForwardKeyConfirmation {
            id: pending.forward.id,
            request: pending.request,
        });
    }
    if let Some(pending) = app.port_forwards.password.prompt() {
        return Some(Overlay::PortForwardPassword {
            id: pending.forward.id,
            request: pending.request,
        });
    }
    if app.port_forwards.logs_open {
        return Some(Overlay::PortForwardLogs);
    }
    if app.port_forwards.delete_pending.is_some() {
        return Some(Overlay::PortForwardDelete);
    }
    if app.hosts.delete_pending.is_some() {
        return Some(Overlay::HostDelete);
    }
    if app.pending_locked_close.is_some() {
        return Some(Overlay::LockedTabClose);
    }
    if app.show_quit_confirm {
        return Some(Overlay::Quit);
    }
    if app.show_crash_dialog {
        return Some(Overlay::Crash);
    }
    if app.workspace_recovery.show_prompt {
        return Some(Overlay::WorkspaceRecovery);
    }
    if app.show_apply_workspace_confirm {
        return Some(Overlay::WorkspaceApply);
    }
    if app.workspace_manager.import_dialog.is_some() {
        return Some(Overlay::WorkspaceImport);
    }
    if app.workspace_manager.delete_id.is_some() {
        return Some(Overlay::WorkspaceDelete);
    }
    if app.workspace_manager.rename_id.is_some() {
        return Some(Overlay::WorkspaceRename);
    }
    if app.workspace_manager.save_pending {
        return Some(if app.workspace_manager.save_conflict.is_some() {
            Overlay::WorkspaceSaveConflict
        } else {
            Overlay::WorkspaceSave
        });
    }
    if app.active_command_info().is_some() {
        return Some(Overlay::CommandInfo);
    }
    if app.active_draft_id().is_some_and(|draft_id| {
        app.terminal.drafts.get(&draft_id).is_some_and(|draft| {
            draft.ssh.attempt.state == SshConnectionState::WaitingForKeyConfirmation
        })
    }) {
        return Some(Overlay::SshKeyConfirmation);
    }
    if app.active_tab == ActiveTab::Workspace(Workspace::Sftp) {
        if app.sftp.state.left.pane.attempt.state == SshConnectionState::WaitingForKeyConfirmation {
            return Some(Overlay::SftpKeyConfirmation {
                pane: SftpPane::Local,
            });
        }
        if app.sftp.state.right.pane.attempt.state == SshConnectionState::WaitingForKeyConfirmation
        {
            return Some(Overlay::SftpKeyConfirmation {
                pane: SftpPane::Remote,
            });
        }
    }
    None
}

impl Overlay {
    /// Whether this modal is rendered by the application-level modal host.
    ///
    /// Connection key prompts are drawn inside their owning page, but still
    /// participate in keyboard routing through this overlay selection.
    pub(crate) const fn is_hosted(self) -> bool {
        !matches!(
            self,
            Self::SshKeyConfirmation | Self::SftpKeyConfirmation { .. }
        )
    }

    /// Cancels or dismisses this modal.
    pub(crate) fn cancel(self, app: &mut App) -> Task<Message> {
        match self {
            Self::CommandInfo => Task::done(Message::CommandHistoryInfoClose),
            Self::WorkspaceSave => Task::done(Message::WorkspaceSaveCancel),
            Self::WorkspaceSaveConflict => Task::done(Message::WorkspaceSaveConflictCancel),
            Self::WorkspaceRename => Task::done(Message::WorkspaceRenameCancel),
            Self::WorkspaceDelete => Task::done(Message::WorkspaceDeleteCancel),
            Self::WorkspaceImport => Task::done(Message::WorkspaceImportCancel),
            Self::WorkspaceApply => Task::done(Message::WorkspaceApplyCancel),
            Self::WorkspaceRecovery => Task::done(Message::WorkspaceRecovery(
                crate::workspace::recovery::Message::RestoreDismiss,
            )),
            Self::Crash => {
                app.show_crash_dialog = false;
                Task::none()
            }
            Self::Quit => {
                app.show_quit_confirm = false;
                Task::none()
            }
            Self::LockedTabClose => {
                app.pending_locked_close = None;
                Task::none()
            }
            Self::HostDelete => Task::done(Message::Hosts(
                crate::hosts::message::Message::HostDeleteCancel,
            )),
            Self::HostExportPassword | Self::HostImportPassword | Self::HostImportConfirm => {
                app.hosts.transfer_dialog = None;
                Task::none()
            }
            Self::PortForwardDelete => Task::done(Message::PortForward(
                crate::port_forward::message::Message::PortForwardDeleteCancel,
            )),
            Self::PortForwardLogs => Task::done(Message::PortForward(
                crate::port_forward::message::Message::PortForwardHideLogs,
            )),
            Self::PortForwardKeyConfirmation { id, request } => Task::done(Message::PortForward(
                crate::port_forward::message::Message::PortForwardKeyCancel { id, request },
            )),
            Self::PortForwardPassword { id, request } => Task::done(Message::PortForward(
                crate::port_forward::message::Message::PortForwardPasswordCancel { id, request },
            )),
            Self::SnippetDelete => Task::done(Message::Snippets(
                crate::snippets::message::Message::SnippetDeleteCancel,
            )),
            Self::SnippetPackageDelete => Task::done(Message::Snippets(
                crate::snippets::message::Message::SnippetPackageDeleteCancel,
            )),
            Self::KeychainDelete => Task::done(Message::Keychain(
                crate::keychain::message::Message::KeychainDeleteCancel,
            )),
            Self::IdentityDelete => Task::done(Message::Keychain(
                crate::keychain::message::Message::IdentityDeleteCancel,
            )),
            Self::KnownHostDelete => Task::done(Message::KnownHosts(
                crate::known_hosts::message::Message::KnownHostDeleteCancel,
            )),
            Self::SftpDelete => Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpDeleteCancel,
            )),
            Self::SftpTransfer => {
                let message = match &app.sftp.state.transfer_modal {
                    Some(crate::sftp::SftpTransferModal::Discard { .. }) => {
                        crate::sftp::message::Message::SftpCancelTransferCancel
                    }
                    Some(crate::sftp::SftpTransferModal::Overwrite(_)) => {
                        crate::sftp::message::Message::SftpOverwriteCancel
                    }
                    None => return Task::none(),
                };
                Task::done(Message::Sftp(message))
            }
            Self::SftpRename => Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpRenameCancel,
            )),
            Self::SftpNewFolder => Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpNewFolderCancel,
            )),
            Self::SshKeyConfirmation | Self::SftpKeyConfirmation { .. } => Task::none(),
        }
    }

    /// Confirms this modal's primary action.
    pub(crate) fn confirm(self, app: &App) -> Task<Message> {
        match self {
            Self::CommandInfo
            | Self::WorkspaceSaveConflict
            | Self::WorkspaceImport
            | Self::Crash
            | Self::PortForwardLogs => Task::none(),
            Self::WorkspaceSave => Task::done(Message::WorkspaceSaveConfirm),
            Self::WorkspaceRename => Task::done(Message::WorkspaceRenameSubmit),
            Self::WorkspaceDelete => Task::done(Message::WorkspaceDeleteConfirm),
            Self::WorkspaceApply => Task::done(Message::WorkspaceApplyConfirm),
            Self::WorkspaceRecovery => Task::done(Message::WorkspaceRecovery(
                crate::workspace::recovery::Message::RestoreConfirm,
            )),
            Self::Quit => Task::done(Message::WindowCloseConfirm),
            Self::LockedTabClose => Task::done(Message::Terminal(
                crate::terminal::message::Message::TermCloseLockedConfirm,
            )),
            Self::HostDelete => app.hosts.delete_pending.map_or_else(Task::none, |id| {
                Task::done(Message::Hosts(crate::hosts::message::Message::DeleteHost(
                    id,
                )))
            }),
            Self::HostExportPassword | Self::HostImportPassword | Self::HostImportConfirm => {
                let Some(dialog) = app.hosts.transfer_dialog.as_ref() else {
                    return Task::none();
                };
                if !dialog.confirm_enabled() {
                    return Task::none();
                }
                let message = match dialog.kind {
                    crate::hosts::state::HostTransferKind::Export => {
                        crate::hosts::message::Message::HostExportSubmit
                    }
                    crate::hosts::state::HostTransferKind::ImportPassword => {
                        crate::hosts::message::Message::HostImportPasswordSubmit
                    }
                    crate::hosts::state::HostTransferKind::ImportConfirm => {
                        crate::hosts::message::Message::HostImportConfirm
                    }
                };
                Task::done(Message::Hosts(message))
            }
            Self::PortForwardDelete => {
                app.port_forwards
                    .delete_pending
                    .map_or_else(Task::none, |id| {
                        Task::done(Message::PortForward(
                            crate::port_forward::message::Message::PortForwardDelete(id),
                        ))
                    })
            }
            Self::SnippetDelete => app.snippets.delete_pending.map_or_else(Task::none, |id| {
                Task::done(Message::Snippets(
                    crate::snippets::message::Message::SnippetDelete(id),
                ))
            }),
            Self::SnippetPackageDelete => {
                app.snippets
                    .package_delete_pending
                    .map_or_else(Task::none, |id| {
                        Task::done(Message::Snippets(
                            crate::snippets::message::Message::SnippetPackageDelete(id),
                        ))
                    })
            }
            Self::KeychainDelete => app.keychain.delete_pending.map_or_else(Task::none, |id| {
                Task::done(Message::Keychain(
                    crate::keychain::message::Message::KeychainDelete(id),
                ))
            }),
            Self::IdentityDelete => {
                app.keychain
                    .identity_delete_pending
                    .map_or_else(Task::none, |id| {
                        Task::done(Message::Keychain(
                            crate::keychain::message::Message::IdentityDelete(id),
                        ))
                    })
            }
            Self::KnownHostDelete => app
                .known_hosts
                .delete_pending
                .map_or_else(Task::none, |id| {
                    Task::done(Message::KnownHosts(
                        crate::known_hosts::message::Message::KnownHostDelete(id),
                    ))
                }),
            Self::SftpDelete => Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpDeleteConfirm,
            )),
            Self::SftpTransfer => {
                let message = match &app.sftp.state.transfer_modal {
                    Some(crate::sftp::SftpTransferModal::Discard { .. }) => {
                        crate::sftp::message::Message::SftpCancelTransfer
                    }
                    Some(crate::sftp::SftpTransferModal::Overwrite(_)) => {
                        crate::sftp::message::Message::SftpOverwriteReplace
                    }
                    None => return Task::none(),
                };
                Task::done(Message::Sftp(message))
            }
            Self::SftpRename => Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpRenameConfirm,
            )),
            Self::SftpNewFolder => Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpNewFolderConfirm,
            )),
            Self::SshKeyConfirmation => Task::done(Message::SshConnect(
                crate::ssh_connect::message::Message::SshKeyConfirmAccept,
            )),
            Self::PortForwardKeyConfirmation { id, request } => Task::done(Message::PortForward(
                crate::port_forward::message::Message::PortForwardKeyConfirm {
                    id,
                    request,
                    save: true,
                },
            )),
            Self::PortForwardPassword { id, request } => Task::done(Message::PortForward(
                crate::port_forward::message::Message::PortForwardPasswordSubmit { id, request },
            )),
            Self::SftpKeyConfirmation { pane } => Task::done(Message::Sftp(
                crate::sftp::message::Message::SftpKeyConfirmAccept { pane },
            )),
        }
    }

    /// Requests focus for this modal's editable control, if it has one.
    pub(crate) fn focus(self) -> Task<Message> {
        let id = match self {
            Self::WorkspaceSave => "workspace_save_name",
            Self::WorkspaceRename => "workspace_rename_name",
            Self::HostExportPassword => crate::hosts::transfer_view::HOST_EXPORT_PASSWORD_INPUT_ID,
            Self::HostImportPassword => crate::hosts::transfer_view::HOST_IMPORT_PASSWORD_INPUT_ID,
            Self::PortForwardPassword { .. } => {
                crate::port_forward::view::PORT_FORWARD_PASSWORD_INPUT_ID
            }
            Self::SftpRename => "sftp_rename_name",
            Self::SftpNewFolder => "sftp_new_folder_name",
            Self::CommandInfo
            | Self::WorkspaceSaveConflict
            | Self::WorkspaceDelete
            | Self::WorkspaceImport
            | Self::WorkspaceApply
            | Self::WorkspaceRecovery
            | Self::Crash
            | Self::Quit
            | Self::LockedTabClose
            | Self::HostDelete
            | Self::HostImportConfirm
            | Self::PortForwardDelete
            | Self::PortForwardLogs
            | Self::PortForwardKeyConfirmation { .. }
            | Self::SnippetDelete
            | Self::SnippetPackageDelete
            | Self::KeychainDelete
            | Self::IdentityDelete
            | Self::KnownHostDelete
            | Self::SftpDelete
            | Self::SftpTransfer
            | Self::SshKeyConfirmation
            | Self::SftpKeyConfirmation { .. } => return operation::unfocus(),
        };
        operation::focus(Id::new(id))
    }
}

#[cfg(test)]
mod tests {
    use super::{Overlay, active};
    use crate::core::data::HostId;
    use crate::core::test_support::app;
    use crate::sftp::SftpPane;

    #[test]
    fn domain_modal_wins_over_shell_modal_in_one_resolution_order() {
        let mut app = app();
        app.hosts.delete_pending = Some(HostId(7));
        app.show_quit_confirm = true;

        assert_eq!(active(&app), Some(Overlay::HostDelete));
    }

    #[test]
    fn closing_a_page_modal_reveals_the_shell_modal() {
        let mut app = app();
        app.show_quit_confirm = true;

        assert_eq!(active(&app), Some(Overlay::Quit));
        app.show_quit_confirm = false;
        assert_eq!(active(&app), None);
    }

    #[test]
    fn page_owned_key_prompts_do_not_use_the_global_modal_host() {
        assert!(!Overlay::SshKeyConfirmation.is_hosted());
        assert!(
            !Overlay::SftpKeyConfirmation {
                pane: SftpPane::Remote
            }
            .is_hosted()
        );
        assert!(Overlay::Quit.is_hosted());
    }
}
