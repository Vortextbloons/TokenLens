-- TokenLens schema v3: compress raw_json to zstd BLOB.
--
-- Adds a `raw_json_zstd BLOB` column to `usage_events`. New writes go to the
-- BLOB column; the original `raw_json TEXT` column is kept for backward
-- compatibility with rows from older DBs. The read path picks the compressed
-- copy if present, falling back to the legacy TEXT column otherwise.
--
-- The TEXT column can be dropped in a future major-version migration once we
-- are confident no users have rows from before this point.

ALTER TABLE usage_events ADD COLUMN raw_json_zstd BLOB;

-- Record schema version
INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (3, datetime('now'));
