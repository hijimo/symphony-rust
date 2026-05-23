use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum WebPlatformError {
    #[error("Authentication required")]
    Unauthorized,

    #[error("Invalid username or password")]
    InvalidCredentials,

    #[error("Access denied")]
    Forbidden,

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Conflict(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Pool error: {0}")]
    Pool(#[from] r2d2::Error),

    /// Platform token is invalid or expired (TOKEN_001).
    #[error("Platform token invalid: {0}")]
    TokenInvalid(String),

    /// External service unavailable (EXT_001) - GitLab/GitHub/AI.
    #[error("External service unavailable: {0}")]
    ExternalService(String),

    /// Rate limited (EXT_002) - too many requests.
    /// The u64 value is the Retry-After in seconds.
    #[error("Rate limited")]
    RateLimited(u64),

    /// AI generation rate limited (EXT_002) - specific to AI endpoints.
    #[error("AI rate limited")]
    AiRateLimited(u64),

    /// Alert rule not found (ALERT_001).
    #[error("Alert rule not found: {0}")]
    AlertRuleNotFound(String),

    /// Notification channel config invalid (ALERT_002).
    #[error("Channel config invalid: {0}")]
    AlertChannelInvalid(String),

    /// Test notification send failed (ALERT_003).
    #[error("Notification send failed: {0}")]
    AlertNotificationFailed(String),
}

impl IntoResponse for WebPlatformError {
    fn into_response(self) -> Response {
        let (status, code, message, show_type) = match &self {
            WebPlatformError::Unauthorized => {
                (StatusCode::UNAUTHORIZED, "AUTH_001", self.to_string(), 9)
            }
            WebPlatformError::InvalidCredentials => {
                (StatusCode::UNAUTHORIZED, "AUTH_003", self.to_string(), 2)
            }
            WebPlatformError::Forbidden => (StatusCode::FORBIDDEN, "AUTH_002", self.to_string(), 2),
            WebPlatformError::NotFound(msg) => (StatusCode::NOT_FOUND, "BIZ_002", msg.clone(), 2),
            WebPlatformError::BadRequest(msg) => {
                (StatusCode::BAD_REQUEST, "BIZ_001", msg.clone(), 1)
            }
            WebPlatformError::Conflict(msg) => (StatusCode::CONFLICT, "BIZ_003", msg.clone(), 1),
            WebPlatformError::TokenInvalid(msg) => {
                (StatusCode::BAD_REQUEST, "TOKEN_001", msg.clone(), 1)
            }
            WebPlatformError::ExternalService(msg) => {
                (StatusCode::BAD_GATEWAY, "EXT_001", msg.clone(), 4)
            }
            WebPlatformError::RateLimited(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                "EXT_002",
                "请求过于频繁，请稍后重试".to_string(),
                1,
            ),
            WebPlatformError::AiRateLimited(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                "EXT_002",
                "AI 生成请求过于频繁，请稍后重试（限制：10次/分钟）".to_string(),
                1,
            ),
            WebPlatformError::AlertRuleNotFound(msg) => {
                (StatusCode::NOT_FOUND, "ALERT_001", msg.clone(), 2)
            }
            WebPlatformError::AlertChannelInvalid(msg) => {
                (StatusCode::BAD_REQUEST, "ALERT_002", msg.clone(), 1)
            }
            WebPlatformError::AlertNotificationFailed(msg) => {
                (StatusCode::BAD_GATEWAY, "ALERT_003", msg.clone(), 1)
            }
            WebPlatformError::Internal(_)
            | WebPlatformError::Database(_)
            | WebPlatformError::Pool(_) => {
                tracing::error!("Internal error: {:?}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "SYS_001",
                    "Internal server error".to_string(),
                    2,
                )
            }
        };

        let body = json!({
            "data": serde_json::Value::Null,
            "success": false,
            "retCode": code,
            "retMsg": message,
            "showType": show_type
        });

        let mut response = (status, Json(body)).into_response();

        // Add Retry-After header for rate limited responses
        match &self {
            WebPlatformError::RateLimited(retry_after)
            | WebPlatformError::AiRateLimited(retry_after) => {
                response
                    .headers_mut()
                    .insert("Retry-After", retry_after.to_string().parse().unwrap());
            }
            _ => {}
        }

        response
    }
}

pub type Result<T> = std::result::Result<T, WebPlatformError>;
