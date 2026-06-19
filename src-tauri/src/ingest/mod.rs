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
use crate::scan;
use crate::settings;
use crate::types::{ScanResult, SourceKind, UsageEvent};
use chrono::Utc;
use rayon::prelude::*;
use rusqlite::params;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, warn};
use walkdir::WalkDir;

const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024;
const JSONL_BUF_CAPACITY: usize = 256 * 1024;
/// Skip walking extremely deep trees during directory scans.
const MAX_SCAN_DEPTH: usize = 8;
/// Cap files per scan to avoid runaway imports on huge trees.
const MAX_FILES_PER_SCAN: usize = 10_000;

struct ScanSettings {
    redact: bool,
    store_raw: bool,
}

struct ScanState {
    known_hashes: HashSet<String>,
    file_offsets: HashMap<String, db::FileOffsetState>,
}

#[derive(Clone)]
struct FilePlan {
    path: PathBuf,
    key: String,
    start_offset: u64,
    byte_size: i64,
    file_mtime: Option<i64>,
}

fn load_scan_state(source_id: i64) -> AppResult<ScanState> {
    db::with_conn(|conn| {
        Ok(ScanState {
            known_hashes: db::load_known_event_hashes(conn)?,
            file_offsets: db::load_file_offsets(conn, source_id)?,
        })
    })
}

fn file_key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn file_mtime_millis(metadata: &std::fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|ts| ts.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

fn supports_incremental_reads(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .as_deref(),
        Some("jsonl") | Some("ndjson") | Some("log") | Some("txt")
    )
}

fn select_start_offset(
    path: &Path,
    metadata: &std::fs::Metadata,
    previous: Option<&db::FileOffsetState>,
) -> u64 {
    let current_size = metadata.len();
    let current_mtime = file_mtime_millis(metadata);

    if let Some(prev) = previous {
        if prev.file_mtime == current_mtime && prev.byte_offset == current_size as i64 {
            return current_size;
        }
        if supports_incremental_reads(path)
            && prev.byte_offset > 0
            && current_size as i64 > prev.byte_offset
        {
            return prev.byte_offset as u64;
        }
    }

    0
}

fn build_file_plan(path: &Path, state: &ScanState) -> AppResult<Option<FilePlan>> {
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            debug!("Skipping unreadable file {}: {}", path.display(), e);
            return Ok(None);
        }
    };
    if !metadata.is_file() {
        return Ok(None);
    }

    let size = metadata.len();
    if size > MAX_FILE_SIZE {
        warn!(
            "Skipping {}: too large ({} bytes > {} max)",
            path.display(),
            size,
            MAX_FILE_SIZE
        );
        return Ok(None);
    }

    let key = file_key(path);
    let current_mtime = file_mtime_millis(&metadata);
    let previous = state.file_offsets.get(&key);
    if previous
        .map(|prev| prev.file_mtime == current_mtime && prev.byte_offset == size as i64)
        .unwrap_or(false)
    {
        return Ok(None);
    }

    let start_offset = select_start_offset(path, &metadata, previous);
    Ok(Some(FilePlan {
        path: path.to_path_buf(),
        key,
        start_offset,
        byte_size: size as i64,
        file_mtime: current_mtime,
    }))
}

fn filter_known_events(
    events: Vec<UsageEvent>,
    known_hashes: &mut HashSet<String>,
) -> Vec<UsageEvent> {
    let mut filtered = Vec::with_capacity(events.len());
    for ev in events {
        if known_hashes.insert(ev.event_hash.clone()) {
            filtered.push(ev);
        }
    }
    filtered
}

/// Parse a single file (JSONL, JSON array, or "log" with embedded JSON).
/// Returns a list of normalized events that are not duplicates of each other.
pub fn parse_file(path: &Path, start_offset: u64) -> AppResult<Vec<UsageEvent>> {
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
    if start_offset == 0 && size <= JSONL_BUF_CAPACITY as u64 {
        return parse_file_in_memory(path);
    }

    // Large files and incremental resumes: stream JSONL line-by-line.
    parse_file_streaming(path, start_offset)
}

fn parse_file_in_memory(path: &Path) -> AppResult<Vec<UsageEvent>> {
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
    Ok(out)
}

fn parse_file_streaming(path: &Path, start_offset: u64) -> AppResult<Vec<UsageEvent>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            debug!("Skipping unreadable file {}: {}", path.display(), e);
            return Ok(vec![]);
        }
    };
    let reader = BufReader::with_capacity(JSONL_BUF_CAPACITY, file);
    let mut reader = reader;
    if start_offset > 0 {
        reader
            .seek(SeekFrom::Start(start_offset))
            .map_err(AppError::Io)?;
    }
    let mut out = Vec::new();
    let mut seen_hashes = std::collections::HashSet::new();

    if start_offset > 0 && should_discard_partial_line(path, start_offset)? {
        let mut discarded = String::new();
        if reader.read_line(&mut discarded).map_err(AppError::Io)? == 0 {
            return Ok(out);
        }
    }

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(AppError::Io)?;
        if n == 0 {
            break;
        }
        parse_jsonl_line(&line, path, &mut out, &mut seen_hashes);
    }
    Ok(out)
}

fn should_discard_partial_line(path: &Path, start_offset: u64) -> AppResult<bool> {
    if start_offset == 0 {
        return Ok(false);
    }

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return Err(AppError::Io(e)),
    };
    let prev = start_offset.saturating_sub(1);
    file.seek(SeekFrom::Start(prev)).map_err(AppError::Io)?;
    let mut buf = [0u8; 1];
    let n = file.read(&mut buf).map_err(AppError::Io)?;
    if n == 0 {
        return Ok(false);
    }
    Ok(buf[0] != b'\n' && buf[0] != b'\r')
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

fn process_file(
    path: &Path,
    source_id: i64,
    result: &mut ScanResult,
    scan_settings: &ScanSettings,
    state: &mut ScanState,
) -> AppResult<()> {
    let Some(plan) = build_file_plan(path, state)? else {
        return Ok(());
    };
    let events = parse_file(&plan.path, plan.start_offset)?;
    persist_planned_file(&plan, source_id, result, scan_settings, state, events)
}

fn persist_planned_file(
    plan: &FilePlan,
    source_id: i64,
    result: &mut ScanResult,
    scan_settings: &ScanSettings,
    state: &mut ScanState,
    events: Vec<UsageEvent>,
) -> AppResult<()> {
    result.files_scanned += 1;

    if events.is_empty() {
        db::with_conn_mut(|conn| {
            db::upsert_file_offset(
                conn,
                source_id,
                &plan.key,
                plan.byte_size,
                plan.file_mtime,
            )
        })?;
        state.file_offsets.insert(
            plan.key.clone(),
            db::FileOffsetState {
                byte_offset: plan.byte_size,
                file_mtime: plan.file_mtime,
            },
        );
        return Ok(());
    }

    let original_count = events.len() as i64;
    let mut events = filter_known_events(events, &mut state.known_hashes);
    let filtered_count = events.len() as i64;
    let skipped_duplicates = original_count - filtered_count;

    if events.is_empty() {
        result.events_skipped_duplicate += skipped_duplicates;
        db::with_conn_mut(|conn| {
            db::upsert_file_offset(
                conn,
                source_id,
                &plan.key,
                plan.byte_size,
                plan.file_mtime,
            )
        })?;
        state.file_offsets.insert(
            plan.key.clone(),
            db::FileOffsetState {
                byte_offset: plan.byte_size,
                file_mtime: plan.file_mtime,
            },
        );
        return Ok(());
    }

    post_process_events(&mut events, scan_settings);
    let inserted = persist_events(&events, source_id)?;
    result.events_inserted += inserted;
    result.events_skipped_duplicate += skipped_duplicates + (filtered_count - inserted);

    db::with_conn_mut(|conn| {
        db::upsert_file_offset(
            conn,
            source_id,
            &plan.key,
            plan.byte_size,
            plan.file_mtime,
        )
    })?;
    state.file_offsets.insert(
        plan.key.clone(),
        db::FileOffsetState {
            byte_offset: plan.byte_size,
            file_mtime: plan.file_mtime,
        },
    );

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

    let mut state = load_scan_state(source_id)?;
    let files = collect_scan_files(path);
    if files.is_empty() {
        result.duration_ms = start.elapsed().as_millis() as i64;
        return Ok(result);
    }

    let mut plans = Vec::new();
    for p in files {
        match build_file_plan(&p, &state) {
            Ok(Some(plan)) => plans.push(plan),
            Ok(None) => {}
            Err(e) => result.errors.push(format!("{}: {}", p.display(), e)),
        }
    }

    let parsed: Vec<(FilePlan, AppResult<Vec<UsageEvent>>)> = plans
        .par_iter()
        .map(|plan| (plan.clone(), parse_file(&plan.path, plan.start_offset)))
        .collect();

    for (plan, file_result) in parsed {
        match file_result {
            Ok(events) => {
                if let Err(e) = persist_planned_file(
                    &plan,
                    source_id,
                    &mut result,
                    &scan_settings,
                    &mut state,
                    events,
                ) {
                    result.errors.push(format!("{}: {}", plan.path.display(), e));
                }
            }
            Err(e) => {
                result.errors.push(format!("{}: {}", plan.path.display(), e));
            }
        }
    }
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
    let mut result = ScanResult::default();
    let mut state = load_scan_state(source_id)?;
    process_file(path, source_id, &mut result, &scan_settings, &mut state)?;
    Ok(result.events_inserted as usize)
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
