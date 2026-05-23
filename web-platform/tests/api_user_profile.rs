mod common;

use reqwest::StatusCode;

#[tokio::test]
async fn test_get_profile_success() {
    let app = common::TestApp::new().await;
    let resp = app.get("/api/user/profile", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["username"].as_str().unwrap(), "admin");
    assert_eq!(body["data"]["role"].as_str().unwrap(), "admin");
}

#[tokio::test]
async fn test_update_display_name_success() {
    let app = common::TestApp::new().await;
    app.create_test_user("profuser", "Pass123456", "user").await;
    let token = app.login_get_token("profuser", "Pass123456").await;

    let resp = app
        .put(
            "/api/user/profile",
            &serde_json::json!({ "displayName": "New Display Name" }),
            Some(&token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let profile_resp = app.get("/api/user/profile", Some(&token)).await;
    let body: serde_json::Value = profile_resp.json().await.unwrap();
    assert_eq!(
        body["data"]["displayName"].as_str().unwrap(),
        "New Display Name"
    );
}

#[tokio::test]
async fn test_get_config_no_config_returns_defaults() {
    let app = common::TestApp::new().await;
    app.create_test_user("cfguser", "Pass123456", "user").await;
    let token = app.login_get_token("cfguser", "Pass123456").await;

    let resp = app.get("/api/user/config", Some(&token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["hasGitlabToken"].as_bool().unwrap(), false);
    assert_eq!(body["data"]["hasGithubToken"].as_bool().unwrap(), false);
    assert!(body["data"]["gitlabHost"].is_null());
}

#[tokio::test]
async fn test_update_config_set_token() {
    let app = common::TestApp::new().await;
    app.create_test_user("cfgset", "Pass123456", "user").await;
    let token = app.login_get_token("cfgset", "Pass123456").await;

    let resp = app
        .put(
            "/api/user/config",
            &serde_json::json!({
                "gitlabToken": "glpat-xxxxxxxxxxxx",
                "gitlabHost": "https://gitlab.example.com"
            }),
            Some(&token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let config_resp = app.get("/api/user/config", Some(&token)).await;
    let body: serde_json::Value = config_resp.json().await.unwrap();
    assert_eq!(body["data"]["hasGitlabToken"].as_bool().unwrap(), true);
    assert_eq!(
        body["data"]["gitlabHost"].as_str().unwrap(),
        "https://gitlab.example.com"
    );
}

#[tokio::test]
async fn test_get_config_token_not_leaked() {
    let app = common::TestApp::new().await;
    app.create_test_user("cfgleak", "Pass123456", "user").await;
    let token = app.login_get_token("cfgleak", "Pass123456").await;

    app.put(
        "/api/user/config",
        &serde_json::json!({
            "gitlabToken": "gitlab-token-secret-value",
            "gitlabHost": "https://gitlab.com"
        }),
        Some(&token),
    )
    .await;

    let config_resp = app.get("/api/user/config", Some(&token)).await;
    let body: serde_json::Value = config_resp.json().await.unwrap();
    let body_str = serde_json::to_string(&body).unwrap();
    assert!(!body_str.contains("gitlab-token-secret-value"));
}

#[tokio::test]
async fn test_profile_unauthenticated_returns_401() {
    let app = common::TestApp::new().await;
    let resp = app.get("/api/user/profile", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_config_unauthenticated_returns_401() {
    let app = common::TestApp::new().await;
    let resp = app.get("/api/user/config", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
