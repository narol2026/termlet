//! Saved-workspace lifecycle and import/export updates.

use iced::Task;

use super::{
    App, Message, begin_workspace_apply, default_workspace_name, missing_ssh_host_count,
    present_workspace_import, refresh_workspace_manager, workspace_transfer_file_dialog,
};
use crate::core::data::LogCategory;
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::WorkspaceSave => {
            if crate::workspace::capture(app).is_none() {
                refresh_workspace_manager(app);
                return Task::done(Message::ShowToast(
                    t!("app.workspace_saved_empty").to_string(),
                ));
            }
            refresh_workspace_manager(app);
            app.workspace_manager.save_name = default_workspace_name(app);
            app.workspace_manager.save_conflict = None;
            app.workspace_manager.save_snapshot = None;
            app.workspace_manager.save_pending = true;
            Task::none()
        }
        Message::WorkspaceSaveName(name) => {
            app.workspace_manager.save_name = name;
            Task::none()
        }
        Message::WorkspaceSaveConfirm => {
            let Some(snapshot) = crate::workspace::capture(app) else {
                app.workspace_manager.save_pending = false;
                app.workspace_manager.save_name = String::new();
                app.workspace_manager.save_conflict = None;
                app.workspace_manager.save_snapshot = None;
                return Task::done(Message::ShowToast(
                    t!("app.workspace_saved_empty").to_string(),
                ));
            };
            let name = app.workspace_manager.save_name.trim().to_string();
            if name.is_empty() {
                return Task::done(Message::ShowWarningToast(
                    t!("workspace.name_required").to_string(),
                ));
            }
            if app
                .workspace_manager
                .list
                .iter()
                .any(|saved| saved.name == name)
            {
                app.workspace_manager.save_conflict = Some(name);
                app.workspace_manager.save_snapshot = Some(snapshot);
                return Task::none();
            }
            match crate::workspace::save_named_workspace(&name, &snapshot) {
                Ok(saved) => {
                    app.workspace_manager.save_pending = false;
                    app.workspace_manager.save_name = String::new();
                    refresh_workspace_manager(app);
                    Task::done(Message::ShowToast(
                        t!("workspace.saved", name = saved.name.as_str()).to_string(),
                    ))
                }
                Err(error) => Task::done(Message::ShowErrorToast(error)),
            }
        }
        Message::WorkspaceSaveOverwrite => {
            let Some(name) = app.workspace_manager.save_conflict.take() else {
                return Task::none();
            };
            let Some(snapshot) = app.workspace_manager.save_snapshot.take() else {
                return Task::none();
            };
            match crate::workspace::overwrite_workspace_by_name(&name, &snapshot) {
                Ok(saved) => {
                    app.workspace_manager.save_pending = false;
                    app.workspace_manager.save_name = String::new();
                    app.workspace_manager.save_conflict = None;
                    app.workspace_manager.save_snapshot = None;
                    refresh_workspace_manager(app);
                    Task::done(Message::ShowToast(
                        t!("workspace.overwritten", name = saved.name.as_str()).to_string(),
                    ))
                }
                Err(error) => Task::done(Message::ShowErrorToast(error)),
            }
        }
        Message::WorkspaceSaveKeepDuplicate => {
            let Some(name) = app.workspace_manager.save_conflict.take() else {
                return Task::none();
            };
            let Some(snapshot) = app.workspace_manager.save_snapshot.take() else {
                return Task::none();
            };
            match crate::workspace::save_named_workspace(&name, &snapshot) {
                Ok(saved) => {
                    app.workspace_manager.save_pending = false;
                    app.workspace_manager.save_name = String::new();
                    app.workspace_manager.save_conflict = None;
                    app.workspace_manager.save_snapshot = None;
                    refresh_workspace_manager(app);
                    Task::done(Message::ShowToast(
                        t!("workspace.saved", name = saved.name.as_str()).to_string(),
                    ))
                }
                Err(error) => Task::done(Message::ShowErrorToast(error)),
            }
        }
        Message::WorkspaceSaveConflictCancel => {
            app.workspace_manager.save_conflict = None;
            app.workspace_manager.save_snapshot = None;
            Task::none()
        }
        Message::WorkspaceSaveCancel => {
            app.workspace_manager.save_pending = false;
            app.workspace_manager.save_name = String::new();
            app.workspace_manager.save_conflict = None;
            app.workspace_manager.save_snapshot = None;
            Task::none()
        }
        Message::WorkspaceApply(id) => {
            let Some(saved) = crate::workspace::load_workspace(&id) else {
                return Task::done(Message::ShowWarningToast(
                    t!("workspace.apply_empty").to_string(),
                ));
            };
            begin_workspace_apply(app, saved.name, saved.snapshot);
            Task::none()
        }
        Message::WorkspaceApplyConfirm => {
            let Some(target) = app.pending_apply_workspace.take() else {
                return Task::none();
            };
            crate::cursor::close_active_dropdown(app);
            app.show_apply_workspace_confirm = false;
            let missing_hosts = missing_ssh_host_count(&target.snapshot, &app.hosts.list);
            crate::terminal::update::close_all_terminal_tabs_for_workspace_apply(app);
            let restore_task = crate::workspace::restore_workspace(app, &target.snapshot);
            app.add_log(
                LogCategory::Terminal,
                format!(
                    "Applied workspace \"{}\": {} tab(s), {} missing SSH host(s)",
                    target.name,
                    app.terminal.tabs.len(),
                    missing_hosts
                ),
            );
            let toast = if missing_hosts > 0 {
                Message::ShowWarningToast(
                    t!("workspace.apply_partial", count = missing_hosts).to_string(),
                )
            } else {
                Message::ShowToast(t!("workspace.apply_saved").to_string())
            };
            Task::batch([restore_task, Task::done(toast)])
        }
        Message::WorkspaceApplyCancel => {
            app.show_apply_workspace_confirm = false;
            app.pending_apply_workspace = None;
            Task::none()
        }
        Message::WorkspaceRenameStart(id) => {
            let Some(saved) = app
                .workspace_manager
                .list
                .iter()
                .find(|saved| saved.id == id)
            else {
                return Task::none();
            };
            app.workspace_manager.rename_id = Some(id);
            app.workspace_manager.rename_name = saved.name.clone();
            Task::none()
        }
        Message::WorkspaceRenameInput(name) => {
            if app.workspace_manager.rename_id.is_some() {
                app.workspace_manager.rename_name = name;
            }
            Task::none()
        }
        Message::WorkspaceRenameSubmit => {
            let Some(id) = app.workspace_manager.rename_id.take() else {
                return Task::none();
            };
            let name = app.workspace_manager.rename_name.trim().to_string();
            if name.is_empty() {
                app.workspace_manager.rename_id = Some(id);
                return Task::done(Message::ShowWarningToast(
                    t!("workspace.name_required").to_string(),
                ));
            }
            match crate::workspace::rename_workspace(&id, &name) {
                Ok(()) => {
                    if app.workspace_manager.cursor.selected_key() == Some(&id) {
                        app.workspace_manager.cursor.clear();
                    }
                    refresh_workspace_manager(app);
                    Task::done(Message::ShowToast(t!("workspace.renamed").to_string()))
                }
                Err(error) => Task::done(Message::ShowErrorToast(error)),
            }
        }
        Message::WorkspaceRenameCancel => {
            app.workspace_manager.rename_id = None;
            app.workspace_manager.rename_name = String::new();
            Task::none()
        }
        Message::WorkspaceDeleteRequest(id) => {
            app.workspace_manager.delete_id = Some(id);
            Task::none()
        }
        Message::WorkspaceDeleteConfirm => {
            let Some(id) = app.workspace_manager.delete_id.take() else {
                return Task::none();
            };
            match crate::workspace::delete_workspace(&id) {
                Ok(()) => {
                    refresh_workspace_manager(app);
                    Task::done(Message::ShowToast(t!("workspace.deleted").to_string()))
                }
                Err(error) => Task::done(Message::ShowErrorToast(error)),
            }
        }
        Message::WorkspaceDeleteCancel => {
            app.workspace_manager.delete_id = None;
            Task::none()
        }
        Message::WorkspaceSelect(id) => {
            app.workspace_manager.cursor.select(id);
            Task::none()
        }
        Message::WorkspaceHoverEnter(id) => {
            app.workspace_manager.hovered_id = Some(id);
            Task::none()
        }
        Message::WorkspaceHoverLeave(id) => {
            if app.workspace_manager.hovered_id.as_deref() == Some(id.as_str()) {
                app.workspace_manager.hovered_id = None;
            }
            Task::none()
        }
        Message::WorkspaceTransferMenuToggle => crate::cursor::toggle_dropdown(
            app,
            crate::core::dropdown::DropdownTarget::WorkspaceTransferMenu,
        ),
        Message::WorkspaceExportRequest => {
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::WorkspaceTransferMenu,
            );
            if app.workspace_manager.list.is_empty() {
                return Task::done(Message::ShowToast(t!("workspace.export_empty").to_string()));
            }
            let json_bytes = match crate::workspace::export::workspaces_to_export_json(
                &app.workspace_manager.list,
            ) {
                Ok(bytes) => bytes,
                Err(error) => return Task::done(Message::ShowErrorToast(error)),
            };
            let title = t!("workspace.export_title").to_string();
            Task::perform(
                async move {
                    let handle = workspace_transfer_file_dialog(title).save_file().await;
                    let Some(handle) = handle else {
                        return Ok::<Option<std::path::PathBuf>, String>(None);
                    };
                    let path = handle.path().to_path_buf();
                    std::fs::write(&path, &json_bytes)
                        .map_err(|error| format!("failed to write file: {error}"))?;
                    Ok(Some(path))
                },
                Message::WorkspaceExportSaved,
            )
        }
        Message::WorkspaceExportSaved(result) => match result {
            Ok(Some(path)) => {
                let msg = t!(
                    "workspace.export_success",
                    path = path.display().to_string()
                )
                .to_string();
                Task::done(Message::ShowToast(msg))
            }
            Ok(None) => Task::none(),
            Err(error) => Task::done(Message::ShowErrorToast(error)),
        },
        Message::WorkspaceImportRequest => {
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::WorkspaceTransferMenu,
            );
            let title = t!("workspace.import_title").to_string();
            Task::perform(
                async move {
                    workspace_transfer_file_dialog(title)
                        .pick_file()
                        .await
                        .map(|handle| handle.path().to_path_buf())
                },
                Message::WorkspaceImportPicked,
            )
        }
        Message::WorkspaceImportPicked(Some(path)) => {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) => {
                    return Task::done(Message::ShowErrorToast(format!(
                        "failed to read file: {error}"
                    )));
                }
            };
            let workspaces = match crate::workspace::export::parse_export_json(&text) {
                Ok(workspaces) => workspaces,
                Err(error) => return Task::done(Message::ShowErrorToast(error)),
            };
            present_workspace_import(app, workspaces)
        }
        Message::WorkspaceImportPicked(None) => Task::none(),
        Message::WorkspaceImportConfirm => {
            let Some(dialog) = app.workspace_manager.import_dialog.take() else {
                return Task::none();
            };
            let pending = dialog.pending_workspaces;
            match crate::workspace::import_workspaces(&pending) {
                Ok((new_count, dup_count)) => {
                    refresh_workspace_manager(app);
                    app.add_log(
                        LogCategory::Terminal,
                        format!(
                            "Imported {new_count} workspace(s), skipped {dup_count} duplicate(s)"
                        ),
                    );
                    let msg = if dup_count > 0 {
                        t!(
                            "workspace.import_success_with_dups",
                            new = new_count,
                            dup = dup_count
                        )
                        .to_string()
                    } else {
                        t!("workspace.import_success", n = new_count).to_string()
                    };
                    Task::done(Message::ShowToast(msg))
                }
                Err(error) => Task::done(Message::ShowErrorToast(error)),
            }
        }
        Message::WorkspaceImportCancel => {
            app.workspace_manager.import_dialog = None;
            Task::none()
        }
        Message::WorkspaceRecovery(inner) => crate::workspace::recovery::update(app, &inner),
        _ => unreachable!("workspace handler received another message"),
    }
}
