//! Host vault form, selection, and transfer-dialog state.

use iced::Point;
use secrecy::{ExposeSecret, SecretString};

use super::TagPickerItem;
use super::export::ExportHost;
use crate::core::context_menu::ContextMenuAction;
use crate::core::data::{AuthType, Host, HostId, IdentityId, KeychainEntryId};
use crate::core::dropdown::{DropdownState, TransferMenuAction};
use crate::core::forms::FormDraft;
use crate::core::grid_cursor::GridCursor;
use crate::core::list_cursor::ListCursor;

/// State owned by the host-vault workspace.
pub struct HostsState {
    pub list: Vec<Host>,
    pub form: FormState,
    pub form_anim: f32,
    pub form_closing: bool,
    pub hovered_id: Option<HostId>,
    /// Selection in the vault list. Storing the host id keeps the highlight on
    /// the same host when filtering or recency ordering changes its position.
    pub cursor: GridCursor<HostId>,
    /// Columns in the current grid layout, measured by the view. Arrow keys
    /// use this to move a whole row at a time.
    pub grid_columns: usize,
    pub search_query: String,
    pub recent_ids: Vec<HostId>,
    pub grid_view: bool,
    pub delete_pending: Option<HostId>,
    pub context_menu: Option<HostsContextMenu>,
    pub cursor_pos: Point,
    /// Export/import dialog (`None` = closed).
    pub transfer_dialog: Option<HostTransferDialog>,
    pub transfer_menu: DropdownState<TransferMenuAction>,
    /// Active tag filter on the vaults host list (`All` = no filtering).
    pub tag_filter: crate::core::data::HostTagFilter,
    pub tag_filter_picker: DropdownState<crate::core::data::HostTagFilter>,
}

impl Default for HostsState {
    fn default() -> Self {
        Self {
            list: Vec::new(),
            form: FormState::default(),
            form_anim: 1.0,
            form_closing: false,
            hovered_id: None,
            cursor: GridCursor::new(),
            grid_columns: 1,
            search_query: String::new(),
            recent_ids: Vec::new(),
            grid_view: false,
            delete_pending: None,
            context_menu: None,
            cursor_pos: Point::ORIGIN,
            transfer_dialog: None,
            transfer_menu: DropdownState::new(),
            tag_filter: crate::core::data::HostTagFilter::All,
            tag_filter_picker: DropdownState::new(),
        }
    }
}

impl HostsState {
    pub fn filtered(&self) -> Vec<&Host> {
        let q = self.search_query.to_lowercase();
        let mut hosts: Vec<&Host> = self
            .list
            .iter()
            .filter(|h| {
                self.tag_filter.matches(h)
                    && (q.is_empty()
                        || h.name.to_lowercase().contains(&q)
                        || h.hostname.to_lowercase().contains(&q)
                        || h.username.to_lowercase().contains(&q))
            })
            .collect();
        self.sort_by_recency(&mut hosts);
        hosts
    }

    /// Distinct tag names used across all hosts, sorted case-insensitively.
    ///
    /// Used by the toolbar tag filter and the form's tag suggestions.
    pub fn all_tags(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for host in &self.list {
            for tag in &host.tags {
                if tag.is_empty() || seen.iter().any(|t| t == tag) {
                    continue;
                }
                seen.push(tag.clone());
            }
        }
        seen.sort_by_key(|a| a.to_lowercase());
        seen
    }

    /// Sort hosts so recently connected ones appear first.
    pub fn sort_by_recency(&self, hosts: &mut Vec<&Host>) {
        hosts.sort_by(|a, b| {
            let a_idx = self.recent_ids.iter().position(|id| *id == a.id);
            let b_idx = self.recent_ids.iter().position(|id| *id == b.id);
            match (a_idx, b_idx) {
                (Some(i), Some(j)) => i.cmp(&j),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
    }

    /// Return all hosts sorted so recently connected ones appear first.
    pub fn sorted_by_recency(&self) -> Vec<&Host> {
        let mut hosts: Vec<&Host> = self.list.iter().collect();
        self.sort_by_recency(&mut hosts);
        hosts
    }

    pub fn recent(&self) -> Vec<&Host> {
        self.recent_ids
            .iter()
            .filter_map(|id| self.list.iter().find(|h| h.id == *id))
            .collect()
    }

    pub fn record_recent(&mut self, host_id: HostId) {
        self.recent_ids.retain(|id| *id != host_id);
        self.recent_ids.insert(0, host_id);
        self.recent_ids.truncate(8);
    }
}

#[derive(Debug, Clone)]
pub struct HostsContextMenu {
    pub host_id: HostId,
    pub position: Point,
    pub cursor: ListCursor<ContextMenuAction>,
}

/// Which transfer dialog is currently shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTransferKind {
    Export,
    /// Import: asking for the decryption passphrase.
    ImportPassword,
    /// Import: parsed file, asking for confirmation.
    ImportConfirm,
}

/// State for the host export/import dialogs.
///
/// `None` on [`HostsState`] means closed.
pub struct HostTransferDialog {
    pub kind: HostTransferKind,
    pub password: SecretString,
    pub password_visible: bool,
    /// Guards against double-submit while an async task is running.
    pub busy: bool,
    /// Shimmer animation phase while `busy`; advanced by `AnimTick`.
    pub anim_t: f32,
    pub pending_path: Option<std::path::PathBuf>,
    /// Import: raw encrypted bytes waiting for a passphrase.
    pub pending_bytes: Vec<u8>,
    pub pending_hosts: Vec<ExportHost>,
    pub new_count: usize,
    pub dup_count: usize,
    pub error: Option<String>,
}

impl HostTransferDialog {
    pub fn new_export() -> Self {
        Self {
            kind: HostTransferKind::Export,
            password: SecretString::from(String::new()),
            password_visible: false,
            busy: false,
            anim_t: 0.0,
            pending_path: None,
            pending_bytes: Vec::new(),
            pending_hosts: Vec::new(),
            new_count: 0,
            dup_count: 0,
            error: None,
        }
    }

    pub fn new_import_password(path: std::path::PathBuf, bytes: Vec<u8>) -> Self {
        Self {
            kind: HostTransferKind::ImportPassword,
            password: SecretString::from(String::new()),
            password_visible: false,
            busy: false,
            anim_t: 0.0,
            pending_path: Some(path),
            pending_bytes: bytes,
            pending_hosts: Vec::new(),
            new_count: 0,
            dup_count: 0,
            error: None,
        }
    }

    pub fn new_import_confirm(
        pending_hosts: Vec<ExportHost>,
        new_count: usize,
        dup_count: usize,
    ) -> Self {
        Self {
            kind: HostTransferKind::ImportConfirm,
            password: SecretString::from(String::new()),
            password_visible: false,
            busy: false,
            anim_t: 0.0,
            pending_path: None,
            pending_bytes: Vec::new(),
            pending_hosts,
            new_count,
            dup_count,
            error: None,
        }
    }

    /// Whether the dialog's primary confirm button is currently actionable.
    ///
    /// Mirrors the button's disabled state and drives the global Enter binding.
    pub fn confirm_enabled(&self) -> bool {
        if self.busy {
            return false;
        }
        match self.kind {
            HostTransferKind::Export | HostTransferKind::ImportPassword => {
                !self.password.expose_secret().trim().is_empty()
            }
            HostTransferKind::ImportConfirm => true,
        }
    }
}

/// Editable state for the host form.
pub struct FormState {
    pub visible: bool,
    pub editing_id: Option<HostId>,
    pub name: String,
    pub hostname: String,
    pub port: String,
    pub username: String,
    pub auth: AuthType,
    /// Saved host selected as the `ProxyJump` target. Stored on `Host` as
    /// `jump_host_id`; the form keeps only the id, not a second copy of the
    /// jump host's credentials.
    pub jump_host_id: Option<HostId>,
    pub jump_picker: DropdownState<HostId>,
    pub key_path: String,
    pub password: SecretString,
    pub password_visible: bool,
    pub identity_picker: DropdownState<IdentityId>,
    pub selected_identity_id: Option<IdentityId>,
    /// Whether the keychain key picker dropdown is open.
    pub keychain_picker: DropdownState<KeychainEntryId>,
    pub tags: Vec<String>,
    pub tag_picker: DropdownState<TagPickerItem>,
    /// Snapshot of the host being edited, taken when the form opened.
    ///
    /// `None` while creating a new host. Used for [`FormDraft::is_dirty`].
    pub original: Option<Host>,
}

impl Default for FormState {
    fn default() -> Self {
        Self {
            visible: false,
            editing_id: None,
            name: String::new(),
            hostname: String::new(),
            port: "22".into(),
            username: String::new(),
            auth: AuthType::Password,
            jump_host_id: None,
            jump_picker: DropdownState::new(),
            key_path: String::new(),
            password: SecretString::from(String::new()),
            password_visible: false,
            identity_picker: DropdownState::new(),
            selected_identity_id: None,
            keychain_picker: DropdownState::new(),
            tags: Vec::new(),
            tag_picker: DropdownState::new(),
            original: None,
        }
    }
}

impl FormState {
    pub fn reset(&mut self) {
        self.name.clear();
        self.hostname.clear();
        self.port = "22".into();
        self.username.clear();
        self.auth = AuthType::Password;
        self.jump_host_id = None;
        self.jump_picker.reset();
        self.key_path.clear();
        self.password = SecretString::from(String::new());
        self.password_visible = false;
        self.editing_id = None;
        self.identity_picker.reset();
        self.selected_identity_id = None;
        self.keychain_picker.reset();
        self.tags.clear();
        self.tag_picker.reset();
        self.original = None;
    }
}

impl FormDraft for FormState {
    fn is_complete(&self) -> bool {
        let port_valid = !self.port.is_empty() && self.port.parse::<u16>().is_ok_and(|p| p >= 1);
        let is_new = self.editing_id.is_none();
        !self.hostname.trim().is_empty()
            && !self.username.trim().is_empty()
            && port_valid
            && (!is_new || self.auth != AuthType::Key || !self.key_path.is_empty())
            && (!is_new
                || self.auth != AuthType::Password
                || !self.password.expose_secret().is_empty())
    }

    fn is_dirty(&self) -> bool {
        let Some(orig) = &self.original else {
            return true;
        };
        // key_path/password only persist under their matching auth type, so a
        // field hidden by the current auth type must not count as a change.
        let saved_key_path = if self.auth == AuthType::Key {
            self.key_path.as_str()
        } else {
            ""
        };
        let saved_password = if self.auth == AuthType::Password {
            self.password.expose_secret()
        } else {
            ""
        };
        self.name != orig.name
            || self.hostname != orig.hostname
            || self.port.parse::<u16>().unwrap_or(22) != orig.port
            || self.username != orig.username
            || self.auth != orig.auth_type
            || self.jump_host_id != orig.jump_host_id
            || self.selected_identity_id != orig.identity_id
            || self.tags != orig.tags
            || saved_key_path != orig.key_path.as_deref().unwrap_or("")
            || saved_password != orig.password_for_edit()
    }
}
