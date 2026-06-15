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
use crate::settings;
use crate::types::{ScanResult, SourceKind, UsageEvent};
use chrono::Utc;
use rusqlite::params;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, warn};
use walkdir::WalkDir;

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

    // Cap file size: 500 MB default.
    const MAX_SIZE: u64 = 500 * 1024 * 1024;
    if size > MAX_SIZE {
        warn!(
            "Skipping {}: too large ({} bytes > {} max)",
            path.display(),
            size,
            MAX_SIZE
        );
        return Ok(vec![]);
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            // Probably binary, skip quietly
            debug!("Skipping non-text file {}: {}", path.display(), e);
            return Ok(vec![]);
        }
    };

    let mut out = Vec::new();
    let mut seen_hashes = std::collections::HashSet::new();

    // Try JSON array first
    if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&raw) {
        for v in arr {
            push_event(&mut out, &mut seen_hashes, v, path);
        }
        return Ok(out);
    }

    // JSONL
    for (lineno, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) => push_event(&mut out, &mut seen_hashes, v, path),
            Err(_) => {
                // Some log lines are not JSON; skip silently.
                let _ = lineno;
            }
        }
    }
    Ok(out)
}

fn push_event(
    out: &mut Vec<UsageEvent>,
    seen: &mut std::collections::HashSet<String>,
    v: Value,
    path: &Path,
) {
    if let Some(mut ev) = normalize::normalize(&v) {
        ev.raw_source_path = Some(path.to_string_lossy().to_string());
        // If raw_json looks like the augmented version, parse the session id out
        if let Some(s) = ev.raw_json.as_deref() {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                if let Some(sid) = parsed.get("__session_id").and_then(|x| x.as_str()) {
                    // We stash the session id in raw_json; the DB layer will pull it out
                    // via a separate field we set on the row. For now, also keep in
                    // raw_json so dedup covers it.
                    let _ = sid;
                }
            }
        }
        // Compute hash
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
    for k in ["sessionID", "session_id", "sessionId", "conversation_id"] {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    String::new()
}

/// Scan a directory recursively and persist all events.
pub fn scan_path(path: &Path, source_id: i64) -> AppResult<ScanResult> {
    let start = Instant::now();
    let mut result = ScanResult::default();

    let s = settings::load_all()?;
    let _estimator_mode = s.token_estimation_mode.clone();
    let redact = s.redact_secrets;
    let store_raw = s.store_raw_json;

    let mut all_events: Vec<UsageEvent> = Vec::new();
    for entry in WalkDir::new(path)
        .max_depth(8)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if !is_supported_extension(p) {
            continue;
        }
        match parse_file(p) {
            Ok(mut events) => {
                result.files_scanned += 1;
                for ev in &mut events {
                    // Optional: estimate missing tokens (not yet implemented for v1)
                    if ev.total_tokens == 0 {
                        let _ = _estimator_mode.clone();
                    }
                    // Compute cost
                    let (provider, model) = (
                        ev.provider.clone().unwrap_or_default(),
                        ev.model.clone().unwrap_or_default(),
                    );
                    if !provider.is_empty() && !model.is_empty() {
                        ev.cost_usd = pricing::compute_cost(
                            &provider,
                            &model,
                            ev.input_tokens,
                            ev.output_tokens,
                            ev.reasoning_tokens,
                            ev.cache_read_tokens,
                            ev.cache_write_tokens,
                        );
                    }
                    // Redact
                    if redact {
                        if let Some(s) = &ev.raw_json {
                            ev.raw_json = Some(redaction::redact(s));
                        }
                    }
                    if !store_raw {
                        ev.raw_json = None;
                    }
                }
                all_events.extend(events);
            }
            Err(e) => {
                result.errors.push(format!("{}: {}", p.display(), e));
            }
        }
    }

    // Persist
    let inserted = persist_events(&all_events, source_id)?;
    result.events_inserted = inserted;
    result.events_skipped_duplicate = (all_events.len() as i64) - inserted;
    result.duration_ms = start.elapsed().as_millis() as i64;

    // Touch the source timestamp / clear error
    let _ = db::with_conn_mut(|conn| {
        db::update_source_scanned(conn, source_id).unwrap_or(());
        Ok::<(), AppError>(())
    });

    Ok(result)
}

fn is_supported_extension(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()).as_deref(),
        Some("jsonl") | Some("json") | Some("log") | Some("ndjson") | Some("txt")
    )
}

/// Persist a batch of events. Returns the number actually inserted (dedup happens at DB level).
pub fn persist_events(events: &[UsageEvent], source_id: i64) -> AppResult<i64> {
    if events.is_empty() {
        return Ok(0);
    }
    let now = Utc::now().to_rfc3339();
    db::with_conn_mut(|conn| {
        let tx = conn.unchecked_transaction()?;
        let mut inserted = 0i64;

        // Session cache: (source_id, source_session_id) -> session_id
        let mut session_cache: std::collections::HashMap<(i64, String), i64> =
            std::collections::HashMap::new();

        for ev in events {
            // Upsert session
            let session_db_id = if let Some(sid) = extract_session_id_from_event(ev) {
                let key = (source_id, sid.clone());
                if let Some(id) = session_cache.get(&key) {
                    Some(*id)
                } else {
                    // Insert or get
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

            // Try insert; skip on duplicate
            let r = tx.execute(
                "INSERT OR IGNORE INTO usage_events
                  (event_hash, timestamp, source_id, session_id, event_type, provider, model,
                   message_role, input_tokens, output_tokens, reasoning_tokens,
                   cache_read_tokens, cache_write_tokens, tool_tokens, total_tokens,
                   cost_usd, exactness, confidence, raw_json, raw_source_path, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21)",
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
                    ev.raw_json,
                    ev.raw_source_path,
                    now,
                ],
            )?;
            if r > 0 {
                inserted += 1;
                // Update session totals
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
    let res = scan_path(path, source_id);
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
    let mut events = parse_file(path)?;
    if events.is_empty() {
        return Ok(0);
    }
    for ev in &mut events {
        let (provider, model) = (
            ev.provider.clone().unwrap_or_default(),
            ev.model.clone().unwrap_or_default(),
        );
        if !provider.is_empty() && !model.is_empty() {
            ev.cost_usd = pricing::compute_cost(
                &provider,
                &model,
                ev.input_tokens,
                ev.output_tokens,
                ev.reasoning_tokens,
                ev.cache_read_tokens,
                ev.cache_write_tokens,
            );
        }
    }
    let inserted = persist_events(&events, source_id)?;
    Ok(inserted as usize)
}

/// Async wrapper for the watcher. Currently the underlying work is sync
/// (rusqlite), but the signature is async to match Tauri command ergonomics.
pub async fn parse_and_persist_file(
    path: &Path,
    source_id: i64,
    kind: SourceKind,
) -> AppResult<usize> {
    parse_and_persist_file_sync(path, source_id, kind)
}

/// Default OpenCode log directories per platform.
pub fn default_opencode_log_paths() -> Vec<PathBuf> {    let mut paths = Vec::new();
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
