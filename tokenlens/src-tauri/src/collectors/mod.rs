//! JSONL inbox collector. Watches the app's local inbox directory for
//! new `.jsonl` files written by the OpenCode plugin (or any external
//! tool that follows the same minimal event format). Imports and archives.

use crate::ingest;
use crate::types::SourceKind;
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;
use walkdir::WalkDir;

pub fn default_inbox_path() -> PathBuf {
    let dir = crate::app_local_data_dir().unwrap_or_else(|| PathBuf::from("."));
    dir.join("inbox")
}

pub fn default_archive_path() -> PathBuf {
    default_inbox_path().join("archive")
}

/// Process a single JSONL file: read all events, persist, then move to archive.
pub fn process_jsonl_file(path: &PathBuf, source_id: i64) -> crate::errors::AppResult<usize> {
    let count = ingest::parse_and_persist_file_sync(path, source_id, SourceKind::OpencodeInbox)?;
    // Move to archive
    let archive = default_archive_path();
    let _ = std::fs::create_dir_all(&archive);
    if let Some(name) = path.file_name() {
        let dest = archive.join(name);
        // Best effort
        let _ = std::fs::rename(path, &dest);
    }
    Ok(count)
}

/// One-shot: scan inbox for new files and import each.
pub fn scan_inbox() -> crate::errors::AppResult<crate::types::ScanResult> {
    let start = std::time::Instant::now();
    let mut result = crate::types::ScanResult::default();

    let inbox = default_inbox_path();
    if !inbox.exists() {
        let _ = std::fs::create_dir_all(&inbox);
        return Ok(result);
    }

    // Find or create source row
    let source_id = crate::db::upsert_source(
        &format!("OpenCode Inbox: {}", inbox.display()),
        SourceKind::OpencodeInbox,
        Some(inbox.to_str().unwrap_or("")),
    )?;

    let mut total = 0usize;
    for entry in WalkDir::new(&inbox).max_depth(2).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let is_jsonl = p
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s == "jsonl" || s == "ndjson")
            .unwrap_or(false);
        if !is_jsonl {
            continue;
        }
        if p.starts_with(default_archive_path()) {
            continue;
        }
        result.files_scanned += 1;
        match process_jsonl_file(&p.to_path_buf(), source_id) {
            Ok(n) => { result.events_inserted += n as i64; total += n; }
            Err(e) => { result.errors.push(format!("{}: {}", p.display(), e)); }
        }
    }
    result.duration_ms = start.elapsed().as_millis() as i64;
    info!("Inbox scan: {} events in {}ms", total, result.duration_ms);
    Ok(result)
}

#[tauri::command]
pub fn scan_inbox_command() -> crate::errors::AppResult<crate::types::ScanResult> {
    scan_inbox()
}

/// Run inbox scanner periodically.
pub fn start_periodic_inbox_scanner(interval: Duration) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        loop {
            tick.tick().await;
            if let Err(e) = scan_inbox() {
                tracing::warn!("Periodic inbox scan failed: {}", e);
            }
        }
    });
}
