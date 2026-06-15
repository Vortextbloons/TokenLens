//! LM Studio adapter (Phase 5).
//!
//! LM Studio runs an OpenAI-compatible server locally and writes
//! per-request logs. TokenLens treats LM Studio usage as local and free.

use crate::types::SourceKind;
use std::path::Path;

pub fn default_lmstudio_paths() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        #[cfg(windows)]
        {
            out.push(home.join("AppData").join("Local").join("LM Studio").join("logs"));
        }
        #[cfg(target_os = "macos")]
        out.push(home.join("Library/Logs/LM Studio"));
        #[cfg(all(unix, not(target_os = "macos")))]
        out.push(home.join(".local/share/lmstudio/logs"));
    }
    out
}

pub fn scan(_path: &Path, _source_id: i64) -> crate::errors::AppResult<crate::types::ScanResult> {
    Ok(crate::types::ScanResult::default())
}

pub fn kind() -> SourceKind {
    SourceKind::Lmstudio
}
