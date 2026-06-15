//! File system watcher. Watches a configured folder for changes and re-scans
//! modified files, picking up new lines from their byte offset (incremental).

use crate::ingest;
use crate::types::SourceKind;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, DebouncedEventKind, Debouncer};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

type FileDebouncer = Debouncer<notify::RecommendedWatcher>;

static WATCHERS: once_cell::sync::OnceCell<
    Arc<Mutex<HashMap<i64, (PathBuf, FileDebouncer)>>>,
> = once_cell::sync::OnceCell::new();

fn watchers() -> &'static Arc<Mutex<HashMap<i64, (PathBuf, FileDebouncer)>>> {
    WATCHERS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

pub async fn start(source_id: i64, path: PathBuf) -> crate::errors::AppResult<()> {
    if !path.exists() {
        return Err(crate::errors::AppError::NotFound(format!(
            "Path does not exist: {}",
            path.display()
        )));
    }
    let mut guard = watchers().lock().await;
    if guard.contains_key(&source_id) {
        info!("Watcher for source {} already running", source_id);
        return Ok(());
    }

    let path_for_thread = path.clone();
    let tx = tokio::sync::mpsc::channel::<Vec<PathBuf>>(64);
    let tx_send = tx.0.clone();

    let mut debouncer = new_debouncer(Duration::from_millis(500), move |res: notify_debouncer_mini::DebounceEventResult| {
        match res {
            Ok(events) => {
                let mut paths: Vec<PathBuf> = events
                    .into_iter()
                    .filter(|e| matches!(e.kind, DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous))
                    .map(|e: DebouncedEvent| e.path)
                    .filter(|p| is_supported(p))
                    .collect();
                paths.sort();
                paths.dedup();
                if !paths.is_empty() {
                    let _ = tx_send.blocking_send(paths);
                }
            }
            Err(e) => {
                warn!("Watcher error: {:?}", e);
            }
        }
    })
    .map_err(|e| crate::errors::AppError::Internal(format!("watcher: {e}")))?;

    debouncer
        .watcher()
        .watch(&path, RecursiveMode::Recursive)
        .map_err(|e| crate::errors::AppError::Internal(format!("watch: {e}")))?;

    // Background task: process change batches
    let mut rx = tx.1;
    let kind = SourceKind::OpencodeLogs;
    tokio::spawn(async move {
        while let Some(paths) = rx.recv().await {
            for p in paths {
                if let Err(e) = ingest::parse_and_persist_file(&p, source_id, kind).await {
                    error!("Failed to process {}: {}", p.display(), e);
                }
            }
        }
    });

    guard.insert(source_id, (path_for_thread, debouncer));
    info!("Started watcher for source {} at {}", source_id, path.display());
    Ok(())
}

pub async fn stop(source_id: i64) {
    let mut guard = watchers().lock().await;
    if let Some((path, _)) = guard.remove(&source_id) {
        info!("Stopped watcher for source {} ({})", source_id, path.display());
    }
}

pub async fn is_running(source_id: i64) -> bool {
    watchers().lock().await.contains_key(&source_id)
}

pub async fn list_active() -> Vec<(i64, PathBuf)> {
    watchers()
        .lock()
        .await
        .iter()
        .map(|(k, (p, _))| (*k, p.clone()))
        .collect()
}

fn is_supported(p: &std::path::Path) -> bool {
    matches!(
        p.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .as_deref(),
        Some("jsonl") | Some("json") | Some("log") | Some("ndjson")
    )
}
