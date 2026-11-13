use iced::Task;
use std::path::Path;

use crate::app::App;
use crate::core::collection::RowMetrics;
use crate::core::data::{
    FileEntry, FileEntryType, Host, LogCategory, SftpSortColumn, SftpSortDirection,
    classify_file_name, read_local_file_entry, sort_file_entries_by,
};
use crate::core::ids::{HostId, SftpSessionId};
use crate::core::message::Message;
use crate::cursor::reveal_grid_item;
use crate::sftp::message::Message as DomainMessage;
use crate::sftp::state::HostPickerState;
use crate::sftp::{SftpPane, SftpPreviewSource, SftpTransferTarget};
use crate::ssh_connect::SshConnectionState;
use rust_i18n::t;
use secrecy::ExposeSecret;

use super::file_ops::open_preview;
use super::session::with_sftp_session;
use super::shared::{
    activate_pane, clear_selection, current_remote_host, focusable_names, handle_file_click,
    join_remote_path, normalize_remote_path_input, request_local_dir_load, set_file_focus,
};

pub(super) fn refresh_remote(app: &mut App) -> Task<Message> {
    refresh_pane(app, SftpPane::Remote)
}

pub(super) fn refresh_left_remote(app: &mut App) -> Task<Message> {
    refresh_pane(app, SftpPane::Local)
}

pub(super) fn refresh_transfer_target(app: &mut App, target: SftpTransferTarget) -> Task<Message> {
    match target.clone() {
        SftpTransferTarget::Local { directory, name } => {
            let path = Path::new(&directory).join(&name);
            let entry = read_local_file_entry(&path).filter(|entry| {
                app.sftp.state.left.pane.show_hidden || !entry.name.starts_with('.')
            });
            apply_transfer_entry_update(app, &target, entry);
            Task::none()
        }
        SftpTransferTarget::LeftRemote { directory, name } => {
            let Some(host) = current_remote_host(app, SftpPane::Local) else {
                app.sftp.state.left.pane.error =
                    Some(t!("sftp.error.no_source_remote").to_string());
                return Task::none();
            };
            remote_entry_task(host, target, directory, name)
        }
        SftpTransferTarget::Remote { directory, name } => {
            let Some(host) = current_remote_host(app, SftpPane::Remote) else {
                app.sftp.state.right.pane.error = Some(t!("sftp.no_remote_host").to_string());
                return Task::none();
            };
            remote_entry_task(host, target, directory, name)
        }
    }
}

fn remote_entry_task(
    host: Host,
    target: SftpTransferTarget,
    directory: String,
    name: String,
) -> Task<Message> {
    Task::perform(
        async move { refresh_remote_entry(host, &directory, &name).await },
        move |result| {
            Message::Sftp(crate::sftp::message::Message::SftpTransferEntryRefreshed {
                target: target.clone(),
                result,
            })
        },
    )
}

async fn refresh_remote_entry(
    host: Host,
    directory: &str,
    name: &str,
) -> Result<Option<FileEntry>, String> {
    let path = join_remote_path(directory, name);
    crate::ssh::sftp_file_entry(host, path, name.to_string()).await
}

pub(super) fn apply_transfer_entry_update(
    app: &mut App,
    target: &SftpTransferTarget,
    entry: Option<FileEntry>,
) {
    let (pane, directory, name, require_connected) = match target {
        SftpTransferTarget::Local { directory, name } => (SftpPane::Local, directory, name, false),
        SftpTransferTarget::LeftRemote { directory, name } => {
            (SftpPane::Local, directory, name, true)
        }
        SftpTransferTarget::Remote { directory, name } => (SftpPane::Remote, directory, name, true),
    };
    let pane_state = app.sftp.state.pane(pane);
    if pane_state.path != *directory || (require_connected && !pane_state.connected) {
        return;
    }
    let pane_state = app.sftp.state.pane_mut(pane);
    upsert_entry(
        &mut pane_state.entries,
        name,
        entry,
        &mut pane_state.selected,
        &mut pane_state.hovered,
        pane_state.sort_column,
        pane_state.sort_direction,
    );
}

fn upsert_entry(
    entries: &mut Vec<FileEntry>,
    name: &str,
    entry: Option<FileEntry>,
    selected: &mut Vec<String>,
    hovered: &mut Option<String>,
    sort_col: SftpSortColumn,
    sort_dir: SftpSortDirection,
) {
    if let Some(entry) = entry {
        if let Some(existing) = entries.iter_mut().find(|existing| existing.name == name) {
            *existing = entry;
        } else {
            entries.push(entry);
        }
        sort_file_entries_by(entries, sort_col, sort_dir);
    } else {
        entries.retain(|entry| entry.name != name);
        selected.retain(|n| n != name);
        if hovered.as_deref() == Some(name) {
            *hovered = None;
        }
    }
}

fn navigate_left_remote(app: &mut App, path: String) -> Task<Message> {
    navigate_remote(app, SftpPane::Local, path)
}

fn left_remote_back(app: &mut App) -> Task<Message> {
    remote_history(app, SftpPane::Local, false)
}

fn left_remote_forward(app: &mut App) -> Task<Message> {
    remote_history(app, SftpPane::Local, true)
}

fn refresh_pane(app: &mut App, pane: SftpPane) -> Task<Message> {
    let Some(host) = current_remote_host(app, pane) else {
        app.sftp.state.pane_mut(pane).error = Some(remote_error(pane));
        return Task::none();
    };
    let pane_state = app.sftp.state.pane_mut(pane);
    pane_state.loading = true;
    pane_state.error = None;
    list_remote_pane(app, host, pane)
}

fn remote_error(pane: SftpPane) -> String {
    match pane {
        SftpPane::Local => t!("sftp.error.no_source_remote").to_string(),
        SftpPane::Remote => t!("sftp.no_remote_host").to_string(),
    }
}

fn navigate_remote(app: &mut App, pane: SftpPane, path: String) -> Task<Message> {
    let Some(host) = current_remote_host(app, pane) else {
        app.sftp.state.pane_mut(pane).error = Some(remote_error(pane));
        return Task::none();
    };
    let pane_state = app.sftp.state.pane_mut(pane);
    if pane_state.history_idx < pane_state.history.len().saturating_sub(1) {
        pane_state.history.truncate(pane_state.history_idx + 1);
    }
    if pane_state.history.last() != Some(&path) {
        pane_state.history.push(path.clone());
        pane_state.history_idx = pane_state.history.len() - 1;
    }
    pane_state.path = path;
    pane_state.loading = true;
    pane_state.error = None;
    clear_selection(&mut app.sftp.state, pane);
    let pane_state = app.sftp.state.pane_mut(pane);
    pane_state.filter.clear();
    pane_state.path_edit = None;
    list_remote_pane(app, host, pane)
}

fn remote_history(app: &mut App, pane: SftpPane, forward: bool) -> Task<Message> {
    let pane_state = app.sftp.state.pane(pane);
    let can_move = if forward {
        pane_state.history_idx < pane_state.history.len().saturating_sub(1)
    } else {
        pane_state.history_idx > 0
    };
    if !can_move {
        return Task::none();
    }
    let Some(host) = current_remote_host(app, pane) else {
        app.sftp.state.pane_mut(pane).error = Some(remote_error(pane));
        return Task::none();
    };
    let pane_state = app.sftp.state.pane_mut(pane);
    if forward {
        pane_state.history_idx += 1;
    } else {
        pane_state.history_idx -= 1;
    }
    pane_state.path = pane_state.history[pane_state.history_idx].clone();
    pane_state.loading = true;
    pane_state.error = None;
    pane_state.filter.clear();
    pane_state.path_edit = None;
    clear_selection(&mut app.sftp.state, pane);
    list_remote_pane(app, host, pane)
}

fn push_local_history(app: &mut App, path: String) {
    let pane_state = &mut app.sftp.state.left.pane;
    if pane_state.history_idx < pane_state.history.len().saturating_sub(1) {
        pane_state.history.truncate(pane_state.history_idx + 1);
    }
    if pane_state.history.last() != Some(&path) {
        pane_state.history.push(path.clone());
        pane_state.history_idx = pane_state.history.len() - 1;
    }
    pane_state.path = path;
    pane_state.filter.clear();
    pane_state.path_edit = None;
    clear_selection(&mut app.sftp.state, SftpPane::Local);
}

fn local_history(app: &mut App, forward: bool) -> bool {
    let pane_state = &mut app.sftp.state.left.pane;
    let can_move = if forward {
        pane_state.history_idx < pane_state.history.len().saturating_sub(1)
    } else {
        pane_state.history_idx > 0
    };
    if !can_move {
        return false;
    }
    if forward {
        pane_state.history_idx += 1;
    } else {
        pane_state.history_idx -= 1;
    }
    pane_state.path = pane_state.history[pane_state.history_idx].clone();
    pane_state.filter.clear();
    pane_state.path_edit = None;
    clear_selection(&mut app.sftp.state, SftpPane::Local);
    true
}

fn show_host_picker(app: &mut App, pane: SftpPane) {
    activate_pane(&mut app.sftp.state, pane);
    *app.sftp.state.host_picker_mut(pane) = HostPickerState::default();
    app.sftp.state.host_picker_mut(pane).open = true;
    app.sftp.state.host_picker_mut(opposite_pane(pane)).open = false;
}

fn click_entry(app: &mut App, pane: SftpPane, name: &str) -> Task<Message> {
    activate_pane(&mut app.sftp.state, pane);
    if name == ".." {
        let path = app.sftp.state.pane(pane).path.clone();
        let Some(parent) = Path::new(&path).parent() else {
            return Task::none();
        };
        let parent = parent.to_string_lossy().to_string();
        let parent = if parent.is_empty() {
            "/".to_string()
        } else {
            parent
        };
        if app.sftp.state.pane(pane).connected {
            return navigate_remote(app, pane, parent);
        }
        return super::update(app, DomainMessage::SftpLocalNavigate(parent));
    }

    set_file_focus(&mut app.sftp.state, pane, name);
    let modifiers = app.sftp.state.keyboard_modifiers;
    let pane_state = app.sftp.state.pane_mut(pane);
    handle_file_click(
        &pane_state.entries,
        &pane_state.filter,
        modifiers,
        name,
        &mut pane_state.selected,
        &mut pane_state.last_clicked,
    );
    Task::none()
}

fn activate_entry(app: &mut App, pane: SftpPane, name: String) -> Task<Message> {
    activate_pane(&mut app.sftp.state, pane);
    let state = app.sftp.state.pane(pane);
    let connected = state.connected;
    let path = state.path.clone();
    let Some(entry) = state
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .cloned()
    else {
        return Task::none();
    };

    if entry.file_type == FileEntryType::Folder {
        if connected {
            return navigate_remote(app, pane, join_remote_path(&path, &name));
        }
        let path = Path::new(&path).join(&name).to_string_lossy().to_string();
        return super::update(app, DomainMessage::SftpLocalNavigate(path));
    }
    if entry.file_type != FileEntryType::File {
        return Task::none();
    }
    if pane == SftpPane::Local && !connected && !Path::new(&path).join(&name).is_file() {
        return Task::none();
    }

    app.sftp.state.pane_mut(pane).selected = vec![name.clone()];
    app.sftp.state.pane_mut(pane).last_clicked = Some(name.clone());
    set_file_focus(&mut app.sftp.state, pane, &name);
    if classify_file_name(&name).allows_text_preview() {
        let source = match (pane, connected) {
            (SftpPane::Local, false) => SftpPreviewSource::Local,
            (SftpPane::Local, true) => SftpPreviewSource::LeftRemote,
            (SftpPane::Remote, _) => SftpPreviewSource::RightRemote,
        };
        return open_preview(app, source, path, name);
    }
    let message = match pane {
        SftpPane::Local => DomainMessage::SftpUploadSelected,
        SftpPane::Remote => DomainMessage::SftpDownloadSelected,
    };
    super::update(app, message)
}

const fn opposite_pane(pane: SftpPane) -> SftpPane {
    match pane {
        SftpPane::Local => SftpPane::Remote,
        SftpPane::Remote => SftpPane::Local,
    }
}

/// List a remote pane at its current path, taking a fresh listing generation so
/// results from superseded navigation or from a disconnected pane are dropped.
pub(crate) fn list_remote_pane(app: &mut App, host: Host, pane: SftpPane) -> Task<Message> {
    let pane_state = app.sftp.state.pane(pane);
    start_remote_listing(
        app,
        host,
        Some(pane_state.path.clone()),
        pane,
        pane_state.show_hidden,
        false,
    )
}

/// Resolve and list the authenticated user's remote home directory. Used for
/// the first listing after connecting, before a concrete path is known.
pub(super) fn list_remote_pane_home(
    app: &mut App,
    host: Host,
    pane: SftpPane,
    save_password: bool,
) -> Task<Message> {
    let show_hidden = app.sftp.state.pane(pane).show_hidden;
    start_remote_listing(app, host, None, pane, show_hidden, save_password)
}

/// Build a remote listing task, stamping it with the pane's next generation.
fn start_remote_listing(
    app: &mut App,
    host: Host,
    path: Option<String>,
    pane: SftpPane,
    show_hidden: bool,
    save_password: bool,
) -> Task<Message> {
    let session_id = app.sftp.active_session;
    let generation = app.sftp.state.begin_remote_listing(pane);
    list_remote_task_inner(
        host,
        path,
        pane,
        show_hidden,
        session_id,
        generation,
        save_password,
    )
}

fn list_remote_task_inner(
    host: Host,
    path: Option<String>,
    pane: SftpPane,
    show_hidden: bool,
    session_id: SftpSessionId,
    generation: u64,
    save_password: bool,
) -> Task<Message> {
    let host_id = Some(host.id);
    let saved_password = if save_password {
        host.password.clone()
    } else {
        None
    };
    Task::perform(
        async move {
            let result = list_remote_dir(host, path.as_deref(), show_hidden).await;
            (pane, result)
        },
        move |(pane, result)| match result {
            Ok((path, entries, _logs)) => {
                Message::Sftp(crate::sftp::message::Message::SftpRemoteListed {
                    pane,
                    path,
                    result: Ok(entries),
                    session_id,
                    generation,
                    host_id,
                    password: saved_password.clone(),
                    save_password,
                })
            }
            Err(error) => Message::Sftp(crate::sftp::message::Message::SftpRemoteListed {
                pane,
                path: "/".to_string(),
                result: Err(error),
                session_id,
                generation,
                host_id,
                password: None,
                save_password,
            }),
        },
    )
}

async fn list_remote_dir(
    host: Host,
    path: Option<&str>,
    show_hidden: bool,
) -> Result<(String, Vec<FileEntry>, Vec<String>), String> {
    match path {
        Some(p) => {
            // The returned path is the server-resolved absolute path (a leading
            // `~` is expanded against the remote home), so the address bar and
            // subsequent navigation use a real path instead of `/~/…`.
            crate::ssh::list_sftp_dir(host, p.to_string(), show_hidden).await
        }
        None => crate::ssh::resolve_and_list_default_sftp_dir(host, show_hidden).await,
    }
}

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::SftpLocalNavigate(path) => {
            if app.sftp.state.left.pane.connected {
                return navigate_left_remote(app, path);
            }
            // Expand a leading `~` before storing: the terminal's cached CWD
            // can arrive as a bare `~` or `~/…` (shell prompt/title form).
            // The local pane must navigate/display a real absolute path,
            // otherwise the address bar renders `/~` and read_dir fails.
            let path = crate::core::paths::expand_home(&path);
            push_local_history(app, path);
            return request_local_dir_load(app);
        }
        DomainMessage::SftpLocalBack => {
            if app.sftp.state.left.pane.connected {
                return left_remote_back(app);
            }
            if local_history(app, false) {
                return request_local_dir_load(app);
            }
        }
        DomainMessage::SftpLocalForward => {
            if app.sftp.state.left.pane.connected {
                return left_remote_forward(app);
            }
            if local_history(app, true) {
                return request_local_dir_load(app);
            }
        }
        DomainMessage::SftpLocalEntriesLoaded(session_id, generation, result) => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                if generation != app.sftp.state.left.pane.load_id {
                    return Task::none();
                }
                app.sftp.state.left.pane.loading = false;
                match result {
                    Ok(entries) => app.sftp.state.left.pane.entries = entries,
                    Err(err) => {
                        app.sftp.state.left.pane.entries.clear();
                        app.sftp.state.left.pane.error = Some(err.clone());
                        app.add_log(LogCategory::Sftp, err);
                    }
                }
                Task::none()
            });
        }
        DomainMessage::SftpLocalFilter(v) => app.sftp.state.left.pane.filter = v,
        DomainMessage::SftpRemoteFilter(v) => app.sftp.state.right.pane.filter = v,
        DomainMessage::SftpToggleFilter(pane) => {
            activate_pane(&mut app.sftp.state, pane);
            let pane = app.sftp.state.pane_mut(pane);
            pane.filter_open = !pane.filter_open;
            if !pane.filter_open {
                pane.filter.clear();
            }
        }
        DomainMessage::SftpSortBy { pane, column } => {
            let pane = app.sftp.state.pane_mut(pane);
            if pane.sort_column == column {
                pane.sort_direction = pane.sort_direction.toggle();
            } else {
                pane.sort_column = column;
                pane.sort_direction = SftpSortDirection::Ascending;
            }
            sort_file_entries_by(&mut pane.entries, pane.sort_column, pane.sort_direction);
            pane.selected.clear();
            pane.last_clicked = None;
            pane.focused.clear();
        }
        DomainMessage::SftpToggleHidden(pane) => {
            let pane_state = app.sftp.state.pane_mut(pane);
            pane_state.show_hidden = !pane_state.show_hidden;
            clear_selection(&mut app.sftp.state, pane);
            if app.sftp.state.pane(pane).connected
                && let Some(host) = current_remote_host(app, pane)
            {
                return list_remote_pane(app, host, pane);
            }
            if pane == SftpPane::Local {
                return request_local_dir_load(app);
            }
        }
        DomainMessage::SftpRefreshLocal => {
            clear_selection(&mut app.sftp.state, SftpPane::Local);
            if app.sftp.state.left.pane.connected {
                return refresh_left_remote(app);
            }
            return request_local_dir_load(app);
        }
        DomainMessage::SftpRefreshRemote => {
            clear_selection(&mut app.sftp.state, SftpPane::Remote);
            return refresh_remote(app);
        }
        DomainMessage::SftpShowLocalHostPicker => {
            show_host_picker(app, SftpPane::Local);
        }
        DomainMessage::SftpHideLocalHostPicker => {
            app.sftp.state.left.host_picker = HostPickerState::default();
        }
        DomainMessage::SftpShowHostPicker => {
            show_host_picker(app, SftpPane::Remote);
        }
        DomainMessage::SftpHideHostPicker => {
            app.sftp.state.right.host_picker = HostPickerState::default();
        }
        DomainMessage::SftpHostPickerFilter(v) => {
            let pane = app.sftp.state.open_host_picker().unwrap_or(SftpPane::Local);
            let picker = app.sftp.state.host_picker_mut(pane);
            picker.filter = v;
            picker.cursor.clear();
        }
        DomainMessage::SftpHostPickerConnect => {
            return connect_host_picker(app);
        }
        DomainMessage::SftpLocalEntryClicked(name) => {
            return click_entry(app, SftpPane::Local, &name);
        }
        DomainMessage::SftpLocalEntryDoubleClicked(name) => {
            return activate_entry(app, SftpPane::Local, name);
        }
        DomainMessage::SftpRemoteEntryClicked(name) => {
            return click_entry(app, SftpPane::Remote, &name);
        }
        DomainMessage::SftpRemoteEntryDoubleClicked(name) => {
            return activate_entry(app, SftpPane::Remote, name);
        }
        DomainMessage::SftpRemoteNavigate(path) => {
            return navigate_remote(app, SftpPane::Remote, path);
        }
        DomainMessage::SftpRemoteBack => {
            return remote_history(app, SftpPane::Remote, false);
        }
        DomainMessage::SftpRemoteForward => {
            return remote_history(app, SftpPane::Remote, true);
        }
        DomainMessage::SftpRemoteListed {
            pane,
            path,
            result,
            session_id,
            generation,
            host_id,
            password,
            save_password,
        } => {
            return with_sftp_session(app, session_id, |app, _is_active| {
                let pane_state = app.sftp.state.pane(pane);
                if !pane_state.is_remote() || generation != pane_state.load_id {
                    // The listing was superseded by a newer request or the
                    // remote session was disconnected while it was in flight.
                    return Task::none();
                }
                let is_initial_connect = !matches!(
                    pane_state.attempt.state,
                    SshConnectionState::Idle | SshConnectionState::Connected
                );
                app.sftp.state.pane_mut(pane).loading = false;
                app.sftp.state.transfer_status = None;
                match result {
                    Ok(mut entries) => {
                        let mut save_task = Task::none();
                        let mut recent_task = Task::none();
                        if is_initial_connect
                            && let Some(id) = host_id
                            && id != HostId(0)
                        {
                            app.hosts.record_recent(id);
                            let recent_ids = app.hosts.recent_ids.clone();
                            recent_task =
                                app.storage_write_task(move |s| s.save_recent_ids(&recent_ids));
                        }
                        let pane_state = app.sftp.state.pane_mut(pane);
                        sort_file_entries_by(
                            &mut entries,
                            pane_state.sort_column,
                            pane_state.sort_direction,
                        );
                        pane_state.attempt.state = SshConnectionState::Connected;
                        pane_state.connected = true;
                        pane_state.path.clone_from(&path);
                        pane_state.entries = entries;
                        pane_state.error = None;
                        pane_state.attempt.pending_key_info = None;
                        pane_state.attempt.password_input =
                            secrecy::SecretString::from(String::new());
                        pane_state.attempt.password_visible = false;
                        // On initial connection the path comes from the server
                        // (resolved home directory), so replace the placeholder.
                        if is_initial_connect {
                            pane_state.history = vec![path];
                            pane_state.history_idx = 0;
                        }

                        if is_initial_connect
                            && save_password
                            && let (Some(id), Some(password)) = (host_id, password)
                            && id != HostId(0)
                            && !password.expose_secret().is_empty()
                        {
                            let stored = app.hosts.list.iter_mut().find(|host| host.id == id).map(
                                |stored| {
                                    stored.password = Some(password);
                                    stored.clone()
                                },
                            );
                            save_task = match stored {
                                Some(host) => app.storage_write_task(move |s| s.upsert_host(&host)),
                                None => Task::done(Message::ShowErrorToast(
                                    t!("sftp.host_disappeared_password").to_string(),
                                )),
                            };
                        }
                        Task::batch(vec![save_task, recent_task])
                    }
                    Err(error) => {
                        let pane_state = app.sftp.state.pane_mut(pane);
                        // Already connected means this was navigation to an invalid
                        // path, so revert to the previous valid directory.
                        if pane_state.attempt.state == SshConnectionState::Connected {
                            pane_state.history.pop();
                            pane_state.history_idx = pane_state.history.len().saturating_sub(1);
                            if let Some(prev) = pane_state.history.last() {
                                pane_state.path.clone_from(prev);
                            }
                            return Task::perform(async move { error }, Message::ShowErrorToast);
                        }
                        pane_state.attempt.state = SshConnectionState::Failed;
                        pane_state.error = Some(error);
                        pane_state.attempt.pending_key_info = None;
                        Task::none()
                    }
                }
            });
        }
        DomainMessage::SftpPathEditStart(pane) => {
            activate_pane(&mut app.sftp.state, pane);
            let pane = app.sftp.state.pane_mut(pane);
            pane.path_edit = Some(pane.path.clone());
        }
        DomainMessage::SftpPathEditCancel(pane) => {
            app.sftp.state.pane_mut(pane).path_edit = None;
        }
        DomainMessage::SftpPathEditChange(pane, value) => {
            app.sftp.state.pane_mut(pane).path_edit = Some(value);
        }
        DomainMessage::SftpPathEditSubmit(pane) => {
            let pane_state = app.sftp.state.pane_mut(pane);
            let Some(raw) = pane_state.path_edit.take() else {
                return Task::none();
            };
            let connected = pane_state.connected;
            let base = pane_state.path.clone();
            if connected {
                let trimmed = raw.trim().to_string();
                if trimmed.is_empty() {
                    return Task::none();
                }
                let path = normalize_remote_path_input(&trimmed, &base);
                return if pane == SftpPane::Local {
                    navigate_left_remote(app, path)
                } else {
                    super::update(app, DomainMessage::SftpRemoteNavigate(path))
                };
            }
            if pane == SftpPane::Remote {
                return super::update(app, DomainMessage::SftpRemoteNavigate(raw));
            }

            let path = crate::core::paths::expand_home(raw.trim());
            if path.is_empty() {
                return Task::none();
            }
            if !Path::new(&path).is_dir() {
                return Task::perform(
                    async move { t!("sftp.path_not_found", path = &path).to_string() },
                    Message::ShowErrorToast,
                );
            }
            return super::update(app, DomainMessage::SftpLocalNavigate(path));
        }
        DomainMessage::SftpPathEditBlurCheck => {
            // After a mouse press anywhere, check whether the path text input
            // still has focus. If it lost focus, cancel edit mode.
            let pane = if app.sftp.state.left.pane.path_edit.is_some() {
                SftpPane::Local
            } else if app.sftp.state.right.pane.path_edit.is_some() {
                SftpPane::Remote
            } else {
                return Task::none();
            };
            let id = match pane {
                SftpPane::Local => iced::widget::Id::from("sftp_path_input_local"),
                SftpPane::Remote => iced::widget::Id::from("sftp_path_input_remote"),
            };
            return iced::widget::operation::is_focused(id).map(move |focused| {
                if focused {
                    Message::Noop
                } else {
                    Message::Sftp(crate::sftp::message::Message::SftpPathEditCancel(pane))
                }
            });
        }
        DomainMessage::SftpViewCursorMoved(point) => {
            app.sftp.state.cursor_pos = point;
        }
        _ => unreachable!("unexpected sftp message in browse: {:?}", msg),
    }
    Task::none()
}

pub(super) fn move_host_picker_cursor_in(
    app: &mut App,
    pane: SftpPane,
    forward: bool,
) -> Task<Message> {
    if !app.sftp.state.host_picker(pane).open {
        return Task::none();
    }
    let host_ids = crate::sftp::host_picker::visible_ids(app, pane);
    if host_ids.is_empty() {
        return Task::none();
    }
    let picker = app.sftp.state.host_picker_mut(pane);
    if forward {
        picker.cursor.move_next(&host_ids);
    } else {
        picker.cursor.move_previous(&host_ids);
    }
    let Some(index) = picker.cursor.selected_index(&host_ids) else {
        return Task::none();
    };
    scroll_host_picker_selection_into_view(app, index)
}

pub(super) fn connect_host_picker_in(app: &App, pane: SftpPane) -> Task<Message> {
    let picker = app.sftp.state.host_picker(pane);
    if !picker.open {
        return Task::none();
    }
    let host_ids = crate::sftp::host_picker::visible_ids(app, pane);
    let Some(host_id) = picker.cursor.selected_or_first(&host_ids).copied() else {
        return Task::none();
    };
    let connect = match pane {
        SftpPane::Local => DomainMessage::SftpConnectLocalHost(host_id),
        SftpPane::Remote => DomainMessage::SftpConnectHost(host_id),
    };
    Task::done(Message::Sftp(connect))
}

fn connect_host_picker(app: &App) -> Task<Message> {
    app.sftp
        .state
        .open_host_picker()
        .map_or_else(Task::none, |pane| connect_host_picker_in(app, pane))
}

fn scroll_host_picker_selection_into_view(app: &App, index: usize) -> Task<Message> {
    use crate::sftp::view::{
        SFTP_HOST_PICKER_ROW_GAP, SFTP_HOST_PICKER_ROW_HEIGHT, SFTP_HOST_PICKER_SCROLL_ID,
    };

    reveal_grid_item(
        app,
        SFTP_HOST_PICKER_SCROLL_ID,
        index,
        1,
        RowMetrics {
            height: SFTP_HOST_PICKER_ROW_HEIGHT,
            gap: SFTP_HOST_PICKER_ROW_GAP,
            top_padding: 0.0,
        },
    )
}

/// The 15 pt icon at iced's default 1.3 line height plus 8 px padding.
const FILE_ROW_HEIGHT: f32 = 27.5;
/// The one-pixel gap between rows in the table column.
const FILE_ROW_GAP: f32 = 1.0;
/// Top padding on the scrollable's inner container.
const FILE_LIST_TOP_PADDING: f32 = 6.0;

fn active_file_names_for(app: &App, pane: SftpPane) -> Vec<String> {
    let pane = app.sftp.state.pane(pane);
    focusable_names(&pane.entries, &pane.filter)
}

pub(super) fn select_file_focus_in(
    app: &mut App,
    pane: SftpPane,
    forward: bool,
    extend: bool,
) -> Task<Message> {
    let names = active_file_names_for(app, pane);
    if names.is_empty() {
        return Task::none();
    }

    let cursor = &mut app.sftp.state.pane_mut(pane).focused;
    move_file_cursor(cursor, &names, forward);
    let Some(index) = cursor.selected_index(&names) else {
        return Task::none();
    };
    let focused = names[index].clone();

    apply_file_focus_selection(app, pane, &names, &focused, extend);
    scroll_file_focus_into_view(app, pane, index)
}

fn move_file_cursor(
    cursor: &mut crate::core::list_cursor::ListCursor<String>,
    names: &[String],
    forward: bool,
) {
    // The parent row remains mouse-only, so every keyboard focus has a real
    // entry to select and the highlight never disappears at either end.
    if cursor.selected_in(names).is_none() {
        let first = if forward { names.first() } else { names.last() };
        if let Some(name) = first {
            cursor.select(name.clone());
        }
        return;
    }

    if forward {
        cursor.move_next(names);
    } else {
        cursor.move_previous(names);
    }
}

fn apply_file_focus_selection(
    app: &mut App,
    pane: SftpPane,
    names: &[String],
    focused: &str,
    extend: bool,
) {
    let pane = app.sftp.state.pane_mut(pane);
    let selected = &mut pane.selected;
    let last_clicked = &mut pane.last_clicked;

    if !extend {
        *selected = vec![focused.to_string()];
        *last_clicked = Some(focused.to_string());
        return;
    }

    let Some(anchor) = last_clicked.as_deref() else {
        *selected = vec![focused.to_string()];
        *last_clicked = Some(focused.to_string());
        return;
    };
    let anchor_index = names.iter().position(|name| name.as_str() == anchor);
    let focus_index = names.iter().position(|name| name.as_str() == focused);
    let (Some(anchor_index), Some(focus_index)) = (anchor_index, focus_index) else {
        *selected = vec![focused.to_string()];
        *last_clicked = Some(focused.to_string());
        return;
    };
    let range = if anchor_index <= focus_index {
        anchor_index..=focus_index
    } else {
        focus_index..=anchor_index
    };
    *selected = names[range].to_vec();
}

pub(super) fn activate_file_focus_in(app: &App, pane: SftpPane) -> Task<Message> {
    let names = active_file_names_for(app, pane);
    let pane_state = app.sftp.state.pane(pane);
    let Some(index) = pane_state.focused.selected_index(&names) else {
        return Task::none();
    };
    let name = names[index].clone();

    let message = match pane {
        SftpPane::Local => DomainMessage::SftpLocalEntryDoubleClicked(name),
        SftpPane::Remote => DomainMessage::SftpRemoteEntryDoubleClicked(name),
    };
    Task::done(Message::Sftp(message))
}

fn scroll_file_focus_into_view(app: &App, pane: SftpPane, index: usize) -> Task<Message> {
    let scroll_id = match pane {
        SftpPane::Local if app.sftp.state.left.pane.is_remote() => {
            crate::sftp::view::SFTP_LOCAL_REMOTE_FILE_LIST_ID
        }
        SftpPane::Local => crate::sftp::view::SFTP_LOCAL_FILE_LIST_ID,
        SftpPane::Remote => crate::sftp::view::SFTP_REMOTE_FILE_LIST_ID,
    };
    reveal_grid_item(
        app,
        scroll_id,
        index,
        1,
        RowMetrics {
            height: FILE_ROW_HEIGHT,
            gap: FILE_ROW_GAP,
            top_padding: FILE_LIST_TOP_PADDING,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::test_support::app;

    fn entry(name: &str, file_type: FileEntryType, size: u64) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            file_type,
            size: Some(size),
            modified: String::new(),
            modified_epoch: Some(0),
            permissions: String::new(),
            kind: String::new(),
        }
    }

    fn names(entries: &[FileEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    fn local_listing(app: &mut App, entries: Vec<FileEntry>) {
        app.sftp.state.left.pane.path = "/home".to_string();
        app.sftp.state.left.pane.entries = entries;
    }

    fn move_file(
        app: &mut App,
        target: crate::core::cursor_focus::CursorTarget,
        direction: crate::core::grid_cursor::GridDirection,
        extend: bool,
    ) {
        let _ = crate::cursor::move_cursor_with_extend(app, target, direction, extend);
    }

    fn activate_file(
        app: &mut App,
        target: crate::core::cursor_focus::CursorTarget,
    ) -> crate::core::test_support::Drained {
        crate::core::test_support::drain(crate::cursor::activate(app, target))
    }

    #[test]
    fn arrow_down_starts_on_the_first_entry_and_selects_it() {
        let mut app = app();
        local_listing(
            &mut app,
            vec![
                entry("a.txt", FileEntryType::File, 1),
                entry("b.txt", FileEntryType::File, 1),
            ],
        );

        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Down,
            false,
        );

        assert_eq!(
            app.sftp.state.left.pane.focused.selected_key(),
            Some(&"a.txt".to_string())
        );
        assert_eq!(app.sftp.state.left.pane.selected, vec!["a.txt"]);
    }

    #[test]
    fn arrow_up_at_the_first_entry_keeps_it_selected() {
        let mut app = app();
        local_listing(
            &mut app,
            vec![
                entry("a.txt", FileEntryType::File, 1),
                entry("b.txt", FileEntryType::File, 1),
            ],
        );

        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Down,
            false,
        );
        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Up,
            false,
        );

        assert_eq!(
            app.sftp.state.left.pane.focused.selected_key(),
            Some(&"a.txt".to_string())
        );
        assert_eq!(app.sftp.state.left.pane.selected, vec!["a.txt"]);
        assert_eq!(
            app.sftp.state.left.pane.last_clicked.as_deref(),
            Some("a.txt")
        );
    }

    #[test]
    fn arrow_keys_walk_only_the_filtered_rows() {
        let mut app = app();
        local_listing(
            &mut app,
            vec![
                entry("a.txt", FileEntryType::File, 1),
                entry("notes.md", FileEntryType::File, 1),
                entry("z.txt", FileEntryType::File, 1),
            ],
        );
        app.sftp.state.left.pane.filter = "notes".to_string();

        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Down,
            false,
        );
        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Down,
            false,
        );

        assert_eq!(
            app.sftp.state.left.pane.focused.selected_key(),
            Some(&"notes.md".to_string())
        );
        assert_eq!(app.sftp.state.left.pane.selected, vec!["notes.md"]);
    }

    #[test]
    fn shift_arrows_extend_from_the_last_clicked_anchor() {
        let mut app = app();
        local_listing(
            &mut app,
            vec![
                entry("a.txt", FileEntryType::File, 1),
                entry("b.txt", FileEntryType::File, 1),
                entry("c.txt", FileEntryType::File, 1),
            ],
        );

        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Down,
            false,
        );
        app.sftp.state.keyboard_modifiers = iced::keyboard::Modifiers::SHIFT;
        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Down,
            true,
        );

        assert_eq!(
            app.sftp.state.left.pane.selected,
            vec!["a.txt".to_string(), "b.txt".to_string()]
        );
        assert_eq!(
            app.sftp.state.left.pane.last_clicked.as_deref(),
            Some("a.txt")
        );
    }

    #[test]
    fn arrows_use_the_active_remote_pane() {
        let mut app = app();
        app.sftp.state.active_pane = SftpPane::Remote;
        app.sftp.state.right.pane.path = "/srv".to_string();
        app.sftp.state.right.pane.entries = vec![entry("site", FileEntryType::Folder, 0)];

        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpRemoteFiles,
            crate::core::grid_cursor::GridDirection::Down,
            false,
        );

        assert_eq!(
            app.sftp.state.right.pane.focused.selected_key(),
            Some(&"site".to_string())
        );
        assert_eq!(app.sftp.state.right.pane.selected, vec!["site"]);
    }

    #[test]
    fn clicking_a_row_moves_the_keyboard_focus_with_it() {
        let mut app = app();
        local_listing(
            &mut app,
            vec![
                entry("a.txt", FileEntryType::File, 1),
                entry("b.txt", FileEntryType::File, 1),
            ],
        );

        let _ = update(
            &mut app,
            DomainMessage::SftpLocalEntryClicked("b.txt".to_string()),
        );

        assert_eq!(
            app.sftp.state.left.pane.focused.selected_key(),
            Some(&"b.txt".to_string())
        );
    }

    #[test]
    fn enter_activates_the_focused_row() {
        let mut app = app();
        local_listing(&mut app, vec![entry("a.txt", FileEntryType::File, 1)]);
        move_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
            crate::core::grid_cursor::GridDirection::Down,
            false,
        );

        let answered = activate_file(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalFiles,
        );

        assert!(
            answered.messages.iter().any(|message| matches!(
                message,
                Message::Sftp(DomainMessage::SftpLocalEntryDoubleClicked(name))
                    if name == "a.txt"
            )),
            "Enter should reuse the row's double-click action: {:?}",
            answered.messages
        );
    }

    /// Apply one listing result the way the remote-listing handler does.
    fn apply(
        entries: &mut Vec<FileEntry>,
        name: &str,
        entry: Option<FileEntry>,
        selected: &mut Vec<String>,
        hovered: &mut Option<String>,
    ) {
        upsert_entry(
            entries,
            name,
            entry,
            selected,
            hovered,
            SftpSortColumn::Name,
            SftpSortDirection::Ascending,
        );
    }

    #[test]
    fn a_reported_change_replaces_the_row_in_place() {
        let mut entries = vec![entry("a.txt", FileEntryType::File, 1)];
        let mut selected = Vec::new();
        let mut hovered = None;

        apply(
            &mut entries,
            "a.txt",
            Some(entry("a.txt", FileEntryType::File, 9)),
            &mut selected,
            &mut hovered,
        );

        assert_eq!(entries.len(), 1, "the row must be updated, not duplicated");
        assert_eq!(entries[0].size, Some(9));
    }

    #[test]
    fn new_entries_are_inserted_in_the_current_sort_order() {
        let mut entries = vec![entry("m.txt", FileEntryType::File, 1)];
        let mut selected = Vec::new();
        let mut hovered = None;

        for name in ["z.txt", "dir", "a.txt"] {
            let file_type = if name == "dir" {
                FileEntryType::Folder
            } else {
                FileEntryType::File
            };
            apply(
                &mut entries,
                name,
                Some(entry(name, file_type, 1)),
                &mut selected,
                &mut hovered,
            );
        }

        // Folders first, then alphabetical — the same order the table renders.
        assert_eq!(names(&entries), ["dir", "a.txt", "m.txt", "z.txt"]);
    }

    #[test]
    fn a_deleted_entry_drops_its_selection_and_hover() {
        let mut entries = vec![
            entry("a.txt", FileEntryType::File, 1),
            entry("b.txt", FileEntryType::File, 1),
        ];
        let mut selected = vec!["a.txt".to_string(), "b.txt".to_string()];
        let mut hovered = Some("a.txt".to_string());

        apply(&mut entries, "a.txt", None, &mut selected, &mut hovered);

        assert_eq!(names(&entries), ["b.txt"]);
        assert_eq!(
            selected,
            ["b.txt"],
            "a selection pointing at a deleted file would act on it later"
        );
        assert_eq!(hovered, None, "the hover followed the deleted row");
    }

    #[test]
    fn deleting_a_row_that_was_not_hovered_leaves_the_hover_alone() {
        let mut entries = vec![
            entry("a.txt", FileEntryType::File, 1),
            entry("b.txt", FileEntryType::File, 1),
        ];
        let mut selected = Vec::new();
        let mut hovered = Some("b.txt".to_string());

        apply(&mut entries, "a.txt", None, &mut selected, &mut hovered);

        assert_eq!(hovered, Some("b.txt".to_string()));
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::core::test_support::{app, drain, host};

    /// The session id the workspace starts on.
    fn session(app: &App) -> SftpSessionId {
        app.sftp.sessions[0].id
    }

    fn sftp_workspace_app() -> App {
        let mut app = app();
        app.active_tab = crate::app::ActiveTab::Workspace(crate::app::Workspace::Sftp);
        app
    }

    fn entry(name: &str, file_type: FileEntryType, size: u64) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            file_type,
            size: Some(size),
            modified: String::new(),
            modified_epoch: Some(0),
            permissions: String::new(),
            kind: String::new(),
        }
    }

    fn names(entries: &[FileEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    fn move_host_picker(
        app: &mut App,
        target: crate::core::cursor_focus::CursorTarget,
        direction: crate::core::grid_cursor::GridDirection,
    ) -> crate::core::test_support::Drained {
        drain(crate::cursor::move_cursor(app, target, direction))
    }

    #[test]
    fn navigating_back_and_forward_walks_the_visited_directories() {
        let mut app = app();

        let _ = update(&mut app, DomainMessage::SftpLocalNavigate("/srv".into()));
        let _ = update(
            &mut app,
            DomainMessage::SftpLocalNavigate("/srv/app".into()),
        );
        assert_eq!(app.sftp.state.left.pane.path, "/srv/app");

        let _ = update(&mut app, DomainMessage::SftpLocalBack);
        assert_eq!(app.sftp.state.left.pane.path, "/srv");
        let _ = update(&mut app, DomainMessage::SftpLocalBack);
        assert_ne!(
            app.sftp.state.left.pane.path, "/srv",
            "the root of the history"
        );

        let _ = update(&mut app, DomainMessage::SftpLocalForward);
        assert_eq!(app.sftp.state.left.pane.path, "/srv");
        let _ = update(&mut app, DomainMessage::SftpLocalForward);
        assert_eq!(app.sftp.state.left.pane.path, "/srv/app");
    }

    #[test]
    fn navigating_after_going_back_drops_the_forward_history() {
        let mut app = app();
        let _ = update(&mut app, DomainMessage::SftpLocalNavigate("/srv".into()));
        let _ = update(
            &mut app,
            DomainMessage::SftpLocalNavigate("/srv/app".into()),
        );
        let _ = update(&mut app, DomainMessage::SftpLocalBack);

        // Browsing somewhere new from here makes the old forward entry dead.
        let _ = update(&mut app, DomainMessage::SftpLocalNavigate("/var".into()));
        let _ = update(&mut app, DomainMessage::SftpLocalForward);

        assert_eq!(
            app.sftp.state.left.pane.path, "/var",
            "forward must not resurrect a path the user navigated away from"
        );
    }

    #[test]
    fn navigating_clears_the_filter_and_the_selection_it_used_to_match() {
        let mut app = app();
        app.sftp.state.left.pane.filter = "txt".into();
        app.sftp.state.left.pane.selected = vec!["notes.txt".into()];
        app.sftp.state.left.pane.path_edit = Some("/srv/app".into());

        let _ = update(
            &mut app,
            DomainMessage::SftpLocalNavigate("/srv/other".into()),
        );

        assert!(
            app.sftp.state.left.pane.filter.is_empty(),
            "a filter that matched the old directory would hide the new one"
        );
        assert!(
            app.sftp.state.left.pane.selected.is_empty(),
            "a selection pointing at the old directory would act on the wrong files"
        );
        assert_eq!(app.sftp.state.left.pane.path_edit, None);
    }

    #[test]
    fn a_tilde_path_is_expanded_before_the_pane_is_navigated() {
        let mut app = app();

        let _ = update(&mut app, DomainMessage::SftpLocalNavigate("~/app".into()));

        let path = Path::new(&app.sftp.state.left.pane.path);
        assert!(
            path.is_absolute(),
            "the address bar must never render a `~`: {}",
            app.sftp.state.left.pane.path
        );
        assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("app"));
    }

    #[test]
    fn sorting_flips_the_direction_when_the_same_column_is_chosen_twice() {
        let mut app = app();
        app.sftp.state.left.pane.entries = vec![
            entry("b.txt", FileEntryType::File, 20),
            entry("a.txt", FileEntryType::File, 10),
        ];
        app.sftp.state.left.pane.selected = vec!["b.txt".into()];
        app.sftp.state.left.pane.show_hidden = true;

        let _ = update(
            &mut app,
            DomainMessage::SftpSortBy {
                pane: SftpPane::Local,
                column: SftpSortColumn::Size,
            },
        );
        assert_eq!(app.sftp.state.left.pane.sort_column, SftpSortColumn::Size);
        assert_eq!(
            app.sftp.state.left.pane.sort_direction,
            SftpSortDirection::Ascending
        );
        assert_eq!(names(&app.sftp.state.left.pane.entries), ["a.txt", "b.txt"]);
        assert!(
            app.sftp.state.left.pane.selected.is_empty(),
            "re-sorting moves the rows out from under any selection"
        );

        let _ = update(
            &mut app,
            DomainMessage::SftpSortBy {
                pane: SftpPane::Local,
                column: SftpSortColumn::Size,
            },
        );
        assert_eq!(
            app.sftp.state.left.pane.sort_direction,
            SftpSortDirection::Descending
        );
        assert_eq!(names(&app.sftp.state.left.pane.entries), ["b.txt", "a.txt"]);

        // A different column starts ascending again.
        let _ = update(
            &mut app,
            DomainMessage::SftpSortBy {
                pane: SftpPane::Local,
                column: SftpSortColumn::Name,
            },
        );
        assert_eq!(app.sftp.state.left.pane.sort_column, SftpSortColumn::Name);
        assert_eq!(
            app.sftp.state.left.pane.sort_direction,
            SftpSortDirection::Ascending
        );
        assert_eq!(names(&app.sftp.state.left.pane.entries), ["a.txt", "b.txt"]);
    }

    #[test]
    fn searching_and_hiding_files_are_independent_per_pane() {
        let mut app = app();

        let _ = update(&mut app, DomainMessage::SftpLocalFilter("txt".into()));
        let _ = update(&mut app, DomainMessage::SftpRemoteFilter("log".into()));
        assert_eq!(app.sftp.state.left.pane.filter, "txt");
        assert_eq!(app.sftp.state.right.pane.filter, "log");

        let _ = update(&mut app, DomainMessage::SftpToggleHidden(SftpPane::Local));
        assert!(app.sftp.state.left.pane.show_hidden);
        assert!(!app.sftp.state.right.pane.show_hidden);

        let _ = update(&mut app, DomainMessage::SftpToggleHidden(SftpPane::Remote));
        assert!(app.sftp.state.right.pane.show_hidden);
    }

    #[test]
    fn closing_the_filter_box_forgets_what_was_typed_in_it() {
        let mut app = sftp_workspace_app();

        let _ = update(&mut app, DomainMessage::SftpToggleFilter(SftpPane::Local));
        assert!(app.sftp.state.left.pane.filter_open);
        let _ = crate::focus::sync(&mut app);
        assert!(
            matches!(
                app.focus.top(),
                Some(crate::focus::FocusId::Search(
                    crate::focus::SearchId::SftpFilter {
                        pane: SftpPane::Local
                    }
                ))
            ),
            "the box takes focus when it is opened"
        );

        let _ = update(&mut app, DomainMessage::SftpLocalFilter("txt".into()));
        let _ = update(&mut app, DomainMessage::SftpToggleFilter(SftpPane::Local));

        assert!(!app.sftp.state.left.pane.filter_open);
        assert!(
            app.sftp.state.left.pane.filter.is_empty(),
            "a hidden filter would silently hide files"
        );
        let _ = crate::focus::sync(&mut app);
        assert!(
            matches!(
                app.focus.top(),
                Some(crate::focus::FocusId::Cursor(
                    crate::core::cursor_focus::CursorTarget::SftpLocalFiles
                ))
            ),
            "closing the filter returns keyboard control to the file table"
        );
    }

    #[test]
    fn the_path_edit_box_starts_from_the_current_directory_and_can_be_abandoned() {
        let mut app = sftp_workspace_app();
        app.sftp.state.left.pane.path = "/srv/app".into();

        let _ = update(&mut app, DomainMessage::SftpPathEditStart(SftpPane::Local));
        assert_eq!(
            app.sftp.state.left.pane.path_edit.as_deref(),
            Some("/srv/app")
        );

        let _ = update(
            &mut app,
            DomainMessage::SftpPathEditChange(SftpPane::Local, "/var/log".into()),
        );
        assert_eq!(
            app.sftp.state.left.pane.path_edit.as_deref(),
            Some("/var/log")
        );

        let _ = update(&mut app, DomainMessage::SftpPathEditCancel(SftpPane::Local));
        assert_eq!(app.sftp.state.left.pane.path_edit, None);
        assert_eq!(
            app.sftp.state.left.pane.path, "/srv/app",
            "the pane did not move"
        );
        let _ = crate::focus::sync(&mut app);
        assert!(
            matches!(
                app.focus.top(),
                Some(crate::focus::FocusId::Cursor(
                    crate::core::cursor_focus::CursorTarget::SftpLocalFiles
                ))
            ),
            "leaving path editing restores file-table keyboard navigation"
        );
    }

    #[test]
    fn submitting_the_path_box_navigates_to_what_was_typed() {
        let mut app = app();
        let target = std::env::temp_dir().to_string_lossy().into_owned();
        let _ = update(&mut app, DomainMessage::SftpPathEditStart(SftpPane::Local));
        let _ = update(
            &mut app,
            DomainMessage::SftpPathEditChange(SftpPane::Local, target.clone()),
        );

        let _ = update(&mut app, DomainMessage::SftpPathEditSubmit(SftpPane::Local));

        assert_eq!(app.sftp.state.left.pane.path, target);
        assert_eq!(app.sftp.state.left.pane.path_edit, None, "the box closes");
    }

    #[test]
    fn a_host_picker_opens_over_the_pane_and_cancels_out() {
        let mut app = sftp_workspace_app();
        app.sftp.state.left.host_picker.cursor.select(HostId(7));

        let _ = update(&mut app, DomainMessage::SftpShowLocalHostPicker);
        let _ = crate::focus::sync(&mut app);

        assert!(app.sftp.state.left.host_picker.open);
        assert_eq!(
            app.focus.top(),
            Some(crate::focus::FocusId::Cursor(
                crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker
            )),
            "the filter box takes focus when the picker opens"
        );
        assert!(
            app.sftp
                .state
                .left
                .host_picker
                .cursor
                .selected_key()
                .is_none(),
            "opening the picker starts without a stale selection"
        );

        let _ = update(
            &mut app,
            DomainMessage::SftpHostPickerFilter("bastion".into()),
        );
        assert_eq!(app.sftp.state.left.host_picker.filter, "bastion");

        let _ = update(&mut app, DomainMessage::SftpHideLocalHostPicker);
        assert!(!app.sftp.state.left.host_picker.open);
        let _ = crate::focus::sync(&mut app);
        assert!(
            matches!(
                app.focus.top(),
                Some(crate::focus::FocusId::Cursor(
                    crate::core::cursor_focus::CursorTarget::SftpLocalFiles
                ))
            ),
            "closing the local picker returns focus to its pane"
        );

        let _ = update(&mut app, DomainMessage::SftpShowHostPicker);
        assert!(app.sftp.state.right.host_picker.open);
        let _ = update(&mut app, DomainMessage::SftpHideHostPicker);
        assert!(!app.sftp.state.right.host_picker.open);
        let _ = crate::focus::sync(&mut app);
        assert!(
            matches!(
                app.focus.top(),
                Some(crate::focus::FocusId::Cursor(
                    crate::core::cursor_focus::CursorTarget::SftpRemoteFiles
                ))
            ),
            "closing the remote picker returns focus to its pane"
        );
    }

    #[test]
    fn the_host_picker_cursor_walks_only_the_visible_hosts() {
        let mut app = sftp_workspace_app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")];

        let _ = update(&mut app, DomainMessage::SftpShowLocalHostPicker);
        move_host_picker(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker,
            crate::core::grid_cursor::GridDirection::Down,
        );
        assert_eq!(
            app.sftp.state.left.host_picker.cursor.selected_key(),
            Some(&HostId(1))
        );

        let _ = update(&mut app, DomainMessage::SftpHostPickerFilter("db".into()));
        assert_eq!(
            app.sftp.state.left.host_picker.cursor.selected_key(),
            None,
            "changing the filter resets the cursor"
        );

        move_host_picker(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker,
            crate::core::grid_cursor::GridDirection::Down,
        );
        assert_eq!(
            app.sftp.state.left.host_picker.cursor.selected_key(),
            Some(&HostId(2)),
            "the cursor skips the hidden web host"
        );
    }

    #[test]
    fn enter_without_moving_connects_the_first_visible_sftp_host() {
        let mut app = sftp_workspace_app();
        app.hosts.list = vec![host(1, "prod-db", "10.0.0.1"), host(2, "db", "10.0.0.2")];
        app.hosts.recent_ids = vec![HostId(2), HostId(1)];
        let _ = update(&mut app, DomainMessage::SftpShowHostPicker);
        let _ = update(&mut app, DomainMessage::SftpHostPickerFilter("db".into()));

        let carried = drain(update(&mut app, DomainMessage::SftpHostPickerConnect));

        assert!(
            carried.messages.iter().any(|message| matches!(
                message,
                Message::Sftp(DomainMessage::SftpConnectHost(id)) if *id == HostId(2)
            )),
            "Enter connects to the first row the picker shows: {:?}",
            carried.messages
        );
    }

    #[test]
    fn enter_connects_the_host_under_the_sftp_picker_cursor() {
        let mut app = sftp_workspace_app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1"), host(2, "db", "10.0.0.2")];
        let _ = update(&mut app, DomainMessage::SftpShowHostPicker);
        move_host_picker(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpRemoteHostPicker,
            crate::core::grid_cursor::GridDirection::Down,
        );
        move_host_picker(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpRemoteHostPicker,
            crate::core::grid_cursor::GridDirection::Down,
        );

        let carried = drain(crate::cursor::activate(
            &app,
            crate::core::cursor_focus::CursorTarget::SftpRemoteHostPicker,
        ));

        assert!(
            carried.messages.iter().any(|message| matches!(
                message,
                Message::Sftp(DomainMessage::SftpConnectHost(id)) if *id == HostId(2)
            )),
            "Enter uses the cursor instead of the first row: {:?}",
            carried.messages
        );
    }

    #[test]
    fn the_local_sftp_host_picker_routes_enter_to_the_left_pane() {
        let mut app = sftp_workspace_app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];
        let _ = update(&mut app, DomainMessage::SftpShowLocalHostPicker);
        move_host_picker(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker,
            crate::core::grid_cursor::GridDirection::Down,
        );

        let carried = drain(crate::cursor::activate(
            &app,
            crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker,
        ));

        assert!(
            carried.messages.iter().any(|message| matches!(
                message,
                Message::Sftp(DomainMessage::SftpConnectLocalHost(id)) if *id == HostId(1)
            )),
            "the left picker connects its left remote pane: {:?}",
            carried.messages
        );
    }

    #[test]
    fn a_host_picker_cursor_does_not_move_the_picker_that_is_closed() {
        let mut app = sftp_workspace_app();
        app.hosts.list = vec![host(1, "web", "10.0.0.1")];
        let _ = update(&mut app, DomainMessage::SftpShowHostPicker);

        let carried = move_host_picker(
            &mut app,
            crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker,
            crate::core::grid_cursor::GridDirection::Down,
        );
        let activation = drain(crate::cursor::activate(
            &app,
            crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker,
        ));

        assert!(carried.messages.is_empty());
        assert!(activation.messages.is_empty());
        assert_eq!(app.sftp.state.left.host_picker.cursor.selected_key(), None);
    }

    #[test]
    fn moving_the_host_picker_cursor_scrolls_the_selected_row_into_view() {
        let mut app = sftp_workspace_app();
        app.hosts.list = (1..=8)
            .map(|id| host(id, &format!("host-{id}"), "10.0.0.1"))
            .collect();
        let _ = update(&mut app, DomainMessage::SftpShowLocalHostPicker);
        app.scroll_viewport_heights.insert(
            crate::sftp::view::SFTP_HOST_PICKER_SCROLL_ID,
            crate::sftp::view::SFTP_HOST_PICKER_ROW_HEIGHT * 2.0,
        );

        let mut scrolled = false;
        for _ in 0..8 {
            scrolled |= move_host_picker(
                &mut app,
                crate::core::cursor_focus::CursorTarget::SftpLocalHostPicker,
                crate::core::grid_cursor::GridDirection::Down,
            )
            .other_actions
                > 0;
        }

        assert_eq!(
            app.sftp.state.left.host_picker.cursor.selected_key(),
            Some(&HostId(8))
        );
        assert!(scrolled, "keyboard navigation reveals off-screen rows");
    }

    #[test]
    fn a_local_listing_result_fills_the_pane_and_clears_the_spinner() {
        let mut app = app();
        let session_id = session(&app);
        app.sftp.state.left.pane.loading = true;
        let generation = app.sftp.state.left.pane.load_id;

        let _ = update(
            &mut app,
            DomainMessage::SftpLocalEntriesLoaded(
                session_id,
                generation,
                Ok(vec![entry("a.txt", FileEntryType::File, 1)]),
            ),
        );

        assert_eq!(names(&app.sftp.state.left.pane.entries), ["a.txt"]);
        assert!(!app.sftp.state.left.pane.loading);
        assert!(app.sftp.state.left.pane.error.is_none());
    }

    #[test]
    fn a_listing_that_arrives_after_the_user_moved_on_is_discarded() {
        let mut app = app();
        let session_id = session(&app);
        app.sftp.state.left.pane.entries = vec![entry("current.txt", FileEntryType::File, 1)];
        let stale_generation = app.sftp.state.left.pane.load_id;

        // Navigating issues a new load; the previous one is now stale.
        let _ = update(&mut app, DomainMessage::SftpLocalNavigate("/srv".into()));
        let _ = update(
            &mut app,
            DomainMessage::SftpLocalEntriesLoaded(
                session_id,
                stale_generation,
                Ok(vec![entry("stale.txt", FileEntryType::File, 1)]),
            ),
        );

        assert_eq!(
            names(&app.sftp.state.left.pane.entries),
            ["current.txt"],
            "a slow listing must not overwrite the directory the user is looking at"
        );
    }

    #[test]
    fn a_failed_listing_reports_the_error_instead_of_an_empty_directory() {
        let mut app = app();
        let session_id = session(&app);
        let generation = app.sftp.state.left.pane.load_id;

        let _ = update(
            &mut app,
            DomainMessage::SftpLocalEntriesLoaded(
                session_id,
                generation,
                Err("permission denied".into()),
            ),
        );

        assert!(app.sftp.state.left.pane.entries.is_empty());
        assert_eq!(
            app.sftp.state.left.pane.error.as_deref(),
            Some("permission denied")
        );
        assert!(!app.sftp.state.left.pane.loading);
    }
}
