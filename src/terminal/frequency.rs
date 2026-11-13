//! Command-frequency tracking for the "used N times" notification.
//!
//! Recognition is driven by the shell-integration path: the terminal backend
//! emits `OSC 633 ; E`/`C` (`CommandExecuted`) for every command the shell is
//! about to run, and [`super::update`] calls [`observe`] with that exact
//! command text. Nothing here inspects keystrokes, PTY bytes, or the terminal
//! grid, and nothing here touches the UI — the caller decides how to present
//! the returned [`FrequencyHit`].
//!
//! Counts and the set of already-prompted commands are persisted per host
//! scope (matching the command-history files), so "used times" survives
//! restarts instead of resetting on every launch.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::core::ids::HostId;

pub const PROMPT_THRESHOLD: u32 = 3;

/// A command that just crossed [`PROMPT_THRESHOLD`] and should be suggested
/// to the user as a snippet.
#[derive(Debug, Clone)]
pub struct FrequencyHit {
    pub command: String,
    pub count: u32,
}

/// Persisted frequency state for one host scope.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandFrequency {
    /// Command → how many times it has been executed.
    pub counts: HashMap<String, u32>,
    /// Commands that have already triggered a prompt, so they are never
    /// suggested again.
    pub prompted: HashSet<String>,
}

/// Load the persisted frequency state for a host scope.
///
/// Returns an empty state when the file is missing or unreadable — this is
/// best-effort cache data.
pub fn load(host_id: Option<HostId>) -> CommandFrequency {
    let path = crate::core::paths::command_frequency_file(host_id);
    if !path.exists() {
        return CommandFrequency::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}

/// Persist the frequency state for a host scope.
///
/// Failures are silently ignored, matching the command-history cache.
pub fn save(host_id: Option<HostId>, state: &CommandFrequency) {
    let path = crate::core::paths::command_frequency_file(host_id);
    if let Ok(json) = serde_json::to_string(state) {
        let _ = crate::core::paths::write_private_atomic(&path, json.as_bytes());
    }
}

/// Record one execution of `command` and return a [`FrequencyHit`] when the
/// command crossed [`PROMPT_THRESHOLD`] and has not been prompted yet.
///
/// `already_saved` suppresses the prompt for commands the user already has in
/// their snippet list; such commands are still counted, but marked as
/// prompted so they are never re-checked.
pub fn record(
    state: &mut CommandFrequency,
    command: &str,
    already_saved: bool,
) -> Option<FrequencyHit> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return None;
    }

    let count = state.counts.entry(cmd.to_string()).or_insert(0);
    *count += 1;
    let current = *count;

    if already_saved {
        state.prompted.insert(cmd.to_string());
        return None;
    }

    if current >= PROMPT_THRESHOLD && !state.prompted.contains(cmd) {
        state.prompted.insert(cmd.to_string());
        return Some(FrequencyHit {
            command: cmd.to_string(),
            count: current,
        });
    }

    None
}

/// Load, record, and persist in one call — the recognition entry point used
/// by the terminal update loop.
pub fn observe(
    host_id: Option<HostId>,
    command: &str,
    already_saved: bool,
) -> Option<FrequencyHit> {
    let mut state = load(host_id);
    let hit = record(&mut state, command, already_saved);
    save(host_id, &state);
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_trims_and_counts() {
        let mut state = CommandFrequency::default();
        assert!(record(&mut state, "  git status  ", false).is_none());
        assert!(record(&mut state, "git status", false).is_none());
        let hit = record(&mut state, "git status", false).unwrap();
        assert_eq!(hit.command, "git status");
        assert_eq!(hit.count, 3);
    }

    #[test]
    fn record_prompts_once() {
        let mut state = CommandFrequency::default();
        for _ in 0..5 {
            let _ = record(&mut state, "ls", false);
        }
        assert!(state.prompted.contains("ls"));
        assert!(record(&mut state, "ls", false).is_none());
    }

    #[test]
    fn record_skips_already_saved() {
        let mut state = CommandFrequency::default();
        for _ in 0..PROMPT_THRESHOLD {
            assert!(record(&mut state, "rm -rf /tmp/x", true).is_none());
        }
        assert!(state.prompted.contains("rm -rf /tmp/x"));
    }

    #[test]
    fn record_ignores_blank() {
        let mut state = CommandFrequency::default();
        assert!(record(&mut state, "   ", false).is_none());
        assert!(state.counts.is_empty());
    }
}
