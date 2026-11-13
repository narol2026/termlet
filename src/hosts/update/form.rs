//! Handles saved-host form editing and validation.

// Rust guideline compliant 2026-10-09

use secrecy::{ExposeSecret, SecretString};

use crate::app::App;
use crate::core::data::{
    AuthType, Host, HostId, HostTagFilter, LogCategory, OsType, Protocol, clean_tag,
};
use crate::core::forms::FormDraft;
use crate::core::message::Message;
use crate::hosts::message::Message as DomainMessage;
use iced::Task;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::ShowForm => {
            let was_visible = app.hosts.form.visible && !app.hosts.form_closing;
            app.hosts.cursor.clear();
            app.hosts.form.reset();
            if let HostTagFilter::Tag(name) = &app.hosts.tag_filter {
                app.hosts.form.tags.push(name.clone());
            }
            app.hosts.form.visible = true;
            app.hosts.form_anim = if was_visible { 1.0 } else { 0.0 };
            app.hosts.form_closing = false;
        }
        DomainMessage::HideForm => {
            crate::cursor::close_active_dropdown(app);
            app.hosts.form_closing = true;
        }
        DomainMessage::FormName(value) => app.hosts.form.name = value,
        DomainMessage::FormHostname(value) => app.hosts.form.hostname = value,
        DomainMessage::FormPort(value) => {
            // Only allow digits 1-65535; reject overflow and port 0.
            let filtered: String = value.chars().filter(char::is_ascii_digit).collect();
            if filtered.is_empty() || filtered.parse::<u16>().is_ok_and(|port| port >= 1) {
                app.hosts.form.port = filtered;
            }
        }
        DomainMessage::FormUsername(value) => {
            app.hosts.form.username = value;
            app.hosts.form.selected_identity_id = None;
        }
        DomainMessage::FormAuth(auth) => {
            app.hosts.form.auth = auth;
            // Identity credentials are username/password pairs and become stale
            // when the user switches to key authentication.
            if auth == AuthType::Key {
                app.hosts.form.selected_identity_id = None;
            }
        }
        DomainMessage::FormToggleJumpPicker => {
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostFormJump,
            );
        }
        DomainMessage::FormJumpSearch(search) => {
            app.hosts.form.jump_picker.set_query(search);
        }
        DomainMessage::FormSelectJump(id) => {
            app.hosts.form.jump_host_id = Some(id);
            crate::cursor::close_dropdown(app, crate::core::dropdown::DropdownTarget::HostFormJump);
        }
        DomainMessage::FormClearJump => {
            app.hosts.form.jump_host_id = None;
            crate::cursor::close_dropdown(app, crate::core::dropdown::DropdownTarget::HostFormJump);
        }
        DomainMessage::FormKeyPath(value) => app.hosts.form.key_path = value,
        DomainMessage::FormFillKeyPath(keychain_id) => {
            if let Some(entry) = app
                .keychain
                .list
                .iter()
                .find(|entry| entry.id == keychain_id)
            {
                app.hosts.form.key_path = entry.path.clone();
            }
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostFormKeychain,
            );
        }
        DomainMessage::FormPassword(value) => {
            app.hosts.form.password = value;
            app.hosts.form.selected_identity_id = None;
        }
        DomainMessage::FormTogglePasswordVisible => {
            app.hosts.form.password_visible = !app.hosts.form.password_visible;
        }
        DomainMessage::FormToggleIdentityPicker => {
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostFormIdentity,
            );
        }
        DomainMessage::FormIdentitySearch(search) => {
            app.hosts.form.identity_picker.set_query(search);
        }
        DomainMessage::FormSelectIdentity(id) => {
            if let Some(identity) = app.keychain.identities.iter().find(|item| item.id == id) {
                app.hosts.form.username = identity.username.clone();
                app.hosts.form.password = SecretString::from(identity.password_for_edit());
                app.hosts.form.selected_identity_id = Some(id);
            }
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostFormIdentity,
            );
        }
        DomainMessage::FormClearIdentity => {
            app.hosts.form.selected_identity_id = None;
            crate::cursor::close_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostFormIdentity,
            );
            // Editing restores the snapshot the form opened with; a new host
            // starts blank.
            if let Some(original) = &app.hosts.form.original {
                app.hosts.form.username = original.username.clone();
                app.hosts.form.password = original.password_for_edit().into();
            } else {
                app.hosts.form.username.clear();
                app.hosts.form.password = SecretString::from(String::new());
            }
        }
        DomainMessage::FormToggleKeychainPicker => {
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostFormKeychain,
            );
        }
        DomainMessage::FormKeychainSearch(search) => {
            app.hosts.form.keychain_picker.set_query(search);
        }
        DomainMessage::SaveHost => return save(app),
        DomainMessage::FormTagPickerToggle => {
            return crate::cursor::toggle_dropdown(
                app,
                crate::core::dropdown::DropdownTarget::HostFormTag,
            );
        }
        DomainMessage::FormTagSearch(search) => {
            app.hosts.form.tag_picker.set_query(search);
        }
        DomainMessage::FormTagAdd(raw) => {
            add_form_tag(app, &raw);
            crate::cursor::close_dropdown(app, crate::core::dropdown::DropdownTarget::HostFormTag);
        }
        DomainMessage::FormTagCreate => {
            let raw = app.hosts.form.tag_picker.query().to_string();
            if add_form_tag(app, &raw) {
                crate::cursor::close_dropdown(
                    app,
                    crate::core::dropdown::DropdownTarget::HostFormTag,
                );
            }
        }
        DomainMessage::FormTagRemove(name) => {
            app.hosts.form.tags.retain(|tag| tag != &name);
        }
        _ => {}
    }
    Task::none()
}

/// Fill the form fields shared by click-to-edit and explicit edit actions.
pub(super) fn select_host(app: &mut App, id: HostId) {
    fill_common_fields(app, id);
}

/// Fill the full editor state, including a still-valid identity selection.
pub(super) fn edit_host(app: &mut App, id: HostId) {
    if !fill_common_fields(app, id) {
        return;
    }
    app.hosts.form.selected_identity_id = app
        .hosts
        .form
        .original
        .as_ref()
        .and_then(|host| host.identity_id)
        .filter(|identity_id| {
            app.keychain
                .identities
                .iter()
                .any(|identity| identity.id == *identity_id)
        });
}

fn fill_common_fields(app: &mut App, id: HostId) -> bool {
    let Some(host) = app.hosts.list.iter().find(|host| host.id == id).cloned() else {
        return false;
    };

    app.hosts.form.name.clone_from(&host.name);
    app.hosts.form.hostname.clone_from(&host.hostname);
    app.hosts.form.port = host.port.to_string();
    app.hosts.form.username.clone_from(&host.username);
    app.hosts.form.auth = host.auth_type;
    app.hosts.form.jump_host_id = host.jump_host_id;
    app.hosts.form.key_path = host.key_path.clone().unwrap_or_default();
    app.hosts.form.password = host.password_for_edit().into();
    app.hosts.form.tags.clone_from(&host.tags);
    crate::cursor::close_dropdown(app, crate::core::dropdown::DropdownTarget::HostFormTag);
    app.hosts.form.editing_id = Some(id);
    app.hosts.form.original = Some(host);
    true
}

fn save(app: &mut App) -> Task<Message> {
    if !app.hosts.form.save_enabled() {
        return Task::none();
    }

    let port = app.hosts.form.port.parse().unwrap_or(22);
    let key_path = key_path_for_auth(app.hosts.form.auth, &app.hosts.form.key_path);
    let password = password_for_auth(app.hosts.form.auth, &app.hosts.form.password);

    if let Some(editing_id) = app.hosts.form.editing_id {
        return save_existing(app, editing_id, port, key_path.as_ref(), password.as_ref());
    }
    save_new(app, port, key_path, password)
}

fn save_existing(
    app: &mut App,
    editing_id: HostId,
    port: u16,
    key_path: Option<&String>,
    password: Option<&SecretString>,
) -> Task<Message> {
    let mut host_to_save = None;
    let name = if app.hosts.form.name.is_empty() {
        &app.hosts.form.hostname
    } else {
        &app.hosts.form.name
    };
    app.add_log(LogCategory::Host, format!("Updated host \"{name}\""));

    if let Some(host) = app.hosts.list.iter_mut().find(|host| host.id == editing_id) {
        host.name.clone_from(&app.hosts.form.name);
        host.hostname.clone_from(&app.hosts.form.hostname);
        host.port = port;
        host.username.clone_from(&app.hosts.form.username);
        host.auth_type = app.hosts.form.auth;
        host.jump_host_id = app.hosts.form.jump_host_id;
        host.identity_id = app.hosts.form.selected_identity_id;
        host.key_path = key_path.cloned();
        host.password = password.cloned();
        host.tags.clone_from(&app.hosts.form.tags);
        host_to_save = Some(host.clone());
    }

    // A retry must dial the edited configuration, not a cached connection.
    for draft in app.terminal.drafts.values_mut() {
        if let Some(pending) = draft.ssh.attempt.pending_host.as_mut()
            && pending.id == editing_id
        {
            pending.name.clone_from(&app.hosts.form.name);
            pending.hostname.clone_from(&app.hosts.form.hostname);
            pending.port = port;
            pending.username.clone_from(&app.hosts.form.username);
            pending.auth_type = app.hosts.form.auth;
            pending.jump_host_id = app.hosts.form.jump_host_id;
            pending.identity_id = app.hosts.form.selected_identity_id;
            pending.key_path = key_path.cloned();
            pending.password = password.cloned();
            pending.tags.clone_from(&app.hosts.form.tags);
        }
    }

    app.hosts.record_recent(editing_id);
    app.hosts.form.visible = false;
    app.hosts.form.reset();
    app.needs_scroll_restore = true;
    let recent_ids = app.hosts.recent_ids.clone();
    app.storage_write_task(move |storage| {
        if let Some(host) = &host_to_save {
            storage.upsert_host(host)?;
        }
        storage.save_recent_ids(&recent_ids)
    })
}

fn save_new(
    app: &mut App,
    port: u16,
    key_path: Option<String>,
    password: Option<SecretString>,
) -> Task<Message> {
    let name = if app.hosts.form.name.is_empty() {
        app.hosts.form.hostname.clone()
    } else {
        app.hosts.form.name.clone()
    };
    app.add_log(LogCategory::Host, format!("Created host \"{name}\""));

    let hostname = app.hosts.form.hostname.clone();
    let username = app.hosts.form.username.clone();
    let auth_type = app.hosts.form.auth;
    let jump_host_id = app.hosts.form.jump_host_id;
    let identity_id = app.hosts.form.selected_identity_id;
    let tags = app.hosts.form.tags.clone();
    let recent_ids = app.hosts.recent_ids.clone();
    app.hosts.form.visible = false;
    app.hosts.form.reset();
    app.needs_scroll_restore = true;

    app.storage_task(
        move |storage| {
            let host_id = storage.next_host_id()?;
            let host = Host {
                id: host_id,
                name,
                hostname,
                port,
                protocol: Protocol::Ssh,
                username,
                auth_type,
                jump_host_id,
                identity_id,
                key_path,
                key_passphrase: None,
                password,
                os_type: OsType::Unknown,
                tags,
                expected_fingerprint: None,
            };
            storage.upsert_host(&host)?;
            storage.save_recent_ids(&recent_ids)?;
            Ok(host)
        },
        |result| match result {
            Ok(host) => Message::Hosts(crate::hosts::message::Message::HostCreated { host }),
            Err(error) => Message::ShowErrorToast(error),
        },
    )
}

pub(super) fn key_path_for_auth(auth_type: AuthType, key_path: &str) -> Option<String> {
    (auth_type == AuthType::Key && !key_path.is_empty()).then(|| key_path.to_string())
}

pub(super) fn password_for_auth(
    auth_type: AuthType,
    password: &SecretString,
) -> Option<SecretString> {
    (auth_type == AuthType::Password && !password.expose_secret().is_empty())
        .then(|| password.clone())
}

/// Add a trimmed, case-sensitive, deduplicated tag to the host form.
pub(super) fn add_form_tag(app: &mut App, raw: &str) -> bool {
    let Some(name) = clean_tag(raw) else {
        return false;
    };
    if app.hosts.form.tags.iter().any(|tag| tag == &name) {
        return false;
    }
    app.hosts.form.tags.push(name);
    true
}
