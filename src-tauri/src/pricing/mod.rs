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
use crate::types::ModelPricing;
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
    if m == "auto" {
        return Some(("cursor", "auto"));
    }
    None
}

/// Resolve pricing for a provider/model, with fallbacks for local runners.
pub fn resolve(provider: &str, model: &str) -> Option<ModelPricing> {
    if let Some(p) = get(provider, model) {
        return Some(p);
    }
    if let Some(p) = get(provider, "any") {
        return Some(p);
    }
    if provider == "cursor" {
        if let Some((p, m)) = cursor_model_alias(model) {
            if let Some(pr) = get(p, m) {
                return Some(pr);
            }
        }
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

/// SQL fragment: event has no resolvable pricing row (cloud models only).
pub fn unpriced_event_sql(event_alias: &str) -> String {
    format!(
        "{event_alias}.provider IS NOT NULL AND {event_alias}.model IS NOT NULL
         AND {event_alias}.provider NOT IN ('local', 'lmstudio')
         AND NOT EXISTS (
           SELECT 1 FROM model_pricing mp
           WHERE (mp.provider = {event_alias}.provider AND mp.model = {event_alias}.model)
              OR (mp.provider = {event_alias}.provider AND mp.model = 'any')
         )"
    )
}

/// Seed default pricing for popular models. Idempotent.
pub fn seed_defaults() -> AppResult<()> {
    let defaults: &[ModelPricing] = &[
        // ── OpenAI ──────────────────────────────────────────────
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-5.5".into(), input_price_per_million: 5.0, output_price_per_million: 30.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.50, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-5.5-pro".into(), input_price_per_million: 30.0, output_price_per_million: 180.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-5.4".into(), input_price_per_million: 2.50, output_price_per_million: 15.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.25, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-5.4-pro".into(), input_price_per_million: 30.0, output_price_per_million: 180.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-5.4-mini".into(), input_price_per_million: 0.75, output_price_per_million: 4.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.075, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-5.4-nano".into(), input_price_per_million: 0.20, output_price_per_million: 1.25, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.02, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-4o".into(), input_price_per_million: 2.50, output_price_per_million: 10.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 1.25, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-4o-mini".into(), input_price_per_million: 0.15, output_price_per_million: 0.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.075, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-4.1".into(), input_price_per_million: 2.0, output_price_per_million: 8.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.50, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-4.1-mini".into(), input_price_per_million: 0.40, output_price_per_million: 1.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.10, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-4.1-nano".into(), input_price_per_million: 0.10, output_price_per_million: 0.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.025, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "o1".into(), input_price_per_million: 15.0, output_price_per_million: 60.0, reasoning_price_per_million: 60.0, cache_read_price_per_million: 7.50, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "o1-mini".into(), input_price_per_million: 3.0, output_price_per_million: 12.0, reasoning_price_per_million: 12.0, cache_read_price_per_million: 1.50, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "o3".into(), input_price_per_million: 10.0, output_price_per_million: 40.0, reasoning_price_per_million: 40.0, cache_read_price_per_million: 2.50, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "o3-mini".into(), input_price_per_million: 1.10, output_price_per_million: 4.40, reasoning_price_per_million: 4.40, cache_read_price_per_million: 0.55, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "o4-mini".into(), input_price_per_million: 1.10, output_price_per_million: 4.40, reasoning_price_per_million: 4.40, cache_read_price_per_million: 0.275, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "o3-deep-research".into(), input_price_per_million: 5.0, output_price_per_million: 20.0, reasoning_price_per_million: 20.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "o4-mini-deep-research".into(), input_price_per_million: 1.0, output_price_per_million: 4.0, reasoning_price_per_million: 4.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "gpt-5.3-codex".into(), input_price_per_million: 1.75, output_price_per_million: 14.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.175, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "openai".into(), model: "computer-use-preview".into(), input_price_per_million: 1.50, output_price_per_million: 6.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Anthropic ────────────────────────────────────────────
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-fable-5".into(), input_price_per_million: 10.0, output_price_per_million: 50.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 1.0, cache_write_price_per_million: 12.50, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-mythos-5".into(), input_price_per_million: 10.0, output_price_per_million: 50.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 1.0, cache_write_price_per_million: 12.50, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-opus-4.8".into(), input_price_per_million: 5.0, output_price_per_million: 25.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.50, cache_write_price_per_million: 6.25, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-opus-4.7".into(), input_price_per_million: 5.0, output_price_per_million: 25.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.50, cache_write_price_per_million: 6.25, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-opus-4.6".into(), input_price_per_million: 5.0, output_price_per_million: 25.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.50, cache_write_price_per_million: 6.25, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-opus-4.5".into(), input_price_per_million: 5.0, output_price_per_million: 25.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.50, cache_write_price_per_million: 6.25, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-opus-4.1".into(), input_price_per_million: 15.0, output_price_per_million: 75.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 1.50, cache_write_price_per_million: 18.75, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-opus-4".into(), input_price_per_million: 15.0, output_price_per_million: 75.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 1.50, cache_write_price_per_million: 18.75, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-sonnet-4.6".into(), input_price_per_million: 3.0, output_price_per_million: 15.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.30, cache_write_price_per_million: 3.75, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-sonnet-4.5".into(), input_price_per_million: 3.0, output_price_per_million: 15.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.30, cache_write_price_per_million: 3.75, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-sonnet-4".into(), input_price_per_million: 3.0, output_price_per_million: 15.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.30, cache_write_price_per_million: 3.75, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-haiku-4.5".into(), input_price_per_million: 1.0, output_price_per_million: 5.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.10, cache_write_price_per_million: 1.25, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "anthropic".into(), model: "claude-haiku-3.5".into(), input_price_per_million: 0.80, output_price_per_million: 4.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.08, cache_write_price_per_million: 1.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Google ──────────────────────────────────────────────
        ModelPricing { id: None, provider: "google".into(), model: "gemini-3.1-pro".into(), input_price_per_million: 2.0, output_price_per_million: 12.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.20, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-3-pro".into(), input_price_per_million: 2.0, output_price_per_million: 12.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.20, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-3.5-flash".into(), input_price_per_million: 1.50, output_price_per_million: 9.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.15, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-3-flash".into(), input_price_per_million: 0.50, output_price_per_million: 3.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.05, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-3.1-flash-lite".into(), input_price_per_million: 0.25, output_price_per_million: 1.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.025, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-2.5-pro".into(), input_price_per_million: 1.25, output_price_per_million: 10.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.125, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-2.5-flash".into(), input_price_per_million: 0.30, output_price_per_million: 2.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.03, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-2.5-flash-lite".into(), input_price_per_million: 0.10, output_price_per_million: 0.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.01, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-2.0-flash".into(), input_price_per_million: 0.10, output_price_per_million: 0.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.01, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "google".into(), model: "gemini-2.0-flash-lite".into(), input_price_per_million: 0.075, output_price_per_million: 0.30, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0075, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── DeepSeek ────────────────────────────────────────────
        ModelPricing { id: None, provider: "deepseek".into(), model: "deepseek-v4-flash".into(), input_price_per_million: 0.14, output_price_per_million: 0.28, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0028, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "deepseek".into(), model: "deepseek-v4-pro".into(), input_price_per_million: 0.435, output_price_per_million: 0.87, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.003625, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "deepseek".into(), model: "deepseek-chat".into(), input_price_per_million: 0.14, output_price_per_million: 0.28, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0028, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Mistral ─────────────────────────────────────────────
        ModelPricing { id: None, provider: "mistral".into(), model: "mistral-large".into(), input_price_per_million: 2.0, output_price_per_million: 6.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "mistral-large-latest".into(), input_price_per_million: 2.0, output_price_per_million: 6.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "mistral-medium".into(), input_price_per_million: 0.40, output_price_per_million: 2.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.04, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "mistral-medium-latest".into(), input_price_per_million: 0.40, output_price_per_million: 2.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.04, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "mistral-small".into(), input_price_per_million: 0.10, output_price_per_million: 0.30, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "mistral-small-latest".into(), input_price_per_million: 0.10, output_price_per_million: 0.30, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "codestral".into(), input_price_per_million: 0.30, output_price_per_million: 0.90, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "codestral-latest".into(), input_price_per_million: 0.30, output_price_per_million: 0.90, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "pixtral-large".into(), input_price_per_million: 2.0, output_price_per_million: 6.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "pixtral-large-latest".into(), input_price_per_million: 2.0, output_price_per_million: 6.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "ministral-3b".into(), input_price_per_million: 0.04, output_price_per_million: 0.04, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "ministral-8b".into(), input_price_per_million: 0.10, output_price_per_million: 0.10, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "mistral-nemo".into(), input_price_per_million: 0.15, output_price_per_million: 0.15, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "mixtral-8x22b".into(), input_price_per_million: 2.0, output_price_per_million: 6.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "magistral-medium".into(), input_price_per_million: 2.0, output_price_per_million: 5.0, reasoning_price_per_million: 5.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "mistral".into(), model: "magistral-small".into(), input_price_per_million: 0.50, output_price_per_million: 1.50, reasoning_price_per_million: 1.50, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── xAI / Grok ──────────────────────────────────────────
        ModelPricing { id: None, provider: "xai".into(), model: "grok-4.3".into(), input_price_per_million: 1.25, output_price_per_million: 2.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.20, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "xai".into(), model: "grok-4.20".into(), input_price_per_million: 2.0, output_price_per_million: 6.0, reasoning_price_per_million: 6.0, cache_read_price_per_million: 0.20, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "xai".into(), model: "grok-4".into(), input_price_per_million: 3.0, output_price_per_million: 15.0, reasoning_price_per_million: 15.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "xai".into(), model: "grok-4.1-fast".into(), input_price_per_million: 0.20, output_price_per_million: 0.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.05, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "xai".into(), model: "grok-3".into(), input_price_per_million: 2.0, output_price_per_million: 10.0, reasoning_price_per_million: 10.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "xai".into(), model: "grok-3-mini".into(), input_price_per_million: 0.30, output_price_per_million: 0.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.075, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Cohere ──────────────────────────────────────────────
        ModelPricing { id: None, provider: "cohere".into(), model: "command-a".into(), input_price_per_million: 2.50, output_price_per_million: 10.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "cohere".into(), model: "command-r-plus".into(), input_price_per_million: 2.50, output_price_per_million: 10.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "cohere".into(), model: "command-r".into(), input_price_per_million: 0.15, output_price_per_million: 0.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "cohere".into(), model: "command-r7b".into(), input_price_per_million: 0.0375, output_price_per_million: 0.15, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Subscription / zero-cost models ────────────────────
        ModelPricing { id: None, provider: "local".into(), model: "any".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "lmstudio".into(), model: "any".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode".into(), model: "go".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode".into(), model: "zen".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        // ── Moonshot AI / Kimi ──────────────────────────────
        ModelPricing { id: None, provider: "moonshot".into(), model: "kimi-k2.6".into(), input_price_per_million: 0.95, output_price_per_million: 4.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.16, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "moonshot".into(), model: "kimi-k2.5".into(), input_price_per_million: 0.375, output_price_per_million: 1.90, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.07, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "moonshot".into(), model: "kimi-k2".into(), input_price_per_million: 0.55, output_price_per_million: 2.20, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "moonshot".into(), model: "kimi-k2-thinking".into(), input_price_per_million: 0.60, output_price_per_million: 2.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── MiniMax ─────────────────────────────────────────
        ModelPricing { id: None, provider: "minimax".into(), model: "minimax-m3".into(), input_price_per_million: 0.60, output_price_per_million: 2.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.12, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "minimax".into(), model: "minimax-m2.7".into(), input_price_per_million: 0.30, output_price_per_million: 1.20, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.06, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "minimax".into(), model: "minimax-m2.5".into(), input_price_per_million: 0.27, output_price_per_million: 0.95, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "minimax".into(), model: "minimax-m2.1".into(), input_price_per_million: 0.27, output_price_per_million: 0.95, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "minimax".into(), model: "minimax-m2".into(), input_price_per_million: 0.26, output_price_per_million: 1.00, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Zhipu AI (GLM) ──────────────────────────────────
        ModelPricing { id: None, provider: "zhipu".into(), model: "glm-5.1".into(), input_price_per_million: 0.98, output_price_per_million: 3.08, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.182, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "zhipu".into(), model: "glm-5".into(), input_price_per_million: 1.00, output_price_per_million: 3.20, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "zhipu".into(), model: "glm-4.7".into(), input_price_per_million: 0.40, output_price_per_million: 1.75, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.08, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "zhipu".into(), model: "glm-4.7-flash".into(), input_price_per_million: 0.07, output_price_per_million: 0.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "zhipu".into(), model: "glm-4.5".into(), input_price_per_million: 0.40, output_price_per_million: 1.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "zhipu".into(), model: "glm-4.5-flash".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Qwen (Alibaba) ──────────────────────────────────
        ModelPricing { id: None, provider: "qwen".into(), model: "qwen-3.7-max".into(), input_price_per_million: 2.50, output_price_per_million: 7.50, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.50, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "qwen".into(), model: "qwen-3.7-plus".into(), input_price_per_million: 0.40, output_price_per_million: 1.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "qwen".into(), model: "qwen-3.6-plus".into(), input_price_per_million: 0.40, output_price_per_million: 2.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "qwen".into(), model: "qwen-3-max".into(), input_price_per_million: 1.20, output_price_per_million: 6.00, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.24, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "qwen".into(), model: "qwen-plus".into(), input_price_per_million: 0.40, output_price_per_million: 1.20, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "qwen".into(), model: "qwen-flash".into(), input_price_per_million: 0.05, output_price_per_million: 0.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "qwen".into(), model: "qwq-plus".into(), input_price_per_million: 0.80, output_price_per_million: 2.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Doubao (ByteDance) ──────────────────────────────
        ModelPricing { id: None, provider: "doubao".into(), model: "doubao-seed-2.0-pro".into(), input_price_per_million: 0.47, output_price_per_million: 2.37, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "doubao".into(), model: "doubao-seed-2.0-lite".into(), input_price_per_million: 0.09, output_price_per_million: 0.45, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "doubao".into(), model: "doubao-seed-2.0-mini".into(), input_price_per_million: 0.03, output_price_per_million: 0.29, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "doubao".into(), model: "doubao-pro-256k".into(), input_price_per_million: 0.735, output_price_per_million: 1.324, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "doubao".into(), model: "doubao-lite-128k".into(), input_price_per_million: 0.118, output_price_per_million: 0.147, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── 01.AI / Yi ──────────────────────────────────────
        ModelPricing { id: None, provider: "01ai".into(), model: "yi-lightning".into(), input_price_per_million: 0.99, output_price_per_million: 0.99, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "01ai".into(), model: "yi-large".into(), input_price_per_million: 2.75, output_price_per_million: 2.75, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── DeepInfra (third-party hoster) ──────────────────
        ModelPricing { id: None, provider: "deepinfra".into(), model: "deepseek-v4-flash".into(), input_price_per_million: 0.10, output_price_per_million: 0.20, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.02, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "deepinfra".into(), model: "deepseek-v4-pro".into(), input_price_per_million: 1.30, output_price_per_million: 2.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.10, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "deepinfra".into(), model: "llama-3.3-70b".into(), input_price_per_million: 0.23, output_price_per_million: 0.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "deepinfra".into(), model: "qwen-3-max".into(), input_price_per_million: 1.20, output_price_per_million: 6.00, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.24, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Together AI (third-party hoster) ────────────────
        ModelPricing { id: None, provider: "together".into(), model: "deepseek-v3.1".into(), input_price_per_million: 0.60, output_price_per_million: 1.70, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "together".into(), model: "qwen-3.6-plus".into(), input_price_per_million: 0.50, output_price_per_million: 3.00, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "together".into(), model: "gpt-oss-120b".into(), input_price_per_million: 0.15, output_price_per_million: 0.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Fireworks AI (third-party hoster) ───────────────
        ModelPricing { id: None, provider: "fireworks".into(), model: "llama-3.3-70b".into(), input_price_per_million: 0.90, output_price_per_million: 0.90, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "fireworks".into(), model: "deepseek-v4-pro".into(), input_price_per_million: 1.74, output_price_per_million: 3.48, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.145, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── OpenCode Go (subscription pass-through) ─────────
        ModelPricing { id: None, provider: "opencode-go".into(), model: "deepseek-v4-flash".into(), input_price_per_million: 0.14, output_price_per_million: 0.28, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0028, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "deepseek-v4-pro".into(), input_price_per_million: 0.435, output_price_per_million: 0.87, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.003625, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "minimax-m3".into(), input_price_per_million: 0.60, output_price_per_million: 2.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.12, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "minimax-m2.7".into(), input_price_per_million: 0.30, output_price_per_million: 1.20, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.06, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "kimi-k2.6".into(), input_price_per_million: 0.95, output_price_per_million: 4.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.16, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "kimi-k2.5".into(), input_price_per_million: 0.375, output_price_per_million: 1.90, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.07, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "glm-5.1".into(), input_price_per_million: 0.98, output_price_per_million: 3.08, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.182, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "qwen3.7-plus".into(), input_price_per_million: 0.40, output_price_per_million: 1.60, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "qwen3.6-plus".into(), input_price_per_million: 0.40, output_price_per_million: 2.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "mimo-v2.5".into(), input_price_per_million: 0.14, output_price_per_million: 0.28, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode-go".into(), model: "mimo-v2.5-pro".into(), input_price_per_million: 1.74, output_price_per_million: 3.48, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0145, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── OpenCode Zen free tiers ──────────────────────────
        ModelPricing { id: None, provider: "opencode".into(), model: "qwen3.6-plus-free".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode".into(), model: "minimax-m2.5-free".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode".into(), model: "deepseek-v4-flash-free".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode".into(), model: "minimax-m3-free".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode".into(), model: "nemotron-3-super-free".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "opencode".into(), model: "big-pickle".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── GitHub Copilot (subscription-bundled) ────────────
        ModelPricing { id: None, provider: "github-copilot".into(), model: "gpt-5.4-mini".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "github-copilot".into(), model: "gpt-5.3-codex".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "github-copilot".into(), model: "claude-sonnet-4.6".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "github-copilot".into(), model: "claude-haiku-4.5".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "github-copilot".into(), model: "gemini-3.1-pro-preview".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "github-copilot".into(), model: "gemini-3-flash-preview".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Ollama Cloud ──────────────────────────────────────
        ModelPricing { id: None, provider: "ollama-cloud".into(), model: "glm-5.1".into(), input_price_per_million: 0.98, output_price_per_million: 3.08, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.182, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "ollama-cloud".into(), model: "minimax-m3".into(), input_price_per_million: 0.60, output_price_per_million: 2.40, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.12, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── LM Studio (local models) ─────────────────────────
        ModelPricing { id: None, provider: "lmstudio".into(), model: "qwen3.5-2b".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "lmstudio".into(), model: "qwen3.5-9b".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "lmstudio".into(), model: "qwen3.6-35b-a3b-uncensored-hauhaucs-aggressive".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        ModelPricing { id: None, provider: "lmstudio".into(), model: "gemma-4-e2b-it".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
        // ── NVIDIA NIM ────────────────────────────────────────
        ModelPricing { id: None, provider: "nvidia".into(), model: "z-ai/glm5".into(), input_price_per_million: 0.60, output_price_per_million: 1.92, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: false, source: "seed".into(), updated_at: String::new() },
        // ── Google (local Gemma test) ────────────────────────
        ModelPricing { id: None, provider: "google".into(), model: "gemma-4-31b-it".into(), input_price_per_million: 0.0, output_price_per_million: 0.0, reasoning_price_per_million: 0.0, cache_read_price_per_million: 0.0, cache_write_price_per_million: 0.0, currency: "USD".into(), effective_date: None, is_local: true, source: "seed".into(), updated_at: String::new() },
    ];

    for p in defaults {
        if get(&p.provider, &p.model).is_none() {
            upsert(p)?;
        }
    }
    Ok(())
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
/// but have no row in `model_pricing`. Sorted by total tokens descending so
/// the highest-impact missing models come first.
pub fn list_missing() -> AppResult<Vec<MissingPricingRow>> {
    db::with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT e.provider, e.model,
                    COUNT(*) AS events,
                    COALESCE(SUM(e.total_tokens), 0) AS total_tokens,
                    COALESCE(SUM(e.cost_usd), 0) AS current_cost_usd
             FROM usage_events e
             LEFT JOIN model_pricing p
               ON p.provider = e.provider AND p.model = e.model
             WHERE e.ignored = 0
               AND e.provider IS NOT NULL
               AND e.model IS NOT NULL
               AND p.id IS NULL
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
    })
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
    const BATCH_SIZE: i64 = 1000;
    let mut updated = 0i64;
    let mut offset = 0i64;

    loop {
        let batch: Vec<(i64, f64)> = db::with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, provider, model, input_tokens, output_tokens, reasoning_tokens,
                        cache_read_tokens, cache_write_tokens
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
                let cost = compute_cost(
                    &provider,
                    &model,
                    input,
                    output,
                    reasoning,
                    cache_read,
                    cache_write,
                );
                batch.push((id, cost));
            }
            Ok(batch)
        })?;

        if batch.is_empty() {
            break;
        }
        let n = batch.len() as i64;
        updated += db::with_conn_mut(|conn| flush_cost_updates(conn, &batch))?;
        offset += n;
        if n < BATCH_SIZE {
            break;
        }
    }

    rebuild_session_cost_totals()?;
    let _ = crate::aggregation::rebuild_daily_usage();
    Ok(updated)
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
}
