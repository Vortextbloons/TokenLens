//! Budget evaluation. Reads `budget_daily_tokens` and `budget_monthly_cost`
//! from settings, compares against the current period's totals, and writes
//! `alerts` rows when thresholds are crossed (50%, 80%, 100%).

use crate::db;
use crate::errors::AppResult;
use rusqlite::params;

const ALERT_TYPES: &[&str] = &["daily_tokens", "monthly_cost"];

pub fn evaluate_budgets() -> AppResult<Vec<i64>> {
    let daily_limit: i64 = read_setting("budget_daily_tokens")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let monthly_cost_limit: f64 = read_setting("budget_monthly_cost_usd")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let mut new_alerts = Vec::new();

    if daily_limit > 0 {
        let used: i64 = db::with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COALESCE(SUM(total_tokens), 0) FROM usage_events
                 WHERE date(timestamp) = date('now') AND ignored = 0",
                [], |r| r.get(0),
            )?)
        })?;
        for pct in [100, 80, 50] {
            let threshold = (daily_limit as f64) * (pct as f64) / 100.0;
            if used as f64 >= threshold {
                if !alert_exists("daily_tokens", pct)? {
                    new_alerts.push(insert_alert(
                        "daily_tokens",
                        if pct == 100 { "critical" } else { "warning" },
                        &format!("Daily token limit {}% reached", pct),
                        &format!("You've used {} of {} tokens today ({}% of budget).", used, daily_limit, pct),
                    )?);
                }
            }
        }
    }

    if monthly_cost_limit > 0.0 {
        let cost: f64 = db::with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT COALESCE(SUM(cost_usd), 0) FROM usage_events
                 WHERE date(timestamp) >= date('now', '-30 days') AND ignored = 0",
                [], |r| r.get(0),
            )?)
        })?;
        for pct in [100, 80, 50] {
            let threshold = monthly_cost_limit * (pct as f64) / 100.0;
            if cost >= threshold {
                if !alert_exists("monthly_cost", pct)? {
                    new_alerts.push(insert_alert(
                        "monthly_cost",
                        if pct == 100 { "critical" } else { "warning" },
                        &format!("Monthly cost budget {}% reached", pct),
                        &format!("You've spent ${:.2} of ${:.2} budget ({}%).", cost, monthly_cost_limit, pct),
                    )?);
                }
            }
        }
    }

    Ok(new_alerts)
}

fn read_setting(key: &str) -> Option<String> {
    let r: Result<Option<String>, _> = db::with_conn(|conn| {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()
    });
    r.ok().flatten()
}

fn alert_exists(kind: &str, pct: i64) -> AppResult<bool> {
    db::with_conn(|conn| {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM alerts
             WHERE alert_type = ?1
               AND date(created_at) = date('now')
               AND message LIKE ?2",
            params![kind, format!("%{}% reached%", pct)],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    })
}

fn insert_alert(kind: &str, severity: &str, title: &str, message: &str) -> AppResult<i64> {
    db::with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO alerts (alert_type, severity, title, message, created_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))",
            params![kind, severity, title, message],
        )?;
        Ok(conn.last_insert_rowid())
    })
}

#[tauri::command]
pub fn list_alerts(limit: i64) -> AppResult<Vec<AlertRow>> {
    db::with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, alert_type, severity, title, message, created_at, acknowledged_at
             FROM alerts
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(AlertRow {
                    id: r.get(0)?,
                    alert_type: r.get(1)?,
                    severity: r.get(2)?,
                    title: r.get(3)?,
                    message: r.get(4)?,
                    created_at: r.get(5)?,
                    acknowledged_at: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn acknowledge_alert(id: i64) -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute(
            "UPDATE alerts SET acknowledged_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    })
}

#[tauri::command]
pub fn evaluate_budgets_command() -> AppResult<i64> {
    Ok(evaluate_budgets()?.len() as i64)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AlertRow {
    pub id: i64,
    pub alert_type: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub created_at: String,
    pub acknowledged_at: Option<String>,
}
