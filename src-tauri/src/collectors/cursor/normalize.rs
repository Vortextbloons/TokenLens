//! Normalize Cursor API JSON and CSV exports into canonical UsageEvents.

use crate::ingest::dedup;
use crate::pricing::{self, CostStatus};
use crate::types::{Exactness, UsageEvent};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::path::Path;

const PROVIDER: &str = "cursor";

pub fn is_included_kind(kind: &str) -> bool {
    let k = kind.to_lowercase();
    k.contains("included") || k == "included" || k == "free"
}

pub fn normalize_api_event(v: &Value, source_path: Option<&str>) -> Option<UsageEvent> {
    let ts_raw = v.get("timestamp").and_then(|x| x.as_str())?;
    let ts_ms: i64 = ts_raw.parse().ok()?;
    let timestamp = Utc.timestamp_millis_opt(ts_ms).single()?;

    let model = v
        .get("model")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown")
        .to_string();

    let kind = v
        .get("kind")
        .and_then(|x| x.as_str())
        .unwrap_or("usage")
        .to_string();
    let event_type = cursor_event_type(&kind);

    let token_usage = v.get("tokenUsage");
    let input_tokens = int_field(token_usage, &["inputTokens", "input_tokens"]);
    let output_tokens = int_field(token_usage, &["outputTokens", "output_tokens"]);
    let cache_write_tokens = int_field(token_usage, &["cacheWriteTokens", "cache_write_tokens"]);
    let cache_read_tokens = int_field(token_usage, &["cacheReadTokens", "cache_read_tokens"]);

    let total_tokens = {
        let t = int_field(token_usage, &["totalTokens", "total_tokens"]);
        if t > 0 {
            t
        } else {
            input_tokens + output_tokens + cache_read_tokens + cache_write_tokens
        }
    };

    let session_id = v
        .get("cloudAgentId")
        .or_else(|| v.get("owningUser"))
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("automationId")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });

    let charged_cents = v.get("chargedCents").and_then(|x| x.as_f64());

    let mut raw = v.clone();
    if let Some(sid) = &session_id {
        if let Some(obj) = raw.as_object_mut() {
            obj.insert("__session_id".to_string(), json!(sid));
        }
    }
    redact_secrets_in_value(&mut raw);

    let session_key = session_id.as_deref().unwrap_or("");
    let event_hash = dedup::hash_event(
        &timestamp.to_rfc3339(),
        PROVIDER,
        &model,
        session_key,
        &event_type,
        total_tokens,
        input_tokens,
        output_tokens,
    );

    let mut ev = UsageEvent {
        event_hash,
        timestamp,
        event_type,
        provider: Some(PROVIDER.to_string()),
        model: Some(model.clone()),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
        exactness: Exactness::Exact,
        confidence: 1.0,
        raw_json: Some(raw.to_string()),
        raw_source_path: source_path.map(|p| p.to_string()),
        ..UsageEvent::new(timestamp)
    };

    apply_cursor_cost(&mut ev, &kind, charged_cents);
    Some(ev)
}

pub fn normalize_csv_row(
    date: &str,
    cloud_agent_id: &str,
    automation_id: &str,
    kind: &str,
    model: &str,
    input_cache_write: &str,
    input_no_cache: &str,
    cache_read: &str,
    output_tokens: &str,
    total_tokens: &str,
    cost: &str,
    source_path: Option<&Path>,
) -> Option<UsageEvent> {
    let timestamp = DateTime::parse_from_rfc3339(date)
        .ok()
        .map(|d| d.with_timezone(&Utc))
        .or_else(|| {
            date.parse::<i64>()
                .ok()
                .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
        })?;

    let model = if model.is_empty() {
        "unknown".to_string()
    } else {
        model.to_string()
    };

    let input_tokens = parse_i64(input_no_cache);
    let cache_write_tokens = parse_i64(input_cache_write);
    let cache_read_tokens = parse_i64(cache_read);
    let output_tokens = parse_i64(output_tokens);
    let total_tokens = {
        let t = parse_i64(total_tokens);
        if t > 0 {
            t
        } else {
            input_tokens + output_tokens + cache_read_tokens + cache_write_tokens
        }
    };

    let session_id = if !cloud_agent_id.is_empty() {
        Some(cloud_agent_id.to_string())
    } else if !automation_id.is_empty() {
        Some(automation_id.to_string())
    } else {
        None
    };

    let event_type = cursor_event_type(kind);
    let session_key = session_id.as_deref().unwrap_or("");
    let event_hash = dedup::hash_event(
        &timestamp.to_rfc3339(),
        PROVIDER,
        &model,
        session_key,
        &event_type,
        total_tokens,
        input_tokens,
        output_tokens,
    );

    let charged_cents = parse_cost_cents(cost);

    let mut raw = json!({
        "date": date,
        "kind": kind,
        "model": model,
        "cost": cost,
        "cloudAgentId": cloud_agent_id,
        "automationId": automation_id,
    });
    if let Some(sid) = &session_id {
        raw["__session_id"] = json!(sid);
    }

    let mut ev = UsageEvent {
        event_hash,
        timestamp,
        event_type,
        provider: Some(PROVIDER.to_string()),
        model: Some(model.clone()),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_tokens,
        exactness: Exactness::Exact,
        confidence: 1.0,
        raw_json: Some(raw.to_string()),
        raw_source_path: source_path.map(|p| p.to_string_lossy().to_string()),
        ..UsageEvent::new(timestamp)
    };

    apply_cursor_cost(&mut ev, kind, charged_cents);
    Some(ev)
}

pub fn parse_cursor_csv(path: &Path) -> crate::errors::AppResult<Vec<UsageEvent>> {
    let mut rdr = csv::Reader::from_path(path)?;
    let headers = rdr.headers()?.clone();
    if !looks_like_cursor_csv(&headers) {
        return Ok(vec![]);
    }

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for result in rdr.records() {
        let record = result?;
        let get = |name: &str| -> String {
            headers
                .iter()
                .position(|h| h == name)
                .and_then(|i| record.get(i))
                .unwrap_or("")
                .to_string()
        };

        if let Some(ev) = normalize_csv_row(
            &get("Date"),
            &get("Cloud Agent ID"),
            &get("Automation ID"),
            &get("Kind"),
            &get("Model"),
            &get("Input (w/ Cache Write)"),
            &get("Input (w/o Cache Write)"),
            &get("Cache Read"),
            &get("Output Tokens"),
            &get("Total Tokens"),
            &get("Cost"),
            Some(path),
        ) {
            if seen.insert(ev.event_hash.clone()) {
                out.push(ev);
            }
        }
    }
    Ok(out)
}

fn looks_like_cursor_csv(headers: &csv::StringRecord) -> bool {
    headers.iter().any(|h| h == "Date")
        && headers.iter().any(|h| h == "Model")
        && headers.iter().any(|h| h == "Total Tokens")
}

fn cursor_event_type(kind: &str) -> String {
    if is_included_kind(kind) {
        "included".to_string()
    } else if kind.to_lowercase().contains("usage") {
        "usage_based".to_string()
    } else {
        kind.to_lowercase()
    }
}

fn parse_i64(s: &str) -> i64 {
    s.trim().parse().unwrap_or(0)
}

fn parse_cost_cents(cost: &str) -> Option<f64> {
    let c = cost.trim();
    if c.is_empty() || c.eq_ignore_ascii_case("included") || c.eq_ignore_ascii_case("free") {
        return None;
    }
    if let Ok(v) = c.parse::<f64>() {
        return Some(v * 100.0);
    }
    if let Some(stripped) = c.strip_prefix('$') {
        return stripped.parse::<f64>().ok().map(|v| v * 100.0);
    }
    None
}

fn int_field(obj: Option<&Value>, keys: &[&str]) -> i64 {
    let Some(obj) = obj else {
        return 0;
    };
    for k in keys {
        if let Some(n) = obj.get(*k).and_then(|x| x.as_i64()) {
            return n;
        }
        if let Some(s) = obj.get(*k).and_then(|x| x.as_str()) {
            if let Ok(n) = s.parse::<i64>() {
                return n;
            }
        }
    }
    0
}

pub fn apply_cursor_cost(ev: &mut UsageEvent, kind: &str, charged_cents: Option<f64>) {
    let model = ev.model.clone().unwrap_or_default();
    let breakdown = pricing::compute_cost_breakdown(
        PROVIDER,
        &model,
        ev.input_tokens,
        ev.output_tokens,
        ev.reasoning_tokens,
        ev.cache_read_tokens,
        ev.cache_write_tokens,
    );

    if let Some(cents) = charged_cents {
        if cents > 0.0 && !is_included_kind(kind) {
            ev.cost_usd = cents / 100.0;
            ev.exactness = Exactness::Exact;
            return;
        }
    }

    if is_included_kind(kind) {
        ev.cost_usd = breakdown.cost_usd;
        ev.exactness = Exactness::Estimated;
        return;
    }

    ev.cost_usd = breakdown.cost_usd;
    ev.exactness = match breakdown.status {
        CostStatus::Priced => Exactness::Exact,
        CostStatus::Estimated => Exactness::Estimated,
        CostStatus::Unpriced => Exactness::Unknown,
    };
}

fn redact_secrets_in_value(v: &mut Value) {
    if let Some(obj) = v.as_object_mut() {
        for key in ["sessionToken", "WorkosCursorSessionToken", "token", "cookie"] {
            obj.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn api_event_maps_tokens() {
        let v = json!({
            "timestamp": "1716845903438",
            "model": "composer-2.5",
            "kind": "USAGE_EVENT_KIND_INCLUDED_IN_BUSINESS",
            "tokenUsage": {
                "inputTokens": 100,
                "outputTokens": 50,
                "cacheWriteTokens": 10,
                "cacheReadTokens": 200
            },
            "chargedCents": 0.0
        });
        let ev = normalize_api_event(&v, None).unwrap();
        assert_eq!(ev.provider.as_deref(), Some("cursor"));
        assert_eq!(ev.model.as_deref(), Some("composer-2.5"));
        assert_eq!(ev.input_tokens, 100);
        assert_eq!(ev.output_tokens, 50);
        assert_eq!(ev.event_type, "included");
        assert_eq!(ev.exactness, Exactness::Estimated);
    }

    #[test]
    fn usage_based_uses_charged_cents() {
        let v = json!({
            "timestamp": "1716845903438",
            "model": "claude-opus-4-7-thinking-xhigh",
            "kind": "USAGE_EVENT_KIND_USAGE_BASED",
            "tokenUsage": { "inputTokens": 10, "outputTokens": 20 },
            "chargedCents": 150.5
        });
        let ev = normalize_api_event(&v, None).unwrap();
        assert!((ev.cost_usd - 1.505).abs() < 0.0001);
        assert_eq!(ev.exactness, Exactness::Exact);
    }

    #[test]
    fn csv_row_normalizes() {
        let ev = normalize_csv_row(
            "2026-05-27T19:58:23.438Z",
            "",
            "",
            "Included",
            "composer-2.5",
            "0",
            "91383",
            "758532",
            "10243",
            "860158",
            "Included",
            None,
        )
        .unwrap();
        assert_eq!(ev.input_tokens, 91383);
        assert_eq!(ev.cache_read_tokens, 758532);
        assert_eq!(ev.total_tokens, 860158);
        assert_eq!(ev.exactness, Exactness::Estimated);
    }

    #[test]
    fn dedup_hash_stable() {
        let v = json!({
            "timestamp": "1716845903438",
            "model": "auto",
            "kind": "Included",
            "tokenUsage": { "inputTokens": 1, "outputTokens": 2 }
        });
        let a = normalize_api_event(&v, None).unwrap().event_hash;
        let b = normalize_api_event(&v, None).unwrap().event_hash;
        assert_eq!(a, b);
    }
}
