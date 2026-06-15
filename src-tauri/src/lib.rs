//! TokenLens library entry. Exposes the Tauri builder and the public modules.

pub mod aggregation;
pub mod alerts;
pub mod collectors;
pub mod commands;
pub mod db;
pub mod errors;
pub mod ingest;
pub mod pricing;
pub mod redaction;
pub mod scan;
pub mod settings;
pub mod token_estimator;
pub mod types;
pub mod watcher;

use std::path::PathBuf;
use tauri::Manager;
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;

/// Initialize logging. Returns a guard that must be kept alive for the
/// duration of the program.
pub fn init_logging(debug: bool) -> Option<WorkerGuard> {
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
            let file_appender = tracing_appender::rolling::daily(&dir, "tokenlens.log");
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(non_blocking)
                .with_ansi(false)
                .finish();
            let _ = tracing::subscriber::set_global_default(subscriber);
            return Some(guard);
        }
    }
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
    None
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
            commands::get_breakdown,
            commands::list_events,
            commands::count_events,
            commands::list_pricing,
            commands::upsert_pricing,
            commands::delete_pricing,
            commands::import_pricing_json,
            commands::export_pricing,
            commands::list_missing_pricing,
            commands::recalculate_costs,
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
            commands::cursor_start_login,
            commands::cursor_connect_with_token,
            commands::cursor_disconnect,
            commands::cursor_get_status,
            commands::cursor_sync_now,
            alerts::list_alerts,
            alerts::acknowledge_alert,
            alerts::evaluate_budgets_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
