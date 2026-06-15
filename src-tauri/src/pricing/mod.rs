//! Pricing engine. Maintains a per-(provider, model) price table and computes
//! costs in USD (or configured currency).
//!
//! Cost formula:
//!   non_cached_input = max(0, input - cache_read)
//!   When reasoning is billed separately (o-series): output splits into
//!   (output - reasoning) at output rate + reasoning at reasoning rate.
//!   cache_read and cache_write use their own rates.

use crate::db;
use crate::errors::{AppError, AppResult};
use crate::settings;
use crate::types::{Exactness, ModelPricing};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use std::sync::OnceLock;

/// In-memory cache to avoid hitting SQLite on every event.
static CACHE: OnceLock<parking_lot::RwLock<HashMap<(String, String), ModelPricing>>> =
    OnceLock::new();

fn cache() -> &'static parking_lot::RwLock<HashMap<(String, String), ModelPricing>> {
    CACHE.get_or_init(|| parking_lot::RwLock::new(HashMap::new()))
}

pub fn prime_cache() -> AppResult<()> {
    let map = db::with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, provider, model, input_price_per_million, output_price_per_million,
                    reasoning_price_per_million, cache_read_price_per_million,
                    cache_write_price_per_million, currency, effective_date,
                    is_local, source, updated_at
             FROM model_pricing",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ModelPricing {
                id: r.get(0)?,
                provider: r.get(1)?,
                model: r.get(2)?,
                input_price_per_million: r.get(3)?,
                output_price_per_million: r.get(4)?,
                reasoning_price_per_million: r.get(5)?,
                cache_read_price_per_million: r.get(6)?,
                cache_write_price_per_million: r.get(7)?,
                currency: r.get(8)?,
                effective_date: r.get(9)?,
                is_local: r.get::<_, i64>(10)? != 0,
                source: r.get(11)?,
                updated_at: r.get(12)?,
            })
        })?;
        let mut m = HashMap::new();
        for row in rows {
            let p = row?;
            m.insert((p.provider.clone(), p.model.clone()), p);
        }
        Ok::<_, AppError>(m)
    })?;
    let mut w = cache().write();
    *w = map;
    Ok(())
}

pub fn list_all() -> AppResult<Vec<ModelPricing>> {
    let mut v: Vec<ModelPricing> = cache().read().values().cloned().collect();
    v.sort_by(|a, b| (a.provider.cmp(&b.provider)).then(a.model.cmp(&b.model)));
    Ok(v)
}

pub fn get(provider: &str, model: &str) -> Option<ModelPricing> {
    cache()
        .read()
        .get(&(provider.to_string(), model.to_string()))
        .cloned()
}

fn exists_in_db(provider: &str, model: &str) -> AppResult<bool> {
    db::with_conn(|conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE provider = ?1 AND model = ?2",
            params![provider, model],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    })
}

fn cursor_model_alias(model: &str) -> Option<(&'static str, &'static str)> {
    let m = model.to_lowercase();
    if m.starts_with("claude-opus-4-7") || m.starts_with("claude-opus-4.7") {
        return Some(("anthropic", "claude-opus-4.7"));
    }
    if m.starts_with("claude-opus-4-8") || m.starts_with("claude-opus-4.8") {
        return Some(("anthropic", "claude-opus-4.8"));
    }
    if m.starts_with("claude-sonnet") {
        return Some(("anthropic", "claude-sonnet-4.6"));
    }
    if m.starts_with("claude-haiku") {
        return Some(("anthropic", "claude-haiku-4.5"));
    }
    if m.starts_with("gpt-") || m.contains("codex") {
        return Some(("openai", "gpt-5.3-codex"));
    }
    if m.starts_with("gemini-") {
        return Some(("google", "gemini-2.5-pro"));
    }
    None
}

/// Apps and inference hosts — not model vendors. Costs resolve via underlying model.
pub fn is_aggregator_provider(provider: &str) -> bool {
    matches!(
        provider,
        "opencode-go"
            | "github-copilot"
            | "deepinfra"
            | "together"
            | "fireworks"
            | "ollama-cloud"
            | "nvidia"
            | "cursor"
    )
}

fn normalize_aggregator_model(model: &str) -> String {
    let mut m = model.to_lowercase();
    if let Some(stripped) = m.strip_suffix("-free") {
        m = stripped.to_string();
    }
    if let Some(stripped) = m.strip_suffix("-preview") {
        m = stripped.to_string();
    }
    if let Some(rest) = m.strip_prefix("qwen3.") {
        m = format!("qwen-3.{rest}");
    } else if let Some(rest) = m.strip_prefix("qwen3") {
        m = format!("qwen-3{rest}");
    }
    if let Some((_prefix, rest)) = m.rsplit_once('/') {
        m = rest.to_string();
    }
    if m == "glm5" {
        m = "glm-5".into();
    }
    m
}

fn model_vendor_hint(model: &str) -> Option<&'static str> {
    let m = model.to_lowercase();
    if m.starts_with("gpt-")
        || m.contains("codex")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
    {
        Some("openai")
    } else if m.starts_with("claude-") {
        Some("anthropic")
    } else if m.starts_with("gemini-") || m.starts_with("gemma-") {
        Some("google")
    } else if m.starts_with("deepseek") {
        Some("deepseek")
    } else if m.starts_with("kimi-") {
        Some("moonshot")
    } else if m.starts_with("minimax") {
        Some("minimax")
    } else if m.starts_with("glm-") {
        Some("zhipu")
    } else if m.starts_with("qwen-") || m.starts_with("qwq-") {
        Some("qwen")
    } else if m.starts_with("doubao") {
        Some("doubao")
    } else if m.starts_with("grok-") {
        Some("xai")
    } else if m.starts_with("mistral")
        || m.starts_with("codestral")
        || m.starts_with("pixtral")
        || m.starts_with("ministral")
    {
        Some("mistral")
    } else if m.starts_with("llama") {
        Some("meta")
    } else if m.starts_with("yi-") {
        Some("01ai")
    } else {
        None
    }
}

/// Cursor Composer / Auto published API-equivalent rates (not stored in pricing table).
fn cursor_composer_pricing(model: &str) -> Option<ModelPricing> {
    let m = model.to_lowercase();
    let rates: Option<(&str, f64, f64, f64, f64)> = if m.starts_with("composer-2.5-fast") {
        Some(("composer-2.5-fast", 3.0, 15.0, 0.30, 3.75))
    } else if m.starts_with("composer-2.5") {
        Some(("composer-2.5", 0.50, 2.50, 0.05, 0.625))
    } else if m.starts_with("composer-2") {
        Some(("composer-2", 1.50, 7.50, 0.15, 1.875))
    } else if m == "auto" || m.starts_with("auto-") || m == "default" {
        Some(("auto", 1.25, 6.0, 0.25, 0.0))
    } else {
        None
    };
    rates.map(|(name, inp, out, cr, cw)| ModelPricing {
        id: None,
        provider: "cursor".into(),
        model: name.into(),
        input_price_per_million: inp,
        output_price_per_million: out,
        reasoning_price_per_million: 0.0,
        cache_read_price_per_million: cr,
        cache_write_price_per_million: cw,
        currency: "USD".into(),
        effective_date: None,
        is_local: false,
        source: "builtin".into(),
        updated_at: String::new(),
    })
}

fn resolve_by_model_name(model: &str) -> Option<ModelPricing> {
    let m = normalize_aggregator_model(model);
    if m == "go" || m == "zen" || m == "big-pickle" {
        return get("local", "any");
    }
    if let Some(p) = cursor_composer_pricing(&m) {
        return Some(p);
    }
    if let Some((vendor, alias)) = cursor_model_alias(&m) {
        if let Some(p) = get(vendor, alias) {
            return Some(p);
        }
    }
    if let Some(vendor) = model_vendor_hint(&m) {
        if let Some(p) = get(vendor, &m) {
            return Some(p);
        }
    }
    let cache = cache().read();
    for ((prov, mod_name), row) in cache.iter() {
        if is_aggregator_provider(prov) {
            continue;
        }
        if mod_name == &m {
            return Some(row.clone());
        }
    }
    let mut best: Option<ModelPricing> = None;
    let mut best_len = 0usize;
    for ((prov, mod_name), row) in cache.iter() {
        if is_aggregator_provider(prov) {
            continue;
        }
        if m.starts_with(mod_name.as_str()) && mod_name.len() >= best_len {
            best_len = mod_name.len();
            best = Some(row.clone());
        }
    }
    best
}

/// Resolve pricing for a provider/model, with fallbacks for local runners.
pub fn resolve(provider: &str, model: &str) -> Option<ModelPricing> {
    if let Some(p) = get(provider, model) {
        return Some(p);
    }
    if let Some(p) = get(provider, "any") {
        return Some(p);
    }
    if is_aggregator_provider(provider) {
        return resolve_by_model_name(model);
    }
    if matches!(provider, "local" | "lmstudio") {
        return get("local", "any");
    }
    None
}

pub fn upsert(p: &ModelPricing) -> AppResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let id = db::with_conn_mut(|conn| {
        // Snapshot old values for history
        let prev: Option<ModelPricing> = conn
            .query_row(
                "SELECT id, provider, model, input_price_per_million, output_price_per_million,
                        reasoning_price_per_million, cache_read_price_per_million,
                        cache_write_price_per_million, currency, effective_date,
                        is_local, source, updated_at
                 FROM model_pricing WHERE provider = ?1 AND model = ?2",
                params![p.provider, p.model],
                |r| {
                    Ok(ModelPricing {
                        id: r.get(0)?,
                        provider: r.get(1)?,
                        model: r.get(2)?,
                        input_price_per_million: r.get(3)?,
                        output_price_per_million: r.get(4)?,
                        reasoning_price_per_million: r.get(5)?,
                        cache_read_price_per_million: r.get(6)?,
                        cache_write_price_per_million: r.get(7)?,
                        currency: r.get(8)?,
                        effective_date: r.get(9)?,
                        is_local: r.get::<_, i64>(10)? != 0,
                        source: r.get(11)?,
                        updated_at: r.get(12)?,
                    })
                },
            )
            .optional()?;

        if let Some(prev) = &prev {
            conn.execute(
                "INSERT INTO pricing_history
                 (pricing_id, provider, model, input_price_per_million, output_price_per_million,
                  reasoning_price_per_million, cache_read_price_per_million,
                  cache_write_price_per_million, currency, captured_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'))",
                params![
                    prev.id.unwrap(),
                    prev.provider,
                    prev.model,
                    prev.input_price_per_million,
                    prev.output_price_per_million,
                    prev.reasoning_price_per_million,
                    prev.cache_read_price_per_million,
                    prev.cache_write_price_per_million,
                    prev.currency,
                ],
            )?;
        }

        let is_local_i = if p.is_local { 1 } else { 0 };
        conn.execute(
            "INSERT INTO model_pricing
               (provider, model, input_price_per_million, output_price_per_million,
                reasoning_price_per_million, cache_read_price_per_million,
                cache_write_price_per_million, currency, effective_date,
                is_local, source, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(provider, model) DO UPDATE SET
               input_price_per_million=excluded.input_price_per_million,
               output_price_per_million=excluded.output_price_per_million,
               reasoning_price_per_million=excluded.reasoning_price_per_million,
               cache_read_price_per_million=excluded.cache_read_price_per_million,
               cache_write_price_per_million=excluded.cache_write_price_per_million,
               currency=excluded.currency,
               effective_date=excluded.effective_date,
               is_local=excluded.is_local,
               source=excluded.source,
               updated_at=excluded.updated_at",
            params![
                p.provider,
                p.model,
                p.input_price_per_million,
                p.output_price_per_million,
                p.reasoning_price_per_million,
                p.cache_read_price_per_million,
                p.cache_write_price_per_million,
                p.currency,
                p.effective_date,
                is_local_i,
                p.source,
                now,
            ],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM model_pricing WHERE provider = ?1 AND model = ?2",
            params![p.provider, p.model],
            |r| r.get(0),
        )?;
        Ok(id)
    })?;

    // Update cache
    let mut new_entry = p.clone();
    new_entry.id = Some(id);
    new_entry.updated_at = now;
    cache()
        .write()
        .insert((p.provider.clone(), p.model.clone()), new_entry);
    Ok(id)
}

pub fn delete(provider: &str, model: &str) -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute(
            "DELETE FROM model_pricing WHERE provider = ?1 AND model = ?2",
            params![provider, model],
        )?;
        Ok(())
    })?;
    cache().write().remove(&(provider.to_string(), model.to_string()));
    Ok(())
}

/// Whether a known pricing row covers this provider/model (exact or fallback).
pub fn is_resolved(provider: &str, model: &str) -> bool {
    resolve(provider, model).is_some()
}

/// Best-effort pricing when `missing_price_behavior` is `estimate`.
pub fn estimate_resolve(provider: &str, model: &str) -> Option<ModelPricing> {
    let cache = cache().read();
    let mut best: Option<ModelPricing> = None;
    let mut best_len = 0usize;
    for ((p, m), row) in cache.iter() {
        if p != provider || row.is_local {
            continue;
        }
        if model == m || model.starts_with(m.as_str()) || m.starts_with(model) {
            let len = m.len();
            if len >= best_len {
                best_len = len;
                best = Some(row.clone());
            }
        }
    }
    if best.is_some() {
        return best;
    }
    let rows: Vec<&ModelPricing> = cache
        .iter()
        .filter(|((p, _), r)| p == provider && !r.is_local)
        .map(|(_, r)| r)
        .collect();
    if rows.is_empty() {
        return None;
    }
    Some(average_pricing(provider, model, &rows))
}

fn average_pricing(provider: &str, model: &str, rows: &[&ModelPricing]) -> ModelPricing {
    let n = rows.len() as f64;
    let sum = |f: fn(&ModelPricing) -> f64| rows.iter().map(|r| f(r)).sum::<f64>() / n;
    ModelPricing {
        id: None,
        provider: provider.to_string(),
        model: model.to_string(),
        input_price_per_million: sum(|r| r.input_price_per_million),
        output_price_per_million: sum(|r| r.output_price_per_million),
        reasoning_price_per_million: sum(|r| r.reasoning_price_per_million),
        cache_read_price_per_million: sum(|r| r.cache_read_price_per_million),
        cache_write_price_per_million: sum(|r| r.cache_write_price_per_million),
        currency: rows[0].currency.clone(),
        effective_date: None,
        is_local: false,
        source: "estimate".into(),
        updated_at: String::new(),
    }
}

fn missing_price_behavior() -> String {
    settings::load_all()
        .map(|s| s.missing_price_behavior)
        .unwrap_or_else(|_| "warn".to_string())
}

/// Outcome of pricing an event — distinguishes exact, estimated, and missing rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostStatus {
    Priced,
    Estimated,
    Unpriced,
}

pub struct CostBreakdown {
    pub cost_usd: f64,
    pub status: CostStatus,
}

pub fn compute_cost_breakdown(
    provider: &str,
    model: &str,
    input: i64,
    output: i64,
    reasoning: i64,
    cache_read: i64,
    cache_write: i64,
) -> CostBreakdown {
    let resolved = resolve(provider, model);
    let estimated = resolved.is_none();
    let Some(p) = resolved.or_else(|| {
        if missing_price_behavior() == "estimate" {
            estimate_resolve(provider, model)
        } else {
            None
        }
    }) else {
        return CostBreakdown {
            cost_usd: 0.0,
            status: CostStatus::Unpriced,
        };
    };
    if p.is_local {
        return CostBreakdown {
            cost_usd: 0.0,
            status: CostStatus::Priced,
        };
    }
    let status = if estimated {
        CostStatus::Estimated
    } else {
        CostStatus::Priced
    };
    CostBreakdown {
        cost_usd: cost_from_pricing(&p, input, output, reasoning, cache_read, cache_write),
        status,
    }
}

fn cost_from_pricing(
    p: &ModelPricing,
    input: i64,
    output: i64,
    reasoning: i64,
    cache_read: i64,
    cache_write: i64,
) -> f64 {
    let input = input.max(0);
    let output = output.max(0);
    let reasoning = reasoning.max(0);
    let cache_read = cache_read.max(0);
    let cache_write = cache_write.max(0);

    let non_cached_input = (input - cache_read).max(0);

    let (billable_output, billable_reasoning) =
        if p.reasoning_price_per_million > 0.0 && reasoning > 0 && reasoning <= output {
            (output - reasoning, reasoning)
        } else {
            (output, 0)
        };

    let cost = (non_cached_input as f64) * p.input_price_per_million
        + (billable_output as f64) * p.output_price_per_million
        + (billable_reasoning as f64) * p.reasoning_price_per_million
        + (cache_read as f64) * p.cache_read_price_per_million
        + (cache_write as f64) * p.cache_write_price_per_million;
    cost / 1_000_000.0
}

/// Compute cost for a usage event, given provider/model.
pub fn compute_cost(
    provider: &str,
    model: &str,
    input: i64,
    output: i64,
    reasoning: i64,
    cache_read: i64,
    cache_write: i64,
) -> f64 {
    compute_cost_breakdown(provider, model, input, output, reasoning, cache_read, cache_write).cost_usd
}

/// SQL fragment: event has no assignable cost after ingest/recalculate.
/// Aggregator providers (Cursor, OpenCode Go, Copilot, etc.) resolve via underlying
/// vendor models and never get direct `model_pricing` rows — so we key off stored
/// cost/exactness instead of a bare JOIN to `model_pricing`.
pub fn unpriced_event_sql(event_alias: &str) -> String {
    format!(
        "{event_alias}.provider IS NOT NULL AND {event_alias}.model IS NOT NULL
         AND {event_alias}.provider NOT IN ('local', 'lmstudio')
         AND COALESCE({event_alias}.cost_usd, 0) = 0
         AND {event_alias}.exactness = 'unknown'"
    )
}

/// Remove aggregator / reseller rows from the pricing table (first-party vendors only).
pub fn purge_aggregator_pricing() -> AppResult<()> {
    let providers = [
        "opencode-go",
        "github-copilot",
        "deepinfra",
        "together",
        "fireworks",
        "ollama-cloud",
        "nvidia",
        "cursor",
    ];
    db::with_conn_mut(|conn| {
        for p in providers {
            conn.execute("DELETE FROM model_pricing WHERE provider = ?1", params![p])?;
        }
        Ok(())
    })?;
    let mut cache = cache().write();
    cache.retain(|(prov, _), _| !is_aggregator_provider(prov));
    Ok(())
}

/// Seed JSON may include Cursor Composer rows; other aggregator providers are excluded.
fn is_seedable_provider(provider: &str) -> bool {
    provider == "cursor" || !is_aggregator_provider(provider)
}

/// Seed default pricing from `pricing/pricing-seed-2026.json`. Idempotent.
/// First-party models only — aggregator/reseller providers are excluded,
/// except Cursor Composer models which are Cursor-specific products.
pub fn seed_defaults() -> AppResult<i64> {
    purge_aggregator_pricing()?;
    let json = include_str!("../../../pricing/pricing-seed-2026.json");
    let all: Vec<ModelPricing> = serde_json::from_str(json)
        .map_err(|e| AppError::Internal(format!("Failed to parse pricing seed JSON: {e}")))?;
    let mut inserted = 0i64;
    for p in &all {
        if !is_seedable_provider(&p.provider) {
            continue;
        }
        // Check SQLite — not the in-memory cache (empty until prime_cache runs).
        if !exists_in_db(&p.provider, &p.model)? {
            upsert(p)?;
            inserted += 1;
        }
    }
    if inserted > 0 {
        tracing::info!("Seeded {inserted} pricing row(s) from defaults");
    }
    Ok(inserted)
}

/// Insert any seed JSON rows that are missing from the DB (e.g. after a seed file update).
pub fn sync_seed_rows() -> AppResult<i64> {
    let n = seed_defaults()?;
    prime_cache()?;
    Ok(n)
}

/// Bulk upsert a set of pricing rows.
///
/// Each row goes through the same `upsert` path so pricing_history is recorded
/// exactly the same as a single-row edit. Rows with `effective_date == None`
/// or an empty `source` string are normalized so downstream code can rely on
/// non-null values.
///
/// Returns a summary of the operation so the UI can show "inserted N, updated M".
pub fn bulk_upsert(rows: &[ModelPricing]) -> AppResult<BulkImportSummary> {
    let mut summary = BulkImportSummary::default();
    summary.received = rows.len() as i64;
    for p in rows {
        // Sanity-check: refuse rows with empty provider/model to avoid silent
        // corruption of the unique key.
        if p.provider.trim().is_empty() || p.model.trim().is_empty() {
            summary.skipped += 1;
            summary.errors.push(format!(
                "skipping row with empty provider/model (provider={:?}, model={:?})",
                p.provider, p.model
            ));
            continue;
        }
        // Was this an update or an insert? Inspect the cache + DB before
        // delegating to `upsert` so we can report counts.
        let existed = get(&p.provider, &p.model).is_some();
        upsert(p)?;
        if existed {
            summary.updated += 1;
        } else {
            summary.inserted += 1;
        }
    }
    Ok(summary)
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct BulkImportSummary {
    pub received: i64,
    pub inserted: i64,
    pub updated: i64,
    pub skipped: i64,
    pub errors: Vec<String>,
}

/// Return (provider, model) pairs that appear in `usage_events` (not ignored)
/// but cannot be priced via direct rows or aggregator → first-party resolution.
/// Sorted by total tokens descending so the highest-impact missing models come first.
pub fn list_missing() -> AppResult<Vec<MissingPricingRow>> {
    let rows: Vec<MissingPricingRow> = db::with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT e.provider, e.model,
                    COUNT(*) AS events,
                    COALESCE(SUM(e.total_tokens), 0) AS total_tokens,
                    COALESCE(SUM(e.cost_usd), 0) AS current_cost_usd
             FROM usage_events e
             WHERE e.ignored = 0
               AND e.provider IS NOT NULL
               AND e.model IS NOT NULL
             GROUP BY e.provider, e.model
             ORDER BY total_tokens DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(MissingPricingRow {
                    provider: r.get(0)?,
                    model: r.get(1)?,
                    events: r.get(2)?,
                    total_tokens: r.get(3)?,
                    current_cost_usd: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;

    Ok(rows
        .into_iter()
        .filter(|r| !is_resolved(&r.provider, &r.model))
        .collect())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MissingPricingRow {
    pub provider: String,
    pub model: String,
    pub events: i64,
    pub total_tokens: i64,
    /// Cost currently recorded for events of this model (likely 0 since
    /// there's no pricing row). Useful so the AI can see "this missing model
    /// is silently costing you nothing in the dashboard".
    pub current_cost_usd: f64,
}

/// Recalculate costs for all events using current pricing.
pub fn recalculate_all() -> AppResult<i64> {
    use crate::collectors::cursor::normalize;
    use crate::types::UsageEvent;

    const BATCH_SIZE: i64 = 1000;
    let mut updated = 0i64;
    let mut offset = 0i64;

    loop {
        let batch: Vec<(i64, f64, String)> = db::with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, provider, model, input_tokens, output_tokens, reasoning_tokens,
                        cache_read_tokens, cache_write_tokens, raw_json
                 FROM usage_events
                 WHERE ignored = 0 AND provider IS NOT NULL AND model IS NOT NULL
                 ORDER BY id
                 LIMIT ?1 OFFSET ?2",
            )?;
            let mut rows = stmt.query(params![BATCH_SIZE, offset])?;
            let mut batch = Vec::new();
            while let Some(row) = rows.next()? {
                let id: i64 = row.get(0)?;
                let provider: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
                let model: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
                let input: i64 = row.get(3)?;
                let output: i64 = row.get(4)?;
                let reasoning: i64 = row.get(5)?;
                let cache_read: i64 = row.get(6)?;
                let cache_write: i64 = row.get(7)?;
                let raw_json: Option<String> = row.get(8)?;

                let (cost, exactness) = if provider == "cursor" {
                    if let Some(raw) = raw_json.as_deref() {
                        let mut ev = UsageEvent {
                            provider: Some(provider.clone()),
                            model: Some(model.clone()),
                            input_tokens: input,
                            output_tokens: output,
                            reasoning_tokens: reasoning,
                            cache_read_tokens: cache_read,
                            cache_write_tokens: cache_write,
                            ..UsageEvent::new(chrono::Utc::now())
                        };
                        if normalize::apply_cursor_cost_from_raw(&mut ev, raw) {
                            (ev.cost_usd, ev.exactness.as_str().to_string())
                        } else {
                            let b = compute_cost_breakdown(
                                &provider,
                                &model,
                                input,
                                output,
                                reasoning,
                                cache_read,
                                cache_write,
                            );
                            (b.cost_usd, exactness_for_status(b.status))
                        }
                    } else {
                        let b = compute_cost_breakdown(
                            &provider,
                            &model,
                            input,
                            output,
                            reasoning,
                            cache_read,
                            cache_write,
                        );
                        (b.cost_usd, exactness_for_status(b.status))
                    }
                } else {
                    let b = compute_cost_breakdown(
                        &provider,
                        &model,
                        input,
                        output,
                        reasoning,
                        cache_read,
                        cache_write,
                    );
                    (b.cost_usd, exactness_for_status(b.status))
                };
                batch.push((id, cost, exactness));
            }
            Ok(batch)
        })?;

        if batch.is_empty() {
            break;
        }
        let n = batch.len() as i64;
        updated += db::with_conn_mut(|conn| flush_cost_and_exactness_updates(conn, &batch))?;
        offset += n;
        if n < BATCH_SIZE {
            break;
        }
    }

    rebuild_session_cost_totals()?;
    let _ = crate::aggregation::rebuild_daily_usage();
    Ok(updated)
}

fn exactness_for_status(status: CostStatus) -> String {
    match status {
        CostStatus::Priced => Exactness::Exact.as_str().to_string(),
        CostStatus::Estimated => Exactness::Estimated.as_str().to_string(),
        CostStatus::Unpriced => Exactness::Unknown.as_str().to_string(),
    }
}

/// Re-sync `sessions.total_tokens` / `total_cost_usd` from usage_events.
pub fn rebuild_session_cost_totals() -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute_batch(
            "UPDATE sessions SET
                total_tokens = COALESCE((
                    SELECT SUM(e.total_tokens) FROM usage_events e
                    WHERE e.session_id = sessions.id AND e.ignored = 0
                ), 0),
                total_cost_usd = COALESCE((
                    SELECT SUM(e.cost_usd) FROM usage_events e
                    WHERE e.session_id = sessions.id AND e.ignored = 0
                ), 0);",
        )?;
        Ok(())
    })
}

fn flush_cost_and_exactness_updates(
    conn: &mut rusqlite::Connection,
    batch: &[(i64, f64, String)],
) -> AppResult<i64> {
    let tx = conn.unchecked_transaction()?;
    let mut updated = 0i64;
    for (id, cost, exactness) in batch {
        tx.execute(
            "UPDATE usage_events SET cost_usd = ?1, exactness = ?2 WHERE id = ?3",
            params![cost, exactness, id],
        )?;
        updated += 1;
    }
    tx.commit()?;
    Ok(updated)
}

fn flush_cost_updates(conn: &mut rusqlite::Connection, batch: &[(i64, f64)]) -> AppResult<i64> {
    let tx = conn.unchecked_transaction()?;
    let mut updated = 0i64;
    for (id, cost) in batch {
        tx.execute(
            "UPDATE usage_events SET cost_usd = ?1 WHERE id = ?2",
            params![cost, id],
        )?;
        updated += 1;
    }
    tx.commit()?;
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ModelPricing;

    fn seed(p: ModelPricing) {
        cache().write().insert((p.provider.clone(), p.model.clone()), p);
    }

    #[test]
    fn pricing_seed_includes_cursor_composers() {
        let json = include_str!("../../../pricing/pricing-seed-2026.json");
        let all: Vec<ModelPricing> =
            serde_json::from_str(json).expect("pricing seed JSON should parse");
        for model in [
            "composer-2.5-fast",
            "composer-2.5",
            "composer-2",
            "auto",
        ] {
            assert!(
                all.iter()
                    .any(|p| p.provider == "cursor" && p.model == model),
                "seed missing cursor/{model}"
            );
        }
    }

    #[test]
    fn pricing_seed_includes_opencode_free_models() {
        let json = include_str!("../../../pricing/pricing-seed-2026.json");
        let all: Vec<ModelPricing> =
            serde_json::from_str(json).expect("pricing seed JSON should parse");
        for model in [
            "qwen3.6-plus-free",
            "minimax-m2.5-free",
            "minimax-m3-free",
        ] {
            assert!(
                all.iter()
                    .any(|p| p.provider == "opencode" && p.model == model),
                "seed missing opencode/{model}"
            );
        }
    }

    #[test]
    fn cache_read_not_double_billed_at_input_rate() {
        seed(ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "gpt-4o".into(),
            input_price_per_million: 2.5,
            output_price_per_million: 10.0,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 1.25,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "test".into(),
            updated_at: String::new(),
        });
        let cost = compute_cost("openai", "gpt-4o", 1000, 500, 0, 600, 0);
        let expected = (400.0 * 2.5 + 600.0 * 1.25 + 500.0 * 10.0) / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-12, "got {cost}, expected {expected}");
    }

    #[test]
    fn reasoning_not_double_billed_when_output_includes_it() {
        seed(ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "o1-mini".into(),
            input_price_per_million: 3.0,
            output_price_per_million: 12.0,
            reasoning_price_per_million: 12.0,
            cache_read_price_per_million: 0.0,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "test".into(),
            updated_at: String::new(),
        });
        let cost = compute_cost("openai", "o1-mini", 100, 100, 40, 0, 0);
        let expected = (100.0 * 3.0 + 60.0 * 12.0 + 40.0 * 12.0) / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-12, "got {cost}, expected {expected}");
    }

    #[test]
    fn aggregator_resolves_to_first_party_model() {
        seed(ModelPricing {
            id: None,
            provider: "deepseek".into(),
            model: "deepseek-v4-flash".into(),
            input_price_per_million: 0.14,
            output_price_per_million: 0.28,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.0028,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "test".into(),
            updated_at: String::new(),
        });
        let p = resolve("opencode-go", "deepseek-v4-flash").unwrap();
        assert_eq!(p.provider, "deepseek");
        assert_eq!(p.model, "deepseek-v4-flash");
    }

    #[test]
    fn aggregator_normalizes_qwen3_alias() {
        seed(ModelPricing {
            id: None,
            provider: "qwen".into(),
            model: "qwen-3.6-plus".into(),
            input_price_per_million: 0.40,
            output_price_per_million: 2.40,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.0,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "test".into(),
            updated_at: String::new(),
        });
        let p = resolve("opencode-go", "qwen3.6-plus").unwrap();
        assert_eq!(p.model, "qwen-3.6-plus");
    }

    #[test]
    fn cursor_composer_uses_builtin_rates() {
        let p = resolve("cursor", "composer-2.5").unwrap();
        assert!((p.input_price_per_million - 0.50).abs() < 1e-9);
    }

    #[test]
    fn aggregator_kimi_not_counted_as_missing() {
        seed(ModelPricing {
            id: None,
            provider: "moonshot".into(),
            model: "kimi-k2.5".into(),
            input_price_per_million: 0.375,
            output_price_per_million: 1.90,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.07,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "test".into(),
            updated_at: String::new(),
        });
        assert!(is_resolved("opencode-go", "kimi-k2.5"));
        assert!(!is_resolved("opencode-go", "kimi-k9-unknown"));
    }
}
