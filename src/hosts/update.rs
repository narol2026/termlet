//! Dispatches saved-host, new-tab, navigation, and transfer messages.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::App;
use crate::core::diagnostics::Level;
use crate::core::message::Message;
use crate::hosts::message::Message as DomainMessage;

mod animation;
mod connect;
mod crud;
mod form;
mod navigation;
mod new_tab;
mod transfer;

pub(crate) use animation::tick_animations;

/// Route one hosts-domain message to its owning update module.
pub fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "hosts::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        message @ (DomainMessage::Nav(_)
        | DomainMessage::SelectWorkspace(_)
        | DomainMessage::SelectTab(_)
        | DomainMessage::OpenLocalTerminal) => navigation::update(app, &message),

        message @ (DomainMessage::OpenNewTab
        | DomainMessage::NewTabQueryChanged(_)
        | DomainMessage::NewTabConnect
        | DomainMessage::NewTabSelectUp
        | DomainMessage::NewTabSelectDown
        | DomainMessage::CloseNewTab(_)
        | DomainMessage::NewTabHovered { .. }) => new_tab::update(app, message),

        message @ DomainMessage::ConnectHost(_) => connect::update(app, &message),

        message @ (DomainMessage::SearchChanged(_)
        | DomainMessage::SelectHost(_)
        | DomainMessage::HostHoverEnter(_)
        | DomainMessage::HostHoverLeave(_)
        | DomainMessage::EditHost(_)
        | DomainMessage::DeleteHost(_)
        | DomainMessage::HostDeleteRequest(_)
        | DomainMessage::HostDeleteCancel
        | DomainMessage::DuplicateHost(_)
        | DomainMessage::HostsContextMenuOpen(_)
        | DomainMessage::HostsContextMenuClose
        | DomainMessage::HostsViewCursorMoved(_)
        | DomainMessage::ToggleView
        | DomainMessage::HostCreated { .. }
        | DomainMessage::HostDuplicated { .. }
        | DomainMessage::OsDetected { .. }
        | DomainMessage::HostTagFilterToggle
        | DomainMessage::HostTagFilterSelect(_)) => crud::update(app, message),

        message @ (DomainMessage::ShowForm
        | DomainMessage::HideForm
        | DomainMessage::FormName(_)
        | DomainMessage::FormHostname(_)
        | DomainMessage::FormPort(_)
        | DomainMessage::FormUsername(_)
        | DomainMessage::FormAuth(_)
        | DomainMessage::FormToggleJumpPicker
        | DomainMessage::FormJumpSearch(_)
        | DomainMessage::FormSelectJump(_)
        | DomainMessage::FormClearJump
        | DomainMessage::FormKeyPath(_)
        | DomainMessage::FormPassword(_)
        | DomainMessage::FormTogglePasswordVisible
        | DomainMessage::FormToggleIdentityPicker
        | DomainMessage::FormIdentitySearch(_)
        | DomainMessage::FormSelectIdentity(_)
        | DomainMessage::FormClearIdentity
        | DomainMessage::FormToggleKeychainPicker
        | DomainMessage::FormKeychainSearch(_)
        | DomainMessage::SaveHost
        | DomainMessage::FormFillKeyPath(_)
        | DomainMessage::FormTagPickerToggle
        | DomainMessage::FormTagSearch(_)
        | DomainMessage::FormTagAdd(_)
        | DomainMessage::FormTagCreate
        | DomainMessage::FormTagRemove(_)) => form::update(app, message),

        message @ (DomainMessage::HostExportRequest
        | DomainMessage::HostExportClose
        | DomainMessage::HostExportPasswordChanged(_)
        | DomainMessage::HostExportSubmit
        | DomainMessage::HostExportSaved(_)
        | DomainMessage::HostImportRequest
        | DomainMessage::HostImportPicked(_)
        | DomainMessage::HostImportPasswordChanged(_)
        | DomainMessage::HostImportPasswordSubmit
        | DomainMessage::HostImportParsed(_)
        | DomainMessage::HostImportConfirm
        | DomainMessage::HostsImported { .. }
        | DomainMessage::HostImportClose
        | DomainMessage::HostTransferTogglePasswordVisible
        | DomainMessage::HostTransferMenuToggle) => transfer::update(app, message),
    }
}

#[cfg(test)]
mod tests;
