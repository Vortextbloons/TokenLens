//! Centralized error type for TokenLens.
//!
//! All Tauri commands return `Result<T, AppError>`. AppError serializes to a
//! structured shape that the frontend can display cleanly.

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(format!("{e:#}"))
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<tauri_plugin_store::Error> for AppError {
    fn from(e: tauri_plugin_store::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<csv::Error> for AppError {
    fn from(e: csv::Error) -> Self {
        AppError::Io(std::io::Error::other(format!("csv: {e}")))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Network(e.to_string())
    }
}

/// Frontend-friendly error payload. Avoids leaking SQL/internal details but
/// still gives the user something useful to read.
#[derive(Debug, Serialize)]
pub struct AppErrorPayload {
    pub kind: String,
    pub message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let kind = match self {
            AppError::Db(_) => "db",
            AppError::Io(_) => "io",
            AppError::Serde(_) => "serde",
            AppError::Config(_) => "config",
            AppError::NotFound(_) => "not_found",
            AppError::Permission(_) => "permission",
            AppError::Invalid(_) => "invalid",
            AppError::Network(_) => "network",
            AppError::Parse(_) => "parse",
            AppError::Internal(_) => "internal",
            AppError::Other(_) => "other",
        };
        let message = match self {
            AppError::Db(_) => "database error".to_string(),
            _ => self.to_string(),
        };
        let payload = AppErrorPayload {
            kind: kind.to_string(),
            message,
        };
        payload.serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
