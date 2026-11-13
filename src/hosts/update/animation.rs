//! Advances host-owned panel animations.

// Rust guideline compliant 2026-10-09

use crate::app::App;

/// Advance host-form and transfer-dialog animations on each UI tick.
///
/// These animations are driven by the global animation message rather than a
/// host-domain message, so the hosts updater exposes this entry point.
pub(crate) fn tick_animations(app: &mut App) {
    if app.hosts.form_closing {
        app.hosts.form_anim =
            (app.hosts.form_anim - crate::ui::helpers::SLIDE_PANEL_ANIM_STEP).max(0.0);
        if app.hosts.form_anim <= 0.0 {
            app.hosts.form.visible = false;
            app.hosts.form_closing = false;
            app.hosts.form.reset();
            app.needs_scroll_restore = true;
        }
    } else {
        app.hosts.form_anim =
            (app.hosts.form_anim + crate::ui::helpers::SLIDE_PANEL_ANIM_STEP).min(1.0);
    }

    // The scrypt-based import/export work can take about a second.
    if let Some(dialog) = app.hosts.transfer_dialog.as_mut()
        && dialog.busy
    {
        dialog.anim_t += 0.016;
    }
}
