//! The spotlight command palette's command registry.
//!
//! A flat list of [`PaletteCommand`]s — the static terminal / SFTP commands —
//! is the single input to both the picker UI and the execute step.  Dispatch
//! is via [`PaletteCommandKind`] enum variants, so no control-flow string
//! sentinels are involved.

use lucide_icons::Icon;
use rust_i18n::t;

use crate::core::data::HostId;
use crate::core::i18n::t_static;

/// Grouping used for fixed ordering and the trailing category tag on each row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Terminal,
    Sftp,
}

impl Category {
    /// Localized display name for the trailing category tag.
    pub fn label(self) -> &'static str {
        match self {
            Category::Terminal => t_static(t!("terminal.spotlight_cat_terminal")),
            Category::Sftp => t_static(t!("terminal.spotlight_cat_sftp")),
        }
    }
}

/// What executing a palette entry does.
#[derive(Debug, Clone)]
pub enum PaletteCommandKind {
    NewTab,
    OpenLocalTerminal,
    CloseTab,
    OpenInSftp,
}

#[derive(Debug, Clone)]
pub struct PaletteCommand {
    /// Stable identifier (logging only, never localized).
    pub id: String,
    /// Resolved display name (localized).
    pub label: String,
    /// Resolved description (localized; may be empty).
    pub description: String,
    pub icon: Icon,
    pub category: Category,
    pub kind: PaletteCommandKind,
}

impl PaletteCommand {
    fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        description: impl Into<String>,
        icon: Icon,
        category: Category,
        kind: PaletteCommandKind,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: description.into(),
            icon,
            category,
            kind,
        }
    }
}

/// Assemble the full command list in fixed category order.
///
/// The host-less case is a local terminal. `Open in SFTP` depends on the
/// working directory reported through OSC 7, so it is omitted where local
/// shell integration is unavailable.
pub fn build_palette_commands(host_id: Option<HostId>) -> Vec<PaletteCommand> {
    let mut cmds = Vec::new();

    if crate::terminal::supports_osc_shell_integration(host_id) {
        cmds.push(PaletteCommand::new(
            "sftp:open_in_sftp",
            t!("terminal.spotlight_open_sftp"),
            t!("terminal.spotlight_open_sftp_desc"),
            Icon::FolderOpen,
            Category::Sftp,
            PaletteCommandKind::OpenInSftp,
        ));
    }

    cmds.push(PaletteCommand::new(
        "terminal:new_tab",
        t!("terminal.spotlight_new_tab"),
        t!("terminal.spotlight_new_tab_desc"),
        Icon::Plus,
        Category::Terminal,
        PaletteCommandKind::NewTab,
    ));
    cmds.push(PaletteCommand::new(
        "terminal:open_local",
        t!("terminal.spotlight_open_local_terminal"),
        t!("terminal.spotlight_open_local_terminal_desc"),
        Icon::Terminal,
        Category::Terminal,
        PaletteCommandKind::OpenLocalTerminal,
    ));
    cmds.push(PaletteCommand::new(
        "terminal:close_tab",
        t!("terminal.spotlight_close_tab"),
        t!("terminal.spotlight_close_tab_desc"),
        Icon::X,
        Category::Terminal,
        PaletteCommandKind::CloseTab,
    ));

    cmds
}

/// Filter and rank `all` by `query` using fuzzy subsequence matching.
///
/// An empty query returns the full list unchanged (fixed category order).  Each
/// command is scored as the best match across its label / description / id;
/// matches are kept and sorted by descending score (stable, so ties preserve
/// registration order).
pub fn filter_commands(all: &[PaletteCommand], query: &str) -> Vec<PaletteCommand> {
    let query = query.trim();
    if query.is_empty() {
        return all.to_vec();
    }

    let mut scored: Vec<(i32, PaletteCommand)> = all
        .iter()
        .filter_map(|c| {
            let score = [&c.label, &c.description, &c.id]
                .iter()
                .filter_map(|target| crate::core::fuzzy::fuzzy_match(query, target))
                .max();
            score.map(|s| (s, c.clone()))
        })
        .collect();

    scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    scored.into_iter().map(|(_, c)| c).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{PaletteCommandKind, build_palette_commands};
    use crate::core::data::HostId;

    fn has_open_in_sftp(host_id: Option<HostId>) -> bool {
        build_palette_commands(host_id)
            .iter()
            .any(|command| matches!(command.kind, PaletteCommandKind::OpenInSftp))
    }

    #[test]
    fn palette_command_ids_are_unique() {
        let commands = build_palette_commands(Some(HostId(1)));
        let ids: HashSet<_> = commands.iter().map(|command| command.id.as_str()).collect();

        assert_eq!(ids.len(), commands.len());
    }

    #[test]
    fn open_in_sftp_is_available_only_when_the_directory_can_be_tracked() {
        assert_eq!(
            has_open_in_sftp(None),
            !cfg!(windows),
            "Windows local terminals cannot supply the OSC 7 working directory"
        );
        assert!(
            has_open_in_sftp(Some(HostId(1))),
            "SSH terminals support the working directory required by Open in SFTP"
        );
    }
}
