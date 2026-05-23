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
async fn test_list_members() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/members-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .get(
            &format!("/api/projects/{}/members", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    let members = body["data"].as_array().unwrap();
    // Creator is automatically added as owner
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["role"], "owner");
}

#[tokio::test]
async fn test_add_member_success() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/add-member-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    // Create a user to add
    app.create_test_user("newmember", "Pass123456", "user")
        .await;
    let new_user_id = app.get_user_id("newmember").await;

    let resp = app
        .post(
            &format!("/api/projects/{}/members", project_id),
            &json!({
                "user_id": new_user_id,
                "role": "member"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["user_id"], new_user_id);
    assert_eq!(body["data"]["role"], "member");
    assert_eq!(body["data"]["username"], "newmember");
}

#[tokio::test]
async fn test_add_member_duplicate() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/dup-member-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    app.create_test_user("dupmember", "Pass123456", "user")
        .await;
    let user_id = app.get_user_id("dupmember").await;

    // Add member first time
    app.post(
        &format!("/api/projects/{}/members", project_id),
        &json!({
            "user_id": user_id,
            "role": "member"
        }),
        Some(&app.admin_token),
    )
    .await;

    // Try to add again
    let resp = app
        .post(
            &format!("/api/projects/{}/members", project_id),
            &json!({
                "user_id": user_id,
                "role": "member"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_003");
}

#[tokio::test]
async fn test_add_member_non_owner() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/nonowner-add-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    // Create two users
    app.create_test_user("memberuser", "Pass123456", "user")
        .await;
    app.create_test_user("targetuser", "Pass123456", "user")
        .await;
    let member_user_id = app.get_user_id("memberuser").await;
    let target_user_id = app.get_user_id("targetuser").await;

    // Add memberuser as a member (not owner)
    app.post(
        &format!("/api/projects/{}/members", project_id),
        &json!({
            "user_id": member_user_id,
            "role": "member"
        }),
        Some(&app.admin_token),
    )
    .await;

    let member_token = app.login_get_token("memberuser", "Pass123456").await;

    // Member tries to add another user - should be forbidden
    let resp = app
        .post(
            &format!("/api/projects/{}/members", project_id),
            &json!({
                "user_id": target_user_id,
                "role": "member"
            }),
            Some(&member_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");
}

#[tokio::test]
async fn test_add_member_user_not_found() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/notfound-member-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .post(
            &format!("/api/projects/{}/members", project_id),
            &json!({
                "user_id": 99999,
                "role": "member"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_002");
}

#[tokio::test]
async fn test_update_member_role() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/role-update-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    app.create_test_user("roleuser", "Pass123456", "user").await;
    let user_id = app.get_user_id("roleuser").await;

    // Add as member
    app.post(
        &format!("/api/projects/{}/members", project_id),
        &json!({
            "user_id": user_id,
            "role": "member"
        }),
        Some(&app.admin_token),
    )
    .await;

    // Promote to owner
    let resp = app
        .put(
            &format!("/api/projects/{}/members/{}", project_id, user_id),
            &json!({ "role": "owner" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["role"], "owner");
}

#[tokio::test]
async fn test_remove_member_success() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/remove-member-proj",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    app.create_test_user("removeme", "Pass123456", "user").await;
    let user_id = app.get_user_id("removeme").await;

    // Add member
    app.post(
        &format!("/api/projects/{}/members", project_id),
        &json!({
            "user_id": user_id,
            "role": "member"
        }),
        Some(&app.admin_token),
    )
    .await;

    // Remove member
    let resp = app
        .delete(
            &format!("/api/projects/{}/members/{}", project_id, user_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);

    // Verify member is gone
    let list_resp = app
        .get(
            &format!("/api/projects/{}/members", project_id),
            Some(&app.admin_token),
        )
        .await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let members = list_body["data"].as_array().unwrap();
    assert_eq!(members.len(), 1); // Only the owner remains
}

#[tokio::test]
async fn test_remove_member_last_owner() {
    let app = common::TestApp::new().await;

    // Create project as a regular user (who becomes the sole owner)
    app.create_test_user("soleowner", "Pass123456", "user")
        .await;
    let owner_token = app.login_get_token("soleowner", "Pass123456").await;
    let create_body = app
        .create_test_project("https://github.com/owner/lastowner-proj", &owner_token)
        .await;
    let project_id = app.get_project_id(&create_body);
    let owner_user_id = app.get_user_id("soleowner").await;

    // Admin tries to remove the last owner
    let resp = app
        .delete(
            &format!("/api/projects/{}/members/{}", project_id, owner_user_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_003");
}

#[tokio::test]
async fn test_sync_members_no_auth() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project("https://github.com/owner/sync-proj", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .post(
            &format!("/api/projects/{}/members/sync", project_id),
            &json!({}),
            None,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_001");
}
