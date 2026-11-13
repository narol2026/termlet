/// A form draft's save gate: the view disables the Save button on it and the
/// submit handler ignores a spurious submit with it. It splits what used to be
/// one flat `&&` chain into two orthogonal questions — is the draft complete
/// ([`FormDraft::is_complete`]) and does it change the record it edits
/// ([`FormDraft::is_dirty`]). Implementors snapshot the record as
/// `original: Option<Record>` when editing begins, so `is_dirty` needs no
/// lookup.
pub trait FormDraft {
    /// Required fields are filled and valid, independent of whether anything
    /// changed since the form opened.
    fn is_complete(&self) -> bool;

    /// The draft differs from the record being edited. Always `true` when
    /// creating (no `original` snapshot), so a fresh, empty form is gated by
    /// [`FormDraft::is_complete`] alone.
    fn is_dirty(&self) -> bool;

    /// Combined gate: complete **and** changed.
    fn save_enabled(&self) -> bool {
        self.is_complete() && self.is_dirty()
    }
}
