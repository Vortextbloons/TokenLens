//! OpenCode SQLite database collector.
//!
//! OpenCode stores token usage in `~/.local/share/opencode/opencode.db`, not in
//! the plain-text files under `log/`. This collector reads step-finish parts and
//! message-level usage from the DB (read-only, safe while OpenCode is running).

use crate::errors::{AppError, AppResult};
use crate::ingest::{dedup, persist_events};
use crate::pricing;
use crate::redaction;
use crate::scan::PERSIST_BATCH_SIZE;
use crate::settings;
use crate::types::{Exactness, ScanResult, UsageEvent};
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Default locations for OpenCode's SQLite database(s).
pub fn default_db_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut share_dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        share_dirs.push(home.join(".local").join("share").join("opencode"));
        #[cfg(windows)]
        {
            share_dirs.push(home.join("AppData").join("Local").join("opencode"));
            share_dirs.push(home.join("AppData").join("Roaming").join("opencode"));
        }
    }
    for share in share_dirs {
        let main = share.join("opencode.db");
        if main.is_file() {
            paths.push(main);
        }
        if let Ok(entries) = std::fs::read_dir(&share) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("opencode-") && name.ends_with(".db") {
                    paths.push(entry.path());
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn open_readonly(path: &Path) -> AppResult<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(10))?;
    Ok(conn)
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

/// Scan an OpenCode SQLite database and persist usage events in batches.
pub fn scan_db(path: &Path, source_id: i64) -> AppResult<ScanResult> {
    let start = Instant::now();
    let mut result = ScanResult::default();
    result.files_scanned = 1;

    if !path.is_file() {
        return Err(AppError::NotFound(format!("database not found: {}", path.display())));
    }

    let conn = open_readonly(path)?;
    if table_exists(&conn, "part") {
        load_and_persist_from_parts(&conn, source_id, &mut result)?;
    } else if table_exists(&conn, "messages") {
        load_and_persist_from_messages_v1(&conn, source_id, &mut result)?;
    } else {
        result.errors.push(format!(
            "unrecognized OpenCode schema in {}",
            path.display()
        ));
    }

    result.duration_ms = start.elapsed().as_millis() as i64;
    info!(
        path = %path.display(),
        inserted = result.events_inserted,
        skipped = result.events_skipped_duplicate,
        "OpenCode DB import complete"
    );
    Ok(result)
}

fn flush_db_batch(
    batch: &mut Vec<UsageEvent>,
    source_id: i64,
    result: &mut ScanResult,
) -> AppResult<()> {
    if batch.is_empty() {
        return Ok(());
    }
    apply_privacy_settings(batch)?;
    let n = batch.len() as i64;
    let inserted = persist_events(batch, source_id)?;
    result.events_inserted += inserted;
    result.events_skipped_duplicate += n - inserted;
    batch.clear();
    Ok(())
}

fn apply_privacy_settings(batch: &mut [UsageEvent]) -> AppResult<()> {
    let s = settings::load_all()?;
    for ev in batch.iter_mut() {
        if s.redact_secrets {
            if let Some(raw) = &ev.raw_json {
                ev.raw_json = Some(redaction::redact(raw));
            }
        }
        if !s.store_raw_json {
            ev.raw_json = None;
        }
    }
    Ok(())
}

/// Current OpenCode schema: per-step token accounting in `part.data`.
fn load_and_persist_from_parts(
    conn: &Connection,
    source_id: i64,
    result: &mut ScanResult,
) -> AppResult<()> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.session_id, p.message_id, p.data,
                COALESCE(m.time_created, s.time_created, 0) AS ts,
                m.data AS message_data
         FROM part p
         LEFT JOIN message m ON m.id = p.message_id
         LEFT JOIN session s ON s.id = p.session_id
         WHERE p.data LIKE '%step-finish%'
           AND (json_extract(p.data, '$.type') = 'step-finish'
                OR json_extract(p.data, '$.type') = 'step.finish')",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    let mut batch = Vec::with_capacity(PERSIST_BATCH_SIZE);
    let mut parsed = 0usize;

    for row in rows {
        let (part_id, session_id, message_id, data_str, ts_ms, message_data) = row?;
        let data: Value = match serde_json::from_str(&data_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(mut ev) = part_to_event(&data, &session_id, &part_id, ts_ms) else {
            continue;
        };
        if let Some(md) = message_data.as_deref() {
            enrich_from_message_blob(&mut ev, md);
        }
        ev.raw_source_path = Some(format!("opencode.db:part:{part_id}"));
        if let Some(mid) = message_id {
            if let Some(obj) = ev.raw_json.as_mut().and_then(|s| serde_json::from_str::<Value>(s).ok()) {
                let mut m = obj.as_object().cloned().unwrap_or_default();
                m.insert("__session_id".into(), Value::String(session_id.clone()));
                m.insert("__message_id".into(), Value::String(mid));
                ev.raw_json = Some(serde_json::to_string(&m).unwrap_or_default());
            }
        }
        apply_cost(&mut ev);
        batch.push(ev);
        parsed += 1;
        if batch.len() >= PERSIST_BATCH_SIZE {
            flush_db_batch(&mut batch, source_id, result)?;
        }
    }

    flush_db_batch(&mut batch, source_id, result)?;
    debug!(path = "opencode.db", parsed, "OpenCode DB part scan");
    Ok(())
}

/// Older schema with token columns on `messages`.
fn load_and_persist_from_messages_v1(
    conn: &Connection,
    source_id: i64,
    result: &mut ScanResult,
) -> AppResult<()> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, model, prompt_tokens, completion_tokens,
                COALESCE(created_at, 0) AS ts
         FROM messages
         WHERE COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0) > 0",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;

    let mut batch = Vec::with_capacity(PERSIST_BATCH_SIZE);

    for row in rows {
        let (msg_id, session_id, role, model, input, output, ts) = row?;
        let input = input.unwrap_or(0);
        let output = output.unwrap_or(0);
        let model = model.unwrap_or_else(|| "unknown".into());
        let provider = crate::ingest::normalize::detect_provider(&model);
        let timestamp = ms_to_datetime(ts);
        let total = input + output;
        let hash = dedup::hash_event(
            &timestamp.to_rfc3339(),
            &provider,
            &model,
            &session_id,
            "message",
            total,
            input,
            output,
        );
        let raw = serde_json::json!({
            "__session_id": session_id,
            "__message_id": msg_id,
            "role": role,
            "model": model,
        });
        let mut ev = UsageEvent {
            event_hash: hash,
            timestamp,
            source_id: None,
            session_id: None,
            project_id: None,
            event_type: "message".into(),
            provider: Some(provider.clone()),
            model: Some(model),
            message_role: Some(role),
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            tool_tokens: 0,
            total_tokens: total,
            cost_usd: 0.0,
            exactness: Exactness::Exact,
            confidence: 0.95,
            raw_json: Some(raw.to_string()),
            raw_source_path: Some(format!("opencode.db:message:{msg_id}")),
        };
        apply_cost(&mut ev);
        batch.push(ev);
        if batch.len() >= PERSIST_BATCH_SIZE {
            flush_db_batch(&mut batch, source_id, result)?;
        }
    }

    flush_db_batch(&mut batch, source_id, result)?;
    Ok(())
}

/// Load all step-finish events (used by tests).
fn load_from_parts(conn: &Connection) -> AppResult<Vec<UsageEvent>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.session_id, p.message_id, p.data,
                COALESCE(m.time_created, s.time_created, 0) AS ts,
                m.data AS message_data
         FROM part p
         LEFT JOIN message m ON m.id = p.message_id
         LEFT JOIN session s ON s.id = p.session_id
         WHERE p.data LIKE '%step-finish%'
           AND (json_extract(p.data, '$.type') = 'step-finish'
                OR json_extract(p.data, '$.type') = 'step.finish')",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (part_id, session_id, message_id, data_str, ts_ms, message_data) = row?;
        let data: Value = match serde_json::from_str(&data_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(mut ev) = part_to_event(&data, &session_id, &part_id, ts_ms) else {
            continue;
        };
        if let Some(md) = message_data.as_deref() {
            enrich_from_message_blob(&mut ev, md);
        }
        ev.raw_source_path = Some(format!("opencode.db:part:{part_id}"));
        if let Some(mid) = message_id {
            if let Some(obj) = ev.raw_json.as_mut().and_then(|s| serde_json::from_str::<Value>(s).ok()) {
                let mut m = obj.as_object().cloned().unwrap_or_default();
                m.insert("__session_id".into(), Value::String(session_id.clone()));
                m.insert("__message_id".into(), Value::String(mid));
                ev.raw_json = Some(serde_json::to_string(&m).unwrap_or_default());
            }
        }
        apply_cost(&mut ev);
        out.push(ev);
    }
    Ok(out)
}

/// Older schema with token columns on `messages`.
fn load_from_messages_v1(conn: &Connection) -> AppResult<Vec<UsageEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role, model, prompt_tokens, completion_tokens,
                COALESCE(created_at, 0) AS ts
         FROM messages
         WHERE COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0) > 0",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (msg_id, session_id, role, model, input, output, ts) = row?;
        let input = input.unwrap_or(0);
        let output = output.unwrap_or(0);
        let model = model.unwrap_or_else(|| "unknown".into());
        let provider = crate::ingest::normalize::detect_provider(&model);
        let timestamp = ms_to_datetime(ts);
        let total = input + output;
        let hash = dedup::hash_event(
            &timestamp.to_rfc3339(),
            &provider,
            &model,
            &session_id,
            "message",
            total,
            input,
            output,
        );
        let raw = serde_json::json!({
            "__session_id": session_id,
            "__message_id": msg_id,
            "role": role,
            "model": model,
        });
        let mut ev = UsageEvent {
            event_hash: hash,
            timestamp,
            source_id: None,
            session_id: None,
            project_id: None,
            event_type: "message".into(),
            provider: Some(provider.clone()),
            model: Some(model),
            message_role: Some(role),
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            tool_tokens: 0,
            total_tokens: total,
            cost_usd: 0.0,
            exactness: Exactness::Exact,
            confidence: 0.95,
            raw_json: Some(raw.to_string()),
            raw_source_path: Some(format!("opencode.db:message:{msg_id}")),
        };
        apply_cost(&mut ev);
        out.push(ev);
    }
    Ok(out)
}

fn part_to_event(data: &Value, session_id: &str, part_id: &str, ts_ms: i64) -> Option<UsageEvent> {
    let tokens = data.get("tokens")?;
    let input = int_field(tokens, &["input", "input_tokens", "prompt_tokens"]);
    let output = int_field(tokens, &["output", "output_tokens", "completion_tokens"]);
    let reasoning = int_field(tokens, &["reasoning", "reasoning_tokens"]);
    let cache_read = int_field(
        tokens.get("cache").unwrap_or(tokens),
        &["read", "cache_read", "cache_read_tokens", "cached_tokens"],
    );
    let cache_write = int_field(
        tokens.get("cache").unwrap_or(tokens),
        &["write", "cache_write", "cache_write_tokens"],
    );
    let total = int_field(tokens, &["total", "total_tokens"]).max(input + output + reasoning);
    if total == 0 && input == 0 && output == 0 {
        return None;
    }

    let model = str_field(data, &["modelID", "model_id", "model"])
        .or_else(|| str_field(data.get("model").unwrap_or(data), &["id", "modelID"]))
        .unwrap_or_else(|| "unknown".into());
    let provider = str_field(data, &["providerID", "provider_id", "provider"])
        .unwrap_or_else(|| crate::ingest::normalize::detect_provider(&model));

    let timestamp = if ts_ms > 0 {
        ms_to_datetime(ts_ms)
    } else {
        Utc::now()
    };

    let event_type = str_field(data, &["type"]).unwrap_or_else(|| "step.finish".into());
    let hash = dedup::hash_event(
        &timestamp.to_rfc3339(),
        &provider,
        &model,
        session_id,
        &event_type,
        total,
        input,
        output,
    );

    let mut raw_obj = data.as_object().cloned().unwrap_or_default();
    raw_obj.insert("__session_id".into(), Value::String(session_id.to_string()));
    raw_obj.insert("__part_id".into(), Value::String(part_id.to_string()));

    Some(UsageEvent {
        event_hash: hash,
        timestamp,
        source_id: None,
        session_id: None,
        project_id: None,
        event_type,
        provider: Some(provider),
        model: Some(model),
        message_role: None,
        input_tokens: input,
        output_tokens: output,
        reasoning_tokens: reasoning,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        tool_tokens: 0,
        total_tokens: total,
        cost_usd: 0.0,
        exactness: Exactness::Exact,
        confidence: 0.95,
        raw_json: Some(serde_json::to_string(&Value::Object(raw_obj)).ok()?),
        raw_source_path: None,
    })
}

fn enrich_from_message_blob(ev: &mut UsageEvent, message_data: &str) {
    let Ok(v) = serde_json::from_str::<Value>(message_data) else {
        return;
    };
    if ev.model.as_deref() == Some("unknown") {
        if let Some(m) = str_field(&v, &["modelID", "model_id"]) {
            ev.model = Some(m);
        } else if let Some(m) = v
            .get("model")
            .and_then(|m| str_field(m, &["id", "modelID", "modelID"]))
        {
            ev.model = Some(m);
        }
    }
    if let Some(p) = str_field(&v, &["providerID", "provider_id"]) {
        ev.provider = Some(p);
    } else if let Some(m) = ev.model.as_deref() {
        ev.provider = Some(crate::ingest::normalize::detect_provider(m));
    }
    if ev.message_role.is_none() {
        ev.message_role = str_field(&v, &["role"]);
    }
}

fn apply_cost(ev: &mut UsageEvent) {
    let provider = ev.provider.clone().unwrap_or_default();
    let model = ev.model.clone().unwrap_or_default();
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

fn ms_to_datetime(ms: i64) -> chrono::DateTime<Utc> {
    if ms > 1_000_000_000_000 {
        Utc.timestamp_millis_opt(ms).single().unwrap_or_else(Utc::now)
    } else {
        Utc.timestamp_opt(ms, 0).single().unwrap_or_else(Utc::now)
    }
}

fn int_field(v: &Value, keys: &[&str]) -> i64 {
    for k in keys {
        if let Some(n) = v.get(*k).and_then(|x| x.as_i64()) {
            return n;
        }
    }
    0
}

fn str_field(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_step_finish_json() {
        let data: Value = serde_json::json!({
            "type": "step-finish",
            "modelID": "gpt-4o",
            "providerID": "openai",
            "tokens": { "input": 100, "output": 50, "total": 150 }
        });
        let ev = part_to_event(&data, "ses_abc", "part_1", 1_700_000_000_000).unwrap();
        assert_eq!(ev.input_tokens, 100);
        assert_eq!(ev.output_tokens, 50);
        assert_eq!(ev.model.as_deref(), Some("gpt-4o"));
    }

    #[test]
    #[ignore = "requires local OpenCode install"]
    fn scan_local_opencode_db() {
        let paths = default_db_paths();
        assert!(!paths.is_empty(), "expected opencode.db on this machine");
        let conn = open_readonly(&paths[0]).unwrap();
        let events = load_from_parts(&conn).unwrap();
        eprintln!("parsed {} step-finish events from {}", events.len(), paths[0].display());
        assert!(!events.is_empty(), "expected token events in opencode.db");
    }
}
