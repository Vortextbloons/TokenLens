// Mirrors the Rust pricing engine for mock/dev mode parity.

import type { ModelPricing } from "@/types/contracts";
import type { UsageEvent } from "@/types/contracts";
import contextWindows from "../../pricing/context-windows.json";

type ProviderAlias = {
  nativeProvider: string;
  modelMap?: Record<string, string>;
};

type ContextWindowFallback = {
  model: string;
  context_window_tokens: number;
};

const AGGREGATOR_ALIASES: Record<string, ProviderAlias[]> = {
  "opencode-go": [
    { nativeProvider: "zhipu" },
    { nativeProvider: "moonshot" },
    { nativeProvider: "deepseek" },
    { nativeProvider: "xiaomi" },
    { nativeProvider: "minimax" },
    {
      nativeProvider: "qwen",
      modelMap: {
        "qwen3.7-max": "qwen-3.7-max",
        "qwen3.7-plus": "qwen-3.7-plus",
        "qwen3.6-plus": "qwen-3.6-plus",
        "qwen3.5-plus": "qwen-3.5-plus",
      },
    },
  ],
  nvidia: [
    {
      nativeProvider: "zhipu",
      modelMap: { "z-ai/glm5": "glm-5" },
    },
  ],
};

const CONTEXT_WINDOW_FALLBACKS = contextWindows as ContextWindowFallback[];

export function resolvePricing(
  provider: string,
  model: string,
  table: ModelPricing[],
): ModelPricing | null {
  const exact = table.find((p) => p.provider === provider && p.model === model);
  if (exact) return exact;
  const any = table.find((p) => p.provider === provider && p.model === "any");
  if (any) return any;
  if (provider === "local" || provider === "lmstudio") {
    return table.find((p) => p.provider === "local" && p.model === "any") ?? null;
  }
  const aliases = AGGREGATOR_ALIASES[provider];
  if (aliases) {
    for (const alias of aliases) {
      const nativeModel = alias.modelMap?.[model] ?? model;
      const hit = table.find(
        (p) => p.provider === alias.nativeProvider && p.model === nativeModel,
      );
      if (hit) return hit;
    }
  }
  return null;
}

export function resolveContextWindow(
  provider: string,
  model: string,
  table: ModelPricing[],
): number | null {
  const exact = resolvePricing(provider, model, table);
  if (exact?.context_window_tokens) return exact.context_window_tokens;
  const m = model.toLowerCase();
  return CONTEXT_WINDOW_FALLBACKS.find((row) => m.startsWith(row.model))?.context_window_tokens ?? null;
}

function estimatePricing(
  provider: string,
  model: string,
  table: ModelPricing[],
): ModelPricing | null {
  let best: ModelPricing | null = null;
  let bestLen = 0;
  for (const row of table) {
    if (row.provider !== provider || row.is_local) continue;
    if (model === row.model || model.startsWith(row.model) || row.model.startsWith(model)) {
      if (row.model.length >= bestLen) {
        bestLen = row.model.length;
        best = row;
      }
    }
  }
  if (best) return best;
  const rows = table.filter((p) => p.provider === provider && !p.is_local);
  if (!rows.length) return null;
  const n = rows.length;
  const avg = (f: (p: ModelPricing) => number) =>
    rows.reduce((a, r) => a + f(r), 0) / n;
  return {
    id: null,
    provider,
    model,
    input_price_per_million: avg((r) => r.input_price_per_million),
    output_price_per_million: avg((r) => r.output_price_per_million),
    reasoning_price_per_million: avg((r) => r.reasoning_price_per_million),
    cache_read_price_per_million: avg((r) => r.cache_read_price_per_million),
    cache_write_price_per_million: avg((r) => r.cache_write_price_per_million),
    context_window_tokens: null,
    currency: rows[0].currency,
    effective_date: null,
    is_local: false,
    source: "estimate",
    updated_at: "",
  };
}

function costFromPricing(
  p: ModelPricing,
  input: number,
  output: number,
  reasoning: number,
  cacheRead: number,
  cacheWrite: number,
): number {
  const nonCachedInput = Math.max(0, input - cacheRead);
  const billable =
    p.reasoning_price_per_million > 0 && reasoning > 0 && reasoning <= output
      ? { output: output - reasoning, reasoning }
      : { output, reasoning: 0 };
  const cost =
    nonCachedInput * p.input_price_per_million +
    billable.output * p.output_price_per_million +
    billable.reasoning * p.reasoning_price_per_million +
    cacheRead * p.cache_read_price_per_million +
    cacheWrite * p.cache_write_price_per_million;
  return cost / 1_000_000;
}

export function isResolved(provider: string, model: string, table: ModelPricing[]): boolean {
  return resolvePricing(provider, model, table) !== null;
}

export function computeEventCost(
  provider: string,
  model: string,
  input: number,
  output: number,
  reasoning: number,
  cacheRead: number,
  cacheWrite: number,
  table: ModelPricing[],
  missingBehavior = "warn",
): { cost: number; unpriced: boolean; estimated: boolean } {
  let p = resolvePricing(provider, model, table);
  let estimated = false;
  if (!p && missingBehavior === "estimate") {
    p = estimatePricing(provider, model, table);
    estimated = !!p;
  }
  if (!p) return { cost: 0, unpriced: true, estimated: false };
  if (p.is_local) return { cost: 0, unpriced: false, estimated: false };
  return {
    cost: costFromPricing(p, input, output, reasoning, cacheRead, cacheWrite),
    unpriced: false,
    estimated,
  };
}

export function cacheSavingsForEvent(
  provider: string,
  model: string,
  cacheRead: number,
  table: ModelPricing[],
): number {
  const p = resolvePricing(provider, model, table);
  if (!p || cacheRead <= 0) return 0;
  const delta = Math.max(0, p.input_price_per_million - p.cache_read_price_per_million);
  return (cacheRead * delta) / 1_000_000;
}

export function peakContextForEvents(events: UsageEvent[]): number {
  let peak = 0;
  for (const e of events) {
    peak = Math.max(peak, e.input_tokens + e.cache_read_tokens + e.tool_tokens);
  }
  return peak;
}

export function quoteSessionSwap(
  events: UsageEvent[],
  targetProvider: string,
  targetModel: string,
  table: ModelPricing[],
): { simulated_cost_usd: number; target_pricing_status: string } {
  let simulated_cost_usd = 0;
  let status: "priced" | "estimated" | "unpriced" = "priced";
  for (const e of events) {
    const { cost, unpriced, estimated } = computeEventCost(
      targetProvider,
      targetModel,
      e.input_tokens,
      e.output_tokens,
      e.reasoning_tokens,
      e.cache_read_tokens,
      e.cache_write_tokens,
      table,
    );
    simulated_cost_usd += cost;
    if (unpriced) status = "unpriced";
    else if (estimated && status !== "unpriced") status = "estimated";
  }
  return { simulated_cost_usd, target_pricing_status: status };
}
