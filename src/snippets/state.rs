//! Snippet and package forms, selection, and dialog state.

use iced::Point;
use iced::widget::text_editor;

use crate::core::context_menu::ContextMenuAction;
use crate::core::data::{Snippet, SnippetId, SnippetPackage, SnippetPackageId};
use crate::core::dropdown::DropdownState;
use crate::core::forms::FormDraft;
use crate::core::grid_cursor::GridCursor;
use crate::core::list_cursor::ListCursor;

pub struct SnippetFormState {
    pub visible: bool,
    pub editing_id: Option<SnippetId>,
    pub description: String,
    pub script: text_editor::Content,
    pub package_id: Option<SnippetPackageId>,
    pub package_picker: DropdownState<SnippetPackageId>,
    /// Snapshot of the snippet being edited; `None` while creating.
    pub original: Option<Snippet>,
}

impl SnippetFormState {
    pub fn reset(&mut self) {
        self.editing_id = None;
        self.description.clear();
        self.script = text_editor::Content::new();
        self.package_id = None;
        self.package_picker.reset();
        self.original = None;
    }
}

impl FormDraft for SnippetFormState {
    fn is_complete(&self) -> bool {
        !self.description.trim().is_empty() && !self.script.text().trim().is_empty()
    }

    fn is_dirty(&self) -> bool {
        let Some(orig) = &self.original else {
            return true;
        };
        self.description.trim() != orig.description.as_str()
            || self.script.text().trim() != orig.script.as_str()
            || self.package_id != orig.package_id
    }
}

pub struct SnippetPackageFormState {
    pub visible: bool,
    pub editing_id: Option<SnippetPackageId>,
    pub name: String,
    pub description: String,
    /// Snapshot of the package being edited; `None` while creating.
    pub original: Option<SnippetPackage>,
}

impl SnippetPackageFormState {
    pub fn reset(&mut self) {
        self.editing_id = None;
        self.name.clear();
        self.description.clear();
        self.original = None;
    }
}

impl FormDraft for SnippetPackageFormState {
    fn is_complete(&self) -> bool {
        !self.name.trim().is_empty()
    }

    fn is_dirty(&self) -> bool {
        let Some(orig) = &self.original else {
            return true;
        };
        self.name.trim() != orig.name.as_str()
            || self.description.trim() != orig.description.as_str()
    }
}

#[derive(Debug, Clone)]
pub enum SnippetContextMenu {
    Snippet {
        id: SnippetId,
        position: Point,
        cursor: ListCursor<ContextMenuAction>,
    },
    Package {
        id: SnippetPackageId,
        position: Point,
        cursor: ListCursor<ContextMenuAction>,
    },
}

/// State owned by the snippets workspace.
pub struct SnippetsState {
    pub list: Vec<Snippet>,
    pub form: SnippetFormState,
    pub delete_pending: Option<SnippetId>,
    pub right_show_form: bool,
    pub right_editing_id: Option<SnippetId>,
    pub right_description: String,
    pub right_script: text_editor::Content,
    pub right_package_id: Option<SnippetPackageId>,
    pub right_package_picker: DropdownState<SnippetPackageId>,
    pub packages: Vec<SnippetPackage>,
    pub package_form: SnippetPackageFormState,
    pub package_delete_pending: Option<SnippetPackageId>,
    pub show_new_menu: bool,
    pub grid_view: bool,
    pub hovered_package_id: Option<SnippetPackageId>,
    pub hovered_snippet_id: Option<SnippetId>,
    pub cursor: GridCursor<SnippetSelection>,
    pub grid_columns: usize,
    pub context_menu: Option<SnippetContextMenu>,
    pub cursor_pos: Point,
    pub form_anim: f32,
    pub form_closing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnippetSelection {
    Snippet(SnippetId),
    Package(SnippetPackageId),
}

impl SnippetsState {
    pub fn selected_snippet(&self) -> Option<SnippetId> {
        match self.cursor.selected_key() {
            Some(SnippetSelection::Snippet(id)) => Some(*id),
            Some(SnippetSelection::Package(_)) | None => None,
        }
    }

    pub fn selected_package(&self) -> Option<SnippetPackageId> {
        match self.cursor.selected_key() {
            Some(SnippetSelection::Package(id)) => Some(*id),
            Some(SnippetSelection::Snippet(_)) | None => None,
        }
    }
}
