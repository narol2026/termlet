//! Workspace offline export/import: versioned JSON payload.
//!
//! Unlike host export, workspace snapshots carry no secrets (host ids, labels,
//! working directories), so the payload is plain JSON with no passphrase.
//!
//! Field policy:
//! - `id` is not exported; imported workspaces get fresh ids from the caller.
//! - `updated_at` is preserved so the import keeps the original sort order
//!   intact.

use serde::{Deserialize, Serialize};

use crate::workspace::{SavedWorkspace, WorkspaceSnapshot};

pub const WORKSPACE_EXPORT_VERSION: u32 = 1;
pub const WORKSPACE_EXPORT_FILE_NAME: &str = "termlet-workspaces.json";
pub const WORKSPACE_EXPORT_FILTER_NAME: &str = "JSON";
pub const WORKSPACE_EXPORT_EXTENSION: &str = "json";

/// A workspace as it appears in an export file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportWorkspace {
    pub name: String,
    pub updated_at: u64,
    pub snapshot: WorkspaceSnapshot,
}

/// Versioned envelope so the schema can evolve without breaking importers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportFile {
    pub version: u32,
    pub exported_at: String,
    pub workspace_count: usize,
    pub workspaces: Vec<ExportWorkspace>,
}

/// Serialize a workspace list into pretty JSON bytes.
pub fn workspaces_to_export_json(workspaces: &[SavedWorkspace]) -> Result<Vec<u8>, String> {
    let list = workspaces
        .iter()
        .map(|w| ExportWorkspace {
            name: w.name.clone(),
            updated_at: w.updated_at,
            snapshot: w.snapshot.clone(),
        })
        .collect::<Vec<_>>();

    let file = ExportFile {
        version: WORKSPACE_EXPORT_VERSION,
        exported_at: now_timestamp(),
        workspace_count: list.len(),
        workspaces: list,
    };

    serde_json::to_vec_pretty(&file).map_err(|e| format!("failed to serialize export: {e}"))
}

/// Parse export JSON and validate it.
pub fn parse_export_json(text: &str) -> Result<Vec<ExportWorkspace>, String> {
    let file: ExportFile =
        serde_json::from_str(text).map_err(|e| format!("invalid export file: {e}"))?;

    if file.version != WORKSPACE_EXPORT_VERSION {
        return Err(format!(
            "unsupported export version {} (expected {WORKSPACE_EXPORT_VERSION})",
            file.version
        ));
    }

    if file.workspace_count != file.workspaces.len() {
        return Err(format!(
            "export workspace_count ({}) does not match the number of workspaces ({})",
            file.workspace_count,
            file.workspaces.len()
        ));
    }

    for w in &file.workspaces {
        if w.name.trim().is_empty() {
            return Err("export contains a workspace with an empty name".to_string());
        }
    }

    Ok(file.workspaces)
}

fn now_timestamp() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{SnapshotSession, SnapshotSessionKind, SnapshotTab};

    fn sample_saved() -> SavedWorkspace {
        SavedWorkspace {
            id: "6b20d9a5-5e50-4c4e-8e10-3c1e7f6f2b1a".to_string(),
            name: "dev".to_string(),
            updated_at: 1_700_000_000,
            snapshot: WorkspaceSnapshot {
                version: 1,
                tabs: vec![SnapshotTab::Session { session: 0 }],
                active_tab: Some(0),
                sessions: vec![SnapshotSession {
                    kind: SnapshotSessionKind::Local {
                        start_dir: Some("/tmp".to_string()),
                    },
                    label_override: None,
                }],
                splits: vec![],
            },
        }
    }

    #[test]
    fn export_omits_id_and_round_trips() {
        let json = workspaces_to_export_json(&[sample_saved()]).unwrap();
        let text = String::from_utf8(json).unwrap();
        assert!(!text.contains("\"id\""), "id field must not be present");
        assert!(text.contains("dev"));

        let parsed = parse_export_json(&text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "dev");
        assert_eq!(parsed[0].updated_at, 1_700_000_000);
    }

    #[test]
    fn parse_rejects_wrong_version() {
        let err = parse_export_json(
            r#"{"version":99,"exported_at":"","workspace_count":0,"workspaces":[]}"#,
        )
        .unwrap_err();
        assert!(err.contains("version"));
    }

    #[test]
    fn parse_rejects_empty_name() {
        let err = parse_export_json(
            r#"{"version":1,"exported_at":"","workspace_count":1,"workspaces":[{"name":"  ","updated_at":1,"snapshot":{"version":1,"tabs":[],"active_tab":null,"sessions":[],"splits":[]}}]}"#,
        )
        .unwrap_err();
        assert!(err.contains("empty name"));
    }

    #[test]
    fn parse_rejects_count_mismatch() {
        let err = parse_export_json(
            r#"{"version":1,"exported_at":"","workspace_count":5,"workspaces":[]}"#,
        )
        .unwrap_err();
        assert!(err.contains("workspace_count"));
    }
}
