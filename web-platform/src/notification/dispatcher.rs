use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{error, info};

use super::channel::{NotificationChannel, NotificationResult};
use super::{AlertEvent, NotificationError};

/// The notification dispatcher routes alert events to configured channels
/// based on severity filters.
#[derive(Clone)]
pub struct NotificationDispatcher {
    channels: Arc<RwLock<Vec<ChannelEntry>>>,
}

/// Internal entry pairing a channel with its severity filter.
struct ChannelEntry {
    channel: Arc<dyn NotificationChannel>,
    severity_filter: Vec<String>,
    enabled: bool,
}

impl NotificationDispatcher {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a channel with its severity filter.
    pub async fn add_channel(
        &self,
        channel: Arc<dyn NotificationChannel>,
        severity_filter: Vec<String>,
        enabled: bool,
    ) {
        let mut channels = self.channels.write().await;
        channels.push(ChannelEntry {
            channel,
            severity_filter,
            enabled,
        });
    }

    /// Clear all channels (used before reload).
    pub async fn clear_channels(&self) {
        let mut channels = self.channels.write().await;
        channels.clear();
    }

    /// Dispatch an alert event to all matching channels.
    pub async fn dispatch(&self, alert: &AlertEvent) -> Vec<NotificationResult> {
        let channels = self.channels.read().await;
        let severity_str = alert.severity.to_string();

        let mut results = Vec::new();

        for entry in channels.iter() {
            if !entry.enabled {
                continue;
            }
            if !entry.severity_filter.contains(&severity_str) {
                continue;
            }

            match entry.channel.send(alert).await {
                Ok(result) => {
                    if result.success {
                        info!(
                            channel_id = entry.channel.channel_id(),
                            alert_id = %alert.id,
                            "Notification sent successfully"
                        );
                    } else {
                        error!(
                            channel_id = entry.channel.channel_id(),
                            alert_id = %alert.id,
                            error = ?result.error_message,
                            "Notification send reported failure"
                        );
                    }
                    results.push(result);
                }
                Err(e) => {
                    error!(
                        channel_id = entry.channel.channel_id(),
                        alert_id = %alert.id,
                        error = %e,
                        "Failed to send notification"
                    );
                    results.push(NotificationResult {
                        channel_id: entry.channel.channel_id().to_string(),
                        channel_type: entry.channel.channel_type().to_string(),
                        success: false,
                        error_message: Some(e.to_string()),
                        response_time_ms: 0,
                        sent_at: chrono::Utc::now(),
                    });
                }
            }
        }

        results
    }

    /// Send a test notification to a specific channel by ID.
    pub async fn send_test(
        &self,
        channel_id: &str,
        message: &str,
        operator: &str,
    ) -> Result<NotificationResult, NotificationError> {
        let channels = self.channels.read().await;

        let entry = channels
            .iter()
            .find(|e| e.channel.channel_id() == channel_id);

        match entry {
            None => Err(NotificationError::ChannelNotFound(channel_id.to_string())),
            Some(e) if !e.enabled => {
                Err(NotificationError::ChannelDisabled(channel_id.to_string()))
            }
            Some(e) => e.channel.send_test(message, operator).await,
        }
    }

    /// Get a channel by ID (for health checks, etc.).
    pub async fn get_channel(&self, channel_id: &str) -> Option<Arc<dyn NotificationChannel>> {
        let channels = self.channels.read().await;
        channels
            .iter()
            .find(|e| e.channel.channel_id() == channel_id)
            .map(|e| e.channel.clone())
    }
}

impl Default for NotificationDispatcher {
    fn default() -> Self {
        Self::new()
    }
}
