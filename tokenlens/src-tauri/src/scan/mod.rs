//! Heavy-operation coordination: run work off the UI thread and prevent
//! overlapping scans that would contend on the single SQLite writer.

use crate::errors::{AppError, AppResult};
use once_cell::sync::Lazy;
use parking_lot::Mutex;

static HEAVY_OP_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// Run work on Tokio's blocking thread pool so the webview stays responsive.
pub async fn run_blocking<F, R>(f: F) -> AppResult<R>
where
    F: FnOnce() -> AppResult<R> + Send + 'static,
    R: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Internal(format!("background task failed: {e}")))?
}

/// Run CPU/IO-heavy work off the UI thread. Only one exclusive operation may
/// run at a time (scans, recalculations, vacuum, etc.).
pub async fn run_exclusive_blocking<F, R>(f: F) -> AppResult<R>
where
    F: FnOnce() -> AppResult<R> + Send + 'static,
    R: Send + 'static,
{
    run_blocking(move || {
        let _guard = HEAVY_OP_LOCK.try_lock().ok_or_else(|| {
            AppError::Invalid(
                "Another scan or heavy operation is already in progress. Please wait for it to finish."
                    .into(),
            )
        })?;
        f()
    })
    .await
}

/// How many events to persist per SQLite transaction during large imports.
pub const PERSIST_BATCH_SIZE: usize = 500;
