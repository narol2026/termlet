use iced::Point;
use secrecy::SecretString;

use crate::core::data::{IdentityId, KeyType, KeychainEntryId};

use super::state::GeneratedKeyData;

#[derive(Debug, Clone)]
pub enum Message {
    KeychainToggleNewMenu,
    KeychainShowForm,
    KeychainHideForm,
    KeychainEdit(KeychainEntryId),
    KeychainDeleteRequest(KeychainEntryId),
    KeychainDeleteCancel,
    KeychainDelete(KeychainEntryId),
    KeychainFormName(String),
    KeychainFormKeyType(KeyType),
    KeychainFormPassphrase(SecretString),
    KeychainFormTogglePasswordVisible,
    KeychainFormFingerprint(String),
    KeychainFormPublicKey(String),
    KeychainFormPath(String),
    KeychainFormSubmit,
    KeychainFormGenerated(Result<GeneratedKeyData, String>),
    KeychainEntryCreated {
        entry: crate::core::data::KeychainEntry,
    },
    KeychainSearchChanged(String),
    KeychainToggleView,
    KeychainSelect(KeychainEntryId),
    KeychainHoverEnter(KeychainEntryId),
    KeychainHoverLeave(KeychainEntryId),
    KeychainContextMenuOpen(KeychainEntryId),
    KeychainContextMenuClose,
    KeychainViewCursorMoved(Point),
    IdentityShowForm,
    IdentityHideForm,
    IdentityEdit(IdentityId),
    IdentityDeleteRequest(IdentityId),
    IdentityDeleteCancel,
    IdentityDelete(IdentityId),
    IdentityFormLabel(String),
    IdentityFormUsername(String),
    IdentityFormPassword(SecretString),
    IdentityFormTogglePasswordVisible,
    IdentityFormSubmit,
    IdentityCreated {
        identity: crate::core::data::Identity,
    },
    IdentityHoverEnter(IdentityId),
    IdentitySelect(IdentityId),
    IdentityHoverLeave(IdentityId),
    IdentityContextMenuOpen(IdentityId),
    IdentityContextMenuClose,
}
