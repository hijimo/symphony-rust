use axum::{
    extract::{Query, State},
    Json,
};
use std::collections::HashMap;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::models::alert::{
    AlertChannelsResponse, AlertHistoryQuery, AlertHistoryRecord, AlertRule, AlertRulesResponse,
    ChannelTypeInfo, ConfigFieldSchema, NotificationChannelConfig, NotificationChannelRow,
    TestNotificationRequest, TestNotificationResponse, UpdateAlertChannelsRequest,
    UpdateAlertChannelsResponse, UpdateAlertRulesRequest, UpdateAlertRulesResponse,
    UpdateChannelItem,
};
use crate::models::{PaginationData, ResponseData};
use crate::notification::channel::NotificationChannel;
use crate::repository::AlertRepository;
use crate::AppState;

/// GET /api/admin/alerts
///
/// List alert history with pagination and filters.
pub async fn list_alert_history(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
    Query(query): Query<AlertHistoryQuery>,
) -> Result<Json<ResponseData<PaginationData<AlertHistoryRecord>>>, WebPlatformError> {
    // Validate severity filter if provided
    if let Some(ref sev) = query.severity {
        if !["critical", "warning", "info"].contains(&sev.as_str()) {
            return Err(WebPlatformError::BadRequest(format!(
                "Invalid severity filter: {}",
                sev
            )));
        }
    }

    // Validate status filter if provided
    if let Some(ref status) = query.status {
        if !["sent", "failed", "suppressed"].contains(&status.as_str()) {
            return Err(WebPlatformError::BadRequest(format!(
                "Invalid status filter: {}",
                status
            )));
        }
    }

    let page_no = query.effective_page_no();
    let page_size = query.effective_page_size();

    let (records, total_count) = state.repo.query_alert_history(&query).await?;

    let pagination = PaginationData::new(records, total_count, page_no, page_size);
    Ok(Json(ResponseData::success(pagination)))
}

/// GET /api/admin/alerts/rules
///
/// Get all alert rule configurations.
pub async fn get_alert_rules(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
) -> Result<Json<ResponseData<AlertRulesResponse>>, WebPlatformError> {
    let rules = state.repo.get_all_alert_rules().await?;
    Ok(Json(ResponseData::success(AlertRulesResponse { rules })))
}

/// PUT /api/admin/alerts/rules
///
/// Batch update alert rule configurations.
pub async fn update_alert_rules(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
    Json(req): Json<UpdateAlertRulesRequest>,
) -> Result<Json<ResponseData<UpdateAlertRulesResponse>>, WebPlatformError> {
    if req.rules.is_empty() {
        return Err(WebPlatformError::BadRequest(
            "rules array cannot be empty".to_string(),
        ));
    }

    // Known rule IDs
    let known_rules = [
        "task_timeout",
        "task_failure",
        "service_crash",
        "concurrency_saturation",
        "consecutive_failures",
        "api_unreachable",
    ];

    let mut updated_count: u32 = 0;

    for item in &req.rules {
        // Validate rule_id exists
        if !known_rules.contains(&item.rule_id.as_str()) {
            return Err(WebPlatformError::AlertRuleNotFound(item.rule_id.clone()));
        }

        // Validate cooldown_seconds range
        if let Some(cooldown) = item.cooldown_seconds {
            if !(60..=3600).contains(&cooldown) {
                return Err(WebPlatformError::BadRequest(format!(
                    "cooldownSeconds must be between 60 and 3600, got {}",
                    cooldown
                )));
            }
        }

        // Update in database
        let threshold_json = item
            .threshold
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_else(|_| "{}".to_string()));

        state
            .repo
            .update_alert_rule(
                &item.rule_id,
                item.enabled,
                threshold_json.as_deref(),
                item.cooldown_seconds,
            )
            .await?;

        updated_count += 1;
    }

    // Reload rules in the alert engine if available
    if let Some(ref alert_manager) = state.alert_manager {
        let _ = alert_manager.reload_rules().await;
    }

    // Return updated rules
    let rules = state.repo.get_all_alert_rules().await?;
    let updated_rules: Vec<AlertRule> = rules
        .into_iter()
        .filter(|r| req.rules.iter().any(|item| item.rule_id == r.rule_id))
        .collect();

    Ok(Json(ResponseData::success(UpdateAlertRulesResponse {
        updated_count,
        rules: updated_rules,
    })))
}

/// GET /api/admin/alerts/channels
///
/// Get all notification channel configurations.
pub async fn get_alert_channels(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
) -> Result<Json<ResponseData<AlertChannelsResponse>>, WebPlatformError> {
    let rows = state.repo.get_all_notification_channels().await?;

    let channels: Vec<NotificationChannelConfig> = rows
        .into_iter()
        .map(|row| row_to_masked_config(row, &state.encryption_key))
        .collect();

    let available_types = build_available_types();

    Ok(Json(ResponseData::success(AlertChannelsResponse {
        channels,
        available_types,
    })))
}

/// PUT /api/admin/alerts/channels
///
/// Update notification channel configurations (full replacement).
pub async fn update_alert_channels(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
    Json(req): Json<UpdateAlertChannelsRequest>,
) -> Result<Json<ResponseData<UpdateAlertChannelsResponse>>, WebPlatformError> {
    // Validate each channel
    for item in &req.channels {
        validate_channel_item(item)?;
    }

    // Convert to DB rows
    let rows: Vec<NotificationChannelRow> = req
        .channels
        .into_iter()
        .map(|item| {
            let channel_id = item
                .channel_id
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

            let config_json = serde_json::to_string(&item.config).unwrap_or_default();
            let config_encrypted = encrypt_config(&config_json, &state.encryption_key);

            let severity_filter_json =
                serde_json::to_string(&item.severity_filter).unwrap_or_default();

            let now = chrono::Utc::now().to_rfc3339();

            NotificationChannelRow {
                channel_id,
                name: item.name,
                channel_type: item.channel_type,
                enabled: item.enabled,
                config_encrypted,
                severity_filter_json,
                last_test_at: None,
                last_test_success: None,
                created_at: now.clone(),
                updated_at: now,
            }
        })
        .collect();

    state.repo.save_notification_channels(rows.clone()).await?;

    // Reload channels in the dispatcher if available
    if let Some(ref alert_manager) = state.alert_manager {
        let _ = alert_manager
            .reload_channels(&state.repo, &state.encryption_key)
            .await;
    }

    // Return masked configs
    let channels: Vec<NotificationChannelConfig> = rows
        .into_iter()
        .map(|row| row_to_masked_config(row, &state.encryption_key))
        .collect();

    Ok(Json(ResponseData::success(UpdateAlertChannelsResponse {
        channels,
    })))
}

/// POST /api/admin/alerts/test
///
/// Send a test notification to verify channel connectivity.
pub async fn test_notification(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<TestNotificationRequest>,
) -> Result<Json<ResponseData<TestNotificationResponse>>, WebPlatformError> {
    // Rate limit: 3 per minute per user
    let user_id: i64 = claims.sub.parse().unwrap_or(0);
    if let Err(retry_after) = state.phase3_rate_limiter.check("alerts_test", user_id, 3) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    // Find the channel in DB
    let row = state
        .repo
        .get_notification_channel(&req.channel_id)
        .await?
        .ok_or_else(|| {
            WebPlatformError::AlertChannelInvalid(format!("Channel not found: {}", req.channel_id))
        })?;

    if !row.enabled {
        return Err(WebPlatformError::AlertChannelInvalid(format!(
            "Channel is disabled: {}",
            req.channel_id
        )));
    }

    // Decrypt config and build channel instance
    let config_json = decrypt_config(&row.config_encrypted, &state.encryption_key)?;
    let config: HashMap<String, serde_json::Value> =
        serde_json::from_str(&config_json).unwrap_or_default();

    let message = req
        .message
        .unwrap_or_else(|| "这是一条测试通知".to_string());

    // Send test based on channel type
    let result = match row.channel_type.as_str() {
        "dingtalk" => {
            let webhook_url = config
                .get("webhook_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let secret = config
                .get("secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let channel = crate::notification::DingTalkChannel::new(
                row.channel_id.clone(),
                webhook_url,
                secret,
            );

            channel
                .send_test(&message, &claims.username)
                .await
                .map_err(|e| {
                    WebPlatformError::AlertNotificationFailed(format!(
                        "DingTalk send failed: {}",
                        e
                    ))
                })?
        }
        other => {
            return Err(WebPlatformError::AlertChannelInvalid(format!(
                "Unsupported channel type: {}",
                other
            )));
        }
    };

    // Update test result in DB
    let now = chrono::Utc::now().to_rfc3339();
    let _ = state
        .repo
        .update_channel_test_result(&row.channel_id, result.success, &now)
        .await;

    if !result.success {
        return Err(WebPlatformError::AlertNotificationFailed(
            result
                .error_message
                .unwrap_or_else(|| "Unknown error".to_string()),
        ));
    }

    Ok(Json(ResponseData::success(TestNotificationResponse {
        success: true,
        channel_id: row.channel_id,
        channel_type: row.channel_type,
        sent_at: result.sent_at.to_rfc3339(),
        response_time_ms: result.response_time_ms,
    })))
}

// ==================== Helper Functions ====================

/// Validate a channel update item.
fn validate_channel_item(item: &UpdateChannelItem) -> Result<(), WebPlatformError> {
    // Only dingtalk is currently supported
    if item.channel_type != "dingtalk" {
        return Err(WebPlatformError::AlertChannelInvalid(format!(
            "Unsupported channel type: {}. Currently only 'dingtalk' is supported.",
            item.channel_type
        )));
    }

    // Validate severity filter values
    let valid_severities = ["critical", "warning", "info"];
    for sev in &item.severity_filter {
        if !valid_severities.contains(&sev.as_str()) {
            return Err(WebPlatformError::BadRequest(format!(
                "Invalid severity filter value: {}",
                sev
            )));
        }
    }

    // Validate dingtalk config
    if item.channel_type == "dingtalk" {
        let webhook_url = item
            .config
            .get("webhook_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if webhook_url.is_empty() {
            return Err(WebPlatformError::AlertChannelInvalid(
                "webhook_url is required for dingtalk channel".to_string(),
            ));
        }

        if !webhook_url.starts_with("https://oapi.dingtalk.com/robot/send") {
            return Err(WebPlatformError::AlertChannelInvalid(
                "webhook_url must start with https://oapi.dingtalk.com/robot/send".to_string(),
            ));
        }
    }

    Ok(())
}

/// Convert a DB row to a masked API response config.
fn row_to_masked_config(
    row: NotificationChannelRow,
    encryption_key: &[u8; 32],
) -> NotificationChannelConfig {
    let config = match decrypt_config(&row.config_encrypted, encryption_key) {
        Ok(json_str) => {
            let parsed: HashMap<String, serde_json::Value> =
                serde_json::from_str(&json_str).unwrap_or_default();
            mask_config(&parsed)
        }
        Err(_) => HashMap::new(),
    };

    let severity_filter: Vec<String> =
        serde_json::from_str(&row.severity_filter_json).unwrap_or_default();

    NotificationChannelConfig {
        channel_id: row.channel_id,
        name: row.name,
        channel_type: row.channel_type,
        enabled: row.enabled,
        config,
        config_masked: true,
        severity_filter,
        last_test_at: row.last_test_at,
        last_test_success: row.last_test_success,
        updated_at: row.updated_at,
    }
}

/// Mask sensitive config fields for API responses.
fn mask_config(config: &HashMap<String, serde_json::Value>) -> HashMap<String, serde_json::Value> {
    let mut masked = HashMap::new();

    for (key, value) in config {
        let masked_value = match key.as_str() {
            "webhook_url" => {
                let s = value.as_str().unwrap_or_default();
                if s.len() > 44 {
                    serde_json::Value::String(format!("{}****{}", &s[..40], &s[s.len() - 4..]))
                } else {
                    serde_json::Value::String("****".to_string())
                }
            }
            "secret" | "password" => {
                let s = value.as_str().unwrap_or_default();
                if s.len() > 4 {
                    serde_json::Value::String(format!("****{}", &s[s.len() - 4..]))
                } else {
                    serde_json::Value::String("****".to_string())
                }
            }
            _ => value.clone(),
        };
        masked.insert(key.clone(), masked_value);
    }

    masked
}

/// Encrypt config JSON using AES-256-GCM (reuses existing crypto module pattern).
fn encrypt_config(plaintext: &str, key: &[u8; 32]) -> String {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use rand::RngCore;

    let cipher = Aes256Gcm::new_from_slice(key).expect("valid key length");
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .expect("encryption should not fail");

    // Format: base64(nonce || ciphertext)
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(&combined)
}

/// Decrypt config from AES-256-GCM encrypted base64 string.
fn decrypt_config(encrypted: &str, key: &[u8; 32]) -> Result<String, WebPlatformError> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use base64::Engine;

    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| WebPlatformError::Internal(format!("Failed to decode config: {}", e)))?;

    if combined.len() < 12 {
        return Err(WebPlatformError::Internal(
            "Invalid encrypted config: too short".to_string(),
        ));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key).expect("valid key length");
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| WebPlatformError::Internal(format!("Failed to decrypt config: {}", e)))?;

    String::from_utf8(plaintext)
        .map_err(|e| WebPlatformError::Internal(format!("Invalid UTF-8 in config: {}", e)))
}

/// Build the list of available channel types with their config schemas.
fn build_available_types() -> Vec<ChannelTypeInfo> {
    vec![
        ChannelTypeInfo {
            channel_type: "dingtalk".to_string(),
            name: "钉钉群机器人".to_string(),
            config_schema: HashMap::from([
                (
                    "webhook_url".to_string(),
                    ConfigFieldSchema {
                        field_type: "string".to_string(),
                        required: true,
                        description: "钉钉 Webhook URL".to_string(),
                    },
                ),
                (
                    "secret".to_string(),
                    ConfigFieldSchema {
                        field_type: "string".to_string(),
                        required: false,
                        description: "加签密钥（HMAC-SHA256）".to_string(),
                    },
                ),
            ]),
            status: None,
        },
        ChannelTypeInfo {
            channel_type: "slack".to_string(),
            name: "Slack".to_string(),
            config_schema: HashMap::from([(
                "webhook_url".to_string(),
                ConfigFieldSchema {
                    field_type: "string".to_string(),
                    required: true,
                    description: "Slack Incoming Webhook URL".to_string(),
                },
            )]),
            status: Some("coming_soon".to_string()),
        },
        ChannelTypeInfo {
            channel_type: "email".to_string(),
            name: "邮件".to_string(),
            config_schema: HashMap::from([
                (
                    "smtp_host".to_string(),
                    ConfigFieldSchema {
                        field_type: "string".to_string(),
                        required: true,
                        description: "SMTP 服务器地址".to_string(),
                    },
                ),
                (
                    "smtp_port".to_string(),
                    ConfigFieldSchema {
                        field_type: "number".to_string(),
                        required: true,
                        description: "SMTP 端口".to_string(),
                    },
                ),
                (
                    "username".to_string(),
                    ConfigFieldSchema {
                        field_type: "string".to_string(),
                        required: true,
                        description: "SMTP 用户名".to_string(),
                    },
                ),
                (
                    "password".to_string(),
                    ConfigFieldSchema {
                        field_type: "string".to_string(),
                        required: true,
                        description: "SMTP 密码".to_string(),
                    },
                ),
                (
                    "recipients".to_string(),
                    ConfigFieldSchema {
                        field_type: "array".to_string(),
                        required: true,
                        description: "收件人列表".to_string(),
                    },
                ),
            ]),
            status: Some("coming_soon".to_string()),
        },
        ChannelTypeInfo {
            channel_type: "webhook".to_string(),
            name: "自定义 Webhook".to_string(),
            config_schema: HashMap::from([
                (
                    "url".to_string(),
                    ConfigFieldSchema {
                        field_type: "string".to_string(),
                        required: true,
                        description: "Webhook URL".to_string(),
                    },
                ),
                (
                    "headers".to_string(),
                    ConfigFieldSchema {
                        field_type: "object".to_string(),
                        required: false,
                        description: "自定义请求头".to_string(),
                    },
                ),
            ]),
            status: Some("coming_soon".to_string()),
        },
    ]
}
