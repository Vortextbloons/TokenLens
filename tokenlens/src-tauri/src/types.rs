//! Core domain types shared between collectors, ingest, and commands.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Confidence/accuracy label for a token/cost value.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Exactness {
    /// Reported by the provider/API.
    Exact,
    /// Computed by TokenLens from a text estimate.
    Estimated,
    /// Mix of exact and estimated across fields.
    Mixed,
    /// We have no idea — do not trust.
    #[default]
    Unknown,
}

impl Exactness {
    pub fn as_str(self) -> &'static str {
        match self {
            Exactness::Exact => "exact",
            Exactness::Estimated => "estimated",
            Exactness::Mixed => "mixed",
            Exactness::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for Exactness {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "exact" => Exactness::Exact,
            "estimated" => Exactness::Estimated,
            "mixed" => Exactness::Mixed,
            _ => Exactness::Unknown,
        })
    }
}

/// Source kinds the app knows about.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    OpencodeLogs,
    OpencodeInbox,
    Veyra,
    Lmstudio,
    OaiProxy,
    Manual,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::OpencodeLogs => "opencode_logs",
            SourceKind::OpencodeInbox => "opencode_inbox",
            SourceKind::Veyra => "veyra",
            SourceKind::Lmstudio => "lmstudio",
            SourceKind::OaiProxy => "oai_proxy",
            SourceKind::Manual => "manual",
        }
    }
}

impl std::str::FromStr for SourceKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "opencode_logs" => SourceKind::OpencodeLogs,
            "opencode_inbox" => SourceKind::OpencodeInbox,
            "veyra" => SourceKind::Veyra,
            "lmstudio" => SourceKind::Lmstudio,
            "oai_proxy" => SourceKind::OaiProxy,
            "manual" => SourceKind::Manual,
            _ => return Err(()),
        })
    }
}

/// Raw or normalized event before persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    /// Canonical hash used for dedup.
    pub event_hash: String,
    /// Event time (UTC).
    pub timestamp: DateTime<Utc>,
    /// Source id (DB row) — None if not yet inserted.
    pub source_id: Option<i64>,
    /// Session id (DB row) — None if not yet resolved.
    pub session_id: Option<i64>,
    /// Project id (DB row) — None if not yet resolved.
    pub project_id: Option<i64>,
    /// Event type: message, completion, tool_call, session_start, etc.
    pub event_type: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub message_role: Option<String>,

    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub tool_tokens: i64,
    pub total_tokens: i64,

    pub cost_usd: f64,
    pub exactness: Exactness,
    /// Confidence 0.0..=1.0.
    pub confidence: f64,

    /// Original raw JSON (post-redaction).
    pub raw_json: Option<String>,
    pub raw_source_path: Option<String>,
}

impl UsageEvent {
    pub fn new(timestamp: DateTime<Utc>) -> Self {
        Self {
            event_hash: String::new(),
            timestamp,
            source_id: None,
            session_id: None,
            project_id: None,
            event_type: "message".to_string(),
            provider: None,
            model: None,
            message_role: None,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            tool_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            exactness: Exactness::Unknown,
            confidence: 0.0,
            raw_json: None,
            raw_source_path: None,
        }
    }
}

/// Pricing record (mirrors `model_pricing` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub id: Option<i64>,
    pub provider: String,
    pub model: String,
    pub input_price_per_million: f64,
    pub output_price_per_million: f64,
    pub reasoning_price_per_million: f64,
    pub cache_read_price_per_million: f64,
    pub cache_write_price_per_million: f64,
    pub currency: String,
    pub effective_date: Option<String>,
    pub is_local: bool,
    pub source: String,
    pub updated_at: String,
}

/// Source record (mirrors `sources` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub path: Option<String>,
    pub enabled: bool,
    pub last_scanned_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

/// Session record (mirrors `sessions` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    pub source_session_id: String,
    pub source_id: Option<i64>,
    pub project_id: Option<i64>,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub exactness: String,
    pub raw_ref: Option<String>,
}

/// Overview KPIs for the dashboard.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OverviewStats {
    pub tokens_today: i64,
    pub tokens_week: i64,
    pub tokens_month: i64,
    pub cost_today_usd: f64,
    pub cost_week_usd: f64,
    pub cost_month_usd: f64,
    pub most_used_model: Option<String>,
    pub most_expensive_model: Option<String>,
    pub largest_session_id: Option<i64>,
    pub largest_session_tokens: i64,
    pub avg_tokens_per_session: f64,
    pub input_output_ratio: f64,
    pub reasoning_token_pct: f64,
    pub cache_savings_usd: f64,
    pub sessions_count: i64,
    pub exactness_mix: ExactnessMix,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExactnessMix {
    pub exact: i64,
    pub estimated: i64,
    pub mixed: i64,
    pub unknown: i64,
}

/// Time-bucketed series for charts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeseriesPoint {
    pub date: String, // YYYY-MM-DD
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
}

/// Filter for queries (date range + dimensions).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryFilter {
    pub start_date: Option<String>, // YYYY-MM-DD
    pub end_date: Option<String>,   // YYYY-MM-DD
    pub project_id: Option<i64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub source_id: Option<i64>,
    pub exactness: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Breakdown by a dimension (model, project, provider, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakdown {
    pub key: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub sessions_count: i64,
}

/// Result of scanning a source.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScanResult {
    pub files_scanned: i64,
    pub events_inserted: i64,
    pub events_skipped_duplicate: i64,
    pub errors: Vec<String>,
    pub duration_ms: i64,
}
