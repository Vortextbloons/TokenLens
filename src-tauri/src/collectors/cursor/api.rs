//! Cursor dashboard HTTP client.

use crate::collectors::cursor::auth::CursorCredentials;
use crate::errors::{AppError, AppResult};
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ORIGIN, REFERER};
use serde::Deserialize;
use serde_json::Value;
use tracing::debug;

const USAGE_SUMMARY: &str = "https://cursor.com/api/usage-summary";
const FILTERED_EVENTS: &str = "https://cursor.com/api/dashboard/get-filtered-usage-events";

pub struct CursorClient {
    http: reqwest::Client,
}

impl CursorClient {
    pub fn new(creds: &CursorCredentials) -> AppResult<Self> {
        let mut headers = HeaderMap::new();
        let cookie = format!("WorkosCursorSessionToken={}", creds.session_token);
        headers.insert(
            reqwest::header::COOKIE,
            HeaderValue::from_str(&cookie)
                .map_err(|e| AppError::Internal(format!("cookie header: {e}")))?,
        );
        headers.insert(ORIGIN, HeaderValue::from_static("https://cursor.com"));
        headers.insert(REFERER, HeaderValue::from_static("https://cursor.com/dashboard"));

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent("TokenLens/0.1")
            .build()?;
        Ok(Self { http })
    }

    pub async fn validate(&self) -> AppResult<UsageSummary> {
        let resp = self.http.get(USAGE_SUMMARY).send().await?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::Permission(
                "Cursor session expired or invalid. Please reconnect.".into(),
            ));
        }
        if !resp.status().is_success() {
            return Err(AppError::Network(format!(
                "usage-summary HTTP {}",
                resp.status()
            )));
        }
        resp.json::<UsageSummary>()
            .await
            .map_err(|e| AppError::Network(format!("usage-summary parse: {e}")))
    }

    pub async fn fetch_events_page(
        &self,
        page: u32,
        page_size: u32,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> AppResult<FilteredUsageResponse> {
        let mut body = serde_json::json!({
            "page": page,
            "pageSize": page_size,
        });
        if let Some(s) = start_ms {
            body["startDate"] = Value::String(s.to_string());
        }
        if let Some(e) = end_ms {
            body["endDate"] = Value::String(e.to_string());
        }

        let resp = self
            .http
            .post(FILTERED_EVENTS)
            .json(&body)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::Permission(
                "Cursor session expired or invalid. Please reconnect.".into(),
            ));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Network(format!(
                "get-filtered-usage-events HTTP {status}: {text}"
            )));
        }

        resp.json::<FilteredUsageResponse>()
            .await
            .map_err(|e| AppError::Network(format!("events parse: {e}")))
    }

    pub async fn fetch_all_events(
        &self,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> AppResult<Vec<Value>> {
        let page_size = 100u32;
        let mut page = 1u32;
        let mut all = Vec::new();

        loop {
            let resp = self
                .fetch_events_page(page, page_size, start_ms, end_ms)
                .await?;
            let total = resp.total_usage_events_count.unwrap_or(0);
            let batch = resp.usage_events_display.unwrap_or_default();
            let batch_len = batch.len();
            all.extend(batch);
            debug!(
                "Cursor events page {page}: {batch_len} rows (total {total})"
            );
            if batch_len == 0 {
                break;
            }
            if total > 0 {
                if (page as u64) * (page_size as u64) >= total {
                    break;
                }
            } else if (batch_len as u32) < page_size {
                // total missing/zero — keep paging while full pages arrive
                break;
            }
            page += 1;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        Ok(all)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub billing_cycle_start: Option<String>,
    pub billing_cycle_end: Option<String>,
    pub membership_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilteredUsageResponse {
    pub total_usage_events_count: Option<u64>,
    pub usage_events_display: Option<Vec<Value>>,
}

pub fn iso_to_ms(iso: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|d| d.timestamp_millis())
}

pub fn datetime_to_ms(dt: DateTime<Utc>) -> i64 {
    dt.timestamp_millis()
}

pub fn billing_range_ms(summary: &UsageSummary) -> (Option<i64>, Option<i64>) {
    let start = summary
        .billing_cycle_start
        .as_deref()
        .and_then(iso_to_ms);
    let end = summary.billing_cycle_end.as_deref().and_then(iso_to_ms);
    (start, end)
}
