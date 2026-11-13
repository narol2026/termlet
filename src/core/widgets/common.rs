//! Shared visual primitives for reusable widgets.

// Rust guideline compliant 2026-10-09

use iced::{Color, Font};

pub(super) const LUCIDE_FONT: Font = Font {
    family: iced::font::Family::Name("lucide"),
    ..Font::DEFAULT
};

pub(super) fn with_alpha(base: Color, alpha: f32) -> Color {
    Color { a: alpha, ..base }
}
