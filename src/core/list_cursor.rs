//! Tracks an active item in an ordered, changing list.
//!
//! `T` is a stable identity for one row and must be unique in every item slice
//! passed to this cursor. If a list can contain duplicate values, give each row
//! a separate stable ID and use that ID as `T`.
//!
//! A stored key may be absent from the current visible items. This lets a
//! selection survive temporary filtering; use [`ListCursor::selected_in`] when
//! only a visible selection is valid.

// Rust guideline compliant 2026-10-09

/// Tracks the selected identity in an ordered list.
///
/// The cursor stores a unique identity instead of an index. Callers pass the
/// current ordered items to every operation, so filtering or reordering does
/// not silently move the selection to another item.
///
/// The stored key survives temporary absence from the list. It may therefore
/// refer to an item that is not currently visible.
///
/// An empty cursor starts at the first item when moving forward and at the last
/// item when moving backward. Movement stops at either end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListCursor<T> {
    selected_key: Option<T>,
}

impl<T> Default for ListCursor<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ListCursor<T> {
    /// Creates a cursor with no selected item.
    #[must_use]
    pub const fn new() -> Self {
        Self { selected_key: None }
    }

    /// Returns the stored identity, even when it is not visible.
    ///
    /// Use [`Self::selected_in`] when the caller needs a currently visible item.
    #[must_use]
    pub fn selected_key(&self) -> Option<&T> {
        self.selected_key.as_ref()
    }

    /// Stores a unique item identity directly.
    pub fn select(&mut self, item: T) {
        self.selected_key = Some(item);
    }

    /// Clears the current selection.
    pub fn clear(&mut self) {
        self.selected_key = None;
    }
}

impl<T: Clone + Eq> ListCursor<T> {
    /// Moves the selection to the next item.
    ///
    /// An empty cursor selects the first item. A selection at the end stays
    /// put, and a selection that is missing from `items` selects the first
    /// item.
    pub fn move_next(&mut self, items: &[T]) {
        self.move_by(items, Step::Next, false);
    }

    /// Moves the selection to the previous item.
    ///
    /// An empty cursor selects the last item. A selection at the start stays
    /// put, and a selection that is missing from `items` selects the last item.
    pub fn move_previous(&mut self, items: &[T]) {
        self.move_by(items, Step::Previous, false);
    }

    /// Moves to the next item, wrapping from the end to the first.
    pub fn move_next_wrapped(&mut self, items: &[T]) {
        self.move_by(items, Step::Next, true);
    }

    /// Moves to the previous item, wrapping from the first to the last.
    pub fn move_previous_wrapped(&mut self, items: &[T]) {
        self.move_by(items, Step::Previous, true);
    }

    /// Returns the stored identity when it is present in `items`.
    #[must_use]
    pub fn selected_in<'a>(&self, items: &'a [T]) -> Option<&'a T> {
        self.selected_index(items)
            .and_then(|index| items.get(index))
    }

    /// Returns the position of the stored unique identity in `items`.
    #[must_use]
    pub fn selected_index(&self, items: &[T]) -> Option<usize> {
        let selected = self.selected_key.as_ref()?;
        items.iter().position(|item| item == selected)
    }

    /// Returns the visible selection, or the first item as a fallback.
    ///
    /// The fallback does not replace the stored identity.
    #[must_use]
    pub fn selected_or_first<'a>(&self, items: &'a [T]) -> Option<&'a T> {
        self.selected_in(items).or_else(|| items.first())
    }

    fn move_by(&mut self, items: &[T], step: Step, wrapped: bool) {
        if items.is_empty() {
            return;
        }

        let next = match (step, self.selected_index(items)) {
            (Step::Next, Some(index)) if wrapped => (index + 1) % items.len(),
            (Step::Previous, Some(index)) if wrapped => {
                index.checked_sub(1).unwrap_or(items.len() - 1)
            }
            (Step::Next, Some(index)) => (index + 1).min(items.len() - 1),
            (Step::Previous, Some(index)) => index.saturating_sub(1),
            (Step::Next, None) => 0,
            (Step::Previous, None) => items.len() - 1,
        };
        self.select(items[next].clone());
    }
}

#[derive(Clone, Copy)]
enum Step {
    Next,
    Previous,
}

#[cfg(test)]
mod tests {
    use super::ListCursor;

    #[test]
    fn next_from_empty_selects_the_first_item() {
        let mut cursor = ListCursor::new();

        cursor.move_next(&["alpha", "beta", "gamma"]);

        assert_eq!(cursor.selected_key(), Some(&"alpha"));
    }

    #[test]
    fn previous_from_empty_selects_the_last_item() {
        let mut cursor = ListCursor::new();

        cursor.move_previous(&["alpha", "beta", "gamma"]);

        assert_eq!(cursor.selected_key(), Some(&"gamma"));
    }

    #[test]
    fn next_at_the_end_stays_on_the_last_item() {
        let mut cursor = ListCursor::new();
        cursor.select("gamma");

        cursor.move_next(&["alpha", "beta", "gamma"]);

        assert_eq!(cursor.selected_key(), Some(&"gamma"));
    }

    #[test]
    fn previous_at_the_start_stays_on_the_first_item() {
        let mut cursor = ListCursor::new();
        cursor.select("alpha");

        cursor.move_previous(&["alpha", "beta", "gamma"]);

        assert_eq!(cursor.selected_key(), Some(&"alpha"));
    }

    #[test]
    fn selection_follows_its_item_after_the_list_is_reordered() {
        let mut cursor = ListCursor::new();
        cursor.select("beta");

        cursor.move_next(&["gamma", "beta", "alpha"]);

        assert_eq!(cursor.selected_key(), Some(&"alpha"));
    }

    #[test]
    fn a_missing_selection_starts_from_the_direction_end() {
        let mut cursor = ListCursor::new();
        cursor.select("missing");

        cursor.move_next(&["alpha", "beta", "gamma"]);
        assert_eq!(cursor.selected_key(), Some(&"alpha"));

        cursor.select("missing");
        cursor.move_previous(&["alpha", "beta", "gamma"]);
        assert_eq!(cursor.selected_key(), Some(&"gamma"));
    }

    #[test]
    fn selected_in_requires_the_item_to_be_visible() {
        let mut cursor = ListCursor::new();
        cursor.select("beta");

        assert_eq!(
            cursor.selected_in(&["alpha", "beta", "gamma"]),
            Some(&"beta")
        );
        assert_eq!(cursor.selected_in(&["alpha", "gamma"]), None);
    }

    #[test]
    fn a_selection_returns_after_temporary_absence() {
        let mut cursor = ListCursor::new();
        cursor.select("beta");

        assert_eq!(cursor.selected_key(), Some(&"beta"));
        assert_eq!(cursor.selected_in(&["alpha", "gamma"]), None);
        assert_eq!(
            cursor.selected_in(&["alpha", "beta", "gamma"]),
            Some(&"beta")
        );
    }

    #[test]
    fn falling_back_to_the_first_item_keeps_the_stored_key() {
        let mut cursor = ListCursor::new();
        cursor.select("missing");

        assert_eq!(cursor.selected_or_first(&["alpha", "beta"]), Some(&"alpha"));
        assert_eq!(cursor.selected_key(), Some(&"missing"));
    }

    #[test]
    fn clear_removes_the_selection() {
        let mut cursor = ListCursor::new();
        cursor.select("beta");

        cursor.clear();

        assert_eq!(cursor.selected_key(), None);
    }

    #[test]
    fn wrapped_movement_crosses_both_ends() {
        let mut cursor = ListCursor::new();
        cursor.select("gamma");

        cursor.move_next_wrapped(&["alpha", "beta", "gamma"]);
        assert_eq!(cursor.selected_key(), Some(&"alpha"));

        cursor.move_previous_wrapped(&["alpha", "beta", "gamma"]);
        assert_eq!(cursor.selected_key(), Some(&"gamma"));
    }

    #[test]
    fn wrapped_movement_handles_empty_lists() {
        let mut cursor = ListCursor::<&str>::new();

        cursor.move_next_wrapped(&[]);
        assert_eq!(cursor.selected_key(), None);
        cursor.move_previous_wrapped(&[]);
        assert_eq!(cursor.selected_key(), None);
    }
}
