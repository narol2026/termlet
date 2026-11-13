//! Split-pane geometry, drop targeting, and pane-tree transformations.

// Rust guideline compliant 2026-10-09

mod drop;
mod geometry;
mod tree;

pub(super) use drop::{dragged_min_extent, drop_target, too_small, too_small_task};
pub(super) use geometry::{SplitDirection, axis, drop_grid, preview_geometry};
pub(super) use tree::{inject, pane_for_session, split_configuration};
