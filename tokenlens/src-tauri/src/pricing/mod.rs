//! Pricing engine. Maintains a per-(provider, model) price table and computes
//! costs in USD (or configured currency).
//!
//! Cost formula:
//!   cost = (input * in_price + output * out_price + reasoning * r_price
//!         + cache_read * cr_price + cache_write * cw_price) / 1_000_000

use crate::db;
use crate::errors::{AppError, AppResult};
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

/// Compute cost for a usage event, given provider/model. Returns 0 for local
/// models even if pricing is missing.
pub fn compute_cost(
    provider: &str,
    model: &str,
    input: i64,
    output: i64,
    reasoning: i64,
    cache_read: i64,
    cache_write: i64,
) -> f64 {
    let Some(p) = get(provider, model) else {
        return 0.0;
    };
    if p.is_local {
        return 0.0;
    }
    let cost = (input as f64) * p.input_price_per_million
        + (output as f64) * p.output_price_per_million
        + (reasoning as f64) * p.reasoning_price_per_million
        + (cache_read as f64) * p.cache_read_price_per_million
        + (cache_write as f64) * p.cache_write_price_per_million;
    cost / 1_000_000.0
}

/// Seed default pricing for popular models. Idempotent.
pub fn seed_defaults() -> AppResult<()> {
    let defaults: &[ModelPricing] = &[
        // OpenAI (per 1M tokens, USD, as of 2025)
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "gpt-4o".into(),
            input_price_per_million: 2.50,
            output_price_per_million: 10.00,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 1.25,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            input_price_per_million: 0.15,
            output_price_per_million: 0.60,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.075,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "gpt-4.1".into(),
            input_price_per_million: 2.00,
            output_price_per_million: 8.00,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.50,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "gpt-4.1-mini".into(),
            input_price_per_million: 0.40,
            output_price_per_million: 1.60,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.10,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "o1".into(),
            input_price_per_million: 15.00,
            output_price_per_million: 60.00,
            reasoning_price_per_million: 60.00,
            cache_read_price_per_million: 7.50,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "o1-mini".into(),
            input_price_per_million: 3.00,
            output_price_per_million: 12.00,
            reasoning_price_per_million: 12.00,
            cache_read_price_per_million: 1.50,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "o3".into(),
            input_price_per_million: 10.00,
            output_price_per_million: 40.00,
            reasoning_price_per_million: 40.00,
            cache_read_price_per_million: 2.50,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "o3-mini".into(),
            input_price_per_million: 1.10,
            output_price_per_million: 4.40,
            reasoning_price_per_million: 4.40,
            cache_read_price_per_million: 0.55,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "openai".into(),
            model: "o4-mini".into(),
            input_price_per_million: 1.10,
            output_price_per_million: 4.40,
            reasoning_price_per_million: 4.40,
            cache_read_price_per_million: 0.275,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        // Anthropic
        ModelPricing {
            id: None,
            provider: "anthropic".into(),
            model: "claude-sonnet-4-5".into(),
            input_price_per_million: 3.00,
            output_price_per_million: 15.00,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.30,
            cache_write_price_per_million: 3.75,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "anthropic".into(),
            model: "claude-opus-4".into(),
            input_price_per_million: 15.00,
            output_price_per_million: 75.00,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 1.50,
            cache_write_price_per_million: 18.75,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "anthropic".into(),
            model: "claude-haiku-4".into(),
            input_price_per_million: 1.00,
            output_price_per_million: 5.00,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.10,
            cache_write_price_per_million: 1.25,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        // Google
        ModelPricing {
            id: None,
            provider: "google".into(),
            model: "gemini-2.5-pro".into(),
            input_price_per_million: 1.25,
            output_price_per_million: 10.00,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.31,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        ModelPricing {
            id: None,
            provider: "google".into(),
            model: "gemini-2.5-flash".into(),
            input_price_per_million: 0.075,
            output_price_per_million: 0.30,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.01875,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: false,
            source: "seed".into(),
            updated_at: String::new(),
        },
        // Local models — all $0
        ModelPricing {
            id: None,
            provider: "local".into(),
            model: "any".into(),
            input_price_per_million: 0.0,
            output_price_per_million: 0.0,
            reasoning_price_per_million: 0.0,
            cache_read_price_per_million: 0.0,
            cache_write_price_per_million: 0.0,
            currency: "USD".into(),
            effective_date: None,
            is_local: true,
            source: "seed".into(),
            updated_at: String::new(),
        },
    ];

    for p in defaults {
        if get(&p.provider, &p.model).is_none() {
            upsert(p)?;
        }
    }
    Ok(())
}

/// Recalculate costs for all events using current pricing.
pub fn recalculate_all() -> AppResult<i64> {
    db::with_conn_mut(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, provider, model, input_tokens, output_tokens, reasoning_tokens,
                    cache_read_tokens, cache_write_tokens
             FROM usage_events
             WHERE ignored = 0 AND provider IS NOT NULL AND model IS NOT NULL",
        )?;
        let rows: Vec<(i64, String, String, i64, i64, i64, i64, i64)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })?
            .filter_map(|x| x.ok())
            .collect();
        drop(stmt);

        let tx = conn.unchecked_transaction()?;
        let mut updated = 0i64;
        for (id, provider, model, input, output, reasoning, cache_read, cache_write) in &rows {
            let cost = compute_cost(provider, model, *input, *output, *reasoning, *cache_read, *cache_write);
            tx.execute(
                "UPDATE usage_events SET cost_usd = ?1 WHERE id = ?2",
                params![cost, id],
            )?;
            updated += 1;
        }
        tx.commit()?;
        Ok(updated)
    })
}
