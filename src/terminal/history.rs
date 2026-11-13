//! Shared helpers for the per-session MRU command history.
//!
//! Each [`TerminalSession`] keeps an MRU ring of the commands run in that
//! terminal, stored oldest-first; the sidebar renders it reversed so the newest
//! command is at the top, and the list is persisted per host scope to
//! `~/.termlet/command_history/<scope>.json`. Commands arrive from the OSC
//! 633 ; C hook and from a direct re-run in the sidebar, which has to re-sort
//! the command to the front even when the shell emits no OSC 633 for it.
//!
//! The ring is bounded by the `command_history_limit` preference — picked in
//! Profiles from [`crate::core::data::COMMAND_HISTORY_LIMITS`], and 1000 by
//! default — so a raised limit keeps more of what a lowered one drops.

use crate::core::cache::CommandHistoryEntry;
use crate::terminal::TerminalSession;

/// Record `cmd` as the most recently executed command for `session`.
///
/// `limit` is the configured ring size; it is applied on every record so a
/// limit lowered mid-session takes effect as soon as the next command runs.
/// The command is inserted verbatim — no prompt stripping.
pub fn record(session: &mut TerminalSession, cmd: &str, limit: usize) {
    if push_mru(&mut session.recent_commands, cmd, limit) {
        crate::core::cache::save_command_history(session.host_id, &session.recent_commands);
    }
}

/// Move `cmd` to the most-recent slot (the end of the vector) of the MRU ring
/// buffer: deduplicate any existing match, then drop the oldest entries while
/// the ring is over `limit`.
///
/// Returns `true` when the command was recorded, `false` when it was blank
/// (whitespace only) and the ring was left untouched.
fn push_mru(recent_commands: &mut Vec<CommandHistoryEntry>, cmd: &str, limit: usize) -> bool {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return false;
    }
    recent_commands.retain(|entry| entry.command != cmd);
    recent_commands.push(CommandHistoryEntry {
        command: cmd.to_string(),
        executed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    });
    if recent_commands.len() > limit {
        recent_commands.drain(..recent_commands.len() - limit);
    }
    true
}

/// Drop the oldest entries until at most `limit` are left.
///
/// A ring restored from disk was written under whatever limit was in force
/// then, so history loaded into a session has to be trimmed before it is shown.
pub fn trim_to_limit(recent_commands: &mut Vec<CommandHistoryEntry>, limit: usize) {
    let excess = recent_commands.len().saturating_sub(limit);
    recent_commands.drain(..excess);
}

/// The command the shell announced for the prompt cycle that is still in
/// flight, for a report that has to name it.
///
/// A shell with integration announces the command over OSC 633 before it runs
/// and restores the authoritative working directory with OSC 7 once the prompt
/// is back, so `working_directory_synced` being `false` means one cycle is in
/// flight and the newest entry of the MRU ring is that command. When the flag
/// is `true` the report came from a shell-side hook that ran without announcing
/// a command — a `Ctrl-C`, say — and naming the newest entry would blame
/// whichever command happened to run last.
pub fn in_flight_command(
    working_directory_synced: bool,
    recent_commands: &[CommandHistoryEntry],
) -> Option<&str> {
    if working_directory_synced {
        return None;
    }
    recent_commands.last().map(|entry| entry.command.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_mru_moves_existing_entry_to_most_recent_slot() {
        let mut cmds = vec![entry("a", 1), entry("b", 2), entry("c", 3)];
        push_mru(&mut cmds, "b", 10);
        assert_eq!(
            cmds.iter().map(|e| e.command.as_str()).collect::<Vec<_>>(),
            vec!["a", "c", "b"]
        );
        assert!(cmds[2].executed_at > 3);
    }

    #[test]
    fn push_mru_never_duplicates() {
        let mut cmds = vec![entry("x", 1), entry("y", 2)];
        push_mru(&mut cmds, "x", 10);
        assert_eq!(
            cmds.iter().map(|e| e.command.as_str()).collect::<Vec<_>>(),
            vec!["y", "x"]
        );
    }

    #[test]
    fn push_mru_drops_oldest_when_the_ring_is_at_its_limit() {
        let mut cmds: Vec<CommandHistoryEntry> =
            (0..5).map(|i| entry(&format!("c{i}"), i)).collect();
        push_mru(&mut cmds, "new", 5);
        assert_eq!(cmds.len(), 5);
        assert_eq!(cmds[4].command, "new");
        assert!(!cmds.iter().any(|e| e.command == "c0"));
    }

    #[test]
    fn push_mru_grows_the_ring_past_the_hundred_the_sidebar_used_to_hold() {
        let mut cmds: Vec<CommandHistoryEntry> = Vec::new();
        for i in 0..150 {
            push_mru(&mut cmds, &format!("c{i}"), 10_000);
        }
        assert_eq!(cmds.len(), 150);
        assert_eq!(cmds[0].command, "c0");
    }

    #[test]
    fn trim_to_limit_keeps_the_newest_commands() {
        let mut cmds: Vec<CommandHistoryEntry> =
            (0..5).map(|i| entry(&format!("c{i}"), i)).collect();
        trim_to_limit(&mut cmds, 2);
        assert_eq!(
            cmds.iter().map(|e| e.command.as_str()).collect::<Vec<_>>(),
            vec!["c3", "c4"]
        );
    }

    #[test]
    fn trim_to_limit_leaves_a_ring_under_the_limit_alone() {
        let mut cmds = vec![entry("a", 1)];
        trim_to_limit(&mut cmds, 1000);
        assert_eq!(cmds.len(), 1);
    }

    #[test]
    fn a_command_is_in_flight_only_until_the_prompt_comes_back() {
        let cmds = vec![entry("ls", 1), entry("make -j8", 2)];

        assert_eq!(in_flight_command(false, &cmds), Some("make -j8"));
        assert_eq!(
            in_flight_command(true, &cmds),
            None,
            "a status reported with the prompt back belongs to no announced command"
        );
    }

    #[test]
    fn a_session_that_never_announced_a_command_has_none_in_flight() {
        assert_eq!(in_flight_command(false, &[]), None);
    }

    #[test]
    fn record_ignores_blank_commands() {
        let mut cmds = vec![entry("a", 1)];
        push_mru(&mut cmds, "  ", 10);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].command, "a");
    }

    fn entry(command: &str, executed_at: i64) -> CommandHistoryEntry {
        CommandHistoryEntry {
            command: command.to_string(),
            executed_at,
        }
    }
}
