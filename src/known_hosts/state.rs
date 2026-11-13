//! Known-hosts selection, details, and dialog state.

use iced::Point;

use crate::core::context_menu::ContextMenuAction;
use crate::core::data::{Host, KnownHost};
use crate::core::grid_cursor::GridCursor;
use crate::core::list_cursor::ListCursor;

#[derive(Debug, Clone)]
pub struct KnownHostContextMenu {
    pub id: usize,
    pub position: Point,
    pub cursor: ListCursor<ContextMenuAction>,
}

/// State owned by the known-hosts workspace.
#[derive(Debug, Clone)]
pub struct KnownHostsDomain {
    pub entries: Vec<KnownHost>,
    pub search: String,
    pub grid_view: bool,
    pub hovered_id: Option<usize>,
    pub cursor: GridCursor<usize>,
    pub grid_columns: usize,
    pub delete_pending: Option<usize>,
    pub context_menu: Option<KnownHostContextMenu>,
    pub cursor_pos: Point,
    pub detail_open: bool,
    pub detail_anim: f32,
    pub detail_closing: bool,
}

impl KnownHostsDomain {
    pub fn new(grid_view: bool) -> Self {
        Self {
            entries: Vec::new(),
            search: String::new(),
            grid_view,
            hovered_id: None,
            cursor: GridCursor::new(),
            grid_columns: 1,
            delete_pending: None,
            context_menu: None,
            cursor_pos: Point::ORIGIN,
            detail_open: false,
            detail_anim: 1.0,
            detail_closing: false,
        }
    }

    pub fn filtered<'a>(&'a self, hosts: &'a [Host]) -> Vec<(&'a KnownHost, Option<&'a Host>)> {
        let query = self.search.to_lowercase();
        self.entries
            .iter()
            .map(|known_host| {
                let matched = hosts.iter().find(|host| {
                    host.hostname.eq_ignore_ascii_case(&known_host.hostname)
                        && host.port == known_host.port
                });
                (known_host, matched)
            })
            .filter(|(known_host, matched)| {
                query.is_empty()
                    || known_host.hostname.to_lowercase().contains(&query)
                    || matched.is_some_and(|host| host.name.to_lowercase().contains(&query))
            })
            .collect()
    }
}
