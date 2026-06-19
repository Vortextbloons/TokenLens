// Shared TypeScript types mirroring Rust contracts in src-tauri/src/types.rs

export type Exactness = "exact" | "estimated" | "mixed" | "unknown";

export type SourceKind =
  | "opencode_logs"
  | "opencode_inbox"
  | "veyra"
  | "lmstudio"
  | "oai_proxy"
  | "manual"
  | "cursor";

export interface CursorConnectionStatus {
  connected: boolean;
  email_or_user_label: string | null;
  expires_at: string | null;
  last_sync_at: string | null;
  last_sync_result: string | null;
  events_total: number;
  tokens_total: number;
}

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
  context_window_tokens: number | null;
  currency: string;
  effective_date: string | null;
  is_local: boolean;
  source: string;
  updated_at: string;
}

export interface OverviewStats {
  tokens_lifetime: number;
  cost_lifetime_usd: number;
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
  unpriced_events: number;
  unpriced_tokens: number;
  exactness_mix: {
    exact: number;
    estimated: number;
    mixed: number;
    unknown: number;
  };
  /** Tokens in the user-selected date range. Pairs with `prev_period_tokens`. */
  period_tokens: number;
  /** Cost (USD) in the user-selected date range. Pairs with `prev_period_cost_usd`. */
  period_cost_usd: number;
  /** Tokens in the immediately preceding period of the same length as the user's selected date range. 0 for "all time" or empty prior windows. */
  prev_period_tokens: number;
  /** Cost (USD) in the immediately preceding period of the same length as the user's selected date range. */
  prev_period_cost_usd: number;
}

export interface TimeseriesPoint {
  date: string;
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  total_tokens: number;
  cost_usd: number;
}

export interface CacheEfficiencyPoint {
  date: string;
  cache_read_tokens: number;
  cache_write_tokens: number;
  cache_savings_usd: number;
}

export interface CacheEfficiencyRow {
  key: string;
  cache_read_tokens: number;
  cache_write_tokens: number;
  cache_savings_usd: number;
  total_tokens: number;
  cost_usd: number;
  sessions_count: number;
}

export interface CacheEfficiencyReport {
  series: CacheEfficiencyPoint[];
  by_provider: CacheEfficiencyRow[];
  by_model: CacheEfficiencyRow[];
}

export interface AnomalyHighlight {
  kind: "session" | "day" | string;
  label: string;
  session_id: number | null;
  date: string | null;
  provider: string | null;
  model: string | null;
  total_tokens: number;
  baseline_tokens: number;
  ratio: number;
  event_count: number;
  model_switches: number;
  peak_context_tokens: number;
  peak_context_pct: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  cost_usd: number;
  reason: string;
}

export interface ContextUtilizationRow {
  session_id: number;
  label: string;
  provider: string | null;
  model: string | null;
  last_seen_at: string | null;
  peak_context_tokens: number;
  context_window_tokens: number | null;
  utilization_pct: number;
  event_count: number;
  model_switches: number;
  cost_usd: number;
}

export interface ContextUtilizationPoint {
  date: string;
  avg_utilization_pct: number;
  max_utilization_pct: number;
  sessions_over_80: number;
}

export interface ContextUtilizationReport {
  sessions: ContextUtilizationRow[];
  trend: ContextUtilizationPoint[];
}

export interface SessionSwapQuote {
  session_id: number;
  current_provider: string | null;
  current_model: string | null;
  target_provider: string;
  target_model: string;
  current_cost_usd: number;
  simulated_cost_usd: number;
  delta_usd: number;
  delta_pct: number;
  target_context_window_tokens: number | null;
  target_pricing_status: string;
  events_count: number;
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
  local_date?: string | null;
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
  budget_daily_tokens?: number;
  budget_monthly_cost_usd?: number;
}

export interface AlertRow {
  id: number;
  alert_type: string;
  severity: string;
  title: string;
  message: string;
  created_at: string;
  acknowledged_at: string | null;
}

export interface AppError {
  kind: string;
  message: string;
}
