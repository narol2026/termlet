use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, TransactionBehavior, params};
use secrecy::{ExposeSecret, SecretString};

use crate::core::data::{
    AuthType, Host, Identity, KeychainEntry, OsType, PortForward, PortForwardKind, Protocol,
    Snippet, SnippetPackage,
};
use crate::core::diagnostics::Level;
use crate::core::ids::{
    HostId, IdentityId, KeychainEntryId, PortForwardId, SnippetId, SnippetPackageId,
};

// Storage engine
//
// A single long-lived SQLite connection (WAL + busy_timeout) shared behind a
// mutex. Schema changes are versioned with `PRAGMA user_version` and applied
// through the ordered `MIGRATIONS` list, so an already-released database can
// be upgraded in place instead of being deleted and recreated.
//
// V1 is the baseline idempotent schema. It is deliberately idempotent so
// databases created before versioning existed (they have `user_version == 0`
// but already contain every table) can be stamped to V1 without losing rows.
// Every future schema change is a new V2/V3/... migration appended to
// `MIGRATIONS`.
//
// `Storage` is `Send + Sync`, so a later move of writes off the UI thread is a
// drop-in change (all methods take `&self`).

/// Highest schema version known to this build. Keep this in sync with the
/// largest `Migration::version` in `MIGRATIONS`.
const SCHEMA_VERSION: i64 = 2;

/// V1 baseline schema. Every statement is idempotent (`CREATE TABLE IF NOT
/// EXISTS`, `DROP TABLE IF EXISTS`), so a database that predates versioning
/// keeps its rows and is simply stamped to version 1 on first open. Child
/// tables carry their real `FOREIGN KEY`/`ON DELETE` rules from the start, so
/// fresh databases enforce referential integrity without an extra migration.
const SCHEMA_V1: &str = "
DROP TABLE IF EXISTS known_hosts;

CREATE TABLE IF NOT EXISTS hosts (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    hostname TEXT NOT NULL,
    port INTEGER NOT NULL,
    username TEXT NOT NULL,
    auth_type TEXT NOT NULL,
    key_path TEXT,
    password TEXT,
    os_type TEXT,
    tags TEXT NOT NULL DEFAULT '[]',
    identity_id INTEGER
);
CREATE TABLE IF NOT EXISTS recent_connections (
    position INTEGER PRIMARY KEY,
    host_id INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS port_forwards (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    host_id INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    bind_address TEXT NOT NULL,
    listen_port INTEGER NOT NULL,
    dest_host TEXT NOT NULL,
    dest_port INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS snippet_packages (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS snippets (
    id INTEGER PRIMARY KEY,
    description TEXT NOT NULL,
    script TEXT NOT NULL,
    package_id INTEGER REFERENCES snippet_packages(id) ON DELETE SET NULL,
    last_used_at TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS keychain_entries (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    key_type TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    public_key TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS identities (
    id INTEGER PRIMARY KEY,
    label TEXT NOT NULL,
    username TEXT NOT NULL DEFAULT '',
    password TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS preferences (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS entity_counters (
    name TEXT PRIMARY KEY,
    next_id INTEGER NOT NULL
);
";

/// V2 adds the optional single-hop `ProxyJump` relation. Deleting a jump host
/// clears the reference rather than deleting the target host.
const SCHEMA_V2: &str = "
ALTER TABLE hosts ADD COLUMN jump_host_id INTEGER REFERENCES hosts(id) ON DELETE SET NULL;
";

/// An ordered, versioned schema change. `sql` is applied inside a transaction
/// and must not repeat the same DDL twice; the framework guarantees it runs
/// only once per database by stamping `PRAGMA user_version`.
struct Migration {
    version: i64,
    sql: &'static str,
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("failed to inspect {table} schema: {e}"))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("failed to read {table} schema: {e}"))?;
    for name in names {
        if name.map_err(|e| format!("failed to read {table} column: {e}"))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Ordered migrations, lowest version first. Never reorder or edit an already
/// shipped migration; add a new entry instead.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: SCHEMA_V1,
    },
    Migration {
        version: 2,
        sql: SCHEMA_V2,
    },
];

/// Apply pending migrations in order and stamp `PRAGMA user_version`.
///
/// Each migration runs in its own `IMMEDIATE` transaction together with the
/// version bump, so a failure cannot leave the database half-upgraded. A
/// database whose recorded version is newer than `target` is rejected instead
/// of being silently downgraded or partially read.
fn migrate(conn: &mut Connection, migrations: &[Migration], target: i64) -> Result<(), String> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| format!("failed to read database schema version: {e}"))?;

    for migration in migrations {
        if migration.version <= current {
            continue;
        }
        if migration.version > target {
            continue;
        }

        // V2 is an additive column migration. A database that already has the
        // column (for example a pre-versioned database created by a newer
        // build and then stamped back to version 0) must still migrate without
        // replaying `ALTER TABLE ... ADD COLUMN`.
        let already_applied =
            migration.version == 2 && column_exists(conn, "hosts", "jump_host_id")?;

        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| {
                format!(
                    "failed to begin schema migration {}: {e}",
                    migration.version
                )
            })?;
        if !already_applied {
            tx.execute_batch(migration.sql).map_err(|e| {
                format!(
                    "failed to apply schema migration {}: {e}",
                    migration.version
                )
            })?;
        }
        tx.execute_batch(&format!("PRAGMA user_version = {};", migration.version))
            .map_err(|e| format!("failed to record schema version {}: {e}", migration.version))?;
        tx.commit().map_err(|e| {
            format!(
                "failed to commit schema migration {}: {e}",
                migration.version
            )
        })?;
    }

    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| format!("failed to read database schema version: {e}"))?;

    if current > target {
        return Err(format!(
            "database schema version {current} is newer than this build supports (version {target}); refusing to open"
        ));
    }

    Ok(())
}

pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    /// Open (creating if needed) the production database and apply the schema.
    pub fn open() -> Result<Storage, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "open",
            std::time::Duration::from_millis(50),
        );
        Self::open_file(&crate::core::paths::config_dir().join("hosts.sqlite"))
    }

    /// Open a database at an arbitrary path (tests).
    #[cfg(test)]
    pub fn open_at(path: &Path) -> Result<Storage, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "open_at",
            std::time::Duration::from_millis(50),
        );
        Self::open_file(path)
    }

    /// Open an in-memory database (tests, and the degraded-mode fallback when
    /// the on-disk database cannot be opened).
    pub fn open_in_memory() -> Result<Storage, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "open_in_memory",
            std::time::Duration::from_millis(50),
        );
        let mut conn = Connection::open_in_memory()
            .map_err(|e| format!("failed to open in-memory db: {e}"))?;
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;",
        )
        .map_err(|e| format!("failed to configure in-memory db: {e}"))?;
        migrate(&mut conn, MIGRATIONS, SCHEMA_VERSION)?;
        Ok(Storage {
            conn: Mutex::new(conn),
        })
    }

    fn open_file(path: &Path) -> Result<Storage, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create config dir: {e}"))?;
        }
        let mut conn = Connection::open(path)
            .map_err(|e| format!("failed to open hosts db {}: {e}", path.display()))?;
        crate::core::paths::restrict_file_permissions(path)?;
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
        .map_err(|e| format!("failed to configure hosts db: {e}"))?;
        migrate(&mut conn, MIGRATIONS, SCHEMA_VERSION)?;
        crate::core::paths::restrict_sqlite_sidecars(path)?;
        Ok(Storage {
            conn: Mutex::new(conn),
        })
    }

    /// Lock the connection and run `f` against it.
    fn with_conn<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|_error| "storage connection poisoned".to_string())?;
        f(&mut conn)
    }

    /// Allocate the next persisted entity id from a durable `entity_counters`
    /// row under an `IMMEDIATE` transaction.
    ///
    /// The counter is seeded from `MAX(id) + 1` once and advanced in place on
    /// every subsequent allocation, so two concurrently-running Termlet
    /// processes cannot hand out the same id even if neither has inserted its
    /// row yet. A crash can leave a gap but never a collision; gaps are
    /// harmless for opaque entity identifiers.
    fn next_id_for(
        &self,
        table: &str,
        id_column: &str,
        fallback_start: u32,
    ) -> Result<u32, String> {
        self.with_conn(|conn| {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|e| format!("failed to start id allocation transaction: {e}"))?;
            let next = match tx.query_row(
                "SELECT next_id FROM entity_counters WHERE name = ?1",
                params![table],
                |row| row.get::<_, u32>(0),
            ) {
                Ok(next) => {
                    tx.execute(
                        "UPDATE entity_counters SET next_id = next_id + 1 WHERE name = ?1",
                        params![table],
                    )
                    .map_err(|e| format!("failed to advance id counter for {table}: {e}"))?;
                    next
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    let next = tx
                        .query_row(
                            &format!(
                                "SELECT CASE WHEN MAX({id_column}) IS NULL THEN ?1 ELSE MAX({id_column}) + 1 END FROM {table}"
                            ),
                            params![fallback_start],
                            |row| row.get::<_, u32>(0),
                        )
                        .map_err(|e| format!("failed to seed id counter for {table}: {e}"))?;
                    tx.execute(
                        "INSERT INTO entity_counters (name, next_id) VALUES (?1, ?2)",
                        params![table, next.saturating_add(1)],
                    )
                    .map_err(|e| format!("failed to seed id counter for {table}: {e}"))?;
                    next
                }
                Err(e) => {
                    return Err(format!("failed to read id counter for {table}: {e}"));
                }
            };
            tx.commit()
                .map_err(|e| format!("failed to commit id allocation: {e}"))?;
            Ok(next)
        })
    }

    /// Allocate the next host id. Host ids historically start at 100.
    pub fn next_host_id(&self) -> Result<HostId, String> {
        self.next_id_for("hosts", "id", 100).map(HostId)
    }

    pub fn next_snippet_id(&self) -> Result<SnippetId, String> {
        self.next_id_for("snippets", "id", 1).map(SnippetId)
    }

    pub fn next_snippet_package_id(&self) -> Result<SnippetPackageId, String> {
        self.next_id_for("snippet_packages", "id", 1)
            .map(SnippetPackageId)
    }

    pub fn next_port_forward_id(&self) -> Result<PortForwardId, String> {
        self.next_id_for("port_forwards", "id", 1)
            .map(PortForwardId)
    }

    pub fn next_keychain_entry_id(&self) -> Result<KeychainEntryId, String> {
        self.next_id_for("keychain_entries", "id", 1)
            .map(KeychainEntryId)
    }

    pub fn next_identity_id(&self) -> Result<IdentityId, String> {
        self.next_id_for("identities", "id", 1).map(IdentityId)
    }

    pub fn load_hosts(&self) -> Result<Vec<Host>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_hosts",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, hostname, port, username, auth_type, key_path, password, os_type, tags, identity_id, jump_host_id \
                     FROM hosts ORDER BY id DESC",
                )
                .map_err(|e| format!("failed to prepare host query: {e}"))?;
            let rows = stmt
                .query_map([], host_from_row)
                .map_err(|e| format!("failed to query hosts: {e}"))?;
            let mut hosts = Vec::new();
            for row in rows {
                hosts.push(row.map_err(|e| format!("failed to parse host row: {e}"))?);
            }
            Ok(hosts)
        })
    }

    /// Load a single host by id (`None` if absent). Used by the ssh-bridge
    /// subprocess, which needs exactly one host without loading the whole table.
    pub fn load_host(&self, id: HostId) -> Result<Option<Host>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_host",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            match conn.query_row(
                "SELECT id, name, hostname, port, username, auth_type, key_path, password, os_type, tags, identity_id, jump_host_id \
                 FROM hosts WHERE id = ?1",
                params![id],
                host_from_row,
            ) {
                Ok(host) => Ok(Some(host)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(format!("failed to load host {id}: {e}")),
            }
        })
    }

    pub fn upsert_host(&self, host: &Host) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "upsert_host",
            std::time::Duration::from_millis(50),
        );
        let password = match &host.password {
            Some(p) if !p.expose_secret().is_empty() => {
                Some(crate::core::crypto::encrypt(p.expose_secret())?)
            }
            // `None` stays `None`; a present-but-empty secret is stored as an
            // empty string, matching the "no saved password" convention.
            _ => host
                .password
                .as_ref()
                .map(|p| p.expose_secret().to_string()),
        };

        self.with_conn(|conn| {
            conn.execute(
                "
                INSERT INTO hosts (id, name, hostname, port, username, auth_type, key_path, password, os_type, tags, identity_id, jump_host_id)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    hostname = excluded.hostname,
                    port = excluded.port,
                    username = excluded.username,
                    auth_type = excluded.auth_type,
                    key_path = excluded.key_path,
                    password = excluded.password,
                    os_type = excluded.os_type,
                    tags = excluded.tags,
                    identity_id = excluded.identity_id,
                    jump_host_id = excluded.jump_host_id
                ",
                params![
                    host.id,
                    host.name,
                    host.hostname,
                    host.port,
                    host.username,
                    auth_type_to_str(host.auth_type),
                    host.key_path,
                    password,
                    host.os_type.to_str(),
                    tags_to_db(&host.tags),
                    host.identity_id,
                    host.jump_host_id,
                ],
            )
            .map_err(|e| format!("failed to save host {}: {e}", host.id))?;
            Ok(())
        })
    }

    pub fn delete_host(&self, id: HostId) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "delete_host",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute("DELETE FROM hosts WHERE id = ?1", params![id])
                .map_err(|e| format!("failed to delete host {id}: {e}"))?;
            Ok(())
        })
    }

    pub fn load_recent_ids(&self) -> Result<Vec<HostId>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_recent_ids",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT host_id FROM recent_connections ORDER BY position ASC")
                .map_err(|e| format!("failed to prepare recent query: {e}"))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, HostId>(0))
                .map_err(|e| format!("failed to query recent connections: {e}"))?;
            let mut ids = Vec::new();
            for row in rows {
                ids.push(row.map_err(|e| format!("failed to parse recent row: {e}"))?);
            }
            Ok(ids)
        })
    }

    pub fn save_recent_ids(&self, ids: &[HostId]) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "save_recent_ids",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let tx = conn
                .transaction()
                .map_err(|e| format!("failed to begin recent connections transaction: {e}"))?;
            tx.execute("DELETE FROM recent_connections", [])
                .map_err(|e| format!("failed to clear recent connections: {e}"))?;
            for (position, host_id) in ids.iter().enumerate() {
                let position = i64::try_from(position).map_err(|error| {
                    format!("recent connection position exceeds SQLite range: {error}")
                })?;
                tx.execute(
                    "INSERT INTO recent_connections (position, host_id) VALUES (?1, ?2)",
                    params![position, host_id],
                )
                .map_err(|e| format!("failed to save recent connection: {e}"))?;
            }
            tx.commit()
                .map_err(|e| format!("failed to commit recent connections: {e}"))?;
            Ok(())
        })
    }

    pub fn load_port_forwards(&self) -> Result<Vec<PortForward>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_port_forwards",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, kind, host_id, bind_address, listen_port, dest_host, dest_port \
                     FROM port_forwards ORDER BY id ASC",
                )
                .map_err(|e| format!("failed to prepare port forward query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    let kind: String = row.get(2)?;
                    let kind = match kind.as_str() {
                        "Local" => PortForwardKind::Local,
                        "Remote" => PortForwardKind::Remote,
                        "Dynamic" => PortForwardKind::Dynamic,
                        _ => PortForwardKind::Local,
                    };
                    Ok(PortForward {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        kind,
                        host_id: row.get(3)?,
                        bind_address: row.get(4)?,
                        listen_port: row.get(5)?,
                        dest_host: row.get(6)?,
                        dest_port: row.get(7)?,
                    })
                })
                .map_err(|e| format!("failed to query port forwards: {e}"))?;
            let mut forwards = Vec::new();
            for row in rows {
                forwards.push(row.map_err(|e| format!("failed to parse port forward row: {e}"))?);
            }
            Ok(forwards)
        })
    }

    pub fn upsert_port_forward(&self, forward: &PortForward) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "upsert_port_forward",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute(
                "
                INSERT INTO port_forwards (id, name, kind, host_id, bind_address, listen_port, dest_host, dest_port)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    kind = excluded.kind,
                    host_id = excluded.host_id,
                    bind_address = excluded.bind_address,
                    listen_port = excluded.listen_port,
                    dest_host = excluded.dest_host,
                    dest_port = excluded.dest_port
                ",
                params![
                    forward.id,
                    forward.name,
                    port_forward_kind_to_str(forward.kind),
                    forward.host_id,
                    forward.bind_address,
                    forward.listen_port,
                    forward.dest_host,
                    forward.dest_port
                ],
            )
            .map_err(|e| format!("failed to save port forward {}: {e}", forward.id))?;
            Ok(())
        })
    }

    pub fn delete_port_forward(&self, id: PortForwardId) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "delete_port_forward",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute("DELETE FROM port_forwards WHERE id = ?1", params![id])
                .map_err(|e| format!("failed to delete port forward {id}: {e}"))?;
            Ok(())
        })
    }

    pub fn load_snippets(&self) -> Result<Vec<Snippet>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_snippets",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, description, script, package_id, last_used_at FROM snippets \
                     ORDER BY last_used_at DESC, id DESC",
                )
                .map_err(|e| format!("failed to prepare snippet query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Snippet {
                        id: row.get(0)?,
                        description: row.get(1)?,
                        script: row.get(2)?,
                        package_id: row.get(3)?,
                        last_used_at: row.get(4)?,
                    })
                })
                .map_err(|e| format!("failed to query snippets: {e}"))?;
            let mut snippets = Vec::new();
            for row in rows {
                snippets.push(row.map_err(|e| format!("failed to parse snippet row: {e}"))?);
            }
            Ok(snippets)
        })
    }

    pub fn upsert_snippet(&self, snippet: &Snippet) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "upsert_snippet",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute(
                "
                INSERT INTO snippets (id, description, script, package_id, last_used_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(id) DO UPDATE SET
                    description = excluded.description,
                    script = excluded.script,
                    package_id = excluded.package_id,
                    last_used_at = excluded.last_used_at
                ",
                params![
                    snippet.id,
                    snippet.description,
                    snippet.script,
                    snippet.package_id,
                    snippet.last_used_at
                ],
            )
            .map_err(|e| format!("failed to save snippet {}: {e}", snippet.id))?;
            Ok(())
        })
    }

    pub fn delete_snippet(&self, id: SnippetId) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "delete_snippet",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])
                .map_err(|e| format!("failed to delete snippet {id}: {e}"))?;
            Ok(())
        })
    }

    pub fn load_snippet_packages(&self) -> Result<Vec<SnippetPackage>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_snippet_packages",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, name, description FROM snippet_packages ORDER BY id ASC")
                .map_err(|e| format!("failed to prepare package query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(SnippetPackage {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                    })
                })
                .map_err(|e| format!("failed to query packages: {e}"))?;
            let mut packages = Vec::new();
            for row in rows {
                packages.push(row.map_err(|e| format!("failed to parse package row: {e}"))?);
            }
            Ok(packages)
        })
    }

    pub fn upsert_snippet_package(&self, pkg: &SnippetPackage) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "upsert_snippet_package",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute(
                "
                INSERT INTO snippet_packages (id, name, description)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    description = excluded.description
                ",
                params![pkg.id, pkg.name, pkg.description],
            )
            .map_err(|e| format!("failed to save package {}: {e}", pkg.id))?;
            Ok(())
        })
    }

    pub fn delete_snippet_package(&self, id: SnippetPackageId) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "delete_snippet_package",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute("DELETE FROM snippet_packages WHERE id = ?1", params![id])
                .map_err(|e| format!("failed to delete package {id}: {e}"))?;
            Ok(())
        })
    }

    pub fn load_preferences(&self) -> Result<HashMap<String, String>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_preferences",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT key, value FROM preferences")
                .map_err(|e| format!("failed to prepare preferences query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|e| format!("failed to query preferences: {e}"))?;
            let mut map = HashMap::new();
            for row in rows {
                let (key, value) =
                    row.map_err(|e| format!("failed to parse preference row: {e}"))?;
                map.insert(key, value);
            }
            Ok(map)
        })
    }

    /// Load a single preference (`None` when the key is absent).
    ///
    /// This is intentionally narrower than [`Storage::load_preferences`] so
    /// cross-window preference polling can read just the keys it cares about
    /// without deserializing the whole table.
    pub fn load_preference(&self, key: &str) -> Result<Option<String>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_preference",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let value = match conn.query_row(
                "SELECT value FROM preferences WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            ) {
                Ok(value) => Some(value),
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(e) => return Err(format!("failed to load preference {key}: {e}")),
            };

            Ok(value)
        })
    }

    pub fn save_preference(&self, key: &str, value: &str) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "save_preference",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO preferences (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|e| format!("failed to save preference {key}: {e}"))?;
            Ok(())
        })
    }

    pub fn load_keychain_entries(&self) -> Result<Vec<KeychainEntry>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_keychain_entries",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, name, key_type, fingerprint, public_key, path, created_at FROM keychain_entries ORDER BY id ASC",
                )
                .map_err(|e| format!("failed to prepare keychain query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(KeychainEntry {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        key_type: row.get(2)?,
                        fingerprint: row.get(3)?,
                        public_key: row.get(4)?,
                        path: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                })
                .map_err(|e| format!("failed to query keychain: {e}"))?;
            let mut entries = Vec::new();
            for row in rows {
                entries.push(row.map_err(|e| format!("failed to parse keychain row: {e}"))?);
            }
            Ok(entries)
        })
    }

    pub fn upsert_keychain_entry(&self, entry: &KeychainEntry) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "upsert_keychain_entry",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute(
                "
                INSERT INTO keychain_entries (id, name, key_type, fingerprint, public_key, path, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    key_type = excluded.key_type,
                    fingerprint = excluded.fingerprint,
                    public_key = excluded.public_key,
                    path = excluded.path,
                    created_at = excluded.created_at
                ",
                params![
                    entry.id,
                    entry.name,
                    entry.key_type,
                    entry.fingerprint,
                    entry.public_key,
                    entry.path,
                    entry.created_at,
                ],
            )
            .map_err(|e| format!("failed to save keychain entry {}: {e}", entry.id))?;
            Ok(())
        })
    }

    pub fn delete_keychain_entry(&self, id: KeychainEntryId) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "delete_keychain_entry",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute("DELETE FROM keychain_entries WHERE id = ?1", params![id])
                .map_err(|e| format!("failed to delete keychain entry {id}: {e}"))?;
            Ok(())
        })
    }

    pub fn load_identities(&self) -> Result<Vec<Identity>, String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "load_identities",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, label, username, password, created_at FROM identities ORDER BY id ASC",
                )
                .map_err(|e| format!("failed to prepare identities query: {e}"))?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(Identity {
                        id: row.get(0)?,
                        label: row.get(1)?,
                        username: row.get(2)?,
                        password: decrypt_optional(Some(row.get::<_, String>(3)?))
                            .map_err(|e| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    3,
                                    rusqlite::types::Type::Text,
                                    Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        e,
                                    )),
                                )
                            })?
                            .map(SecretString::from)
                            .unwrap_or_default(),
                        created_at: row.get(4)?,
                    })
                })
                .map_err(|e| format!("failed to query identities: {e}"))?;
            let mut entries = Vec::new();
            for row in rows {
                entries.push(row.map_err(|e| format!("failed to parse identity row: {e}"))?);
            }
            Ok(entries)
        })
    }

    pub fn upsert_identity(&self, entry: &Identity) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "upsert_identity",
            std::time::Duration::from_millis(50),
        );
        let password = if entry.password.expose_secret().is_empty() {
            String::new()
        } else {
            crate::core::crypto::encrypt(entry.password.expose_secret())?
        };
        self.with_conn(|conn| {
            conn.execute(
                "
                INSERT INTO identities (id, label, username, password, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(id) DO UPDATE SET
                    label = excluded.label,
                    username = excluded.username,
                    password = excluded.password,
                    created_at = excluded.created_at
                ",
                params![
                    entry.id,
                    entry.label,
                    entry.username,
                    password,
                    entry.created_at,
                ],
            )
            .map_err(|e| format!("failed to save identity {}: {e}", entry.id))?;
            Ok(())
        })
    }

    pub fn delete_identity(&self, id: IdentityId) -> Result<(), String> {
        let _guard = crate::core::diagnostics::perf_guard(
            Level::Debug,
            "STORAGE",
            "delete_identity",
            std::time::Duration::from_millis(50),
        );
        self.with_conn(|conn| {
            conn.execute("DELETE FROM identities WHERE id = ?1", params![id])
                .map_err(|e| format!("failed to delete identity {id}: {e}"))?;
            Ok(())
        })
    }
}

/// Map a `hosts` row into a [`Host`].
fn host_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Host> {
    let auth: String = row.get(5)?;
    let auth_type = match auth.as_str() {
        "Password" => AuthType::Password,
        "Key" => AuthType::Key,
        _ => AuthType::Password,
    };
    let os_str: Option<String> = row.get(8).ok();
    let os_type = os_str.as_deref().map_or(OsType::Unknown, OsType::from_str);
    let tags_json: String = row.get(9)?;
    Ok(Host {
        id: row.get(0)?,
        name: row.get(1)?,
        hostname: row.get(2)?,
        port: row.get(3)?,
        protocol: Protocol::Ssh,
        username: row.get(4)?,
        auth_type,
        jump_host_id: row.get(11)?,
        identity_id: row.get(10)?,
        key_path: row.get(6)?,
        key_passphrase: None,
        password: decrypt_optional(row.get(7)?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                )
            })?
            .map(SecretString::from),
        os_type,
        tags: tags_from_db(&tags_json),
        expected_fingerprint: None,
    })
}

/// Serialize a host's tag list into the DB column value (JSON array).
fn tags_to_db(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string())
}

/// Parse the DB `tags` column (a JSON array) back into a `Vec<String>`.
fn tags_from_db(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn auth_type_to_str(auth_type: AuthType) -> &'static str {
    match auth_type {
        AuthType::Password => "Password",
        AuthType::Key => "Key",
    }
}

fn port_forward_kind_to_str(kind: PortForwardKind) -> &'static str {
    match kind {
        PortForwardKind::Local => "Local",
        PortForwardKind::Remote => "Remote",
        PortForwardKind::Dynamic => "Dynamic",
    }
}

/// Decrypt a stored credential, returning `None` when no value is present.
/// Encryption/decryption errors are propagated to the caller instead of being
/// silently converted into an empty credential.
fn decrypt_optional(secret: Option<String>) -> Result<Option<String>, String> {
    let Some(secret) = secret else {
        return Ok(None);
    };
    if secret.is_empty() {
        return Ok(Some(secret));
    }
    crate::core::crypto::decrypt(&secret).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mem() -> Storage {
        Storage::open_in_memory().unwrap()
    }

    fn test_host(id: HostId) -> Host {
        Host {
            id,
            name: format!("host-{id}"),
            hostname: format!("host-{id}.example.com"),
            port: 22,
            protocol: Protocol::Ssh,
            username: "root".into(),
            auth_type: AuthType::Key,
            jump_host_id: None,
            identity_id: None,
            key_path: None,
            key_passphrase: None,
            password: None,
            os_type: OsType::Linux,
            tags: Vec::new(),
            expected_fingerprint: None,
        }
    }

    fn test_port_forward(id: PortForwardId, host_id: HostId) -> PortForward {
        PortForward {
            id,
            name: format!("forward-{id}"),
            kind: PortForwardKind::Local,
            host_id,
            bind_address: "127.0.0.1".into(),
            listen_port: 8080,
            dest_host: "localhost".into(),
            dest_port: 80,
        }
    }

    fn temp_db(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "termlet_storage_test_{name}_{}.sqlite",
            std::process::id()
        ))
    }

    #[test]
    fn schema_creates_expected_tables_and_columns() {
        let s = mem();
        let conn = s.conn.lock().unwrap();

        // known_hosts (legacy) must be gone.
        let known: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='known_hosts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(known, 0);

        // Every column the app reads must exist in a fresh database.
        let cols = [
            "os_type",
            "package_id",
            "last_used_at",
            "tags",
            "identity_id",
        ];
        for col in cols {
            assert!(
                column_exists(&conn, col),
                "column `{col}` missing from schema"
            );
        }
    }

    fn column_exists(conn: &Connection, name: &str) -> bool {
        // Check the tables that could hold these columns.
        for table in ["hosts", "snippets"] {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            let cols = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
            for c in cols {
                if c.unwrap() == name {
                    return true;
                }
            }
        }
        false
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn schema_is_idempotent_across_reopens() {
        let path = temp_db("idempotent");
        let _ = std::fs::remove_file(&path);
        {
            let s = Storage::open_at(&path).unwrap();
            s.conn
                .lock()
                .unwrap()
                .execute(
                    "INSERT INTO hosts (name, hostname, port, username, auth_type, tags)
                     VALUES ('prod', 'h', 22, 'u', 'Password', '[\"prod\"]')",
                    [],
                )
                .unwrap()
        };
        // Reopening re-applies the idempotent schema: no error, row intact.
        let s = Storage::open_at(&path).unwrap();
        let hosts = s.load_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "prod");
        assert_eq!(hosts[0].tags, vec!["prod"]);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn open_at_restricts_sqlite_sidecars() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_db("wal_permissions");
        let _ = std::fs::remove_file(&path);
        let wal = std::path::PathBuf::from(format!("{}-wal", path.display()));
        let shm = std::path::PathBuf::from(format!("{}-shm", path.display()));
        let _ = std::fs::remove_file(&wal);
        let _ = std::fs::remove_file(&shm);

        {
            let s = Storage::open_at(&path).unwrap();
            s.conn
                .lock()
                .unwrap()
                .execute(
                    "INSERT INTO hosts (name, hostname, port, username, auth_type, tags)
                     VALUES ('prod', 'h', 22, 'u', 'Password', '[]')",
                    [],
                )
                .unwrap();

            for sidecar in [&wal, &shm] {
                let metadata = std::fs::metadata(sidecar).unwrap();
                assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            }
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&wal);
        let _ = std::fs::remove_file(&shm);
    }

    #[test]
    fn fresh_database_records_current_schema_version() {
        let s = mem();
        let conn = s.conn.lock().unwrap();
        assert_eq!(user_version(&conn), SCHEMA_VERSION);
    }

    #[test]
    fn baseline_migration_stamps_a_pre_versioned_database() {
        let path = temp_db("pre_versioned");
        let _ = std::fs::remove_file(&path);

        // Create a normal V1 database and insert a row.
        {
            let s = Storage::open_at(&path).unwrap();
            s.conn
                .lock()
                .unwrap()
                .execute(
                    "INSERT INTO hosts (name, hostname, port, username, auth_type, tags)
                     VALUES ('legacy', 'h', 22, 'u', 'Password', '[]')",
                    [],
                )
                .unwrap()
        };

        // Simulate a database that predates versioning: same tables, but no
        // user_version stamp.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA user_version = 0;").unwrap();
        };

        let s = Storage::open_at(&path).unwrap();
        assert_eq!(user_version(&s.conn.lock().unwrap()), SCHEMA_VERSION);
        assert_eq!(s.load_hosts().unwrap().len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migrate_applies_ordered_deltas_exactly_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        let migrations = &[
            Migration {
                version: 1,
                sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);",
            },
            Migration {
                version: 2,
                sql: "ALTER TABLE t ADD COLUMN label TEXT NOT NULL DEFAULT '';",
            },
        ];

        migrate(&mut conn, migrations, 2).unwrap();
        assert_eq!(user_version(&conn), 2);

        // Re-running the same list is a no-op for already-applied versions.
        migrate(&mut conn, migrations, 2).unwrap();
        assert_eq!(user_version(&conn), 2);

        let label_columns: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('t') WHERE name = 'label'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(label_columns, 1);
    }

    #[test]
    fn newer_database_schema_is_rejected() {
        let path = temp_db("newer");
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        };

        match Storage::open_at(&path) {
            Ok(_) => panic!("expected open to fail for a newer schema version"),
            Err(err) => assert!(
                err.contains("newer than this build supports"),
                "unexpected error: {err}"
            ),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn host_crud_roundtrip_and_secret() {
        crate::core::crypto::with_test_key(|| {
            let s = mem();
            let h = Host {
                id: HostId(1),
                name: "prod".into(),
                hostname: "prod.example.com".into(),
                port: 22,
                protocol: Protocol::Ssh,
                username: "root".into(),
                auth_type: AuthType::Password,
                jump_host_id: None,
                identity_id: None,
                key_path: None,
                key_passphrase: None,
                password: Some(SecretString::from("hunter2".to_string())),
                os_type: OsType::Linux,
                tags: vec!["prod".into(), "eu-west".into()],
                expected_fingerprint: None,
            };
            s.upsert_host(&h).unwrap();

            let loaded = s.load_hosts().unwrap();
            assert_eq!(loaded.len(), 1);
            assert_eq!(
                loaded[0].password.as_ref().unwrap().expose_secret(),
                "hunter2"
            );
            assert_eq!(loaded[0].tags, vec!["prod", "eu-west"]);

            let one = s.load_host(HostId(1)).unwrap().unwrap();
            assert_eq!(one.name, "prod");
            assert_eq!(one.password.as_ref().unwrap().expose_secret(), "hunter2");
            assert_eq!(one.tags, vec!["prod", "eu-west"]);

            // Re-upsert with a different tag set exercises the ON CONFLICT
            // update branch; tags must be rewritten, not merged/stale.
            let mut edited = one.clone();
            edited.tags = vec!["staging".into()];
            s.upsert_host(&edited).unwrap();
            let reloaded = s.load_host(HostId(1)).unwrap().unwrap();
            assert_eq!(reloaded.tags, vec!["staging"]);

            assert!(s.load_host(HostId(99)).unwrap().is_none());

            s.delete_host(HostId(1)).unwrap();
            assert!(s.load_host(HostId(1)).unwrap().is_none());
        });
    }

    #[test]
    fn host_jump_host_id_roundtrip_and_delete_clears_reference() {
        crate::core::crypto::with_test_key(|| {
            let s = mem();
            let mut jump = test_host(HostId(100));
            jump.name = "bastion".into();
            jump.hostname = "bastion.example".into();
            s.upsert_host(&jump).unwrap();

            let mut target = test_host(HostId(101));
            target.name = "app".into();
            target.hostname = "app.internal".into();
            target.jump_host_id = Some(HostId(100));
            s.upsert_host(&target).unwrap();

            let loaded = s.load_host(HostId(101)).unwrap().unwrap();
            assert_eq!(loaded.jump_host_id, Some(HostId(100)));

            s.delete_host(HostId(100)).unwrap();
            let cleared = s.load_host(HostId(101)).unwrap().unwrap();
            assert_eq!(cleared.jump_host_id, None);
        });
    }

    #[test]
    fn recent_ids_replace_all() {
        let s = mem();
        s.upsert_host(&test_host(HostId(1))).unwrap();
        s.upsert_host(&test_host(HostId(2))).unwrap();
        s.save_recent_ids(&[HostId(1), HostId(2)]).unwrap();
        assert_eq!(s.load_recent_ids().unwrap(), vec![HostId(1), HostId(2)]);

        s.save_recent_ids(&[]).unwrap();
        assert!(s.load_recent_ids().unwrap().is_empty());
    }

    #[test]
    fn port_forward_crud_roundtrip() {
        let s = mem();
        s.upsert_host(&test_host(HostId(1))).unwrap();
        let f = PortForward {
            id: PortForwardId(1),
            name: "web".into(),
            kind: PortForwardKind::Local,
            host_id: HostId(1),
            bind_address: "127.0.0.1".into(),
            listen_port: 8080,
            dest_host: "localhost".into(),
            dest_port: 80,
        };
        s.upsert_port_forward(&f).unwrap();
        let loaded = s.load_port_forwards().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "web");
        s.delete_port_forward(PortForwardId(1)).unwrap();
        assert!(s.load_port_forwards().unwrap().is_empty());
    }

    #[test]
    fn delete_host_cascades_to_children() {
        let s = mem();
        s.upsert_host(&test_host(HostId(1))).unwrap();
        s.upsert_host(&test_host(HostId(2))).unwrap();
        s.save_recent_ids(&[HostId(1), HostId(2)]).unwrap();

        for (id, host_id) in [
            (PortForwardId(1), HostId(1)),
            (PortForwardId(2), HostId(1)),
            (PortForwardId(3), HostId(2)),
        ] {
            s.upsert_port_forward(&test_port_forward(id, host_id))
                .unwrap();
        }

        s.delete_host(HostId(1)).unwrap();

        assert_eq!(s.load_recent_ids().unwrap(), vec![HostId(2)]);
        let forwards = s.load_port_forwards().unwrap();
        assert_eq!(forwards.len(), 1);
        assert_eq!(forwards[0].host_id, HostId(2));
    }

    #[test]
    fn foreign_key_rejects_orphan_port_forward() {
        let s = mem();
        let result = s.upsert_port_forward(&test_port_forward(PortForwardId(1), HostId(999)));
        assert!(result.is_err());
    }

    #[test]
    fn snippet_package_delete_unassigns_snippets() {
        let s = mem();
        s.upsert_snippet_package(&SnippetPackage {
            id: SnippetPackageId(1),
            name: "pkg".into(),
            description: String::new(),
        })
        .unwrap();
        s.upsert_snippet(&Snippet {
            id: SnippetId(1),
            description: "s".into(),
            script: "echo hi".into(),
            package_id: Some(SnippetPackageId(1)),
            last_used_at: String::new(),
        })
        .unwrap();

        s.delete_snippet_package(SnippetPackageId(1)).unwrap();
        assert!(s.load_snippet_packages().unwrap().is_empty());
        let snips = s.load_snippets().unwrap();
        assert_eq!(snips.len(), 1);
        assert!(snips[0].package_id.is_none());
    }

    #[test]
    fn keychain_entry_crud_roundtrip() {
        let s = mem();
        let e = KeychainEntry {
            id: KeychainEntryId(1),
            name: "id_ed25519".into(),
            key_type: "ssh-ed25519".into(),
            fingerprint: "SHA256:abc".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            path: "~/.ssh/id_ed25519".into(),
            created_at: String::new(),
        };
        s.upsert_keychain_entry(&e).unwrap();
        assert_eq!(s.load_keychain_entries().unwrap().len(), 1);
        s.delete_keychain_entry(KeychainEntryId(1)).unwrap();
        assert!(s.load_keychain_entries().unwrap().is_empty());
    }

    #[test]
    fn identity_crud_roundtrip_and_secret() {
        crate::core::crypto::with_test_key(|| {
            let s = mem();
            s.upsert_identity(&Identity {
                id: IdentityId(1),
                label: "root".into(),
                username: "root".into(),
                password: SecretString::from("pw".to_string()),
                created_at: String::new(),
            })
            .unwrap();
            let loaded = s.load_identities().unwrap();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].password.expose_secret(), "pw");
            s.delete_identity(IdentityId(1)).unwrap();
            assert!(s.load_identities().unwrap().is_empty());
        });
    }

    #[test]
    fn plain_preference_stays_plain() {
        let s = mem();
        s.save_preference("theme_mode", "dark").unwrap();
        let prefs = s.load_preferences().unwrap();
        assert_eq!(prefs.get("theme_mode").unwrap(), "dark");
    }

    #[test]
    fn load_single_preference() {
        let s = mem();
        assert_eq!(s.load_preference("theme_mode").unwrap(), None);
        s.save_preference("theme_mode", "Dracula").unwrap();
        assert_eq!(
            s.load_preference("theme_mode").unwrap(),
            Some("Dracula".to_string())
        );
    }

    #[test]
    fn next_ids_are_allocated_from_database() {
        let s = mem();

        // Hosts keep their historical 100-based space.
        assert_eq!(s.next_host_id().unwrap(), HostId(100));
        // A second allocation advances even before any row is inserted, which
        // is what prevents two app processes from observing the same max.
        assert_eq!(s.next_host_id().unwrap(), HostId(101));
        s.upsert_host(&Host {
            id: HostId(100),
            name: "first".into(),
            hostname: "host".into(),
            port: 22,
            protocol: Protocol::Ssh,
            username: "user".into(),
            auth_type: AuthType::Password,
            jump_host_id: None,
            identity_id: None,
            key_path: None,
            key_passphrase: None,
            password: None,
            os_type: OsType::Unknown,
            tags: vec![],
            expected_fingerprint: None,
        })
        .unwrap();
        assert_eq!(s.next_host_id().unwrap(), HostId(102));

        // All other persisted entities start at 1 and follow their own tables.
        assert_eq!(s.next_snippet_id().unwrap(), SnippetId(1));
        assert_eq!(s.next_snippet_package_id().unwrap(), SnippetPackageId(1));
        assert_eq!(s.next_port_forward_id().unwrap(), PortForwardId(1));
        assert_eq!(s.next_keychain_entry_id().unwrap(), KeychainEntryId(1));
        assert_eq!(s.next_identity_id().unwrap(), IdentityId(1));
    }
}
