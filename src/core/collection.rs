//! Shared geometry for keyboard-driven collection views.

// Rust guideline compliant 2026-10-09

/// Vertical dimensions of a scrollable collection row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RowMetrics {
    pub(crate) height: f32,
    pub(crate) gap: f32,
    pub(crate) top_padding: f32,
}

/// Fixed height of the standard icon-and-two-line collection card.
pub(crate) const CARD_HEIGHT: f32 = 74.0;
/// Vertical gap between rendered collection rows.
pub(crate) const ROW_GAP: f32 = 12.0;
/// Height of a section heading shared by multi-section lists.
pub(crate) const SECTION_HEADER_HEIGHT: f32 = 26.0;
/// Gap between a section heading and its grid.
pub(crate) const SECTION_HEADER_GAP: f32 = 4.0;
/// Gap between top-level sections in a scrollable collection.
pub(crate) const SECTION_GAP: f32 = 16.0;
/// Height of the one-pixel divider used between collection sections.
pub(crate) const SECTION_DIVIDER_HEIGHT: f32 = 1.0;
/// Height of an empty collection state inside a section.
pub(crate) const EMPTY_HEIGHT: f32 = 56.0;

/// Returns the effective column count for a responsive collection.
pub(crate) fn columns(grid_view: bool, measured_columns: usize) -> usize {
    if grid_view {
        measured_columns.max(1)
    } else {
        1
    }
}

/// Returns row metrics for the standard collection card.
pub(crate) const fn metrics(top_padding: f32) -> RowMetrics {
    RowMetrics {
        height: CARD_HEIGHT,
        gap: ROW_GAP,
        top_padding,
    }
}

/// Returns the rendered height of a grid section.
pub(crate) fn grid_height(
    item_count: usize,
    columns: usize,
    top_padding: f32,
    bottom_padding: f32,
) -> f32 {
    if item_count == 0 {
        return EMPTY_HEIGHT;
    }
    let columns = columns.max(1);
    let rows = item_count.div_ceil(columns);
    top_padding
        + rows as f32 * CARD_HEIGHT
        + rows.saturating_sub(1) as f32 * ROW_GAP
        + bottom_padding
}

/// Returns the height of a titled grid section.
pub(crate) fn section_height(
    item_count: usize,
    columns: usize,
    top_padding: f32,
    bottom_padding: f32,
) -> f32 {
    SECTION_HEADER_HEIGHT
        + SECTION_HEADER_GAP
        + grid_height(item_count, columns, top_padding, bottom_padding)
}

#[cfg(test)]
mod tests {
    use super::{CARD_HEIGHT, ROW_GAP, SECTION_HEADER_GAP, SECTION_HEADER_HEIGHT};

    #[test]
    fn section_heights_include_headers_gaps_and_partial_rows() {
        let rows = super::grid_height(4, 3, 6.0, 10.0);
        assert_eq!(rows, 6.0 + 2.0 * CARD_HEIGHT + ROW_GAP + 10.0);
        assert_eq!(
            super::section_height(4, 3, 6.0, 10.0),
            rows + SECTION_HEADER_HEIGHT + SECTION_HEADER_GAP
        );
    }
}
