mod common;

use reqwest::StatusCode;

#[tokio::test]
async fn test_login_valid_credentials_returns_token() {
    let app = common::TestApp::new().await;
    let resp = app.login("admin", "admin123").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["data"]["token"].is_string());
    assert!(body["data"]["user"]["username"].as_str().unwrap() == "admin");
    assert!(body["success"].as_bool().unwrap());
}

#[tokio::test]
async fn test_login_wrong_password_returns_401() {
    let app = common::TestApp::new().await;
    let resp = app.login("admin", "wrongpassword").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_login_nonexistent_user_returns_401() {
    let app = common::TestApp::new().await;
    let resp = app.login("ghost_user", "anypassword").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_login_empty_username_returns_400() {
    let app = common::TestApp::new().await;
    let resp = app.login("", "admin123").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_login_empty_password_returns_400() {
    let app = common::TestApp::new().await;
    let resp = app.login("admin", "").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_login_rate_limit_triggers() {
    let app = common::TestApp::new().await;
    for _ in 0..5 {
        app.login("admin", "wrongpass").await;
    }
    let resp = app.login("admin", "wrongpass").await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["retMsg"]
        .as_str()
        .unwrap()
        .contains("too many login attempts"));
}

#[tokio::test]
async fn test_change_password_success() {
    let app = common::TestApp::new().await;
    app.create_test_user("chgpass", "OldPass123", "user").await;
    let token = app.login_get_token("chgpass", "OldPass123").await;

    let resp = app
        .put(
            "/api/auth/password",
            &serde_json::json!({
                "oldPassword": "OldPass123",
                "newPassword": "NewPass456"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let login_resp = app.login("chgpass", "NewPass456").await;
    assert_eq!(login_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_change_password_wrong_old_password() {
    let app = common::TestApp::new().await;
    app.create_test_user("chgpass2", "OldPass123", "user").await;
    let token = app.login_get_token("chgpass2", "OldPass123").await;

    let resp = app
        .put(
            "/api/auth/password",
            &serde_json::json!({
                "oldPassword": "WrongOld",
                "newPassword": "NewPass456"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_change_password_unauthenticated_returns_401() {
    let app = common::TestApp::new().await;
    let resp = app
        .put(
            "/api/auth/password",
            &serde_json::json!({
                "oldPassword": "OldPass123",
                "newPassword": "NewPass456"
            }),
            None,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
