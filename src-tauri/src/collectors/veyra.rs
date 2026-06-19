//! Veyra adapter (Phase 5).
//!
//! Veyra writes request/response logs in a structured format under its own
//! data directory. This collector scans for those logs and maps them to
//! canonical events. The exact log layout is documented in Veyra's source
//! and may evolve — for the initial implementation we look for JSONL files
//! with a Veyra-specific header (e.g. `{"veyra_version": ...}`).
//!
//! For the MVP, the adapter is registered as a source kind and provides a
//! `scan_path` function. Real parsing of Veyra's request/response shape
//! lands in Phase 5.

use crate::ingest;
use crate::types::{SourceKind, UsageEvent};
use std::path::Path;
use tracing::warn;

pub fn default_veyra_paths() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        #[cfg(windows)]
        {
            out.push(home.join("AppData").join("Local").join("Veyra").join("logs"));
            out.push(home.join("AppData").join("Roaming").join("Veyra").join("logs"));
        }
        #[cfg(target_os = "macos")]
        out.push(home.join("Library/Logs/Veyra"));
        #[cfg(all(unix, not(target_os = "macos")))]
        out.push(home.join(".local/share/veyra/logs"));
    }
    out
}

/// Parse a single Veyra log file. Best-effort: skip lines we don't
/// understand, persist what we can.
pub fn parse_veyra_file(path: &Path) -> Vec<UsageEvent> {
    // Re-use the generic OpenCode parser — Veyra logs are also JSONL.
    match ingest::parse_file(path, 0) {
        Ok(events) => events,
        Err(e) => {
            warn!("Failed to parse Veyra file {}: {}", path.display(), e);
            vec![]
        }
    }
}

pub fn scan(path: &Path, source_id: i64) -> crate::errors::AppResult<crate::types::ScanResult> {
    let _ = source_id;
    let _ = path;
    // Real implementation lands in Phase 5. For now, return empty.
    warn!("Veyra adapter is a Phase 5 feature; scanning is a no-op.");
    Ok(crate::types::ScanResult::default())
}

pub fn kind() -> SourceKind {
    SourceKind::Veyra
}
