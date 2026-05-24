use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use super::channel::{NotificationChannel, NotificationResult};
use super::{AlertEvent, NotificationError, Severity};
use crate::proxy::EffectiveProxyConfig;

/// DingTalk group robot notification channel.
pub struct DingTalkChannel {
    channel_id: String,
    webhook_url: String,
    secret: Option<String>,
    http_client: reqwest::Client,
}

impl DingTalkChannel {
    pub fn new(channel_id: String, webhook_url: String, secret: Option<String>) -> Self {
        Self::new_with_proxy(channel_id, webhook_url, secret, None)
    }

    pub fn new_with_proxy(
        channel_id: String,
        webhook_url: String,
        secret: Option<String>,
        proxy_config: Option<&EffectiveProxyConfig>,
    ) -> Self {
        let builder = proxy_config
            .and_then(|config| {
                config
                    .apply_to_reqwest_builder(reqwest::Client::builder())
                    .map_err(|e| {
                        tracing::warn!(error = %e, "failed to apply proxy config to DingTalk client");
                        e
                    })
                    .ok()
            })
            .unwrap_or_else(reqwest::Client::builder);

        Self {
            channel_id,
            webhook_url,
            secret,
            http_client: builder
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Generate HMAC-SHA256 signature for DingTalk.
    fn sign(&self, timestamp: i64) -> Option<String> {
        let secret = self.secret.as_ref()?;
        let string_to_sign = format!("{}\n{}", timestamp, secret);

        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(string_to_sign.as_bytes());
        let result = mac.finalize();

        Some(BASE64.encode(result.into_bytes()))
    }

    /// Build the request URL with optional signature params.
    fn build_url(&self) -> String {
        if self.secret.is_some() {
            let timestamp = Utc::now().timestamp_millis();
            let sign = self.sign(timestamp).unwrap_or_default();
            format!(
                "{}&timestamp={}&sign={}",
                self.webhook_url,
                timestamp,
                urlencoding::encode(&sign)
            )
        } else {
            self.webhook_url.clone()
        }
    }

    /// Format an AlertEvent as a DingTalk markdown message.
    fn format_alert_message(&self, alert: &AlertEvent) -> serde_json::Value {
        let severity_icon = match alert.severity {
            Severity::Critical => "🔴",
            Severity::Warning => "🟡",
            Severity::Info => "🔵",
        };

        let project_info = alert.project_name.as_deref().unwrap_or("系统");
        let time_str = alert.fired_at.format("%Y-%m-%d %H:%M:%S").to_string();

        let mut text = format!(
            "### {} {}\n\n**项目**: {}\n\n**详情**: {}\n\n**时间**: {}",
            severity_icon, alert.title, project_info, alert.message, time_str
        );

        if !alert.context.is_empty() {
            text.push_str("\n\n**上下文**:\n");
            for (key, value) in &alert.context {
                text.push_str(&format!("- {}: {}\n", key, value));
            }
        }

        serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": format!("{} {}", severity_icon, alert.title),
                "text": text
            }
        })
    }

    /// Send a JSON body to DingTalk and parse the response.
    async fn do_send(
        &self,
        body: serde_json::Value,
    ) -> Result<NotificationResult, NotificationError> {
        let url = self.build_url();
        let start = std::time::Instant::now();

        let response = self.http_client.post(&url).json(&body).send().await?;

        let elapsed = start.elapsed().as_millis() as u64;
        let status = response.status();
        let response_body: serde_json::Value = response
            .json()
            .await
            .unwrap_or_else(|_| serde_json::json!({}));

        let success =
            status.is_success() && response_body.get("errcode").and_then(|v| v.as_i64()) == Some(0);

        let error_message = if !success {
            Some(format!(
                "HTTP {}: {}",
                status,
                response_body
                    .get("errmsg")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error")
            ))
        } else {
            None
        };

        Ok(NotificationResult {
            channel_id: self.channel_id.clone(),
            channel_type: "dingtalk".to_string(),
            success,
            error_message,
            response_time_ms: elapsed,
            sent_at: Utc::now(),
        })
    }
}

#[async_trait]
impl NotificationChannel for DingTalkChannel {
    fn channel_type(&self) -> &str {
        "dingtalk"
    }

    fn channel_id(&self) -> &str {
        &self.channel_id
    }

    async fn send(&self, alert: &AlertEvent) -> Result<NotificationResult, NotificationError> {
        let body = self.format_alert_message(alert);
        self.do_send(body).await
    }

    async fn send_test(
        &self,
        message: &str,
        operator: &str,
    ) -> Result<NotificationResult, NotificationError> {
        let time_str = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let text = format!(
            "### Symphony 通知测试\n\n**状态**: 连通性验证成功\n\n**渠道**: 钉钉群机器人\n\n**时间**: {}\n\n**操作人**: {}\n\n---\n\n{}",
            time_str, operator, message
        );

        let body = serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": "Symphony 通知测试",
                "text": text
            }
        });

        self.do_send(body).await
    }

    async fn health_check(&self) -> bool {
        self.webhook_url
            .starts_with("https://oapi.dingtalk.com/robot/send")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_dingtalk_sign_generation() {
        let channel = DingTalkChannel::new(
            "ch-1".to_string(),
            "https://oapi.dingtalk.com/robot/send?access_token=test".to_string(),
            Some("SEC_test_secret_key".to_string()),
        );

        let timestamp: i64 = 1700000000000;
        let sign = channel.sign(timestamp);

        assert!(
            sign.is_some(),
            "sign should produce a value when secret is set"
        );

        let sign_value = sign.unwrap();
        // Verify it's valid base64
        assert!(
            base64::engine::general_purpose::STANDARD
                .decode(&sign_value)
                .is_ok(),
            "sign should be valid base64"
        );

        // Verify deterministic: same input produces same output
        let sign2 = channel.sign(timestamp).unwrap();
        assert_eq!(sign_value, sign2, "sign should be deterministic");

        // Verify different timestamp produces different sign
        let sign3 = channel.sign(timestamp + 1000).unwrap();
        assert_ne!(
            sign_value, sign3,
            "different timestamp should produce different sign"
        );

        // Verify no secret produces None
        let channel_no_secret = DingTalkChannel::new(
            "ch-2".to_string(),
            "https://oapi.dingtalk.com/robot/send?access_token=test".to_string(),
            None,
        );
        assert!(channel_no_secret.sign(timestamp).is_none());
    }

    #[test]
    fn test_dingtalk_message_format() {
        let channel = DingTalkChannel::new(
            "ch-1".to_string(),
            "https://oapi.dingtalk.com/robot/send?access_token=test".to_string(),
            None,
        );

        // Test Critical severity
        let alert = AlertEvent {
            id: "alert-1".to_string(),
            rule_id: "service_crash".to_string(),
            severity: Severity::Critical,
            project_id: Some(1),
            project_name: Some("my-project".to_string()),
            title: "服务异常退出".to_string(),
            message: "进程意外退出 (exit code: 137)".to_string(),
            context: HashMap::from([("exit_code".to_string(), "137".to_string())]),
            fired_at: Utc::now(),
        };

        let body = channel.format_alert_message(&alert);

        // Verify structure
        assert_eq!(body["msgtype"], "markdown");
        let text = body["markdown"]["text"].as_str().unwrap();
        let title = body["markdown"]["title"].as_str().unwrap();

        // Critical should have red circle emoji
        assert!(title.contains("🔴"), "critical should use red icon");
        assert!(text.contains("🔴"), "critical text should use red icon");
        assert!(text.contains("my-project"), "should include project name");
        assert!(text.contains("进程意外退出"), "should include message");
        assert!(text.contains("exit_code"), "should include context");
        assert!(text.contains("137"), "should include context value");

        // Test Warning severity
        let warning_alert = AlertEvent {
            severity: Severity::Warning,
            ..alert.clone()
        };
        let warning_body = channel.format_alert_message(&warning_alert);
        let warning_title = warning_body["markdown"]["title"].as_str().unwrap();
        assert!(
            warning_title.contains("🟡"),
            "warning should use yellow icon"
        );

        // Test Info severity
        let info_alert = AlertEvent {
            severity: Severity::Info,
            ..alert.clone()
        };
        let info_body = channel.format_alert_message(&info_alert);
        let info_title = info_body["markdown"]["title"].as_str().unwrap();
        assert!(info_title.contains("🔵"), "info should use blue icon");
    }

    #[test]
    fn test_dingtalk_message_format_without_project() {
        let channel = DingTalkChannel::new(
            "ch-1".to_string(),
            "https://oapi.dingtalk.com/robot/send?access_token=test".to_string(),
            None,
        );

        let alert = AlertEvent {
            id: "alert-2".to_string(),
            rule_id: "api_unreachable".to_string(),
            severity: Severity::Warning,
            project_id: None,
            project_name: None,
            title: "API 不可达".to_string(),
            message: "GitLab API 连续 5 次请求失败".to_string(),
            context: HashMap::new(),
            fired_at: Utc::now(),
        };

        let body = channel.format_alert_message(&alert);
        let text = body["markdown"]["text"].as_str().unwrap();

        // When project_name is None, should show "系统"
        assert!(
            text.contains("系统"),
            "should show '系统' when project_name is None"
        );
        // Should not contain context section when context is empty
        assert!(
            !text.contains("上下文"),
            "should not show context section when empty"
        );
    }
}
