pub mod export;
pub mod form_view;
pub mod message;
pub mod state;
pub mod transfer_view;
pub mod update;
pub mod view;

pub use message::Message;
#[doc(inline)]
pub use state::{FormState, HostsContextMenu, HostsState};

/// Identifies one row in the host form's tag picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TagPickerItem {
    Create(String),
    Existing(String),
}
