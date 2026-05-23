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

mod common;

use reqwest::StatusCode;

#[tokio::test]
async fn test_list_users_returns_paginated_data() {
    let app = common::TestApp::new().await;
    let resp = app.get("/api/admin/users", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["data"]["records"].is_array());
    assert!(body["data"]["totalCount"].as_i64().unwrap() >= 1);
}

#[tokio::test]
async fn test_list_users_search_filter() {
    let app = common::TestApp::new().await;
    app.create_test_user("searchme", "Pass123456", "user").await;
    app.create_test_user("another", "Pass123456", "user").await;

    let resp = app
        .get("/api/admin/users?search=searchme", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["username"].as_str().unwrap(), "searchme");
}

#[tokio::test]
async fn test_list_users_role_filter() {
    let app = common::TestApp::new().await;
    app.create_test_user("roleuser", "Pass123456", "user").await;

    let resp = app
        .get("/api/admin/users?role=user", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    for r in records {
        assert_eq!(r["role"].as_str().unwrap(), "user");
    }
}

#[tokio::test]
async fn test_create_user_success() {
    let app = common::TestApp::new().await;
    let resp = app
        .post(
            "/api/admin/users",
            &serde_json::json!({
                "username": "newuser",
                "password": "Pass123456",
                "role": "user",
                "displayName": "New User"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["success"].as_bool().unwrap());
}

#[tokio::test]
async fn test_create_user_duplicate_username_returns_409() {
    let app = common::TestApp::new().await;
    app.create_test_user("dupuser", "Pass123456", "user").await;

    let resp = app
        .post(
            "/api/admin/users",
            &serde_json::json!({
                "username": "dupuser",
                "password": "Pass123456",
                "role": "user"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_create_user_invalid_username_returns_400() {
    let app = common::TestApp::new().await;
    let resp = app
        .post(
            "/api/admin/users",
            &serde_json::json!({
                "username": "ab",
                "password": "Pass123456",
                "role": "user"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_user_special_chars_username_returns_400() {
    let app = common::TestApp::new().await;
    let resp = app
        .post(
            "/api/admin/users",
            &serde_json::json!({
                "username": "user@name",
                "password": "Pass123456",
                "role": "user"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_delete_user_success() {
    let app = common::TestApp::new().await;
    app.create_test_user("todelete", "Pass123456", "user").await;

    let list_resp = app
        .get("/api/admin/users?search=todelete", Some(&app.admin_token))
        .await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let user_id = list_body["data"]["records"][0]["id"].as_i64().unwrap();

    let resp = app
        .delete(
            &format!("/api/admin/users/{}", user_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let login_resp = app.login("todelete", "Pass123456").await;
    assert_eq!(login_resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_delete_self_returns_400() {
    let app = common::TestApp::new().await;

    let list_resp = app
        .get("/api/admin/users?search=admin", Some(&app.admin_token))
        .await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let admin_id = list_body["data"]["records"][0]["id"].as_i64().unwrap();

    let resp = app
        .delete(
            &format!("/api/admin/users/{}", admin_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_reset_password_success() {
    let app = common::TestApp::new().await;
    app.create_test_user("resetme", "OldPass123", "user").await;

    let list_resp = app
        .get("/api/admin/users?search=resetme", Some(&app.admin_token))
        .await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let user_id = list_body["data"]["records"][0]["id"].as_i64().unwrap();

    let resp = app
        .put(
            &format!("/api/admin/users/{}/reset-password", user_id),
            &serde_json::json!({ "newPassword": "ResetPass789" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let login_old = app.login("resetme", "OldPass123").await;
    assert_eq!(login_old.status(), StatusCode::UNAUTHORIZED);

    let login_new = app.login("resetme", "ResetPass789").await;
    assert_eq!(login_new.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_reset_own_password_returns_400() {
    let app = common::TestApp::new().await;

    let list_resp = app
        .get("/api/admin/users?search=admin", Some(&app.admin_token))
        .await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let admin_id = list_body["data"]["records"][0]["id"].as_i64().unwrap();

    let resp = app
        .put(
            &format!("/api/admin/users/{}/reset-password", admin_id),
            &serde_json::json!({ "newPassword": "NewPass789" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_non_admin_access_returns_403() {
    let app = common::TestApp::new().await;
    app.create_test_user("regular", "Pass123456", "user").await;
    let user_token = app.login_get_token("regular", "Pass123456").await;

    let resp = app.get("/api/admin/users", Some(&user_token)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_unauthenticated_access_returns_401() {
    let app = common::TestApp::new().await;
    let resp = app.get("/api/admin/users", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
