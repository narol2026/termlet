/// Duration of right-side slide panels (terminal sidebar and vault forms).
pub const SLIDE_PANEL_ANIM_DURATION_MS: u32 = 260;
/// Per-tick increment at the app's 60fps animation tick.
pub const SLIDE_PANEL_ANIM_STEP: f32 = 16.0 / SLIDE_PANEL_ANIM_DURATION_MS as f32;

/// Cubic ease-out function (identical to CSS `ease-out`).
///
/// Maps `t` in \[0, 1\] to an eased value in \[0, 1\].
#[inline]
pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// Tick a slide-in/out animation value towards 0 (closing) or 1 (opening).
///
/// Returns `true` when the animation reaches its target.
#[inline]
pub fn tick_slide_anim(anim: &mut f32, closing: bool, step: f32) -> bool {
    if closing {
        *anim = (*anim - step).max(0.0);
        *anim <= 0.0
    } else {
        *anim = (*anim + step).min(1.0);
        *anim >= 1.0
    }
}

/// Truncate `input` for display to at most `max_chars` characters, appending
/// an ellipsis when truncated. Operates on char boundaries so it never panics
/// on a multi-byte UTF-8 sequence.
pub fn truncate_display_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}
