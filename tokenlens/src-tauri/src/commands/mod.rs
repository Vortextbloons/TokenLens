//! Tauri command surface. Every frontend call lands here.

use crate::aggregation;
use crate::collectors;
use crate::db;
use crate::errors::{AppError, AppResult};
use crate::ingest;
use crate::pricing;
use crate::settings::{self, AppSettings};
use crate::types::{
    Breakdown, ModelPricing, OverviewStats, QueryFilter, ScanResult, Session, Source,
    TimeseriesPoint, UsageEvent,
};
use crate::watcher;
use chrono::Utc;
use rusqlite::params;
use std::path::PathBuf;

// ----------------- Settings -----------------

#[tauri::command]
pub fn get_settings() -> AppResult<AppSettings> {
    settings::load_all()
}

#[tauri::command]
pub fn update_settings(s: AppSettings) -> AppResult<AppSettings> {
    settings::save_all(&s)?;
    Ok(s)
}

// ----------------- Sources -----------------

#[tauri::command]
pub fn get_sources() -> AppResult<Vec<Source>> {
    db::with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, path, enabled, last_scanned_at, last_error, created_at
             FROM sources ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Source {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    path: r.get(3)?,
                    enabled: r.get::<_, i64>(4)? != 0,
                    last_scanned_at: r.get(5)?,
                    last_error: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn add_source(name: String, kind: String, path: String) -> AppResult<Source> {
    let kind_parsed: crate::types::SourceKind = kind
        .parse()
        .map_err(|()| AppError::Invalid(format!("unknown source kind: {kind}")))?;
    let id = db::upsert_source(&name, kind_parsed, Some(&path))?;
    let src = db::with_conn(|conn| {
        Ok(conn.query_row(
            "SELECT id, name, kind, path, enabled, last_scanned_at, last_error, created_at FROM sources WHERE id = ?1",
            params![id],
            |r| {
                Ok(Source {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    path: r.get(3)?,
                    enabled: r.get::<_, i64>(4)? != 0,
                    last_scanned_at: r.get(5)?,
                    last_error: r.get(6)?,
                    created_at: r.get(7)?,
                })
            },
        )?)
    })?;
    Ok(src)
}

#[tauri::command]
pub async fn remove_source(id: i64) -> AppResult<()> {
    // Best effort: also stop any watcher
    watcher::stop(id).await;
    db::with_conn_mut(|conn| {
        conn.execute("DELETE FROM file_offsets WHERE source_id = ?1", params![id])?;
        conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        Ok(())
    })
}

#[tauri::command]
pub fn scan_source(id: i64) -> AppResult<ScanResult> {
    let path = db::with_conn(|conn| {
        let p: Option<String> = conn.query_row(
            "SELECT path FROM sources WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(p)
    })?;
    let path = path.ok_or_else(|| AppError::NotFound("source has no path".into()))?;
    let p = PathBuf::from(path);
    let kind = db::with_conn(|conn| {
        let k: String = conn.query_row(
            "SELECT kind FROM sources WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(k)
    })?;
    let kind_parsed: crate::types::SourceKind = kind
        .parse()
        .map_err(|()| AppError::Invalid(format!("unknown source kind: {kind}")))?;
    ingest::scan_source_kind(kind_parsed, &p)
}

#[tauri::command]
pub async fn start_watcher(id: i64) -> AppResult<()> {
    let path = db::with_conn(|conn| {
        let p: Option<String> = conn.query_row(
            "SELECT path FROM sources WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(p)
    })?;
    let path = path.ok_or_else(|| AppError::NotFound("source has no path".into()))?;
    watcher::start(id, PathBuf::from(path)).await
}

#[tauri::command]
pub async fn stop_watcher(id: i64) -> AppResult<()> {
    watcher::stop(id).await;
    Ok(())
}

#[tauri::command]
pub async fn list_watchers() -> AppResult<Vec<(i64, String)>> {
    let v = watcher::list_active().await;
    Ok(v.into_iter().map(|(id, p)| (id, p.to_string_lossy().to_string())).collect())
}

#[tauri::command]
pub fn scan_inbox() -> AppResult<ScanResult> {
    collectors::scan_inbox()
}

#[tauri::command]
pub fn discover_default_sources() -> AppResult<Vec<Source>> {
    let paths = ingest::default_opencode_log_paths();
    let mut created = Vec::new();
    for p in paths.iter().filter(|p| p.exists()) {
        let name = format!("OpenCode: {}", p.display());
        let id = db::upsert_source(&name, crate::types::SourceKind::OpencodeLogs, p.to_str())?;
        let src = db::with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT id, name, kind, path, enabled, last_scanned_at, last_error, created_at FROM sources WHERE id = ?1",
                params![id],
                |r| {
                    Ok(Source {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        kind: r.get(2)?,
                        path: r.get(3)?,
                        enabled: r.get::<_, i64>(4)? != 0,
                        last_scanned_at: r.get(5)?,
                        last_error: r.get(6)?,
                        created_at: r.get(7)?,
                    })
                },
            )?)
        })?;
        created.push(src);
    }
    Ok(created)
}

// ----------------- Analytics -----------------

#[tauri::command]
pub fn get_overview_stats(filter: QueryFilter) -> AppResult<OverviewStats> {
    aggregation::overview(&filter)
}

#[tauri::command]
pub fn get_usage_timeseries(filter: QueryFilter) -> AppResult<Vec<TimeseriesPoint>> {
    aggregation::timeseries(&filter)
}

#[tauri::command]
pub fn get_sessions(filter: QueryFilter) -> AppResult<Vec<Session>> {
    aggregation::list_sessions(&filter)
}

#[tauri::command]
pub fn get_session_detail(id: i64) -> AppResult<Option<Session>> {
    aggregation::session_detail(id)
}

#[tauri::command]
pub fn get_session_events(id: i64) -> AppResult<Vec<UsageEvent>> {
    aggregation::session_events(id)
}

#[tauri::command]
pub fn get_breakdown(filter: QueryFilter, dimension: String) -> AppResult<Vec<Breakdown>> {
    aggregation::breakdown_by(&filter, &dimension)
}

#[tauri::command]
pub fn list_events(filter: QueryFilter) -> AppResult<Vec<UsageEvent>> {
    aggregation::list_events(&filter)
}

#[tauri::command]
pub fn count_events() -> AppResult<i64> {
    aggregation::count_events()
}

// ----------------- Pricing -----------------

#[tauri::command]
pub fn list_pricing() -> AppResult<Vec<ModelPricing>> {
    pricing::list_all()
}

#[tauri::command]
pub fn upsert_pricing(p: ModelPricing) -> AppResult<i64> {
    pricing::upsert(&p)
}

#[tauri::command]
pub fn delete_pricing(provider: String, model: String) -> AppResult<()> {
    pricing::delete(&provider, &model)
}

#[tauri::command]
pub fn recalculate_costs() -> AppResult<i64> {
    pricing::recalculate_all()
}

// ----------------- Cleanup -----------------

#[tauri::command]
pub fn cleanup_raw_events(days: i64) -> AppResult<i64> {
    db::with_conn_mut(|conn| {
        let n = conn.execute(
            "DELETE FROM usage_events WHERE created_at < datetime('now', ?1)",
            params![format!("-{days} days")],
        )?;
        Ok(n as i64)
    })
}

#[tauri::command]
pub fn vacuum_db() -> AppResult<()> {
    db::with_conn(|conn| {
        conn.execute_batch("VACUUM;")?;
        Ok(())
    })
}

#[tauri::command]
pub fn rebuild_daily_aggregates() -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute_batch(
            "DELETE FROM daily_usage;
             INSERT INTO daily_usage (date, provider, model, project_id, input_tokens, output_tokens,
                reasoning_tokens, cache_read_tokens, cache_write_tokens, total_tokens, cost_usd, sessions_count)
             SELECT date(timestamp), provider, model, project_id,
                    SUM(input_tokens), SUM(output_tokens), SUM(reasoning_tokens),
                    SUM(cache_read_tokens), SUM(cache_write_tokens), SUM(total_tokens), SUM(cost_usd),
                    COUNT(DISTINCT session_id)
             FROM usage_events WHERE ignored = 0
             GROUP BY date(timestamp), provider, model, project_id;",
        )?;
        Ok(())
    })
}

#[tauri::command]
pub fn reset_all_data() -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute_batch(
            "DELETE FROM usage_events; DELETE FROM sessions; DELETE FROM daily_usage;
             DELETE FROM alerts; DELETE FROM file_offsets; DELETE FROM inbox_files;
             DELETE FROM projects; DELETE FROM pricing_history;",
        )?;
        Ok(())
    })
}

#[tauri::command]
pub fn db_size_mb() -> AppResult<f64> {
    Ok(aggregation::db_size_bytes()? as f64 / (1024.0 * 1024.0))
}

// ----------------- Exports -----------------

#[tauri::command]
pub fn export_csv(filter: QueryFilter, out_path: String) -> AppResult<i64> {
    let events = aggregation::list_events(&filter)?;
    let mut wtr = csv::Writer::from_path(&out_path)?;
    wtr.write_record([
        "id",
        "timestamp",
        "provider",
        "model",
        "session_id",
        "event_type",
        "input_tokens",
        "output_tokens",
        "reasoning_tokens",
        "cache_read_tokens",
        "cache_write_tokens",
        "total_tokens",
        "cost_usd",
        "exactness",
    ])?;
    for e in &events {
        wtr.write_record([
            e.event_hash.as_str(),
            &e.timestamp.to_rfc3339(),
            e.provider.as_deref().unwrap_or(""),
            e.model.as_deref().unwrap_or(""),
            &e.session_id.map(|x| x.to_string()).unwrap_or_default(),
            &e.event_type,
            &e.input_tokens.to_string(),
            &e.output_tokens.to_string(),
            &e.reasoning_tokens.to_string(),
            &e.cache_read_tokens.to_string(),
            &e.cache_write_tokens.to_string(),
            &e.total_tokens.to_string(),
            &format!("{:.6}", e.cost_usd),
            e.exactness.as_str(),
        ])?;
    }
    wtr.flush()?;
    Ok(events.len() as i64)
}

#[tauri::command]
pub fn export_json(filter: QueryFilter, out_path: String) -> AppResult<i64> {
    let events = aggregation::list_events(&filter)?;
    let json = serde_json::to_string_pretty(&events)?;
    std::fs::write(&out_path, json)?;
    Ok(events.len() as i64)
}

#[tauri::command]
pub fn backup_db(out_path: String) -> AppResult<()> {
    // Copy the live DB file safely. SQLite is in WAL mode, so also copy the
    // -wal and -shm sidecar files to ensure a consistent snapshot.
    let src = crate::db_path();
    std::fs::copy(&src, &out_path)?;
    let wal_src = {
        let mut p = src.clone();
        let mut s = p.file_name().unwrap().to_os_string();
        s.push("-wal");
        p.set_file_name(s);
        p
    };
    let shm_src = {
        let mut p = src.clone();
        let mut s = p.file_name().unwrap().to_os_string();
        s.push("-shm");
        p.set_file_name(s);
        p
    };
    if wal_src.exists() {
        let mut wal_dst = std::path::PathBuf::from(&out_path);
        let mut s = wal_dst.file_name().unwrap().to_os_string();
        s.push("-wal");
        wal_dst.set_file_name(s);
        let _ = std::fs::copy(&wal_src, &wal_dst);
    }
    if shm_src.exists() {
        let mut shm_dst = std::path::PathBuf::from(&out_path);
        let mut s = shm_dst.file_name().unwrap().to_os_string();
        s.push("-shm");
        shm_dst.set_file_name(s);
        let _ = std::fs::copy(&shm_src, &shm_dst);
    }
    Ok(())
}

// ----------------- Sample data -----------------

#[tauri::command]
pub fn generate_sample_data() -> AppResult<i64> {
    use crate::types::Exactness;
    use crate::ingest::dedup;
    use chrono::Duration;

    let now = Utc::now();
    let mut events: Vec<UsageEvent> = Vec::new();

    // 14 days of mixed data
    for day in 0..14 {
        let date = now - Duration::days(day);
        // 3-8 sessions per day
        let session_count = 3 + (day % 6);
        for s in 0..session_count {
            let provider = ["openai", "anthropic", "google", "local"][(day + s) as usize % 4];
            let model = match provider {
                "openai" => ["gpt-4o", "gpt-4o-mini", "o1-mini", "gpt-4.1"][s as usize % 4],
                "anthropic" => ["claude-sonnet-4-5", "claude-haiku-4"][s as usize % 2],
                "google" => ["gemini-2.5-pro", "gemini-2.5-flash"][s as usize % 2],
                _ => ["llama-3.1-8b", "qwen2.5-7b"][s as usize % 2],
            };
            // Each session has 5-15 messages
            let msg_count = 5 + (s % 11);
            for m in 0..msg_count {
                let ts = date - Duration::minutes((msg_count - m) as i64 * 2);
                let input = 200 + ((m * 137 + day * 53 + s * 11) % 1500) as i64;
                let output = 100 + ((m * 73 + day * 17 + s * 7) % 800) as i64;
                let reasoning = if model.contains("o1") || model.contains("o3") || model.contains("o4") {
                    (output as f64 * 0.4) as i64
                } else { 0 };
                let cache = if model.contains("gpt-4o") || model.contains("claude") {
                    ((input as f64) * 0.6) as i64
                } else { 0 };
                let total = input + output;
                let ev = UsageEvent {
                    event_hash: dedup::hash_event(
                        &ts.to_rfc3339(),
                        provider,
                        model,
                        &format!("sess-{day}-{s}"),
                        "message",
                        total, input, output,
                    ),
                    timestamp: ts,
                    source_id: None,
                    session_id: None,
                    project_id: None,
                    event_type: "message".to_string(),
                    provider: Some(provider.to_string()),
                    model: Some(model.to_string()),
                    message_role: Some(if m % 2 == 0 { "user".into() } else { "assistant".into() }),
                    input_tokens: input,
                    output_tokens: output,
                    reasoning_tokens: reasoning,
                    cache_read_tokens: cache,
                    cache_write_tokens: 0,
                    tool_tokens: 0,
                    total_tokens: total,
                    cost_usd: 0.0,
                    exactness: Exactness::Exact,
                    confidence: 0.95,
                    raw_json: Some(format!(
                        r#"{{"sessionID":"sess-{day}-{s}","modelID":"{model}","providerID":"{provider}","type":"message"}}"#
                    )),
                    raw_source_path: None,
                };
                events.push(ev);
            }
        }
    }

    // Create a source row for samples
    let source_id = db::upsert_source(
        "Sample Data (built-in)",
        crate::types::SourceKind::Manual,
        Some("<built-in>"),
    )?;

    let inserted = crate::ingest::persist_events(&events, source_id)?;
    Ok(inserted)
}
