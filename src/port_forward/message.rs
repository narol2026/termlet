use iced::Point;
use secrecy::SecretString;

use crate::core::data::{HostId, PortForward, PortForwardId, PortForwardKind, ServerKeyInfo};

#[derive(Debug, Clone)]
// Variants carry the module prefix (iced message-enum convention) so they stay
// unambiguous once composed into the app-wide `Message`.
#[allow(
    clippy::enum_variant_names,
    reason = "variant names follow the message kinds in the protocol"
)]
pub enum Message {
    PortForwardShowForm,
    PortForwardHideForm,
    PortForwardEdit(PortForwardId),
    PortForwardDelete(PortForwardId),
    PortForwardDeleteRequest(PortForwardId),
    PortForwardDeleteCancel,
    PortForwardSelect(PortForwardId),
    PortForwardStart(PortForwardId),
    PortForwardKeyProbed {
        id: PortForwardId,
        request: u64,
        result: Result<ServerKeyInfo, String>,
    },
    PortForwardKeyConfirm {
        id: PortForwardId,
        request: u64,
        save: bool,
    },
    PortForwardKeyCancel {
        id: PortForwardId,
        request: u64,
    },
    PortForwardPasswordChanged {
        id: PortForwardId,
        request: u64,
        value: SecretString,
    },
    PortForwardPasswordSubmit {
        id: PortForwardId,
        request: u64,
    },
    PortForwardPasswordCancel {
        id: PortForwardId,
        request: u64,
    },
    PortForwardPasswordToggleVisible {
        id: PortForwardId,
        request: u64,
    },
    PortForwardStop(PortForwardId),
    PortForwardToggle(PortForwardId),
    PortForwardStarted {
        id: PortForwardId,
        success: bool,
        logs: Vec<String>,
    },
    PortForwardStopped {
        id: PortForwardId,
        generation: u64,
        logs: Vec<String>,
    },
    PortForwardShowLogs(PortForwardId),
    PortForwardHideLogs,
    PortForwardCopyLogs(PortForwardId),
    PortForwardFormName(String),
    PortForwardFormKind(PortForwardKind),
    PortForwardFormHost(HostId),
    PortForwardFormBindAddress(String),
    PortForwardFormListenPort(String),
    PortForwardFormDestHost(String),
    PortForwardFormDestPort(String),
    PortForwardFormToggleHostPicker,
    PortForwardFormHostSearch(String),
    PortForwardFormSubmit,
    PortForwardCreated {
        forward: PortForward,
    },
    PortForwardToggleView,
    PortForwardDuplicate(PortForwardId),
    PortForwardHoverEnter(PortForwardId),
    PortForwardHoverLeave(PortForwardId),
    PortForwardContextMenuOpen(PortForwardId),
    PortForwardContextMenuClose,
    PortForwardViewCursorMoved(Point),
    PortForwardSearchChanged(String),
}
