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

//! Phase 3 API integration tests: Full HTTP endpoint tests against real GitLab.
//!
//! These tests spin up the full web-platform server and test the Phase 3
//! kanban/issue endpoints end-to-end with a real GitLab instance.

mod common;

use reqwest::StatusCode;
use serde_json::Value;

const GITLAB_HOST: &str = "http://gitlab.jushuitan-inc.com:8081";
const GITLAB_PROJECT_URL: &str =
    "http://gitlab.jushuitan-inc.com:8081/zimei10525/symphony_e2e_test_repo";

fn gitlab_token() -> String {
    std::env::var("GITLAB_TOKEN")
        .expect("GITLAB_TOKEN must be set for Phase 3 GitLab integration tests")
}

/// Helper to set up a test environment with a project configured for the real GitLab.
async fn setup_phase3_test() -> (common::TestApp, i64, String) {
    let app = common::TestApp::new().await;
    let token = app.admin_token.clone();
    let gitlab_token = gitlab_token();

    // Configure admin's GitLab token (handler encrypts it internally)
    app.put(
        "/api/user/config",
        &serde_json::json!({
            "gitlabToken": gitlab_token,
            "gitlabHost": GITLAB_HOST
        }),
        Some(&token),
    )
    .await;

    // Create a project pointing to the real GitLab repo
    let project_resp = app.create_test_project(GITLAB_PROJECT_URL, &token).await;
    let project_id = app.get_project_id(&project_resp);

    (app, project_id, token)
}

// ==================== Kanban Endpoint Tests ====================

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_kanban_get_success() {
    let (app, project_id, token) = setup_phase3_test().await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&token),
        )
        .await;

    let status = resp.status();
    let body: Value = resp.json().await.unwrap();
    println!(
        "Kanban response status={}, body={}",
        status,
        serde_json::to_string_pretty(&body).unwrap()
    );
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert_eq!(body["retCode"], "0");

    // Verify kanban structure
    let data = &body["data"];
    assert!(data["todo"].is_object(), "todo column should exist");
    assert!(
        data["in_progress"].is_object(),
        "in_progress column should exist"
    );
    assert!(data["pr"].is_object(), "pr column should exist");
    assert!(data["todo"]["issues"].is_array());
    assert!(data["in_progress"]["issues"].is_array());
    assert!(data["pr"]["merge_requests"].is_array());

    // Verify todo column has total_count and has_more
    assert!(data["todo"]["total_count"].is_number());
    assert!(data["todo"]["has_more"].is_boolean());

    println!(
        "Kanban: todo={}, in_progress={}, pr={}",
        data["todo"]["issues"].as_array().unwrap().len(),
        data["in_progress"]["issues"].as_array().unwrap().len(),
        data["pr"]["merge_requests"].as_array().unwrap().len()
    );
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_kanban_with_todo_limit() {
    let (app, project_id, token) = setup_phase3_test().await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban?todo_limit=1", project_id),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    let todo_issues = body["data"]["todo"]["issues"].as_array().unwrap();
    assert!(todo_issues.len() <= 1, "Should respect todo_limit=1");
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_kanban_unauthorized() {
    let (app, project_id, _token) = setup_phase3_test().await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            None, // No token
        )
        .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["retCode"], "AUTH_001");
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_kanban_project_not_found() {
    let app = common::TestApp::new().await;

    let resp = app
        .get("/api/projects/99999/kanban", Some(&app.admin_token))
        .await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_kanban_non_member_access() {
    let (app, project_id, _token) = setup_phase3_test().await;

    // Create a regular user who is NOT a member of the project
    app.create_test_user("outsider", "pass1234", "user").await;
    let outsider_token = app.login_get_token("outsider", "pass1234").await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&outsider_token),
        )
        .await;

    // Should be forbidden (non-member)
    assert!(
        resp.status() == StatusCode::FORBIDDEN || resp.status() == StatusCode::NOT_FOUND,
        "Non-member should not access kanban, got: {}",
        resp.status()
    );
}

// ==================== Issue Creation Tests ====================

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_create_issue_success() {
    let (app, project_id, token) = setup_phase3_test().await;

    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &serde_json::json!({
                "title": format!("[API Test] Issue created at {}", timestamp),
                "description": "## 描述\n\n自动化测试创建的 Issue\n\n## Acceptance Criteria\n\n- [ ] 测试通过",
                "labels": ["test"]
            }),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["retCode"], "0");

    let data = &body["data"];
    assert!(data["iid"].is_number(), "Should return issue iid");
    assert!(data["title"].as_str().unwrap().contains("[API Test]"));
    assert_eq!(data["state"], "opened");
    assert!(data["web_url"].as_str().unwrap().contains("gitlab"));

    println!("Created issue #{}: {}", data["iid"], data["title"]);
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_create_issue_validation_title_too_long() {
    let (app, project_id, token) = setup_phase3_test().await;

    let long_title = "x".repeat(201);
    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &serde_json::json!({
                "title": long_title
            }),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["retCode"], "BIZ_001");
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_create_issue_validation_empty_title() {
    let (app, project_id, token) = setup_phase3_test().await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &serde_json::json!({
                "title": ""
            }),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_create_issue_unauthorized() {
    let (app, project_id, _token) = setup_phase3_test().await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &serde_json::json!({
                "title": "Should fail"
            }),
            None,
        )
        .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ==================== Issue Detail Tests ====================

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_get_issue_detail() {
    let (app, project_id, token) = setup_phase3_test().await;

    // Use issue #1 which we know exists from the GitLab client tests
    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1", project_id),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    let data = &body["data"];
    assert_eq!(data["iid"], 1);
    assert!(data["title"].is_string());
    assert!(data["state"].is_string());
    assert!(data["author"].is_object());
    assert!(data["web_url"].is_string());
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_get_issue_not_found() {
    let (app, project_id, token) = setup_phase3_test().await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/99999", project_id),
            Some(&token),
        )
        .await;

    // Should be 404 or 502 depending on how GitLab 404 is mapped
    assert!(
        resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::BAD_GATEWAY,
        "Expected 404 or 502 for non-existent issue, got: {}",
        resp.status()
    );
}

// ==================== Issue MRs Tests ====================

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_get_issue_mrs() {
    let (app, project_id, token) = setup_phase3_test().await;

    // Issue #8 has related MRs (from our GitLab client test)
    let resp = app
        .get(
            &format!("/api/projects/{}/issues/8/mrs", project_id),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    let data = body["data"].as_array().unwrap();
    println!("Issue #8 has {} related MRs", data.len());
    // We know from the client test that issue #8 has at least 1 MR
    assert!(!data.is_empty(), "Issue #8 should have related MRs");

    let mr = &data[0];
    assert!(mr["iid"].is_number());
    assert!(mr["title"].is_string());
    assert!(mr["state"].is_string());
}

// ==================== MR Detail Tests ====================

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_get_mr_detail() {
    let (app, project_id, token) = setup_phase3_test().await;

    // MR !2 exists (from our GitLab client test)
    let resp = app
        .get(&format!("/api/projects/{}/mrs/2", project_id), Some(&token))
        .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    let data = &body["data"];
    assert_eq!(data["iid"], 2);
    assert!(data["title"].is_string());
    assert!(data["state"].is_string());
    assert!(data["source_branch"].is_string());
    assert!(data["target_branch"].is_string());
    assert!(data["author"].is_object());
    assert!(data["web_url"].is_string());
}

// ==================== Cache Behavior Tests ====================

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_kanban_cache_behavior() {
    let (app, project_id, token) = setup_phase3_test().await;

    // First request - should not be cached
    let resp1 = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&token),
        )
        .await;
    assert_eq!(resp1.status(), StatusCode::OK);
    let body1: Value = resp1.json().await.unwrap();
    assert_eq!(body1["data"]["cached"], false);

    // Second request immediately - should be cached
    let resp2 = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&token),
        )
        .await;
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2: Value = resp2.json().await.unwrap();
    assert_eq!(
        body2["data"]["cached"], true,
        "Second request should be cached"
    );
}

// ==================== AI Generate Tests (rate limit only, no Azure) ====================

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_ai_generate_validation_prompt_too_short() {
    let (app, project_id, token) = setup_phase3_test().await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &serde_json::json!({
                "prompt": "hi"  // Too short (min 5 chars)
            }),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["retCode"], "BIZ_001");
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_ai_generate_validation_prompt_too_long() {
    let (app, project_id, token) = setup_phase3_test().await;

    let long_prompt = "x".repeat(2001);
    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &serde_json::json!({
                "prompt": long_prompt
            }),
            Some(&token),
        )
        .await;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["retCode"], "BIZ_001");
}

#[tokio::test]
#[ignore = "requires GITLAB_TOKEN and a real GitLab test project"]
async fn test_ai_generate_unauthorized() {
    let (app, project_id, _token) = setup_phase3_test().await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &serde_json::json!({
                "prompt": "修复登录页面的样式问题"
            }),
            None,
        )
        .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
