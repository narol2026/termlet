//! Split drag-grid and preview geometry.

// Rust guideline compliant 2026-10-09

use iced::widget::pane_grid;
use iced::{Point, Rectangle, Size};

/// Direction in which a dragged session enters a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::terminal) enum SplitDirection {
    Left,
    Right,
    Top,
    Bottom,
}

impl SplitDirection {
    pub(in crate::terminal) const fn label(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

/// Visual and semantic result of pointing at the 3×2 drop grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::terminal) struct DropGrid {
    pub(in crate::terminal) index: usize,
    pub(in crate::terminal) direction: SplitDirection,
}

/// Pixel geometry for the split drag-preview overlay.
#[derive(Debug, Clone, Copy)]
pub(in crate::terminal) struct PreviewGeometry {
    pub(in crate::terminal) highlight: Option<Rectangle>,
    pub(in crate::terminal) divider: Option<Rectangle>,
    pub(in crate::terminal) outline: [Rectangle; 4],
}

pub(in crate::terminal) fn drop_grid(bounds: Rectangle, point: Point) -> DropGrid {
    let cell_w = bounds.width / 3.0;
    let cell_h = bounds.height / 2.0;
    let col = ((point.x - bounds.x) / cell_w).clamp(0.0, 2.0) as usize;
    let row = ((point.y - bounds.y) / cell_h).clamp(0.0, 1.0) as usize;
    let index = row * 3 + col + 1;
    let direction = match index {
        1 | 4 => SplitDirection::Left,
        2 => SplitDirection::Top,
        3 | 6 => SplitDirection::Right,
        5 => SplitDirection::Bottom,
        _ => SplitDirection::Right,
    };
    DropGrid { index, direction }
}

pub(in crate::terminal) fn preview_geometry(
    bounds: Rectangle,
    grid: usize,
) -> Option<PreviewGeometry> {
    let width = bounds.width;
    let height = bounds.height;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let highlight = match grid {
        1 | 4 => Some(Rectangle::new(
            bounds.position(),
            Size::new(width / 2.0, height),
        )),
        2 => Some(Rectangle::new(
            bounds.position(),
            Size::new(width, height / 2.0),
        )),
        3 | 6 => Some(Rectangle::new(
            Point::new(bounds.x + width / 2.0, bounds.y),
            Size::new(width / 2.0, height),
        )),
        5 => Some(Rectangle::new(
            Point::new(bounds.x, bounds.y + height / 2.0),
            Size::new(width, height / 2.0),
        )),
        _ => None,
    };

    let divider = match grid {
        1 | 3 | 4 | 6 => Some(Rectangle::new(
            Point::new(bounds.x + width / 2.0 - 0.5, bounds.y),
            Size::new(1.0, height),
        )),
        2 | 5 => Some(Rectangle::new(
            Point::new(bounds.x, bounds.y + height / 2.0 - 0.5),
            Size::new(width, 1.0),
        )),
        _ => None,
    };

    let outline = [
        Rectangle::new(bounds.position(), Size::new(width, 1.0)),
        Rectangle::new(
            Point::new(bounds.x, bounds.y + height - 1.0),
            Size::new(width, 1.0),
        ),
        Rectangle::new(bounds.position(), Size::new(1.0, height)),
        Rectangle::new(
            Point::new(bounds.x + width - 1.0, bounds.y),
            Size::new(1.0, height),
        ),
    ];

    Some(PreviewGeometry {
        highlight,
        divider,
        outline,
    })
}

pub(in crate::terminal) fn axis(direction: SplitDirection) -> (pane_grid::Axis, bool) {
    match direction {
        SplitDirection::Left => (pane_grid::Axis::Vertical, true),
        SplitDirection::Right => (pane_grid::Axis::Vertical, false),
        SplitDirection::Top => (pane_grid::Axis::Horizontal, true),
        SplitDirection::Bottom => (pane_grid::Axis::Horizontal, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(10.0, 20.0), Size::new(300.0, 200.0))
    }

    #[test]
    fn the_drop_grid_numbers_cells_row_by_row() {
        let bounds = bounds();
        assert_eq!(drop_grid(bounds, bounds.position()).index, 1);
        assert_eq!(drop_grid(bounds, Point::new(160.0, 120.0)).index, 5);
        assert_eq!(drop_grid(bounds, Point::new(309.0, 219.0)).index, 6);
    }

    #[test]
    fn grid_cells_map_to_their_split_side() {
        let bounds = bounds();
        assert_eq!(
            drop_grid(bounds, Point::new(20.0, 30.0)).direction,
            SplitDirection::Left
        );
        assert_eq!(
            drop_grid(bounds, Point::new(160.0, 30.0)).direction,
            SplitDirection::Top
        );
        assert_eq!(
            drop_grid(bounds, Point::new(300.0, 120.0)).direction,
            SplitDirection::Right
        );
        assert_eq!(
            drop_grid(bounds, Point::new(160.0, 210.0)).direction,
            SplitDirection::Bottom
        );
    }

    #[test]
    fn points_outside_the_grid_clamp_to_the_nearest_cell() {
        let bounds = bounds();
        assert_eq!(drop_grid(bounds, Point::new(-500.0, -500.0)).index, 1);
        assert_eq!(drop_grid(bounds, Point::new(9999.0, 9999.0)).index, 6);
    }

    #[test]
    fn preview_geometry_matches_the_drop_grid() {
        let bounds = bounds();
        let left = preview_geometry(bounds, 1).expect("single-pane grid has geometry");
        let left_highlight = left.highlight.expect("left half is highlighted");
        let divider = left.divider.expect("vertical divider is drawn");

        assert_eq!(left_highlight.width, bounds.width / 2.0);
        assert_eq!(left_highlight.height, bounds.height);
        assert_eq!(divider.x, bounds.x + bounds.width / 2.0 - 0.5);

        let no_highlight = preview_geometry(bounds, 0).expect("outline-only preview");
        assert!(no_highlight.highlight.is_none());
        assert!(no_highlight.divider.is_none());
    }

    #[test]
    fn splitting_along_a_direction_divides_the_matching_axis() {
        let (split_axis, before) = axis(SplitDirection::Left);
        assert!(matches!(split_axis, pane_grid::Axis::Vertical));
        assert!(before, "the new pane takes the left half");

        let (split_axis, before) = axis(SplitDirection::Right);
        assert!(matches!(split_axis, pane_grid::Axis::Vertical));
        assert!(!before, "the new pane takes the right half");

        let (split_axis, before) = axis(SplitDirection::Top);
        assert!(matches!(split_axis, pane_grid::Axis::Horizontal));
        assert!(before);

        let (split_axis, before) = axis(SplitDirection::Bottom);
        assert!(matches!(split_axis, pane_grid::Axis::Horizontal));
        assert!(!before);
    }
}
