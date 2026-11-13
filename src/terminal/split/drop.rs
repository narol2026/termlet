//! Split drop-target resolution and minimum-size checks.

// Rust guideline compliant 2026-10-09

use iced::widget::pane_grid;
use iced::{Point, Rectangle, Size, Task};

use super::geometry::{SplitDirection, axis};
use crate::app::{App, SessionId, SplitId};
use crate::core::message::Message;
use crate::terminal::Split;
use crate::terminal::split_layout::{
    CONTAINER_PADDING, PANE_MIN, PANE_SPACING, allows_split, grid_min_size, min_extent,
    pane_min_size,
};

/// Concrete destination for a split drag.
#[derive(Debug, Clone, Copy)]
pub(in crate::terminal) struct DropTarget {
    pub(in crate::terminal) pane: Option<pane_grid::Pane>,
    pub(in crate::terminal) split_id: Option<SplitId>,
    pub(in crate::terminal) bounds: Rectangle,
}

pub(in crate::terminal) fn drop_target(
    app: &App,
    target_session: SessionId,
    bounds: Rectangle,
    point: Point,
) -> Option<DropTarget> {
    if let Some((split_id, split)) = active_split(app, target_session) {
        pane_at_point(split, bounds, point).map(|(pane, bounds)| DropTarget {
            pane: Some(pane),
            split_id: Some(split_id),
            bounds,
        })
    } else if bounds.contains(point) {
        Some(DropTarget {
            pane: None,
            split_id: None,
            bounds,
        })
    } else {
        None
    }
}

pub(in crate::terminal) fn too_small(
    bounds: Rectangle,
    direction: SplitDirection,
    dragged: Size,
) -> bool {
    !allows_split(bounds.size(), axis(direction).0, dragged)
}

pub(in crate::terminal) fn dragged_min_extent(app: &App, session_id: SessionId) -> Size {
    app.split_for_session(session_id)
        .and_then(|split_id| app.terminal.splits.get(&split_id))
        .map_or_else(pane_min_size, |split| min_extent(split.panes.layout()))
}

fn minimum_warning() -> (f32, f32) {
    (PANE_MIN.width, PANE_MIN.height)
}

pub(in crate::terminal) fn too_small_task() -> Task<Message> {
    let (width, height) = minimum_warning();
    Task::done(Message::ShowWarningToast(
        rust_i18n::t!(
            "terminal.split_too_small",
            min_width = format!("{width:.0}"),
            min_height = format!("{height:.0}")
        )
        .to_string(),
    ))
}

fn active_split(app: &App, session_id: SessionId) -> Option<(SplitId, &Split)> {
    let split_id = app.split_for_session(session_id)?;
    Some((split_id, app.terminal.splits.get(&split_id)?))
}

fn pane_at_point(
    split: &Split,
    bounds: Rectangle,
    point: Point,
) -> Option<(pane_grid::Pane, Rectangle)> {
    let padding = CONTAINER_PADDING;
    let inner_bounds = Rectangle {
        x: bounds.x + padding,
        y: bounds.y + padding,
        width: (bounds.width - padding * 2.0).max(0.0),
        height: (bounds.height - padding * 2.0).max(0.0),
    };
    if !inner_bounds.contains(point) {
        return None;
    }

    split
        .panes
        .layout()
        .pane_regions(
            PANE_SPACING,
            grid_min_size(split, inner_bounds.size()),
            inner_bounds.size(),
        )
        .into_iter()
        .map(|(pane, region)| {
            (
                pane,
                Rectangle {
                    x: inner_bounds.x + region.x,
                    y: inner_bounds.y + region.y,
                    width: region.width,
                    height: region.height,
                },
            )
        })
        .find(|(_, pane_bounds)| pane_bounds.contains(point))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::split::pane_for_session;

    #[test]
    fn a_split_that_would_squeeze_a_pane_below_the_minimum_is_refused() {
        let roomy = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1200.0,
            height: 800.0,
        };

        assert!(
            !too_small(roomy, SplitDirection::Right, pane_min_size()),
            "a minimum-sized pane dropped on a 1200×800 target leaves both halves usable"
        );

        let cramped = Rectangle {
            x: 0.0,
            y: 0.0,
            width: PANE_MIN.width - 1.0,
            height: PANE_MIN.height,
        };
        assert!(
            too_small(cramped, SplitDirection::Right, pane_min_size()),
            "a pane narrower than PANE_MIN cannot be split"
        );
    }

    #[test]
    fn a_drop_is_aimed_at_the_pane_under_the_pointer() {
        let mut app = crate::core::test_support::app();
        let first = SessionId(1);
        let second = SessionId(2);
        let (mut panes, first_pane) = pane_grid::State::new(first);
        panes
            .split(pane_grid::Axis::Vertical, first_pane, second)
            .expect("two panes can be split");
        let split_id = app.allocate_split_id();
        app.terminal.splits.insert(
            split_id,
            Split {
                id: split_id,
                focus: pane_for_session(&panes, first),
                previous_focus: None,
                panes,
            },
        );
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };

        let target = drop_target(&app, first, bounds, Point::new(200.0, 300.0))
            .expect("a drop inside the split is a target");
        assert_eq!(target.split_id, Some(split_id));
        assert!(
            target.pane.is_some(),
            "the pointer resolves to one pane of the split"
        );

        // A standalone session has no split behind it: anywhere inside its own
        // bounds is the pane itself, and anywhere outside is not a drop.
        let standalone = drop_target(&app, SessionId(9), bounds, Point::new(400.0, 300.0))
            .expect("a drop inside a standalone pane");
        assert_eq!(standalone.split_id, None);
        assert_eq!(standalone.pane, None);

        assert!(
            drop_target(&app, SessionId(9), bounds, Point::new(900.0, 700.0)).is_none(),
            "a pointer outside the pane is not a drop target"
        );
    }
}
