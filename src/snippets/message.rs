use iced::Point;

use crate::core::data::{Snippet, SnippetId, SnippetPackage, SnippetPackageId};

#[derive(Debug, Clone)]
// Variants carry the module prefix (iced message-enum convention) so they stay
// unambiguous once composed into the app-wide `Message`.
#[allow(
    clippy::enum_variant_names,
    reason = "variant names follow the message kinds in the protocol"
)]
pub enum Message {
    SnippetShowForm,
    SnippetHideForm,
    SnippetEdit(SnippetId),
    SnippetDeleteRequest(SnippetId),
    SnippetDeleteCancel,
    SnippetDelete(SnippetId),
    SnippetFormDescription(String),
    SnippetFormScript(iced::widget::text_editor::Action),
    SnippetFormPackage(Option<SnippetPackageId>),
    SnippetFormTogglePackagePicker,
    SnippetFormSubmit,
    SnippetCopyScript(SnippetId),
    SnippetRunScript(SnippetId),
    SnippetPasteScript(SnippetId),
    SnippetRightSidebarFormDescription(String),
    SnippetRightSidebarFormScript(iced::widget::text_editor::Action),
    SnippetRightSidebarFormPackage(Option<SnippetPackageId>),
    SnippetRightSidebarTogglePackagePicker,
    SnippetRightSidebarFormSubmit,
    SnippetCreated { snippet: Snippet },
    SnippetRightSidebarShowAdd,
    SnippetRightSidebarCancelAdd,
    SnippetRightSidebarEdit(SnippetId),
    SnippetPackageShowForm,
    SnippetPackageHideForm,
    SnippetPackageEdit(SnippetPackageId),
    SnippetPackageDeleteRequest(SnippetPackageId),
    SnippetPackageDeleteCancel,
    SnippetPackageDelete(SnippetPackageId),
    SnippetPackageFormName(String),
    SnippetPackageFormDescription(String),
    SnippetPackageFormSubmit,
    SnippetPackageCreated { package: SnippetPackage },
    SnippetToggleNewMenu,
    SnippetToggleView,
    SnippetSelect(SnippetId),
    SnippetPackageSelect(SnippetPackageId),
    SnippetHoverEnter(SnippetId),
    SnippetHoverLeave(SnippetId),
    SnippetContextMenuOpen(SnippetId),
    SnippetPackageContextMenuOpen(SnippetPackageId),
    SnippetContextMenuClose,
    SnippetViewCursorMoved(Point),
    SnippetPackageHoverEnter(SnippetPackageId),
    SnippetPackageHoverLeave(SnippetPackageId),
}
