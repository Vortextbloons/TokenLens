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
pub fn update_settings(app: tauri::AppHandle, s: AppSettings) -> AppResult<AppSettings> {
    // Save the rest of the settings first so a flaky autostart toggle can
    // never block unrelated changes (e.g. token estimation mode).
    settings::save_all(&s)?;

    // The autostart plugin walks the Windows registry and writes the path
    // of the current executable. In `tauri dev` or a relocated build, that
    // path can be invalid and the call returns an io::Error ("The system
    // cannot find the file specified."). Rather than fail the whole save,
    // we log a warning and continue — the autostart toggle can be retried
    // by flipping the switch again once the build is in a stable location.
    sync_autostart(&app, s.autostart);

    Ok(s)
}

/// Best-effort sync of the system autostart entry with the user's setting.
/// Logs and swallows errors so callers can still succeed.
fn sync_autostart(app: &tauri::AppHandle, enable: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let result = if enable {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    if let Err(e) = result {
        tracing::warn!(
            "autostart {} failed (settings saved anyway): {}",
            if enable { "enable" } else { "disable" },
            e
        );
    }
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
    watcher::stop(id).await;
    db::with_conn_mut(|conn| {
        delete_events_for_sources(conn, &[id])?;
        delete_sessions_for_sources(conn, &[id]);
        conn.execute("DELETE FROM file_offsets WHERE source_id = ?1", params![id])?;
        conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        Ok(())
    })
}

#[tauri::command]
pub async fn scan_source(id: i64) -> AppResult<ScanResult> {
    crate::scan::run_exclusive_blocking(move || scan_source_sync(id)).await
}

fn scan_source_sync(id: i64) -> AppResult<ScanResult> {
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
    // Reuse existing source row id when scanning a registered path.
    if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("db") {
        return crate::collectors::opencode_db::scan_db(&p, id);
    }
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
pub async fn scan_inbox() -> AppResult<ScanResult> {
    crate::scan::run_exclusive_blocking(collectors::scan_inbox).await
}

#[tauri::command]
pub fn discover_default_sources() -> AppResult<Vec<Source>> {
    let mut created = Vec::new();
    // Prefer OpenCode SQLite DB (actual token usage lives here).
    for p in ingest::default_opencode_db_paths().iter().filter(|p| p.exists()) {
        let name = format!("OpenCode DB: {}", p.display());
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
    let paths = ingest::default_opencode_log_paths();
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
pub fn sync_pricing_seed() -> AppResult<i64> {
    pricing::sync_seed_rows()
}

#[tauri::command]
pub async fn recalculate_costs() -> AppResult<i64> {
    crate::scan::run_exclusive_blocking(pricing::recalculate_all).await
}

/// Re-run token estimation on events that have no exact token count. Uses
/// the active `token_estimation_mode` setting (`chars4` or `tiktoken`).
/// Returns the number of events whose `total_tokens` was updated.
#[tauri::command]
pub async fn recalculate_token_estimates() -> AppResult<i64> {
    crate::scan::run_exclusive_blocking(|| {
        let mode = settings::load_all()?.token_estimation_mode;
        crate::token_estimator::recalculate_unknown_events(&mode)
    })
    .await
}

/// Bulk import a list of pricing rows produced by an external AI research
/// workflow (see `docs/pricing-research-preset.md`).
///
/// Each row goes through `pricing::upsert` so the existing pricing_history
/// audit log is preserved. Empty / blank rows are skipped. After import the
/// caller is expected to run `recalculate_costs` to refresh event costs.
#[tauri::command]
pub fn import_pricing_json(rows: Vec<ModelPricing>) -> AppResult<pricing::BulkImportSummary> {
    pricing::bulk_upsert(&rows)
}

/// Export the full `model_pricing` table as a JSON array. Used by the pricing
/// research workflow so the AI can see what's already configured and avoid
/// suggesting duplicates.
#[tauri::command]
pub fn export_pricing() -> AppResult<Vec<ModelPricing>> {
    pricing::list_all()
}

/// List `(provider, model)` pairs that appear in `usage_events` but have no
/// matching row in `model_pricing`. Sorted by total tokens (impact) desc.
///
/// This is the headless equivalent of running the "missing pricing" SQL
/// query in the docs; the UI calls it instead of shelling out to sqlite3.
#[tauri::command]
pub fn list_missing_pricing() -> AppResult<Vec<pricing::MissingPricingRow>> {
    pricing::list_missing()
}

// ----------------- Cleanup -----------------

#[tauri::command]
pub fn cleanup_raw_events(days: i64) -> AppResult<i64> {
    db::with_conn_mut(|conn| {
        let n = conn.execute(
            "DELETE FROM usage_events WHERE date(timestamp) < date('now', ?1)",
            params![format!("-{days} days")],
        )?;
        Ok(n as i64)
    })
}

#[tauri::command]
pub async fn vacuum_db() -> AppResult<()> {
    crate::scan::run_exclusive_blocking(|| {
        db::with_conn(|conn| {
            conn.execute_batch("VACUUM;")?;
            Ok(())
        })
    })
    .await
}

#[tauri::command]
pub async fn rebuild_daily_aggregates() -> AppResult<()> {
    crate::scan::run_exclusive_blocking(crate::aggregation::rebuild_daily_usage).await
}

#[tauri::command]
pub fn reset_all_data() -> AppResult<ResetSummary> {
    // Stop all filesystem watchers first so they don't race the delete.
    let active = tauri::async_runtime::block_on(crate::watcher::list_active());
    for (id, _) in active {
        let _ = tauri::async_runtime::block_on(crate::watcher::stop(id));
    }

    let _ = crate::collectors::cursor::disconnect();

    db::with_conn_mut(|conn| {
        let mut summary = ResetSummary::default();

        summary.events =
            conn.execute("DELETE FROM usage_events", [])? as i64;
        summary.sessions =
            conn.execute("DELETE FROM sessions", [])? as i64;
        summary.daily_usage =
            conn.execute("DELETE FROM daily_usage", [])? as i64;
        summary.alerts =
            conn.execute("DELETE FROM alerts", [])? as i64;
        summary.file_offsets =
            conn.execute("DELETE FROM file_offsets", [])? as i64;
        summary.inbox_files =
            conn.execute("DELETE FROM inbox_files", [])? as i64;
        summary.projects =
            conn.execute("DELETE FROM projects", [])? as i64;
        summary.pricing_history =
            conn.execute("DELETE FROM pricing_history", [])? as i64;
        summary.sources =
            conn.execute("DELETE FROM sources", [])? as i64;
        summary.settings =
            conn.execute("DELETE FROM settings", [])? as i64;
        summary.cursor_credentials =
            conn.execute("DELETE FROM cursor_credentials", [])? as i64;
        // Note: model_pricing is intentionally preserved — it's your reference
        // data, not user content.

        Ok(summary)
    })
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ResetSummary {
    pub events: i64,
    pub sessions: i64,
    pub daily_usage: i64,
    pub alerts: i64,
    pub file_offsets: i64,
    pub inbox_files: i64,
    pub projects: i64,
    pub pricing_history: i64,
    pub sources: i64,
    pub settings: i64,
    pub cursor_credentials: i64,
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
        "event_hash",
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
    db::with_conn(|conn| {
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    })?;

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
pub async fn generate_sample_data() -> AppResult<i64> {
    crate::scan::run_exclusive_blocking(generate_sample_data_sync).await
}

fn generate_sample_data_sync() -> AppResult<i64> {
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

    for ev in &mut events {
        let (provider, model) = (
            ev.provider.clone().unwrap_or_default(),
            ev.model.clone().unwrap_or_default(),
        );
        if !provider.is_empty() && !model.is_empty() {
            ev.cost_usd = crate::pricing::compute_cost(
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

    // Create a source row for samples
    let source_id = db::upsert_source(
        "Sample Data (built-in)",
        crate::types::SourceKind::Manual,
        Some("<built-in>"),
    )?;

    let inserted = crate::ingest::persist_events(&events, source_id)?;
    Ok(inserted)
}

/// Remove all sample data previously generated by `generate_sample_data`.
///
/// Sample events live under a synthetic source named "Sample Data (built-in)".
/// We delete every event (and the source row itself) tied to that source.
/// `usage_events.source_id` is `ON DELETE SET NULL`, so we must clear events
/// explicitly — otherwise they'd survive as orphan rows and still show up in
/// analytics.
#[tauri::command]
pub fn purge_sample_data() -> AppResult<i64> {
    const SAMPLE_SOURCE_NAME: &str = "Sample Data (built-in)";
    const SAMPLE_SOURCE_PATH: &str = "<built-in>";

    db::with_conn_mut(|conn| {
        // Collect ids of the built-in sample source(s).
        let mut stmt = conn.prepare(
            "SELECT id FROM sources
             WHERE name = ?1 OR (path = ?2 AND kind = 'manual')",
        )?;
        let source_ids: Vec<i64> = stmt
            .query_map(params![SAMPLE_SOURCE_NAME, SAMPLE_SOURCE_PATH], |r| r.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);

        if source_ids.is_empty() {
            return Ok(0);
        }

        let total_events_deleted = delete_events_for_sources(conn, &source_ids)?;
        delete_sessions_for_sources(conn, &source_ids);
        delete_file_offsets_for_sources(conn, &source_ids);

        // Clean up the synthetic source rows.
        for id in &source_ids {
            conn.execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        }

        Ok(total_events_deleted)
    })
}

fn delete_events_for_sources(
    conn: &rusqlite::Connection,
    source_ids: &[i64],
) -> AppResult<i64> {
    let placeholders = std::iter::repeat("?")
        .take(source_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "DELETE FROM usage_events WHERE source_id IN ({})",
        placeholders
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        source_ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let n = conn.execute(&sql, params.as_slice())?;
    Ok(n as i64)
}

fn delete_sessions_for_sources(
    conn: &rusqlite::Connection,
    source_ids: &[i64],
) {
    let placeholders = std::iter::repeat("?")
        .take(source_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "DELETE FROM sessions WHERE source_id IN ({})",
        placeholders
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        source_ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let _ = conn.execute(&sql, params.as_slice());
}

fn delete_file_offsets_for_sources(
    conn: &rusqlite::Connection,
    source_ids: &[i64],
) {
    let placeholders = std::iter::repeat("?")
        .take(source_ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "DELETE FROM file_offsets WHERE source_id IN ({})",
        placeholders
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        source_ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let _ = conn.execute(&sql, params.as_slice());
}

// ----------------- Cursor -----------------

#[tauri::command]
pub async fn cursor_start_login(app: tauri::AppHandle) -> AppResult<()> {
    crate::collectors::cursor::login::start_login(app).await
}

#[tauri::command]
pub async fn cursor_connect_with_token(
    app: tauri::AppHandle,
    token: String,
) -> AppResult<()> {
    crate::collectors::cursor::login::connect_manual(app, token).await
}

#[tauri::command]
pub fn cursor_disconnect() -> AppResult<()> {
    crate::collectors::cursor::disconnect()
}

#[tauri::command]
pub fn cursor_get_status() -> AppResult<crate::types::CursorConnectionStatus> {
    crate::collectors::cursor::status()
}

#[tauri::command]
pub async fn cursor_sync_now() -> AppResult<ScanResult> {
    crate::collectors::cursor::sync_now(true).await
}
