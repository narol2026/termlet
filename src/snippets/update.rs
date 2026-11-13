use iced::Task;

use super::message::Message as DomainMessage;
use crate::app::App;
use crate::core::data::{LogCategory, Snippet, SnippetId, SnippetPackage, SnippetPackageId};
use crate::core::diagnostics::Level;
use crate::core::forms::FormDraft;
use crate::core::message::Message;

use crate::update::restore_scrolls;
use rust_i18n::t;

use super::state::{SnippetContextMenu, SnippetSelection};

pub fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "snippets::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        DomainMessage::SnippetShowForm => {
            app.snippets.show_new_menu = false;
            app.snippets.cursor.clear();
            let was_visible = (app.snippets.form.visible || app.snippets.package_form.visible)
                && !app.snippets.form_closing;
            app.snippets.form.reset();
            app.snippets.form.visible = true;
            app.snippets.form_anim = if was_visible { 1.0 } else { 0.0 };
            app.snippets.form_closing = false;
            app.snippets.package_form.visible = false;
            app.needs_scroll_restore = true;
        }
        DomainMessage::SnippetHideForm => {
            crate::cursor::close_active_dropdown(app);
            app.snippets.form_closing = true;
        }
        DomainMessage::SnippetEdit(id) => {
            app.snippets.context_menu = None;
            open_snippet_editor(app, id);
        }
        DomainMessage::SnippetDeleteRequest(id) => {
            app.snippets.delete_pending = Some(id);
            app.snippets.context_menu = None;
            return restore_scrolls(app);
        }
        DomainMessage::SnippetDeleteCancel => {
            app.snippets.delete_pending = None;
            return restore_scrolls(app);
        }
        DomainMessage::SnippetDelete(id) => {
            let desc = app
                .snippets
                .list
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.description.clone())
                .unwrap_or_default();
            app.snippets.list.retain(|s| s.id != id);
            if app.snippets.cursor.selected_key() == Some(&SnippetSelection::Snippet(id)) {
                app.snippets.cursor.clear();
            }
            app.snippets.delete_pending = None;
            if app.snippets.form.editing_id == Some(id) {
                app.snippets.form.visible = false;
                app.snippets.form.reset();
                app.needs_scroll_restore = true;
            }
            if app.snippets.right_editing_id == Some(id) {
                app.snippets.right_editing_id = None;
                app.snippets.right_show_form = false;
                app.snippets.right_description.clear();
                app.snippets.right_script = iced::widget::text_editor::Content::new();
                app.snippets.right_package_id = None;
            }
            app.add_log(LogCategory::Snippet, format!("Deleted snippet \"{desc}\""));
            let save = app.storage_task(
                move |s| s.delete_snippet(id),
                move |result| match result {
                    Ok(()) => Message::Noop,
                    Err(e) => Message::ShowErrorToast(e),
                },
            );
            return Task::batch([save, restore_scrolls(app)]);
        }
        DomainMessage::SnippetFormDescription(v) => app.snippets.form.description = v,
        DomainMessage::SnippetFormScript(action) => {
            app.snippets.form.script.perform(action);
        }
        DomainMessage::SnippetFormPackage(id) => {
            app.snippets.form.package_id = id;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SnippetFormPackage,
            );
        }
        DomainMessage::SnippetFormTogglePackagePicker => {
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SnippetFormPackage,
            );
        }
        DomainMessage::SnippetFormSubmit => {
            return submit_form(app);
        }
        DomainMessage::SnippetCopyScript(id) => {
            activate_snippet(app, id);
            if let Some(snippet) = app.snippets.list.iter().find(|s| s.id == id) {
                let script = snippet.script.clone();
                return Task::batch([
                    iced::clipboard::write(script),
                    Task::done(Message::ShowToast(t!("snippets.script_copied").to_string())),
                ]);
            }
        }
        DomainMessage::SnippetRunScript(id) => {
            return run_in_terminal(app, id, true);
        }
        DomainMessage::SnippetPasteScript(id) => {
            return paste_to_terminal(app, id);
        }
        DomainMessage::SnippetRightSidebarFormDescription(v) => {
            app.snippets.right_description = v;
        }
        DomainMessage::SnippetRightSidebarFormScript(action) => {
            app.snippets.right_script.perform(action);
        }
        DomainMessage::SnippetRightSidebarFormPackage(id) => {
            app.snippets.right_package_id = id;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SnippetSidebarPackage,
            );
        }
        DomainMessage::SnippetRightSidebarTogglePackagePicker => {
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::SnippetSidebarPackage,
            );
        }
        DomainMessage::SnippetRightSidebarFormSubmit => {
            let desc = app.snippets.right_description.trim().to_string();
            let script = app.snippets.right_script.text().trim().to_string();
            if !desc.is_empty() && !script.is_empty() {
                if let Some(editing_id) = app.snippets.right_editing_id {
                    if let Some(snippet) = app.snippets.list.iter_mut().find(|s| s.id == editing_id)
                    {
                        snippet.description.clone_from(&desc);
                        snippet.script = script;
                        snippet.package_id = app.snippets.right_package_id;
                        snippet.last_used_at = now_millis();
                        let snippet = snippet.clone();
                        sort_snippets(app);
                        app.snippets.right_editing_id = None;
                        app.snippets.right_description.clear();
                        app.snippets.right_script = iced::widget::text_editor::Content::new();
                        app.snippets.right_package_id = None;
                        app.snippets.right_show_form = false;
                        return app.storage_task(
                            move |s| s.upsert_snippet(&snippet),
                            move |result| match result {
                                Ok(()) => {
                                    Message::ShowToast(t!("snippets.snippet_updated").to_string())
                                }
                                Err(e) => Message::ShowErrorToast(e),
                            },
                        );
                    }
                } else {
                    let package_id = app.snippets.right_package_id;
                    let last_used_at = now_millis();
                    app.snippets.right_description.clear();
                    app.snippets.right_script = iced::widget::text_editor::Content::new();
                    app.snippets.right_package_id = None;
                    app.snippets.right_show_form = false;
                    return app.storage_task(
                        move |s| {
                            let snippet_id = s.next_snippet_id()?;
                            let snippet = Snippet {
                                id: snippet_id,
                                description: desc,
                                script,
                                package_id,
                                last_used_at,
                            };
                            s.upsert_snippet(&snippet)?;
                            Ok(snippet)
                        },
                        move |result| match result {
                            Ok(snippet) => Message::Snippets(
                                crate::snippets::message::Message::SnippetCreated { snippet },
                            ),
                            Err(e) => Message::ShowErrorToast(e),
                        },
                    );
                }
            }
        }
        DomainMessage::SnippetCreated { snippet } => {
            app.snippets.list.insert(0, snippet);
            sort_snippets(app);
            return Task::done(Message::ShowToast(t!("snippets.snippet_added").to_string()));
        }
        DomainMessage::SnippetRightSidebarShowAdd => {
            app.snippets.right_show_form = true;
            app.snippets.right_editing_id = None;
        }
        DomainMessage::SnippetRightSidebarCancelAdd => {
            app.snippets.right_show_form = false;
            app.snippets.right_editing_id = None;
            app.snippets.right_description.clear();
            app.snippets.right_script = iced::widget::text_editor::Content::new();
            app.snippets.right_package_id = None;
        }
        DomainMessage::SnippetRightSidebarEdit(id) => {
            if let Some(snippet) = app.snippets.list.iter().find(|s| s.id == id).cloned() {
                app.snippets.cursor.select(SnippetSelection::Snippet(id));
                app.snippets.right_show_form = true;
                app.snippets.right_editing_id = Some(id);
                app.snippets.right_description = snippet.description;
                app.snippets.right_script =
                    iced::widget::text_editor::Content::with_text(&snippet.script);
                app.snippets.right_package_id = snippet.package_id;
            }
        }
        // Package messages
        DomainMessage::SnippetPackageShowForm => {
            app.snippets.show_new_menu = false;
            app.snippets.cursor.clear();
            let was_visible = (app.snippets.form.visible || app.snippets.package_form.visible)
                && !app.snippets.form_closing;
            app.snippets.package_form.reset();
            app.snippets.package_form.visible = true;
            app.snippets.form_anim = if was_visible { 1.0 } else { 0.0 };
            app.snippets.form_closing = false;
            app.snippets.form.visible = false;
            app.needs_scroll_restore = true;
        }
        DomainMessage::SnippetPackageHideForm => {
            crate::cursor::close_active_dropdown(app);
            app.snippets.form_closing = true;
        }
        DomainMessage::SnippetPackageEdit(id) => {
            app.snippets.context_menu = None;
            open_package_editor(app, id);
        }
        DomainMessage::SnippetPackageDeleteRequest(id) => {
            app.snippets.context_menu = None;
            app.snippets.package_delete_pending = Some(id);
            return restore_scrolls(app);
        }
        DomainMessage::SnippetPackageDeleteCancel => {
            app.snippets.package_delete_pending = None;
            return restore_scrolls(app);
        }
        DomainMessage::SnippetPackageDelete(id) => {
            let name = app
                .snippets
                .packages
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            for snippet in &mut app.snippets.list {
                if snippet.package_id == Some(id) {
                    snippet.package_id = None;
                }
            }
            app.snippets.packages.retain(|p| p.id != id);
            if app.snippets.cursor.selected_key() == Some(&SnippetSelection::Package(id)) {
                app.snippets.cursor.clear();
            }
            app.snippets.package_delete_pending = None;
            if app.snippets.package_form.editing_id == Some(id) {
                app.snippets.package_form.visible = false;
                app.snippets.package_form.reset();
                app.needs_scroll_restore = true;
            }
            app.add_log(
                LogCategory::Snippet,
                format!("Deleted snippet package \"{name}\""),
            );
            let save = app.storage_task(
                move |s| s.delete_snippet_package(id),
                move |result| match result {
                    Ok(()) => Message::Noop,
                    Err(e) => Message::ShowErrorToast(e),
                },
            );
            return Task::batch([save, restore_scrolls(app)]);
        }
        DomainMessage::SnippetPackageFormName(v) => app.snippets.package_form.name = v,
        DomainMessage::SnippetPackageFormDescription(v) => {
            app.snippets.package_form.description = v;
        }
        DomainMessage::SnippetPackageFormSubmit => {
            return submit_package_form(app);
        }
        DomainMessage::SnippetPackageCreated { package } => {
            app.snippets.packages.insert(0, package);
            return Task::none();
        }
        DomainMessage::SnippetToggleNewMenu => {
            app.snippets.show_new_menu = !app.snippets.show_new_menu;
        }
        DomainMessage::SnippetToggleView => {
            app.snippets.grid_view = !app.snippets.grid_view;
            let value = if app.snippets.grid_view {
                "true"
            } else {
                "false"
            };
            return app.storage_write_task(move |s| s.save_preference("snippets_grid_view", value));
        }
        DomainMessage::SnippetHoverEnter(id) => {
            app.snippets.hovered_snippet_id = Some(id);
        }
        DomainMessage::SnippetSelect(id) => {
            activate_snippet(app, id);
        }
        DomainMessage::SnippetHoverLeave(id) => {
            if app.snippets.hovered_snippet_id == Some(id) {
                app.snippets.hovered_snippet_id = None;
            }
        }
        DomainMessage::SnippetPackageHoverEnter(id) => {
            app.snippets.hovered_package_id = Some(id);
        }
        DomainMessage::SnippetPackageSelect(id) => {
            activate_package(app, id);
        }
        DomainMessage::SnippetPackageHoverLeave(id) => {
            if app.snippets.hovered_package_id == Some(id) {
                app.snippets.hovered_package_id = None;
            }
        }
        DomainMessage::SnippetContextMenuOpen(id) => {
            activate_snippet(app, id);
            app.snippets.context_menu = Some(SnippetContextMenu::Snippet {
                id,
                position: app.window.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::SnippetEdit,
                    crate::core::context_menu::ContextMenuAction::SnippetCopy,
                    crate::core::context_menu::ContextMenuAction::SnippetDelete,
                ]),
            });
            return Task::none();
        }
        DomainMessage::SnippetPackageContextMenuOpen(id) => {
            activate_package(app, id);
            app.snippets.context_menu = Some(SnippetContextMenu::Package {
                id,
                position: app.window.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::SnippetPackageEdit,
                    crate::core::context_menu::ContextMenuAction::SnippetPackageDelete,
                ]),
            });
            return Task::none();
        }
        DomainMessage::SnippetContextMenuClose => {
            app.snippets.context_menu = None;
            return Task::none();
        }
        DomainMessage::SnippetViewCursorMoved(pos) => {
            app.snippets.cursor_pos = pos;
        }
    }
    Task::none()
}

fn activate_snippet(app: &mut App, id: SnippetId) {
    if app.snippets.form.visible || app.snippets.package_form.visible {
        open_snippet_editor(app, id);
    } else {
        app.snippets.cursor.select(SnippetSelection::Snippet(id));
    }
}

fn activate_package(app: &mut App, id: SnippetPackageId) {
    if app.snippets.form.visible || app.snippets.package_form.visible {
        open_package_editor(app, id);
    } else {
        app.snippets.cursor.select(SnippetSelection::Package(id));
    }
}

fn open_snippet_editor(app: &mut App, id: SnippetId) {
    app.snippets.cursor.select(SnippetSelection::Snippet(id));
    let was_visible = (app.snippets.form.visible || app.snippets.package_form.visible)
        && !app.snippets.form_closing;
    let Some(snippet) = app.snippets.list.iter().find(|s| s.id == id).cloned() else {
        return;
    };
    let form = &mut app.snippets.form;
    form.visible = true;
    form.editing_id = Some(id);
    app.snippets.form_anim = if was_visible { 1.0 } else { 0.0 };
    app.snippets.form_closing = false;
    form.original = Some(snippet.clone());
    form.description = snippet.description;
    form.script = iced::widget::text_editor::Content::with_text(&snippet.script);
    form.package_id = snippet.package_id;
    app.snippets.package_form.visible = false;
    app.needs_scroll_restore = true;
}

fn open_package_editor(app: &mut App, id: SnippetPackageId) {
    app.snippets.cursor.select(SnippetSelection::Package(id));
    let was_visible = (app.snippets.form.visible || app.snippets.package_form.visible)
        && !app.snippets.form_closing;
    let Some(pkg) = app.snippets.packages.iter().find(|p| p.id == id).cloned() else {
        return;
    };
    let form = &mut app.snippets.package_form;
    form.visible = true;
    form.editing_id = Some(id);
    app.snippets.form_anim = if was_visible { 1.0 } else { 0.0 };
    app.snippets.form_closing = false;
    form.original = Some(pkg.clone());
    form.name = pkg.name;
    form.description = pkg.description;
    app.snippets.form.visible = false;
    app.needs_scroll_restore = true;
}

fn terminal_cmd(app: &mut App, id: SnippetId, with_enter: bool) -> Option<iced::Task<Message>> {
    let snippet = app.snippets.list.iter().find(|s| s.id == id)?.clone();
    let Some(sid) = app.active_terminal_session_id() else {
        return Some(Task::done(Message::ShowToast(
            t!("snippets.no_active_terminal").to_string(),
        )));
    };
    let session = app.terminal.sessions.iter_mut().find(|s| s.id == sid)?;
    let mut data = snippet.script.into_bytes();
    if with_enter {
        data.push(b'\r');
    }
    session.terminal.handle(iced_term::Command::ProxyToBackend(
        iced_term::BackendCommand::Write(data),
    ));
    None
}

fn run_in_terminal(app: &mut App, id: SnippetId, with_enter: bool) -> iced::Task<Message> {
    if let Some(toast) = terminal_cmd(app, id, with_enter) {
        return toast;
    }
    Task::batch([
        touch_snippet(app, id),
        Task::done(Message::ShowToast(t!("snippets.running").to_string())),
    ])
}

fn paste_to_terminal(app: &mut App, id: SnippetId) -> iced::Task<Message> {
    if let Some(toast) = terminal_cmd(app, id, false) {
        return toast;
    }
    Task::batch([
        touch_snippet(app, id),
        Task::done(Message::ShowToast(t!("snippets.pasted").to_string())),
    ])
}

fn now_millis() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn touch_snippet(app: &mut App, id: SnippetId) -> iced::Task<Message> {
    let Some(snippet) = app.snippets.list.iter_mut().find(|s| s.id == id) else {
        return iced::Task::none();
    };
    snippet.last_used_at = now_millis();
    let snippet = snippet.clone();
    sort_snippets(app);
    app.storage_write_task(move |s| s.upsert_snippet(&snippet))
}

fn sort_snippets(app: &mut App) {
    app.snippets
        .list
        .sort_by(|a, b| b.last_used_at.cmp(&a.last_used_at));
}

fn submit_form(app: &mut App) -> iced::Task<Message> {
    let form = &app.snippets.form;
    if !form.save_enabled() {
        return iced::Task::none();
    }
    let description = form.description.trim().to_string();
    let script = form.script.text().trim().to_string();

    if let Some(id) = form.editing_id {
        let updated = app
            .snippets
            .list
            .iter_mut()
            .find(|s| s.id == id)
            .map(|snippet| {
                snippet.description.clone_from(&description);
                snippet.script = script;
                snippet.package_id = form.package_id;
                snippet.last_used_at = now_millis();
                snippet.clone()
            });
        if let Some(snippet) = updated {
            app.add_log(
                LogCategory::Snippet,
                format!("Updated snippet \"{description}\""),
            );
            sort_snippets(app);
            app.snippets.form.visible = false;
            app.snippets.form.reset();
            app.needs_scroll_restore = true;
            return app.storage_write_task(move |s| s.upsert_snippet(&snippet));
        }
    } else {
        let description_for_log = description.clone();
        let package_id = form.package_id;
        let last_used_at = now_millis();
        app.add_log(
            LogCategory::Snippet,
            format!("Created snippet \"{description_for_log}\""),
        );
        app.snippets.form.visible = false;
        app.snippets.form.reset();
        app.needs_scroll_restore = true;
        return app.storage_task(
            move |s| {
                let snippet_id = s.next_snippet_id()?;
                let snippet = Snippet {
                    id: snippet_id,
                    description,
                    script,
                    package_id,
                    last_used_at,
                };
                s.upsert_snippet(&snippet)?;
                Ok(snippet)
            },
            move |result| match result {
                Ok(snippet) => {
                    Message::Snippets(crate::snippets::message::Message::SnippetCreated { snippet })
                }
                Err(e) => Message::ShowErrorToast(e),
            },
        );
    }

    app.snippets.form.visible = false;
    app.snippets.form.reset();
    app.needs_scroll_restore = true;
    iced::Task::none()
}

fn submit_package_form(app: &mut App) -> iced::Task<Message> {
    let form = &app.snippets.package_form;
    if !form.save_enabled() {
        return iced::Task::none();
    }
    let name = form.name.trim().to_string();
    let description = form.description.trim().to_string();

    if let Some(id) = form.editing_id {
        let updated = app
            .snippets
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .map(|pkg| {
                pkg.name.clone_from(&name);
                pkg.description = description;
                pkg.clone()
            });
        if let Some(pkg) = updated {
            app.add_log(
                LogCategory::Snippet,
                format!("Updated snippet package \"{name}\""),
            );
            app.snippets.package_form.visible = false;
            app.snippets.package_form.reset();
            app.needs_scroll_restore = true;
            return app.storage_write_task(move |s| s.upsert_snippet_package(&pkg));
        }
    } else {
        let name_for_log = name.clone();
        app.add_log(
            LogCategory::Snippet,
            format!("Created snippet package \"{name_for_log}\""),
        );
        app.snippets.package_form.visible = false;
        app.snippets.package_form.reset();
        app.needs_scroll_restore = true;
        return app.storage_task(
            move |s| {
                let package_id = s.next_snippet_package_id()?;
                let pkg = SnippetPackage {
                    id: package_id,
                    name,
                    description,
                };
                s.upsert_snippet_package(&pkg)?;
                Ok(pkg)
            },
            move |result| match result {
                Ok(package) => {
                    Message::Snippets(crate::snippets::message::Message::SnippetPackageCreated {
                        package,
                    })
                }
                Err(e) => Message::ShowErrorToast(e),
            },
        );
    }

    app.snippets.package_form.visible = false;
    app.snippets.package_form.reset();
    app.needs_scroll_restore = true;
    iced::Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::grid_cursor::GridDirection;
    use crate::core::test_support::app;

    fn snippet(id: u32, description: &str, script: &str) -> Snippet {
        Snippet {
            id: SnippetId(id),
            description: description.to_string(),
            script: script.to_string(),
            package_id: None,
            last_used_at: String::new(),
        }
    }

    fn package(id: u32, name: &str) -> SnippetPackage {
        SnippetPackage {
            id: SnippetPackageId(id),
            name: name.to_string(),
            description: String::new(),
        }
    }

    /// The editor is filled through its own actions, the way the widget sends
    /// them; this is the closest a test gets to typing.
    fn type_text(content: &mut iced::widget::text_editor::Content, text: &str) {
        for ch in text.chars() {
            content.perform(iced::widget::text_editor::Action::Edit(
                iced::widget::text_editor::Edit::Insert(ch),
            ));
        }
    }

    #[test]
    fn the_new_snippet_form_starts_empty_and_closes_the_package_form() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::SnippetShowForm);
        assert!(app.snippets.form.visible);
        assert!(app.snippets.form.description.is_empty());
        assert!(app.snippets.form.script.text().is_empty());

        let _ = update(&mut app, DomainMessage::SnippetPackageShowForm);
        let _ = update(&mut app, DomainMessage::SnippetShowForm);
        assert!(
            !app.snippets.package_form.visible,
            "the two forms share the same panel"
        );
    }

    #[test]
    fn an_incomplete_snippet_is_not_submitted() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::SnippetShowForm);
        let _ = update(
            &mut app,
            DomainMessage::SnippetFormDescription("list files".into()),
        );

        let _ = update(&mut app, DomainMessage::SnippetFormSubmit);

        assert!(app.snippets.form.visible, "the form waits for the script");
        assert!(app.snippets.list.is_empty());
    }

    #[test]
    fn editing_a_snippet_loads_it_and_marks_it_clean() {
        let mut app = app();
        app.snippets.list = vec![snippet(1, "list files", "ls -la")];

        let _ = update(&mut app, DomainMessage::SnippetEdit(SnippetId(1)));

        assert!(app.snippets.form.visible);
        assert_eq!(app.snippets.form.editing_id, Some(SnippetId(1)));
        assert_eq!(app.snippets.form.description, "list files");
        assert_eq!(app.snippets.form.script.text(), "ls -la");
        assert!(!app.snippets.form.is_dirty());
    }

    #[test]
    fn a_created_snippet_lands_at_the_top_of_the_list() {
        let mut app = app();
        app.snippets.list = vec![snippet(1, "old", "true")];

        let _ = update(
            &mut app,
            DomainMessage::SnippetCreated {
                snippet: snippet(2, "new", "echo hi"),
            },
        );

        assert_eq!(app.snippets.list.len(), 2);
        assert_eq!(app.snippets.list[0].id, SnippetId(2));
    }

    #[test]
    fn snippets_are_ordered_by_most_recently_used() {
        let mut app = app();
        let mut older = snippet(1, "older", "true");
        older.last_used_at = "1000".into();
        let mut newer = snippet(2, "newer", "true");
        newer.last_used_at = "2000".into();
        app.snippets.list = vec![older, newer];

        let _ = update(
            &mut app,
            DomainMessage::SnippetCreated {
                snippet: {
                    let mut middle = snippet(3, "middle", "true");
                    middle.last_used_at = "1500".into();
                    middle
                },
            },
        );

        let order: Vec<u32> = app.snippets.list.iter().map(|s| s.id.0).collect();
        assert_eq!(order, [2, 3, 1], "newest use first");
    }

    #[test]
    fn deleting_a_snippet_asks_first_and_clears_both_editors() {
        let mut app = app();
        app.snippets.list = vec![snippet(1, "list files", "ls -la")];
        let _ = update(&mut app, DomainMessage::SnippetEdit(SnippetId(1)));
        app.snippets.right_show_form = true;
        app.snippets.right_editing_id = Some(SnippetId(1));
        app.snippets.right_description = "list files".into();

        let _ = update(&mut app, DomainMessage::SnippetDeleteRequest(SnippetId(1)));
        assert_eq!(app.snippets.delete_pending, Some(SnippetId(1)));

        let _ = update(&mut app, DomainMessage::SnippetDeleteCancel);
        assert!(app.snippets.list.len() == 1, "cancelling deletes nothing");

        let _ = update(&mut app, DomainMessage::SnippetDelete(SnippetId(1)));
        assert!(app.snippets.list.is_empty());
        assert!(!app.snippets.form.visible, "the panel form closes");
        assert!(
            !app.snippets.right_show_form && app.snippets.right_editing_id.is_none(),
            "the sidebar editor closes too"
        );
        assert!(app.snippets.right_description.is_empty());
    }

    #[test]
    fn the_sidebar_add_form_edits_a_new_snippet_and_cancelling_clears_it() {
        let mut app = app();

        let _ = update(&mut app, DomainMessage::SnippetRightSidebarShowAdd);
        assert!(app.snippets.right_show_form);
        assert!(
            app.snippets.right_editing_id.is_none(),
            "this is a new snippet"
        );

        // `ShowAdd` switches the editor to "new"; the panel's own state is
        // emptied when it is closed again.
        let _ = update(
            &mut app,
            DomainMessage::SnippetRightSidebarFormDescription("half typed".into()),
        );
        let _ = update(&mut app, DomainMessage::SnippetRightSidebarCancelAdd);
        assert!(!app.snippets.right_show_form);
        assert!(app.snippets.right_description.is_empty());
        assert!(app.snippets.right_script.text().is_empty());
        assert_eq!(app.snippets.right_package_id, None);
    }

    #[test]
    fn the_sidebar_editor_updates_the_snippet_it_was_opened_on() {
        let mut app = app();
        app.snippets.list = vec![snippet(1, "list files", "ls -la")];
        let _ = update(
            &mut app,
            DomainMessage::SnippetRightSidebarEdit(SnippetId(1)),
        );
        assert_eq!(app.snippets.right_editing_id, Some(SnippetId(1)));
        assert_eq!(app.snippets.right_description, "list files");
        assert_eq!(app.snippets.right_script.text(), "ls -la");

        let _ = update(
            &mut app,
            DomainMessage::SnippetRightSidebarFormDescription("list all".into()),
        );
        let _ = update(&mut app, DomainMessage::SnippetRightSidebarFormSubmit);

        assert_eq!(app.snippets.list[0].description, "list all");
        assert!(
            !app.snippets.right_show_form,
            "the sidebar form closes on save"
        );
        assert!(
            !app.snippets.list[0].last_used_at.is_empty(),
            "an edit refreshes the recency stamp"
        );
    }

    #[test]
    fn the_sidebar_editor_refuses_a_snippet_without_a_script() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::SnippetRightSidebarShowAdd);
        let _ = update(
            &mut app,
            DomainMessage::SnippetRightSidebarFormDescription("only a name".into()),
        );

        let _ = update(&mut app, DomainMessage::SnippetRightSidebarFormSubmit);

        assert!(app.snippets.list.is_empty());
        assert!(app.snippets.right_show_form, "the form stays open");
    }

    #[test]
    fn a_typed_script_reaches_the_editor() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::SnippetShowForm);

        let mut content = iced::widget::text_editor::Content::new();
        type_text(&mut content, "echo hi");
        let _ = update(
            &mut app,
            DomainMessage::SnippetFormDescription("greet".into()),
        );
        // Feed the same action the widget would send for each keystroke.
        for ch in "echo hi".chars() {
            let _ = update(
                &mut app,
                DomainMessage::SnippetFormScript(iced::widget::text_editor::Action::Edit(
                    iced::widget::text_editor::Edit::Insert(ch),
                )),
            );
        }

        assert_eq!(app.snippets.form.script.text(), "echo hi");
        assert!(app.snippets.form.save_enabled());
    }

    #[test]
    fn a_package_needs_a_name_and_lands_in_the_list() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::SnippetPackageShowForm);
        assert!(app.snippets.package_form.visible);

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageFormName("   ".into()),
        );
        let _ = update(&mut app, DomainMessage::SnippetPackageFormSubmit);
        assert!(
            app.snippets.packages.is_empty(),
            "a blank name is not saved"
        );

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageCreated {
                package: package(1, "ops"),
            },
        );
        assert_eq!(app.snippets.packages.len(), 1);
        assert_eq!(app.snippets.packages[0].name, "ops");
    }

    #[test]
    fn deleting_a_package_keeps_its_snippets() {
        let mut app = app();
        let mut owned = snippet(1, "in a package", "true");
        owned.package_id = Some(SnippetPackageId(1));
        app.snippets.packages = vec![package(1, "ops")];
        app.snippets.list = vec![owned];

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageDeleteRequest(SnippetPackageId(1)),
        );
        assert_eq!(
            app.snippets.package_delete_pending,
            Some(SnippetPackageId(1))
        );
        let _ = update(&mut app, DomainMessage::SnippetPackageDeleteCancel);
        assert_eq!(app.snippets.package_delete_pending, None);
        assert_eq!(app.snippets.packages.len(), 1);

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageDelete(SnippetPackageId(1)),
        );
        assert!(app.snippets.packages.is_empty());
        assert_eq!(
            app.snippets.list.len(),
            1,
            "deleting a folder does not delete what is in it"
        );
    }

    #[test]
    fn editing_a_package_loads_it_and_marks_it_clean() {
        let mut app = app();
        app.snippets.packages = vec![package(1, "ops")];

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageEdit(SnippetPackageId(1)),
        );

        assert_eq!(
            app.snippets.package_form.editing_id,
            Some(SnippetPackageId(1))
        );
        assert_eq!(app.snippets.package_form.name, "ops");
        assert!(!app.snippets.package_form.is_dirty());
        assert!(
            !app.snippets.form.visible,
            "the package form takes the panel"
        );
    }

    #[test]
    fn toggling_the_layout_flips_between_grid_and_list() {
        let mut app = app();
        let was_grid = app.snippets.grid_view;

        let _ = update(&mut app, DomainMessage::SnippetToggleView);
        assert_ne!(app.snippets.grid_view, was_grid);

        let _ = update(&mut app, DomainMessage::SnippetToggleView);
        assert_eq!(app.snippets.grid_view, was_grid);
    }

    #[test]
    fn hover_events_track_the_snippet_and_the_package_under_the_cursor() {
        let mut app = app();

        let _ = update(&mut app, DomainMessage::SnippetHoverEnter(SnippetId(1)));
        assert_eq!(app.snippets.hovered_snippet_id, Some(SnippetId(1)));
        let _ = update(&mut app, DomainMessage::SnippetHoverLeave(SnippetId(1)));
        assert_eq!(app.snippets.hovered_snippet_id, None);

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageHoverEnter(SnippetPackageId(2)),
        );
        assert_eq!(app.snippets.hovered_package_id, Some(SnippetPackageId(2)));
        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageHoverLeave(SnippetPackageId(2)),
        );
        assert_eq!(app.snippets.hovered_package_id, None);
    }

    #[test]
    fn snippet_and_package_selection_are_mutually_exclusive() {
        let mut app = app();
        app.snippets.list = vec![snippet(1, "deploy", "cargo run")];
        app.snippets.packages = vec![package(2, "infra")];

        let _ = update(&mut app, DomainMessage::SnippetSelect(SnippetId(1)));
        assert_eq!(app.snippets.selected_snippet(), Some(SnippetId(1)));
        assert_eq!(app.snippets.selected_package(), None);

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageSelect(SnippetPackageId(2)),
        );
        assert_eq!(app.snippets.selected_snippet(), None);
        assert_eq!(app.snippets.selected_package(), Some(SnippetPackageId(2)));

        let _ = update(&mut app, DomainMessage::SnippetSelect(SnippetId(1)));
        assert_eq!(app.snippets.selected_snippet(), Some(SnippetId(1)));
        assert_eq!(app.snippets.selected_package(), None);
    }

    #[test]
    fn selecting_across_sections_rebinds_the_open_editor() {
        let mut app = app();
        app.snippets.list = vec![snippet(1, "deploy", "cargo run")];
        app.snippets.packages = vec![package(2, "infra")];

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageEdit(SnippetPackageId(2)),
        );
        assert!(app.snippets.package_form.visible);
        assert!(!app.snippets.form.visible);

        let _ = update(&mut app, DomainMessage::SnippetSelect(SnippetId(1)));
        assert_eq!(app.snippets.selected_snippet(), Some(SnippetId(1)));
        assert!(app.snippets.form.visible);
        assert!(!app.snippets.package_form.visible);
        assert_eq!(app.snippets.form.editing_id, Some(SnippetId(1)));

        let _ = update(
            &mut app,
            DomainMessage::SnippetPackageSelect(SnippetPackageId(2)),
        );
        assert_eq!(app.snippets.selected_package(), Some(SnippetPackageId(2)));
        assert!(app.snippets.package_form.visible);
        assert!(!app.snippets.form.visible);
        assert_eq!(
            app.snippets.package_form.editing_id,
            Some(SnippetPackageId(2))
        );
    }

    #[test]
    fn cursor_moves_within_the_active_snippet_section() {
        let mut app = app();
        app.snippets.packages = vec![package(1, "one"), package(2, "two")];
        app.snippets.list = vec![snippet(1, "first", "one"), snippet(2, "second", "two")];
        app.snippets.grid_view = true;
        app.snippets.grid_columns = 2;

        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::Snippets,
            GridDirection::Down,
        );
        assert_eq!(app.snippets.selected_package(), Some(SnippetPackageId(1)));
        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::Snippets,
            GridDirection::Down,
        );
        assert_eq!(
            app.snippets.selected_snippet(),
            Some(SnippetId(1)),
            "down at the package-grid edge enters Snippets"
        );
        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::Snippets,
            GridDirection::Up,
        );
        assert_eq!(
            app.snippets.selected_package(),
            Some(SnippetPackageId(2)),
            "up from Snippets returns to the last package"
        );
    }
}
