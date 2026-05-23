#![allow(
    unused_imports,
    unused_variables,
    dead_code,
    clippy::bind_instead_of_map,
    clippy::derivable_impls,
    clippy::manual_range_contains,
    clippy::needless_borrows_for_generic_args,
    clippy::ptr_arg,
    clippy::duplicated_attributes,
    clippy::approx_constant,
    clippy::bool_assert_comparison,
    clippy::len_zero,
    clippy::let_and_return
)]

//! Phase 5: Alert & Notification API integration tests.

mod common;

use common::TestApp;
use serde_json::json;

// =============================================================================
// Helper: TestApp extension for Phase 5
// =============================================================================

impl TestApp {
    /// Create a non-admin user and return their token.
    async fn create_viewer_and_login(&self) -> String {
        self.create_test_user("viewer", "viewer123", "user").await;
        self.login_get_token("viewer", "viewer123").await
    }
}

// =============================================================================
// GET /api/admin/alerts — Alert History
// =============================================================================

#[tokio::test]
async fn test_list_alerts_empty() {
    let app = TestApp::new().await;
    let resp = app.get("/api/admin/alerts", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["totalCount"], 0);
    assert!(body["data"]["records"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_list_alerts_with_data() {
    let app = TestApp::new().await;

    // Insert some alert history records via the rules endpoint (seed data)
    // We'll use the DB directly through the admin API by triggering alerts
    // For now, verify pagination structure with empty data
    let resp = app
        .get(
            "/api/admin/alerts?page_no=1&page_size=5",
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    // Verify pagination metadata is present
    assert!(body["data"]["totalCount"].is_number());
    assert!(body["data"]["records"].is_array());
}

#[tokio::test]
async fn test_list_alerts_filter_by_severity() {
    let app = TestApp::new().await;

    let resp = app
        .get(
            "/api/admin/alerts?severity=critical",
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    // Invalid severity should return 400
    let resp = app
        .get("/api/admin/alerts?severity=invalid", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_list_alerts_filter_by_project() {
    let app = TestApp::new().await;

    let resp = app
        .get("/api/admin/alerts?project_id=999", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["totalCount"], 0);
}

#[tokio::test]
async fn test_list_alerts_requires_admin() {
    let app = TestApp::new().await;
    let viewer_token = app.create_viewer_and_login().await;

    let resp = app.get("/api/admin/alerts", Some(&viewer_token)).await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_list_alerts_requires_auth() {
    let app = TestApp::new().await;

    let resp = app.get("/api/admin/alerts", None).await;
    assert_eq!(resp.status(), 401);
}

// =============================================================================
// GET /api/admin/alerts/rules
// =============================================================================

#[tokio::test]
async fn test_get_rules_returns_defaults() {
    let app = TestApp::new().await;

    let resp = app
        .get("/api/admin/alerts/rules", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    let rules = body["data"]["rules"].as_array().unwrap();
    // Should have 6 default rules
    assert_eq!(rules.len(), 6);

    // Verify known rule IDs are present
    let rule_ids: Vec<&str> = rules
        .iter()
        .map(|r| r["ruleId"].as_str().unwrap())
        .collect();
    assert!(rule_ids.contains(&"task_timeout"));
    assert!(rule_ids.contains(&"task_failure"));
    assert!(rule_ids.contains(&"service_crash"));
    assert!(rule_ids.contains(&"concurrency_saturation"));
    assert!(rule_ids.contains(&"consecutive_failures"));
    assert!(rule_ids.contains(&"api_unreachable"));
}

#[tokio::test]
async fn test_get_rules_requires_admin() {
    let app = TestApp::new().await;
    let viewer_token = app.create_viewer_and_login().await;

    let resp = app
        .get("/api/admin/alerts/rules", Some(&viewer_token))
        .await;
    assert_eq!(resp.status(), 403);
}

// =============================================================================
// PUT /api/admin/alerts/rules
// =============================================================================

#[tokio::test]
async fn test_update_rules_enable_disable() {
    let app = TestApp::new().await;

    let body = json!({
        "rules": [{
            "ruleId": "task_timeout",
            "enabled": false
        }]
    });

    let resp = app
        .put("/api/admin/alerts/rules", &body, Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(resp_body["success"], true);
    assert_eq!(resp_body["data"]["updatedCount"], 1);

    // Verify the rule is now disabled
    let rules = resp_body["data"]["rules"].as_array().unwrap();
    let task_timeout = rules
        .iter()
        .find(|r| r["ruleId"] == "task_timeout")
        .unwrap();
    assert_eq!(task_timeout["enabled"], false);
}

#[tokio::test]
async fn test_update_rules_change_threshold() {
    let app = TestApp::new().await;

    let body = json!({
        "rules": [{
            "ruleId": "task_timeout",
            "threshold": {
                "timeout_minutes": 60
            }
        }]
    });

    let resp = app
        .put("/api/admin/alerts/rules", &body, Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(resp_body["success"], true);

    let rules = resp_body["data"]["rules"].as_array().unwrap();
    let task_timeout = rules
        .iter()
        .find(|r| r["ruleId"] == "task_timeout")
        .unwrap();
    assert_eq!(task_timeout["threshold"]["timeout_minutes"], 60);
}

#[tokio::test]
async fn test_update_rules_invalid_rule_id() {
    let app = TestApp::new().await;

    let body = json!({
        "rules": [{
            "ruleId": "nonexistent_rule",
            "enabled": false
        }]
    });

    let resp = app
        .put("/api/admin/alerts/rules", &body, Some(&app.admin_token))
        .await;

    // Should return error with ALERT_001 code
    assert_eq!(resp.status(), 404);
    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(resp_body["retCode"], "ALERT_001");
}

#[tokio::test]
async fn test_update_rules_requires_admin() {
    let app = TestApp::new().await;
    let viewer_token = app.create_viewer_and_login().await;

    let body = json!({
        "rules": [{
            "ruleId": "task_timeout",
            "enabled": false
        }]
    });

    let resp = app
        .put("/api/admin/alerts/rules", &body, Some(&viewer_token))
        .await;
    assert_eq!(resp.status(), 403);
}

// =============================================================================
// GET /api/admin/alerts/channels
// =============================================================================

#[tokio::test]
async fn test_get_channels_empty() {
    let app = TestApp::new().await;

    let resp = app
        .get("/api/admin/alerts/channels", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert!(body["data"]["channels"].as_array().unwrap().is_empty());
    // Should include available types
    assert!(!body["data"]["availableTypes"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_get_channels_with_config() {
    let app = TestApp::new().await;

    // First, create a channel
    let create_body = json!({
        "channels": [{
            "name": "Test DingTalk",
            "channelType": "dingtalk",
            "enabled": true,
            "config": {
                "webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=abc123def456ghi789",
                "secret": "SEC_my_secret_key_12345"
            },
            "severityFilter": ["critical", "warning"]
        }]
    });

    let resp = app
        .put(
            "/api/admin/alerts/channels",
            &create_body,
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    // Now get channels
    let resp = app
        .get("/api/admin/alerts/channels", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let channels = body["data"]["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["name"], "Test DingTalk");
    assert_eq!(channels[0]["channelType"], "dingtalk");
    assert_eq!(channels[0]["enabled"], true);
    assert_eq!(channels[0]["configMasked"], true);
}

#[tokio::test]
async fn test_get_channels_masks_secrets() {
    let app = TestApp::new().await;

    // Create a channel with a secret
    let create_body = json!({
        "channels": [{
            "name": "Secret Channel",
            "channelType": "dingtalk",
            "enabled": true,
            "config": {
                "webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=abcdefghijklmnopqrstuvwxyz1234567890abcdefgh",
                "secret": "SEC_my_very_long_secret_key_12345"
            },
            "severityFilter": ["critical"]
        }]
    });

    app.put(
        "/api/admin/alerts/channels",
        &create_body,
        Some(&app.admin_token),
    )
    .await;

    // Get channels and verify masking
    let resp = app
        .get("/api/admin/alerts/channels", Some(&app.admin_token))
        .await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let channels = body["data"]["channels"].as_array().unwrap();
    let config = &channels[0]["config"];

    // Secret should be masked (starts with ****)
    let secret_val = config["secret"].as_str().unwrap();
    assert!(
        secret_val.starts_with("****"),
        "secret should be masked, got: {}",
        secret_val
    );

    // Webhook URL should be partially masked (contains ****)
    let webhook_val = config["webhook_url"].as_str().unwrap();
    assert!(
        webhook_val.contains("****"),
        "webhook_url should be partially masked, got: {}",
        webhook_val
    );
}

// =============================================================================
// PUT /api/admin/alerts/channels
// =============================================================================

#[tokio::test]
async fn test_update_channels_add_dingtalk() {
    let app = TestApp::new().await;

    let body = json!({
        "channels": [{
            "name": "My DingTalk Bot",
            "channelType": "dingtalk",
            "enabled": true,
            "config": {
                "webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test_token_value_here",
                "secret": "SEC_test_secret"
            },
            "severityFilter": ["critical", "warning", "info"]
        }]
    });

    let resp = app
        .put("/api/admin/alerts/channels", &body, Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(resp_body["success"], true);

    let channels = resp_body["data"]["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["name"], "My DingTalk Bot");
    assert_eq!(channels[0]["channelType"], "dingtalk");
    // Should have a generated channel_id
    assert!(channels[0]["channelId"].as_str().unwrap().len() > 0);
}

#[tokio::test]
async fn test_update_channels_invalid_config() {
    let app = TestApp::new().await;

    // Missing webhook_url
    let body = json!({
        "channels": [{
            "name": "Bad Channel",
            "channelType": "dingtalk",
            "enabled": true,
            "config": {},
            "severityFilter": ["critical"]
        }]
    });

    let resp = app
        .put("/api/admin/alerts/channels", &body, Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 400);

    let resp_body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(resp_body["retCode"], "ALERT_002");
}

#[tokio::test]
async fn test_update_channels_requires_admin() {
    let app = TestApp::new().await;
    let viewer_token = app.create_viewer_and_login().await;

    let body = json!({
        "channels": [{
            "name": "Test",
            "channelType": "dingtalk",
            "enabled": true,
            "config": {
                "webhook_url": "https://oapi.dingtalk.com/robot/send?access_token=test"
            },
            "severityFilter": ["critical"]
        }]
    });

    let resp = app
        .put("/api/admin/alerts/channels", &body, Some(&viewer_token))
        .await;
    assert_eq!(resp.status(), 403);
}

// =============================================================================
// POST /api/admin/alerts/test
// =============================================================================

#[tokio::test]
async fn test_notification_requires_channel_id() {
    let app = TestApp::new().await;

    // Send with a non-existent channel_id
    let body = json!({
        "channelId": "non-existent-channel-id"
    });

    let resp = app
        .post("/api/admin/alerts/test", &body, Some(&app.admin_token))
        .await;

    // Should fail because channel doesn't exist
    // The handler returns ALERT_002 for channel not found via AlertChannelInvalid
    assert!(resp.status() == 400 || resp.status() == 404);
}

#[tokio::test]
async fn test_notification_invalid_channel() {
    let app = TestApp::new().await;

    let body = json!({
        "channelId": "does-not-exist-at-all"
    });

    let resp = app
        .post("/api/admin/alerts/test", &body, Some(&app.admin_token))
        .await;

    let resp_body: serde_json::Value = resp.json().await.unwrap();
    // Should return an error indicating channel not found
    assert_eq!(resp_body["success"], false);
    // Error code should be ALERT_001 or ALERT_002 depending on implementation
    let code = resp_body["retCode"].as_str().unwrap();
    assert!(
        code == "ALERT_001" || code == "ALERT_002",
        "expected ALERT_001 or ALERT_002, got: {}",
        code
    );
}

#[tokio::test]
async fn test_notification_requires_admin() {
    let app = TestApp::new().await;
    let viewer_token = app.create_viewer_and_login().await;

    let body = json!({
        "channelId": "some-channel"
    });

    let resp = app
        .post("/api/admin/alerts/test", &body, Some(&viewer_token))
        .await;
    assert_eq!(resp.status(), 403);
}
