use iced::Point;

#[derive(Debug, Clone)]
// Variants carry the module prefix (iced message-enum convention) so they stay
// unambiguous once composed into the app-wide `Message`.
#[allow(
    clippy::enum_variant_names,
    reason = "variant names follow the message kinds in the protocol"
)]
pub enum Message {
    KnownHostsLoaded(Result<Vec<crate::core::data::KnownHost>, String>),
    KnownHostDeleteRequest(usize),
    KnownHostDeleteCancel,
    KnownHostDelete(usize),
    KnownHostSearchChanged(String),
    KnownHostToggleView,
    KnownHostSelect(usize),
    KnownHostHoverEnter(usize),
    KnownHostHoverLeave(usize),
    KnownHostContextMenuOpen(usize),
    KnownHostContextMenuClose,
    KnownHostViewCursorMoved(Point),
    KnownHostShowDetails(usize),
    KnownHostHideDetails,
}
