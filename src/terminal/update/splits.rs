//! Split pane interaction updates.

// Rust guideline compliant 2026-10-09

use iced::Task;
use iced::widget::pane_grid;

use super::{
    App, DomainMessage, clamp_resize, close_terminal_guarded, focus_split_pane, unsplit_keep_pane,
};
use crate::core::message::Message;

pub(super) fn update(app: &mut App, msg: &DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::SplitClicked(split_id, pane) => focus_split_pane(app, *split_id, *pane),
        DomainMessage::SplitDragged(split_id, event) => {
            let pane = {
                let Some(split) = app.terminal.splits.get_mut(split_id) else {
                    return Task::none();
                };
                split.apply_drag(event)
            };
            focus_split_pane(app, *split_id, pane)
        }
        DomainMessage::SplitResized(
            split_id,
            pane_grid::ResizeEvent { split, ratio },
            available,
        ) => {
            if let Some(state) = app.terminal.splits.get_mut(split_id) {
                let ratio = clamp_resize(state, *available, *split, *ratio);
                state.panes.resize(*split, ratio);
            }
            Task::none()
        }
        DomainMessage::SplitClose(split_id, pane) => close_terminal_guarded(
            app,
            crate::terminal::TerminalCloseTarget::Pane(*split_id, *pane),
        ),
        DomainMessage::SplitUnsplit(split_id, pane) => unsplit_keep_pane(app, *split_id, *pane),
        _ => unreachable!("terminal split handler received another message"),
    }
}
