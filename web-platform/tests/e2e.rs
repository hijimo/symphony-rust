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
async fn test_e2e_admin_full_flow() {
    let app = common::TestApp::new().await;

    // Admin creates a user
    let create_resp = app
        .post(
            "/api/admin/users",
            &serde_json::json!({
                "username": "e2e_user1",
                "password": "UserPass123",
                "role": "user",
                "displayName": "E2E User"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(create_resp.status(), StatusCode::OK);

    // List users and verify the new user is present
    let list_resp = app
        .get("/api/admin/users?search=e2e_user1", Some(&app.admin_token))
        .await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let records = list_body["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    let user_id = records[0]["id"].as_i64().unwrap();

    // Reset user password
    let reset_resp = app
        .put(
            &format!("/api/admin/users/{}/reset-password", user_id),
            &serde_json::json!({ "newPassword": "ResetPass456" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(reset_resp.status(), StatusCode::OK);

    // Verify new password works
    let login_resp = app.login("e2e_user1", "ResetPass456").await;
    assert_eq!(login_resp.status(), StatusCode::OK);

    // Delete user
    let del_resp = app
        .delete(
            &format!("/api/admin/users/{}", user_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    // Verify deleted user cannot login
    let login_after_del = app.login("e2e_user1", "ResetPass456").await;
    assert_eq!(login_after_del.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_e2e_user_full_flow() {
    let app = common::TestApp::new().await;

    // Admin creates user
    app.create_test_user("e2e_flow", "InitPass123", "user")
        .await;

    // User logs in
    let token = app.login_get_token("e2e_flow", "InitPass123").await;

    // User updates profile
    let profile_resp = app
        .put(
            "/api/user/profile",
            &serde_json::json!({ "displayName": "E2E Flow User" }),
            Some(&token),
        )
        .await;
    assert_eq!(profile_resp.status(), StatusCode::OK);

    // User configures token
    let config_resp = app
        .put(
            "/api/user/config",
            &serde_json::json!({
                "gitlabToken": "glpat-test-token",
                "gitlabHost": "https://gitlab.example.com"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(config_resp.status(), StatusCode::OK);

    // Verify config
    let get_config = app.get("/api/user/config", Some(&token)).await;
    let cfg_body: serde_json::Value = get_config.json().await.unwrap();
    assert_eq!(cfg_body["data"]["hasGitlabToken"].as_bool().unwrap(), true);

    // User changes password
    let chg_resp = app
        .put(
            "/api/auth/password",
            &serde_json::json!({
                "oldPassword": "InitPass123",
                "newPassword": "NewPass789"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(chg_resp.status(), StatusCode::OK);

    // Login with new password
    let new_login = app.login("e2e_flow", "NewPass789").await;
    assert_eq!(new_login.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_e2e_token_invalidation_on_password_change() {
    let app = common::TestApp::new().await;
    app.create_test_user("e2e_token", "Pass123456", "user")
        .await;

    // Get initial token
    let old_token = app.login_get_token("e2e_token", "Pass123456").await;

    // Verify old token works
    let profile_resp = app.get("/api/user/profile", Some(&old_token)).await;
    assert_eq!(profile_resp.status(), StatusCode::OK);

    // Wait to ensure the token's iat is strictly before the invalidation timestamp
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // Change password (invalidates old tokens)
    let chg_resp = app
        .put(
            "/api/auth/password",
            &serde_json::json!({
                "oldPassword": "Pass123456",
                "newPassword": "NewPass789"
            }),
            Some(&old_token),
        )
        .await;
    assert_eq!(chg_resp.status(), StatusCode::OK);

    // Old token should now be invalid
    let old_token_resp = app.get("/api/user/profile", Some(&old_token)).await;
    assert_eq!(old_token_resp.status(), StatusCode::UNAUTHORIZED);

    // New login should work
    let new_token = app.login_get_token("e2e_token", "NewPass789").await;
    let new_profile = app.get("/api/user/profile", Some(&new_token)).await;
    assert_eq!(new_profile.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_e2e_privilege_escalation_blocked() {
    let app = common::TestApp::new().await;
    app.create_test_user("e2e_priv", "Pass123456", "user").await;
    let user_token = app.login_get_token("e2e_priv", "Pass123456").await;

    // User cannot list admin users
    let list_resp = app.get("/api/admin/users", Some(&user_token)).await;
    assert_eq!(list_resp.status(), StatusCode::FORBIDDEN);

    // User cannot create users
    let create_resp = app
        .post(
            "/api/admin/users",
            &serde_json::json!({
                "username": "hacker",
                "password": "Pass123456",
                "role": "admin"
            }),
            Some(&user_token),
        )
        .await;
    assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);

    // User cannot delete users
    let del_resp = app.delete("/api/admin/users/1", Some(&user_token)).await;
    assert_eq!(del_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_e2e_deleted_user_token_invalid() {
    let app = common::TestApp::new().await;
    app.create_test_user("e2e_del", "Pass123456", "user").await;

    let user_token = app.login_get_token("e2e_del", "Pass123456").await;

    // Verify token works
    let resp = app.get("/api/user/profile", Some(&user_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Admin deletes the user
    let list_resp = app
        .get("/api/admin/users?search=e2e_del", Some(&app.admin_token))
        .await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let user_id = list_body["data"]["records"][0]["id"].as_i64().unwrap();

    app.delete(
        &format!("/api/admin/users/{}", user_id),
        Some(&app.admin_token),
    )
    .await;

    // Deleted user's token should fail (user not found on profile lookup)
    let after_del = app.get("/api/user/profile", Some(&user_token)).await;
    assert!(
        after_del.status() == StatusCode::UNAUTHORIZED
            || after_del.status() == StatusCode::NOT_FOUND
    );
}
