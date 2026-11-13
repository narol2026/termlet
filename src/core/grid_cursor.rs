//! Tracks an active item in an ordered grid.

use crate::core::list_cursor::ListCursor;

/// A direction in a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridDirection {
    Up,
    Down,
    Left,
    Right,
}

/// A [`ListCursor`] with row-aware movement.
///
/// The cursor stores a unique item identity, so filtering or reordering the
/// grid keeps the highlight on the same row. Callers pass the current visible
/// items and column count on every move because responsive layouts can change
/// the number of columns while the cursor stays put. The stored identity may
/// temporarily be absent from the visible items.
///
/// Vertical movement crosses one row. Horizontal movement stays inside the
/// current row instead of wrapping onto the row above or below. An empty cursor
/// starts from the edge the key points at, and movement stops at the grid edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridCursor<T> {
    inner: ListCursor<T>,
}

impl<T> Default for GridCursor<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> GridCursor<T> {
    /// Creates a cursor with no selected item.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: ListCursor::new(),
        }
    }

    /// Returns the stored identity, even when it is not visible.
    #[must_use]
    pub fn selected_key(&self) -> Option<&T> {
        self.inner.selected_key()
    }

    /// Selects an item directly.
    pub fn select(&mut self, item: T) {
        self.inner.select(item);
    }

    /// Clears the current selection.
    pub fn clear(&mut self) {
        self.inner.clear();
    }
}

impl<T: Clone + Eq> GridCursor<T> {
    /// Returns the position of the stored unique identity in `items`.
    #[must_use]
    pub fn selected_index(&self, items: &[T]) -> Option<usize> {
        self.inner.selected_index(items)
    }

    /// Returns the stored identity when it is present in `items`.
    #[must_use]
    pub fn selected_in<'a>(&self, items: &'a [T]) -> Option<&'a T> {
        self.inner.selected_in(items)
    }

    /// Moves the selection in `direction`, respecting row boundaries.
    pub fn move_by(&mut self, items: &[T], columns: usize, direction: GridDirection) {
        match direction {
            GridDirection::Up | GridDirection::Down => {
                self.move_vertical(items, columns, direction == GridDirection::Up);
            }
            GridDirection::Left | GridDirection::Right => {
                self.move_horizontal(items, columns, direction == GridDirection::Left);
            }
        }
    }

    /// Moves through stacked grid sections.
    ///
    /// Movement stays inside the current section except when a vertical key
    /// cannot move any further; the cursor then enters the first item of the
    /// next non-empty section, or the last item of the previous one.
    /// Horizontal movement never crosses a section boundary.
    pub fn move_in_sections(
        &mut self,
        sections: &[Vec<T>],
        columns: usize,
        direction: GridDirection,
        preferred_section: Option<usize>,
    ) {
        let edge_section = || match direction {
            GridDirection::Up | GridDirection::Left => {
                sections.iter().rposition(|section| !section.is_empty())
            }
            GridDirection::Down | GridDirection::Right => {
                sections.iter().position(|section| !section.is_empty())
            }
        };
        let section_index = preferred_section
            .filter(|&index| {
                sections
                    .get(index)
                    .is_some_and(|section| !section.is_empty())
            })
            .or_else(|| {
                self.selected_key().and_then(|selected| {
                    sections
                        .iter()
                        .position(|section| section.iter().any(|item| item == selected))
                })
            })
            .or_else(edge_section);
        let Some(section_index) = section_index else {
            return;
        };

        let before = self.selected_key().cloned();
        self.move_by(&sections[section_index], columns, direction);
        if before != self.selected_key().cloned() {
            return;
        }

        match direction {
            GridDirection::Down => {
                if let Some(section) = sections[section_index + 1..]
                    .iter()
                    .find(|section| !section.is_empty())
                {
                    self.select(section[0].clone());
                }
            }
            GridDirection::Up => {
                if let Some(section) = sections[..section_index]
                    .iter()
                    .rev()
                    .find(|section| !section.is_empty())
                {
                    self.select(section.last().expect("section is non-empty").clone());
                }
            }
            GridDirection::Left | GridDirection::Right => {}
        }
    }

    fn move_vertical(&mut self, items: &[T], columns: usize, up: bool) {
        let Some(last) = items.len().checked_sub(1) else {
            return;
        };
        let step = columns.max(1);
        let Some(index) = self.selected_index(items) else {
            self.select(items[if up { last } else { 0 }].clone());
            return;
        };
        let target = if up {
            index.checked_sub(step)
        } else {
            index.checked_add(step).filter(|target| *target <= last)
        };
        if let Some(target) = target {
            self.select(items[target].clone());
        }
    }

    fn move_horizontal(&mut self, items: &[T], columns: usize, left: bool) {
        let Some(last) = items.len().checked_sub(1) else {
            return;
        };
        let columns = columns.max(1);
        let Some(index) = self.selected_index(items) else {
            self.select(items[if left { last } else { 0 }].clone());
            return;
        };
        let row = index / columns;
        let target = if left {
            index
                .checked_sub(1)
                .filter(|target| target / columns == row)
        } else {
            index
                .checked_add(1)
                .filter(|target| *target <= last && target / columns == row)
        };
        if let Some(target) = target {
            self.select(items[target].clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GridCursor, GridDirection};

    fn grid() -> Vec<&'static str> {
        vec!["1", "2", "3", "4", "5", "6", "7"]
    }

    #[test]
    fn vertical_moves_cross_one_row() {
        let mut cursor = GridCursor::new();
        cursor.select("1");

        cursor.move_by(&grid(), 3, GridDirection::Down);
        assert_eq!(cursor.selected_key(), Some(&"4"));
        cursor.move_by(&grid(), 3, GridDirection::Down);
        assert_eq!(cursor.selected_key(), Some(&"7"));
    }

    #[test]
    fn horizontal_moves_stay_inside_the_current_row() {
        let mut cursor = GridCursor::new();
        cursor.select("3");

        cursor.move_by(&grid(), 3, GridDirection::Right);
        assert_eq!(
            cursor.selected_key(),
            Some(&"3"),
            "right at the row edge does not wrap"
        );
        cursor.move_by(&grid(), 3, GridDirection::Left);
        assert_eq!(cursor.selected_key(), Some(&"2"));
    }

    #[test]
    fn a_partial_last_row_has_nothing_below_it() {
        let mut cursor = GridCursor::new();
        cursor.select("7");

        cursor.move_by(&grid(), 3, GridDirection::Down);

        assert_eq!(cursor.selected_key(), Some(&"7"));
    }

    #[test]
    fn movement_starts_from_the_edge_the_key_points_at() {
        let mut cursor = GridCursor::new();

        cursor.move_by(&grid(), 3, GridDirection::Down);
        assert_eq!(cursor.selected_key(), Some(&"1"));

        cursor.clear();
        cursor.move_by(&grid(), 3, GridDirection::Up);
        assert_eq!(cursor.selected_key(), Some(&"7"));
    }

    #[test]
    fn a_changed_column_count_uses_the_new_layout() {
        let mut cursor = GridCursor::new();
        cursor.select("1");

        cursor.move_by(&grid(), 2, GridDirection::Down);
        assert_eq!(cursor.selected_key(), Some(&"3"));
        cursor.move_by(&grid(), 2, GridDirection::Right);
        assert_eq!(cursor.selected_key(), Some(&"4"));
    }

    #[test]
    fn an_empty_grid_is_harmless() {
        let mut cursor: GridCursor<&str> = GridCursor::new();

        cursor.move_by(&[], 3, GridDirection::Down);

        assert_eq!(cursor.selected_key(), None);
    }

    #[test]
    fn selected_in_requires_the_item_to_be_visible() {
        let mut cursor = GridCursor::new();
        cursor.select("4");

        assert_eq!(cursor.selected_in(&grid()), Some(&"4"));
        assert_eq!(cursor.selected_in(&["alpha", "gamma"]), None);
    }

    #[test]
    fn vertical_edges_cross_into_adjacent_non_empty_sections() {
        let mut cursor = GridCursor::new();
        let sections = vec![vec!["a", "b", "c"], Vec::new(), vec!["d", "e"]];

        cursor.select("c");
        cursor.move_in_sections(&sections, 2, GridDirection::Down, Some(0));
        assert_eq!(
            cursor.selected_key(),
            Some(&"d"),
            "down at the last row enters the next non-empty section"
        );

        cursor.move_in_sections(&sections, 2, GridDirection::Up, Some(2));
        assert_eq!(
            cursor.selected_key(),
            Some(&"c"),
            "up from the first row returns to the previous section's last item"
        );
    }

    #[test]
    fn horizontal_edges_never_cross_sections() {
        let mut cursor = GridCursor::new();
        let sections = vec![vec!["a", "b"], vec!["c", "d"]];
        cursor.select("b");

        cursor.move_in_sections(&sections, 2, GridDirection::Right, Some(0));

        assert_eq!(cursor.selected_key(), Some(&"b"));
    }
}
