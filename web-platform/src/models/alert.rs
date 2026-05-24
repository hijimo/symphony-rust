use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ==================== Enums ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
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
    pub fn parse_or_info(s: &str) -> Self {
        match s {
            "critical" => Severity::Critical,
            "warning" => Severity::Warning,
            "info" => Severity::Info,
            _ => Severity::Info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Severity;

    #[test]
    fn severity_parse_or_info_defaults_unknown_values_to_info() {
        assert_eq!(Severity::parse_or_info("critical"), Severity::Critical);
        assert_eq!(Severity::parse_or_info("warning"), Severity::Warning);
        assert_eq!(Severity::parse_or_info("info"), Severity::Info);
        assert_eq!(Severity::parse_or_info("unknown"), Severity::Info);
    }
}

// ==================== DB Models ====================

/// Alert rule stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertRule {
    pub rule_id: String,
    pub name: String,
    pub description: String,
    pub severity: String,
    pub enabled: bool,
    pub threshold: HashMap<String, serde_json::Value>,
    pub cooldown_seconds: i64,
    pub updated_at: String,
}

/// Alert history record stored in the database.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertHistoryRecord {
    pub id: i64,
    pub rule_id: String,
    pub severity: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub title: String,
    pub message: String,
    pub context: Option<HashMap<String, String>>,
    pub fired_at: String,
    pub resolved_at: Option<String>,
    pub notified_at: Option<String>,
    pub notification_channel: Option<String>,
    pub notification_status: Option<String>,
}

/// Notification channel configuration stored in the database.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotificationChannelConfig {
    pub channel_id: String,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub config: HashMap<String, serde_json::Value>,
    pub config_masked: bool,
    pub severity_filter: Vec<String>,
    pub last_test_at: Option<String>,
    pub last_test_success: Option<bool>,
    pub updated_at: String,
}

// ==================== API Response Structs ====================

/// Response for GET /api/admin/alerts/rules
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertRulesResponse {
    pub rules: Vec<AlertRule>,
}

/// Response for PUT /api/admin/alerts/rules
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRulesResponse {
    pub updated_count: u32,
    pub rules: Vec<AlertRule>,
}

/// Response for GET /api/admin/alerts/channels
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertChannelsResponse {
    pub channels: Vec<NotificationChannelConfig>,
    pub available_types: Vec<ChannelTypeInfo>,
}

/// Response for PUT /api/admin/alerts/channels
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertChannelsResponse {
    pub channels: Vec<NotificationChannelConfig>,
}

/// Channel type metadata for available channel types.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChannelTypeInfo {
    #[serde(rename = "type")]
    pub channel_type: String,
    pub name: String,
    pub config_schema: HashMap<String, ConfigFieldSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Schema definition for a config field.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFieldSchema {
    #[serde(rename = "type")]
    pub field_type: String,
    pub required: bool,
    pub description: String,
}

/// Response for POST /api/admin/alerts/test
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestNotificationResponse {
    pub success: bool,
    pub channel_id: String,
    pub channel_type: String,
    pub sent_at: String,
    pub response_time_ms: u64,
}

// ==================== API Request Structs ====================

/// Request body for PUT /api/admin/alerts/rules
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRulesRequest {
    pub rules: Vec<UpdateAlertRuleItem>,
}

/// Individual rule update item.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertRuleItem {
    pub rule_id: String,
    pub enabled: Option<bool>,
    pub threshold: Option<HashMap<String, serde_json::Value>>,
    pub cooldown_seconds: Option<i64>,
}

/// Request body for PUT /api/admin/alerts/channels
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAlertChannelsRequest {
    pub channels: Vec<UpdateChannelItem>,
}

/// Individual channel update item.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChannelItem {
    pub channel_id: Option<String>,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub config: HashMap<String, serde_json::Value>,
    pub severity_filter: Vec<String>,
}

/// Request body for POST /api/admin/alerts/test
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestNotificationRequest {
    pub channel_id: String,
    pub message: Option<String>,
}

// ==================== Query Parameters ====================

/// Query parameters for GET /api/admin/alerts
#[derive(Debug, Deserialize, ToSchema)]
pub struct AlertHistoryQuery {
    pub page_no: Option<i64>,
    pub page_size: Option<i64>,
    pub severity: Option<String>,
    pub rule_id: Option<String>,
    pub project_id: Option<i64>,
    pub status: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

impl AlertHistoryQuery {
    pub fn effective_page_no(&self) -> i64 {
        self.page_no.unwrap_or(1).max(1)
    }

    pub fn effective_page_size(&self) -> i64 {
        self.page_size.unwrap_or(20).clamp(1, 100)
    }
}

// ==================== Internal Event Struct ====================

/// Alert event produced by the rule evaluator, passed to the notification dispatcher.
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
    pub fired_at: chrono::DateTime<chrono::Utc>,
}

// ==================== Repository Helper Structs ====================

/// Database row model for notification channels (raw DB representation).
#[derive(Debug, Clone)]
pub struct NotificationChannelRow {
    pub channel_id: String,
    pub name: String,
    pub channel_type: String,
    pub enabled: bool,
    pub config_encrypted: String,
    pub severity_filter_json: String,
    pub last_test_at: Option<String>,
    pub last_test_success: Option<bool>,
    pub created_at: String,
    pub updated_at: String,
}

/// Parameters for inserting an alert history record.
#[derive(Debug, Clone)]
pub struct InsertAlertHistory {
    pub rule_id: String,
    pub severity: String,
    pub project_id: Option<i64>,
    pub title: String,
    pub message: String,
    pub context_json: Option<String>,
    pub fired_at: String,
}
