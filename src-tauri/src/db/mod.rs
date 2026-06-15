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

const MIGRATION_001: &str = include_str!("../../migrations/001_init.sql");
const MIGRATION_002: &str = include_str!("../../migrations/002_cursor_credentials.sql");
const MIGRATION_003: &str = include_str!("../../migrations/003_compress_raw_json.sql");

struct EmbeddedMigration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const EMBEDDED_MIGRATIONS: &[EmbeddedMigration] = &[
    EmbeddedMigration {
        version: 1,
        name: "001_init.sql",
        sql: MIGRATION_001,
    },
    EmbeddedMigration {
        version: 2,
        name: "002_cursor_credentials.sql",
        sql: MIGRATION_002,
    },
    EmbeddedMigration {
        version: 3,
        name: "003_compress_raw_json.sql",
        sql: MIGRATION_003,
    },
];

fn embedded_sql(version: i64) -> Option<&'static str> {
    EMBEDDED_MIGRATIONS
        .iter()
        .find(|m| m.version == version)
        .map(|m| m.sql)
}

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
    // 8 KB pages are the SQLite default and best for our access pattern
    // (mostly small writes from ingest, occasional full scans for analytics).
    // Larger pages would waste space on the mostly-empty trailing pages of
    // the `usage_events` table after a retention sweep.
    conn.pragma_update(None, "page_size", 8192_i64)?;
    // Negative value means "auto-tune up to N bytes" — leave room for the
    // OS to use the file as anonymous memory without paging.
    conn.pragma_update(None, "mmap_size", 256_i64 * 1024 * 1024)?;
    // Keep the WAL file from ballooning on heavy ingest bursts.
    conn.pragma_update(None, "wal_autocheckpoint", 1000_i64)?;
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

    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |r| r.get(0),
    )?;

    // Migrations are in src-tauri/migrations/*.sql, ordered numerically.
    let migrations_dir = migrations_path();
    if !migrations_dir.exists() {
        warn!(
            "migrations dir not found at {}, using embedded SQL",
            migrations_dir.display()
        );
        apply_embedded_migrations(conn, current)?;
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
        let sql = match std::fs::read_to_string(entry.path()) {
            Ok(s) => s,
            Err(e) => {
                warn!(
                    "failed to read migration {}: {e}, using embedded fallback",
                    name_str
                );
                embedded_sql(version).ok_or_else(|| {
                    AppError::Internal(format!("no embedded migration for version {version}"))
                })?
                .to_string()
            }
        };
        conn.execute_batch(&sql)?;
        info!("Applied migration {}", name_str);
    }
    Ok(())
}

/// Backfill: compress any existing `raw_json` TEXT rows into `raw_json_zstd`
/// BLOB and clear the TEXT column. Runs once after schema v3. This is
/// the recovery path for users who already had a large DB before the
/// compression migration — the next app launch will transparently shrink
/// the file by 5-10× (the typical zstd ratio for raw JSONL). VACUUMs
/// the file at the end so the freed pages actually return to the OS
/// instead of just sitting in SQLite's freelist.
pub fn backfill_compress_raw_json() -> AppResult<()> {
    let did_work = with_conn_mut(|conn| {
        // Quick check: if there are no rows to compress, skip entirely.
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM usage_events
                 WHERE raw_json IS NOT NULL AND raw_json_zstd IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if pending == 0 {
            return Ok(false);
        }
        info!("Backfilling zstd compression for {pending} raw_json rows...");

        // Process in batches so we don't hold a long-running transaction
        // and so progress shows up in the log.
        const BATCH: i64 = 500;
        let mut offset: i64 = 0;
        loop {
            let mut stmt = conn.prepare(
                "SELECT id, raw_json FROM usage_events
                 WHERE raw_json IS NOT NULL AND raw_json_zstd IS NULL
                 ORDER BY id LIMIT ?1 OFFSET ?2",
            )?;
            let mut rows = stmt.query(params![BATCH, offset])?;
            let mut updates: Vec<(i64, Option<Vec<u8>>)> = Vec::new();
            while let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let text: Option<String> = row.get(1)?;
                let blob = text.as_deref().and_then(crate::raw_json_codec::compress);
                updates.push((id, blob));
            }
            drop(rows);
            drop(stmt);
            if updates.is_empty() {
                break;
            }
            let tx = conn.unchecked_transaction()?;
            for (id, blob) in &updates {
                tx.execute(
                    "UPDATE usage_events
                     SET raw_json_zstd = ?1, raw_json = NULL
                     WHERE id = ?2",
                    params![blob, id],
                )?;
            }
            tx.commit()?;
            offset += updates.len() as i64;
            if (updates.len() as i64) < BATCH {
                break;
            }
        }
        info!("raw_json backfill complete");
        Ok(true)
    })?;

    // Only VACUUM if we actually changed rows. VACUUM rewrites the whole
    // DB file and isn't worth the IO when there's nothing to reclaim.
    if did_work {
        info!("Rebuilding DB file to reclaim space from compressed raw_json backfill");
        with_conn(|conn| {
            conn.execute_batch("VACUUM;")?;
            Ok(())
        })?;
    }
    Ok(())
}

/// Build `daily_usage` from `usage_events` if it has never been populated
/// (or was wiped, e.g. after a "Rebuild daily aggregates" that crashed
/// mid-way). The aggregation queries for the Overview page take a fast
/// path through `daily_usage`, so a missing or empty table would silently
/// zero out the rolling-window KPIs. This is intentionally a no-op when
/// the table is non-empty — full rebuilds on every launch would be
/// O(N) over all events.
pub fn ensure_daily_usage_built() -> AppResult<()> {
    with_conn_mut(|conn| {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM daily_usage", [], |r| r.get(0))
            .unwrap_or(0);
        if count > 0 {
            return Ok(());
        }
        let events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM usage_events WHERE ignored = 0",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if events == 0 {
            // Nothing to aggregate. Skip the rebuild; the user has no
            // data yet.
            return Ok(());
        }
        info!("daily_usage is empty; warming it up from {events} events");
        conn.execute_batch(
            "INSERT INTO daily_usage (date, provider, model, project_id, input_tokens, output_tokens,
                reasoning_tokens, cache_read_tokens, cache_write_tokens, total_tokens, cost_usd, sessions_count)
             SELECT date(timestamp), provider, model, project_id,
                    SUM(input_tokens), SUM(output_tokens), SUM(reasoning_tokens),
                    SUM(cache_read_tokens), SUM(cache_write_tokens), SUM(total_tokens), SUM(cost_usd),
                    COUNT(DISTINCT session_id)
             FROM usage_events WHERE ignored = 0
             GROUP BY date(timestamp), provider, model, project_id;",
        )?;
        info!("daily_usage warm-up complete");
        Ok(())
    })
}

fn apply_embedded_migrations(conn: &Connection, current: i64) -> AppResult<()> {
    for m in EMBEDDED_MIGRATIONS {
        if m.version <= current {
            continue;
        }
        conn.execute_batch(m.sql)?;
        info!("Applied embedded migration {}", m.name);
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
