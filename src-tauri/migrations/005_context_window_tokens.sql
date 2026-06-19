-- Add optional model context window metadata for utilization analytics.

ALTER TABLE model_pricing ADD COLUMN context_window_tokens INTEGER;

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (5, datetime('now'));
