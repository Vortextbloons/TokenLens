// Shared TypeScript types mirroring Rust contracts in src-tauri/src/types.rs

export type Exactness = "exact" | "estimated" | "mixed" | "unknown";

export type SourceKind =
  | "opencode_logs"
  | "opencode_inbox"
  | "veyra"
  | "lmstudio"
  | "oai_proxy"
  | "manual";

export interface Source {
  id: number;
  name: string;
  kind: string;
  path: string | null;
  enabled: boolean;
  last_scanned_at: string | null;
  last_error: string | null;
  created_at: string;
}

export interface Session {
  id: number;
  source_session_id: string;
  source_id: number | null;
  project_id: number | null;
  title: string | null;
  started_at: string | null;
  last_seen_at: string | null;
  provider: string | null;
  model: string | null;
  total_tokens: number;
  total_cost_usd: number;
  exactness: string;
  raw_ref: string | null;
}

export interface UsageEvent {
  event_hash: string;
  timestamp: string;
  source_id: number | null;
  session_id: number | null;
  project_id: number | null;
  event_type: string;
  provider: string | null;
  model: string | null;
  message_role: string | null;
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  tool_tokens: number;
  total_tokens: number;
  cost_usd: number;
  exactness: Exactness;
  confidence: number;
  raw_json: string | null;
  raw_source_path: string | null;
}

export interface ModelPricing {
  id: number | null;
  provider: string;
  model: string;
  input_price_per_million: number;
  output_price_per_million: number;
  reasoning_price_per_million: number;
  cache_read_price_per_million: number;
  cache_write_price_per_million: number;
  currency: string;
  effective_date: string | null;
  is_local: boolean;
  source: string;
  updated_at: string;
}

export interface OverviewStats {
  tokens_today: number;
  tokens_week: number;
  tokens_month: number;
  cost_today_usd: number;
  cost_week_usd: number;
  cost_month_usd: number;
  most_used_model: string | null;
  most_expensive_model: string | null;
  largest_session_id: number | null;
  largest_session_tokens: number;
  avg_tokens_per_session: number;
  input_output_ratio: number;
  reasoning_token_pct: number;
  cache_savings_usd: number;
  sessions_count: number;
  exactness_mix: {
    exact: number;
    estimated: number;
    mixed: number;
    unknown: number;
  };
}

export interface TimeseriesPoint {
  date: string;
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cache_read_tokens: number;
  total_tokens: number;
  cost_usd: number;
}

export interface Breakdown {
  key: string;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
  sessions_count: number;
}

export interface QueryFilter {
  start_date?: string | null;
  end_date?: string | null;
  project_id?: number | null;
  provider?: string | null;
  model?: string | null;
  source_id?: number | null;
  exactness?: string | null;
  limit?: number | null;
  offset?: number | null;
}

export interface ScanResult {
  files_scanned: number;
  events_inserted: number;
  events_skipped_duplicate: number;
  errors: string[];
  duration_ms: number;
}

export interface AppSettings {
  currency: string;
  autostart: boolean;
  start_minimized: boolean;
  theme: string;
  store_raw_json: boolean;
  store_message_text: boolean;
  redact_secrets: boolean;
  anonymize_paths: boolean;
  raw_retention_days: number;
  max_db_size_mb: number;
  auto_cleanup: boolean;
  default_range: string;
  missing_price_behavior: string;
  token_estimation_mode: string;
  watcher_enabled: boolean;
  debug_logging: boolean;
  collector_endpoint_enabled: boolean;
}

export interface AppError {
  kind: string;
  message: string;
}
