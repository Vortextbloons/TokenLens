//! OpenCode-specific collector helpers.

use crate::ingest;
use crate::types::SourceKind;
use std::path::Path;

pub fn kind() -> SourceKind {
    SourceKind::OpencodeLogs
}

pub fn default_paths() -> Vec<std::path::PathBuf> {
    ingest::default_opencode_log_paths()
}

pub fn scan(path: &Path, source_id: i64) -> crate::errors::AppResult<crate::types::ScanResult> {
    ingest::scan_path(path, source_id)
}
