pub mod channel;
pub mod dingtalk;
pub mod dispatcher;

pub use channel::{NotificationChannel, NotificationResult};
pub use dingtalk::DingTalkChannel;
pub use dispatcher::NotificationDispatcher;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Alert severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Critical => write!(f, "critical"),
            Severity::Warning => write!(f, "warning"),
            Severity::Info => write!(f, "info"),
        }
    }
}

impl Severity {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "critical" => Some(Severity::Critical),
            "warning" => Some(Severity::Warning),
            "info" => Some(Severity::Info),
            _ => None,
        }
    }
}

/// An alert event produced by the alert engine and consumed by the notification dispatcher.
#[derive(Debug, Clone, Serialize)]
pub struct AlertEvent {
    pub id: String,
    pub rule_id: String,
    pub severity: Severity,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub title: String,
    pub message: String,
    pub context: HashMap<String, String>,
    pub fired_at: DateTime<Utc>,
}

/// Error type for notification operations.
#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Channel not found: {0}")]
    ChannelNotFound(String),

    #[error("Channel disabled: {0}")]
    ChannelDisabled(String),

    #[error("Send failed: {0}")]
    SendFailed(String),
}
