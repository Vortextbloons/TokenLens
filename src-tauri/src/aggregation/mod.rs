//! Aggregation queries: KPIs, breakdowns, time series.
//!
//! Most queries are pure SQL over `usage_events` + `sessions` + `daily_usage`.

use crate::db;
use crate::errors::AppResult;
use crate::pricing;
use crate::types::{
    AnomalyHighlight, Breakdown, CacheEfficiencyPoint, CacheEfficiencyReport, CacheEfficiencyRow,
    ContextUtilizationPoint, ContextUtilizationReport, ContextUtilizationRow, ExactnessMix,
    OverviewStats, QueryFilter, Session, TimeseriesPoint,
};
use rusqlite::{params, OptionalExtension};
use std::collections::{HashMap, HashSet};

fn col(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

fn build_where_clauses(
    f: &QueryFilter,
    include_dates: bool,
    prefix: &str,
) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value as V;
    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<V> = Vec::new();
    clauses.push(format!("{} = 0", col(prefix, "ignored")));
    if include_dates {
        let ts = col(prefix, "timestamp");
        if let Some(d) = &f.start_date {
            clauses.push(format!("date({ts}) >= date(?1)"));
            args.push(V::Text(d.clone()));
        }
        if let Some(d) = &f.end_date {
            let ph = args.len() + 1;
            clauses.push(format!("date({ts}) <= date(?{ph})"));
            args.push(V::Text(d.clone()));
        }
    }
    if let Some(p) = f.project_id {
        let ph = args.len() + 1;
        clauses.push(format!("{} = ?{ph}", col(prefix, "project_id")));
        args.push(V::Integer(p));
    }
    if let Some(p) = &f.provider {
        let ph = args.len() + 1;
        clauses.push(format!("{} = ?{ph}", col(prefix, "provider")));
        args.push(V::Text(p.clone()));
    }
    if let Some(m) = &f.model {
        let ph = args.len() + 1;
        clauses.push(format!("{} = ?{ph}", col(prefix, "model")));
        args.push(V::Text(m.clone()));
    }
    if let Some(s) = f.source_id {
        let ph = args.len() + 1;
        clauses.push(format!("{} = ?{ph}", col(prefix, "source_id")));
        args.push(V::Integer(s));
    }
    if let Some(e) = &f.exactness {
        let ph = args.len() + 1;
        clauses.push(format!("{} = ?{ph}", col(prefix, "exactness")));
        args.push(V::Text(e.clone()));
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_sql, args)
}

/// Build a `WHERE` clause for the `daily_usage` table. The `daily_usage`
/// table is pre-aggregated by (date, provider, model, project_id) and has
/// no `ignored` / `source_id` / `exactness` columns, so those filters are
/// dropped on this path. Returns `None` if the user has set one of those
/// unsupported filters, signalling that the caller should fall back to the
/// `usage_events` scan.
fn build_where_daily(
    f: &QueryFilter,
    include_dates: bool,
) -> Option<(String, Vec<rusqlite::types::Value>)> {
    if f.source_id.is_some() || f.exactness.is_some() {
        return None;
    }
    use rusqlite::types::Value as V;
    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<V> = Vec::new();
    if include_dates {
        if let Some(d) = &f.start_date {
            clauses.push("date >= date(?1)".to_string());
            args.push(V::Text(d.clone()));
        }
        if let Some(d) = &f.end_date {
            let ph = args.len() + 1;
            clauses.push(format!("date <= date(?{ph})"));
            args.push(V::Text(d.clone()));
        }
    }
    if let Some(p) = f.project_id {
        let ph = args.len() + 1;
        clauses.push(format!("project_id = ?{ph}"));
        args.push(V::Integer(p));
    }
    if let Some(p) = &f.provider {
        let ph = args.len() + 1;
        clauses.push(format!("provider = ?{ph}"));
        args.push(V::Text(p.clone()));
    }
    if let Some(m) = &f.model {
        let ph = args.len() + 1;
        clauses.push(format!("model = ?{ph}"));
        args.push(V::Text(m.clone()));
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    Some((where_sql, args))
}

/// Full filter including the user's selected date range (charts, breakdowns).
fn build_where(f: &QueryFilter) -> (String, Vec<rusqlite::types::Value>) {
    build_where_clauses(f, true, "")
}

/// Dimension filters only — excludes start/end dates.
fn build_dimension_where(f: &QueryFilter) -> (String, Vec<rusqlite::types::Value>) {
    build_where_clauses(f, false, "")
}

/// Event-table filter with alias prefix (e.g. `e` for joins).
fn build_event_where(f: &QueryFilter) -> (String, Vec<rusqlite::types::Value>) {
    build_where_clauses(f, true, "e")
}

#[derive(Debug, Clone)]
struct SessionAggregate {
    session_id: i64,
    label: String,
    provider: Option<String>,
    model: Option<String>,
    last_seen_at: Option<String>,
    total_tokens: i64,
    cost_usd: f64,
    event_count: i64,
    model_switches: i64,
    peak_context_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
}

#[derive(Debug, Clone)]
struct DailyAggregate {
    date: String,
    total_tokens: i64,
    cost_usd: f64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    session_count: i64,
    event_count: i64,
    model_count: i64,
}

fn session_label(title: Option<String>, source_session_id: String) -> String {
    title.unwrap_or(source_session_id)
}

fn rolling_median(values: &[i64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = values.iter().map(|v| *v as f64).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn context_pct(peak_context_tokens: i64, context_window_tokens: Option<i64>) -> f64 {
    match context_window_tokens.filter(|v| *v > 0) {
        Some(limit) => (peak_context_tokens.max(0) as f64 / limit as f64) * 100.0,
        None => 0.0,
    }
}

fn cache_savings_for(provider: Option<&str>, model: Option<&str>, cache_read_tokens: i64) -> f64 {
    let (Some(provider), Some(model)) = (provider, model) else {
        return 0.0;
    };
    let Some(p) = pricing::resolve(provider, model) else {
        return 0.0;
    };
    let delta = (p.input_price_per_million - p.cache_read_price_per_million).max(0.0);
    (cache_read_tokens.max(0) as f64) * delta / 1_000_000.0
}

fn load_session_aggregates(filter: &QueryFilter) -> AppResult<Vec<SessionAggregate>> {
    let (where_sql, args) = build_event_where(filter);
    let q = format!(
        "SELECT s.id,
                s.title,
                s.source_session_id,
                COALESCE(MAX(e.provider), s.provider) AS provider,
                COALESCE(MAX(e.model), s.model) AS model,
                MAX(e.timestamp) AS last_seen_at,
                COALESCE(SUM(e.total_tokens), 0) AS total_tokens,
                COALESCE(SUM(e.cost_usd), 0.0) AS cost_usd,
                COUNT(*) AS event_count,
                COUNT(DISTINCT e.model) AS model_switches,
                COALESCE(MAX(e.input_tokens + e.cache_read_tokens + e.tool_tokens), 0) AS peak_context_tokens,
                COALESCE(SUM(e.cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(e.cache_write_tokens), 0) AS cache_write_tokens
         FROM sessions s
         INNER JOIN usage_events e ON e.session_id = s.id
         {where_sql}
         GROUP BY s.id
         ORDER BY last_seen_at ASC",
    );

    db::with_conn(|conn| {
        let mut stmt = conn.prepare(&q)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(SessionAggregate {
                    session_id: r.get(0)?,
                    label: session_label(r.get::<_, Option<String>>(1)?, r.get(2)?),
                    provider: r.get(3)?,
                    model: r.get(4)?,
                    last_seen_at: r.get(5)?,
                    total_tokens: r.get(6)?,
                    cost_usd: r.get(7)?,
                    event_count: r.get(8)?,
                    model_switches: r.get(9)?,
                    peak_context_tokens: r.get(10)?,
                    cache_read_tokens: r.get(11)?,
                    cache_write_tokens: r.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

fn load_daily_aggregates(filter: &QueryFilter) -> AppResult<Vec<DailyAggregate>> {
    let (where_sql, args) = build_event_where(filter);
    let q = format!(
        "SELECT date(e.timestamp) AS d,
                COALESCE(SUM(e.total_tokens), 0) AS total_tokens,
                COALESCE(SUM(e.cost_usd), 0.0) AS cost_usd,
                COALESCE(SUM(e.cache_read_tokens), 0) AS cache_read_tokens,
                COALESCE(SUM(e.cache_write_tokens), 0) AS cache_write_tokens,
                COUNT(DISTINCT e.session_id) AS session_count,
                COUNT(*) AS event_count,
                COUNT(DISTINCT e.model) AS model_count
         FROM usage_events e
         {where_sql}
         GROUP BY d
         ORDER BY d ASC",
    );

    db::with_conn(|conn| {
        let mut stmt = conn.prepare(&q)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(DailyAggregate {
                    date: r.get(0)?,
                    total_tokens: r.get(1)?,
                    cost_usd: r.get(2)?,
                    cache_read_tokens: r.get(3)?,
                    cache_write_tokens: r.get(4)?,
                    session_count: r.get(5)?,
                    event_count: r.get(6)?,
                    model_count: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn overview(filter: &QueryFilter) -> AppResult<OverviewStats> {
    let (where_sql, args) = build_where(filter);
    let (rolling_where_sql, rolling_args) = build_dimension_where(filter);
    // Use the frontend-provided local date for rolling windows so "today"
    // matches the user's calendar day, not UTC.  Falls back to the system
    // local date when the caller doesn't supply one.
    let local_today = filter
        .local_date
        .clone()
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    db::with_conn(|conn| {
        // Rolling windows (today/week/month/lifetime) are pre-aggregated in
        // `daily_usage`, so we can avoid the per-event scan even when the
        // DB has millions of rows. We fall back to the events scan if the
        // user has set filters that `daily_usage` doesn't carry
        // (source_id, exactness).
        let daily_rolls = build_where_daily(filter, false);
        let (rolling_tokens_today, rolling_tokens_week, rolling_tokens_month,
             rolling_cost_today, rolling_cost_week, rolling_cost_month,
             tokens_lifetime, cost_lifetime) = if let Some((d_where, d_args)) = daily_rolls {
            let q = format!(
                "SELECT
                    COALESCE(SUM(CASE WHEN date=?1 THEN total_tokens ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date>=date(?1,'-6 days') THEN total_tokens ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date>=date(?1,'-29 days') THEN total_tokens ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date=?1 THEN cost_usd ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date>=date(?1,'-6 days') THEN cost_usd ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date>=date(?1,'-29 days') THEN cost_usd ELSE 0 END), 0),
                    COALESCE(SUM(total_tokens), 0),
                    COALESCE(SUM(cost_usd), 0)
                 FROM daily_usage {d_where}",
            );
            let mut all_args: Vec<rusqlite::types::Value> = Vec::new();
            all_args.push(rusqlite::types::Value::Text(local_today.clone()));
            all_args.extend(d_args);
            let mut stmt = conn.prepare(&q)?;
            let row = stmt.query_row(rusqlite::params_from_iter(all_args.iter()), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, f64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, f64>(7)?,
                ))
            })?;
            drop(stmt);
            row
        } else {
            // Fallback: scan usage_events with the same CASE expressions.
            let q = format!(
                "SELECT
                    COALESCE(SUM(CASE WHEN date(timestamp)=?1 THEN total_tokens ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date(timestamp)>=date(?1,'-6 days') THEN total_tokens ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date(timestamp)>=date(?1,'-29 days') THEN total_tokens ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date(timestamp)=?1 THEN cost_usd ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date(timestamp)>=date(?1,'-6 days') THEN cost_usd ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN date(timestamp)>=date(?1,'-29 days') THEN cost_usd ELSE 0 END), 0),
                    COALESCE(SUM(total_tokens), 0),
                    COALESCE(SUM(cost_usd), 0)
                 FROM usage_events {rolling_where_sql}",
            );
            let mut all_args: Vec<rusqlite::types::Value> = Vec::new();
            all_args.push(rusqlite::types::Value::Text(local_today.clone()));
            all_args.extend(rolling_args);
            let mut stmt = conn.prepare(&q)?;
            let row = stmt.query_row(rusqlite::params_from_iter(all_args.iter()), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, f64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, f64>(7)?,
                ))
            })?;
            drop(stmt);
            row
        };
        let tokens_today = rolling_tokens_today;
        let tokens_week = rolling_tokens_week;
        let tokens_month = rolling_tokens_month;
        let cost_today = rolling_cost_today;
        let cost_week = rolling_cost_week;
        let cost_month = rolling_cost_month;

        // Period-scoped aggregates for the selected date range. Routed to
        // `daily_usage` when both endpoints are set and the user's
        // filters are daily-compatible, since the table is already
        // indexed by date.
        let daily_period = if filter.start_date.is_some() && filter.end_date.is_some() {
            build_where_daily(filter, true)
        } else {
            None
        };
        let (period_tokens, period_cost, in_t, out_t, reas_t) = if let Some((d_where, d_args)) = daily_period {
            let q = format!(
                "SELECT
                    COALESCE(SUM(total_tokens), 0),
                    COALESCE(SUM(cost_usd), 0),
                    COALESCE(SUM(input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(reasoning_tokens), 0)
                 FROM daily_usage {d_where}",
            );
            let mut stmt = conn.prepare(&q)?;
            let row = stmt.query_row(rusqlite::params_from_iter(d_args.iter()), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })?;
            drop(stmt);
            row
        } else {
            let q_period = format!(
                "SELECT
                    COALESCE(SUM(total_tokens), 0) AS period_tokens,
                    COALESCE(SUM(cost_usd), 0) AS period_cost,
                    COALESCE(SUM(input_tokens), 0) AS in_t,
                    COALESCE(SUM(output_tokens), 0) AS out_t,
                    COALESCE(SUM(reasoning_tokens), 0) AS reas_t,
                    COALESCE(SUM(cache_read_tokens), 0) AS cache_t
                 FROM usage_events {where_sql}",
            );
            let mut stmt = conn.prepare(&q_period)?;
            let row = stmt.query_row(rusqlite::params_from_iter(args.iter()), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })?;
            drop(stmt);
            row
        };

        // Previous-period aggregate (period-over-period delta).
        // The "previous" window has the same length as the selected range and
        // ends the day before start_date. If start_date is null (i.e. "all
        // time") or the dates can't be parsed, the prior window is empty and
        // both values stay 0. Routed through `daily_usage` for the same
        // reason as the period query: at most ~date-range rows scanned
        // instead of every event.
        let (prev_period_tokens, prev_period_cost_usd) = match (
            filter.start_date.as_deref(),
            filter.end_date.as_deref(),
        ) {
            (Some(start), Some(end)) => {
                if let Some((d_where, _)) = build_where_daily(filter, false) {
                    let prev_q = format!(
                        "SELECT
                            COALESCE(SUM(total_tokens), 0),
                            COALESCE(SUM(cost_usd), 0)
                         FROM daily_usage
                         WHERE date >= date(?1, '-' || (julianday(?2) - julianday(?1)) || ' days')
                           AND date <= date(?1, '-1 day') {d_where}"
                    );
                    conn.query_row(
                        &prev_q,
                        rusqlite::params![start, end],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)),
                    )?
                } else {
                    let prev_q = format!(
                        "SELECT
                            COALESCE(SUM(total_tokens), 0),
                            COALESCE(SUM(cost_usd), 0)
                         FROM usage_events
                         WHERE date(timestamp) >= date(?1, '-' || (julianday(?2) - julianday(?1)) || ' days')
                           AND date(timestamp) <= date(?1, '-1 day')"
                    );
                    conn.query_row(
                        &prev_q,
                        rusqlite::params![start, end],
                        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)),
                    )?
                }
            }
            _ => (0, 0.0),
        };

        // Most used model
        let q2 = format!(
            "SELECT model, COALESCE(SUM(total_tokens),0) AS s
             FROM usage_events {where_sql} AND model IS NOT NULL
             GROUP BY model ORDER BY s DESC LIMIT 1",
        );
        let most_used = conn
            .query_row(
                &q2,
                rusqlite::params_from_iter(args.iter()),
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;

        // Most expensive model
        let q3 = format!(
            "SELECT model, COALESCE(SUM(cost_usd),0) AS s
             FROM usage_events {where_sql} AND model IS NOT NULL
             GROUP BY model ORDER BY s DESC LIMIT 1",
        );
        let most_exp = conn
            .query_row(
                &q3,
                rusqlite::params_from_iter(args.iter()),
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, f64>(1)?)),
            )
            .optional()?;

        // Largest session within the active filter window.
        let q4 = format!(
            "SELECT session_id, SUM(total_tokens) AS t
             FROM usage_events {where_sql} AND session_id IS NOT NULL
             GROUP BY session_id ORDER BY t DESC LIMIT 1",
        );
        let largest = conn
            .query_row(
                &q4,
                rusqlite::params_from_iter(args.iter()),
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;

        // Sessions count
        let q5 = format!("SELECT COUNT(DISTINCT session_id) FROM usage_events {where_sql}");
        let sessions_count: i64 = conn.query_row(
            &q5,
            rusqlite::params_from_iter(args.iter()),
            |r| r.get(0),
        )?;

        // Avg tokens / session within the selected date range.
        let avg: f64 = if sessions_count > 0 {
            (period_tokens.max(0) as f64) / (sessions_count as f64)
        } else {
            0.0
        };

        // Reasoning %
        let total = (in_t + out_t).max(1) as f64;
        let reasoning_pct = (reas_t as f64) / total * 100.0;

        // I/O ratio
        let io_ratio = if out_t > 0 {
            in_t as f64 / out_t as f64
        } else {
            0.0
        };

        // Cache savings: per-model (input_rate - cache_rate) * cache_read tokens.
        let q_cache = format!(
            "SELECT COALESCE(SUM(
                e.cache_read_tokens * MAX(0.0,
                    COALESCE(mp.input_price_per_million, 0) - COALESCE(mp.cache_read_price_per_million, 0)
                ) / 1000000.0
             ), 0.0)
             FROM usage_events e
             LEFT JOIN model_pricing mp ON mp.provider = e.provider AND mp.model = e.model
             {where_sql}",
        );
        let cache_savings: f64 = conn.query_row(
            &q_cache,
            rusqlite::params_from_iter(args.iter()),
            |r| r.get(0),
        )?;

        let unpriced_sql = pricing::unpriced_event_sql("e");
        let q_unpriced = format!(
            "SELECT COUNT(*), COALESCE(SUM(e.total_tokens), 0)
             FROM usage_events e
             {where_sql} AND ({unpriced_sql})",
        );
        let (unpriced_events, unpriced_tokens): (i64, i64) = conn.query_row(
            &q_unpriced,
            rusqlite::params_from_iter(args.iter()),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        // Exactness mix
        let q6 = format!(
            "SELECT exactness, COUNT(*) FROM usage_events {where_sql} GROUP BY exactness",
        );
        let mut stmt = conn.prepare(&q6)?;
        let mut mix = ExactnessMix::default();
        let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (k, v) = row?;
            match k.as_str() {
                "exact" => mix.exact = v,
                "estimated" => mix.estimated = v,
                "mixed" => mix.mixed = v,
                _ => mix.unknown = v,
            }
        }

        Ok(OverviewStats {
            tokens_lifetime,
            cost_lifetime_usd: cost_lifetime,
            tokens_today,
            tokens_week,
            tokens_month,
            cost_today_usd: cost_today,
            cost_week_usd: cost_week,
            cost_month_usd: cost_month,
            most_used_model: most_used.and_then(|(m, _)| m),
            most_expensive_model: most_exp.and_then(|(m, _)| m),
            largest_session_id: largest.map(|(id, _)| id),
            largest_session_tokens: largest.map(|(_, t)| t).unwrap_or(0),
            avg_tokens_per_session: avg,
            input_output_ratio: io_ratio,
            reasoning_token_pct: reasoning_pct,
            cache_savings_usd: cache_savings,
            sessions_count,
            unpriced_events,
            unpriced_tokens,
            exactness_mix: mix,
            period_tokens,
            period_cost_usd: period_cost,
            prev_period_tokens,
            prev_period_cost_usd,
        })
    })
}

pub fn timeseries(filter: &QueryFilter) -> AppResult<Vec<TimeseriesPoint>> {
    let (where_sql, args) = build_where(filter);
    let q = format!(
        "SELECT date(timestamp) AS d,
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(reasoning_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_write_tokens), 0),
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM(cost_usd), 0)
          FROM usage_events {where_sql}
          GROUP BY d ORDER BY d ASC",
    );
    db::with_conn(|conn| {
        let mut stmt = conn.prepare(&q)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(TimeseriesPoint {
                    date: r.get(0)?,
                    input_tokens: r.get(1)?,
                    output_tokens: r.get(2)?,
                    reasoning_tokens: r.get(3)?,
                    cache_read_tokens: r.get(4)?,
                    cache_write_tokens: r.get(5)?,
                    total_tokens: r.get(6)?,
                    cost_usd: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn breakdown_by(
    filter: &QueryFilter,
    dimension: &str, // "model" | "provider" | "project" | "source" | "exactness"
) -> AppResult<Vec<Breakdown>> {
    let (where_sql, args) = build_where(filter);
    let (col, label) = match dimension {
        "model" => ("model", "model"),
        "provider" => ("provider", "provider"),
        "project" => ("project_id", "project"),
        "source" => ("source_id", "source"),
        "exactness" => ("exactness", "exactness"),
        other => {
            return Err(crate::errors::AppError::Invalid(format!(
                "unknown dimension: {other}"
            )))
        }
    };
    let q = format!(
        "SELECT COALESCE(CAST({col} AS TEXT), '(none)') AS k,
                COALESCE(SUM(total_tokens), 0) AS tt,
                COALESCE(SUM(input_tokens), 0) AS it,
                COALESCE(SUM(output_tokens), 0) AS ot,
                COALESCE(SUM(cost_usd), 0) AS ct,
                COUNT(DISTINCT session_id) AS sc
         FROM usage_events {where_sql}
         GROUP BY k
         ORDER BY tt DESC
         LIMIT 50",
    );
    let _ = label;
    db::with_conn(|conn| {
        let mut stmt = conn.prepare(&q)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(Breakdown {
                    key: r.get(0)?,
                    total_tokens: r.get(1)?,
                    input_tokens: r.get(2)?,
                    output_tokens: r.get(3)?,
                    cost_usd: r.get(4)?,
                    sessions_count: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn cache_efficiency(filter: &QueryFilter) -> AppResult<CacheEfficiencyReport> {
    #[derive(Default)]
    struct Agg {
        cache_read_tokens: i64,
        cache_write_tokens: i64,
        total_tokens: i64,
        cost_usd: f64,
        cache_savings_usd: f64,
        sessions: HashSet<i64>,
    }

    let (where_sql, args) = build_where(filter);
    let q = format!(
        "SELECT date(e.timestamp) AS d,
                COALESCE(e.provider, '(none)') AS provider,
                COALESCE(e.model, '(none)') AS model,
                e.session_id,
                COALESCE(e.cache_read_tokens, 0) AS cache_read_tokens,
                COALESCE(e.cache_write_tokens, 0) AS cache_write_tokens,
                COALESCE(e.total_tokens, 0) AS total_tokens,
                COALESCE(e.cost_usd, 0.0) AS cost_usd
         FROM usage_events e
         {where_sql}
         ORDER BY d ASC, provider ASC, model ASC",
    );

    db::with_conn(|conn| {
        let mut stmt = conn.prepare(&q)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, f64>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut series: HashMap<String, Agg> = HashMap::new();
        let mut by_provider: HashMap<String, Agg> = HashMap::new();
        let mut by_model: HashMap<String, Agg> = HashMap::new();

        for (date, provider, model, session_id, cache_read, cache_write, total_tokens, cost_usd) in rows {
            let savings = cache_savings_for(Some(provider.as_str()), Some(model.as_str()), cache_read);
            let entry = series.entry(date).or_default();
            entry.cache_read_tokens += cache_read;
            entry.cache_write_tokens += cache_write;
            entry.total_tokens += total_tokens;
            entry.cost_usd += cost_usd;
            entry.cache_savings_usd += savings;
            if let Some(session_id) = session_id {
                entry.sessions.insert(session_id);
            }

            let entry = by_provider.entry(provider).or_default();
            entry.cache_read_tokens += cache_read;
            entry.cache_write_tokens += cache_write;
            entry.total_tokens += total_tokens;
            entry.cost_usd += cost_usd;
            entry.cache_savings_usd += savings;
            if let Some(session_id) = session_id {
                entry.sessions.insert(session_id);
            }

            let entry = by_model.entry(model).or_default();
            entry.cache_read_tokens += cache_read;
            entry.cache_write_tokens += cache_write;
            entry.total_tokens += total_tokens;
            entry.cost_usd += cost_usd;
            entry.cache_savings_usd += savings;
            if let Some(session_id) = session_id {
                entry.sessions.insert(session_id);
            }
        }

        let mut series_out: Vec<CacheEfficiencyPoint> = series
            .into_iter()
            .map(|(date, a)| CacheEfficiencyPoint {
                date,
                cache_read_tokens: a.cache_read_tokens,
                cache_write_tokens: a.cache_write_tokens,
                cache_savings_usd: a.cache_savings_usd,
            })
            .collect();
        series_out.sort_by(|a, b| a.date.cmp(&b.date));

        let mut provider_out: Vec<CacheEfficiencyRow> = by_provider
            .into_iter()
            .map(|(key, a)| CacheEfficiencyRow {
                key,
                cache_read_tokens: a.cache_read_tokens,
                cache_write_tokens: a.cache_write_tokens,
                cache_savings_usd: a.cache_savings_usd,
                total_tokens: a.total_tokens,
                cost_usd: a.cost_usd,
                sessions_count: a.sessions.len() as i64,
            })
            .collect();
        provider_out.sort_by(|a, b| b.cache_savings_usd.total_cmp(&a.cache_savings_usd));

        let mut model_out: Vec<CacheEfficiencyRow> = by_model
            .into_iter()
            .map(|(key, a)| CacheEfficiencyRow {
                key,
                cache_read_tokens: a.cache_read_tokens,
                cache_write_tokens: a.cache_write_tokens,
                cache_savings_usd: a.cache_savings_usd,
                total_tokens: a.total_tokens,
                cost_usd: a.cost_usd,
                sessions_count: a.sessions.len() as i64,
            })
            .collect();
        model_out.sort_by(|a, b| b.cache_savings_usd.total_cmp(&a.cache_savings_usd));

        Ok(CacheEfficiencyReport {
            series: series_out,
            by_provider: provider_out,
            by_model: model_out,
        })
    })
}

pub fn anomaly_highlights(filter: &QueryFilter) -> AppResult<Vec<AnomalyHighlight>> {
    let sessions = load_session_aggregates(filter)?;
    let daily = load_daily_aggregates(filter)?;
    const WINDOW: usize = 7;

    let mut out: Vec<AnomalyHighlight> = Vec::new();

    for (idx, s) in sessions.iter().enumerate() {
        if idx < WINDOW {
            continue;
        }
        let window = &sessions[(idx - WINDOW)..idx];
        let baseline = rolling_median(&window.iter().map(|x| x.total_tokens).collect::<Vec<_>>());
        if baseline <= 0.0 {
            continue;
        }
        let ratio = s.total_tokens as f64 / baseline;
        if ratio < 2.0 {
            continue;
        }
        let limit = pricing::context_window_tokens(s.provider.as_deref().unwrap_or(""), s.model.as_deref().unwrap_or(""));
        let peak_pct = context_pct(s.peak_context_tokens, limit);
        let reason = if s.model_switches > 1 {
            "model switching"
        } else if peak_pct >= 80.0 {
            "context saturation"
        } else if s.cache_write_tokens > s.cache_read_tokens && s.cache_write_tokens > 0 {
            "cache rebuilds"
        } else if s.event_count >= 10 {
            "retry-heavy"
        } else {
            "volume spike"
        };
        out.push(AnomalyHighlight {
            kind: "session".into(),
            label: s.label.clone(),
            session_id: Some(s.session_id),
            date: s.last_seen_at.clone().map(|v| v.chars().take(10).collect()),
            provider: s.provider.clone(),
            model: s.model.clone(),
            total_tokens: s.total_tokens,
            baseline_tokens: baseline.round() as i64,
            ratio,
            event_count: s.event_count,
            model_switches: s.model_switches,
            peak_context_tokens: s.peak_context_tokens,
            peak_context_pct: peak_pct,
            cache_read_tokens: s.cache_read_tokens,
            cache_write_tokens: s.cache_write_tokens,
            cost_usd: s.cost_usd,
            reason: reason.into(),
        });
    }

    for (idx, d) in daily.iter().enumerate() {
        if idx < WINDOW {
            continue;
        }
        let window = &daily[(idx - WINDOW)..idx];
        let baseline = rolling_median(&window.iter().map(|x| x.total_tokens).collect::<Vec<_>>());
        if baseline <= 0.0 {
            continue;
        }
        let ratio = d.total_tokens as f64 / baseline;
        if ratio < 2.0 {
            continue;
        }
        let reason = if d.cache_write_tokens > d.cache_read_tokens && d.cache_write_tokens > 0 {
            "cache churn"
        } else if d.session_count >= 10 {
            "session surge"
        } else if d.model_count > 1 {
            "model switching"
        } else {
            "volume spike"
        };
        out.push(AnomalyHighlight {
            kind: "day".into(),
            label: d.date.clone(),
            session_id: None,
            date: Some(d.date.clone()),
            provider: None,
            model: None,
            total_tokens: d.total_tokens,
            baseline_tokens: baseline.round() as i64,
            ratio,
            event_count: d.event_count,
            model_switches: d.model_count,
            peak_context_tokens: 0,
            peak_context_pct: 0.0,
            cache_read_tokens: d.cache_read_tokens,
            cache_write_tokens: d.cache_write_tokens,
            cost_usd: d.cost_usd,
            reason: reason.into(),
        });
    }

    out.sort_by(|a, b| b.ratio.total_cmp(&a.ratio).then(b.total_tokens.cmp(&a.total_tokens)));
    out.truncate(12);
    Ok(out)
}

pub fn context_utilization(filter: &QueryFilter) -> AppResult<ContextUtilizationReport> {
    let sessions = load_session_aggregates(filter)?;
    let all_rows: Vec<ContextUtilizationRow> = sessions
        .into_iter()
        .map(|s| {
            let limit = pricing::context_window_tokens(s.provider.as_deref().unwrap_or(""), s.model.as_deref().unwrap_or(""));
            ContextUtilizationRow {
                session_id: s.session_id,
                label: s.label,
                provider: s.provider,
                model: s.model,
                last_seen_at: s.last_seen_at,
                peak_context_tokens: s.peak_context_tokens,
                context_window_tokens: limit,
                utilization_pct: context_pct(s.peak_context_tokens, limit),
                event_count: s.event_count,
                model_switches: s.model_switches,
                cost_usd: s.cost_usd,
            }
        })
        .collect();
    let mut rows = all_rows.clone();
    rows.sort_by(|a, b| b.utilization_pct.total_cmp(&a.utilization_pct).then(b.peak_context_tokens.cmp(&a.peak_context_tokens)));
    rows.truncate(100);

    let mut buckets: HashMap<String, (Vec<f64>, i64)> = HashMap::new();
    for row in &all_rows {
        let Some(limit) = row.context_window_tokens.filter(|v| *v > 0) else {
            continue;
        };
        let date = row
            .last_seen_at
            .as_deref()
            .and_then(|s| s.get(0..10))
            .unwrap_or("unknown")
            .to_string();
        let bucket = buckets.entry(date).or_insert_with(|| (Vec::new(), 0));
        bucket.0.push((row.peak_context_tokens.max(0) as f64 / limit as f64) * 100.0);
        if row.utilization_pct >= 80.0 {
            bucket.1 += 1;
        }
    }

    let mut trend: Vec<ContextUtilizationPoint> = buckets
        .into_iter()
        .map(|(date, (vals, over_80))| {
            let avg = if vals.is_empty() { 0.0 } else { vals.iter().sum::<f64>() / vals.len() as f64 };
            let max = vals.into_iter().fold(0.0, f64::max);
            ContextUtilizationPoint {
                date,
                avg_utilization_pct: avg,
                max_utilization_pct: max,
                sessions_over_80: over_80,
            }
        })
        .collect();
    trend.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(ContextUtilizationReport { sessions: rows, trend })
}

pub fn list_sessions(filter: &QueryFilter) -> AppResult<Vec<Session>> {
    let (where_sql, mut args) = build_event_where(filter);

    let mut extra = Vec::new();
    if let Some(p) = filter.project_id {
        let ph = args.len() + 1;
        extra.push(format!("s.project_id = ?{ph}"));
        args.push(rusqlite::types::Value::Integer(p));
    }

    let extra_sql = if extra.is_empty() {
        String::new()
    } else {
        format!(" AND {}", extra.join(" AND "))
    };

    let mut q = format!(
        "SELECT s.id, s.source_session_id, s.source_id, s.project_id, s.title,
                MIN(e.timestamp) AS started_at,
                MAX(e.timestamp) AS last_seen_at,
                COALESCE(MAX(e.provider), s.provider) AS provider,
                COALESCE(MAX(e.model), s.model) AS model,
                COALESCE(SUM(e.total_tokens), 0) AS total_tokens,
                COALESCE(SUM(e.cost_usd), 0.0) AS total_cost_usd,
                s.exactness, s.raw_ref
         FROM usage_events e
         INNER JOIN sessions s ON s.id = e.session_id
         {where_sql}{extra_sql}
         GROUP BY s.id
         ORDER BY last_seen_at DESC",
    );

    if let Some(lim) = filter.limit {
        q.push_str(&format!(" LIMIT {lim}"));
    } else {
        q.push_str(" LIMIT 500");
    }
    if let Some(off) = filter.offset {
        q.push_str(&format!(" OFFSET {off}"));
    }

    db::with_conn(|conn| {
        let mut stmt = conn.prepare(&q)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                Ok(Session {
                    id: r.get(0)?,
                    source_session_id: r.get(1)?,
                    source_id: r.get::<_, Option<i64>>(2)?,
                    project_id: r.get::<_, Option<i64>>(3)?,
                    title: r.get(4)?,
                    started_at: r.get(5)?,
                    last_seen_at: r.get(6)?,
                    provider: r.get(7)?,
                    model: r.get(8)?,
                    total_tokens: r.get(9)?,
                    total_cost_usd: r.get(10)?,
                    exactness: r.get(11)?,
                    raw_ref: r.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn session_detail(session_id: i64) -> AppResult<Option<Session>> {
    db::with_conn(|conn| {
        let r = conn
            .query_row(
                "SELECT s.id, s.source_session_id, s.source_id, s.project_id, s.title,
                        MIN(e.timestamp) AS started_at,
                        MAX(e.timestamp) AS last_seen_at,
                        COALESCE(MAX(e.provider), s.provider) AS provider,
                        COALESCE(MAX(e.model), s.model) AS model,
                        COALESCE(SUM(e.total_tokens), 0) AS total_tokens,
                        COALESCE(SUM(e.cost_usd), 0.0) AS total_cost_usd,
                        s.exactness, s.raw_ref
                 FROM sessions s
                 LEFT JOIN usage_events e ON e.session_id = s.id AND e.ignored = 0
                 WHERE s.id = ?1
                 GROUP BY s.id",
                params![session_id],
                |r| {
                    Ok(Session {
                        id: r.get(0)?,
                        source_session_id: r.get(1)?,
                        source_id: r.get::<_, Option<i64>>(2)?,
                        project_id: r.get::<_, Option<i64>>(3)?,
                        title: r.get(4)?,
                        started_at: r.get(5)?,
                        last_seen_at: r.get(6)?,
                        provider: r.get(7)?,
                        model: r.get(8)?,
                        total_tokens: r.get(9)?,
                        total_cost_usd: r.get(10)?,
                        exactness: r.get(11)?,
                        raw_ref: r.get(12)?,
                    })
                },
            )
            .optional()?;
        Ok(r)
    })
}

pub fn session_events(session_id: i64) -> AppResult<Vec<crate::types::UsageEvent>> {
    use crate::types::{Exactness, UsageEvent};
    db::with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, event_hash, timestamp, source_id, session_id, project_id,
                    event_type, provider, model, message_role,
                    input_tokens, output_tokens, reasoning_tokens,
                    cache_read_tokens, cache_write_tokens, tool_tokens,
                    total_tokens, cost_usd, exactness, confidence,
                    raw_json, raw_json_zstd, raw_source_path
             FROM usage_events
             WHERE session_id = ?1 AND ignored = 0
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id], |r| {
                let exact_str: String = r.get(18)?;
                let exactness: Exactness = exact_str.parse().unwrap_or_default();
                Ok(UsageEvent {
                    event_hash: r.get(1)?,
                    timestamp: chrono::DateTime::parse_from_rfc3339(&r.get::<_, String>(2)?)
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    source_id: r.get(3)?,
                    session_id: r.get(4)?,
                    project_id: r.get(5)?,
                    event_type: r.get(6)?,
                    provider: r.get(7)?,
                    model: r.get(8)?,
                    message_role: r.get(9)?,
                    input_tokens: r.get(10)?,
                    output_tokens: r.get(11)?,
                    reasoning_tokens: r.get(12)?,
                    cache_read_tokens: r.get(13)?,
                    cache_write_tokens: r.get(14)?,
                    tool_tokens: r.get(15)?,
                    total_tokens: r.get(16)?,
                    cost_usd: r.get(17)?,
                    exactness,
                    confidence: r.get(19)?,
                    raw_json: decode_raw_json(r.get(20)?, r.get(21)?),
                    raw_source_path: r.get(22)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn list_events(filter: &QueryFilter) -> AppResult<Vec<crate::types::UsageEvent>> {
    use crate::types::{Exactness, UsageEvent};
    let (where_sql, args) = build_where(filter);
    let mut q = format!(
        "SELECT id, event_hash, timestamp, source_id, session_id, project_id,
                event_type, provider, model, message_role,
                input_tokens, output_tokens, reasoning_tokens,
                cache_read_tokens, cache_write_tokens, tool_tokens,
                total_tokens, cost_usd, exactness, confidence,
                raw_json, raw_json_zstd, raw_source_path
         FROM usage_events {where_sql}
         ORDER BY timestamp DESC"
    );
    if let Some(lim) = filter.limit {
        q.push_str(&format!(" LIMIT {lim}"));
    } else {
        q.push_str(" LIMIT 500");
    }
    if let Some(off) = filter.offset {
        q.push_str(&format!(" OFFSET {off}"));
    }
    db::with_conn(|conn| {
        let mut stmt = conn.prepare(&q)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args.iter()), |r| {
                let exact_str: String = r.get(18)?;
                let exactness: Exactness = exact_str.parse().unwrap_or_default();
                Ok(UsageEvent {
                    event_hash: r.get(1)?,
                    timestamp: chrono::DateTime::parse_from_rfc3339(&r.get::<_, String>(2)?)
                        .map(|d| d.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now()),
                    source_id: r.get(3)?,
                    session_id: r.get(4)?,
                    project_id: r.get(5)?,
                    event_type: r.get(6)?,
                    provider: r.get(7)?,
                    model: r.get(8)?,
                    message_role: r.get(9)?,
                    input_tokens: r.get(10)?,
                    output_tokens: r.get(11)?,
                    reasoning_tokens: r.get(12)?,
                    cache_read_tokens: r.get(13)?,
                    cache_write_tokens: r.get(14)?,
                    tool_tokens: r.get(15)?,
                    total_tokens: r.get(16)?,
                    cost_usd: r.get(17)?,
                    exactness,
                    confidence: r.get(19)?,
                    raw_json: decode_raw_json(r.get(20)?, r.get(21)?),
                    raw_source_path: r.get(22)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Pick the preferred representation of `raw_json` for a row.
///
/// `raw_json_zstd` is a `BLOB` populated by new writes (schema v3+). If
/// present, decompress it. Otherwise fall back to the legacy `raw_json`
/// TEXT column for rows written before the migration ran.
fn decode_raw_json(
    text: Option<String>,
    zstd_blob: Option<Vec<u8>>,
) -> Option<String> {
    if let Some(blob) = zstd_blob {
        if let Some(s) = crate::raw_json_codec::decompress(&blob) {
            return Some(s);
        }
    }
    text
}

pub fn rebuild_daily_usage() -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute_batch(
            "DELETE FROM daily_usage;
             INSERT INTO daily_usage (date, provider, model, project_id, input_tokens, output_tokens,
                reasoning_tokens, cache_read_tokens, cache_write_tokens, total_tokens, cost_usd, sessions_count)
             SELECT date(timestamp), provider, model, project_id,
                    SUM(input_tokens), SUM(output_tokens), SUM(reasoning_tokens),
                    SUM(cache_read_tokens), SUM(cache_write_tokens), SUM(total_tokens), SUM(cost_usd),
                    COUNT(DISTINCT session_id)
             FROM usage_events WHERE ignored = 0
             GROUP BY date(timestamp), provider, model, project_id;",
        )?;
        Ok(())
    })
}

pub fn count_events() -> AppResult<i64> {
    db::with_conn(|conn| {
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM usage_events WHERE ignored = 0", [], |r| {
                r.get(0)
            })?;
        Ok(n)
    })
}

pub fn db_size_bytes() -> AppResult<u64> {
    db::with_conn(|conn| {
        let s: i64 = conn.query_row("SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()", [], |r| r.get(0))?;
        Ok(s as u64)
    })
}
