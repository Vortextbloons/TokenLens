//! SQLite database wrapper using rusqlite (bundled).
//!
//! Single writer connection guarded by a Mutex. WAL mode is enabled in the
//! migration. The DB file lives under the app's local data dir.

use crate::errors::{AppError, AppResult};
use crate::types::SourceKind;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

static DB: OnceCell<Arc<Mutex<Connection>>> = OnceCell::new();

/// Initialize the DB. Must be called once at app startup.
pub fn init(db_path: &Path) -> AppResult<()> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    run_migrations(&conn)?;
    DB.set(Arc::new(Mutex::new(conn)))
        .map_err(|_| AppError::Internal("DB already initialized".into()))?;
    info!(path = %db_path.display(), "Database initialized");
    Ok(())
}

/// Access the shared DB connection. Panics if init was not called.
pub fn with_conn<F, R>(f: F) -> AppResult<R>
where
    F: FnOnce(&Connection) -> AppResult<R>,
{
    let arc = DB.get().ok_or_else(|| AppError::Internal("DB not initialized".into()))?;
    let guard = arc.lock();
    f(&guard)
}

/// Mutate the shared DB connection. Held for the duration of `f`.
pub fn with_conn_mut<F, R>(f: F) -> AppResult<R>
where
    F: FnOnce(&mut Connection) -> AppResult<R>,
{
    let arc = DB.get().ok_or_else(|| AppError::Internal("DB not initialized".into()))?;
    let mut guard = arc.lock();
    f(&mut guard)
}

fn run_migrations(conn: &Connection) -> AppResult<()> {
    // 1. Ensure schema_version exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;

    let current: Option<i64> = conn
        .query_row(
            "SELECT MAX(version) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let current = current.unwrap_or(0);

    // Migrations are in src-tauri/migrations/*.sql, ordered numerically.
    let migrations_dir = migrations_path();
    if !migrations_dir.exists() {
        warn!(
            "migrations dir not found at {}, skipping",
            migrations_dir.display()
        );
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&migrations_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s == "sql")
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        // File name like "001_init.sql"
        let version: i64 = name_str
            .split('_')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if version <= current {
            continue;
        }
        let sql = std::fs::read_to_string(entry.path())?;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(&sql)?;
        tx.commit()?;
        info!("Applied migration {}", name_str);
    }
    Ok(())
}

fn migrations_path() -> PathBuf {
    // In dev (cargo run from src-tauri/), CWD is src-tauri/.
    // In production, the binary's exe dir is used.
    // We try a few likely locations.
    let candidates = [
        PathBuf::from("migrations"),
        PathBuf::from("../migrations"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("migrations")))
            .unwrap_or_default(),
    ];
    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    PathBuf::from("migrations")
}

// ---------------------- Helper query builders ----------------------

pub fn upsert_source(
    name: &str,
    kind: SourceKind,
    path: Option<&str>,
) -> AppResult<i64> {
    with_conn_mut(|conn| {
        // Try to find existing source by name+kind
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM sources WHERE name = ?1 AND kind = ?2",
                params![name, kind.as_str()],
                |r| r.get(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO sources (name, kind, path, enabled, created_at) VALUES (?1, ?2, ?3, 1, datetime('now'))",
            params![name, kind.as_str(), path],
        )?;
        Ok(conn.last_insert_rowid())
    })
}

pub fn update_source_scanned(conn: &Connection, id: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE sources SET last_scanned_at = datetime('now'), last_error = NULL WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn update_source_error(conn: &Connection, id: i64, err: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE sources SET last_error = ?1 WHERE id = ?2",
        params![err, id],
    )?;
    Ok(())
}
