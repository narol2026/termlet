//! Collection grid layout helpers.

// Rust guideline compliant 2026-10-09

use iced::widget::{column, container, row, text};
use iced::{Element, Length};

use crate::core::message::Message;

/// Shared column and card width for a collection grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollectionLayout {
    pub columns: usize,
    pub item_width: f32,
}

/// Calculate collection columns and card width for the available space.
pub fn collection_layout(
    available_width: f32,
    grid_view: bool,
    minimum_item_width: f32,
    maximum_columns: usize,
) -> CollectionLayout {
    let columns = if grid_view {
        ((available_width / minimum_item_width).floor() as usize).clamp(1, maximum_columns.max(1))
    } else {
        1
    };
    let gap_width = (columns.saturating_sub(1)) as f32 * 12.0;
    CollectionLayout {
        columns,
        item_width: (available_width - gap_width) / columns as f32,
    }
}

/// Lay out collection items into fixed-height rows with trailing spacers.
pub fn collection_rows<'a>(
    items: impl IntoIterator<Item = Element<'a, Message>>,
    layout: CollectionLayout,
    row_height: f32,
    row_gap: f32,
) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut current: Vec<Element<'a, Message>> = Vec::with_capacity(layout.columns);

    for item in items {
        current.push(container(item).height(Length::Fixed(row_height)).into());
        if current.len() == layout.columns {
            while current.len() < layout.columns {
                current.push(container(text("")).width(Length::Fill).into());
            }
            rows.push(row(std::mem::take(&mut current)).spacing(row_gap).into());
            current = Vec::with_capacity(layout.columns);
        }
    }

    if !current.is_empty() {
        while current.len() < layout.columns {
            current.push(container(text("")).width(Length::Fill).into());
        }
        rows.push(row(current).spacing(row_gap).into());
    }

    column(rows).spacing(row_gap).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_layout_calculates_columns_and_gaps() {
        let grid = collection_layout(1_000.0, true, 320.0, 6);
        assert_eq!(grid.columns, 3);
        assert_eq!(grid.item_width, (1_000.0 - 24.0) / 3.0);

        let list = collection_layout(1_000.0, false, 320.0, 6);
        assert_eq!(list.columns, 1);
        assert_eq!(list.item_width, 1_000.0);
    }
}
