//! Shared control sizing and text measurement helpers.

// Rust guideline compliant 2026-10-09

use iced::widget::text;
use iced::{Padding, Pixels};

/// Height of every single-line control that sits in a toolbar or form row:
/// buttons, dropdown triggers and search/text fields.
///
/// This is the dense content-chrome height, independent from the navigation
/// heights ([`crate::views::TAB_HEIGHT`] for the topbar tab strip and
/// [`crate::views::ITEM_HEIGHT`] for sidebar/list rows), so the toolbars can be
/// tightened without dragging the navigation along. A control reaches this
/// height through [`control_padding`] — never by hard-coding its padding — so
/// the height stays put when a label, icon size or font size changes.
pub const CONTROL_HEIGHT: f32 = 30.0;

/// Rendered height of a single `text_size` line, derived from iced's own
/// default line height (currently `LineHeight::Relative(1.3)`) instead of
/// duplicating the ratio here.
fn control_line_height(text_size: f32) -> f32 {
    f32::from(text::LineHeight::default().to_absolute(Pixels(text_size)))
}

/// Vertical padding that lands a `text_size`-tall single line exactly on
/// [`CONTROL_HEIGHT`]. Symmetric, so the content stays vertically centered.
pub fn control_vertical_padding(text_size: f32) -> f32 {
    ((CONTROL_HEIGHT - control_line_height(text_size)) / 2.0).max(0.0)
}

/// Padding for a single-line control of [`CONTROL_HEIGHT`]: the vertical part
/// is derived from the content's line height, the horizontal part is the
/// caller's own. Use this instead of `Padding::from([y, x])` for any control
/// that should line up with the other controls in its row.
pub fn control_padding(text_size: f32, horizontal: f32) -> Padding {
    let vertical = control_vertical_padding(text_size);
    Padding {
        top: vertical,
        right: horizontal,
        bottom: vertical,
        left: horizontal,
    }
}

/// Terminal-style display width of a char (1 or 2 cells) so width estimates
/// can account for CJK, emoji, and fullwidth glyphs instead of counting bytes.
pub fn char_cells(c: char) -> u16 {
    let cp = c as u32;
    let wide = (0x1100..=0x115F).contains(&cp) // Hangul Jamo
        || (0x2E80..=0xA4CF).contains(&cp) // CJK Radicals..Yi
        || (0xAC00..=0xD7A3).contains(&cp) // Hangul Syllables
        || (0xF900..=0xFAFF).contains(&cp) // CJK Compatibility Ideographs
        || (0xFE10..=0xFE19).contains(&cp) // Vertical forms
        || (0xFE30..=0xFE6F).contains(&cp) // CJK Compatibility Forms
        || (0xFF00..=0xFF60).contains(&cp) // Fullwidth Forms
        || (0xFFE0..=0xFFE6).contains(&cp) // Fullwidth Signs
        || (0x1F300..=0x1F64F).contains(&cp) // Emoticons..pictographs
        || (0x1F900..=0x1F9FF).contains(&cp) // Supplemental Symbols
        || (0x20000..=0x2FFFD).contains(&cp) // CJK Ext B..F
        || (0x30000..=0x3FFFD).contains(&cp);
    if wide { 2 } else { 1 }
}

/// Approximate rendered width of `s` in px, given the width of one narrow cell
/// at the target font size (wide chars count double).
pub fn text_width_estimate(s: &str, px_per_cell: f32) -> f32 {
    s.chars()
        .map(|c| f32::from(char_cells(c)) * px_per_cell)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of [`control_padding`] is that a control lands exactly
    /// on [`CONTROL_HEIGHT`] whatever the text size, so the vaults toolbars
    /// cannot drift away from the topbar tabs again.
    #[test]
    fn control_padding_lands_on_control_height() {
        for text_size in [11.0, 12.0, 13.0, 14.0, 20.0] {
            let padding = control_padding(text_size, 8.0);
            let total = padding.top + control_line_height(text_size) + padding.bottom;
            assert!(
                (total - CONTROL_HEIGHT).abs() < f32::EPSILON,
                "text size {text_size} produced {total} instead of {CONTROL_HEIGHT}"
            );
        }
    }

    /// Text larger than the control itself cannot be padded into it; clamping
    /// keeps the padding non-negative instead of producing a negative height.
    #[test]
    fn control_padding_never_goes_negative() {
        assert_eq!(control_vertical_padding(40.0), 0.0);
    }

    #[test]
    fn wide_glyphs_take_two_cells_and_narrow_ones_take_one() {
        // ASCII, Latin accents and Cyrillic are single-width.
        for narrow in ['a', 'Z', '0', '/', 'é', 'Ω', 'д'] {
            assert_eq!(char_cells(narrow), 1, "{narrow:?} should be one cell");
        }

        // CJK, Hangul, fullwidth punctuation and emoji occupy two cells, which
        // is what keeps the width estimates from under-counting these rows.
        for wide in ['中', 'あ', '한', '，', '！', '😀', '🙂'] {
            assert_eq!(char_cells(wide), 2, "{wide:?} should be two cells");
        }
    }

    #[test]
    fn width_estimates_count_cells_not_bytes() {
        // Four CJK glyphs cost as much as eight Latin ones.
        assert_eq!(text_width_estimate("中文字符", 10.0), 80.0);
        assert_eq!(text_width_estimate("abcdefgh", 10.0), 80.0);
        assert_eq!(text_width_estimate("", 10.0), 0.0);

        assert!(text_width_estimate("中文", 10.0) > text_width_estimate("ab", 10.0));
    }
}
