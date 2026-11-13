//! Typed identifiers.
//!
//! Every domain ID is a newtype over `u32` rather than a bare `usize`, so the
//! compiler rejects mixing e.g. a `HostId` where a `SnippetId` is expected, and
//! "identity" stays distinct from "array index / position" — those remain plain
//! `usize` (see [`crate::core::data::KnownHost::line_number`] and the queue
//! indices in [`crate::core::message::Message`]). The IDs are re-exported from
//! [`crate::app`] and [`crate::core::data`] so existing import paths keep
//! working.

use rusqlite::types::{FromSql, ToSql};

/// Generates a copyable, hashable, orderable, `Display`-able newtype over
/// `u32`, plus the `rusqlite` `ToSql`/`FromSql` impls so the storage layer can
/// read/write these IDs as plain INTEGER columns with no conversion at the call
/// site.
macro_rules! id_type {
    ($($(#[$meta:meta])* $t:ident),* $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
            pub struct $t(pub u32);

            impl std::fmt::Display for $t {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{}", self.0)
                }
            }

            impl FromSql for $t {
                fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                    u32::column_result(value).map($t)
                }
            }

            impl ToSql for $t {
                fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                    self.0.to_sql()
                }
            }
        )*
    };
}

// Runtime identifiers (terminal sessions, connection drafts, splits, tabs).
id_type!(
    /// Stable identifier for a terminal session.
    SessionId,
    /// Stable identifier for a new-tab / SSH connection draft.
    DraftId,
    /// Stable identifier for a split group.
    SplitId,
    /// Stable identifier for a dynamic tab in the topbar.
    TabId,
);

// Persisted entity identifiers.
id_type!(
    /// Stable identifier for an SSH host.
    HostId,
    /// Stable identifier for a code snippet.
    SnippetId,
    /// Stable identifier for a snippet package (group of snippets).
    SnippetPackageId,
    /// Stable identifier for a port-forward rule.
    PortForwardId,
    /// Stable identifier for a keychain entry.
    KeychainEntryId,
    /// Stable identifier for an SSH identity.
    IdentityId,
);

// SFTP sessions use their own id space, distinct from terminal `SessionId`.
id_type!(
    /// Stable identifier for an SFTP session (separate space from `SessionId`).
    SftpSessionId,
);
