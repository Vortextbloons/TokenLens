//! Cursor dashboard collector: auth, API sync, and CSV import.
//!
//! When the `cursor` feature is disabled, the API/auth/login modules are
//! excluded from the build entirely. The normalize module (CSV import) is
//! always available because it doesn't depend on reqwest.

use crate::errors::AppResult;
use std::path::Path;

#[cfg(feature = "cursor")]
pub mod api;
#[cfg(feature = "cursor")]
pub mod auth;
#[cfg(feature = "cursor")]
pub mod login;
pub mod normalize;

#[cfg(feature = "cursor")]
mod runtime {
    use crate::db;
    use crate::errors::{AppError, AppResult};
    use crate::ingest;
    use crate::scan;
    use crate::settings;
    use crate::types::{CursorConnectionStatus, ScanResult, SourceKind, UsageEvent};
    use chrono::Utc;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::{Duration, Instant};
    use tracing::{debug, info, warn};

    pub const SOURCE_NAME: &str = "Cursor Account";
    pub const SOURCE_PATH: &str = "cursor://account";

    static LAST_BACKGROUND_SYNC_MS: AtomicI64 = AtomicI64::new(0);
    const BACKGROUND_SYNC_INTERVAL_MS: i64 = 3_600_000;

    fn imported_totals() -> AppResult<(i64, i64)> {
        db::with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(e.total_tokens), 0)
                 FROM usage_events e
                 INNER JOIN sources s ON s.id = e.source_id
                 WHERE e.ignored = 0 AND s.kind = 'cursor'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(Into::into)
        })
    }

    pub fn status() -> AppResult<CursorConnectionStatus> {
        let creds = super::auth::load()?;
        Ok(match creds {
            None => CursorConnectionStatus::default(),
            Some(c) => {
                let (events_total, tokens_total) = imported_totals().unwrap_or((0, 0));
                CursorConnectionStatus {
                    connected: true,
                    email_or_user_label: c.label,
                    expires_at: c.expires_at.map(|d| d.to_rfc3339()),
                    last_sync_at: c.last_sync_at.map(|d| d.to_rfc3339()),
                    last_sync_result: c.last_sync_result,
                    events_total,
                    tokens_total,
                }
            }
        })
    }

    pub fn disconnect() -> AppResult<()> {
        super::auth::delete()?;
        info!("Cursor account disconnected");
        Ok(())
    }

    pub async fn sync_now(force: bool) -> AppResult<ScanResult> {
        if !force {
            let last = LAST_BACKGROUND_SYNC_MS.load(Ordering::Relaxed);
            let now = Utc::now().timestamp_millis();
            if last > 0 && now - last < BACKGROUND_SYNC_INTERVAL_MS {
                debug!("Cursor background sync skipped (ran recently)");
                return Ok(ScanResult::default());
            }
        }
        let result = scan::run_exclusive_blocking_async(move || sync_inner(force)).await?;
        LAST_BACKGROUND_SYNC_MS.store(Utc::now().timestamp_millis(), Ordering::Relaxed);
        Ok(result)
    }

    /// `full_resync`: when true (manual sync / first connect), pull the full
    /// billing cycle from Cursor. When false (background), only fetch events
    /// after the last cursor.
    pub async fn sync_inner(full_resync: bool) -> AppResult<ScanResult> {
        use super::api::{billing_range_ms, datetime_to_ms, CursorClient};
        let start = Instant::now();
        let mut result = ScanResult::default();

        let creds = super::auth::load()?.ok_or_else(|| {
            AppError::NotFound("Cursor account not connected".into())
        })?;

        let client = CursorClient::new(&creds)?;
        let summary = client.validate().await?;

        let (start_ms, end_ms) = if !full_resync && creds.last_sync_cursor.is_some() {
            (
                creds.last_sync_cursor.map(datetime_to_ms),
                Some(Utc::now().timestamp_millis()),
            )
        } else {
            billing_range_ms(&summary)
        };

        let raw_events = client.fetch_all_events(start_ms, end_ms).await?;
        result.files_scanned = 1;

        let mut events: Vec<UsageEvent> = raw_events
            .iter()
            .filter_map(|v| super::normalize::normalize_api_event(v, Some(SOURCE_PATH)))
            .collect();

        if events.is_empty() {
            result.duration_ms = start.elapsed().as_millis() as i64;
            super::auth::update_sync_meta(Utc::now(), creds.last_sync_cursor, "0 new events", 0)?;
            return Ok(result);
        }

        apply_privacy_settings(&mut events)?;

        let source_id = db::upsert_source(SOURCE_NAME, SourceKind::Cursor, Some(SOURCE_PATH))?;
        let inserted = ingest::persist_events(&events, source_id)?;
        result.events_inserted = inserted;
        result.events_skipped_duplicate = events.len() as i64 - inserted;

        let newest = events.iter().map(|e| e.timestamp).max();
        let summary_text = format!(
            "{} inserted, {} duplicates",
            result.events_inserted, result.events_skipped_duplicate
        );
        super::auth::update_sync_meta(Utc::now(), newest, &summary_text, result.events_inserted)?;

        let _ = db::with_conn_mut(|conn| db::update_source_scanned(conn, source_id));

        result.duration_ms = start.elapsed().as_millis() as i64;
        info!(
            "Cursor sync: {} inserted in {}ms",
            result.events_inserted, result.duration_ms
        );
        Ok(result)
    }

    fn apply_privacy_settings(events: &mut [UsageEvent]) -> AppResult<()> {
        let s = settings::load_all()?;
        if s.redact_secrets {
            for ev in events.iter_mut() {
                if let Some(raw) = &ev.raw_json {
                    ev.raw_json = Some(crate::redaction::redact(raw));
                }
            }
        }
        if !s.store_raw_json {
            for ev in events.iter_mut() {
                ev.raw_json = None;
            }
        }
        Ok(())
    }

    pub fn start_periodic_sync() {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if super::auth::is_connected() {
                if let Err(e) = sync_now(false).await {
                    warn!("Startup Cursor sync skipped: {e}");
                }
            }
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                if !super::auth::is_connected() {
                    continue;
                }
                if let Err(e) = sync_now(false).await {
                    warn!("Periodic Cursor sync failed: {e}");
                }
            }
        });
    }
}

#[cfg(feature = "cursor")]
pub use runtime::*;

/// CSV import path is always available — no reqwest dependency.
pub fn import_csv_file(path: &Path) -> AppResult<usize> {
    let mut events = normalize::parse_cursor_csv(path)?;
    if events.is_empty() {
        return Ok(0);
    }
    apply_privacy_settings_csv(&mut events)?;
    let source_id = crate::db::upsert_source(
        &format!("Cursor CSV: {}", path.display()),
        crate::types::SourceKind::Cursor,
        path.to_str(),
    )?;
    let inserted = crate::ingest::persist_events(&events, source_id)?;
    Ok(inserted as usize)
}

fn apply_privacy_settings_csv(events: &mut [crate::types::UsageEvent]) -> AppResult<()> {
    let s = crate::settings::load_all()?;
    if s.redact_secrets {
        for ev in events.iter_mut() {
            if let Some(raw) = &ev.raw_json {
                ev.raw_json = Some(crate::redaction::redact(raw));
            }
        }
    }
    if !s.store_raw_json {
        for ev in events.iter_mut() {
            ev.raw_json = None;
        }
    }
    Ok(())
}
