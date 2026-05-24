mod common;

use reqwest::StatusCode;
use serde_json::json;
use web_platform::models::{ServiceStatus, ServiceStatusUpdate};
use web_platform::repository::ProjectRepository;

#[tokio::test]
async fn test_create_project_success() {
    let app = common::TestApp::new().await;
    let resp = app
        .post(
            "/api/projects",
            &json!({
                "git_url": "https://github.com/owner/my-repo"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["retCode"], "0");
    assert_eq!(body["data"]["platform"], "github");
    assert_eq!(body["data"]["namespace"], "owner");
    assert_eq!(body["data"]["repoName"], "my-repo");
    assert_eq!(body["data"]["name"], "my-repo");
    assert_eq!(body["data"]["myRole"], "owner");
    assert!(body["data"]["id"].as_i64().unwrap() > 0);
}

#[tokio::test]
async fn test_create_project_invalid_url() {
    let app = common::TestApp::new().await;
    let resp = app
        .post(
            "/api/projects",
            &json!({
                "git_url": "not-a-valid-url"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["retCode"], "BIZ_001");
}

#[tokio::test]
async fn test_create_project_duplicate_url() {
    let app = common::TestApp::new().await;

    // Create first project
    let resp = app
        .post(
            "/api/projects",
            &json!({
                "git_url": "https://github.com/owner/dup-repo"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Try to create duplicate
    let resp = app
        .post(
            "/api/projects",
            &json!({
                "git_url": "https://github.com/owner/dup-repo"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["retCode"], "BIZ_003");
}

#[tokio::test]
async fn test_create_project_no_auth() {
    let app = common::TestApp::new().await;
    let resp = app
        .post(
            "/api/projects",
            &json!({
                "git_url": "https://github.com/owner/repo"
            }),
            None,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_001");
}

#[tokio::test]
async fn test_list_projects_admin_sees_all() {
    let app = common::TestApp::new().await;

    // Create a regular user and have them create a project
    app.create_test_user("projuser1", "Pass123456", "user")
        .await;
    let user_token = app.login_get_token("projuser1", "Pass123456").await;
    app.create_test_project("https://github.com/user1/repo-a", &user_token)
        .await;

    // Admin should see the project even though they're not a member
    let resp = app.get("/api/projects", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert!(body["data"]["totalCount"].as_i64().unwrap() >= 1);
}

#[tokio::test]
async fn test_list_projects_user_sees_own() {
    let app = common::TestApp::new().await;

    // Create two users, each with their own project
    app.create_test_user("projuser2", "Pass123456", "user")
        .await;
    app.create_test_user("projuser3", "Pass123456", "user")
        .await;
    let token2 = app.login_get_token("projuser2", "Pass123456").await;
    let token3 = app.login_get_token("projuser3", "Pass123456").await;

    app.create_test_project("https://github.com/user2/repo-b", &token2)
        .await;
    app.create_test_project("https://github.com/user3/repo-c", &token3)
        .await;

    // User2 should only see their own project
    let resp = app.get("/api/projects", Some(&token2)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["totalCount"].as_i64().unwrap(), 1);
    let records = body["data"]["records"].as_array().unwrap();
    assert_eq!(records[0]["repoName"], "repo-b");
}

#[tokio::test]
async fn test_list_projects_pagination() {
    let app = common::TestApp::new().await;

    // Create multiple projects
    for i in 0..5 {
        app.create_test_project(
            &format!("https://github.com/owner/page-repo-{}", i),
            &app.admin_token,
        )
        .await;
    }

    // Request page 1 with size 2
    let resp = app
        .get(
            "/api/projects?page_no=1&page_size=2",
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["pageNo"], 1);
    assert_eq!(body["data"]["pageSize"], 2);
    assert_eq!(body["data"]["records"].as_array().unwrap().len(), 2);
    assert!(body["data"]["totalCount"].as_i64().unwrap() >= 5);

    // Request page 2
    let resp = app
        .get(
            "/api/projects?page_no=2&page_size=2",
            Some(&app.admin_token),
        )
        .await;
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["pageNo"], 2);
    assert_eq!(body["data"]["records"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_list_projects_filter_by_platform() {
    let app = common::TestApp::new().await;

    app.create_test_project("https://github.com/owner/gh-proj", &app.admin_token)
        .await;
    app.create_test_project("https://gitlab.com/group/gl-proj", &app.admin_token)
        .await;

    let resp = app
        .get("/api/projects?platform=github", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    for r in records {
        assert_eq!(r["platform"], "github");
    }
}

#[tokio::test]
async fn test_list_projects_filter_by_status() {
    let app = common::TestApp::new().await;

    app.create_test_project("https://github.com/owner/status-proj", &app.admin_token)
        .await;

    // All new projects start as "stopped"
    let resp = app
        .get("/api/projects?status=stopped", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    for r in records {
        assert_eq!(r["serviceStatus"], "stopped");
    }

    // Filter by "running" should return none (no projects started)
    let resp = app
        .get("/api/projects?status=running", Some(&app.admin_token))
        .await;
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["totalCount"].as_i64().unwrap(), 0);
}

#[tokio::test]
async fn test_get_project_success() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/get-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .get(
            &format!("/api/projects/{}", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["id"], project_id);
    assert_eq!(body["data"]["repoName"], "get-proj");
}

#[tokio::test]
async fn test_get_project_not_found() {
    let app = common::TestApp::new().await;
    let resp = app.get("/api/projects/99999", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_002");
}

#[tokio::test]
async fn test_get_project_non_member() {
    let app = common::TestApp::new().await;

    // Create project as admin
    let create_body = app
        .create_test_project("https://github.com/owner/private-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    // Create a regular user who is NOT a member
    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let resp = app
        .get(
            &format!("/api/projects/{}", project_id),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");
}

#[tokio::test]
async fn test_update_project_owner() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/update-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .put(
            &format!("/api/projects/{}", project_id),
            &json!({
                "name": "Updated Name",
                "description": "New description",
                "max_concurrent_agents": 5
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["name"], "Updated Name");
    assert_eq!(body["data"]["description"], "New description");
    assert_eq!(body["data"]["maxConcurrentAgents"], 5);
}

#[tokio::test]
async fn test_update_project_member_forbidden() {
    let app = common::TestApp::new().await;

    // Create project as admin
    let create_body = app
        .create_test_project(
            "https://github.com/owner/member-update-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    // Create a user and add them as a member (not owner)
    app.create_test_user("memberonly", "Pass123456", "user")
        .await;
    let member_token = app.login_get_token("memberonly", "Pass123456").await;
    let member_user_id = app.get_user_id("memberonly").await;

    app.post(
        &format!("/api/projects/{}/members", project_id),
        &json!({
            "user_id": member_user_id,
            "role": "member"
        }),
        Some(&app.admin_token),
    )
    .await;

    // Member tries to update - should be forbidden
    let resp = app
        .put(
            &format!("/api/projects/{}", project_id),
            &json!({ "name": "Hacked Name" }),
            Some(&member_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");
}

#[tokio::test]
async fn test_delete_project_success() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/delete-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    // Project starts as "stopped", so deletion should succeed
    let resp = app
        .delete(
            &format!("/api/projects/{}", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    // Verify it's gone
    let get_resp = app
        .get(
            &format!("/api/projects/{}", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_project_running_service() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/running-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let status_update = ServiceStatusUpdate {
        status: ServiceStatus::Running,
        pid: Some(12345),
        error_message: None,
    };
    app.repo
        .update_service_status(project_id, &status_update)
        .await
        .unwrap();

    // Try to delete while running
    let resp = app
        .delete(
            &format!("/api/projects/{}", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_003");
}

#[tokio::test]
async fn test_delete_project_non_owner() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/nodelete-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    // Create a member user
    app.create_test_user("nodelete", "Pass123456", "user").await;
    let member_token = app.login_get_token("nodelete", "Pass123456").await;
    let member_user_id = app.get_user_id("nodelete").await;

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

    // Member tries to delete
    let resp = app
        .delete(
            &format!("/api/projects/{}", project_id),
            Some(&member_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");
}
