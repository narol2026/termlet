//! Key and identity forms, selection, and dialog state.

use iced::Point;
use secrecy::{ExposeSecret, SecretString};

use crate::core::context_menu::ContextMenuAction;
use crate::core::data::{Identity, IdentityId, KeyType, KeychainEntry, KeychainEntryId};
use crate::core::forms::FormDraft;
use crate::core::grid_cursor::GridCursor;
use crate::core::list_cursor::ListCursor;

pub struct KeychainFormState {
    pub visible: bool,
    pub editing_id: Option<KeychainEntryId>,
    pub name: String,
    pub key_type: String,
    pub selected_key_type: Option<KeyType>,
    pub passphrase: SecretString,
    pub passphrase_visible: bool,
    pub fingerprint: String,
    pub public_key: String,
    pub path: String,
    /// Whether the path was manually edited.
    ///
    /// When `false`, the path is kept in sync with the name/key-type
    /// auto-suggestion.
    pub path_manual: bool,
    /// Whether key generation is running off the UI thread.
    pub in_progress: bool,
    /// Snapshot of the entry being edited; `None` while generating.
    pub original: Option<KeychainEntry>,
}

impl KeychainFormState {
    pub fn reset(&mut self) {
        self.editing_id = None;
        self.name.clear();
        self.key_type.clear();
        self.selected_key_type = None;
        self.passphrase = SecretString::from(String::new());
        self.passphrase_visible = false;
        self.fingerprint.clear();
        self.public_key.clear();
        self.path.clear();
        self.path_manual = false;
        self.in_progress = false;
        self.original = None;
    }
}

impl FormDraft for KeychainFormState {
    fn is_complete(&self) -> bool {
        if self.editing_id.is_some() {
            !self.name.trim().is_empty() && !self.key_type.trim().is_empty()
        } else {
            !self.name.trim().is_empty()
                && !self.path.trim().is_empty()
                && self.selected_key_type.is_some()
        }
    }

    fn is_dirty(&self) -> bool {
        let Some(orig) = &self.original else {
            return true;
        };
        self.name != orig.name || self.key_type != orig.key_type
    }
}

/// Result of a background key-generation run.
///
/// The blocking task returns this so the UI thread can build the
/// [`KeychainEntry`] once generation completes.
#[derive(Debug, Clone)]
pub struct GeneratedKeyData {
    pub key_type_display: String,
    pub kind_label: String,
    pub name: String,
    pub path: String,
    pub fingerprint: String,
    pub public_key: String,
}

pub struct IdentityFormState {
    pub visible: bool,
    pub editing_id: Option<IdentityId>,
    pub label: String,
    pub username: String,
    pub password: SecretString,
    pub password_visible: bool,
    /// Snapshot of the identity being edited; `None` while creating.
    pub original: Option<Identity>,
}

impl IdentityFormState {
    pub fn reset(&mut self) {
        self.editing_id = None;
        self.label.clear();
        self.username.clear();
        self.password = SecretString::from(String::new());
        self.password_visible = false;
        self.original = None;
    }
}

impl FormDraft for IdentityFormState {
    fn is_complete(&self) -> bool {
        !self.label.trim().is_empty() && !self.password.expose_secret().is_empty()
    }

    fn is_dirty(&self) -> bool {
        let Some(orig) = &self.original else {
            return true;
        };
        self.label != orig.label
            || self.username != orig.username
            || self.password.expose_secret() != orig.password_for_edit()
    }
}

#[derive(Debug, Clone)]
pub enum KeychainContextMenu {
    Key {
        id: KeychainEntryId,
        position: Point,
        cursor: ListCursor<ContextMenuAction>,
    },
    Identity {
        id: IdentityId,
        position: Point,
        cursor: ListCursor<ContextMenuAction>,
    },
}

/// State owned by the keychain workspace.
pub struct KeychainState {
    pub list: Vec<KeychainEntry>,
    pub form: KeychainFormState,
    pub delete_pending: Option<KeychainEntryId>,
    pub grid_view: bool,
    pub hovered_id: Option<KeychainEntryId>,
    pub cursor: GridCursor<KeychainSelection>,
    pub grid_columns: usize,
    pub search_query: String,
    pub context_menu: Option<KeychainContextMenu>,
    pub cursor_pos: Point,
    pub form_anim: f32,
    pub form_closing: bool,
    pub show_new_menu: bool,
    pub identities: Vec<Identity>,
    pub identity_form: IdentityFormState,
    pub identity_delete_pending: Option<IdentityId>,
    pub hovered_identity_id: Option<IdentityId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainSelection {
    Key(KeychainEntryId),
    Identity(IdentityId),
}

impl KeychainState {
    pub fn filtered_keys(&self) -> Vec<&KeychainEntry> {
        let query = self.search_query.to_lowercase();
        self.list
            .iter()
            .filter(|entry| {
                query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.key_type.to_lowercase().contains(&query)
                    || entry.fingerprint.to_lowercase().contains(&query)
            })
            .collect()
    }

    pub fn select_key(&mut self, id: KeychainEntryId) {
        self.cursor.select(KeychainSelection::Key(id));
    }

    pub fn select_identity(&mut self, id: IdentityId) {
        self.cursor.select(KeychainSelection::Identity(id));
    }

    pub fn selected_key(&self) -> Option<KeychainEntryId> {
        match self.cursor.selected_key() {
            Some(KeychainSelection::Key(id)) => Some(*id),
            Some(KeychainSelection::Identity(_)) | None => None,
        }
    }

    pub fn selected_identity(&self) -> Option<IdentityId> {
        match self.cursor.selected_key() {
            Some(KeychainSelection::Identity(id)) => Some(*id),
            Some(KeychainSelection::Key(_)) | None => None,
        }
    }
}
