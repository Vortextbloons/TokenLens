-- TokenLens schema v4: track file mtimes for incremental scans.

ALTER TABLE file_offsets ADD COLUMN file_mtime INTEGER;

-- Record schema version
INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (4, datetime('now'));
