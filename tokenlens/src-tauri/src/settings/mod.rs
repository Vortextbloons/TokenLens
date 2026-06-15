//! Settings store backed by the `settings` table (key-value).
//!
//! Tauri-plugin-store is also wired up for the frontend, but critical backend
//! settings (pricing table, watcher enabled, etc.) live here for consistency.

use crate::db;
use crate::errors::AppResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    /// Currency display (USD by default).
    pub currency: String,
    /// Auto-start on system boot.
    pub autostart: bool,
    /// Start minimized to tray.
    pub start_minimized: bool,
    /// Theme: light | dark | system.
    pub theme: String,
    /// Store raw JSON for events.
    pub store_raw_json: bool,
    /// Store full message text (off by default).
    pub store_message_text: bool,
    /// Redact secrets from raw JSON.
    pub redact_secrets: bool,
    /// Anonymize filesystem paths.
    pub anonymize_paths: bool,
    /// Days to retain raw JSON.
    pub raw_retention_days: i64,
    /// Max DB size in MB (0 = unlimited).
    pub max_db_size_mb: i64,
    /// Auto-cleanup enabled.
    pub auto_cleanup: bool,
    /// Default date range: 1d, 7d, 30d, 90d, all
    pub default_range: String,
    /// Missing-price behavior: "skip" | "estimate" | "warn"
    pub missing_price_behavior: String,
    /// Token estimation mode: "off" | "chars4" | "tiktoken"
    pub token_estimation_mode: String,
    /// File watcher enabled.
    pub watcher_enabled: bool,
    /// Debug logging.
    pub debug_logging: bool,
    /// Collector endpoint enabled (Phase 5).
    pub collector_endpoint_enabled: bool,
}

impl AppSettings {
    pub fn defaults() -> Self {
        Self {
            currency: "USD".to_string(),
            autostart: false,
            start_minimized: false,
            theme: "system".to_string(),
            store_raw_json: true,
            store_message_text: false,
            redact_secrets: true,
            anonymize_paths: false,
            raw_retention_days: 14,
            max_db_size_mb: 0,
            auto_cleanup: true,
            default_range: "7d".to_string(),
            missing_price_behavior: "warn".to_string(),
            token_estimation_mode: "chars4".to_string(),
            watcher_enabled: true,
            debug_logging: false,
            collector_endpoint_enabled: false,
        }
    }
}

pub fn load_all() -> AppResult<AppSettings> {
    let mut out = AppSettings::defaults();
    let map: HashMap<String, String> = db::with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let mut rows = stmt.query([])?;
        let mut m = HashMap::new();
        while let Some(row) = rows.next()? {
            let k: String = row.get(0)?;
            let v: String = row.get(1)?;
            m.insert(k, v);
        }
        Ok(m)
    })?;
    for (k, v) in map {
        apply_kv(&mut out, &k, &v);
    }
    Ok(out)
}

pub fn save_all(s: &AppSettings) -> AppResult<()> {
    let pairs: Vec<(&'static str, String)> = vec![
        ("currency", s.currency.clone()),
        ("autostart", s.autostart.to_string()),
        ("start_minimized", s.start_minimized.to_string()),
        ("theme", s.theme.clone()),
        ("store_raw_json", s.store_raw_json.to_string()),
        ("store_message_text", s.store_message_text.to_string()),
        ("redact_secrets", s.redact_secrets.to_string()),
        ("anonymize_paths", s.anonymize_paths.to_string()),
        ("raw_retention_days", s.raw_retention_days.to_string()),
        ("max_db_size_mb", s.max_db_size_mb.to_string()),
        ("auto_cleanup", s.auto_cleanup.to_string()),
        ("default_range", s.default_range.clone()),
        ("missing_price_behavior", s.missing_price_behavior.clone()),
        ("token_estimation_mode", s.token_estimation_mode.clone()),
        ("watcher_enabled", s.watcher_enabled.to_string()),
        ("debug_logging", s.debug_logging.to_string()),
        ("collector_endpoint_enabled", s.collector_endpoint_enabled.to_string()),
    ];
    db::with_conn_mut(|conn| {
        let tx = conn.unchecked_transaction()?;
        for (k, v) in pairs {
            tx.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
                rusqlite::params![k, v],
            )?;
        }
        tx.commit()?;
        Ok(())
    })
}

fn apply_kv(s: &mut AppSettings, k: &str, v: &str) {
    match k {
        "currency" => s.currency = v.to_string(),
        "autostart" => s.autostart = v == "true",
        "start_minimized" => s.start_minimized = v == "true",
        "theme" => s.theme = v.to_string(),
        "store_raw_json" => s.store_raw_json = v == "true",
        "store_message_text" => s.store_message_text = v == "true",
        "redact_secrets" => s.redact_secrets = v == "true",
        "anonymize_paths" => s.anonymize_paths = v == "true",
        "raw_retention_days" => s.raw_retention_days = v.parse().unwrap_or(14),
        "max_db_size_mb" => s.max_db_size_mb = v.parse().unwrap_or(0),
        "auto_cleanup" => s.auto_cleanup = v == "true",
        "default_range" => s.default_range = v.to_string(),
        "missing_price_behavior" => s.missing_price_behavior = v.to_string(),
        "token_estimation_mode" => s.token_estimation_mode = v.to_string(),
        "watcher_enabled" => s.watcher_enabled = v == "true",
        "debug_logging" => s.debug_logging = v == "true",
        "collector_endpoint_enabled" => s.collector_endpoint_enabled = v == "true",
        _ => {}
    }
}
