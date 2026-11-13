//! Geometry model for split-view panes.
//!
//! Split layout is governed by a few size rules collected here so the
//! relationship between them is explicit:
//!
//! | Concern | Rule | Where it applies |
//! |---|---|---|
//! | Usable pane minimum | [`PANE_MIN`] (`240 × 160`) | the product decision |
//! | Split gate | [`allows_split`] | drag preview + commit |
//! | Subtree requirement | [`min_extent`] | moved/merged split groups |
//! | Render floor | [`grid_min_size`] | `pane_grid::min_size` |
//! | Resize clamp | [`clamp_split_ratio`] | divider drag |
//!
//! [`PANE_MIN`] is the single source of truth for how small a pane may become.
//! The split gate is authoritative for usability: a split is offered only when
//! the single target pane and the dragged side (which may be a whole split
//! group, judged by [`min_extent`]) both stay usable. Nothing else should
//! hard-code a pane dimension.
//!
//! There is no cap on the number of panes: because every split halves the
//! divided axis, the minimum-size gate already bounds how many panes fit, so a
//! separate count limit would be redundant.
//!
//! `pane_grid` takes one scalar `min_size` for whichever axis a split divides,
//! so [`grid_min_size`] derives it from [`PANE_MIN`]. That scalar is only a
//! *render* floor; the per-axis enforcement lives in [`allows_split`] (creating
//! a split) and [`clamp_split_ratio`] (dragging its divider).

use iced::Size;
use iced::widget::pane_grid;

use crate::terminal::Split;

/// Padding between the split container edge and the pane grid.
pub const CONTAINER_PADDING: f32 = 10.0;
/// Gap between adjacent panes, also the width of a split's resize line.
pub const PANE_SPACING: f32 = 6.0;
pub const PANE_RADIUS: f32 = 6.0;

/// Minimum usable pane size, per axis, in logical pixels.
///
/// Terminals are wider than they are tall, so the two axes have different
/// floors. [`allows_split`] checks the floor for the axis a split divides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneMin {
    pub width: f32,
    pub height: f32,
}

impl PaneMin {
    /// The floor for the axis that `axis` divides.
    ///
    /// `Axis::Vertical` splits left/right and consumes width;
    /// `Axis::Horizontal` splits top/bottom and consumes height.
    pub const fn along(self, axis: pane_grid::Axis) -> f32 {
        match axis {
            pane_grid::Axis::Vertical => self.width,
            pane_grid::Axis::Horizontal => self.height,
        }
    }

    /// Scalar floor for `pane_grid::min_size`.
    ///
    /// That API is a single value applied to both axes, so we pick the larger
    /// of the two minimums: it never permits a pane below *either* axis's
    /// usable minimum. It may over-constrain the smaller axis when there is
    /// room to spare, but [`grid_min_size`] relaxes it to fit the window, and
    /// [`allows_split`] remains the rule that actually gates splits.
    pub const fn grid_floor(self) -> f32 {
        if self.width >= self.height {
            self.width
        } else {
            self.height
        }
    }
}

/// The smallest a pane may be before it stops being usable.
pub const PANE_MIN: PaneMin = PaneMin {
    width: 240.0,
    height: 160.0,
};

/// The axis extent of `size` that a split on `axis` divides.
fn divided_extent(size: Size, axis: pane_grid::Axis) -> f32 {
    match axis {
        pane_grid::Axis::Vertical => size.width,
        pane_grid::Axis::Horizontal => size.height,
    }
}

/// The axis extent of `size` that a split on `axis` leaves untouched.
fn perpendicular_extent(size: Size, axis: pane_grid::Axis) -> f32 {
    match axis {
        pane_grid::Axis::Vertical => size.height,
        pane_grid::Axis::Horizontal => size.width,
    }
}

/// The axis a split on `axis` does not divide.
fn other_axis(axis: pane_grid::Axis) -> pane_grid::Axis {
    match axis {
        pane_grid::Axis::Vertical => pane_grid::Axis::Horizontal,
        pane_grid::Axis::Horizontal => pane_grid::Axis::Vertical,
    }
}

/// Minimum extents of a single pane: the [`PANE_MIN`] pair as a [`Size`].
pub fn pane_min_size() -> Size {
    Size::new(PANE_MIN.width, PANE_MIN.height)
}

/// Size of each child pane if `target` is split on `axis`.
///
/// A split hands the two children the target extent minus one inter-pane gap,
/// shared evenly.
pub fn child_extent(target: Size, axis: pane_grid::Axis) -> f32 {
    (divided_extent(target, axis) - PANE_SPACING) / 2.0
}

/// Minimum extents a pane subtree needs so that every leaf stays usable.
///
/// The divided axis stacks the two children, so its requirement is the sum of
/// the children's requirements plus one inter-pane gap. The perpendicular axis
/// is shared, so the larger child's requirement wins.
pub fn min_extent(node: &pane_grid::Node) -> Size {
    match node {
        pane_grid::Node::Pane(_) => pane_min_size(),
        pane_grid::Node::Split { axis, a, b, .. } => {
            let a = min_extent(a);
            let b = min_extent(b);
            let along = divided_extent(a, *axis) + PANE_SPACING + divided_extent(b, *axis);
            let perp = perpendicular_extent(a, *axis).max(perpendicular_extent(b, *axis));

            match axis {
                pane_grid::Axis::Vertical => Size::new(along, perp),
                pane_grid::Axis::Horizontal => Size::new(perp, along),
            }
        }
    }
}

/// Whether splitting `target` on `axis`, placing a subtree that needs `dragged`
/// on the other side, leaves every resulting leaf pane usable.
///
/// The target side is always a single pane, so it only needs [`PANE_MIN`] along
/// the divided axis. The dragged side may be a whole split group, so it must
/// fit its own [`min_extent`] in *both* axes: along the divided axis it gets one
/// half of the target, and perpendicular to it, the full target extent.
///
/// This is the single usability rule. Both the drag preview and the commit path
/// call it so a target that is shown can always be committed.
pub fn allows_split(target: Size, axis: pane_grid::Axis, dragged: Size) -> bool {
    let along_ok =
        child_extent(target, axis) >= divided_extent(dragged, axis).max(PANE_MIN.along(axis));
    let perp_ok = perpendicular_extent(target, axis)
        >= perpendicular_extent(dragged, axis).max(PANE_MIN.along(other_axis(axis)));

    along_ok && perp_ok
}

/// Number of splits along each `pane_grid::Axis` in a layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AxisSplitCount {
    /// Splits on `Axis::Horizontal` (top/bottom); these consume height.
    horizontal: usize,
    /// Splits on `Axis::Vertical` (left/right); these consume width.
    vertical: usize,
}

impl AxisSplitCount {
    fn of(node: &pane_grid::Node) -> Self {
        match node {
            pane_grid::Node::Pane(_) => Self::default(),
            pane_grid::Node::Split { axis, a, b, .. } => {
                let mut count = Self::of(a).combined(&Self::of(b));
                match axis {
                    pane_grid::Axis::Horizontal => count.horizontal += 1,
                    pane_grid::Axis::Vertical => count.vertical += 1,
                }
                count
            }
        }
    }

    /// Deepest parallel branch per axis, since siblings do not stack.
    fn combined(self, other: &Self) -> Self {
        Self {
            horizontal: self.horizontal.max(other.horizontal),
            vertical: self.vertical.max(other.vertical),
        }
    }
}

/// Largest extent a single pane can get from `available` after `splits` splits
/// on the same axis, or `f32::INFINITY` when the axis is unconstrained.
fn fitted_extent(available: f32, splits: usize) -> f32 {
    if splits == 0 || !available.is_finite() {
        return f32::INFINITY;
    }

    let slots = (splits + 1) as f32;
    ((available - PANE_SPACING * splits as f32) / slots).max(1.0)
}

/// Scalar `pane_grid::min_size` fitted to the space a split group has.
///
/// The nominal floor is [`PaneMin::grid_floor`]. A deeply nested layout cannot
/// honour it without overflowing its container, so the floor is lowered to the
/// largest value that still fits both the width and the height the group was
/// given. It never drops below `1.0`, so `pane_grid` always has a usable bound.
pub fn grid_min_size(split: &Split, available: Size) -> f32 {
    let counts = AxisSplitCount::of(split.panes.layout());
    let fitted = fitted_extent(available.height, counts.horizontal)
        .min(fitted_extent(available.width, counts.vertical));

    PANE_MIN.grid_floor().min(fitted).max(1.0)
}

/// The two subtrees a split divides, with the axis it divides them on.
fn split_sides(
    node: &pane_grid::Node,
    split: pane_grid::Split,
) -> Option<(pane_grid::Axis, &pane_grid::Node, &pane_grid::Node)> {
    match node {
        pane_grid::Node::Pane(_) => None,
        pane_grid::Node::Split { id, axis, a, b, .. } => {
            if *id == split {
                Some((*axis, a, b))
            } else {
                split_sides(a, split).or_else(|| split_sides(b, split))
            }
        }
    }
}

/// Clamp a divider ratio so neither side of `split` shrinks below the minimum
/// extent of its subtree, relaxing proportionally when the two cannot both fit.
///
/// `pane_grid::min_size` is a single scalar shared by both axes, and
/// [`grid_min_size`] lowers it to fit tight layouts, so the widget's own
/// render-time bound cannot enforce the per-axis [`PANE_MIN`]. The resize path
/// calls this instead, so dragging a split line stops at the same rule that
/// [`allows_split`] uses when the split is created.
///
/// `ratio` is interpreted the way `pane_grid` does: the leading extent is
/// `extent * ratio - PANE_SPACING / 2`. The returned ratio is in `[0, 1]` and
/// is safe to hand back to `pane_grid::State::resize`.
pub fn clamp_split_ratio(
    layout: &pane_grid::Node,
    min_size: f32,
    available: Size,
    split: pane_grid::Split,
    ratio: f32,
) -> f32 {
    let ratio = ratio.clamp(0.0, 1.0);
    let Some((axis, a, b)) = split_sides(layout, split) else {
        return ratio;
    };

    // The ratio is relative to the split's own region, computed by `pane_grid`
    // with the same spacing and scalar floor we render with, so recomputing it
    // here gives the exact rectangle the ratio was measured against.
    let regions = layout.split_regions(PANE_SPACING, min_size, available);
    let Some((_, region, _)) = regions.get(&split) else {
        return ratio;
    };

    let extent = divided_extent(region.size(), axis);
    let usable = extent - PANE_SPACING;
    if extent <= 0.0 || usable <= 0.0 {
        return ratio;
    }

    let a_min = divided_extent(min_extent(a), axis);
    let b_min = divided_extent(min_extent(b), axis);

    // If the region genuinely cannot hold both subtrees, scale the floors down
    // in proportion to what each side needs. This is the resize equivalent of
    // `grid_min_size` relaxing the render floor in a too-small window: panes
    // stay divisible instead of one side collapsing to nothing.
    let (a_min, b_min) = if a_min + b_min > usable {
        let total = a_min + b_min;
        if total <= 0.0 {
            return ratio;
        }
        (usable * a_min / total, usable * b_min / total)
    } else {
        (a_min, b_min)
    };

    let min_ratio = (a_min + PANE_SPACING / 2.0) / extent;
    let max_ratio = (usable - b_min + PANE_SPACING / 2.0) / extent;

    ratio.clamp(min_ratio.min(max_ratio), max_ratio.max(min_ratio))
}

/// Clamp a divider drag for `split`, using the same scalar floor the widget
/// renders with so the ratio maps onto the region `pane_grid` measured it in.
pub fn clamp_resize(split: &Split, available: Size, split_id: pane_grid::Split, ratio: f32) -> f32 {
    clamp_split_ratio(
        split.panes.layout(),
        grid_min_size(split, available),
        available,
        split_id,
        ratio,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_splits(splits: &[pane_grid::Axis]) -> pane_grid::State<()> {
        let (mut state, first) = pane_grid::State::new(());
        let mut pane = first;
        for (i, axis) in splits.iter().enumerate() {
            let (new_pane, _) = state.split(*axis, pane, ()).expect("split");
            if i + 1 < splits.len() {
                pane = new_pane;
            }
        }
        state
    }

    #[test]
    fn child_extent_splits_minus_spacing_in_half() {
        let size = Size::new(1000.0, 500.0);
        assert_eq!(child_extent(size, pane_grid::Axis::Vertical), 497.0);
        assert_eq!(child_extent(size, pane_grid::Axis::Horizontal), 247.0);
    }

    #[test]
    fn allows_split_uses_the_divided_axis_floor() {
        // Width floor is 240: 486 -> 240 is allowed, one pixel less is not.
        assert!(allows_split(
            Size::new(486.0, 1000.0),
            pane_grid::Axis::Vertical,
            pane_min_size()
        ));
        assert!(!allows_split(
            Size::new(485.0, 1000.0),
            pane_grid::Axis::Vertical,
            pane_min_size()
        ));

        // Height floor is 160: 326 -> 160 is allowed, one pixel less is not.
        assert!(allows_split(
            Size::new(1000.0, 326.0),
            pane_grid::Axis::Horizontal,
            pane_min_size()
        ));
        assert!(!allows_split(
            Size::new(1000.0, 325.0),
            pane_grid::Axis::Horizontal,
            pane_min_size()
        ));
    }

    #[test]
    fn min_extent_of_a_single_pane_is_the_pane_minimum() {
        let single = state_with_splits(&[]);
        assert_eq!(min_extent(single.layout()), pane_min_size());
    }

    #[test]
    fn min_extent_stacks_along_the_split_and_shares_the_other_axis() {
        // Two panes side by side: widths add, the shared height is one pane's.
        let side_by_side = state_with_splits(&[pane_grid::Axis::Vertical]);
        assert_eq!(
            min_extent(side_by_side.layout()),
            Size::new(240.0 * 2.0 + PANE_SPACING, 160.0)
        );

        // Two panes stacked: heights add, the shared width is one pane's.
        let stacked = state_with_splits(&[pane_grid::Axis::Horizontal]);
        assert_eq!(
            min_extent(stacked.layout()),
            Size::new(240.0, 160.0 * 2.0 + PANE_SPACING)
        );
    }

    #[test]
    fn allows_split_rejects_a_group_that_cannot_fit_itself() {
        // A two-pane side-by-side group needs 486px of width. Splitting a pane
        // in half only works once the target offers the group (and the single
        // target pane) room: 486 * 2 + spacing = 978.
        let group = Size::new(240.0 * 2.0 + PANE_SPACING, 160.0);
        assert!(allows_split(
            Size::new(978.0, 160.0),
            pane_grid::Axis::Vertical,
            group
        ));
        assert!(!allows_split(
            Size::new(977.0, 160.0),
            pane_grid::Axis::Vertical,
            group
        ));
    }

    #[test]
    fn allows_split_checks_the_shared_axis_for_the_dragged_group() {
        // A stacked two-pane group needs 326px of height. Splitting left/right
        // shares the target's height, so a target shorter than that is refused
        // even when its width would comfortably halve.
        let group = Size::new(240.0, 160.0 * 2.0 + PANE_SPACING);
        assert!(allows_split(
            Size::new(2000.0, 326.0),
            pane_grid::Axis::Vertical,
            group
        ));
        assert!(!allows_split(
            Size::new(2000.0, 325.0),
            pane_grid::Axis::Vertical,
            group
        ));
    }

    #[test]
    fn grid_floor_never_falls_below_either_axis_minimum() {
        assert_eq!(PANE_MIN.grid_floor(), PANE_MIN.width.max(PANE_MIN.height));
        assert!(PANE_MIN.grid_floor() >= PANE_MIN.width);
        assert!(PANE_MIN.grid_floor() >= PANE_MIN.height);
    }

    #[test]
    fn split_counts_track_each_axis() {
        let stacked =
            state_with_splits(&[pane_grid::Axis::Horizontal, pane_grid::Axis::Horizontal]);
        assert_eq!(
            AxisSplitCount::of(stacked.layout()),
            AxisSplitCount {
                horizontal: 2,
                vertical: 0,
            }
        );

        let mixed = state_with_splits(&[pane_grid::Axis::Horizontal, pane_grid::Axis::Vertical]);
        assert_eq!(
            AxisSplitCount::of(mixed.layout()),
            AxisSplitCount {
                horizontal: 1,
                vertical: 1,
            }
        );
    }

    #[test]
    fn fitted_extent_shares_axis_between_slots() {
        // (326 - 6) / 2 = 160 for one split, (326 - 12) / 3 for two.
        assert_eq!(fitted_extent(326.0, 1), 160.0);
        assert_eq!(fitted_extent(326.0, 2), 104.666_664);
        assert_eq!(fitted_extent(326.0, 0), f32::INFINITY);
        assert_eq!(fitted_extent(f32::INFINITY, 3), f32::INFINITY);
        // Never below one pixel, even in a degenerate window.
        assert_eq!(fitted_extent(0.0, 4), 1.0);
    }

    #[test]
    fn clamp_split_ratio_stops_at_each_side_minimum() {
        let (mut state, first) = pane_grid::State::new(());
        let (_, split) = state.split(pane_grid::Axis::Vertical, first, ()).unwrap();
        let available = Size::new(1000.0, 400.0);
        let min_size = PANE_MIN.grid_floor();

        let left_ratio = clamp_split_ratio(state.layout(), min_size, available, split, 0.0);
        let left = 1000.0 * left_ratio - PANE_SPACING / 2.0;
        assert!((left - PANE_MIN.width).abs() < 1.0, "left={left}");

        let right_ratio = clamp_split_ratio(state.layout(), min_size, available, split, 1.0);
        let left = 1000.0 * right_ratio - PANE_SPACING / 2.0;
        let right = 1000.0 - left - PANE_SPACING;
        assert!((right - PANE_MIN.width).abs() < 1.0, "right={right}");
    }

    #[test]
    fn clamp_split_ratio_keeps_a_group_above_its_own_minimum() {
        // [top | bottom split again into two rows]: the bottom side is a
        // subtree, so the outer divider must leave it room for two usable
        // panes, not just one.
        let (mut state, first) = pane_grid::State::new(());
        let (bottom, outer) = state.split(pane_grid::Axis::Horizontal, first, ()).unwrap();
        let _ = state
            .split(pane_grid::Axis::Horizontal, bottom, ())
            .unwrap();

        let available = Size::new(1000.0, 1000.0);
        let ratio = clamp_split_ratio(state.layout(), PANE_MIN.height, available, outer, 1.0);
        let top = 1000.0 * ratio - PANE_SPACING / 2.0;
        let bottom_extent = 1000.0 - top - PANE_SPACING;
        let group_min = PANE_MIN.height * 2.0 + PANE_SPACING;
        assert!(
            (bottom_extent - group_min).abs() < 1.5,
            "bottom={bottom_extent}"
        );
    }

    #[test]
    fn clamp_split_ratio_relaxes_proportionally_when_both_floors_do_not_fit() {
        let (mut state, first) = pane_grid::State::new(());
        let (_, split) = state.split(pane_grid::Axis::Vertical, first, ()).unwrap();

        // 240 + 240 + 6 > 400, so both floors scale down to 197 each and the
        // divider lands in the middle instead of collapsing a side.
        let available = Size::new(400.0, 400.0);
        let ratio = clamp_split_ratio(state.layout(), PANE_MIN.width, available, split, 0.0);
        let left = 400.0 * ratio - PANE_SPACING / 2.0;
        assert!((left - 197.0).abs() < 1.0, "left={left}");
    }

    #[test]
    fn clamp_resize_enforces_width_floor_when_the_scalar_floor_is_relaxed() {
        use crate::app::{SessionId, SplitId};

        // A short window with a top/bottom split relaxes the scalar render
        // floor below the width minimum. Before the clamp, that let a left/right
        // divider shrink columns below `PANE_MIN.width`.
        let (mut state, first) = pane_grid::State::new(SessionId(1));
        let (bottom, _) = state
            .split(pane_grid::Axis::Horizontal, first, SessionId(2))
            .unwrap();
        let (_, divider) = state
            .split(pane_grid::Axis::Vertical, bottom, SessionId(3))
            .unwrap();
        let split = Split {
            id: SplitId(1),
            panes: state,
            focus: None,
            previous_focus: None,
        };

        let available = Size::new(1000.0, 400.0);
        assert!(grid_min_size(&split, available) < PANE_MIN.width);

        let ratio = clamp_resize(&split, available, divider, 0.0);
        let left = 1000.0 * ratio - PANE_SPACING / 2.0;
        assert!((left - PANE_MIN.width).abs() < 1.0, "left={left}");
    }
}
