//! Handles encrypted host import and export.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;

use iced::Task;
use secrecy::{ExposeSecret, SecretString};

use crate::app::App;
use crate::core::data::{AuthType, Host, LogCategory, OsType, Protocol};
use crate::core::message::Message;
use crate::hosts::export::ExportHost;
use crate::hosts::message::Message as DomainMessage;
use crate::hosts::state::{HostTransferDialog, HostTransferKind};
use rust_i18n::t;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::HostTransferMenuToggle => {
            app.hosts.hovered_id = None;
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostsTransferMenu,
            );
        }
        DomainMessage::HostExportRequest => {
            close_transfer_menu(app);
            app.hosts.transfer_dialog = Some(HostTransferDialog::new_export());
        }
        DomainMessage::HostExportClose | DomainMessage::HostImportClose => {
            app.hosts.transfer_dialog = None;
        }
        DomainMessage::HostExportPasswordChanged(password)
        | DomainMessage::HostImportPasswordChanged(password) => {
            if let Some(dialog) = app.hosts.transfer_dialog.as_mut() {
                dialog.password = password;
                dialog.error = None;
            }
        }
        DomainMessage::HostExportSubmit => return export(app),
        DomainMessage::HostExportSaved(result) => return export_finished(app, result),
        DomainMessage::HostImportRequest => return begin_import(app),
        DomainMessage::HostImportPicked(Some(path)) => {
            // Exports are always encrypted; retain the bytes for password retry.
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Task::done(Message::ShowErrorToast(format!(
                        "failed to read file: {error}"
                    )));
                }
            };
            app.hosts.transfer_dialog = Some(HostTransferDialog::new_import_password(path, bytes));
        }
        DomainMessage::HostImportPicked(None) => {}
        DomainMessage::HostImportPasswordSubmit => return decrypt_import(app),
        DomainMessage::HostImportParsed(result) => return import_parsed(app, result),
        DomainMessage::HostImportConfirm => return confirm_import(app),
        DomainMessage::HostsImported {
            hosts,
            new_count,
            dup_count,
        } => return imported(app, hosts, new_count, dup_count),
        DomainMessage::HostTransferTogglePasswordVisible => {
            if let Some(dialog) = app.hosts.transfer_dialog.as_mut() {
                dialog.password_visible = !dialog.password_visible;
            }
        }
        _ => {}
    }
    Task::none()
}

fn close_transfer_menu(app: &mut App) {
    crate::cursor::close_dropdown(
        app,
        crate::core::dropdown::DropdownTarget::HostsTransferMenu,
    );
}

fn export(app: &mut App) -> Task<Message> {
    let hosts = &mut app.hosts;
    let Some(dialog) = hosts.transfer_dialog.as_mut() else {
        return Task::none();
    };
    if dialog.busy {
        return Task::none();
    }
    dialog.error = None;

    if hosts.list.is_empty() {
        dialog.error = Some(t!("hosts.export_empty").to_string());
        return Task::none();
    }
    let password = dialog.password.clone();
    if password.expose_secret().trim().is_empty() {
        dialog.error = Some(t!("hosts.export_password_required").to_string());
        return Task::none();
    }
    dialog.busy = true;

    let json_bytes = match crate::hosts::export::hosts_to_export_json(&hosts.list) {
        Ok(bytes) => bytes,
        Err(error) => {
            dialog.busy = false;
            dialog.error = Some(error);
            return Task::none();
        }
    };

    // The scrypt KDF blocks for roughly 100-200 ms, so keep it off UI work.
    let title = t!("hosts.export_title").to_string();
    Task::perform(
        async move {
            let handle = host_transfer_file_dialog(title).save_file().await;
            let Some(handle) = handle else {
                return Ok::<Option<std::path::PathBuf>, String>(None);
            };
            let path = handle.path().to_path_buf();
            let encrypted = tokio::task::spawn_blocking(move || {
                crate::hosts::export::encrypt_export(&json_bytes, password.expose_secret())
            })
            .await
            .map_err(|error| format!("encrypt task failed: {error}"))??;
            std::fs::write(&path, encrypted)
                .map_err(|error| format!("failed to write file: {error}"))?;
            Ok(Some(path))
        },
        |result| Message::Hosts(crate::hosts::message::Message::HostExportSaved(result)),
    )
}

fn export_finished(
    app: &mut App,
    result: Result<Option<std::path::PathBuf>, String>,
) -> Task<Message> {
    if app.hosts.transfer_dialog.is_none() {
        return Task::none();
    }

    let mut completed_path = None;
    let mut error = None;
    match result {
        Ok(Some(path)) => completed_path = Some(path),
        Ok(None) => {}
        Err(message) => error = Some(message),
    }

    if let Some(dialog) = app.hosts.transfer_dialog.as_mut() {
        dialog.busy = false;
        if let Some(error) = error {
            dialog.error = Some(error);
        }
    }
    if let Some(path) = completed_path {
        app.hosts.transfer_dialog = None;
        let message = t!("hosts.export_success", path = path.display().to_string()).to_string();
        return Task::done(Message::ShowToast(message));
    }
    Task::none()
}

fn begin_import(app: &mut App) -> Task<Message> {
    close_transfer_menu(app);
    let title = t!("hosts.import_title").to_string();
    Task::perform(
        async move {
            host_transfer_file_dialog(title)
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        },
        |result| Message::Hosts(crate::hosts::message::Message::HostImportPicked(result)),
    )
}

fn decrypt_import(app: &mut App) -> Task<Message> {
    let Some(dialog) = app.hosts.transfer_dialog.as_mut() else {
        return Task::none();
    };
    if dialog.kind != HostTransferKind::ImportPassword || dialog.busy {
        return Task::none();
    }

    let password = dialog.password.clone();
    // Retain the bytes so a wrong password leaves the dialog usable.
    let bytes = dialog.pending_bytes.clone();
    dialog.busy = true;
    dialog.error = None;
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let plain = crate::hosts::export::decrypt_export(&bytes, password.expose_secret())?;
                let text = String::from_utf8(plain)
                    .map_err(|error| format!("decrypted data is not valid UTF-8: {error}"))?;
                crate::hosts::export::parse_export_json(&text)
            })
            .await
            .map_err(|error| format!("decrypt task failed: {error}"))?
        },
        |result| Message::Hosts(crate::hosts::message::Message::HostImportParsed(result)),
    )
}

fn import_parsed(app: &mut App, result: Result<Vec<ExportHost>, String>) -> Task<Message> {
    // Drop a late result if the password dialog was closed or replaced.
    let is_password_dialog = app
        .hosts
        .transfer_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.kind == HostTransferKind::ImportPassword);
    if !is_password_dialog {
        return Task::none();
    }

    match result {
        Ok(hosts) => present_import_result(app, hosts),
        Err(error) => {
            if let Some(dialog) = app.hosts.transfer_dialog.as_mut() {
                dialog.busy = false;
                dialog.error = Some(format!("{} ({error})", t!("hosts.import_wrong_password")));
            }
            Task::none()
        }
    }
}

fn confirm_import(app: &mut App) -> Task<Message> {
    if app
        .hosts
        .transfer_dialog
        .as_ref()
        .is_none_or(|dialog| dialog.busy)
    {
        return Task::none();
    }
    let (pending, new_count, duplicate_count) = match app.hosts.transfer_dialog.as_mut() {
        Some(dialog) => {
            let hosts = std::mem::take(&mut dialog.pending_hosts);
            (hosts, dialog.new_count, dialog.dup_count)
        }
        None => return Task::none(),
    };
    app.hosts.transfer_dialog = None;

    app.storage_task(
        move |storage| {
            let mut hosts = Vec::with_capacity(pending.len());
            let mut jump_references = Vec::new();
            for exported in pending {
                let jump_host_name = exported.jump_host_name.clone();
                let auth_type = match exported.auth_type.as_str() {
                    "Key" => AuthType::Key,
                    _ => AuthType::Password,
                };
                let host_id = storage.next_host_id()?;
                let host = Host {
                    id: host_id,
                    name: exported.name,
                    hostname: exported.hostname,
                    port: exported.port,
                    protocol: Protocol::Ssh,
                    username: exported.username,
                    auth_type,
                    jump_host_id: None,
                    identity_id: None,
                    key_path: exported.key_path,
                    key_passphrase: None,
                    // Migrating credentials is an explicit export behavior.
                    password: exported.password.map(SecretString::from),
                    os_type: OsType::from_str(&exported.os_type),
                    tags: exported.tags,
                    expected_fingerprint: None,
                };
                storage.upsert_host(&host)?;
                if let Some(jump_host_name) = jump_host_name {
                    jump_references.push((host_id, jump_host_name));
                }
                hosts.push(host);
            }
            for (host_id, jump_host_name) in jump_references {
                if let Some(jump_id) = hosts
                    .iter()
                    .find(|host| host.name == jump_host_name)
                    .map(|host| host.id)
                    && let Some(host) = hosts.iter_mut().find(|host| host.id == host_id)
                {
                    host.jump_host_id = Some(jump_id);
                    storage.upsert_host(host)?;
                }
            }
            Ok(hosts)
        },
        move |result| match result {
            Ok(hosts) => Message::Hosts(crate::hosts::message::Message::HostsImported {
                hosts,
                new_count,
                dup_count: duplicate_count,
            }),
            Err(error) => Message::ShowErrorToast(error),
        },
    )
}

fn imported(
    app: &mut App,
    hosts: Vec<Host>,
    new_count: usize,
    duplicate_count: usize,
) -> Task<Message> {
    for host in hosts {
        app.hosts.list.insert(0, host);
    }
    app.add_log(
        LogCategory::Host,
        format!("Imported {new_count} hosts (skipped {duplicate_count} duplicates)"),
    );
    let message = if duplicate_count > 0 {
        t!(
            "hosts.import_success_with_dups",
            new = new_count,
            dup = duplicate_count
        )
        .to_string()
    } else {
        t!("hosts.import_success", n = new_count).to_string()
    };
    Task::done(Message::ShowToast(message))
}

fn host_transfer_file_dialog(title: String) -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .set_file_name(crate::hosts::export::HOST_EXPORT_FILE_NAME)
        .add_filter(
            crate::hosts::export::HOST_EXPORT_FILTER_NAME,
            crate::hosts::export::HOST_IMPORT_EXTENSIONS,
        )
}

/// Deduplicate parsed hosts before opening the import confirmation.
pub(super) fn present_import_result(app: &mut App, hosts: Vec<ExportHost>) -> Task<Message> {
    let mut seen = HashSet::new();
    let mut new_count = 0usize;
    let mut duplicate_count = 0usize;
    let mut pending = Vec::new();

    for exported in hosts {
        let key = (
            exported.hostname.clone(),
            exported.port,
            exported.username.clone(),
        );
        let already_saved = app.hosts.list.iter().any(|host| {
            host.hostname == exported.hostname
                && host.port == exported.port
                && host.username == exported.username
        });
        if already_saved || !seen.insert(key) {
            duplicate_count += 1;
            continue;
        }
        new_count += 1;
        pending.push(exported);
    }

    if new_count == 0 {
        app.hosts.transfer_dialog = None;
        let message = t!("hosts.import_all_duplicates", n = duplicate_count).to_string();
        return Task::done(Message::ShowToast(message));
    }

    app.hosts.transfer_dialog = Some(HostTransferDialog::new_import_confirm(
        pending,
        new_count,
        duplicate_count,
    ));
    Task::none()
}
