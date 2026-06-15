// In-memory mock backend used when running outside Tauri (e.g. plain Vite dev).
// Lets the UI be developed and demoed without the Tauri shell.

import type {
  AppSettings,
  Breakdown,
  ModelPricing,
  OverviewStats,
  QueryFilter,
  ScanResult,
  Session,
  Source,
  TimeseriesPoint,
  UsageEvent,
} from "@/types/contracts";

const now = new Date();
function ago(days: number, hours = 0): string {
  const d = new Date(now);
  d.setDate(d.getDate() - days);
  d.setHours(d.getHours() - hours);
  return d.toISOString();
}

const SOURCES: Source[] = [
  {
    id: 1,
    name: "OpenCode: ~/.local/share/opencode/log",
    kind: "opencode_logs",
    path: null,
    enabled: true,
    last_scanned_at: ago(0, 1),
    last_error: null,
    created_at: ago(7),
  },
  {
    id: 2,
    name: "Sample Data (built-in)",
    kind: "manual",
    path: "<built-in>",
    enabled: true,
    last_scanned_at: ago(0, 2),
    last_error: null,
    created_at: ago(0, 2),
  },
];

const SESSIONS: Session[] = Array.from({ length: 12 }, (_, i) => ({
  id: i + 1,
  source_session_id: `sess-${i}`,
  source_id: 1 + (i % 2),
  project_id: null,
  title: `Session ${i + 1}: build the dashboard`,
  started_at: ago(i, 3),
  last_seen_at: ago(i, 0),
  provider: ["openai", "anthropic", "google", "local"][i % 4],
  model: ["gpt-4o", "claude-sonnet-4-5", "gemini-2.5-pro", "llama-3.1-8b"][i % 4],
  total_tokens: 5000 + (i * 1731) % 120000,
  total_cost_usd: 0.01 + ((i * 0.27) % 5.5),
  exactness: i % 5 === 0 ? "estimated" : "exact",
  raw_ref: null,
}));

function makeEvents(): UsageEvent[] {
  const out: UsageEvent[] = [];
  let id = 0;
  for (let day = 0; day < 14; day++) {
    for (let s = 0; s < 3 + (day % 4); s++) {
      const sessionId = `sess-${day}-${s}`;
      for (let m = 0; m < 5 + (s % 8); m++) {
        const provider = ["openai", "anthropic", "google", "local"][(day + s + m) % 4];
        const model = ["gpt-4o", "gpt-4o-mini", "claude-sonnet-4-5", "gemini-2.5-flash", "llama-3.1-8b"][(s + m) % 5];
        const input = 200 + ((m * 137 + day * 53 + s * 11) % 1500);
        const output = 100 + ((m * 73 + day * 17 + s * 7) % 800);
        const reasoning = model.includes("o1") || model.includes("o3") ? Math.floor(output * 0.4) : 0;
        const cache = model.includes("gpt-4o") || model.includes("claude") ? Math.floor(input * 0.6) : 0;
        const total = input + output;
        out.push({
          event_hash: `mock-${id++}`,
          timestamp: ago(day, m * 2 + s),
          source_id: 1 + (s % 2),
          session_id: SESSIONS[s]?.id ?? null,
          project_id: null,
          event_type: "message",
          provider,
          model,
          message_role: m % 2 === 0 ? "user" : "assistant",
          input_tokens: input,
          output_tokens: output,
          reasoning_tokens: reasoning,
          cache_read_tokens: cache,
          cache_write_tokens: 0,
          tool_tokens: 0,
          total_tokens: total,
          cost_usd: provider === "local" ? 0 : total * 0.000005,
          exactness: "exact",
          confidence: 0.95,
          raw_json: null,
          raw_source_path: null,
        });
      }
    }
  }
  return out;
}

const EVENTS = makeEvents();

const PRICING: ModelPricing[] = [
  {
    id: 1, provider: "openai", model: "gpt-4o",
    input_price_per_million: 2.5, output_price_per_million: 10.0,
    reasoning_price_per_million: 0, cache_read_price_per_million: 1.25,
    cache_write_price_per_million: 0, currency: "USD", effective_date: null,
    is_local: false, source: "seed", updated_at: ago(7),
  },
  {
    id: 2, provider: "anthropic", model: "claude-sonnet-4-5",
    input_price_per_million: 3.0, output_price_per_million: 15.0,
    reasoning_price_per_million: 0, cache_read_price_per_million: 0.30,
    cache_write_price_per_million: 3.75, currency: "USD", effective_date: null,
    is_local: false, source: "seed", updated_at: ago(7),
  },
  {
    id: 3, provider: "local", model: "any",
    input_price_per_million: 0, output_price_per_million: 0,
    reasoning_price_per_million: 0, cache_read_price_per_million: 0,
    cache_write_price_per_million: 0, currency: "USD", effective_date: null,
    is_local: true, source: "seed", updated_at: ago(7),
  },
];

const SETTINGS: AppSettings = {
  currency: "USD",
  autostart: false,
  start_minimized: false,
  theme: "system",
  store_raw_json: true,
  store_message_text: false,
  redact_secrets: true,
  anonymize_paths: false,
  raw_retention_days: 14,
  max_db_size_mb: 0,
  auto_cleanup: true,
  default_range: "7d",
  missing_price_behavior: "warn",
  token_estimation_mode: "chars4",
  watcher_enabled: true,
  debug_logging: false,
  collector_endpoint_enabled: false,
};

function matchFilter(f: QueryFilter | undefined, ev: UsageEvent): boolean {
  if (!f) return true;
  if (f.provider && ev.provider !== f.provider) return false;
  if (f.model && ev.model !== f.model) return false;
  if (f.exactness && ev.exactness !== f.exactness) return false;
  if (f.start_date && ev.timestamp < f.start_date) return false;
  if (f.end_date && ev.timestamp > f.end_date + "T23:59:59.999Z") return false;
  return true;
}

function overviewFor(filter: QueryFilter): OverviewStats {
  const filtered = EVENTS.filter((e) => matchFilter(filter, e));
  const today = ago(0).slice(0, 10);
  const tokens_today = filtered.filter((e) => e.timestamp.startsWith(today)).reduce((a, b) => a + b.total_tokens, 0);
  const tokens_week = filtered.reduce((a, b) => a + b.total_tokens, 0);
  const tokens_month = tokens_week;
  const cost_today_usd = filtered.filter((e) => e.timestamp.startsWith(today)).reduce((a, b) => a + b.cost_usd, 0);
  const cost_week_usd = filtered.reduce((a, b) => a + b.cost_usd, 0);
  const cost_month_usd = cost_week_usd;
  const byModel: Record<string, number> = {};
  const byModelCost: Record<string, number> = {};
  for (const e of filtered) {
    const k = e.model ?? "unknown";
    byModel[k] = (byModel[k] ?? 0) + e.total_tokens;
    byModelCost[k] = (byModelCost[k] ?? 0) + e.cost_usd;
  }
  const most_used_model = Object.entries(byModel).sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
  const most_expensive_model = Object.entries(byModelCost).sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
  const in_t = filtered.reduce((a, b) => a + b.input_tokens, 0);
  const out_t = filtered.reduce((a, b) => a + b.output_tokens, 0);
  const reas_t = filtered.reduce((a, b) => a + b.reasoning_tokens, 0);
  const cache_t = filtered.reduce((a, b) => a + b.cache_read_tokens, 0);
  return {
    tokens_today,
    tokens_week,
    tokens_month,
    cost_today_usd,
    cost_week_usd,
    cost_month_usd,
    most_used_model,
    most_expensive_model,
    largest_session_id: SESSIONS[0]?.id ?? null,
    largest_session_tokens: SESSIONS[0]?.total_tokens ?? 0,
    avg_tokens_per_session: filtered.length ? tokens_week / Math.max(new Set(filtered.map((e) => e.session_id)).size, 1) : 0,
    input_output_ratio: out_t > 0 ? in_t / out_t : 0,
    reasoning_token_pct: (in_t + out_t) > 0 ? (reas_t / (in_t + out_t)) * 100 : 0,
    cache_savings_usd: (cache_t * 1.25) / 1_000_000,
    sessions_count: new Set(filtered.map((e) => e.session_id)).size,
    exactness_mix: {
      exact: filtered.filter((e) => e.exactness === "exact").length,
      estimated: filtered.filter((e) => e.exactness === "estimated").length,
      mixed: filtered.filter((e) => e.exactness === "mixed").length,
      unknown: filtered.filter((e) => e.exactness === "unknown").length,
    },
  };
}

function timeseriesFor(filter: QueryFilter): TimeseriesPoint[] {
  const filtered = EVENTS.filter((e) => matchFilter(filter, e));
  const map: Record<string, TimeseriesPoint> = {};
  for (const e of filtered) {
    const d = e.timestamp.slice(0, 10);
    const p = (map[d] ??= {
      date: d, input_tokens: 0, output_tokens: 0, reasoning_tokens: 0,
      cache_read_tokens: 0, total_tokens: 0, cost_usd: 0,
    });
    p.input_tokens += e.input_tokens;
    p.output_tokens += e.output_tokens;
    p.reasoning_tokens += e.reasoning_tokens;
    p.cache_read_tokens += e.cache_read_tokens;
    p.total_tokens += e.total_tokens;
    p.cost_usd += e.cost_usd;
  }
  return Object.values(map).sort((a, b) => a.date.localeCompare(b.date));
}

function breakdownFor(filter: QueryFilter, dim: string): Breakdown[] {
  const filtered = EVENTS.filter((e) => matchFilter(filter, e));
  const map: Record<string, Breakdown> = {};
  for (const e of filtered) {
    const key = (dim === "model" ? e.model : dim === "provider" ? e.provider : e.provider) ?? "(none)";
    const p = (map[key] ??= {
      key, total_tokens: 0, input_tokens: 0, output_tokens: 0, cost_usd: 0, sessions_count: 0,
    });
    p.total_tokens += e.total_tokens;
    p.input_tokens += e.input_tokens;
    p.output_tokens += e.output_tokens;
    p.cost_usd += e.cost_usd;
  }
  return Object.values(map).sort((a, b) => b.total_tokens - a.total_tokens);
}

export const MOCK_BACKEND: Record<string, (args: any) => any> = {
  get_settings: () => SETTINGS,
  update_settings: ({ s }: { s: AppSettings }) => Object.assign(SETTINGS, s),
  get_sources: () => SOURCES,
  add_source: ({ name, kind, path }: { name: string; kind: string; path: string }) => {
    const s: Source = {
      id: SOURCES.length + 1, name, kind, path, enabled: true,
      last_scanned_at: null, last_error: null, created_at: new Date().toISOString(),
    };
    SOURCES.push(s);
    return s;
  },
  remove_source: ({ id }: { id: number }) => {
    const i = SOURCES.findIndex((s) => s.id === id);
    if (i >= 0) SOURCES.splice(i, 1);
  },
  scan_source: ({ id }: { id: number }) => {
    const s = SOURCES.find((x) => x.id === id);
    if (s) s.last_scanned_at = new Date().toISOString();
    return { files_scanned: 4, events_inserted: 0, events_skipped_duplicate: 0, errors: [], duration_ms: 142 } satisfies ScanResult;
  },
  start_watcher: () => undefined,
  stop_watcher: () => undefined,
  list_watchers: () => [],
  discover_default_sources: () => SOURCES,
  get_overview_stats: ({ filter }: { filter: QueryFilter }) => overviewFor(filter ?? {}),
  get_usage_timeseries: ({ filter }: { filter: QueryFilter }) => timeseriesFor(filter ?? {}),
  get_sessions: ({ filter }: { filter: QueryFilter }) => {
    if (filter?.model) return SESSIONS.filter((s) => s.model === filter.model);
    if (filter?.provider) return SESSIONS.filter((s) => s.provider === filter.provider);
    return SESSIONS;
  },
  get_session_detail: ({ id }: { id: number }) => SESSIONS.find((s) => s.id === id) ?? null,
  get_session_events: ({ id }: { id: number }) => {
    const s = SESSIONS.find((x) => x.id === id);
    if (!s) return [];
    return EVENTS.filter((e) => e.session_id === id).slice(0, 50);
  },
  get_breakdown: ({ filter, dimension }: { filter: QueryFilter; dimension: string }) =>
    breakdownFor(filter ?? {}, dimension),
  list_events: ({ filter }: { filter: QueryFilter }) =>
    EVENTS.filter((e) => matchFilter(filter, e)).slice(0, filter?.limit ?? 200),
  count_events: () => EVENTS.length,
  list_pricing: () => PRICING,
  upsert_pricing: ({ p }: { p: ModelPricing }) => {
    const i = PRICING.findIndex((x) => x.provider === p.provider && x.model === p.model);
    if (i >= 0) {
      PRICING[i] = { ...p, id: PRICING[i].id, updated_at: new Date().toISOString() };
      return PRICING[i].id!;
    }
    const next: ModelPricing = { ...p, id: PRICING.length + 1, updated_at: new Date().toISOString() };
    PRICING.push(next);
    return next.id!;
  },
  delete_pricing: ({ provider, model }: { provider: string; model: string }) => {
    const i = PRICING.findIndex((x) => x.provider === provider && x.model === model);
    if (i >= 0) PRICING.splice(i, 1);
  },
  recalculate_costs: () => EVENTS.length,
  cleanup_raw_events: () => 0,
  vacuum_db: () => undefined,
  rebuild_daily_aggregates: () => undefined,
  reset_all_data: () => undefined,
  db_size_mb: () => 12.4,
  export_csv: () => 0,
  export_json: () => 0,
  backup_db: () => undefined,
  generate_sample_data: () => 100,
  list_alerts: () => [],
  acknowledge_alert: () => undefined,
  evaluate_budgets_command: () => 0,
  scan_inbox: () => ({ files_scanned: 0, events_inserted: 0, events_skipped_duplicate: 0, errors: [], duration_ms: 0 }),
};
