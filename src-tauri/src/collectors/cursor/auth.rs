//! Cursor session credential storage (encrypted at rest in SQLite).

use crate::db;
use crate::errors::{AppError, AppResult};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct CursorCredentials {
    pub session_token: String,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub label: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub connected_at: DateTime<Utc>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_sync_cursor: Option<DateTime<Utc>>,
    pub last_sync_result: Option<String>,
    pub events_total: i64,
}

impl CursorCredentials {
    /// In-memory credentials for API validation before persisting to the DB.
    pub fn provisional(session_token: impl Into<String>) -> Self {
        Self {
            session_token: session_token.into(),
            user_id: None,
            team_id: None,
            label: None,
            expires_at: None,
            connected_at: Utc::now(),
            last_sync_at: None,
            last_sync_cursor: None,
            last_sync_result: None,
            events_total: 0,
        }
    }
}

fn machine_key() -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"tokenlens-cursor-v1");
    if let Some(dir) = crate::app_local_data_dir() {
        h.update(dir.to_string_lossy().as_bytes());
    }
    h.finalize().into()
}

fn encrypt(plain: &str) -> String {
    let key = machine_key();
    let bytes = plain.as_bytes();
    let encrypted: Vec<u8> = bytes
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect();
    B64.encode(encrypted)
}

fn decrypt(cipher: &str) -> AppResult<String> {
    let key = machine_key();
    let encrypted = B64
        .decode(cipher)
        .map_err(|e| AppError::Internal(format!("credential decode: {e}")))?;
    let plain: Vec<u8> = encrypted
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect();
    String::from_utf8(plain).map_err(|e| AppError::Internal(format!("credential utf8: {e}")))
}

fn parse_opt_ts(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|v| DateTime::parse_from_rfc3339(&v).ok().map(|d| d.with_timezone(&Utc)))
}

fn parse_req_ts(s: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| AppError::Parse(format!("timestamp: {e}")))
}

/// Parse JWT `exp` claim from WorkosCursorSessionToken (best-effort).
pub fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    let payload = token.split('.').nth(1)?;
    let padded = match payload.len() % 4 {
        0 => payload.to_string(),
        n => format!("{}{}", payload, "=".repeat(4 - n)),
    };
    let bytes = B64.decode(padded).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let exp = v.get("exp")?.as_i64()?;
    DateTime::from_timestamp(exp, 0)
}

pub fn load() -> AppResult<Option<CursorCredentials>> {
    db::with_conn(|conn| {
        let row = conn
            .query_row(
                "SELECT session_token, user_id, team_id, label, expires_at, connected_at,
                        last_sync_at, last_sync_cursor, last_sync_result, events_total
                 FROM cursor_credentials WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, i64>(9)?,
                    ))
                },
            )
            .optional()?;

        let Some((
            enc_token,
            user_id,
            team_id,
            label,
            expires_at,
            connected_at,
            last_sync_at,
            last_sync_cursor,
            last_sync_result,
            events_total,
        )) = row
        else {
            return Ok(None);
        };

        let session_token = decrypt(&enc_token)?;
        Ok(Some(CursorCredentials {
            session_token,
            user_id,
            team_id,
            label,
            expires_at: parse_opt_ts(expires_at),
            connected_at: parse_req_ts(&connected_at)?,
            last_sync_at: parse_opt_ts(last_sync_at),
            last_sync_cursor: parse_opt_ts(last_sync_cursor),
            last_sync_result,
            events_total,
        }))
    })
}

pub fn save(
    session_token: &str,
    user_id: Option<&str>,
    team_id: Option<&str>,
    label: Option<&str>,
) -> AppResult<()> {
    let now = Utc::now();
    let expires_at = jwt_expiry(session_token);
    let enc = encrypt(session_token);
    let connected_at = now.to_rfc3339();
    let expires_str = expires_at.map(|d| d.to_rfc3339());

    db::with_conn_mut(|conn| {
        conn.execute(
            "INSERT INTO cursor_credentials
                (id, session_token, user_id, team_id, label, expires_at, connected_at,
                 last_sync_at, last_sync_cursor, last_sync_result, events_total)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, 0)
             ON CONFLICT(id) DO UPDATE SET
                session_token = excluded.session_token,
                user_id = excluded.user_id,
                team_id = excluded.team_id,
                label = excluded.label,
                expires_at = excluded.expires_at,
                connected_at = excluded.connected_at,
                last_sync_at = NULL,
                last_sync_cursor = NULL,
                last_sync_result = NULL,
                events_total = 0",
            rusqlite::params![enc, user_id, team_id, label, expires_str, connected_at],
        )?;
        Ok(())
    })
}

pub fn update_sync_meta(
    last_sync_at: DateTime<Utc>,
    last_sync_cursor: Option<DateTime<Utc>>,
    result_summary: &str,
    events_total_delta: i64,
) -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute(
            "UPDATE cursor_credentials SET
                last_sync_at = ?1,
                last_sync_cursor = COALESCE(?2, last_sync_cursor),
                last_sync_result = ?3,
                events_total = events_total + ?4
             WHERE id = 1",
            rusqlite::params![
                last_sync_at.to_rfc3339(),
                last_sync_cursor.map(|d| d.to_rfc3339()),
                result_summary,
                events_total_delta,
            ],
        )?;
        Ok(())
    })
}

pub fn delete() -> AppResult<()> {
    db::with_conn_mut(|conn| {
        conn.execute("DELETE FROM cursor_credentials WHERE id = 1", [])?;
        Ok(())
    })
}

pub fn is_connected() -> bool {
    load().ok().flatten().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_roundtrip() {
        let plain = "test-session-token-abc123";
        let enc = encrypt(plain);
        assert_ne!(enc, plain);
        assert_eq!(decrypt(&enc).unwrap(), plain);
    }
}
