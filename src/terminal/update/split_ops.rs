//! Split lifecycle, focus, and tab-promotion operations.

// Rust guideline compliant 2026-10-09

use iced::Task;
use iced::widget::pane_grid;

use super::{close_session, focus_session, remove_session};
use crate::app::{App, SessionId, SplitId, TabId};
use crate::core::data::LogCategory;
use crate::core::message::Message;
use crate::terminal::split::{
    SplitDirection, axis as split_axis, inject as inject_split, pane_for_session,
    split_configuration as split_state_to_configuration,
};
use crate::terminal::{Split, TabKind};

/// Close every session in a split group and remove the split's tab, activating
/// the neighboring tab.
pub(super) fn close_split_group(app: &mut App, split_id: SplitId) -> Task<Message> {
    let Some(split) = app.terminal.splits.get(&split_id) else {
        return Task::none();
    };
    let split_session_ids: Vec<SessionId> = split.panes.iter().map(|(_, sid)| *sid).collect();

    // Remove the split entity and its tab before closing its sessions.
    let split_tid = app.tab_id_for_split(split_id);
    app.terminal.splits.remove(&split_id);
    if let Some(tid) = split_tid {
        app.activate_neighbor_after_close(tid);
        app.remove_tab(tid);
    }
    app.terminal.drag_preview = None;
    app.terminal.dragging_terminal_tab = None;

    for sid in &split_session_ids {
        if let Some(label) = remove_session(app, *sid) {
            app.add_log(
                LogCategory::Terminal,
                format!("Closed terminal \"{label}\" (session_id={sid})"),
            );
        }
    }

    Task::none()
}

/// Set `split_id`'s focused pane to the one hosting `session_id`, preserving the
/// previous focus. No-op if the session isn't hosted by this split.
pub(super) fn focus_split_on_session(app: &mut App, split_id: SplitId, session_id: SessionId) {
    let Some(split) = app.terminal.splits.get_mut(&split_id) else {
        return;
    };
    let Some(pane) = pane_for_session(&split.panes, session_id) else {
        return;
    };
    split.focus_pane(pane);
}

/// Detach the dragged side of a split drop. If the dragged tab represents a
/// split group, the whole group is removed from `splits` (along with its tab)
/// and returned as a single `Configuration`; otherwise the standalone session is
/// returned as a single-pane configuration. The sessions themselves are left
/// alive — only the split grouping is torn down.
fn detach_dragged_split(
    app: &mut App,
    dragged_session: SessionId,
) -> pane_grid::Configuration<SessionId> {
    if let Some(split_id) = app.split_for_session(dragged_session) {
        // `split_for_session` just resolved this id, so the lookup normally
        // succeeds; if the split vanished in between, fall through to the
        // standalone-pane path below instead of panicking.
        if let Some(split) = app.terminal.splits.remove(&split_id) {
            if let Some(tid) = app.tab_id_for_split(split_id) {
                app.remove_tab(tid);
            }
            return split_state_to_configuration(&split);
        }
    }
    if let Some(tid) = app.tab_id_for_session(dragged_session) {
        app.remove_tab(tid);
    }
    pane_grid::Configuration::Pane(dragged_session)
}

pub(super) fn create_split(
    app: &mut App,
    target_session: SessionId,
    dragged_session: SessionId,
    direction: SplitDirection,
) -> Task<Message> {
    let (axis, place_before_target) = split_axis(direction);
    // Detach the dragged side up-front (may be a whole split group); this also
    // removes its tab. The target is always a standalone session here.
    let dragged_config = detach_dragged_split(app, dragged_session);

    let target_config = pane_grid::Configuration::Pane(target_session);
    let (a, b) = if place_before_target {
        (dragged_config, target_config)
    } else {
        (target_config, dragged_config)
    };
    let config = pane_grid::Configuration::Split {
        axis,
        ratio: 0.5,
        a: Box::new(a),
        b: Box::new(b),
    };

    let panes = pane_grid::State::with_configuration(config);
    let focus = pane_for_session(&panes, dragged_session);

    // Remove the target session's standalone tab; the split tab takes its place.
    let target_tid = app.tab_id_for_session(target_session);
    let target_pos = target_tid.and_then(|tid| app.terminal.tabs.iter().position(|&t| t == tid));
    if let Some(tid) = target_tid {
        app.remove_tab(tid);
    }

    let split_id = app.allocate_split_id();
    app.terminal.splits.insert(
        split_id,
        Split {
            id: split_id,
            panes,
            focus,
            previous_focus: None,
        },
    );

    let split_tid = allocate_tab_at(
        app,
        TabKind::Split(split_id),
        target_pos.unwrap_or(usize::MAX),
    );
    app.activate_tab(split_tid);

    let dir_label = direction.label();
    app.add_log(
        LogCategory::Terminal,
        format!(
            "Split view ({dir_label}) created with sessions {target_session} and {dragged_session}"
        ),
    );

    focus_session(app, dragged_session)
}

pub(super) fn split_existing_pane(
    app: &mut App,
    split_id: SplitId,
    target_pane: pane_grid::Pane,
    dragged_session: SessionId,
    direction: SplitDirection,
) -> Task<Message> {
    let target_session = app
        .terminal
        .splits
        .get(&split_id)
        .and_then(|s| s.panes.get(target_pane))
        .copied();
    let Some(target_session) = target_session else {
        return Task::none();
    };

    if target_session == dragged_session {
        return focus_split_pane(app, split_id, target_pane);
    }

    // A draggable tab is always its split group's focused pane, so a drag whose
    // session already lives in the target split cannot arise from the drop
    // path. Guard anyway: detaching it below would tear down the very split we
    // are about to extend.
    if app.split_for_session(dragged_session) == Some(split_id) {
        return focus_split_pane(app, split_id, target_pane);
    }

    let (axis, place_before_target) = split_axis(direction);

    let dragged_config = detach_dragged_split(app, dragged_session);

    let merged_config = {
        let Some(split) = app.terminal.splits.get(&split_id) else {
            return Task::none();
        };
        let target_config = split_state_to_configuration(split);
        inject_split(
            target_config,
            target_session,
            axis,
            place_before_target,
            &dragged_config,
        )
    };

    let panes = pane_grid::State::with_configuration(merged_config);
    let focus = pane_for_session(&panes, dragged_session);

    if let Some(split) = app.terminal.splits.get_mut(&split_id) {
        split.panes = panes;
        split.previous_focus = split.focus;
        split.focus = focus;
    }

    let dir_label = direction.label();
    app.add_log(
        LogCategory::Terminal,
        format!(
            "Split pane ({dir_label}) added session {dragged_session} next to {target_session}"
        ),
    );

    focus_session(app, dragged_session)
}

/// Allocate a dynamic tab and move it to `position` in the tab order (positions
/// at or past the end append). Returns the new tab id.
pub(crate) fn allocate_tab_at(app: &mut App, kind: TabKind, position: usize) -> TabId {
    let tid = app.allocate_tab_id(kind);
    if let Some(cur) = app.terminal.tabs.iter().position(|&t| t == tid) {
        let tab = app.terminal.tabs.remove(cur);
        let insert = position.min(app.terminal.tabs.len());
        app.terminal.tabs.insert(insert, tab);
    }
    tid
}

/// Promote a session to a standalone tab at `position`, inheriting `locked`.
/// Used when a split dissolves so the surviving session keeps its lock.
fn promote_session_to_tab(
    app: &mut App,
    session_id: SessionId,
    position: usize,
    locked: bool,
) -> TabId {
    let tid = allocate_tab_at(app, TabKind::Session(session_id), position);
    app.set_tab_locked_by_id(tid, locked);
    tid
}
pub(super) fn focus_split_pane(
    app: &mut App,
    split_id: SplitId,
    pane: pane_grid::Pane,
) -> Task<Message> {
    let session_id = app
        .terminal
        .splits
        .get(&split_id)
        .and_then(|s| s.panes.get(pane))
        .copied();
    let Some(session_id) = session_id else {
        return Task::none();
    };
    focus_session(app, session_id)
}

/// Close a specific pane from the split. Dissolves the split if one pane remains.
pub(super) fn close_split_pane(
    app: &mut App,
    split_id: SplitId,
    pane: pane_grid::Pane,
) -> Task<Message> {
    let (closed_sid, close_result) = {
        let Some(split) = app.terminal.splits.get_mut(&split_id) else {
            return Task::none();
        };
        let closed_sid = split.panes.get(pane).copied();
        let result = split.panes.close(pane);
        (closed_sid, result)
    };
    let Some(closed_sid) = closed_sid else {
        return Task::none();
    };

    if let Some((_, sibling)) = close_result {
        // Repoint focus at the sibling, tracking whether the split now has
        // a single pane (and must dissolve into a standalone tab).
        let (dissolve, sibling_sid) = {
            let Some(split) = app.terminal.splits.get_mut(&split_id) else {
                return close_session(app, closed_sid);
            };
            split.focus = Some(sibling);
            split.previous_focus = None;
            let dissolve = split.panes.len() == 1;
            let sid = split.panes.get(sibling).copied();
            (dissolve, sid)
        };

        if dissolve {
            // Promote the sole remaining session to a standalone tab,
            // carrying over the split's locked flag so closing a single
            // pane does not silently unlock the tab.
            let split_tid = app.tab_id_for_split(split_id);
            let split_pos =
                split_tid.and_then(|tid| app.terminal.tabs.iter().position(|&t| t == tid));
            let was_locked = split_tid.is_some_and(|tid| app.tab_is_locked_by_id(tid));
            app.terminal.splits.remove(&split_id);
            if let Some(tid) = split_tid {
                app.remove_tab(tid);
            }
            if let Some(sid) = sibling_sid {
                let tid =
                    promote_session_to_tab(app, sid, split_pos.unwrap_or(usize::MAX), was_locked);
                app.activate_tab(tid);
            }
        }

        let close_task = close_session(app, closed_sid);

        if let Some(sid) = sibling_sid {
            return Task::batch([close_task, focus_session(app, sid)]);
        }
        close_task
    } else {
        let split_tid = app.tab_id_for_split(split_id);
        app.terminal.splits.remove(&split_id);
        if let Some(tid) = split_tid {
            app.activate_neighbor_after_close(tid);
            app.remove_tab(tid);
        }
        close_session(app, closed_sid)
    }
}

/// Extract the target pane from the split and promote it to a standalone tab.
/// The pane's session is kept alive; other panes remain in the split grid.
/// Dissolves the split if only one pane remains after extraction.
pub(super) fn unsplit_keep_pane(
    app: &mut App,
    split_id: SplitId,
    target: pane_grid::Pane,
) -> Task<Message> {
    let extracted_sid = app
        .terminal
        .splits
        .get(&split_id)
        .and_then(|s| s.panes.get(target))
        .copied();
    let Some(extracted_sid) = extracted_sid else {
        return Task::none();
    };

    // The split tab's current position, so the promoted tab(s) can land near it.
    let split_tid = app.tab_id_for_split(split_id);
    let split_pos = split_tid.and_then(|tid| app.terminal.tabs.iter().position(|&t| t == tid));
    // Unsplit keeps every terminal protected: the promoted tab(s) inherit the
    // split's locked flag so the operation never silently unlocks a tab.
    let was_locked = split_tid.is_some_and(|tid| app.tab_is_locked_by_id(tid));

    let close_result = {
        let Some(split) = app.terminal.splits.get_mut(&split_id) else {
            return Task::none();
        };
        split.panes.close(target)
    };

    if let Some((_, sibling)) = close_result {
        // Repoint focus at the sibling (the pane_grid close already picks a
        // sensible neighbor); the split's single tab keeps its position.
        let (dissolve, sibling_sid) = {
            let Some(split) = app.terminal.splits.get_mut(&split_id) else {
                return Task::none();
            };
            split.focus = Some(sibling);
            split.previous_focus = None;
            let dissolve = split.panes.len() == 1;
            let sid = split.panes.get(sibling).copied();
            (dissolve, sid)
        };

        if dissolve {
            // The split collapses: promote the remaining session too, and
            // drop the split + its tab.
            app.terminal.splits.remove(&split_id);
            if let Some(tid) = split_tid {
                app.remove_tab(tid);
            }
            if let Some(sid) = sibling_sid {
                let tid =
                    promote_session_to_tab(app, sid, split_pos.unwrap_or(usize::MAX), was_locked);
                app.activate_tab(tid);
            }
        }

        promote_session_to_tab(
            app,
            extracted_sid,
            split_pos.map_or(usize::MAX, |p| p + 1),
            was_locked,
        );

        focus_session(app, extracted_sid)
    } else {
        app.terminal.splits.remove(&split_id);
        if let Some(tid) = split_tid {
            app.remove_tab(tid);
        }
        let tid = promote_session_to_tab(
            app,
            extracted_sid,
            split_pos.unwrap_or(usize::MAX),
            was_locked,
        );
        app.activate_tab(tid);
        focus_session(app, extracted_sid)
    }
}

/// Find and close the pane containing the given `session_id`.
pub(super) fn close_split_pane_by_session(
    app: &mut App,
    session_id: SessionId,
) -> Option<Task<Message>> {
    let split_id = app.split_for_session(session_id)?;
    let pane = app
        .terminal
        .splits
        .get(&split_id)?
        .panes
        .iter()
        .find(|(_, sid)| **sid == session_id)
        .map(|(pane, _)| *pane)?;
    Some(close_split_pane(app, split_id, pane))
}
