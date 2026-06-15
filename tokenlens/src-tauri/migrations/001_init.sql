-- TokenLens schema v1 (initial)
-- Idempotent: use IF NOT EXISTS throughout.
-- Journal/WAL/foreign_keys pragmas are applied in db::init before migrations run.

CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,            -- 'opencode_logs', 'opencode_inbox', 'veyra', 'lmstudio', 'oai_proxy', 'manual'
  path TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_scanned_at TEXT,
  last_error TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT,
  path TEXT,
  git_remote TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(path)
);

CREATE TABLE IF NOT EXISTS sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_session_id TEXT NOT NULL,
  source_id INTEGER,
  project_id INTEGER,
  title TEXT,
  started_at TEXT,
  last_seen_at TEXT,
  provider TEXT,
  model TEXT,
  total_tokens INTEGER DEFAULT 0,
  total_cost_usd REAL DEFAULT 0,
  exactness TEXT DEFAULT 'unknown',   -- exact|estimated|mixed|unknown
  raw_ref TEXT,
  UNIQUE(source_id, source_session_id),
  FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE SET NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS usage_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_hash TEXT NOT NULL UNIQUE,
  timestamp TEXT NOT NULL,
  source_id INTEGER,
  session_id INTEGER,
  project_id INTEGER,
  event_type TEXT NOT NULL,           -- 'message', 'completion', 'tool_call', 'session_start', etc.
  provider TEXT,
  model TEXT,
  message_role TEXT,
  input_tokens INTEGER DEFAULT 0,
  output_tokens INTEGER DEFAULT 0,
  reasoning_tokens INTEGER DEFAULT 0,
  cache_read_tokens INTEGER DEFAULT 0,
  cache_write_tokens INTEGER DEFAULT 0,
  tool_tokens INTEGER DEFAULT 0,
  total_tokens INTEGER DEFAULT 0,
  cost_usd REAL DEFAULT 0,
  exactness TEXT DEFAULT 'unknown',
  confidence REAL DEFAULT 0,
  raw_json TEXT,
  raw_source_path TEXT,
  ignored INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE SET NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE SET NULL,
  FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS model_pricing (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_price_per_million REAL DEFAULT 0,
  output_price_per_million REAL DEFAULT 0,
  reasoning_price_per_million REAL DEFAULT 0,
  cache_read_price_per_million REAL DEFAULT 0,
  cache_write_price_per_million REAL DEFAULT 0,
  currency TEXT DEFAULT 'USD',
  effective_date TEXT,
  is_local INTEGER NOT NULL DEFAULT 0,
  source TEXT DEFAULT 'manual',     -- 'manual' | 'seed' | 'api'
  updated_at TEXT NOT NULL,
  UNIQUE(provider, model)
);

CREATE TABLE IF NOT EXISTS pricing_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  pricing_id INTEGER NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_price_per_million REAL,
  output_price_per_million REAL,
  reasoning_price_per_million REAL,
  cache_read_price_per_million REAL,
  cache_write_price_per_million REAL,
  currency TEXT,
  captured_at TEXT NOT NULL,
  FOREIGN KEY(pricing_id) REFERENCES model_pricing(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS daily_usage (
  date TEXT NOT NULL,                -- YYYY-MM-DD
  provider TEXT,
  model TEXT,
  project_id INTEGER,
  input_tokens INTEGER DEFAULT 0,
  output_tokens INTEGER DEFAULT 0,
  reasoning_tokens INTEGER DEFAULT 0,
  cache_read_tokens INTEGER DEFAULT 0,
  cache_write_tokens INTEGER DEFAULT 0,
  total_tokens INTEGER DEFAULT 0,
  cost_usd REAL DEFAULT 0,
  sessions_count INTEGER DEFAULT 0,
  PRIMARY KEY(date, provider, model, project_id)
);

CREATE TABLE IF NOT EXISTS alerts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  alert_type TEXT NOT NULL,         -- 'daily_tokens', 'monthly_cost', 'context_full', 'collector_stale', etc.
  severity TEXT NOT NULL,           -- 'info' | 'warning' | 'critical'
  title TEXT NOT NULL,
  message TEXT NOT NULL,
  context_json TEXT,
  created_at TEXT NOT NULL,
  acknowledged_at TEXT
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS file_offsets (
  source_id INTEGER NOT NULL,
  file_path TEXT NOT NULL,
  byte_offset INTEGER NOT NULL DEFAULT 0,
  last_seen_at TEXT,
  PRIMARY KEY(source_id, file_path),
  FOREIGN KEY(source_id) REFERENCES sources(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS inbox_files (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source_id INTEGER,
  path TEXT NOT NULL UNIQUE,
  size_bytes INTEGER,
  first_seen_at TEXT NOT NULL,
  last_processed_at TEXT,
  processed INTEGER NOT NULL DEFAULT 0,
  archived_at TEXT
);

-- Indexes for fast lookups (plan §8)
CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_events(session_id);
CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_events(model);
CREATE INDEX IF NOT EXISTS idx_usage_project ON usage_events(project_id);
CREATE INDEX IF NOT EXISTS idx_usage_provider ON usage_events(provider);
CREATE INDEX IF NOT EXISTS idx_usage_exactness ON usage_events(exactness);
CREATE INDEX IF NOT EXISTS idx_usage_ignored ON usage_events(ignored);
CREATE INDEX IF NOT EXISTS idx_sessions_last_seen ON sessions(last_seen_at);
CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_id);
CREATE INDEX IF NOT EXISTS idx_sessions_provider_model ON sessions(provider, model);
CREATE INDEX IF NOT EXISTS idx_daily_date ON daily_usage(date);
CREATE INDEX IF NOT EXISTS idx_alerts_created ON alerts(created_at);
CREATE INDEX IF NOT EXISTS idx_pricing_provider_model ON model_pricing(provider, model);

-- Record schema version
INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (1, datetime('now'));
