#[cfg(test)]
use secrecy::ExposeSecret;
use secrecy::SecretString;

use super::message::Message as DomainMessage;
use crate::app::App;
use crate::core::data::{Identity, KeyType, KeychainEntry, LogCategory};
use crate::core::diagnostics::Level;
use crate::core::forms::FormDraft;
use crate::core::message::Message;

use crate::update::restore_scrolls;
use rust_i18n::t;

use super::state::{GeneratedKeyData, KeychainContextMenu, KeychainSelection};

pub fn update(app: &mut App, msg: DomainMessage) -> iced::Task<Message> {
    let _guard = crate::core::diagnostics::perf_guard(
        Level::Debug,
        "UPDATE",
        "keychain::update",
        std::time::Duration::from_millis(16),
    );
    match msg {
        DomainMessage::KeychainToggleNewMenu => {
            app.keychain.show_new_menu = !app.keychain.show_new_menu;
            iced::Task::none()
        }
        DomainMessage::KeychainShowForm => {
            app.keychain.show_new_menu = false;
            app.keychain.cursor.clear();
            let was_visible = (app.keychain.form.visible || app.keychain.identity_form.visible)
                && !app.keychain.form_closing;
            app.keychain.identity_form.visible = false;
            app.needs_scroll_restore = true;
            app.keychain.form.visible = true;
            app.keychain.form_anim = if was_visible { 1.0 } else { 0.0 };
            app.keychain.form_closing = false;
            app.keychain.form.reset();
            let default_kt = KeyType::default();
            app.keychain.form.selected_key_type = Some(default_kt);
            app.keychain.form.path = default_key_path("", default_kt);
            iced::Task::none()
        }
        DomainMessage::KeychainHideForm => {
            app.keychain.form_closing = true;
            iced::Task::none()
        }
        DomainMessage::KeychainEdit(id) => {
            app.keychain.context_menu = None;
            open_key_editor(app, id);
            iced::Task::none()
        }
        DomainMessage::KeychainFormName(s) => {
            app.keychain.form.name = s;
            if !app.keychain.form.path_manual {
                let kt = app
                    .keychain
                    .form
                    .selected_key_type
                    .unwrap_or(KeyType::ED25519);
                app.keychain.form.path = default_key_path(&app.keychain.form.name, kt);
            }
            iced::Task::none()
        }
        DomainMessage::KeychainFormKeyType(kt) => {
            app.keychain.form.selected_key_type = Some(kt);
            app.keychain.form.path_manual = false;
            app.keychain.form.path = default_key_path(&app.keychain.form.name, kt);
            iced::Task::none()
        }
        DomainMessage::KeychainFormPassphrase(s) => {
            app.keychain.form.passphrase = s;
            iced::Task::none()
        }
        DomainMessage::KeychainFormTogglePasswordVisible => {
            app.keychain.form.passphrase_visible = !app.keychain.form.passphrase_visible;
            iced::Task::none()
        }
        DomainMessage::KeychainFormFingerprint(s) => {
            app.keychain.form.fingerprint = s;
            iced::Task::none()
        }
        DomainMessage::KeychainFormPublicKey(s) => {
            app.keychain.form.public_key = s;
            iced::Task::none()
        }
        DomainMessage::KeychainFormPath(s) => {
            app.keychain.form.path = s;
            app.keychain.form.path_manual = true;
            iced::Task::none()
        }
        DomainMessage::KeychainFormSubmit => {
            let is_edit = app.keychain.form.editing_id.is_some();
            if app.keychain.form.in_progress {
                return iced::Task::none();
            }
            if !app.keychain.form.save_enabled() {
                return iced::Task::none();
            }

            if !is_edit {
                let kt = app
                    .keychain
                    .form
                    .selected_key_type
                    .unwrap_or(KeyType::ED25519);
                let key_type_display = kt.ssh_key_type().to_string();
                let kind_label = kt.label().to_string();
                let path = crate::core::paths::expand_home(&app.keychain.form.path);
                let passphrase = app.keychain.form.passphrase.clone();
                let name = app.keychain.form.name.clone();
                let comment = if app.keychain.form.name.trim().is_empty() {
                    t!("keychain.default_key_name").to_string()
                } else {
                    app.keychain.form.name.trim().to_string()
                };

                // Fail before the expensive key-generation step. The writer
                // also uses create_new so a race cannot replace either file.
                if std::path::Path::new(&path).exists() {
                    let msg = t!("keychain.key_exists", path = path.as_str()).to_string();
                    app.add_log(LogCategory::Keychain, msg.clone());
                    return iced::Task::done(Message::ShowErrorToast(msg));
                }
                let pub_path = format!("{path}.pub");
                if std::path::Path::new(&pub_path).exists() {
                    let msg =
                        t!("keychain.pubkey_exists", pub_path = pub_path.as_str()).to_string();
                    app.add_log(LogCategory::Keychain, msg.clone());
                    return iced::Task::done(Message::ShowErrorToast(msg));
                }

                app.keychain.form.in_progress = true;

                let task = iced::Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || -> Result<GeneratedKeyData, String> {
                            let generated = crate::keychain::keygen::generate_keypair(
                                std::path::Path::new(&path),
                                kt,
                                &passphrase,
                                &comment,
                            )?;
                            Ok(GeneratedKeyData {
                                key_type_display,
                                kind_label,
                                name,
                                path,
                                fingerprint: generated.fingerprint,
                                public_key: generated.public_key,
                            })
                        })
                        .await
                        .unwrap_or_else(|e| Err(format!("generation task failed: {e}")))
                    },
                    |v0| {
                        Message::Keychain(crate::keychain::message::Message::KeychainFormGenerated(
                            v0,
                        ))
                    },
                );
                return task;
            }

            // Edit mode: fast synchronous save.
            let Some(id) = app.keychain.form.editing_id else {
                return iced::Task::done(Message::ShowErrorToast(
                    "Missing keychain entry id while editing".to_string(),
                ));
            };
            let entry = KeychainEntry {
                id,
                name: app.keychain.form.name.clone(),
                key_type: app.keychain.form.key_type.clone(),
                fingerprint: app.keychain.form.fingerprint.clone(),
                public_key: app.keychain.form.public_key.clone(),
                path: app.keychain.form.path.clone(),
                created_at: String::new(),
            };

            let save = app.storage_write_task({
                let entry = entry.clone();
                move |s| s.upsert_keychain_entry(&entry)
            });

            if let Some(existing) = app.keychain.list.iter_mut().find(|e| e.id == id) {
                *existing = entry.clone();
            }
            app.add_log(
                LogCategory::Keychain,
                t!("keychain.updated_key", name = entry.name.as_str()),
            );

            app.keychain.form.visible = false;
            app.keychain.form.reset();
            app.needs_scroll_restore = true;
            save
        }
        DomainMessage::KeychainFormGenerated(result) => {
            app.keychain.form.in_progress = false;
            match result {
                Ok(data) => {
                    let name = data.name.clone();
                    let kind_label = data.kind_label.clone();
                    app.add_log(
                        LogCategory::Keychain,
                        t!(
                            "keychain.generated",
                            name = name.as_str(),
                            kind = kind_label.as_str()
                        ),
                    );

                    app.keychain.form.visible = false;
                    app.keychain.form.reset();
                    app.needs_scroll_restore = true;
                    app.storage_task(
                        move |s| {
                            let entry_id = s.next_keychain_entry_id()?;
                            let entry = KeychainEntry {
                                id: entry_id,
                                name: data.name,
                                key_type: data.key_type_display,
                                fingerprint: data.fingerprint,
                                public_key: data.public_key,
                                path: data.path,
                                created_at: String::new(),
                            };
                            s.upsert_keychain_entry(&entry)?;
                            Ok(entry)
                        },
                        |result| match result {
                            Ok(entry) => Message::Keychain(
                                crate::keychain::message::Message::KeychainEntryCreated { entry },
                            ),
                            Err(e) => Message::ShowErrorToast(e),
                        },
                    )
                }
                Err(error) => {
                    let msg = t!("keychain.gen_failed", msg = error.as_str()).to_string();
                    app.add_log(LogCategory::Keychain, msg.clone());
                    iced::Task::done(Message::ShowErrorToast(msg))
                }
            }
        }
        DomainMessage::KeychainEntryCreated { entry } => {
            app.keychain.list.push(entry);
            iced::Task::none()
        }
        DomainMessage::KeychainDeleteRequest(id) => {
            app.keychain.context_menu = None;
            app.keychain.delete_pending = Some(id);
            restore_scrolls(app)
        }
        DomainMessage::KeychainDeleteCancel => {
            app.keychain.delete_pending = None;
            restore_scrolls(app)
        }
        DomainMessage::KeychainDelete(id) => {
            let name = app
                .keychain
                .list
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.name.clone())
                .unwrap_or_default();
            app.keychain.list.retain(|e| e.id != id);
            if app.keychain.cursor.selected_key() == Some(&KeychainSelection::Key(id)) {
                app.keychain.cursor.clear();
            }
            app.keychain.delete_pending = None;
            if app.keychain.form.editing_id == Some(id) {
                app.keychain.form.visible = false;
                app.keychain.form.reset();
                app.needs_scroll_restore = true;
            }
            app.add_log(
                LogCategory::Keychain,
                t!("keychain.deleted_key", name = name.as_str()),
            );
            let save = app.storage_task(
                move |s| s.delete_keychain_entry(id),
                |result| match result {
                    Ok(()) => Message::Noop,
                    Err(e) => Message::ShowErrorToast(e),
                },
            );
            iced::Task::batch([save, restore_scrolls(app)])
        }
        DomainMessage::KeychainSearchChanged(query) => {
            app.keychain.search_query = query;
            iced::Task::none()
        }
        DomainMessage::KeychainToggleView => {
            app.keychain.grid_view = !app.keychain.grid_view;
            let value = if app.keychain.grid_view {
                "true"
            } else {
                "false"
            };
            app.storage_write_task(move |s| s.save_preference("keychain_grid_view", value))
        }
        DomainMessage::KeychainSelect(id) => {
            activate_key_entry(app, id);
            iced::Task::none()
        }
        DomainMessage::KeychainHoverEnter(id) => {
            app.keychain.hovered_id = Some(id);
            iced::Task::none()
        }
        DomainMessage::KeychainHoverLeave(id) => {
            if app.keychain.hovered_id == Some(id) {
                app.keychain.hovered_id = None;
            }
            iced::Task::none()
        }
        DomainMessage::KeychainContextMenuOpen(id) => {
            activate_key_entry(app, id);
            app.keychain.context_menu = Some(KeychainContextMenu::Key {
                id,
                position: app.window.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::KeychainEdit,
                    crate::core::context_menu::ContextMenuAction::KeychainDelete,
                ]),
            });
            iced::Task::none()
        }
        DomainMessage::KeychainContextMenuClose => {
            app.keychain.context_menu = None;
            iced::Task::none()
        }
        DomainMessage::KeychainViewCursorMoved(pos) => {
            app.keychain.cursor_pos = pos;
            iced::Task::none()
        }
        DomainMessage::IdentityShowForm => {
            app.keychain.show_new_menu = false;
            app.keychain.cursor.clear();
            let was_visible = (app.keychain.form.visible || app.keychain.identity_form.visible)
                && !app.keychain.form_closing;
            app.keychain.form.visible = false;
            app.needs_scroll_restore = true;
            app.keychain.identity_form.visible = true;
            app.keychain.form_anim = if was_visible { 1.0 } else { 0.0 };
            app.keychain.form_closing = false;
            app.keychain.identity_form.editing_id = None;
            app.keychain.identity_form.reset();
            iced::Task::none()
        }
        DomainMessage::IdentityHideForm => {
            app.keychain.identity_form.visible = false;
            app.keychain.identity_form.reset();
            app.needs_scroll_restore = true;
            iced::Task::none()
        }
        DomainMessage::IdentityEdit(id) => {
            app.keychain.context_menu = None;
            open_identity_editor(app, id);
            iced::Task::none()
        }
        DomainMessage::IdentityDeleteRequest(id) => {
            app.keychain.context_menu = None;
            app.keychain.identity_delete_pending = Some(id);
            restore_scrolls(app)
        }
        DomainMessage::IdentityDeleteCancel => {
            app.keychain.identity_delete_pending = None;
            restore_scrolls(app)
        }
        DomainMessage::IdentityDelete(id) => {
            let name = app
                .keychain
                .identities
                .iter()
                .find(|i| i.id == id)
                .map(|i| i.label.clone())
                .unwrap_or_default();
            app.keychain.identities.retain(|i| i.id != id);
            if app.keychain.cursor.selected_key() == Some(&KeychainSelection::Identity(id)) {
                app.keychain.cursor.clear();
            }
            app.keychain.identity_delete_pending = None;
            if app.keychain.identity_form.editing_id == Some(id) {
                app.keychain.identity_form.visible = false;
                app.keychain.identity_form.reset();
                app.needs_scroll_restore = true;
            }
            app.add_log(
                LogCategory::Keychain,
                t!("keychain.deleted_identity", name = name.as_str()),
            );
            let save = app.storage_task(
                move |s| s.delete_identity(id),
                |result| match result {
                    Ok(()) => Message::Noop,
                    Err(e) => Message::ShowErrorToast(e),
                },
            );
            iced::Task::batch([save, restore_scrolls(app)])
        }
        DomainMessage::IdentityFormLabel(s) => {
            app.keychain.identity_form.label = s;
            iced::Task::none()
        }
        DomainMessage::IdentityFormUsername(s) => {
            app.keychain.identity_form.username = s;
            iced::Task::none()
        }
        DomainMessage::IdentityFormPassword(s) => {
            app.keychain.identity_form.password = s;
            iced::Task::none()
        }
        DomainMessage::IdentityFormTogglePasswordVisible => {
            app.keychain.identity_form.password_visible =
                !app.keychain.identity_form.password_visible;
            iced::Task::none()
        }
        DomainMessage::IdentityFormSubmit => {
            let form = &app.keychain.identity_form;
            if !form.save_enabled() {
                return iced::Task::none();
            }
            let label = form.label.clone();
            let username = form.username.clone();
            let password = form.password.clone();

            if let Some(id) = form.editing_id {
                let entry = Identity {
                    id,
                    label: label.clone(),
                    username,
                    password,
                    created_at: String::new(),
                };
                if let Some(existing) = app.keychain.identities.iter_mut().find(|i| i.id == id) {
                    *existing = entry.clone();
                }
                app.add_log(
                    LogCategory::Keychain,
                    t!("keychain.updated_identity", name = entry.label.as_str()),
                );
                app.keychain.identity_form.visible = false;
                app.keychain.identity_form.reset();
                app.needs_scroll_restore = true;
                return app.storage_write_task(move |s| s.upsert_identity(&entry));
            }

            app.add_log(
                LogCategory::Keychain,
                t!("keychain.created_identity", name = label.as_str()),
            );
            app.keychain.identity_form.visible = false;
            app.keychain.identity_form.reset();
            app.needs_scroll_restore = true;
            app.storage_task(
                move |s| {
                    let id = s.next_identity_id()?;
                    let entry = Identity {
                        id,
                        label,
                        username,
                        password,
                        created_at: String::new(),
                    };
                    s.upsert_identity(&entry)?;
                    Ok(entry)
                },
                |result| match result {
                    Ok(identity) => {
                        Message::Keychain(crate::keychain::message::Message::IdentityCreated {
                            identity,
                        })
                    }
                    Err(e) => Message::ShowErrorToast(e),
                },
            )
        }
        DomainMessage::IdentityCreated { identity } => {
            app.keychain.identities.push(identity);
            iced::Task::none()
        }
        DomainMessage::IdentityHoverEnter(id) => {
            app.keychain.hovered_identity_id = Some(id);
            iced::Task::none()
        }
        DomainMessage::IdentitySelect(id) => {
            activate_identity_entry(app, id);
            iced::Task::none()
        }
        DomainMessage::IdentityHoverLeave(id) => {
            if app.keychain.hovered_identity_id == Some(id) {
                app.keychain.hovered_identity_id = None;
            }
            iced::Task::none()
        }
        DomainMessage::IdentityContextMenuOpen(id) => {
            activate_identity_entry(app, id);
            app.keychain.context_menu = Some(KeychainContextMenu::Identity {
                id,
                position: app.window.cursor_pos,
                cursor: crate::context_menu::initial_cursor(&[
                    crate::core::context_menu::ContextMenuAction::IdentityEdit,
                    crate::core::context_menu::ContextMenuAction::IdentityDelete,
                ]),
            });
            iced::Task::none()
        }
        DomainMessage::IdentityContextMenuClose => {
            app.keychain.context_menu = None;
            iced::Task::none()
        }
    }
}

fn activate_key_entry(app: &mut App, id: crate::core::data::KeychainEntryId) {
    if app.keychain.form.visible || app.keychain.identity_form.visible {
        open_key_editor(app, id);
    } else {
        app.keychain.select_key(id);
    }
}

fn activate_identity_entry(app: &mut App, id: crate::core::data::IdentityId) {
    if app.keychain.form.visible || app.keychain.identity_form.visible {
        open_identity_editor(app, id);
    } else {
        app.keychain.select_identity(id);
    }
}

fn open_key_editor(app: &mut App, id: crate::core::data::KeychainEntryId) {
    app.keychain.select_key(id);
    let was_visible = (app.keychain.form.visible || app.keychain.identity_form.visible)
        && !app.keychain.form_closing;
    let Some(entry) = app.keychain.list.iter().find(|e| e.id == id).cloned() else {
        return;
    };
    app.keychain.identity_form.visible = false;
    app.keychain.form.visible = true;
    app.keychain.form_anim = if was_visible { 1.0 } else { 0.0 };
    app.keychain.form_closing = false;
    app.keychain.form.editing_id = Some(id);
    app.keychain.form.original = Some(entry.clone());
    app.keychain.form.name.clone_from(&entry.name);
    app.keychain.form.key_type.clone_from(&entry.key_type);
    app.keychain.form.selected_key_type = None;
    app.keychain.form.passphrase = SecretString::from(String::new());
    app.keychain.form.fingerprint.clone_from(&entry.fingerprint);
    app.keychain.form.public_key.clone_from(&entry.public_key);
    app.keychain.form.path.clone_from(&entry.path);
    app.keychain.form.path_manual = true;
}

fn open_identity_editor(app: &mut App, id: crate::core::data::IdentityId) {
    app.keychain.select_identity(id);
    let was_visible = (app.keychain.form.visible || app.keychain.identity_form.visible)
        && !app.keychain.form_closing;
    let Some(entry) = app.keychain.identities.iter().find(|i| i.id == id).cloned() else {
        return;
    };
    app.keychain.form.visible = false;
    app.keychain.identity_form.visible = true;
    app.keychain.form_anim = if was_visible { 1.0 } else { 0.0 };
    app.keychain.form_closing = false;
    app.keychain.identity_form.editing_id = Some(id);
    app.keychain.identity_form.original = Some(entry.clone());
    app.keychain.identity_form.label.clone_from(&entry.label);
    app.keychain
        .identity_form
        .username
        .clone_from(&entry.username);
    app.keychain.identity_form.password = entry.password_for_edit().into();
}

/// Derive a filename-safe suffix from the human-readable key name.
///
/// Alphanumeric characters are kept (lowercased), everything else collapses
/// into a single `-`. An empty/unslugable name falls back to `key`.
fn slugify_name(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut prev_sep = false;
    for ch in name.trim().chars() {
        if ch.is_alphanumeric() {
            slug.extend(ch.to_lowercase());
            prev_sep = false;
        } else if !prev_sep {
            slug.push('-');
            prev_sep = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "key".to_string()
    } else {
        slug.to_string()
    }
}

/// Build the suggested private-key path for a key type, appending the name
/// slug when one is present (`<home>/.ssh/id_ed25519_my-github-key`).
fn default_key_path(name: &str, kt: KeyType) -> String {
    let file_name = if name.trim().is_empty() {
        format!("id_{}", kt.ssh_type())
    } else {
        format!("id_{}_{}", kt.ssh_type(), slugify_name(name))
    };
    crate::core::paths::home_dir()
        .join(".ssh")
        .join(file_name)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::grid_cursor::GridDirection;

    #[test]
    fn slugify_lowercases_and_separates() {
        assert_eq!(slugify_name("My GitHub Key"), "my-github-key");
        assert_eq!(slugify_name("  repo/prod  "), "repo-prod");
    }

    #[test]
    fn slugify_keeps_unicode_alphanumeric() {
        assert_eq!(slugify_name("生产密钥"), "生产密钥");
    }

    #[test]
    fn slugify_falls_back_for_empty_input() {
        assert_eq!(slugify_name(""), "key");
        assert_eq!(slugify_name("  "), "key");
        assert_eq!(slugify_name("***"), "key");
    }

    #[test]
    fn key_and_identity_selection_are_mutually_exclusive() {
        let mut app = crate::core::test_support::app();
        app.keychain.list = vec![entry(1, "deploy")];
        app.keychain.identities = vec![identity(1, "prod")];

        let _ = update(&mut app, DomainMessage::KeychainSelect(KeychainEntryId(1)));
        assert_eq!(app.keychain.selected_key(), Some(KeychainEntryId(1)));
        assert_eq!(app.keychain.selected_identity(), None);

        let _ = update(&mut app, DomainMessage::IdentitySelect(IdentityId(1)));
        assert_eq!(app.keychain.selected_key(), None);
        assert_eq!(app.keychain.selected_identity(), Some(IdentityId(1)));

        let _ = update(&mut app, DomainMessage::KeychainSelect(KeychainEntryId(1)));
        assert_eq!(app.keychain.selected_key(), Some(KeychainEntryId(1)));
        assert_eq!(app.keychain.selected_identity(), None);
    }

    #[test]
    fn selecting_across_sections_rebinds_the_open_editor() {
        let mut app = crate::core::test_support::app();
        app.keychain.list = vec![entry(1, "deploy")];
        app.keychain.identities = vec![identity(1, "prod")];

        let _ = update(&mut app, DomainMessage::KeychainEdit(KeychainEntryId(1)));
        assert!(app.keychain.form.visible);
        assert!(!app.keychain.identity_form.visible);

        let _ = update(&mut app, DomainMessage::IdentitySelect(IdentityId(1)));
        assert_eq!(app.keychain.selected_identity(), Some(IdentityId(1)));
        assert!(!app.keychain.form.visible);
        assert!(app.keychain.identity_form.visible);
        assert_eq!(app.keychain.identity_form.editing_id, Some(IdentityId(1)));

        let _ = update(&mut app, DomainMessage::KeychainSelect(KeychainEntryId(1)));
        assert_eq!(app.keychain.selected_key(), Some(KeychainEntryId(1)));
        assert!(app.keychain.form.visible);
        assert!(!app.keychain.identity_form.visible);
        assert_eq!(app.keychain.form.editing_id, Some(KeychainEntryId(1)));
    }

    #[test]
    fn cursor_moves_within_the_active_keychain_section() {
        let mut app = crate::core::test_support::app();
        app.keychain.list = vec![entry(1, "one"), entry(2, "two")];
        app.keychain.identities = vec![identity(1, "prod")];
        app.keychain.grid_columns = 2;

        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::Keychain,
            GridDirection::Down,
        );
        assert_eq!(app.keychain.selected_key(), Some(KeychainEntryId(1)));
        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::Keychain,
            GridDirection::Down,
        );
        assert_eq!(
            app.keychain.selected_identity(),
            Some(IdentityId(1)),
            "down at the key-grid edge enters Identities"
        );
        let _ = crate::cursor::move_cursor(
            &mut app,
            crate::core::cursor_focus::CursorTarget::Keychain,
            GridDirection::Up,
        );
        assert_eq!(
            app.keychain.selected_key(),
            Some(KeychainEntryId(2)),
            "up from Identities returns to the last key"
        );
    }

    #[test]
    fn the_suggested_key_path_names_the_key_type_and_the_entry() {
        let ssh_dir = crate::core::paths::home_dir().join(".ssh");
        let expected = |file_name: &str| ssh_dir.join(file_name).to_string_lossy().into_owned();

        assert_eq!(
            default_key_path("GitHub", KeyType::ED25519),
            expected("id_ed25519_github")
        );
        assert_eq!(
            default_key_path("prod bastion", KeyType::RSA),
            expected("id_rsa_prod-bastion")
        );
        // An unnamed key keeps ssh's own default filename so it can be picked
        // up without an `-i` flag.
        assert_eq!(
            default_key_path("   ", KeyType::ECDSA),
            expected("id_ecdsa")
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_suggested_key_path_uses_userprofile_when_home_is_unset() {
        const CHILD_MARKER: &str = "TERMLET_KEY_PATH_HOME_UNSET_CHILD";

        if std::env::var_os(CHILD_MARKER).is_some() {
            let profile = std::path::PathBuf::from(
                std::env::var_os("USERPROFILE").expect("Windows sets USERPROFILE"),
            );
            let suggested = std::path::PathBuf::from(default_key_path("", KeyType::ED25519));
            let ssh_dir = profile.join(".ssh");

            assert!(
                suggested.starts_with(&profile),
                "the suggestion escaped the Windows profile: {}",
                suggested.display()
            );
            assert_eq!(suggested.parent(), Some(ssh_dir.as_path()));
            return;
        }

        let test_name = concat!(
            "keychain::update::tests::",
            "the_suggested_key_path_uses_userprofile_when_home_is_unset"
        );
        let output =
            std::process::Command::new(std::env::current_exe().expect("the test executable"))
                .args(["--exact", test_name, "--nocapture"])
                .env_remove("HOME")
                .env(CHILD_MARKER, "1")
                .output()
                .expect("run the environment-isolated assertion");

        assert!(
            output.status.success(),
            "the child test failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Message flows

    use crate::core::data::{HostId, IdentityId, KeychainEntryId};
    use crate::core::test_support::app;
    use crate::keychain::message::Message as DomainMessage;

    fn entry(id: u32, name: &str) -> KeychainEntry {
        KeychainEntry {
            id: KeychainEntryId(id),
            name: name.to_string(),
            key_type: "ED25519".to_string(),
            fingerprint: "SHA256:abc".to_string(),
            public_key: "ssh-ed25519 AAAA".to_string(),
            path: format!("~/.ssh/{name}"),
            created_at: String::new(),
        }
    }

    fn identity(id: u32, label: &str) -> Identity {
        Identity {
            id: IdentityId(id),
            label: label.to_string(),
            username: "deploy".to_string(),
            password: SecretString::from("hunter2"),
            created_at: String::new(),
        }
    }

    #[test]
    fn the_new_key_form_starts_blank_but_ready_to_generate() {
        let mut app = app();
        app.keychain.form.name = "left over".into();

        let _ = update(&mut app, DomainMessage::KeychainShowForm);

        assert!(app.keychain.form.visible);
        assert!(
            app.keychain.form.name.is_empty(),
            "a new key starts unnamed"
        );
        assert_eq!(app.keychain.form.editing_id, None);
        let key_type = app
            .keychain
            .form
            .selected_key_type
            .expect("the form needs a type before it can generate anything");
        assert_eq!(
            app.keychain.form.path,
            default_key_path("", key_type),
            "an unnamed key is offered ssh's own default path"
        );
        assert!(
            !app.keychain.show_new_menu,
            "the menu closes behind the form it opened"
        );
    }

    #[test]
    fn typing_a_name_follows_into_the_path_until_the_path_is_edited_by_hand() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::KeychainShowForm);
        let suggested_type = app.keychain.form.selected_key_type.unwrap();

        let _ = update(&mut app, DomainMessage::KeychainFormName("GitHub".into()));
        let suggested = app.keychain.form.path.clone();
        assert!(
            suggested.ends_with("github"),
            "the suggestion follows the name: {suggested}"
        );
        assert!(suggested.contains(suggested_type.ssh_type()));
        assert!(!app.keychain.form.path_manual);

        // Once the path is typed by hand it stops following the name.
        let _ = update(
            &mut app,
            DomainMessage::KeychainFormPath("/tmp/my-key".into()),
        );
        assert!(app.keychain.form.path_manual);
        let _ = update(&mut app, DomainMessage::KeychainFormName("Other".into()));
        assert_eq!(app.keychain.form.path, "/tmp/my-key");
    }

    #[test]
    fn changing_the_key_type_re_suggests_the_path_for_the_new_type() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::KeychainShowForm);
        let _ = update(&mut app, DomainMessage::KeychainFormName("GitHub".into()));

        let _ = update(&mut app, DomainMessage::KeychainFormKeyType(KeyType::RSA));
        let suggested = app.keychain.form.path.clone();
        assert_eq!(app.keychain.form.selected_key_type, Some(KeyType::RSA));
        assert!(suggested.contains("id_rsa"), "{suggested}");

        // A hand-typed path is dropped when the type changes: the filename has
        // to match the key that will be written there.
        let _ = update(
            &mut app,
            DomainMessage::KeychainFormPath("/tmp/keep-me".into()),
        );
        let _ = update(&mut app, DomainMessage::KeychainFormKeyType(KeyType::ECDSA));
        assert!(app.keychain.form.path.contains("id_ecdsa"));
        assert!(
            !app.keychain.form.path_manual,
            "the path follows the type again"
        );
    }

    #[test]
    fn editing_a_saved_key_loads_it_and_keeps_a_baseline_for_the_dirty_check() {
        let mut app = app();
        app.keychain.list = vec![entry(1, "github")];

        let _ = update(&mut app, DomainMessage::KeychainEdit(KeychainEntryId(1)));

        assert_eq!(app.keychain.form.editing_id, Some(KeychainEntryId(1)));
        assert_eq!(app.keychain.form.name, "github");
        assert_eq!(app.keychain.form.key_type, "ED25519");
        assert_eq!(app.keychain.form.path, "~/.ssh/github");
        assert!(app.keychain.form.original.is_some());
        assert!(!app.keychain.form.is_dirty());
    }

    #[test]
    fn generating_a_key_is_refused_while_the_form_is_incomplete() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::KeychainShowForm);
        app.keychain.form.path = "/tmp/incomplete".into();
        // No name yet.

        let _ = update(&mut app, DomainMessage::KeychainFormSubmit);

        assert!(
            app.keychain.form.visible,
            "the form stays open until it is complete"
        );
        assert!(!app.keychain.form.in_progress, "nothing was started");
    }

    #[test]
    fn a_finished_generation_closes_the_form_and_the_key_arrives_from_storage() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::KeychainShowForm);

        let _ = update(
            &mut app,
            DomainMessage::KeychainFormGenerated(Ok(crate::keychain::state::GeneratedKeyData {
                key_type_display: "ED25519".into(),
                kind_label: "ed25519".into(),
                name: "github".into(),
                path: "~/.ssh/github".into(),
                fingerprint: "SHA256:abc".into(),
                public_key: "ssh-ed25519 AAAA".into(),
            })),
        );

        assert!(
            !app.keychain.form.visible,
            "the form closes once the key exists"
        );
        assert!(!app.keychain.form.in_progress);

        // The entry itself comes back from the storage worker once it has been
        // written, so that is the message that fills the list.
        let _ = update(
            &mut app,
            DomainMessage::KeychainEntryCreated {
                entry: entry(1, "github"),
            },
        );
        assert_eq!(
            app.keychain.list.first().map(|k| k.name.as_str()),
            Some("github"),
            "the generated key joins the list"
        );
    }

    #[test]
    fn a_failed_generation_reports_and_leaves_the_form_usable() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::KeychainShowForm);
        app.keychain.form.in_progress = true;
        let logs_before = app.logs.len();

        let _ = update(
            &mut app,
            DomainMessage::KeychainFormGenerated(Err("generation failed".into())),
        );

        assert!(
            app.keychain.form.visible,
            "the user needs the form back to retry"
        );
        assert!(!app.keychain.form.in_progress);
        assert!(app.keychain.list.is_empty());
        assert!(
            app.logs.len() > logs_before,
            "a failed key generation must be visible in the log, not silent"
        );
        assert!(
            app.logs
                .last()
                .unwrap()
                .message
                .contains("generation failed"),
            "the log entry names the failure: {}",
            app.logs.last().unwrap().message
        );
    }

    #[test]
    fn deleting_a_key_asks_first() {
        let mut app = app();
        app.keychain.list = vec![entry(1, "github"), entry(2, "prod")];

        let _ = update(
            &mut app,
            DomainMessage::KeychainDeleteRequest(KeychainEntryId(1)),
        );
        assert_eq!(app.keychain.delete_pending, Some(KeychainEntryId(1)));

        let _ = update(&mut app, DomainMessage::KeychainDeleteCancel);
        assert_eq!(app.keychain.delete_pending, None);
        assert_eq!(app.keychain.list.len(), 2, "cancelling deletes nothing");

        let _ = update(&mut app, DomainMessage::KeychainDelete(KeychainEntryId(1)));
        assert_eq!(app.keychain.list.len(), 1);
        assert_eq!(app.keychain.list[0].id, KeychainEntryId(2));
        assert_eq!(app.keychain.delete_pending, None);
    }

    #[test]
    fn deleting_the_key_open_in_the_form_closes_it() {
        let mut app = app();
        app.keychain.list = vec![entry(1, "github")];
        let _ = update(&mut app, DomainMessage::KeychainEdit(KeychainEntryId(1)));

        let _ = update(&mut app, DomainMessage::KeychainDelete(KeychainEntryId(1)));

        assert!(!app.keychain.form.visible);
        assert!(app.keychain.form.editing_id.is_none());
    }

    #[test]
    fn the_form_resets_after_a_generation_so_the_next_key_starts_clean() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::KeychainShowForm);
        let _ = update(&mut app, DomainMessage::KeychainFormName("first".into()));
        let _ = update(
            &mut app,
            DomainMessage::KeychainFormGenerated(Ok(crate::keychain::state::GeneratedKeyData {
                key_type_display: "ED25519".into(),
                kind_label: "ed25519".into(),
                name: "first".into(),
                path: "~/.ssh/first".into(),
                fingerprint: String::new(),
                public_key: String::new(),
            })),
        );

        let _ = update(&mut app, DomainMessage::KeychainShowForm);
        assert!(
            app.keychain.form.name.is_empty(),
            "no carry-over into the next key"
        );
    }

    #[test]
    fn an_identity_needs_a_label_and_a_password_before_it_can_be_saved() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::IdentityShowForm);
        assert!(app.keychain.identity_form.visible);

        let _ = update(&mut app, DomainMessage::IdentityFormLabel("deploy".into()));
        let _ = update(&mut app, DomainMessage::IdentityFormSubmit);
        assert!(
            app.keychain.identity_form.visible,
            "a password is still missing"
        );
        assert!(app.keychain.identities.is_empty());

        let _ = update(
            &mut app,
            DomainMessage::IdentityFormPassword(SecretString::from("hunter2")),
        );
        let _ = update(
            &mut app,
            DomainMessage::IdentityCreated {
                identity: identity(1, "deploy"),
            },
        );
        assert_eq!(app.keychain.identities.len(), 1);
    }

    #[test]
    fn editing_an_identity_loads_it_and_deleting_it_asks_first() {
        let mut app = app();
        app.keychain.identities = vec![identity(1, "deploy")];

        let _ = update(&mut app, DomainMessage::IdentityEdit(IdentityId(1)));
        assert_eq!(app.keychain.identity_form.label, "deploy");
        assert_eq!(app.keychain.identity_form.username, "deploy");
        assert_eq!(
            app.keychain.identity_form.password.expose_secret(),
            "hunter2"
        );

        let _ = update(
            &mut app,
            DomainMessage::IdentityDeleteRequest(IdentityId(1)),
        );
        assert_eq!(app.keychain.identity_delete_pending, Some(IdentityId(1)));

        let _ = update(&mut app, DomainMessage::IdentityDeleteCancel);
        assert_eq!(app.keychain.identity_delete_pending, None);
        assert_eq!(app.keychain.identities.len(), 1);

        let _ = update(&mut app, DomainMessage::IdentityDelete(IdentityId(1)));
        assert!(app.keychain.identities.is_empty());
    }

    #[test]
    fn a_host_offers_no_identity_once_that_identity_is_gone() {
        let mut app = app();
        app.keychain.identities = vec![identity(1, "deploy")];
        let mut host = crate::core::test_support::host(1, "web", "10.0.0.1");
        host.identity_id = Some(IdentityId(1));
        app.hosts.list = vec![host];

        // While the identity exists, opening the host offers it.
        let _ = crate::hosts::update::update(
            &mut app,
            crate::hosts::message::Message::EditHost(HostId(1)),
        );
        assert_eq!(app.hosts.form.selected_identity_id, Some(IdentityId(1)));

        // Deleting an identity removes only the identity itself; the host keeps
        // the id it was pointing at. Every reader filters that id against the
        // live list, so the form stops offering it rather than showing a
        // credential that no longer exists.
        let _ = update(&mut app, DomainMessage::IdentityDelete(IdentityId(1)));
        let _ = crate::hosts::update::update(&mut app, crate::hosts::message::Message::HideForm);
        let _ = crate::hosts::update::update(
            &mut app,
            crate::hosts::message::Message::EditHost(HostId(1)),
        );
        assert_eq!(app.hosts.form.selected_identity_id, None);
    }
}
