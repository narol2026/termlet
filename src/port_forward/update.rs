//! Dispatches port-forward UI, lifecycle, and authentication messages.

// Rust guideline compliant 2026-10-09

use iced::Task;

use crate::app::App;
use crate::core::diagnostics::Level;
use crate::core::message::Message;
use crate::port_forward::message::Message as DomainMessage;

mod lifecycle;
mod ui;
mod worker;

pub(crate) use lifecycle::{begin_authentication, start_worker};
pub(crate) use lifecycle::{delete_forwards_for_host, stop_all};

/// Route one port-forward message to its owning update module.
pub fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "port_forwards::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        message @ (DomainMessage::PortForwardSearchChanged(_)
        | DomainMessage::PortForwardShowForm
        | DomainMessage::PortForwardHideForm
        | DomainMessage::PortForwardEdit(_)
        | DomainMessage::PortForwardDelete(_)
        | DomainMessage::PortForwardDeleteRequest(_)
        | DomainMessage::PortForwardDeleteCancel
        | DomainMessage::PortForwardSelect(_)
        | DomainMessage::PortForwardShowLogs(_)
        | DomainMessage::PortForwardHideLogs
        | DomainMessage::PortForwardCopyLogs(_)
        | DomainMessage::PortForwardFormName(_)
        | DomainMessage::PortForwardFormKind(_)
        | DomainMessage::PortForwardFormHost(_)
        | DomainMessage::PortForwardFormBindAddress(_)
        | DomainMessage::PortForwardFormListenPort(_)
        | DomainMessage::PortForwardFormDestHost(_)
        | DomainMessage::PortForwardFormDestPort(_)
        | DomainMessage::PortForwardFormToggleHostPicker
        | DomainMessage::PortForwardFormHostSearch(_)
        | DomainMessage::PortForwardFormSubmit
        | DomainMessage::PortForwardCreated { .. }
        | DomainMessage::PortForwardToggleView
        | DomainMessage::PortForwardDuplicate(_)
        | DomainMessage::PortForwardHoverEnter(_)
        | DomainMessage::PortForwardHoverLeave(_)
        | DomainMessage::PortForwardContextMenuOpen(_)
        | DomainMessage::PortForwardContextMenuClose
        | DomainMessage::PortForwardViewCursorMoved(_)) => ui::update(app, message),

        message @ (DomainMessage::PortForwardStart(_)
        | DomainMessage::PortForwardKeyProbed { .. }
        | DomainMessage::PortForwardKeyConfirm { .. }
        | DomainMessage::PortForwardKeyCancel { .. }
        | DomainMessage::PortForwardPasswordChanged { .. }
        | DomainMessage::PortForwardPasswordSubmit { .. }
        | DomainMessage::PortForwardPasswordCancel { .. }
        | DomainMessage::PortForwardPasswordToggleVisible { .. }
        | DomainMessage::PortForwardStop(_)
        | DomainMessage::PortForwardToggle(_)
        | DomainMessage::PortForwardStarted { .. }
        | DomainMessage::PortForwardStopped { .. }) => lifecycle::update(app, message),
    }
}

#[cfg(test)]
mod tests;
