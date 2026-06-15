-- Cursor account credentials and sync cursor (v2)

CREATE TABLE IF NOT EXISTS cursor_credentials (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  session_token TEXT NOT NULL,
  user_id TEXT,
  team_id TEXT,
  label TEXT,
  expires_at TEXT,
  connected_at TEXT NOT NULL,
  last_sync_at TEXT,
  last_sync_cursor TEXT,
  last_sync_result TEXT,
  events_total INTEGER NOT NULL DEFAULT 0
);

INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (2, datetime('now'));
