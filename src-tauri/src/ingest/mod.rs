//! Ingest pipeline: scan files, parse lines, normalize, dedup, persist.
//!
//! `scan_path` walks a directory, reads JSON/JSONL/log files, and pipes every
//! line that parses as JSON through the normalizer. Dedup is enforced by the
//! `event_hash` unique constraint at the DB layer.

pub mod dedup;
pub mod normalize;

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::pricing;
use crate::redaction;
use crate::scan::{self, PERSIST_BATCH_SIZE};
use crate::settings;
use crate::types::{ScanResult, SourceKind, UsageEvent};
use chrono::Utc;
use rayon::prelude::*;
use rusqlite::params;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, warn};
use walkdir::WalkDir;

const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024;
const JSONL_BUF_CAPACITY: usize = 256 * 1024;
/// Skip walking extremely deep trees during directory scans.
const MAX_SCAN_DEPTH: usize = 8;
/// Cap files per scan to avoid runaway imports on huge trees.
/// Cap files per scan to avoid runaway imports on huge trees.
const MAX_FILES_PER_SCAN: usize = 10_000;
/// Process this many files in parallel before flushing to SQLite.
const PARALLEL_FILE_CHUNK: usize = 64;

struct ScanSettings {
    redact: bool,
    store_raw: bool,
}

/// Parse a single file (JSONL, JSON array, or "log" with embedded JSON).
/// Returns a list of normalized events that are not duplicates of each other.
pub fn parse_file(path: &Path) -> AppResult<Vec<UsageEvent>> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return Err(AppError::Io(e)),
    };
    if !metadata.is_file() {
        return Ok(vec![]);
    }
    let size = metadata.len();

    if size > MAX_FILE_SIZE {
        warn!(
            "Skipping {}: too large ({} bytes > {} max)",
            path.display(),
            size,
            MAX_FILE_SIZE
        );
        return Ok(vec![]);
    }

    // Small files: read whole buffer once (faster for tiny JSON arrays).
    if size <= JSONL_BUF_CAPACITY as u64 {
        return parse_file_in_memory(path, size);
    }

    // Large files: stream JSONL line-by-line to avoid loading hundreds of MB.
    parse_file_streaming(path)
}

fn parse_file_in_memory(path: &Path, size: u64) -> AppResult<Vec<UsageEvent>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            debug!("Skipping non-text file {}: {}", path.display(), e);
            return Ok(vec![]);
        }
    };

    let mut out = Vec::new();
    let mut seen_hashes = std::collections::HashSet::new();

    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&raw) {
        for v in arr {
            push_event(&mut out, &mut seen_hashes, v, path);
        }
        return Ok(out);
    }

    for line in raw.lines() {
        parse_jsonl_line(line, path, &mut out, &mut seen_hashes);
    }
    let _ = size;
    Ok(out)
}

fn parse_file_streaming(path: &Path) -> AppResult<Vec<UsageEvent>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            debug!("Skipping unreadable file {}: {}", path.display(), e);
            return Ok(vec![]);
        }
    };
    let reader = BufReader::with_capacity(JSONL_BUF_CAPACITY, file);
    let mut out = Vec::new();
    let mut seen_hashes = std::collections::HashSet::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => return Err(AppError::Io(e)),
        };
        parse_jsonl_line(&line, path, &mut out, &mut seen_hashes);
    }
    Ok(out)
}

fn parse_jsonl_line(
    line: &str,
    path: &Path,
    out: &mut Vec<UsageEvent>,
    seen_hashes: &mut std::collections::HashSet<String>,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if let Ok(v) = serde_json::from_str::<Value>(line) {
        push_event(out, seen_hashes, v, path);
    }
}

fn push_event(
    out: &mut Vec<UsageEvent>,
    seen: &mut std::collections::HashSet<String>,
    v: Value,
    path: &Path,
) {
    if let Some(mut ev) = normalize::normalize(&v) {
        ev.raw_source_path = Some(path.to_string_lossy().to_string());
        if let Some(s) = ev.raw_json.as_deref() {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                if let Some(sid) = parsed.get("__session_id").and_then(|x| x.as_str()) {
                    let _ = sid;
                }
            }
        }
        let hash = dedup::hash_event(
            &ev.timestamp.to_rfc3339(),
            ev.provider.as_deref().unwrap_or(""),
            ev.model.as_deref().unwrap_or(""),
            &extract_session_id(&v),
            &ev.event_type,
            ev.total_tokens,
            ev.input_tokens,
            ev.output_tokens,
        );
        if seen.insert(hash.clone()) {
            ev.event_hash = hash;
            out.push(ev);
        }
    }
}

fn extract_session_id(v: &Value) -> String {
    session_id_from_value(v).or_else(|| {
        v.get("info").and_then(session_id_from_value)
    }).unwrap_or_default()
}

fn session_id_from_value(v: &Value) -> Option<String> {
    for k in ["sessionID", "session_id", "sessionId", "conversation_id", "__session_id"] {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

fn collect_scan_files(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in WalkDir::new(path)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if is_supported_extension(p) {
            files.push(p.to_path_buf());
            if files.len() >= MAX_FILES_PER_SCAN {
                warn!(
                    "Scan hit file cap ({}); remaining files in {} skipped",
                    MAX_FILES_PER_SCAN,
                    path.display()
                );
                break;
            }
        }
    }
    files
}

fn post_process_events(events: &mut [UsageEvent], settings: &ScanSettings) {
    for ev in events.iter_mut() {
        let (provider, model) = (
            ev.provider.clone().unwrap_or_default(),
            ev.model.clone().unwrap_or_default(),
        );
        if !provider.is_empty() && !model.is_empty() {
            let breakdown = pricing::compute_cost_breakdown(
                &provider,
                &model,
                ev.input_tokens,
                ev.output_tokens,
                ev.reasoning_tokens,
                ev.cache_read_tokens,
                ev.cache_write_tokens,
            );
            ev.cost_usd = breakdown.cost_usd;
            if matches!(breakdown.status, pricing::CostStatus::Estimated) {
                ev.exactness = crate::types::Exactness::Estimated;
            }
        }
        if settings.redact {
            if let Some(s) = &ev.raw_json {
                ev.raw_json = Some(redaction::redact(s));
            }
        }
        if !settings.store_raw {
            ev.raw_json = None;
        }
    }
}

fn flush_batch(
    batch: &mut Vec<UsageEvent>,
    source_id: i64,
    result: &mut ScanResult,
) -> AppResult<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let batch_len = batch.len() as i64;
    let inserted = persist_events(batch, source_id)?;
    result.events_inserted += inserted;
    result.events_skipped_duplicate += batch_len - inserted;
    batch.clear();
    Ok(())
}

fn push_into_batch(
    batch: &mut Vec<UsageEvent>,
    mut events: Vec<UsageEvent>,
    source_id: i64,
    result: &mut ScanResult,
) -> AppResult<()> {
    for ev in events.drain(..) {
        batch.push(ev);
        if batch.len() >= PERSIST_BATCH_SIZE {
            flush_batch(batch, source_id, result)?;
        }
    }
    Ok(())
}

/// Scan a directory recursively and persist all events.
pub fn scan_path(path: &Path, source_id: i64) -> AppResult<ScanResult> {
    let start = Instant::now();
    let mut result = ScanResult::default();

    let s = settings::load_all()?;
    let scan_settings = ScanSettings {
        redact: s.redact_secrets,
        store_raw: s.store_raw_json,
    };

    let files = collect_scan_files(path);
    if files.is_empty() {
        result.duration_ms = start.elapsed().as_millis() as i64;
        return Ok(result);
    }

    let mut batch: Vec<UsageEvent> = Vec::with_capacity(PERSIST_BATCH_SIZE);

    for file_chunk in files.chunks(PARALLEL_FILE_CHUNK) {
        let parsed: Vec<(PathBuf, AppResult<Vec<UsageEvent>>)> = file_chunk
            .par_iter()
            .map(|p| {
                let parsed = parse_file(p);
                (p.clone(), parsed)
            })
            .collect();

        for (p, file_result) in parsed {
            match file_result {
                Ok(mut events) => {
                    result.files_scanned += 1;
                    post_process_events(&mut events, &scan_settings);
                    push_into_batch(&mut batch, events, source_id, &mut result)?;
                }
                Err(e) => {
                    result.errors.push(format!("{}: {}", p.display(), e));
                }
            }
        }
    }

    flush_batch(&mut batch, source_id, &mut result)?;
    result.duration_ms = start.elapsed().as_millis() as i64;

    let _ = db::with_conn_mut(|conn| {
        db::update_source_scanned(conn, source_id).unwrap_or(());
        Ok::<(), AppError>(())
    });

    Ok(result)
}

fn is_supported_extension(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).as_deref(),
        Some("jsonl") | Some("json") | Some("log") | Some("ndjson") | Some("txt") | Some("csv")
    )
}

fn scan_cursor_csv(path: &Path, _source_id: i64) -> AppResult<ScanResult> {
    let start = Instant::now();
    let mut result = ScanResult::default();
    result.files_scanned = 1;
    match crate::collectors::cursor::import_csv_file(path) {
        Ok(n) => {
            result.events_inserted = n as i64;
            result.duration_ms = start.elapsed().as_millis() as i64;
            Ok(result)
        }
        Err(e) => {
            result.errors.push(format!("{}: {}", path.display(), e));
            result.duration_ms = start.elapsed().as_millis() as i64;
            Ok(result)
        }
    }
}

/// Persist a batch of events. Returns the number actually inserted (dedup happens at DB level).
pub fn persist_events(events: &[UsageEvent], source_id: i64) -> AppResult<i64> {
    if events.is_empty() {
        return Ok(0);
    }
    let now = Utc::now().to_rfc3339();
    // Re-read settings here so all call sites (scan path, sample data,
    // cursor sync, opencode_db) honour the user's current toggle without
    // needing to thread a parameter through.
    let store_raw = settings::load_all().map(|s| s.store_raw_json).unwrap_or(true);
    db::with_conn_mut(|conn| {
        let tx = conn.unchecked_transaction()?;
        let mut inserted = 0i64;

        let mut session_cache: std::collections::HashMap<(i64, String), i64> =
            std::collections::HashMap::new();

        for ev in events {
            let session_db_id = if let Some(sid) = extract_session_id_from_event(ev) {
                let key = (source_id, sid.clone());
                if let Some(id) = session_cache.get(&key) {
                    Some(*id)
                } else {
                    tx.execute(
                        "INSERT OR IGNORE INTO sessions
                            (source_session_id, source_id, provider, model, started_at, last_seen_at, exactness)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
                        params![
                            sid,
                            source_id,
                            ev.provider,
                            ev.model,
                            ev.timestamp.to_rfc3339(),
                            ev.exactness.as_str(),
                        ],
                    )?;
                    let id: i64 = tx.query_row(
                        "SELECT id FROM sessions WHERE source_id = ?1 AND source_session_id = ?2",
                        params![source_id, sid],
                        |r| r.get(0),
                    )?;
                    session_cache.insert(key, id);
                    Some(id)
                }
            } else {
                None
            };

            // Only persist raw_json when the user opted in. New writes
            // (schema v3+) store a zstd-compressed BLOB; the legacy TEXT
            // column is left NULL to avoid duplicating the payload across
            // two columns. The read path in `aggregation::decode_raw_json`
            // transparently decompresses the BLOB and falls back to TEXT
            // for rows written before this migration.
            let raw_blob: Option<Vec<u8>> = if store_raw {
                ev.raw_json
                    .as_deref()
                    .and_then(crate::raw_json_codec::compress)
            } else {
                None
            };
            let raw_text: Option<String> = None;
            let r = tx.execute(
                "INSERT OR IGNORE INTO usage_events
                  (event_hash, timestamp, source_id, session_id, event_type, provider, model,
                   message_role, input_tokens, output_tokens, reasoning_tokens,
                   cache_read_tokens, cache_write_tokens, tool_tokens, total_tokens,
                   cost_usd, exactness, confidence, raw_json, raw_json_zstd,
                   raw_source_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    ev.event_hash,
                    ev.timestamp.to_rfc3339(),
                    source_id,
                    session_db_id,
                    ev.event_type,
                    ev.provider,
                    ev.model,
                    ev.message_role,
                    ev.input_tokens,
                    ev.output_tokens,
                    ev.reasoning_tokens,
                    ev.cache_read_tokens,
                    ev.cache_write_tokens,
                    ev.tool_tokens,
                    ev.total_tokens,
                    ev.cost_usd,
                    ev.exactness.as_str(),
                    ev.confidence,
                    raw_text,
                    raw_blob,
                    ev.raw_source_path,
                    now,
                ],
            )?;
            if r > 0 {
                inserted += 1;
                if let Some(sid) = session_db_id {
                    tx.execute(
                        "UPDATE sessions SET
                            last_seen_at = MAX(IFNULL(last_seen_at, ''), ?1),
                            total_tokens = total_tokens + ?2,
                            total_cost_usd = total_cost_usd + ?3,
                            provider = COALESCE(provider, ?4),
                            model = COALESCE(model, ?5)
                         WHERE id = ?6",
                        params![
                            ev.timestamp.to_rfc3339(),
                            ev.total_tokens,
                            ev.cost_usd,
                            ev.provider,
                            ev.model,
                            sid,
                        ],
                    )?;
                }
            }
        }

        // Keep daily_usage in sync: find the calendar dates affected by the
        // newly inserted events and rebuild their aggregated rows so the
        // rolling-window KPIs (today / week / month) are always up-to-date.
        if inserted > 0 {
            let hashes_json = serde_json::to_string(
                &events.iter().map(|e| &e.event_hash).collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| "[]".into());
            let affected_dates: Vec<String> = {
                let mut stmt = tx.prepare(
                    "SELECT DISTINCT date(timestamp) FROM usage_events \
                     WHERE event_hash IN (SELECT value FROM json_each(?1))",
                )?;
                let dates: Vec<String> = stmt
                    .query_map(params![hashes_json], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                dates
            };
            for date in &affected_dates {
                tx.execute(
                    "DELETE FROM daily_usage WHERE date = ?1",
                    params![date],
                )?;
                tx.execute(
                    "INSERT INTO daily_usage \
                     (date, provider, model, project_id, input_tokens, output_tokens, \
                      reasoning_tokens, cache_read_tokens, cache_write_tokens, total_tokens, \
                      cost_usd, sessions_count) \
                     SELECT date(timestamp), provider, model, project_id, \
                            SUM(input_tokens), SUM(output_tokens), SUM(reasoning_tokens), \
                            SUM(cache_read_tokens), SUM(cache_write_tokens), SUM(total_tokens), \
                            SUM(cost_usd), COUNT(DISTINCT session_id) \
                     FROM usage_events \
                     WHERE ignored = 0 AND date(timestamp) = ?1 \
                     GROUP BY date(timestamp), provider, model, project_id",
                    params![date],
                )?;
            }
        }

        tx.commit()?;
        Ok(inserted)
    })
}

fn extract_session_id_from_event(ev: &UsageEvent) -> Option<String> {
    let raw_str = ev.raw_json.as_deref()?;
    let v: Value = serde_json::from_str(raw_str).ok()?;
    for k in ["__session_id", "sessionID", "session_id", "sessionId", "conversation_id"] {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

/// Find the source id for a given kind+path (creating if missing), then scan.
pub fn scan_source_kind(kind: SourceKind, path: &Path) -> AppResult<ScanResult> {
    let name = format!("{}: {}", kind.as_str(), path.display());
    let source_id = db::upsert_source(&name, kind, Some(path.to_str().unwrap_or("")))?;
    let res = if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("db") {
        crate::collectors::opencode_db::scan_db(path, source_id)
    } else if kind == SourceKind::Cursor
        && path.is_file()
        && path.extension().and_then(|s| s.to_str()) == Some("csv")
    {
        scan_cursor_csv(path, source_id)
    } else {
        scan_path(path, source_id)
    };
    match &res {
        Ok(r) => {
            let _ = db::with_conn_mut(|conn| {
                if r.errors.is_empty() {
                    let _ = db::update_source_scanned(conn, source_id);
                } else {
                    let _ = db::update_source_error(conn, source_id, &r.errors.join("; "));
                }
                Ok::<(), AppError>(())
            });
        }
        Err(e) => {
            let _ = db::with_conn_mut(|conn| {
                let _ = db::update_source_error(conn, source_id, &e.to_string());
                Ok::<(), AppError>(())
            });
        }
    }
    res
}

/// Parse a file and persist all events. Synchronous version used by the
/// scanner, inbox collector, and other non-watcher code paths.
pub fn parse_and_persist_file_sync(
    path: &Path,
    source_id: i64,
    _kind: SourceKind,
) -> AppResult<usize> {
    let s = settings::load_all()?;
    let scan_settings = ScanSettings {
        redact: s.redact_secrets,
        store_raw: s.store_raw_json,
    };
    let mut events = parse_file(path)?;
    if events.is_empty() {
        return Ok(0);
    }
    post_process_events(&mut events, &scan_settings);
    let inserted = persist_events(&events, source_id)?;
    Ok(inserted as usize)
}

/// Async wrapper for the watcher. Offloads sync SQLite work to a blocking thread.
pub async fn parse_and_persist_file(
    path: &Path,
    source_id: i64,
    kind: SourceKind,
) -> AppResult<usize> {
    let path = path.to_path_buf();
    scan::run_blocking(move || parse_and_persist_file_sync(&path, source_id, kind)).await
}

/// Default OpenCode log directories per platform.
pub fn default_opencode_log_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        #[cfg(windows)]
        {
            paths.push(home.join(r".local\share\opencode\log"));
            paths.push(home.join("AppData").join("Local").join("opencode").join("log"));
            paths.push(home.join("AppData").join("Roaming").join("opencode").join("log"));
        }
        #[cfg(target_os = "macos")]
        {
            paths.push(home.join(".local/share/opencode/log"));
            paths.push(home.join("Library/Logs/opencode"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            paths.push(home.join(".local/share/opencode/log"));
        }
    }
    paths
}

/// OpenCode stores usage data in SQLite, not the text log folder.
pub fn default_opencode_db_paths() -> Vec<PathBuf> {
    crate::collectors::opencode_db::default_db_paths()
}
