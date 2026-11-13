//! Split divider grip rendering.

// Rust guideline compliant 2026-10-09

use iced::mouse::Cursor;
use iced::widget::{self, canvas};
use iced::{Color, Point, Rectangle, Size, Theme};

use crate::core::message::Message;
use crate::terminal::split_layout::PANE_SPACING;

/// Three-dot grip matching VS Code's compact pane-divider affordance.
const DIVIDER_DOT_COUNT: usize = 3;
/// Compact dot diameter that remains visible in the narrow pane gap.
const DIVIDER_DOT_SIZE: f32 = 2.0;
/// Space between dots, keeping the grip legible without looking heavy.
const DIVIDER_DOT_GAP: f32 = 2.0;

pub(super) struct PaneDividerDots {
    pub(super) node: widget::pane_grid::Node,
    pub(super) min_size: f32,
    pub(super) color: Color,
}

impl canvas::Program<Message> for PaneDividerDots {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let radius = DIVIDER_DOT_SIZE / 2.0;

        for bounds in divider_dot_bounds(&self.node, self.min_size, bounds.size()) {
            frame.fill(&canvas::Path::circle(bounds.center(), radius), self.color);
        }

        vec![frame.into_geometry()]
    }
}

/// Dot rectangles positioned on each split's current divider line.
fn divider_dot_bounds(
    node: &widget::pane_grid::Node,
    min_size: f32,
    size: Size,
) -> impl Iterator<Item = Rectangle> {
    node.split_regions(PANE_SPACING, min_size, size)
        .into_values()
        .flat_map(|(axis, region, ratio)| {
            let line = axis.split_line_bounds(region, ratio, PANE_SPACING);
            split_dots(axis, line)
        })
}

/// Three dots centered on one split, parallel to the divider.
fn split_dots(axis: widget::pane_grid::Axis, region: Rectangle) -> [Rectangle; DIVIDER_DOT_COUNT] {
    let center = region.center();
    let step = DIVIDER_DOT_SIZE + DIVIDER_DOT_GAP;
    let span = step * (DIVIDER_DOT_COUNT - 1) as f32;

    std::array::from_fn(|index| {
        let offset = index as f32 * step - span / 2.0;
        let (x, y) = match axis {
            widget::pane_grid::Axis::Vertical => (center.x, center.y + offset),
            widget::pane_grid::Axis::Horizontal => (center.x + offset, center.y),
        };

        Rectangle::new(
            Point::new(x - DIVIDER_DOT_SIZE / 2.0, y - DIVIDER_DOT_SIZE / 2.0),
            Size::new(DIVIDER_DOT_SIZE, DIVIDER_DOT_SIZE),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_divider_dots_follow_the_resized_divider() {
        let bounds = Rectangle::new(Point::new(20.0, 30.0), Size::new(640.0, 480.0));

        for axis in [
            widget::pane_grid::Axis::Vertical,
            widget::pane_grid::Axis::Horizontal,
        ] {
            let (mut state, pane) = widget::pane_grid::State::new(());
            let (_, split) = state
                .split(axis, pane, ())
                .expect("a fresh pane can be split");
            state.resize(split, 0.25);

            let (_, region, ratio) = state
                .layout()
                .split_regions(PANE_SPACING, 1.0, bounds.size())
                .into_values()
                .next()
                .expect("the split has a region");
            let divider = axis.split_line_bounds(region, ratio, PANE_SPACING);
            let divider_center = divider.center();
            let dots: Vec<_> = divider_dot_bounds(state.layout(), 1.0, bounds.size()).collect();

            assert_eq!(dots.len(), DIVIDER_DOT_COUNT);
            match axis {
                widget::pane_grid::Axis::Vertical => {
                    assert!(
                        dots.iter()
                            .all(|dot| (dot.center().x - divider_center.x).abs() < f32::EPSILON)
                    );
                }
                widget::pane_grid::Axis::Horizontal => {
                    assert!(
                        dots.iter()
                            .all(|dot| (dot.center().y - divider_center.y).abs() < f32::EPSILON)
                    );
                }
            }
            assert!((dots[1].center().x - divider_center.x).abs() < f32::EPSILON);
            assert!((dots[1].center().y - divider_center.y).abs() < f32::EPSILON);
        }
    }
}
