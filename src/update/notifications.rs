//! Toast and transient prompt updates.

use iced::Task;

use super::open_snippet_form_for_command;
use super::{App, Message};
use crate::core::data::LogCategory;

pub(super) fn update(app: &mut App, msg: Message) -> Task<Message> {
    match msg {
        Message::ShowToast(message) => show(app, message, crate::app::ToastKind::Info),
        Message::ShowWarningToast(message) => show(app, message, crate::app::ToastKind::Warning),
        Message::ShowErrorToast(message) => show(app, message, crate::app::ToastKind::Error),
        Message::DismissToast(id) => {
            if app.toast.as_ref().map(|t| t.id) == Some(id) {
                app.toast_closing = true;
            }
            Task::none()
        }
        Message::CommandFrequencyNotify { command, count } => {
            app.add_log(
                LogCategory::Terminal,
                format!("Command frequency prompt: \"{command}\" used {count} times"),
            );
            app.command_frequency_prompt =
                Some(crate::app::CommandFrequencyPrompt { command, count });
            app.command_frequency_prompt_anim = 0.0;
            app.command_frequency_prompt_closing = false;
            app.command_frequency_prompt_seq += 1;
            let seq = app.command_frequency_prompt_seq;
            // Same 8 s as the session-CWD prompt.
            Task::perform(
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                    seq
                },
                Message::CommandFrequencyDismiss,
            )
        }
        Message::CommandFrequencyDismiss(seq) => {
            if app.command_frequency_prompt_seq == seq && app.command_frequency_prompt.is_some() {
                app.command_frequency_prompt_closing = true;
            }
            Task::none()
        }
        Message::CommandFrequencyAddToSnippet { command } => {
            app.command_frequency_prompt_closing = true;
            open_snippet_form_for_command(app, &command);
            Task::none()
        }
        _ => unreachable!("notification handler received another message"),
    }
}

fn show(app: &mut App, message: String, kind: crate::app::ToastKind) -> Task<Message> {
    app.add_log(LogCategory::System, format!("[Toast] {message}"));
    app.toast_seq += 1;
    let id = app.toast_seq;
    app.toast = Some(crate::app::Toast { id, message, kind });
    app.toast_anim = 0.0;
    app.toast_closing = false;
    Task::perform(
        async {
            tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
        },
        move |()| Message::DismissToast(id),
    )
}
