mod common;

use reqwest::StatusCode;
use serde_json::json;
use web_platform::models::{ServiceStatus, ServiceStatusUpdate};
use web_platform::repository::ProjectRepository;

async fn mark_project_running(app: &common::TestApp, project_id: i64) {
    let status_update = ServiceStatusUpdate {
        status: ServiceStatus::Running,
        pid: Some(12345),
        error_message: None,
    };
    app.repo
        .update_service_status(project_id, &status_update)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_start_service() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/start-svc-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .post(
            &format!("/api/projects/{}/start", project_id),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["status"], "running");
}

#[tokio::test]
async fn test_start_service_already_running() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/already-running-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    mark_project_running(&app, project_id).await;

    // Try to start again
    let resp = app
        .post(
            &format!("/api/projects/{}/start", project_id),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_003");
}

#[tokio::test]
async fn test_stop_service() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/stop-svc-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    mark_project_running(&app, project_id).await;

    // Stop
    let resp = app
        .post(
            &format!("/api/projects/{}/stop", project_id),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["status"], "stopped");
}

#[tokio::test]
async fn test_stop_service_already_stopped() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/already-stopped-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    // Try to stop a project that was never started
    let resp = app
        .post(
            &format!("/api/projects/{}/stop", project_id),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    // The handler returns Conflict when service is not running
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_003");
}

#[tokio::test]
async fn test_restart_service() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/restart-svc-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    mark_project_running(&app, project_id).await;

    // Restart
    let resp = app
        .post(
            &format!("/api/projects/{}/restart", project_id),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["status"], "running");
}

#[tokio::test]
async fn test_get_service_status() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/status-svc-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .get(
            &format!("/api/projects/{}/status", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["status"], "stopped");
}

#[tokio::test]
async fn test_service_control_non_owner() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/svc-perm-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    // Create a member user
    app.create_test_user("svcmember", "Pass123456", "user")
        .await;
    let member_user_id = app.get_user_id("svcmember").await;
    let member_token = app.login_get_token("svcmember", "Pass123456").await;

    // Add as member (not owner)
    app.post(
        &format!("/api/projects/{}/members", project_id),
        &json!({
            "user_id": member_user_id,
            "role": "member"
        }),
        Some(&app.admin_token),
    )
    .await;

    // Member tries to start - should be forbidden
    let resp = app
        .post(
            &format!("/api/projects/{}/start", project_id),
            &json!({}),
            Some(&member_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");

    // Member tries to stop - should be forbidden
    let resp = app
        .post(
            &format!("/api/projects/{}/stop", project_id),
            &json!({}),
            Some(&member_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Member tries to restart - should be forbidden
    let resp = app
        .post(
            &format!("/api/projects/{}/restart", project_id),
            &json!({}),
            Some(&member_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
