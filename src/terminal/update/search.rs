//! In-terminal search updates.

use iced::Task;

use super::{App, DomainMessage, rerun_terminal_search, search_options, toggle_search_option};
use crate::core::data::LogCategory;
use crate::core::message::Message;

pub(super) fn update(app: &mut App, msg: DomainMessage) -> Task<Message> {
    match msg {
        DomainMessage::TerminalSearch(session_id, query) => {
            let mut match_count = 0;
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
            {
                session.search_query.clone_from(&query);
                session.search_matches_count = 0;
                session.search_index = 0;
                if query.is_empty() {
                    session.terminal.clear_selection();
                    return Task::none();
                }
                let options = search_options(session);
                let (matches, idx) = session.terminal.search_jump(&query, 0, options);
                session.search_matches_count = matches.len();
                session.search_index = idx.unwrap_or(0);
                match_count = session.search_matches_count;
            }
            if match_count > 0 {
                app.add_log(
                    LogCategory::Terminal,
                    format!("Searched in terminal: \"{query}\" — {match_count} match(es)"),
                );
            }
            Task::none()
        }
        DomainMessage::SearchNext(session_id) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                && session.search_matches_count > 0
            {
                session.search_index = (session.search_index + 1) % session.search_matches_count;
                let options = search_options(session);
                session
                    .terminal
                    .search_jump(&session.search_query, session.search_index, options);
            }
            Task::none()
        }
        DomainMessage::SearchPrev(session_id) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                && session.search_matches_count > 0
            {
                session.search_index = session
                    .search_index
                    .checked_sub(1)
                    .unwrap_or(session.search_matches_count - 1);
                let options = search_options(session);
                session
                    .terminal
                    .search_jump(&session.search_query, session.search_index, options);
            }
            Task::none()
        }
        DomainMessage::SearchToggleCase(session_id) => {
            toggle_search_option(app, session_id, |session| {
                session.search_case_sensitive = !session.search_case_sensitive;
            });
            Task::none()
        }
        DomainMessage::SearchToggleWholeWord(session_id) => {
            toggle_search_option(app, session_id, |session| {
                session.search_whole_word = !session.search_whole_word;
            });
            Task::none()
        }
        DomainMessage::SearchToggleRegex(session_id) => {
            toggle_search_option(app, session_id, |session| {
                session.search_regex = !session.search_regex;
            });
            Task::none()
        }
        DomainMessage::SearchClose(session_id) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
                && session.show_search
            {
                // `AnimTick` hides the bar and clears the selection after the
                // exit animation; the query survives until session close.
                session.search_closing = true;
            }
            Task::none()
        }
        DomainMessage::ShowSearch(session_id) => {
            if let Some(session) = app
                .terminal
                .sessions
                .iter_mut()
                .find(|s| s.id == session_id)
            {
                session.show_search = true;
                session.search_closing = false;
                session.search_anim = 0.0;
                rerun_terminal_search(session);
            }
            Task::none()
        }
        _ => unreachable!("terminal search handler received another message"),
    }
}
