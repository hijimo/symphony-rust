use thiserror::Error;

use crate::config::ConfigValidationError;

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("HTTP error: status {0}")]
    HttpError(u16),

    #[error("Rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("Request timed out")]
    Timeout,

    #[error("Connection refused")]
    ConnectionRefused,

    #[error("Server error: {0}")]
    ServerError(u16),

    #[error("Circuit breaker is open")]
    CircuitOpen,

    #[error("Invalid token")]
    InvalidToken,

    #[error("Authentication expired during operation")]
    AuthExpired,

    #[error("Workflow state not found: {0}")]
    MissingState(String),

    #[error("Unknown action: {0}")]
    UnknownAction(String),

    #[error("Failed to deserialize response: {0}")]
    Deserialization(#[from] serde_json::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Partial label operation: added={added:?}, failed_to_remove={failed:?}")]
    PartialLabelUpdate { added: Vec<String>, failed: Vec<String> },

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    Forbidden(String),

    #[error("Invalid request: {0}")]
    Unprocessable(String),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigValidationError),
}

impl PlatformError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::ConnectionRefused
                | Self::ServerError(_)
                | Self::RateLimited { .. }
        )
    }

    pub fn from_status(status: u16, body: &str) -> Self {
        match status {
            401 => Self::AuthExpired,
            403 => Self::Forbidden(body.to_string()),
            404 => Self::NotFound(body.to_string()),
            422 => Self::Unprocessable(body.to_string()),
            429 => Self::RateLimited {
                retry_after_ms: 60_000,
            },
            s if s >= 500 => Self::ServerError(s),
            s => Self::HttpError(s),
        }
    }
}
