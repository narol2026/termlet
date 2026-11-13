//! Central routing for every keyboard-driven collection cursor.

// Rust guideline compliant 2026-10-09

use iced::Task;
use iced::widget::operation::AbsoluteOffset;
use iced::widget::{Id, operation};

use crate::app::{App, Workspace};
use crate::core::collection::{self, RowMetrics};
use crate::core::cursor_focus::CursorTarget;
use crate::core::data::{
    COMMAND_HISTORY_LIMITS, HostId, HostTagFilter, IdentityId, KeychainEntryId, LocalShell,
    NavItem, RightSidebarTab, SnippetPackageId, clean_tag,
};
use crate::core::dropdown::{DropdownState, DropdownTarget, ROW_GAP, TransferMenuAction};
use crate::core::grid_cursor::GridDirection;
use crate::core::locale::Locale;
use crate::core::message::Message;
use crate::core::theme::ThemeMode;
use crate::hosts::TagPickerItem;
use crate::keychain::KeychainSelection;
use crate::sftp::SftpPane;
use crate::snippets::SnippetSelection;
use crate::terminal::SidebarState;

/// Scroll a selected row fully into view.
///
/// An unknown viewport reveals the row from its top edge. Once the viewport is
/// measured, movement only scrolls when the row crosses either visible edge.
pub(crate) fn reveal_row(
    app: &App,
    scroll_id: &'static str,
    index: usize,
    metrics: RowMetrics,
) -> Task<Message> {
    let row_top = metrics.top_padding + index as f32 * (metrics.height + metrics.gap);
    let row_bottom = row_top + metrics.height;
    let current_y = app.scroll_offsets.get(scroll_id).copied().unwrap_or(0.0);
    let viewport_height = app
        .scroll_viewport_heights
        .get(scroll_id)
        .copied()
        .unwrap_or(0.0);
    let target_y = if viewport_height <= 1.0 || row_top < current_y {
        row_top
    } else if row_bottom > current_y + viewport_height {
        row_bottom - viewport_height
    } else {
        return Task::none();
    };

    operation::scroll_to(
        Id::from(scroll_id),
        AbsoluteOffset {
            x: None,
            y: Some(target_y.max(0.0)),
        },
    )
}

/// Scroll a selected item in a grid into view.
///
/// Grid cursors store the visible item index; scrolling operates on rendered
/// rows, so the index is divided by the measured column count first.
pub(crate) fn reveal_grid_item(
    app: &App,
    scroll_id: &'static str,
    item_index: usize,
    columns: usize,
    metrics: RowMetrics,
) -> Task<Message> {
    reveal_row(app, scroll_id, item_index / columns.max(1), metrics)
}

/// Resolve the collection shown by the active Vaults page.
///
/// Overlay precedence is handled by the app-level focus policy before this
/// target is requested.
pub(crate) fn vault_target(app: &App) -> Option<CursorTarget> {
    if app.active_tab != crate::app::ActiveTab::Workspace(Workspace::Vaults) {
        return None;
    }

    match app.active_nav {
        NavItem::Hosts => Some(CursorTarget::Hosts),
        NavItem::Keychain => Some(CursorTarget::Keychain),
        NavItem::PortForwarding => Some(CursorTarget::PortForwards),
        NavItem::Snippets => Some(CursorTarget::Snippets),
        NavItem::KnownHosts => Some(CursorTarget::KnownHosts),
        NavItem::Settings => Some(CursorTarget::Workspaces),
        NavItem::Logs => None,
    }
}

/// Resolve the active SFTP file pane.
///
/// Overlay precedence is handled by the app-level focus policy before this
/// target is requested.
pub(crate) fn sftp_file_target(app: &App) -> Option<CursorTarget> {
    if app.active_tab != crate::app::ActiveTab::Workspace(Workspace::Sftp) {
        return None;
    }
    match app.sftp.state.active_pane {
        SftpPane::Local => Some(CursorTarget::SftpLocalFiles),
        SftpPane::Remote => Some(CursorTarget::SftpRemoteFiles),
    }
}

/// Returns the host form's visible keychain options.
pub(crate) fn host_form_keychain_ids(app: &App) -> Vec<KeychainEntryId> {
    let query = app.hosts.form.keychain_picker.query().to_lowercase();
    app.keychain
        .list
        .iter()
        .filter(|entry| {
            query.is_empty()
                || entry.name.to_lowercase().contains(&query)
                || entry.path.to_lowercase().contains(&query)
                || entry.key_type.to_lowercase().contains(&query)
        })
        .map(|entry| entry.id)
        .collect()
}

/// Returns the host form's visible `ProxyJump` options.
pub(crate) fn host_form_jump_ids(app: &App) -> Vec<HostId> {
    let query = app.hosts.form.jump_picker.query().to_lowercase();
    app.hosts
        .list
        .iter()
        .filter(|host| Some(host.id) != app.hosts.form.editing_id)
        .filter(|host| {
            query.is_empty()
                || host.name.to_lowercase().contains(&query)
                || host.hostname.to_lowercase().contains(&query)
                || host.username.to_lowercase().contains(&query)
        })
        .map(|host| host.id)
        .collect()
}

/// Returns the host form's visible identity options.
pub(crate) fn host_form_identity_ids(app: &App) -> Vec<IdentityId> {
    let query = app.hosts.form.identity_picker.query().to_lowercase();
    app.keychain
        .identities
        .iter()
        .filter(|identity| {
            query.is_empty()
                || identity.label.to_lowercase().contains(&query)
                || identity.username.to_lowercase().contains(&query)
        })
        .map(|identity| identity.id)
        .collect()
}

/// Returns the host form's visible tag-picker rows.
pub(crate) fn host_form_tag_ids(app: &App) -> Vec<TagPickerItem> {
    let query = app.hosts.form.tag_picker.query().trim();
    let query_lc = query.to_lowercase();
    let all_tags = app.hosts.all_tags();
    let candidate = clean_tag(query);
    let candidate_is_new = candidate.as_ref().is_none_or(|candidate| {
        !all_tags
            .iter()
            .any(|tag| tag.to_lowercase() == candidate.to_lowercase())
    });

    let mut items = Vec::with_capacity(all_tags.len() + 1);
    if candidate_is_new {
        items.push(TagPickerItem::Create(candidate.unwrap_or_default()));
    }
    items.extend(
        all_tags
            .into_iter()
            .filter(|tag| !app.hosts.form.tags.contains(tag))
            .filter(|tag| query_lc.is_empty() || tag.to_lowercase().contains(&query_lc))
            .map(TagPickerItem::Existing),
    );
    items
}

/// Returns the port-forward form's visible host options.
pub(crate) fn port_forward_host_ids(app: &App) -> Vec<HostId> {
    let query = app.port_forwards.form.host_picker.query().to_lowercase();
    app.hosts
        .list
        .iter()
        .filter(|host| {
            query.is_empty()
                || host.name.to_lowercase().contains(&query)
                || host.hostname.to_lowercase().contains(&query)
                || host.username.to_lowercase().contains(&query)
        })
        .map(|host| host.id)
        .collect()
}

/// Returns the package IDs shown by either snippet package picker.
pub(crate) fn snippet_package_ids(app: &App) -> Vec<SnippetPackageId> {
    app.snippets
        .packages
        .iter()
        .map(|package| package.id)
        .collect()
}

/// Returns the host toolbar's tag-filter options.
pub(crate) fn host_tag_filter_ids(app: &App) -> Vec<HostTagFilter> {
    std::iter::once(HostTagFilter::All)
        .chain(app.hosts.all_tags().into_iter().map(HostTagFilter::Tag))
        .collect()
}

/// Returns the fixed theme options shown by Settings.
const fn settings_theme_ids() -> &'static [ThemeMode] {
    ThemeMode::all()
}

/// Returns the fixed history-limit options shown by Settings.
const fn settings_history_ids() -> &'static [usize] {
    &COMMAND_HISTORY_LIMITS
}

/// Returns the fixed local-shell options shown by Windows Profiles.
const fn settings_local_shell_ids() -> &'static [LocalShell] {
    LocalShell::windows_options()
}

/// Returns the fixed diagnostic-level options shown by Settings.
pub(crate) const fn settings_diagnostics_ids() -> &'static [crate::core::diagnostics::Level] {
    use crate::core::diagnostics::Level;
    &[
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ]
}

fn reset_dropdown(app: &mut App, target: DropdownTarget) {
    match target {
        DropdownTarget::HostFormKeychain => app.hosts.form.keychain_picker.reset(),
        DropdownTarget::HostFormJump => app.hosts.form.jump_picker.reset(),
        DropdownTarget::HostFormIdentity => app.hosts.form.identity_picker.reset(),
        DropdownTarget::HostFormTag => app.hosts.form.tag_picker.reset(),
        DropdownTarget::PortForwardHost => app.port_forwards.form.host_picker.reset(),
        DropdownTarget::SnippetFormPackage => app.snippets.form.package_picker.reset(),
        DropdownTarget::SnippetSidebarPackage => app.snippets.right_package_picker.reset(),
        DropdownTarget::HostsTagFilter => app.hosts.tag_filter_picker.reset(),
        DropdownTarget::HostsTransferMenu => app.hosts.transfer_menu.reset(),
        DropdownTarget::WorkspaceTransferMenu => app.workspace_manager.transfer_menu.reset(),
        DropdownTarget::SettingsTheme => app.preferences.theme_picker.reset(),
        DropdownTarget::SettingsLocale => app.preferences.locale_picker.reset(),
        DropdownTarget::SettingsShellIntegration => {
            app.preferences.shell_integration_picker.reset();
        }
        DropdownTarget::SettingsLocalShell => app.preferences.local_shell_picker.reset(),
        DropdownTarget::SettingsHistoryLimit => {
            app.preferences.command_history_limit_picker.reset();
        }
        DropdownTarget::SettingsDiagnostics => app.preferences.diagnostics_picker.reset(),
    }
}

/// Closes the active dropdown and discards its transient filter and focus.
pub(crate) fn close_active_dropdown(app: &mut App) {
    if let Some(target) = app.active_dropdown.take() {
        reset_dropdown(app, target);
    }
}

/// Closes the active dropdown and lets the focus stack restore its owner.
pub(crate) fn dismiss_active_dropdown(app: &mut App) -> Task<Message> {
    if app.active_dropdown.is_none() {
        return Task::none();
    }
    close_active_dropdown(app);
    Task::none()
}

/// Closes one dropdown when it is the active popup.
pub(crate) fn close_dropdown(app: &mut App, target: DropdownTarget) {
    if app.active_dropdown == Some(target) {
        app.active_dropdown = None;
        reset_dropdown(app, target);
    }
}

/// Opens a dropdown, or closes it when it is already active.
pub(crate) fn toggle_dropdown(app: &mut App, target: DropdownTarget) -> Task<Message> {
    if app.active_dropdown == Some(target) {
        return dismiss_active_dropdown(app);
    }
    close_active_dropdown(app);

    let index = match target {
        DropdownTarget::HostFormKeychain => {
            let items = host_form_keychain_ids(app);
            let current = app
                .keychain
                .list
                .iter()
                .find(|entry| entry.path == app.hosts.form.key_path)
                .map(|entry| entry.id);
            app.hosts.form.keychain_picker.begin(current, &items);
            app.hosts.form.keychain_picker.active_index(&items)
        }
        DropdownTarget::HostFormJump => {
            let items = host_form_jump_ids(app);
            app.hosts
                .form
                .jump_picker
                .begin(app.hosts.form.jump_host_id, &items);
            app.hosts.form.jump_picker.active_index(&items)
        }
        DropdownTarget::HostFormIdentity => {
            let items = host_form_identity_ids(app);
            app.hosts
                .form
                .identity_picker
                .begin(app.hosts.form.selected_identity_id, &items);
            app.hosts.form.identity_picker.active_index(&items)
        }
        DropdownTarget::HostFormTag => {
            let items = host_form_tag_ids(app);
            app.hosts.form.tag_picker.begin(None, &items);
            app.hosts.form.tag_picker.active_index(&items)
        }
        DropdownTarget::PortForwardHost => {
            let items = port_forward_host_ids(app);
            let current = app.port_forwards.form.host_id;
            app.port_forwards.form.host_picker.begin(current, &items);
            app.port_forwards.form.host_picker.active_index(&items)
        }
        DropdownTarget::SnippetFormPackage => {
            let items = snippet_package_ids(app);
            app.snippets
                .form
                .package_picker
                .begin(app.snippets.form.package_id, &items);
            app.snippets.form.package_picker.active_index(&items)
        }
        DropdownTarget::SnippetSidebarPackage => {
            let items = snippet_package_ids(app);
            app.snippets
                .right_package_picker
                .begin(app.snippets.right_package_id, &items);
            app.snippets.right_package_picker.active_index(&items)
        }
        DropdownTarget::HostsTagFilter => {
            let items = host_tag_filter_ids(app);
            let current = app.hosts.tag_filter.clone();
            app.hosts.tag_filter_picker.begin(Some(current), &items);
            app.hosts.tag_filter_picker.active_index(&items)
        }
        DropdownTarget::HostsTransferMenu => {
            app.hosts
                .transfer_menu
                .begin(None, TransferMenuAction::all());
            app.hosts
                .transfer_menu
                .active_index(TransferMenuAction::all())
        }
        DropdownTarget::WorkspaceTransferMenu => {
            app.workspace_manager
                .transfer_menu
                .begin(None, TransferMenuAction::all());
            app.workspace_manager
                .transfer_menu
                .active_index(TransferMenuAction::all())
        }
        DropdownTarget::SettingsTheme => {
            app.preferences
                .theme_picker
                .begin(Some(app.preferences.theme_mode), settings_theme_ids());
            app.preferences
                .theme_picker
                .active_index(settings_theme_ids())
        }
        DropdownTarget::SettingsLocale => {
            app.preferences
                .locale_picker
                .begin(Some(app.preferences.locale), Locale::ALL);
            app.preferences.locale_picker.active_index(Locale::ALL)
        }
        DropdownTarget::SettingsShellIntegration => {
            app.preferences
                .shell_integration_picker
                .begin(Some(app.preferences.shell_integration), &[true, false]);
            app.preferences
                .shell_integration_picker
                .active_index(&[true, false])
        }
        DropdownTarget::SettingsLocalShell => {
            app.preferences.local_shell_picker.begin(
                Some(app.preferences.local_shell),
                settings_local_shell_ids(),
            );
            app.preferences
                .local_shell_picker
                .active_index(settings_local_shell_ids())
        }
        DropdownTarget::SettingsHistoryLimit => {
            app.preferences.command_history_limit_picker.begin(
                Some(app.preferences.command_history_limit),
                settings_history_ids(),
            );
            app.preferences
                .command_history_limit_picker
                .active_index(settings_history_ids())
        }
        DropdownTarget::SettingsDiagnostics => {
            app.preferences.diagnostics_picker.begin(
                Some(app.preferences.diagnostics_level),
                settings_diagnostics_ids(),
            );
            app.preferences
                .diagnostics_picker
                .active_index(settings_diagnostics_ids())
        }
    };

    app.active_dropdown = Some(target);
    index.map_or_else(Task::none, |index| {
        reveal_row(
            app,
            target.scroll_id(),
            index,
            RowMetrics {
                height: target.row_height(),
                gap: ROW_GAP,
                top_padding: 0.0,
            },
        )
    })
}

/// Moves keyboard focus inside the active dropdown.
pub(crate) fn move_dropdown(
    app: &mut App,
    target: DropdownTarget,
    direction: GridDirection,
) -> Task<Message> {
    if app.active_dropdown != Some(target) {
        return Task::none();
    }

    let index = match target {
        DropdownTarget::HostFormKeychain => {
            let items = host_form_keychain_ids(app);
            move_dropdown_state(&mut app.hosts.form.keychain_picker, &items, direction)
        }
        DropdownTarget::HostFormJump => {
            let items = host_form_jump_ids(app);
            move_dropdown_state(&mut app.hosts.form.jump_picker, &items, direction)
        }
        DropdownTarget::HostFormIdentity => {
            let items = host_form_identity_ids(app);
            move_dropdown_state(&mut app.hosts.form.identity_picker, &items, direction)
        }
        DropdownTarget::HostFormTag => {
            let items = host_form_tag_ids(app);
            move_dropdown_state(&mut app.hosts.form.tag_picker, &items, direction)
        }
        DropdownTarget::PortForwardHost => {
            let items = port_forward_host_ids(app);
            move_dropdown_state(&mut app.port_forwards.form.host_picker, &items, direction)
        }
        DropdownTarget::SnippetFormPackage => {
            let items = snippet_package_ids(app);
            move_dropdown_state(&mut app.snippets.form.package_picker, &items, direction)
        }
        DropdownTarget::SnippetSidebarPackage => {
            let items = snippet_package_ids(app);
            move_dropdown_state(&mut app.snippets.right_package_picker, &items, direction)
        }
        DropdownTarget::HostsTagFilter => {
            let items = host_tag_filter_ids(app);
            move_dropdown_state(&mut app.hosts.tag_filter_picker, &items, direction)
        }
        DropdownTarget::HostsTransferMenu => move_dropdown_state(
            &mut app.hosts.transfer_menu,
            TransferMenuAction::all(),
            direction,
        ),
        DropdownTarget::WorkspaceTransferMenu => move_dropdown_state(
            &mut app.workspace_manager.transfer_menu,
            TransferMenuAction::all(),
            direction,
        ),
        DropdownTarget::SettingsTheme => move_dropdown_state(
            &mut app.preferences.theme_picker,
            settings_theme_ids(),
            direction,
        ),
        DropdownTarget::SettingsLocale => {
            move_dropdown_state(&mut app.preferences.locale_picker, Locale::ALL, direction)
        }
        DropdownTarget::SettingsShellIntegration => move_dropdown_state(
            &mut app.preferences.shell_integration_picker,
            &[true, false],
            direction,
        ),
        DropdownTarget::SettingsLocalShell => move_dropdown_state(
            &mut app.preferences.local_shell_picker,
            settings_local_shell_ids(),
            direction,
        ),
        DropdownTarget::SettingsHistoryLimit => move_dropdown_state(
            &mut app.preferences.command_history_limit_picker,
            settings_history_ids(),
            direction,
        ),
        DropdownTarget::SettingsDiagnostics => move_dropdown_state(
            &mut app.preferences.diagnostics_picker,
            settings_diagnostics_ids(),
            direction,
        ),
    };

    let Some(index) = index else {
        return Task::none();
    };
    reveal_row(
        app,
        target.scroll_id(),
        index,
        RowMetrics {
            height: target.row_height(),
            gap: ROW_GAP,
            top_padding: 0.0,
        },
    )
}

fn move_dropdown_state<T: Clone + Eq>(
    state: &mut DropdownState<T>,
    items: &[T],
    direction: GridDirection,
) -> Option<usize> {
    match direction {
        GridDirection::Up | GridDirection::Left => state.move_previous(items),
        GridDirection::Down | GridDirection::Right => state.move_next(items),
    }
    state.active_index(items)
}

/// Returns the message for the currently focused dropdown option.
pub(crate) fn dropdown_activation(app: &App, target: DropdownTarget) -> Option<Message> {
    if app.active_dropdown != Some(target) {
        return None;
    }

    match target {
        DropdownTarget::HostFormKeychain => {
            let items = host_form_keychain_ids(app);
            let id = *app.hosts.form.keychain_picker.active(&items)?;
            Some(Message::Hosts(
                crate::hosts::message::Message::FormFillKeyPath(id),
            ))
        }
        DropdownTarget::HostFormJump => {
            let items = host_form_jump_ids(app);
            let id = *app.hosts.form.jump_picker.active(&items)?;
            Some(Message::Hosts(
                crate::hosts::message::Message::FormSelectJump(id),
            ))
        }
        DropdownTarget::HostFormIdentity => {
            let items = host_form_identity_ids(app);
            let id = *app.hosts.form.identity_picker.active(&items)?;
            Some(Message::Hosts(
                crate::hosts::message::Message::FormSelectIdentity(id),
            ))
        }
        DropdownTarget::HostFormTag => {
            let items = host_form_tag_ids(app);
            match app.hosts.form.tag_picker.active(&items)? {
                TagPickerItem::Create(_) => Some(Message::Hosts(
                    crate::hosts::message::Message::FormTagCreate,
                )),
                TagPickerItem::Existing(tag) => Some(Message::Hosts(
                    crate::hosts::message::Message::FormTagAdd(tag.clone()),
                )),
            }
        }
        DropdownTarget::PortForwardHost => {
            let items = port_forward_host_ids(app);
            let id = *app.port_forwards.form.host_picker.active(&items)?;
            Some(Message::PortForward(
                crate::port_forward::message::Message::PortForwardFormHost(id),
            ))
        }
        DropdownTarget::SnippetFormPackage => {
            let items = snippet_package_ids(app);
            let id = *app.snippets.form.package_picker.active(&items)?;
            Some(Message::Snippets(
                crate::snippets::message::Message::SnippetFormPackage(Some(id)),
            ))
        }
        DropdownTarget::SnippetSidebarPackage => {
            let items = snippet_package_ids(app);
            let id = *app.snippets.right_package_picker.active(&items)?;
            Some(Message::Snippets(
                crate::snippets::message::Message::SnippetRightSidebarFormPackage(Some(id)),
            ))
        }
        DropdownTarget::HostsTagFilter => {
            let items = host_tag_filter_ids(app);
            let filter = app.hosts.tag_filter_picker.active(&items)?.clone();
            Some(Message::Hosts(
                crate::hosts::message::Message::HostTagFilterSelect(filter),
            ))
        }
        DropdownTarget::HostsTransferMenu => {
            let action = *app.hosts.transfer_menu.active(TransferMenuAction::all())?;
            Some(match action {
                TransferMenuAction::Export => {
                    Message::Hosts(crate::hosts::message::Message::HostExportRequest)
                }
                TransferMenuAction::Import => {
                    Message::Hosts(crate::hosts::message::Message::HostImportRequest)
                }
            })
        }
        DropdownTarget::WorkspaceTransferMenu => {
            let action = *app
                .workspace_manager
                .transfer_menu
                .active(TransferMenuAction::all())?;
            Some(match action {
                TransferMenuAction::Export => Message::WorkspaceExportRequest,
                TransferMenuAction::Import => Message::WorkspaceImportRequest,
            })
        }
        DropdownTarget::SettingsTheme => {
            let mode = *app.preferences.theme_picker.active(settings_theme_ids())?;
            Some(Message::SetTheme(mode))
        }
        DropdownTarget::SettingsLocale => {
            let locale = *app.preferences.locale_picker.active(Locale::ALL)?;
            Some(Message::SetLocale(locale))
        }
        DropdownTarget::SettingsShellIntegration => {
            let enabled = *app
                .preferences
                .shell_integration_picker
                .active(&[true, false])?;
            Some(Message::SetShellIntegration(enabled))
        }
        DropdownTarget::SettingsLocalShell => {
            let shell = *app
                .preferences
                .local_shell_picker
                .active(settings_local_shell_ids())?;
            Some(Message::SetLocalShell(shell))
        }
        DropdownTarget::SettingsHistoryLimit => {
            let limit = *app
                .preferences
                .command_history_limit_picker
                .active(settings_history_ids())?;
            Some(Message::SetCommandHistoryLimit(limit))
        }
        DropdownTarget::SettingsDiagnostics => {
            let level = *app
                .preferences
                .diagnostics_picker
                .active(settings_diagnostics_ids())?;
            Some(Message::SetDiagnosticsLevel(level))
        }
    }
}

/// Move the collection identified by `target`.
#[cfg(test)]
pub(crate) fn move_cursor(
    app: &mut App,
    target: CursorTarget,
    direction: GridDirection,
) -> Task<Message> {
    move_cursor_with_extend(app, target, direction, false)
}

/// Move a collection cursor, optionally extending a range selection.
pub(crate) fn move_cursor_with_extend(
    app: &mut App,
    target: CursorTarget,
    direction: GridDirection,
    extend: bool,
) -> Task<Message> {
    match target {
        CursorTarget::Hosts => {
            let visible: Vec<_> = app.hosts.filtered().iter().map(|host| host.id).collect();
            let columns = if app.hosts.grid_view {
                app.hosts.grid_columns
            } else {
                1
            };
            app.hosts.cursor.move_by(&visible, columns, direction);
        }
        CursorTarget::Keychain => move_keychain(app, direction),
        CursorTarget::PortForwards => {
            let visible: Vec<_> = app
                .port_forwards
                .filtered()
                .iter()
                .map(|forward| forward.id)
                .collect();
            let columns = if app.port_forwards.grid_view {
                app.port_forwards.grid_columns
            } else {
                1
            };
            app.port_forwards
                .cursor
                .move_by(&visible, columns, direction);
        }
        CursorTarget::Snippets => move_snippets(app, direction),
        CursorTarget::KnownHosts => {
            let visible: Vec<_> = app
                .known_hosts
                .filtered(&app.hosts.list)
                .iter()
                .map(|(known_host, _)| known_host.line_number)
                .collect();
            let columns = if app.known_hosts.grid_view {
                app.known_hosts.grid_columns
            } else {
                1
            };
            app.known_hosts.cursor.move_by(&visible, columns, direction);
        }
        CursorTarget::Workspaces => {
            let visible: Vec<_> = app
                .workspace_manager
                .list
                .iter()
                .map(|workspace| workspace.id.clone())
                .collect();
            move_list_cursor(&mut app.workspace_manager.cursor, &visible, direction);
        }
        CursorTarget::RightSidebarSnippets
        | CursorTarget::RightSidebarCommands
        | CursorTarget::RightSidebarTheme => {
            return move_right_sidebar(app, target, direction);
        }
        CursorTarget::SftpLocalFiles
        | CursorTarget::SftpRemoteFiles
        | CursorTarget::SftpLocalHostPicker
        | CursorTarget::SftpRemoteHostPicker => {
            return crate::sftp::update::move_cursor(app, target, direction, extend);
        }
    }
    reveal_collection_selection(app, target)
}

/// Store the measured grid width for a collection.
pub(crate) fn set_columns(app: &mut App, target: CursorTarget, columns: usize) -> Task<Message> {
    let columns = columns.max(1);
    match target {
        CursorTarget::Hosts => app.hosts.grid_columns = columns,
        CursorTarget::Keychain => app.keychain.grid_columns = columns,
        CursorTarget::PortForwards => app.port_forwards.grid_columns = columns,
        CursorTarget::Snippets => app.snippets.grid_columns = columns,
        CursorTarget::KnownHosts => app.known_hosts.grid_columns = columns,
        CursorTarget::Workspaces
        | CursorTarget::RightSidebarSnippets
        | CursorTarget::RightSidebarCommands
        | CursorTarget::RightSidebarTheme
        | CursorTarget::SftpLocalFiles
        | CursorTarget::SftpRemoteFiles
        | CursorTarget::SftpLocalHostPicker
        | CursorTarget::SftpRemoteHostPicker => {}
    }
    reveal_collection_selection(app, target)
}

/// Run the primary action for the selected item in a focused collection.
pub(crate) fn activate(app: &App, target: CursorTarget) -> Task<Message> {
    if matches!(
        target,
        CursorTarget::SftpLocalFiles
            | CursorTarget::SftpRemoteFiles
            | CursorTarget::SftpLocalHostPicker
            | CursorTarget::SftpRemoteHostPicker
    ) {
        return crate::sftp::update::activate_cursor(app, target);
    }
    activation(app, target).map_or_else(Task::none, Task::done)
}

pub(crate) fn activation(app: &App, target: CursorTarget) -> Option<Message> {
    match target {
        CursorTarget::Hosts => {
            let visible: Vec<_> = app.hosts.filtered().iter().map(|host| host.id).collect();
            app.hosts
                .cursor
                .selected_in(&visible)
                .copied()
                .map(|id| Message::Hosts(crate::hosts::message::Message::ConnectHost(id)))
        }
        CursorTarget::Keychain => match app.keychain.cursor.selected_key() {
            Some(KeychainSelection::Key(id))
                if app
                    .keychain
                    .filtered_keys()
                    .iter()
                    .any(|entry| entry.id == *id) =>
            {
                Some(Message::Keychain(
                    crate::keychain::message::Message::KeychainEdit(*id),
                ))
            }
            Some(KeychainSelection::Identity(id))
                if app
                    .keychain
                    .identities
                    .iter()
                    .any(|identity| identity.id == *id) =>
            {
                Some(Message::Keychain(
                    crate::keychain::message::Message::IdentityEdit(*id),
                ))
            }
            Some(KeychainSelection::Key(_) | KeychainSelection::Identity(_)) | None => None,
        },
        CursorTarget::PortForwards => {
            let visible: Vec<_> = app
                .port_forwards
                .filtered()
                .iter()
                .map(|forward| forward.id)
                .collect();
            app.port_forwards
                .cursor
                .selected_in(&visible)
                .copied()
                .map(|id| {
                    Message::PortForward(crate::port_forward::message::Message::PortForwardToggle(
                        id,
                    ))
                })
        }
        CursorTarget::Snippets => match app.snippets.cursor.selected_key() {
            Some(SnippetSelection::Snippet(id))
                if app.snippets.list.iter().any(|snippet| snippet.id == *id) =>
            {
                Some(Message::Snippets(
                    crate::snippets::message::Message::SnippetCopyScript(*id),
                ))
            }
            Some(SnippetSelection::Package(id))
                if app
                    .snippets
                    .packages
                    .iter()
                    .any(|package| package.id == *id) =>
            {
                Some(Message::Snippets(
                    crate::snippets::message::Message::SnippetPackageSelect(*id),
                ))
            }
            Some(SnippetSelection::Snippet(_) | SnippetSelection::Package(_)) | None => None,
        },
        CursorTarget::KnownHosts => {
            let visible: Vec<_> = app
                .known_hosts
                .filtered(&app.hosts.list)
                .iter()
                .map(|(known_host, _)| known_host.line_number)
                .collect();
            app.known_hosts
                .cursor
                .selected_in(&visible)
                .copied()
                .map(|id| {
                    Message::KnownHosts(crate::known_hosts::message::Message::KnownHostShowDetails(
                        id,
                    ))
                })
        }
        CursorTarget::Workspaces => {
            let visible: Vec<_> = app
                .workspace_manager
                .list
                .iter()
                .map(|workspace| workspace.id.clone())
                .collect();
            app.workspace_manager
                .cursor
                .selected_in(&visible)
                .map(|id| Message::WorkspaceApply(id.clone()))
        }
        CursorTarget::RightSidebarSnippets => app
            .sidebar()
            .and_then(|sidebar| sidebar.cursors.selected_snippet())
            .filter(|id| app.snippets.list.iter().any(|snippet| snippet.id == **id))
            .copied()
            .map(|id| Message::Snippets(crate::snippets::message::Message::SnippetRunScript(id))),
        CursorTarget::RightSidebarCommands => {
            let sidebar = app.sidebar()?;
            let selected = sidebar.cursors.selected_command()?;
            let session_id = app.active_terminal_session_id()?;
            let session = app
                .terminal
                .sessions
                .iter()
                .find(|session| session.id == session_id)?;
            let index = crate::views::right_sidebar::visible_command_indices(
                &session.recent_commands,
                &sidebar.commands_filter,
            )
            .into_iter()
            .find(|&index| session.recent_commands[index].command == *selected)?;
            Some(Message::CommandHistoryRun(index))
        }
        CursorTarget::RightSidebarTheme => app
            .sidebar()
            .and_then(|sidebar| sidebar.cursors.selected_theme())
            .copied()
            .map(Message::SetTheme),
        CursorTarget::SftpLocalFiles
        | CursorTarget::SftpRemoteFiles
        | CursorTarget::SftpLocalHostPicker
        | CursorTarget::SftpRemoteHostPicker => None,
    }
}

fn move_keychain(app: &mut App, direction: GridDirection) {
    let preferred_section = match app.keychain.cursor.selected_key() {
        Some(KeychainSelection::Key(_)) => Some(0),
        Some(KeychainSelection::Identity(_)) => Some(1),
        None => None,
    };
    let sections = vec![
        app.keychain
            .filtered_keys()
            .iter()
            .map(|entry| KeychainSelection::Key(entry.id))
            .collect(),
        app.keychain
            .identities
            .iter()
            .map(|identity| KeychainSelection::Identity(identity.id))
            .collect(),
    ];
    let columns = if app.keychain.grid_view {
        app.keychain.grid_columns
    } else {
        1
    };
    app.keychain
        .cursor
        .move_in_sections(&sections, columns, direction, preferred_section);
}

fn move_snippets(app: &mut App, direction: GridDirection) {
    let preferred_section = match app.snippets.cursor.selected_key() {
        Some(SnippetSelection::Package(_)) => Some(0),
        Some(SnippetSelection::Snippet(_)) => Some(1),
        None => None,
    };
    let sections = vec![
        app.snippets
            .packages
            .iter()
            .map(|package| SnippetSelection::Package(package.id))
            .collect(),
        app.snippets
            .list
            .iter()
            .map(|snippet| SnippetSelection::Snippet(snippet.id))
            .collect(),
    ];
    let columns = if app.snippets.grid_view {
        app.snippets.grid_columns
    } else {
        1
    };
    app.snippets
        .cursor
        .move_in_sections(&sections, columns, direction, preferred_section);
}

fn move_right_sidebar(
    app: &mut App,
    target: CursorTarget,
    direction: GridDirection,
) -> Task<Message> {
    let tab = match target {
        CursorTarget::RightSidebarSnippets => RightSidebarTab::Snippets,
        CursorTarget::RightSidebarCommands => RightSidebarTab::Commands,
        CursorTarget::RightSidebarTheme => RightSidebarTab::Theme,
        _ => return Task::none(),
    };
    let Some(sidebar_tab) = app.sidebar().map(|sidebar| sidebar.active_tab) else {
        return Task::none();
    };
    if sidebar_tab != tab {
        return Task::none();
    }

    let snippets = if tab == RightSidebarTab::Snippets {
        app.snippets.list.iter().map(|snippet| snippet.id).collect()
    } else {
        Vec::new()
    };
    let commands = if tab == RightSidebarTab::Commands {
        let filter = app
            .sidebar()
            .map_or("", |sidebar| sidebar.commands_filter.as_str());
        app.active_terminal_session_id()
            .and_then(|session_id| {
                app.terminal
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
            })
            .map(|session| {
                crate::views::right_sidebar::visible_command_indices(
                    &session.recent_commands,
                    filter,
                )
                .into_iter()
                .map(|index| session.recent_commands[index].command.clone())
                .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let themes = ThemeMode::all();

    update_active_sidebars(app, |sidebar| {
        sidebar
            .cursors
            .move_in(tab, &snippets, &commands, themes, direction);
    });

    let selected_index = app.sidebar().and_then(|sidebar| match tab {
        RightSidebarTab::Snippets => sidebar.cursors.selected_snippet().and_then(|selected| {
            snippets
                .iter()
                .position(|snippet_id| snippet_id == selected)
        }),
        RightSidebarTab::Commands => sidebar
            .cursors
            .selected_command()
            .and_then(|selected| commands.iter().position(|command| command == selected)),
        RightSidebarTab::Theme => sidebar
            .cursors
            .selected_theme()
            .and_then(|selected| themes.iter().position(|theme| theme == selected)),
    });

    scroll_right_sidebar_row(app, tab, selected_index)
}

fn move_list_cursor<T: Clone + Eq>(
    cursor: &mut crate::core::list_cursor::ListCursor<T>,
    items: &[T],
    direction: GridDirection,
) {
    match direction {
        GridDirection::Up | GridDirection::Left => cursor.move_previous(items),
        GridDirection::Down | GridDirection::Right => cursor.move_next(items),
    }
}

fn reveal_collection_selection(app: &App, target: CursorTarget) -> Task<Message> {
    match target {
        CursorTarget::Hosts => {
            use crate::hosts::view::{HOSTS_GRID_TOP_PADDING, VAULTS_HOSTS_SCROLL_ID};
            let visible: Vec<_> = app.hosts.filtered().iter().map(|host| host.id).collect();
            let Some(index) = app.hosts.cursor.selected_index(&visible) else {
                return Task::none();
            };
            let columns = collection::columns(app.hosts.grid_view, app.hosts.grid_columns);
            reveal_grid_item(
                app,
                VAULTS_HOSTS_SCROLL_ID,
                index,
                columns,
                collection::metrics(HOSTS_GRID_TOP_PADDING),
            )
        }
        CursorTarget::PortForwards => {
            use crate::port_forward::view::{
                PORT_FORWARDS_GRID_TOP_PADDING, PORT_FORWARDS_SCROLL_ID,
            };
            let visible: Vec<_> = app
                .port_forwards
                .filtered()
                .iter()
                .map(|forward| forward.id)
                .collect();
            let Some(index) = app.port_forwards.cursor.selected_index(&visible) else {
                return Task::none();
            };
            let columns =
                collection::columns(app.port_forwards.grid_view, app.port_forwards.grid_columns);
            reveal_grid_item(
                app,
                PORT_FORWARDS_SCROLL_ID,
                index,
                columns,
                collection::metrics(PORT_FORWARDS_GRID_TOP_PADDING),
            )
        }
        CursorTarget::KnownHosts => {
            use crate::known_hosts::view::{KNOWN_HOSTS_GRID_TOP_PADDING, KNOWN_HOSTS_SCROLL_ID};
            let visible: Vec<_> = app
                .known_hosts
                .filtered(&app.hosts.list)
                .iter()
                .map(|(known_host, _)| known_host.line_number)
                .collect();
            let Some(index) = app.known_hosts.cursor.selected_index(&visible) else {
                return Task::none();
            };
            let columns =
                collection::columns(app.known_hosts.grid_view, app.known_hosts.grid_columns);
            reveal_grid_item(
                app,
                KNOWN_HOSTS_SCROLL_ID,
                index,
                columns,
                collection::metrics(KNOWN_HOSTS_GRID_TOP_PADDING),
            )
        }
        CursorTarget::Keychain => reveal_keychain_selection(app),
        CursorTarget::Snippets => reveal_snippet_selection(app),
        CursorTarget::Workspaces => reveal_workspace_selection(app),
        CursorTarget::RightSidebarSnippets
        | CursorTarget::RightSidebarCommands
        | CursorTarget::RightSidebarTheme
        | CursorTarget::SftpLocalFiles
        | CursorTarget::SftpRemoteFiles
        | CursorTarget::SftpLocalHostPicker
        | CursorTarget::SftpRemoteHostPicker => Task::none(),
    }
}

fn reveal_keychain_selection(app: &App) -> Task<Message> {
    use crate::keychain::view::{
        KEYCHAIN_GRID_TOP_PADDING, KEYCHAIN_KEY_GRID_BOTTOM_PADDING, KEYCHAIN_SCROLL_ID,
    };
    let columns = collection::columns(app.keychain.grid_view, app.keychain.grid_columns);
    let keys = app.keychain.filtered_keys();
    let identities = &app.keychain.identities;
    let (index, top_padding) = match app.keychain.cursor.selected_key() {
        Some(KeychainSelection::Key(id)) => {
            let Some(index) = keys.iter().position(|entry| entry.id == *id) else {
                return Task::none();
            };
            (index, KEYCHAIN_GRID_TOP_PADDING)
        }
        Some(KeychainSelection::Identity(id)) => {
            let Some(index) = identities.iter().position(|entry| entry.id == *id) else {
                return Task::none();
            };
            let keys_height = collection::section_height(
                keys.len(),
                columns,
                KEYCHAIN_GRID_TOP_PADDING,
                KEYCHAIN_KEY_GRID_BOTTOM_PADDING,
            );
            (
                index,
                keys_height
                    + collection::SECTION_GAP
                    + collection::SECTION_DIVIDER_HEIGHT
                    + collection::SECTION_GAP
                    + collection::SECTION_HEADER_HEIGHT
                    + collection::SECTION_HEADER_GAP
                    + KEYCHAIN_GRID_TOP_PADDING,
            )
        }
        None => return Task::none(),
    };
    reveal_grid_item(
        app,
        KEYCHAIN_SCROLL_ID,
        index,
        columns,
        RowMetrics {
            height: collection::CARD_HEIGHT,
            gap: collection::ROW_GAP,
            top_padding,
        },
    )
}

fn reveal_snippet_selection(app: &App) -> Task<Message> {
    use crate::snippets::view::{
        SNIPPETS_GRID_TOP_PADDING, SNIPPETS_PACKAGE_GRID_BOTTOM_PADDING, SNIPPETS_SCROLL_ID,
    };
    let columns = collection::columns(app.snippets.grid_view, app.snippets.grid_columns);
    let (index, top_padding) = match app.snippets.cursor.selected_key() {
        Some(SnippetSelection::Package(id)) => {
            let Some(index) = app
                .snippets
                .packages
                .iter()
                .position(|package| package.id == *id)
            else {
                return Task::none();
            };
            (
                index,
                collection::SECTION_HEADER_HEIGHT
                    + collection::SECTION_HEADER_GAP
                    + SNIPPETS_GRID_TOP_PADDING,
            )
        }
        Some(SnippetSelection::Snippet(id)) => {
            let Some(index) = app
                .snippets
                .list
                .iter()
                .position(|snippet| snippet.id == *id)
            else {
                return Task::none();
            };
            let package_section_height = collection::section_height(
                app.snippets.packages.len(),
                columns,
                SNIPPETS_GRID_TOP_PADDING,
                SNIPPETS_PACKAGE_GRID_BOTTOM_PADDING,
            );
            (
                index,
                package_section_height
                    + collection::SECTION_GAP
                    + collection::SECTION_DIVIDER_HEIGHT
                    + collection::SECTION_GAP
                    + collection::SECTION_HEADER_HEIGHT
                    + collection::SECTION_HEADER_GAP
                    + SNIPPETS_GRID_TOP_PADDING,
            )
        }
        None => return Task::none(),
    };
    reveal_grid_item(
        app,
        SNIPPETS_SCROLL_ID,
        index,
        columns,
        RowMetrics {
            height: collection::CARD_HEIGHT,
            gap: collection::ROW_GAP,
            top_padding,
        },
    )
}

fn reveal_workspace_selection(app: &App) -> Task<Message> {
    use crate::views::workspaces::{
        WORKSPACE_LIST_TOP_PADDING, WORKSPACE_ROW_GAP, workspace_row_height,
    };
    let visible: Vec<_> = app
        .workspace_manager
        .list
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect();
    let Some(index) = app.workspace_manager.cursor.selected_index(&visible) else {
        return Task::none();
    };
    reveal_grid_item(
        app,
        crate::views::settings::SETTINGS_SCROLL_ID,
        index,
        1,
        RowMetrics {
            height: workspace_row_height(),
            gap: WORKSPACE_ROW_GAP,
            top_padding: WORKSPACE_LIST_TOP_PADDING,
        },
    )
}

/// Apply a sidebar mutation to every pane that shares the current sidebar.
fn update_active_sidebars(app: &mut App, mut update: impl FnMut(&mut SidebarState)) {
    if app.is_active_session_in_split() {
        for sid in app.active_split_pane_sessions() {
            if let Some(session) = app.terminal.sessions.iter_mut().find(|s| s.id == sid) {
                update(&mut session.sidebar);
            }
        }
    } else if let Some(sidebar) = app.sidebar_mut() {
        update(sidebar);
    }
}

pub(crate) fn scroll_right_sidebar_row(
    app: &App,
    tab: RightSidebarTab,
    selected_index: Option<usize>,
) -> Task<Message> {
    use crate::views::right_sidebar::{
        RIGHT_SIDEBAR_COMMAND_GAP, RIGHT_SIDEBAR_COMMAND_ROW_HEIGHT,
        RIGHT_SIDEBAR_COMMANDS_SCROLL_ID, RIGHT_SIDEBAR_SNIPPET_GAP,
        RIGHT_SIDEBAR_SNIPPET_ROW_HEIGHT, RIGHT_SIDEBAR_SNIPPETS_SCROLL_ID,
        RIGHT_SIDEBAR_THEME_GAP, RIGHT_SIDEBAR_THEME_SCROLL_ID,
    };
    let Some(index) = selected_index else {
        return Task::none();
    };
    let (scroll_id, row_height, gap) = match tab {
        RightSidebarTab::Snippets => (
            RIGHT_SIDEBAR_SNIPPETS_SCROLL_ID,
            RIGHT_SIDEBAR_SNIPPET_ROW_HEIGHT,
            RIGHT_SIDEBAR_SNIPPET_GAP,
        ),
        RightSidebarTab::Commands => (
            RIGHT_SIDEBAR_COMMANDS_SCROLL_ID,
            RIGHT_SIDEBAR_COMMAND_ROW_HEIGHT,
            RIGHT_SIDEBAR_COMMAND_GAP,
        ),
        RightSidebarTab::Theme => (
            RIGHT_SIDEBAR_THEME_SCROLL_ID,
            crate::views::ITEM_HEIGHT,
            RIGHT_SIDEBAR_THEME_GAP,
        ),
    };
    reveal_grid_item(
        app,
        scroll_id,
        index,
        1,
        RowMetrics {
            height: row_height,
            gap,
            top_padding: 0.0,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{GridDirection, RowMetrics, move_cursor, reveal_grid_item};
    use crate::core::collection::{
        CARD_HEIGHT, ROW_GAP, SECTION_HEADER_GAP, SECTION_HEADER_HEIGHT, grid_height,
        section_height,
    };
    use crate::core::cursor_focus::CursorTarget;
    use crate::core::test_support::{app, drain, host};
    use crate::hosts::view::VAULTS_HOSTS_SCROLL_ID;

    #[test]
    fn grid_item_indices_are_converted_to_rendered_rows() {
        let mut app = app();
        app.scroll_offsets.insert(VAULTS_HOSTS_SCROLL_ID, 0.0);
        app.scroll_viewport_heights
            .insert(VAULTS_HOSTS_SCROLL_ID, 100.0);
        let metrics = RowMetrics {
            height: CARD_HEIGHT,
            gap: ROW_GAP,
            top_padding: 4.0,
        };

        let first_row = drain(reveal_grid_item(
            &app,
            VAULTS_HOSTS_SCROLL_ID,
            2,
            3,
            metrics,
        ));
        assert_eq!(first_row.other_actions, 0, "row zero is already visible");

        let second_row = drain(reveal_grid_item(
            &app,
            VAULTS_HOSTS_SCROLL_ID,
            5,
            3,
            metrics,
        ));
        assert_eq!(
            second_row.other_actions, 1,
            "item five is on the second rendered row"
        );
    }

    #[test]
    fn moving_a_host_cursor_scrolls_its_grid_row_into_view() {
        let mut app = app();
        app.hosts.list = (1..=7)
            .map(|id| host(id, &format!("host-{id}"), "10.0.0.1"))
            .collect();
        app.hosts.grid_view = true;
        app.hosts.grid_columns = 3;
        let selected = app.hosts.filtered()[3].id;
        app.hosts.cursor.select(selected);
        app.scroll_offsets.insert(VAULTS_HOSTS_SCROLL_ID, 0.0);
        app.scroll_viewport_heights
            .insert(VAULTS_HOSTS_SCROLL_ID, 100.0);

        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::Hosts,
            GridDirection::Down,
        ));

        assert_ne!(app.hosts.cursor.selected_key(), Some(&selected));
        assert_eq!(moved.other_actions, 1, "the next grid row is revealed");
    }

    #[test]
    fn every_vault_grid_follows_its_keyboard_selection() {
        let mut app = app();
        app.port_forwards.list = (1..=7)
            .map(|id| crate::core::data::PortForward {
                id: crate::core::data::PortForwardId(id),
                name: format!("forward-{id}"),
                kind: crate::core::data::PortForwardKind::Local,
                host_id: crate::core::data::HostId(1),
                bind_address: "127.0.0.1".into(),
                listen_port: 8000 + id as u16,
                dest_host: "localhost".into(),
                dest_port: 80,
            })
            .collect();
        app.known_hosts.entries = (1..=7)
            .map(|line_number| crate::core::data::KnownHost {
                line_number,
                hostname: format!("host-{line_number}.example"),
                port: 22,
                key_type: "ssh-ed25519".into(),
                public_key: "ssh-ed25519 AAAA".into(),
                fingerprint: format!("SHA256:{line_number}"),
                marker: None,
                raw_pattern: format!("host-{line_number}.example"),
            })
            .collect();
        app.keychain.list = (1..=7)
            .map(|id| crate::core::data::KeychainEntry {
                id: crate::core::data::KeychainEntryId(id),
                name: format!("key-{id}"),
                key_type: "ed25519".into(),
                fingerprint: format!("SHA256:{id}"),
                public_key: "ssh-ed25519 AAAA".into(),
                path: format!("/tmp/key-{id}"),
                created_at: String::new(),
            })
            .collect();
        app.snippets.packages = (1..=7)
            .map(|id| crate::core::data::SnippetPackage {
                id: crate::core::data::SnippetPackageId(id),
                name: format!("package-{id}"),
                description: String::new(),
            })
            .collect();
        for scroll_id in [
            crate::port_forward::view::PORT_FORWARDS_SCROLL_ID,
            crate::known_hosts::view::KNOWN_HOSTS_SCROLL_ID,
            crate::keychain::view::KEYCHAIN_SCROLL_ID,
            crate::snippets::view::SNIPPETS_SCROLL_ID,
        ] {
            app.scroll_offsets.insert(scroll_id, 0.0);
            app.scroll_viewport_heights.insert(scroll_id, 100.0);
        }

        app.port_forwards.grid_view = true;
        app.port_forwards.grid_columns = 3;
        let forward = app.port_forwards.filtered()[3].id;
        app.port_forwards.cursor.select(forward);
        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::PortForwards,
            GridDirection::Down,
        ));
        assert_eq!(moved.other_actions, 1, "port forward grid follows");

        app.known_hosts.grid_view = true;
        app.known_hosts.grid_columns = 3;
        let known = app.known_hosts.filtered(&[])[3].0.line_number;
        app.known_hosts.cursor.select(known);
        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::KnownHosts,
            GridDirection::Down,
        ));
        assert_eq!(moved.other_actions, 1, "known hosts grid follows");

        app.keychain.grid_view = true;
        app.keychain.grid_columns = 3;
        let key = app.keychain.filtered_keys()[3].id;
        app.keychain
            .cursor
            .select(crate::keychain::KeychainSelection::Key(key));
        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::Keychain,
            GridDirection::Down,
        ));
        assert_eq!(moved.other_actions, 1, "keychain grid follows");

        app.keychain.identities = (1..=7)
            .map(|id| crate::core::data::Identity {
                id: crate::core::data::IdentityId(id),
                label: format!("identity-{id}"),
                username: format!("user-{id}"),
                password: secrecy::SecretString::from(format!("password-{id}")),
                created_at: String::new(),
            })
            .collect();
        let identity = app.keychain.identities[3].id;
        app.keychain
            .cursor
            .select(crate::keychain::KeychainSelection::Identity(identity));
        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::Keychain,
            GridDirection::Down,
        ));
        assert_eq!(
            moved.other_actions, 1,
            "the identities section accounts for preceding keys"
        );

        app.snippets.grid_view = true;
        app.snippets.grid_columns = 3;
        let package = app.snippets.packages[3].id;
        app.snippets
            .cursor
            .select(crate::snippets::SnippetSelection::Package(package));
        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::Snippets,
            GridDirection::Down,
        ));
        assert_eq!(moved.other_actions, 1, "snippets grid follows");

        app.snippets.list = (1..=7)
            .map(|id| crate::core::data::Snippet {
                id: crate::core::data::SnippetId(id),
                description: format!("snippet-{id}"),
                script: "echo test".into(),
                package_id: None,
                last_used_at: String::new(),
            })
            .collect();
        let snippet = app.snippets.list[3].id;
        app.snippets
            .cursor
            .select(crate::snippets::SnippetSelection::Snippet(snippet));
        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::Snippets,
            GridDirection::Down,
        ));
        assert_eq!(
            moved.other_actions, 1,
            "the snippets section accounts for preceding packages"
        );
    }

    #[test]
    fn moving_a_workspace_cursor_scrolls_the_settings_page() {
        let mut app = app();
        app.workspace_manager.list = (1..=5)
            .map(|index| crate::workspace::SavedWorkspace {
                id: format!("workspace-{index}"),
                name: format!("Workspace {index}"),
                updated_at: 0,
                snapshot: crate::workspace::WorkspaceSnapshot {
                    version: 1,
                    tabs: Vec::new(),
                    active_tab: None,
                    sessions: Vec::new(),
                    splits: Vec::new(),
                },
            })
            .collect();
        app.workspace_manager.cursor.select("workspace-2".into());
        app.scroll_offsets
            .insert(crate::views::settings::SETTINGS_SCROLL_ID, 0.0);
        app.scroll_viewport_heights
            .insert(crate::views::settings::SETTINGS_SCROLL_ID, 100.0);

        let moved = drain(move_cursor(
            &mut app,
            CursorTarget::Workspaces,
            GridDirection::Down,
        ));

        assert_eq!(
            app.workspace_manager
                .cursor
                .selected_key()
                .map(String::as_str),
            Some("workspace-3")
        );
        assert_eq!(moved.other_actions, 1);
    }

    #[test]
    fn section_heights_include_headers_gaps_and_partial_rows() {
        let rows = grid_height(4, 3, 6.0, 10.0);
        assert_eq!(rows, 6.0 + 2.0 * CARD_HEIGHT + ROW_GAP + 10.0);
        assert_eq!(
            section_height(4, 3, 6.0, 10.0),
            rows + SECTION_HEADER_HEIGHT + SECTION_HEADER_GAP
        );
    }
}
