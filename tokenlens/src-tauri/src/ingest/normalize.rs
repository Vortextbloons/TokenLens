//! Normalizer: turn raw parsed JSON into canonical `UsageEvent`s.
//!
//! Strategy: try several known shapes (OpenCode's `message.updated`,
//! OpenAI-style `usage` blocks, Anthropic, Google, generic), and fall back to
//! a best-effort key scan.

use crate::types::{Exactness, UsageEvent};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

pub fn detect_provider(model: &str) -> String {
    let m = model.to_lowercase();
    if m.starts_with("gpt-") || m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") {
        "openai".into()
    } else if m.starts_with("claude-") {
        "anthropic".into()
    } else if m.starts_with("gemini-") || m.starts_with("palm-") {
        "google".into()
    } else if m.ends_with("-lm-studio") || m.contains("lm-studio") || m.starts_with("local/") {
        "lmstudio".into()
    } else {
        "unknown".into()
    }
}

/// Extract a string from any of the given keys at the top level.
pub fn str_from(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
        // snake_case and camelCase variants
        for variant in [k.to_string(), camel(k)] {
            if let Some(s) = v.get(&variant).and_then(|x| x.as_str()) {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in s.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Extract an i64 from any of the given keys. Handles nested usage objects.
pub fn int_from(v: &Value, keys: &[&str]) -> i64 {
    for k in keys {
        if let Some(n) = v.get(*k).and_then(|x| x.as_i64()) {
            return n;
        }
        for variant in [k.to_string(), camel(k)] {
            if let Some(n) = v.get(&variant).and_then(|x| x.as_i64()) {
                return n;
            }
        }
    }
    0
}

/// Try a list of (key, subkeys) tuples for nested usage objects.
pub fn int_from_nested(v: &Value, parents: &[&str], keys: &[&str]) -> i64 {
    for parent in parents {
        if let Some(child) = v.get(*parent) {
            for k in keys {
                if let Some(n) = child.get(*k).and_then(|x| x.as_i64()) {
                    return n;
                }
                for variant in [k.to_string(), camel(k)] {
                    if let Some(n) = child.get(&variant).and_then(|x| x.as_i64()) {
                        return n;
                    }
                }
            }
            // Also try sub-children like completion_tokens_details
            for sub in ["completion_tokens_details", "output_tokens_details", "prompt_tokens_details", "cache"] {
                if let Some(grand) = child.get(sub) {
                    for k in keys {
                        if let Some(n) = grand.get(*k).and_then(|x| x.as_i64()) {
                            return n;
                        }
                        for variant in [k.to_string(), camel(k)] {
                            if let Some(n) = grand.get(&variant).and_then(|x| x.as_i64()) {
                                return n;
                            }
                        }
                    }
                }
            }
        }
    }
    0
}

/// Recursively walk a Value looking for any of the given keys at any depth.
fn int_from_deep(v: &Value, keys: &[&str]) -> i64 {
    match v {
        Value::Object(map) => {
            for k in keys {
                if let Some(n) = map.get(*k).and_then(|x| x.as_i64()) {
                    return n;
                }
                for variant in [k.to_string(), camel(k)] {
                    if let Some(n) = map.get(&variant).and_then(|x| x.as_i64()) {
                        return n;
                    }
                }
            }
            for (_, child) in map {
                let r = int_from_deep(child, keys);
                if r > 0 {
                    return r;
                }
            }
            0
        }
        _ => 0,
    }
}

/// Try to parse a timestamp from common fields. Defaults to "now".
pub fn parse_timestamp(v: &Value) -> DateTime<Utc> {
    for k in ["timestamp", "createdAt", "created_at", "time", "ts"] {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
                return dt.with_timezone(&Utc);
            }
            if let Ok(n) = s.parse::<i64>() {
                if let Some(dt) = Utc.timestamp_millis_opt(n * 1000).single() {
                    return dt;
                }
                if let Some(dt) = Utc.timestamp_millis_opt(n).single() {
                    return dt;
                }
            }
        }
    }
    if let Some(n) = v.get("timestamp").and_then(|x| x.as_i64()) {
        if let Some(dt) = Utc.timestamp_millis_opt(n * 1000).single() {
            return dt;
        }
        if let Some(dt) = Utc.timestamp_millis_opt(n).single() {
            return dt;
        }
    }
    Utc::now()
}

/// Normalize a raw JSON value into a canonical UsageEvent. Returns None if
/// the event is unrecognizable (no model, no tokens, no useful info).
pub fn normalize(raw: &Value) -> Option<UsageEvent> {
    let mut ev = UsageEvent::new(parse_timestamp(raw));

    // 1) Try known event shapes.

    // OpenCode: { type: "message.updated", info: { tokens, modelID, providerID, sessionID, ... } }
    // OR { type: "step.finish", tokens: {...}, modelID, providerID, sessionID }
    if let Some(model) = str_from(raw, &["model", "modelID", "model_id"]) {
        ev.model = Some(model);
    }
    if let Some(provider) = str_from(raw, &["provider", "providerID", "provider_id"]) {
        ev.provider = Some(provider);
    } else if let Some(m) = &ev.model {
        ev.provider = Some(detect_provider(m));
    }
    if let Some(sid) = str_from(raw, &["sessionID", "session_id", "sessionId", "conversation_id"]) {
        // We stash session id in a special raw field for ingest to pick up
        if let Some(obj) = raw.as_object() {
            let mut with_sid = obj.clone();
            with_sid.insert("__session_id".to_string(), Value::String(sid));
            ev.raw_json = Some(serde_json::to_string(&with_sid).unwrap_or_default());
        }
    } else if let Some(info) = raw.get("info") {
        if let Some(sid) = str_from(info, &["sessionID", "session_id", "sessionId", "conversation_id"]) {
            let mut with_sid = raw.as_object().cloned().unwrap_or_default();
            with_sid.insert("__session_id".to_string(), Value::String(sid));
            ev.raw_json = Some(serde_json::to_string(&Value::Object(with_sid)).unwrap_or_default());
        }
    }
    if let Some(role) = str_from(raw, &["role", "messageRole", "message_role"]) {
        ev.message_role = Some(role);
    }
    if let Some(et) = str_from(raw, &["type", "eventType", "event_type"]) {
        ev.event_type = et;
    }

    // Tokens: try top-level first
    let top_input = int_from(raw, &["input_tokens", "prompt_tokens", "inputTokens", "input", "prompt"]);
    let top_output = int_from(raw, &["output_tokens", "completion_tokens", "outputTokens", "output", "completion"]);
    let top_total = int_from(raw, &["total_tokens", "totalTokens", "total"]);

    // Nested under info / usage / tokens
    let nested_input = int_from_nested(raw, &["info", "usage", "tokens", "message"], &["input_tokens", "prompt_tokens", "inputTokens", "input", "prompt"]);
    let nested_output = int_from_nested(raw, &["info", "usage", "tokens", "message"], &["output_tokens", "completion_tokens", "outputTokens", "output", "completion"]);
    let nested_total = int_from_nested(raw, &["info", "usage", "tokens", "message"], &["total_tokens", "totalTokens", "total"]);

    // Last resort: deep search for these keys
    let deep_input = int_from_deep(raw, &["input_tokens", "prompt_tokens", "input"]);
    let deep_output = int_from_deep(raw, &["output_tokens", "completion_tokens", "output"]);
    let deep_total = int_from_deep(raw, &["total_tokens", "total"]);

    let input = if top_input > 0 { top_input } else if nested_input > 0 { nested_input } else { deep_input };
    let output = if top_output > 0 { top_output } else if nested_output > 0 { nested_output } else { deep_output };

    // Reasoning (computed before total so fallback can include it)
    let reasoning = int_from_nested(raw, &["info", "usage", "tokens", "message", "output_tokens_details", "completion_tokens_details"],
        &["reasoning_tokens", "reasoningTokens"])
        .max(int_from(raw, &["reasoning_tokens", "reasoningTokens"]));

    let total = if top_total > 0 {
        top_total
    } else if nested_total > 0 {
        nested_total
    } else if deep_total > 0 {
        deep_total
    } else {
        input + output + reasoning
    };

    ev.input_tokens = input;
    ev.output_tokens = output;
    ev.total_tokens = total;
    ev.reasoning_tokens = reasoning;

    // Cache
    let cache_read = int_from_nested(raw, &["info", "usage", "prompt_tokens_details", "cache"],
        &["cached_tokens", "cache_read_input_tokens", "cacheReadTokens"])
        .max(int_from(raw, &["cache_read_tokens", "cacheReadTokens", "cache_read_input_tokens"]));
    ev.cache_read_tokens = cache_read;
    let cache_write = int_from_nested(raw, &["info", "usage", "prompt_tokens_details", "cache"],
        &["cache_creation_tokens", "cacheWriteTokens", "cache_creation_input_tokens"])
        .max(int_from(raw, &["cache_write_tokens", "cacheWriteTokens", "cache_creation_input_tokens"]));
    ev.cache_write_tokens = cache_write;

    // Tool tokens: rare in source data, default 0
    ev.tool_tokens = int_from_nested(raw, &["info", "tools", "tool"], &["tokens"]);

    // Exactness
    let has_any = ev.input_tokens > 0 || ev.output_tokens > 0 || ev.total_tokens > 0;
    if has_any {
        // If we got the values from explicit fields, call it exact.
        let has_explicit = (top_input > 0 || top_output > 0 || top_total > 0)
            || (nested_input > 0 || nested_output > 0);
        ev.exactness = if has_explicit { Exactness::Exact } else { Exactness::Unknown };
        ev.confidence = if matches!(ev.exactness, Exactness::Exact) { 0.95 } else { 0.4 };
    } else {
        ev.exactness = Exactness::Unknown;
        ev.confidence = 0.0;
    }

    if ev.model.is_none() && ev.provider.is_none() && !has_any {
        return None;
    }

    if ev.raw_json.is_none() {
        ev.raw_json = Some(serde_json::to_string(raw).unwrap_or_default());
    }

    Some(ev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalizes_openai_shape() {
        let v = json!({
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 200,
                "total_tokens": 300
            }
        });
        let ev = normalize(&v).unwrap();
        assert_eq!(ev.model.as_deref(), Some("gpt-4o"));
        assert_eq!(ev.provider.as_deref(), Some("openai"));
        assert_eq!(ev.input_tokens, 100);
        assert_eq!(ev.output_tokens, 200);
        assert_eq!(ev.total_tokens, 300);
    }

    #[test]
    fn normalizes_anthropic_shape() {
        let v = json!({
            "model": "claude-sonnet-4-5",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20,
                "cache_read_input_tokens": 5
            }
        });
        let ev = normalize(&v).unwrap();
        assert_eq!(ev.provider.as_deref(), Some("anthropic"));
        assert_eq!(ev.cache_read_tokens, 5);
    }

    #[test]
    fn normalizes_opencode_step_finish() {
        let v = json!({
            "type": "step.finish",
            "sessionID": "abc",
            "modelID": "gpt-4o",
            "tokens": {
                "input": 50,
                "output": 75,
                "total": 125
            }
        });
        let ev = normalize(&v).unwrap();
        assert_eq!(ev.event_type, "step.finish");
        assert_eq!(ev.input_tokens, 50);
        assert_eq!(ev.output_tokens, 75);
    }

    #[test]
    fn normalizes_reasoning() {
        let v = json!({
            "model": "o1",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 100,
                "completion_tokens_details": { "reasoning_tokens": 80 }
            }
        });
        let ev = normalize(&v).unwrap();
        assert_eq!(ev.reasoning_tokens, 80);
    }

    #[test]
    fn rejects_empty() {
        let v = json!({ "foo": "bar" });
        assert!(normalize(&v).is_none());
    }
}
