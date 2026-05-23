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
use serde_json::json;

#[tokio::test]
async fn test_get_workflow_default() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/workflow-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .get(
            &format!("/api/projects/{}/workflow", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["template_mode"], "default");
    // Default template should contain rendered content
    assert!(!body["data"]["content"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn test_update_workflow_custom() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/custom-wf-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let custom_content = "# Custom Workflow\n\nDo things differently.";
    let resp = app
        .put(
            &format!("/api/projects/{}/workflow", project_id),
            &json!({
                "template_mode": "custom",
                "content": custom_content
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["template_mode"], "custom");
    assert_eq!(body["data"]["content"], custom_content);
}

#[tokio::test]
async fn test_reset_workflow() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/reset-wf-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    // First set to custom
    app.put(
        &format!("/api/projects/{}/workflow", project_id),
        &json!({
            "template_mode": "custom",
            "content": "# Custom"
        }),
        Some(&app.admin_token),
    )
    .await;

    // Reset back to default
    let resp = app
        .post(
            &format!("/api/projects/{}/workflow/reset", project_id),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["template_mode"], "default");
    // Content should be the rendered default template, not "# Custom"
    assert_ne!(body["data"]["content"].as_str().unwrap(), "# Custom");
    assert!(!body["data"]["content"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn test_workflow_non_member() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/wf-perm-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    // Create a user who is NOT a member
    app.create_test_user("wfoutsider", "Pass123456", "user")
        .await;
    let outsider_token = app.login_get_token("wfoutsider", "Pass123456").await;

    // Non-member tries to get workflow
    let resp = app
        .get(
            &format!("/api/projects/{}/workflow", project_id),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");

    // Non-member tries to update workflow
    let resp = app
        .put(
            &format!("/api/projects/{}/workflow", project_id),
            &json!({
                "template_mode": "custom",
                "content": "hacked"
            }),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Non-member tries to reset workflow
    let resp = app
        .post(
            &format!("/api/projects/{}/workflow/reset", project_id),
            &json!({}),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
