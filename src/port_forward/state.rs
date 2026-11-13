//! Port-forward forms, runtime summaries, and selection state.

use std::collections::HashMap;

use iced::Point;

use crate::core::context_menu::ContextMenuAction;
use crate::core::data::{HostId, PortForward, PortForwardId, PortForwardKind};
use crate::core::dropdown::DropdownState;
use crate::core::forms::FormDraft;
use crate::core::grid_cursor::GridCursor;
use crate::core::list_cursor::ListCursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortForwardStatus {
    Stopped,
    Starting,
    Running,
    /// Disconnect has been requested; the worker is still shutting down.
    Stopping,
    Failed,
}

pub struct PortForwardRuntime {
    pub status: PortForwardStatus,
    pub logs: Vec<String>,
}

impl Default for PortForwardRuntime {
    fn default() -> Self {
        Self {
            status: PortForwardStatus::Stopped,
            logs: Vec::new(),
        }
    }
}

pub struct PortForwardFormState {
    pub visible: bool,
    pub editing_id: Option<PortForwardId>,
    pub name: String,
    pub kind: PortForwardKind,
    pub host_id: Option<HostId>,
    pub bind_address: String,
    pub listen_port: String,
    pub dest_host: String,
    pub dest_port: String,
    pub host_picker: DropdownState<HostId>,
    /// Snapshot of the port forward being edited; `None` while creating.
    pub original: Option<PortForward>,
}

impl PortForwardFormState {
    pub fn reset(&mut self) {
        self.editing_id = None;
        self.name.clear();
        self.kind = PortForwardKind::Local;
        self.host_id = None;
        self.bind_address = "127.0.0.1".into();
        self.listen_port = "8080".into();
        self.dest_host = "127.0.0.1".into();
        self.dest_port = "80".into();
        self.host_picker.reset();
        self.original = None;
    }
}

impl FormDraft for PortForwardFormState {
    fn is_complete(&self) -> bool {
        let needs_dest = self.kind != PortForwardKind::Dynamic;
        self.host_id.is_some()
            && self.listen_port.trim().parse::<u16>().is_ok()
            && (!needs_dest
                || (!self.dest_host.trim().is_empty()
                    && self.dest_port.trim().parse::<u16>().is_ok()))
    }

    fn is_dirty(&self) -> bool {
        let Some(orig) = &self.original else {
            return true;
        };
        self.name.trim() != orig.name
            || self.kind != orig.kind
            || self.host_id != Some(orig.host_id)
            || self.bind_address.trim() != orig.bind_address
            || self.listen_port.trim().parse::<u16>().ok() != Some(orig.listen_port)
            || self.dest_host.trim() != orig.dest_host
            || self.dest_port.trim().parse::<u16>().ok() != Some(orig.dest_port)
    }
}

/// State owned by the port-forward workspace.
pub struct PortForwardState {
    pub(crate) trust: super::trust::TrustState,
    pub(crate) password: super::password::PasswordState,
    pub list: Vec<PortForward>,
    pub runtime: HashMap<PortForwardId, PortForwardRuntime>,
    pub cursor: GridCursor<PortForwardId>,
    pub grid_columns: usize,
    pub form: PortForwardFormState,
    pub logs_open: bool,
    pub delete_pending: Option<PortForwardId>,
    pub grid_view: bool,
    pub hovered_id: Option<PortForwardId>,
    pub context_menu: Option<PortForwardContextMenu>,
    pub cursor_pos: Point,
    pub search_query: String,
    pub form_anim: f32,
    pub form_closing: bool,
}

impl PortForwardState {
    pub fn runtime_for(&self, id: PortForwardId) -> Option<&PortForwardRuntime> {
        self.runtime.get(&id)
    }

    pub fn status_for(&self, id: PortForwardId) -> PortForwardStatus {
        self.runtime_for(id)
            .map_or(PortForwardStatus::Stopped, |r| r.status)
    }

    pub fn filtered(&self) -> Vec<&PortForward> {
        let q = self.search_query.to_lowercase();
        self.list
            .iter()
            .filter(|f| {
                q.is_empty()
                    || f.name.to_lowercase().contains(&q)
                    || f.bind_address.to_lowercase().contains(&q)
                    || f.dest_host.to_lowercase().contains(&q)
                    || f.human_summary().to_lowercase().contains(&q)
            })
            .collect()
    }

    #[allow(
        dead_code,
        reason = "unused accessor; kept beside the other session queries"
    )]
    pub fn has_running(&self) -> bool {
        self.runtime.values().any(|r| {
            matches!(
                r.status,
                PortForwardStatus::Running | PortForwardStatus::Starting
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct PortForwardContextMenu {
    pub forward_id: PortForwardId,
    pub position: Point,
    pub cursor: ListCursor<ContextMenuAction>,
}
