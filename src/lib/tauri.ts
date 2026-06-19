// Typed Tauri invoke wrapper. Falls back to a mock backend when running
// outside Tauri (e.g. plain `vite` dev) so the UI can be developed standalone.

import type {
  AppSettings,
  AnomalyHighlight,
  CacheEfficiencyReport,
  Breakdown,
  CursorConnectionStatus,
  ContextUtilizationReport,
  ModelPricing,
  OverviewStats,
  QueryFilter,
  ScanResult,
  Session,
  SessionSwapQuote,
  Source,
  TimeseriesPoint,
  UsageEvent,
} from "@/types/contracts";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
declare const __APP_VERSION__: string;

// Mock backend is dev-only. In Tauri production builds, `__TAURI_INTERNALS__`
// is always set, so this branch is unreachable. The dynamic import keeps the
// ~600-line mock out of the production bundle entirely.
let mockBackend: Record<string, (args: any) => any> | null = null;
async function getMockBackend(): Promise<Record<string, (args: any) => any> | null> {
  if (inTauri) return null;
  if (mockBackend) return mockBackend;
  if (!import.meta.env.DEV) return null;
  const mod = await import("./mock");
  mockBackend = mod.MOCK_BACKEND as Record<string, (args: any) => any>;
  return mockBackend;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const mocks = await getMockBackend();
  if (mocks) {
    const fn = mocks[cmd];
    if (typeof fn === "function") {
      return fn(args ?? {}) as Promise<T>;
    }
    throw new Error(`[mock] No mock for command: ${cmd}`);
  }
  const { invoke: realInvoke } = await import("@tauri-apps/api/core");
  return realInvoke<T>(cmd, args);
}

// ----------------- Settings -----------------

export const getSettings = () => invoke<AppSettings>("get_settings");
export const updateSettings = (settings: AppSettings) =>
  invoke<AppSettings>("update_settings", { s: settings });

// ----------------- Sources -----------------

export const getSources = () => invoke<Source[]>("get_sources");
export const addSource = (name: string, kind: string, path: string) =>
  invoke<Source>("add_source", { name, kind, path });
export const removeSource = (id: number) => invoke<void>("remove_source", { id });
export const scanSource = (id: number) => invoke<ScanResult>("scan_source", { id });
export const startWatcher = (id: number) => invoke<void>("start_watcher", { id });
export const stopWatcher = (id: number) => invoke<void>("stop_watcher", { id });
export const listWatchers = () => invoke<[number, string][]>("list_watchers");
export const discoverDefaultSources = () =>
  invoke<Source[]>("discover_default_sources");
export const scanInbox = () => invoke<ScanResult>("scan_inbox");

// ----------------- Cursor -----------------

export const cursorStartLogin = () => invoke<void>("cursor_start_login");
export const cursorConnectWithToken = (token: string) =>
  invoke<void>("cursor_connect_with_token", { token });
export const cursorDisconnect = () => invoke<void>("cursor_disconnect");
export const cursorGetStatus = () =>
  invoke<CursorConnectionStatus>("cursor_get_status");
export const cursorSyncNow = () => invoke<ScanResult>("cursor_sync_now");

// ----------------- Analytics -----------------

export const getOverviewStats = (filter: QueryFilter) =>
  invoke<OverviewStats>("get_overview_stats", { filter });
export const getUsageTimeseries = (filter: QueryFilter) =>
  invoke<TimeseriesPoint[]>("get_usage_timeseries", { filter });
export const getSessions = (filter: QueryFilter) =>
  invoke<Session[]>("get_sessions", { filter });
export const getSessionDetail = (id: number) =>
  invoke<Session | null>("get_session_detail", { id });
export const getSessionEvents = (id: number) =>
  invoke<UsageEvent[]>("get_session_events", { id });
export const simulateSessionSwap = (
  sessionId: number,
  targetProvider: string,
  targetModel: string,
) => invoke<SessionSwapQuote | null>("simulate_session_swap", {
  session_id: sessionId,
  target_provider: targetProvider,
  target_model: targetModel,
});
export const getBreakdown = (filter: QueryFilter, dimension: string) =>
  invoke<Breakdown[]>("get_breakdown", { filter, dimension });
export const getAnomalyHighlights = (filter: QueryFilter) =>
  invoke<AnomalyHighlight[]>("get_anomaly_highlights", { filter });
export const getCacheEfficiency = (filter: QueryFilter) =>
  invoke<CacheEfficiencyReport>("get_cache_efficiency", { filter });
export const getContextUtilization = (filter: QueryFilter) =>
  invoke<ContextUtilizationReport>("get_context_utilization", { filter });
export const listEvents = (filter: QueryFilter) =>
  invoke<UsageEvent[]>("list_events", { filter });
export const countEvents = () => invoke<number>("count_events");

// ----------------- Pricing -----------------

export const listPricing = () => invoke<ModelPricing[]>("list_pricing");
export const upsertPricing = (p: ModelPricing) => invoke<number>("upsert_pricing", { p });
export const deletePricing = (provider: string, model: string) =>
  invoke<void>("delete_pricing", { provider, model });
export const syncPricingSeed = () => invoke<number>("sync_pricing_seed");
export const recalculateCosts = () => invoke<number>("recalculate_costs");
export const recalculateTokenEstimates = () =>
  invoke<number>("recalculate_token_estimates");

// Pricing research workflow — see docs/pricing-research-preset.md.
// `importPricingJson` accepts a JSON array of ModelPricing rows (the shape
// the AI preset produces). Each row goes through the same upsert path used
// by manual edits, so pricing_history is recorded and a "ai-research:<url>"
// source can be traced back to the URL the AI used.
export interface BulkImportSummary {
  received: number;
  inserted: number;
  updated: number;
  skipped: number;
  errors: string[];
}
export const importPricingJson = (rows: ModelPricing[]) =>
  invoke<BulkImportSummary>("import_pricing_json", { rows });
export const exportPricing = () => invoke<ModelPricing[]>("export_pricing");

// `MissingPricingRow` mirrors the Rust type in pricing::MissingPricingRow.
export interface MissingPricingRow {
  provider: string;
  model: string;
  events: number;
  total_tokens: number;
  current_cost_usd: number;
}
export const listMissingPricing = () => invoke<MissingPricingRow[]>("list_missing_pricing");

// ----------------- Cleanup -----------------

export const cleanupRawEvents = (days: number) =>
  invoke<number>("cleanup_raw_events", { days });
export const vacuumDb = () => invoke<void>("vacuum_db");
export const rebuildDailyAggregates = () => invoke<void>("rebuild_daily_aggregates");
export interface ResetSummary {
  events: number;
  sessions: number;
  daily_usage: number;
  alerts: number;
  file_offsets: number;
  inbox_files: number;
  projects: number;
  pricing_history: number;
  sources: number;
  settings: number;
}

export const resetAllData = () => invoke<ResetSummary>("reset_all_data");
export const dbSizeMb = () => invoke<number>("db_size_mb");

// ----------------- Exports -----------------

export const exportCsv = (filter: QueryFilter, outPath: string) =>
  invoke<number>("export_csv", { filter, outPath });
export const exportJson = (filter: QueryFilter, outPath: string) =>
  invoke<number>("export_json", { filter, outPath });
export const backupDb = (outPath: string) => invoke<void>("backup_db", { outPath });

// ----------------- Sample data -----------------

export const generateSampleData = () => invoke<number>("generate_sample_data");
export const purgeSampleData = () => invoke<number>("purge_sample_data");

// ----------------- Alerts -----------------

export const listAlerts = (limit: number = 50) => invoke<any[]>("list_alerts", { limit });
export const acknowledgeAlert = (id: number) => invoke<void>("acknowledge_alert", { id });
export const evaluateBudgets = () => invoke<number>("evaluate_budgets_command");

// ----------------- Utility -----------------

export const isTauri = inTauri;

export async function getAppVersion(): Promise<string> {
  if (inTauri) {
    const { getVersion } = await import("@tauri-apps/api/app");
    return getVersion();
  }
  return __APP_VERSION__;
}

/**
 * Show a confirm dialog that works in both Tauri and browser (dev) mode.
 * In Tauri this routes to the native dialog plugin (requires
 * `dialog:allow-ask` / `dialog:allow-confirm` in the capability). In a plain
 * browser it falls back to `window.confirm`. The string is rendered as
 * plain text — no markdown in either path.
 */
export async function confirmDialog(
  message: string,
  opts: { title?: string; kind?: "info" | "warning" | "error" } = {}
): Promise<boolean> {
  if (inTauri) {
    const { ask, confirm } = await import("@tauri-apps/plugin-dialog");
    try {
      return await ask(message, {
        title: opts.title ?? "Confirm",
        kind: opts.kind ?? "warning",
      });
    } catch {
      // Fall back to the simpler `confirm` if `ask` is unavailable.
      return await confirm(message, {
        title: opts.title ?? "Confirm",
        kind: opts.kind ?? "warning",
      });
    }
  }
  return window.confirm(message);
}
