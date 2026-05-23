use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::{AlertEvent, NotificationError};

/// Result of a notification send attempt.
#[derive(Debug, Clone)]
pub struct NotificationResult {
    pub channel_id: String,
    pub channel_type: String,
    pub success: bool,
    pub error_message: Option<String>,
    pub response_time_ms: u64,
    pub sent_at: DateTime<Utc>,
}

/// Trait that all notification channels must implement.
#[async_trait]
pub trait NotificationChannel: Send + Sync {
    /// Channel type identifier (e.g. "dingtalk").
    fn channel_type(&self) -> &str;

    /// Channel unique ID.
    fn channel_id(&self) -> &str;

    /// Send an alert notification.
    async fn send(&self, alert: &AlertEvent) -> Result<NotificationResult, NotificationError>;

    /// Send a test notification message.
    async fn send_test(
        &self,
        message: &str,
        operator: &str,
    ) -> Result<NotificationResult, NotificationError>;

    /// Health check — verify configuration validity without sending.
    async fn health_check(&self) -> bool;
}
