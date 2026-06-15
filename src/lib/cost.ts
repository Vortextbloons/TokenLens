// Mirrors the Rust pricing engine for mock/dev mode parity.

import type { ModelPricing } from "@/types/contracts";

type ProviderAlias = {
  nativeProvider: string;
  modelMap?: Record<string, string>;
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
