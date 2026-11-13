//! Terminal tab drag, reorder, and split preview updates.

use iced::Task;
use iced::widget::Id;

use super::{
    App, DomainMessage, create_split, reorder_terminal_tab, split_existing_pane,
    tab_representative_session, tab_zone_ids,
};
use crate::app::SessionId;
use crate::core::ids::TabId;
use crate::core::message::Message;
use crate::terminal::DragPreview;
use crate::terminal::split::{
    dragged_min_extent, drop_grid, drop_target as split_drop_target, too_small as split_too_small,
    too_small_task as split_too_small_for_split,
};
use crate::terminal::split_layout::pane_min_size;
use crate::terminal::view::TERMINAL_DROP_ZONE_ID;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::TerminalTabDragCancelled => {
            app.terminal.drag_preview = None;
            app.terminal.dragging_terminal_tab = None;
            app.terminal.drag_reorder_target = None;
            app.terminal.drag_insert_before = false;
            Task::none()
        }
        DomainMessage::TerminalTabHovered {
            session_id,
            entered,
        } => {
            if entered {
                app.terminal.hovered_terminal_id = Some(session_id);
            } else if app.terminal.hovered_terminal_id == Some(session_id) {
                app.terminal.hovered_terminal_id = None;
            }
            Task::none()
        }
        DomainMessage::TerminalTabDropped { session_id, point } => {
            app.terminal.drag_preview = None;
            app.terminal.dragging_terminal_tab = None;
            app.terminal.drag_reorder_target = None;
            app.terminal.drag_insert_before = false;

            if !app.terminal.sessions.iter().any(|s| s.id == session_id) {
                return Task::none();
            }

            let mut zone_ids: Vec<Id> = Vec::new();
            if app.active_terminal_session_id().is_some() {
                zone_ids.push(Id::from(TERMINAL_DROP_ZONE_ID));
            }
            zone_ids.extend(tab_zone_ids(app));

            if zone_ids.is_empty() {
                return Task::none();
            }

            iced_drop::zones_on_point(
                move |zones| {
                    Message::Terminal(crate::terminal::message::Message::TerminalDropZonesFound {
                        session_id,
                        zones,
                        point,
                    })
                },
                point,
                Some(zone_ids),
                None,
            )
        }
        DomainMessage::TerminalTabDragging { session_id, point } => {
            app.terminal.dragging_terminal_tab = Some(session_id);

            let mut zone_ids: Vec<Id> = Vec::new();
            if app.active_terminal_session_id().is_some() {
                zone_ids.push(Id::from(TERMINAL_DROP_ZONE_ID));
            }
            zone_ids.extend(tab_zone_ids(app));

            if zone_ids.is_empty() {
                return Task::none();
            }

            iced_drop::zones_on_point(
                move |zones| {
                    Message::Terminal(crate::terminal::message::Message::TerminalDragPreview {
                        zones,
                        point,
                    })
                },
                point,
                Some(zone_ids),
                None,
            )
        }
        DomainMessage::TerminalDragPreview { zones, point } => {
            // Reject stale callbacks that arrive after the drag has already ended.
            // Without this guard, an async zones_on_point callback from a previous
            // TerminalTabDragging event can re-set drag_preview after the drop has
            // been processed, leaving a permanent phantom placeholder overlay.
            if app.terminal.dragging_terminal_tab.is_none() {
                return Task::none();
            }

            // Tab zones reorder; the split drop zone creates a split.
            if let Some(dragged_sid) = app.terminal.dragging_terminal_tab {
                let dragged_pos = app
                    .tab_id_for_terminal_session(dragged_sid)
                    .and_then(|tid| app.terminal.tabs.iter().position(|&t| t == tid));
                let tab_sids: Vec<(TabId, SessionId)> = app
                    .terminal
                    .tabs
                    .iter()
                    .filter_map(|&tid| tab_representative_session(app, tid).map(|sid| (tid, sid)))
                    .collect();
                let mut hit_target: Option<SessionId> = None;
                let mut hit_insert_before = false;
                for (tid, target_sid) in tab_sids {
                    if target_sid == dragged_sid {
                        continue;
                    }
                    let tab_zone_id = Id::from(format!("tab_zone_{target_sid}"));
                    if let Some((_, bounds)) = zones.iter().find(|(zone, _)| *zone == tab_zone_id) {
                        let on_left_half = point.x < bounds.x + bounds.width / 2.0;
                        let target_pos = app.terminal.tabs.iter().position(|&t| t == tid);
                        let should_highlight = match (dragged_pos, target_pos) {
                            (Some(dp), Some(tp)) if dp < tp => !on_left_half,
                            (Some(dp), Some(tp)) if dp > tp => on_left_half,
                            _ => false,
                        };
                        if should_highlight {
                            hit_target = Some(target_sid);
                            hit_insert_before = on_left_half;
                            break;
                        }
                    }
                }
                // Reorder live so the tab follows the cursor.
                app.terminal.drag_reorder_target = None;
                app.terminal.drag_insert_before = false;
                if let Some(target_sid) = hit_target {
                    app.terminal.drag_preview = None;
                    return reorder_terminal_tab(app, dragged_sid, target_sid, hit_insert_before);
                }
            }

            let target = Id::from(TERMINAL_DROP_ZONE_ID);
            if let Some((_, bounds)) = zones.iter().find(|(zone, _)| *zone == target) {
                if let Some(target_id) = app.active_terminal_session_id() {
                    if app.terminal.dragging_terminal_tab == Some(target_id) {
                        app.terminal.drag_preview = None;
                    } else if let Some(drop_target) =
                        split_drop_target(app, target_id, *bounds, point)
                    {
                        let dragged = app
                            .terminal
                            .dragging_terminal_tab
                            .map_or_else(pane_min_size, |dragged| dragged_min_extent(app, dragged));
                        let grid = drop_grid(drop_target.bounds, point);
                        if split_too_small(drop_target.bounds, grid.direction, dragged) {
                            app.terminal.drag_preview = None;
                        } else {
                            app.terminal.drag_preview = Some(DragPreview {
                                grid: grid.index,
                                bounds: drop_target.bounds,
                            });
                        }
                    } else {
                        app.terminal.drag_preview = None;
                    }
                } else {
                    app.terminal.drag_preview = None;
                }
            } else {
                app.terminal.drag_preview = None;
            }
            Task::none()
        }
        DomainMessage::TerminalDropZonesFound {
            session_id,
            zones,
            point,
        } => {
            app.terminal.drag_preview = None;
            app.terminal.drag_reorder_target = None;
            app.terminal.drag_insert_before = false;

            // First try to reorder against a tab zone.
            {
                let dragged_pos = app
                    .tab_id_for_terminal_session(session_id)
                    .and_then(|tid| app.terminal.tabs.iter().position(|&t| t == tid));
                let tab_sids: Vec<(TabId, SessionId)> = app
                    .terminal
                    .tabs
                    .iter()
                    .filter_map(|&tid| tab_representative_session(app, tid).map(|sid| (tid, sid)))
                    .collect();
                for (tid, target_sid) in tab_sids {
                    if target_sid == session_id {
                        continue;
                    }
                    let tab_zone_id = Id::from(format!("tab_zone_{target_sid}"));
                    if let Some((_, bounds)) = zones.iter().find(|(zone, _)| *zone == tab_zone_id) {
                        let on_left_half = point.x < bounds.x + bounds.width / 2.0;
                        let target_pos = app.terminal.tabs.iter().position(|&t| t == tid);
                        let should_drop = match (dragged_pos, target_pos) {
                            (Some(dp), Some(tp)) if dp < tp => !on_left_half,
                            (Some(dp), Some(tp)) if dp > tp => on_left_half,
                            _ => false,
                        };
                        if should_drop {
                            return reorder_terminal_tab(app, session_id, target_sid, on_left_half);
                        }
                    }
                }
            }

            let target = Id::from(TERMINAL_DROP_ZONE_ID);
            let Some((_, bounds)) = zones.iter().find(|(zone, _)| *zone == target) else {
                return Task::none();
            };
            if let Some(target_id) = app.active_terminal_session_id()
                && target_id != session_id
            {
                let Some(drop_target) = split_drop_target(app, target_id, *bounds, point) else {
                    return Task::none();
                };
                let grid = drop_grid(drop_target.bounds, point);
                // Refuse panes below the minimum usable size.
                let dragged = dragged_min_extent(app, session_id);
                if split_too_small(drop_target.bounds, grid.direction, dragged) {
                    return split_too_small_for_split();
                }
                return if let (Some(target_pane), Some(split_id)) =
                    (drop_target.pane, drop_target.split_id)
                {
                    split_existing_pane(app, split_id, target_pane, session_id, grid.direction)
                } else {
                    create_split(app, target_id, session_id, grid.direction)
                };
            }
            Task::none()
        }
        _ => unreachable!("terminal drag handler received another message"),
    }
}
