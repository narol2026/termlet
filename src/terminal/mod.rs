pub mod frequency;
pub mod history;
pub mod message;
pub(crate) mod split;
pub mod split_layout;
pub mod state;
pub mod update;
pub mod view;

pub use message::Message;
pub(crate) use state::supports_osc_shell_integration;
#[doc(inline)]
pub use state::{
    Draft, DragPreview, PendingLockedClose, SidebarState, Split, SpotlightState, SshSessionSecrets,
    Tab, TabKind, TerminalCloseTarget, TerminalContextMenu, TerminalDomain, TerminalSession,
};
