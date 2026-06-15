//! Aggregation queries: KPIs, breakdowns, time series.
//!
//! Most queries are pure SQL over `usage_events` + `sessions` + `daily_usage`.

use crate::db;
use crate::errors::AppResult;
use crate::types::{
    Breakdown, ExactnessMix, OverviewStats, QueryFilter, Session, TimeseriesPoint,
};
use rusqlite::{params, OptionalExtension};

fn build_where(f: &QueryFilter) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value as V;
    let mut clauses: Vec<String> = Vec::new();
    let mut args: Vec<V> = Vec::new();
    clauses.push("ignored = 0".to_string());
    if let Some(d) = &f.start_date {
        clauses.push("date(timestamp) >= date(?1)".into());
        args.push(V::Text(d.clone()));
    }
    if let Some(d) = &f.end_date {
        let ph = args.len() + 1;
        clauses.push(format!("date(timestamp) <= date(?{ph})"));
        args.push(V::Text(d.clone()));
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
    if let Some(s) = f.source_id {
        let ph = args.len() + 1;
        clauses.push(format!("source_id = ?{ph}"));
        args.push(V::Integer(s));
    }
    if let Some(e) = &f.exactness {
        let ph = args.len() + 1;
        clauses.push(format!("exactness = ?{ph}"));
        args.push(V::Text(e.clone()));
    }
    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    (where_sql, args)
}

pub fn overview(filter: &QueryFilter) -> AppResult<OverviewStats> {
    let (where_sql, args) = build_where(filter);
    db::with_conn(|conn| {
        // Tokens / cost today / week / month
        let q = format!(
            "SELECT
                COALESCE(SUM(CASE WHEN date(timestamp)=date('now') THEN total_tokens ELSE 0 END), 0) AS tokens_today,
                COALESCE(SUM(CASE WHEN date(timestamp)>=date('now','-7 days') THEN total_tokens ELSE 0 END), 0) AS tokens_week,
                COALESCE(SUM(CASE WHEN date(timestamp)>=date('now','-30 days') THEN total_tokens ELSE 0 END), 0) AS tokens_month,
                COALESCE(SUM(CASE WHEN date(timestamp)=date('now') THEN cost_usd ELSE 0 END), 0) AS cost_today,
                COALESCE(SUM(CASE WHEN date(timestamp)>=date('now','-7 days') THEN cost_usd ELSE 0 END), 0) AS cost_week,
                COALESCE(SUM(CASE WHEN date(timestamp)>=date('now','-30 days') THEN cost_usd ELSE 0 END), 0) AS cost_month,
                COALESCE(SUM(input_tokens), 0) AS in_t,
                COALESCE(SUM(output_tokens), 0) AS out_t,
                COALESCE(SUM(reasoning_tokens), 0) AS reas_t,
                COALESCE(SUM(cache_read_tokens), 0) AS cache_t
             FROM usage_events {where_sql}",
        );
        let mut stmt = conn.prepare(&q)?;
        let row = stmt.query_row(rusqlite::params_from_iter(args.iter()), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
            ))
        })?;
        let (tokens_today, tokens_week, tokens_month, cost_today, cost_week, cost_month, in_t, out_t, reas_t, cache_t) = row;
        drop(stmt);

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

        // Largest session
        let q4 = format!(
            "SELECT id, total_tokens FROM sessions
             WHERE total_tokens > 0
             ORDER BY total_tokens DESC LIMIT 1",
        );
        let largest = conn
            .query_row(&q4, [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .optional()?;

        // Sessions count
        let q5 = format!("SELECT COUNT(DISTINCT session_id) FROM usage_events {where_sql}");
        let sessions_count: i64 = conn.query_row(
            &q5,
            rusqlite::params_from_iter(args.iter()),
            |r| r.get(0),
        )?;

        // Avg tokens / session
        let avg: f64 = if sessions_count > 0 {
            (tokens_month.max(0) as f64) / (sessions_count as f64)
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

        // Cache savings (approx: cache_read * input_price)
        // Simplified: we'll compute as cache_read * 0.5 * (input price average)
        // For v1, use a flat $1.25 / 1M tokens saving estimate based on cache discount.
        let cache_savings = (cache_t as f64) * 1.25 / 1_000_000.0;

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
            exactness_mix: mix,
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
                    total_tokens: r.get(5)?,
                    cost_usd: r.get(6)?,
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

pub fn list_sessions(filter: &QueryFilter) -> AppResult<Vec<Session>> {
    let mut q = String::from(
        "SELECT id, source_session_id, source_id, project_id, title, started_at,
                last_seen_at, provider, model, total_tokens, total_cost_usd,
                exactness, raw_ref
         FROM sessions WHERE 1=1",
    );
    let mut args: Vec<rusqlite::types::Value> = vec![];

    if let Some(d) = &filter.start_date {
        q.push_str(" AND date(last_seen_at) >= date(?1)");
        args.push(rusqlite::types::Value::Text(d.clone()));
    }
    if let Some(d) = &filter.end_date {
        let ph = args.len() + 1;
        q.push_str(&format!(" AND date(last_seen_at) <= date(?{ph})"));
        args.push(rusqlite::types::Value::Text(d.clone()));
    }
    if let Some(p) = filter.project_id {
        let ph = args.len() + 1;
        q.push_str(&format!(" AND project_id = ?{ph}"));
        args.push(rusqlite::types::Value::Integer(p));
    }
    if let Some(p) = &filter.provider {
        let ph = args.len() + 1;
        q.push_str(&format!(" AND provider = ?{ph}"));
        args.push(rusqlite::types::Value::Text(p.clone()));
    }
    if let Some(m) = &filter.model {
        let ph = args.len() + 1;
        q.push_str(&format!(" AND model = ?{ph}"));
        args.push(rusqlite::types::Value::Text(m.clone()));
    }

    q.push_str(" ORDER BY last_seen_at DESC");
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
                "SELECT id, source_session_id, source_id, project_id, title, started_at,
                        last_seen_at, provider, model, total_tokens, total_cost_usd,
                        exactness, raw_ref
                 FROM sessions WHERE id = ?1",
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
                    total_tokens, cost_usd, exactness, confidence, raw_json, raw_source_path
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
                    raw_json: r.get(20)?,
                    raw_source_path: r.get(21)?,
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
                total_tokens, cost_usd, exactness, confidence, raw_json, raw_source_path
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
                    raw_json: r.get(20)?,
                    raw_source_path: r.get(21)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
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
