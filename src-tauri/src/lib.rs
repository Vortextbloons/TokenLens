//! TokenLens library entry. Exposes the Tauri builder and the public modules.

pub mod aggregation;
pub mod alerts;
pub mod collectors;
pub mod commands;
pub mod db;
pub mod errors;
pub mod ingest;
pub mod pricing;
pub mod raw_json_codec;
pub mod redaction;
pub mod scan;
pub mod settings;
pub mod token_estimator;
pub mod types;
pub mod watcher;

use std::path::PathBuf;
use tauri::Manager;
use tracing::{info, warn};

/// Initialize logging. Returns a writer guard that should be kept alive for
/// the duration of the program (drops the file handle on shutdown).
pub fn init_logging(debug: bool) -> Option<tracing_appender_shim::FileWriter> {
    let filter = if debug {
        "tokenlens_lib=debug,tokenlens=debug,info"
    } else {
        "tokenlens_lib=info,tokenlens=warn,info"
    };
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter));

    // Try to write to a log file in app data dir; fall back to stderr.
    let log_dir = app_data_dir().map(|p| p.join("logs"));
    if let Some(dir) = log_dir {
        if std::fs::create_dir_all(&dir).is_ok() {
            let path = dir.join("tokenlens.log");
            // Daily rolling: append date suffix when the day rolls over.
            match tracing_appender_shim::FileWriter::open(&path) {
                Ok(writer) => {
                    let make_writer = writer.clone();
                    let subscriber = tracing_subscriber::fmt()
                        .with_env_filter(env_filter)
                        .with_writer(move || make_writer.clone())
                        .with_ansi(false)
                        .finish();
                    let _ = tracing::subscriber::set_global_default(subscriber);
                    return Some(writer);
                }
                Err(e) => {
                    warn!("failed to open log file {}: {e}", path.display());
                }
            }
        }
    }
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
    None
}

mod tracing_appender_shim {
    use std::fs::{File, OpenOptions};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A minimal "rolling daily" file writer. Opens (or creates) the given
    /// path. When the current date (UTC) changes since the last write, the
    /// file is renamed with a `YYYY-MM-DD` suffix and a new file is opened.
    /// Stands in for the heavier `tracing-appender` crate to keep the
    /// binary small.
    #[derive(Clone)]
    pub struct FileWriter {
        inner: std::sync::Arc<Mutex<Inner>>,
    }

    struct Inner {
        current_path: PathBuf,
        current_date: String,
        file: File,
    }

    impl FileWriter {
        pub fn open(path: &Path) -> io::Result<Self> {
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            let writer = Self {
                inner: std::sync::Arc::new(Mutex::new(Inner {
                    current_path: path.to_path_buf(),
                    current_date: today_utc(),
                    file,
                })),
            };
            Ok(writer)
        }
    }

    impl Write for FileWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut g = self.inner.lock().unwrap();
            let today = today_utc();
            if today != g.current_date {
                // Roll the file: flush the current descriptor (drop), rename
                // with the previous date stamp, and open a fresh one.
                let prev = g.current_path.clone();
                let stem = prev
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("tokenlens")
                    .to_string();
                let ext = prev
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("log")
                    .to_string();
                let rotated = prev.with_file_name(format!("{stem}-{}.{}", g.current_date, ext));
                let _ = std::fs::rename(&prev, &rotated);
                g.file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&prev)?;
                g.current_date = today;
            }
            g.file.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            let mut g = self.inner.lock().unwrap();
            g.file.flush()
        }
    }

    fn today_utc() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let days = secs / 86_400;
        let (y, m, d) = civil_from_days(days);
        format!("{y:04}-{m:02}-{d:02}")
    }

    /// Howard Hinnant's date algorithm — convert days-since-epoch to (Y, M, D).
    fn civil_from_days(z: i64) -> (i32, u32, u32) {
        let z = z + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = (yoe as i32) + (era as i32) * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
        let y = if m <= 2 { y + 1 } else { y };
        (y, m, d)
    }

    impl Drop for FileWriter {
        fn drop(&mut self) {
            // Best-effort flush; close happens via Drop on inner.file.
            let _ = self.flush();
        }
    }
}

pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|p| p.join("TokenLens"))
}

pub fn app_local_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|p| p.join("TokenLens"))
}

pub fn db_path() -> PathBuf {
    let dir = app_local_data_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.join("tokenlens.sqlite")
}

/// Build and run the Tauri app.
pub fn run() {
    // 1. Logging (settings may fall back to defaults until DB is ready)
    let s = settings::load_all().unwrap_or_else(|_| settings::AppSettings::defaults());
    let _guard = init_logging(s.debug_logging);

    // 2. Database
    let db_path = db_path();
    if let Err(e) = db::init(&db_path) {
        warn!("Failed to init DB at {}: {}", db_path.display(), e);
    }

    // 3. Seed pricing defaults
    if let Err(e) = pricing::seed_defaults() {
        warn!("Failed to seed pricing: {}", e);
    }
    if let Err(e) = pricing::prime_cache() {
        warn!("Failed to prime pricing cache: {}", e);
    }

    info!("TokenLens starting up");

    // One-time backfill: compress any raw_json TEXT rows left over from
    // before schema v3. The DB will only see a meaningful shrink the
    // first time this runs; subsequent launches are no-ops.
    if let Err(e) = db::backfill_compress_raw_json() {
        warn!("raw_json backfill failed: {e}");
    }

    // Populate `daily_usage` if it has never been built. The aggregation
    // queries in `overview()` take a fast path through this pre-aggregated
    // table, so an empty `daily_usage` would silently zero out the
    // rolling-window KPIs. We only trigger when the table is empty to
    // avoid an O(N) rebuild on every launch.
    if let Err(e) = db::ensure_daily_usage_built() {
        warn!("daily_usage warm-up failed: {e}");
    }

    #[cfg(feature = "cursor")]
    collectors::cursor::start_periodic_sync();

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Ensure app data dirs exist
            if let Ok(p) = app.path().app_data_dir() {
                let _ = std::fs::create_dir_all(p.join("logs"));
            }
            if let Ok(p) = app.path().app_local_data_dir() {
                let _ = std::fs::create_dir_all(p.join("inbox"));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::get_sources,
            commands::add_source,
            commands::remove_source,
            commands::scan_source,
            commands::start_watcher,
            commands::stop_watcher,
            commands::list_watchers,
            commands::discover_default_sources,
            commands::get_overview_stats,
            commands::get_usage_timeseries,
            commands::get_sessions,
            commands::get_session_detail,
            commands::get_session_events,
            commands::simulate_session_swap,
            commands::get_breakdown,
            commands::get_anomaly_highlights,
            commands::get_cache_efficiency,
            commands::get_context_utilization,
            commands::list_events,
            commands::count_events,
            commands::list_pricing,
            commands::upsert_pricing,
            commands::delete_pricing,
            commands::sync_pricing_seed,
            commands::import_pricing_json,
            commands::export_pricing,
            commands::list_missing_pricing,
            commands::recalculate_costs,
            commands::recalculate_token_estimates,
            commands::cleanup_raw_events,
            commands::vacuum_db,
            commands::rebuild_daily_aggregates,
            commands::reset_all_data,
            commands::db_size_mb,
            commands::export_csv,
            commands::export_json,
            commands::backup_db,
            commands::generate_sample_data,
            commands::purge_sample_data,
            commands::scan_inbox,
            #[cfg(feature = "cursor")]
            commands::cursor_start_login,
            #[cfg(feature = "cursor")]
            commands::cursor_connect_with_token,
            #[cfg(feature = "cursor")]
            commands::cursor_disconnect,
            #[cfg(feature = "cursor")]
            commands::cursor_get_status,
            #[cfg(feature = "cursor")]
            commands::cursor_sync_now,
            alerts::list_alerts,
            alerts::acknowledge_alert,
            alerts::evaluate_budgets_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
