use iced::keyboard;
use iced::widget::Id;

use crate::app::App;
use crate::core::data::{
    FileEntry, FileEntryType, Host, SftpSortColumn, SftpSortDirection, read_local_dir,
    sort_file_entries_by,
};
use crate::core::message::Message;
use crate::sftp::{SftpExternalDropTarget, SftpPane};

pub(super) const LOCAL_DROP_ZONE_ID: &str = "sftp_local_drop_zone";
pub(super) const REMOTE_DROP_ZONE_ID: &str = "sftp_remote_drop_zone";

pub(super) fn read_local_dir_sorted(
    path: &str,
    show_hidden: bool,
    col: SftpSortColumn,
    dir: SftpSortDirection,
) -> Vec<FileEntry> {
    let mut entries = read_local_dir(path, show_hidden);
    sort_file_entries_by(&mut entries, col, dir);
    entries
}

/// Read the current local directory off the UI thread.
///
/// The returned message carries the SFTP local-list generation at request
/// time; the update handler discards it if the user has navigated again.
pub(super) fn request_local_dir_load(app: &mut App) -> iced::Task<Message> {
    let generation = app.sftp.state.begin_local_listing();
    let session_id = app.sftp.active_session;
    app.sftp.state.left.pane.loading = true;
    app.sftp.state.left.pane.error = None;

    let path = app.sftp.state.left.pane.path.clone();
    let show_hidden = app.sftp.state.left.pane.show_hidden;
    let column = app.sftp.state.left.pane.sort_column;
    let direction = app.sftp.state.left.pane.sort_direction;

    iced::Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                read_local_dir_sorted(&path, show_hidden, column, direction)
            })
            .await
            .map_err(|join_err| format!("local directory read thread panicked: {join_err}"))
        },
        move |result| {
            Message::Sftp(crate::sftp::message::Message::SftpLocalEntriesLoaded(
                session_id, generation, result,
            ))
        },
    )
}

pub(super) fn external_drop_target(
    zones: &[(Id, iced::Rectangle)],
    point: iced::Point,
) -> Option<SftpExternalDropTarget> {
    let local_zone = Id::from(LOCAL_DROP_ZONE_ID);
    let remote_zone = Id::from(REMOTE_DROP_ZONE_ID);

    zones
        .iter()
        .filter(|(_, bounds)| bounds.contains(point))
        .find_map(|(zone, _)| {
            if *zone == remote_zone {
                Some(SftpExternalDropTarget::Remote)
            } else if *zone == local_zone {
                Some(SftpExternalDropTarget::Local)
            } else {
                None
            }
        })
}

fn matches_filter(name: &str, filter_lower: &str) -> bool {
    filter_lower.is_empty() || name.to_lowercase().contains(filter_lower)
}

/// Returns the entries that match the current filter (case-insensitive name contains).
pub(super) fn visible_entries<'a>(entries: &'a [FileEntry], filter: &str) -> Vec<&'a FileEntry> {
    let filter_lower = filter.to_lowercase();
    entries
        .iter()
        .filter(|entry| matches_filter(&entry.name, &filter_lower))
        .collect()
}

/// Returns the visible file names keyboard navigation can select.
pub(super) fn focusable_names(entries: &[FileEntry], filter: &str) -> Vec<String> {
    let filter_lower = filter.to_lowercase();
    entries
        .iter()
        .filter(|entry| matches_filter(&entry.name, &filter_lower))
        .map(|entry| entry.name.clone())
        .collect()
}

/// Records which file pane owns keyboard navigation.
pub(super) fn activate_pane(sftp: &mut crate::sftp::SftpState, pane: SftpPane) {
    sftp.active_pane = pane;
}

#[inline]
pub(super) fn clear_selection(sftp: &mut crate::sftp::SftpState, pane: SftpPane) {
    let pane = sftp.pane_mut(pane);
    pane.selected.clear();
    pane.last_clicked = None;
    pane.focused.clear();
}

/// Moves keyboard focus to `name` in the requested pane.
pub(super) fn set_file_focus(sftp: &mut crate::sftp::SftpState, pane: SftpPane, name: &str) {
    sftp.pane_mut(pane).focused.select(name.to_string());
}

/// Core multi-select click handler.
///
/// - **Plain click**: replaces selection with just the clicked item, updates anchor.
/// - **Shift+Click**: range-selects from the last-clicked anchor to the clicked item
///   (on the currently visible / filtered entry list). Anchor is NOT changed.
/// - **Ctrl/Cmd+Click**: toggles the clicked item in the selection, updates anchor.
///   If the selection becomes empty after toggle, clears the anchor as well.
pub(super) fn handle_file_click(
    entries: &[FileEntry],
    filter: &str,
    modifiers: keyboard::Modifiers,
    name: &str,
    selected: &mut Vec<String>,
    last_clicked: &mut Option<String>,
) {
    let visible = visible_entries(entries, filter);

    if modifiers.shift() {
        // Range select from anchor to clicked item
        if let Some(ref anchor) = *last_clicked {
            let anchor_idx = visible.iter().position(|e| e.name == *anchor);
            let clicked_idx = visible.iter().position(|e| e.name == name);
            if let (Some(a), Some(c)) = (anchor_idx, clicked_idx) {
                let range = if a <= c { a..=c } else { c..=a };
                *selected = visible[range].iter().map(|e| e.name.clone()).collect();
            } else {
                // Anchor or clicked item not in visible list → fall back to single
                *selected = vec![name.to_string()];
                *last_clicked = Some(name.to_string());
            }
        } else {
            // No anchor yet → start with single selection
            *selected = vec![name.to_string()];
            *last_clicked = Some(name.to_string());
        }
    } else if modifiers.command() {
        if let Some(pos) = selected.iter().position(|n| n == name) {
            selected.remove(pos);
        } else {
            selected.push(name.to_string());
        }
        *last_clicked = Some(name.to_string());
        // Clear anchor when selection becomes empty (prevents phantom anchor)
        if selected.is_empty() {
            *last_clicked = None;
        }
    } else {
        *selected = vec![name.to_string()];
        *last_clicked = Some(name.to_string());
    }
}

/// Look up file types for a list of selected names from the appropriate entries list.
pub(super) fn collect_delete_info(
    app: &App,
    pane: SftpPane,
    names: &[String],
) -> (Vec<String>, Vec<FileEntryType>) {
    let entries = &app.sftp.state.pane(pane).entries;
    let mut out_names = Vec::with_capacity(names.len());
    let mut out_types = Vec::with_capacity(names.len());
    for name in names {
        if let Some(entry) = entries.iter().find(|e| e.name == *name) {
            out_names.push(name.clone());
            out_types.push(entry.file_type.clone());
        }
    }
    (out_names, out_types)
}

pub(super) fn current_remote_host(app: &App, pane: SftpPane) -> Option<Host> {
    let pane_state = app.sftp.state.pane(pane);
    let id = pane_state.host_id?;
    let mut host = app.hosts.list.iter().find(|host| host.id == id).cloned()?;
    if host.expected_fingerprint.is_none() {
        host.expected_fingerprint
            .clone_from(&pane_state.attempt.expected_fingerprint);
    }
    Some(host)
}

pub(super) fn join_remote_path(base: &str, name: &str) -> String {
    if base == "/" {
        format!("/{name}")
    } else if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// Normalize a raw remote path typed into the address bar.
///
/// Absolute paths, `~/`-prefixed paths, and a bare `~` are passed through
/// unchanged — the SFTP layer resolves `~` against the remote user's home
/// directory. Anything else is treated as relative to `base` (the current
/// remote directory).
pub(super) fn normalize_remote_path_input(input: &str, base: &str) -> String {
    if input.starts_with('/')
        || !matches!(
            crate::core::paths::classify_tilde(input),
            crate::core::paths::TildeForm::None
        )
    {
        input.to_string()
    } else {
        join_remote_path(base, input)
    }
}

/// Generate a duplicate name for a remote path by appending " (1)" before the
/// extension.  Example: "/path/file.txt" → "/path/file (1).txt"
pub(super) fn duplicate_name_candidate(remote_path: &str, n: u64) -> String {
    let (dir, name) = remote_path.rsplit_once('/').unwrap_or(("", remote_path));

    let new_name = if let Some((stem, ext)) = name.rsplit_once('.') {
        // Don't treat dotfiles (e.g., ".gitignore") as having an extension
        if stem.is_empty() {
            format!("{name} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        }
    } else {
        format!("{name} ({n})")
    };

    if dir.is_empty() {
        new_name
    } else {
        format!("{dir}/{new_name}")
    }
}
