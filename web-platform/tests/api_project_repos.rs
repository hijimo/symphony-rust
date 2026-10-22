//! Multi-repo workspace: `project_repos` API, permission rules and lifecycle hooks.
//!
//! A local axum server stands in for the GitLab instance, so raw-source permission
//! checks run without real network access.

mod common;

use axum::{extract::Path, routing::get, Json, Router};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use web_platform::crypto;
use web_platform::repository::UserConfigRepository;

const PASS: &str = "Pass123456";

/// Fake GitLab `GET /api/v4/projects/:path` returning per-repo access levels.
async fn spawn_mock_gitlab() -> String {
    let app = Router::new().route(
        "/api/v4/projects/{path}",
        get(|Path(path): Path<String>| async move {
            let level = match path.as_str() {
                "team/sdk" => 20,
                "team/backend" | "team/frontend" | "team/docs" => 30,
                _ => {
                    return (
                        axum::http::StatusCode::NOT_FOUND,
                        Json(json!({"message": "404 Project Not Found"})),
                    )
                }
            };
            (
                axum::http::StatusCode::OK,
                Json(json!({
                    "path_with_namespace": path,
                    "permissions": {
                        "project_access": {"access_level": level},
                        "group_access": null
                    }
                })),
            )
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

struct Env {
    app: common::TestApp,
    host: String,
    alice: String,
    bob: String,
    bob_id: i64,
    frontend: i64,
    backend: i64,
    docs: i64,
}

async fn create_project(app: &common::TestApp, url: &str, token: &str) -> i64 {
    let body = app.create_test_project(url, token).await;
    assert_eq!(body["success"], true, "{body}");
    app.get_project_id(&body)
}

async fn give_gitlab_token(app: &common::TestApp, user_id: i64, host: &str) {
    let token = crypto::encrypt("user-gitlab-token", &[0x42u8; 32]).unwrap();
    app.repo
        .upsert_config(user_id, Some(&token), Some(host), None, None, None)
        .await
        .unwrap();
}

async fn add_member(app: &common::TestApp, project: i64, user_id: i64, role: &str, token: &str) {
    let resp = app
        .post(
            &format!("/api/projects/{project}/members"),
            &json!({"user_id": user_id, "role": role}),
            Some(token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

/// alice creates frontend/backend/docs (owner of all). bob is a member-less user.
async fn setup() -> Env {
    let app = common::TestApp::new().await;
    let host = spawn_mock_gitlab().await;
    app.create_test_user("alice", PASS, "user").await;
    app.create_test_user("bob", PASS, "user").await;
    let alice_id = app.get_user_id("alice").await;
    let bob_id = app.get_user_id("bob").await;
    let alice = app.login_get_token("alice", PASS).await;
    let bob = app.login_get_token("bob", PASS).await;
    give_gitlab_token(&app, alice_id, &host).await;

    let frontend = create_project(&app, &format!("{host}/team/frontend"), &alice).await;
    let backend = create_project(&app, &format!("{host}/team/backend"), &alice).await;
    let docs = create_project(&app, &format!("{host}/team/docs"), &alice).await;
    Env {
        app,
        host,
        alice,
        bob,
        bob_id,
        frontend,
        backend,
        docs,
    }
}

async fn put_repos(env: &Env, project: i64, repos: Value, token: &str) -> (StatusCode, Value) {
    let resp = env
        .app
        .put(
            &format!("/api/projects/{project}/repos"),
            &json!({ "repos": repos }),
            Some(token),
        )
        .await;
    let status = resp.status();
    (status, resp.json().await.unwrap())
}

async fn get_repos(env: &Env, project: i64, token: &str) -> Value {
    let resp = env
        .app
        .get(&format!("/api/projects/{project}/repos"), Some(token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    resp.json::<Value>().await.unwrap()["data"].clone()
}

fn audit_actions(env: &Env, project: i64) -> Vec<String> {
    let conn = rusqlite::Connection::open(&env.app.db_path).unwrap();
    let mut stmt = conn
        .prepare("SELECT action FROM audit_logs WHERE project_id = ?1 ORDER BY id")
        .unwrap();
    stmt.query_map([project], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn linked_repo_crud_happy_path() {
    let env = setup().await;
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{
            "alias": "backend",
            "linked_project_id": env.backend,
            "mode": "write",
            "merge_after": []
        }]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let rows = get_repos(&env, env.frontend, &env.alice).await;
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["alias"], "backend");
    assert_eq!(rows[0]["source"], "linked");
    assert_eq!(rows[0]["namespace"], "team");
    assert_eq!(rows[0]["repo_name"], "backend");
    assert_eq!(rows[0]["default_branch"], Value::Null);
    assert_eq!(rows[0]["inherited_default_branch"], "main");
    assert_eq!(rows[0]["mode"], "write");
    assert_eq!(rows[0]["status"], "active");
    assert!(rows[0]["write_granted_by"].as_i64().is_some());

    // Rendered WORKFLOW.md carries the repos block with the inherited branch.
    let resp = env
        .app
        .get(
            &format!("/api/projects/{}/workflow", env.frontend),
            Some(&env.alice),
        )
        .await;
    let wf: Value = resp.json().await.unwrap();
    let content = wf["data"]["content"].as_str().unwrap();
    assert!(content.contains(
        "  repos:\n    - alias: primary\n      slug: \"team/frontend\"\n      role: primary\n      default_branch: \"main\"\n      mode: write\n    - alias: \"backend\"\n      slug: \"team/backend\"\n      role: secondary\n      default_branch: \"main\"\n      mode: write\n      checkout: partial\n"
    ), "{content}");

    // Single-repo project renders no repos block.
    let resp = env
        .app
        .get(
            &format!("/api/projects/{}/workflow", env.docs),
            Some(&env.alice),
        )
        .await;
    let wf: Value = resp.json().await.unwrap();
    assert!(!wf["data"]["content"].as_str().unwrap().contains("repos:"));

    // Clearing the list deletes the row.
    let (status, _) = put_repos(&env, env.frontend, json!([]), &env.alice).await;
    assert_eq!(status, StatusCode::OK);
    assert!(get_repos(&env, env.frontend, &env.alice)
        .await
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(audit_actions(&env, env.frontend), vec!["create", "delete"]);
}

#[tokio::test]
async fn alias_validation() {
    let env = setup().await;
    for alias in ["primary", "Bad_Alias", "", "a/b", "tmp"] {
        let (status, body) = put_repos(
            &env,
            env.frontend,
            json!([{"alias": alias, "linked_project_id": env.backend}]),
            &env.alice,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{alias}: {body}");
    }
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([
            {"alias": "x", "linked_project_id": env.backend},
            {"alias": "x", "linked_project_id": env.docs}
        ]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Exactly one source.
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "x", "linked_project_id": env.backend, "git_url": format!("{}/team/sdk", env.host)}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = put_repos(&env, env.frontend, json!([{"alias": "x"}]), &env.alice).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // merge_after cycle.
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([
            {"alias": "a", "linked_project_id": env.backend, "merge_after": ["b"]},
            {"alias": "b", "linked_project_id": env.docs, "merge_after": ["a"]}
        ]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn non_owner_cannot_edit_and_member_can_read() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "member", &env.alice).await;
    let (status, _) = put_repos(&env, env.frontend, json!([]), &env.bob).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    get_repos(&env, env.frontend, &env.bob).await;
}

#[tokio::test]
async fn linked_source_requires_owner_of_both_projects() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Admin may act on behalf (audited with via_admin).
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.app.admin_token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let conn = rusqlite::Connection::open(&env.app.db_path).unwrap();
    let after: String = conn
        .query_row(
            "SELECT after_json FROM audit_logs WHERE project_id = ?1",
            [env.frontend],
            |r| r.get(0),
        )
        .unwrap();
    assert!(after.contains("\"via_admin\":true"));
}

#[tokio::test]
async fn write_mode_only_by_created_by() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    add_member(&env.app, env.backend, env.bob_id, "owner", &env.alice).await;

    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "mode": "write"}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body["retMsg"].as_str().unwrap().contains("mode: write"));

    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Existing write grant by alice is kept when bob saves unchanged coordinates.
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "mode": "write"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "mode": "write", "checkout": "full"}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = get_repos(&env, env.frontend, &env.bob).await;
    assert_eq!(rows[0]["mode"], "write");
    assert_eq!(rows[0]["checkout"], "full");
}

#[tokio::test]
async fn coordinate_change_by_non_created_by_downgrades_to_read() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    add_member(&env.app, env.docs, env.bob_id, "owner", &env.alice).await;

    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "mode": "write"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.docs, "mode": "write"}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = get_repos(&env, env.frontend, &env.bob).await;
    assert_eq!(rows[0]["linked_project_id"], env.docs);
    assert_eq!(rows[0]["mode"], "read");
    assert_eq!(rows[0]["write_granted_by"], Value::Null);
    assert_eq!(
        audit_actions(&env, env.frontend),
        vec!["create", "update", "downgrade_write"]
    );
}

#[tokio::test]
async fn duplicate_sources_are_rejected() {
    let env = setup().await;
    // Linked backend + raw URL of the same repository (with userinfo).
    let raw_url = env.host.replacen("http://", "http://oauth2:glpat-x@", 1) + "/TEAM/Backend.git";
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([
            {"alias": "backend", "linked_project_id": env.backend},
            {"alias": "backend-raw", "git_url": raw_url, "default_branch": "main"}
        ]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["retMsg"].as_str().unwrap().contains("同一个仓库"));

    // Main project itself.
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "me", "git_url": format!("{}/team/frontend", env.host), "default_branch": "main"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "me", "linked_project_id": env.frontend}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn raw_source_checks_host_token_and_access_level() {
    let env = setup().await;
    // OK: Reporter on team/sdk.
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "sdk", "git_url": format!("{}/team/sdk.git", env.host), "default_branch": "master"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = get_repos(&env, env.frontend, &env.alice).await;
    assert_eq!(rows[0]["source"], "raw");
    assert_eq!(rows[0]["namespace"], "team");
    assert_eq!(rows[0]["repo_name"], "sdk");

    // No access (404 from GitLab).
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "secret", "git_url": format!("{}/team/secret", env.host), "default_branch": "main"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Different host.
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "other", "git_url": "https://gitlab.com/team/sdk", "default_branch": "main"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Raw source requires default_branch.
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "sdk", "git_url": format!("{}/team/sdk", env.host)}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Saver without own GitLab token.
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "sdk2", "git_url": format!("{}/team/docs", env.host), "default_branch": "main"}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["retMsg"].as_str().unwrap().contains("Token"));
}

#[tokio::test]
async fn candidates_are_owner_only() {
    let env = setup().await;
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp = env
        .app
        .get(
            &format!("/api/projects/{}/repos/candidates", env.frontend),
            Some(&env.alice),
        )
        .await;
    let body: Value = resp.json().await.unwrap();
    let list = body["data"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], env.docs);
    assert_eq!(list[0].as_object().unwrap().len(), 2);

    // bob owns frontend only (plus member of docs): no candidates.
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    add_member(&env.app, env.docs, env.bob_id, "member", &env.alice).await;
    let resp = env
        .app
        .get(
            &format!("/api/projects/{}/repos/candidates", env.frontend),
            Some(&env.bob),
        )
        .await;
    let body: Value = resp.json().await.unwrap();
    assert!(body["data"].as_array().unwrap().is_empty());

    // Admin is not an owner of alice's projects: owner-only applies to admins too.
    let resp = env
        .app
        .get(
            &format!("/api/projects/{}/repos/candidates", env.frontend),
            Some(&env.app.admin_token),
        )
        .await;
    let body: Value = resp.json().await.unwrap();
    assert!(body["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn deleting_referenced_project_conflicts() {
    let env = setup().await;
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp = env
        .app
        .delete(&format!("/api/projects/{}", env.backend), Some(&env.alice))
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: Value = resp.json().await.unwrap();
    assert!(body["retMsg"]
        .as_str()
        .unwrap()
        .contains("被 frontend 关联为 backend"));

    // Deleting the referencing project cascades its rows; then backend can go.
    let resp = env
        .app
        .delete(&format!("/api/projects/{}", env.frontend), Some(&env.alice))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = env
        .app
        .delete(&format!("/api/projects/{}", env.backend), Some(&env.alice))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn member_removal_and_downgrade_invalidate_rows_and_block_start() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    add_member(&env.app, env.backend, env.bob_id, "owner", &env.alice).await;
    add_member(&env.app, env.docs, env.bob_id, "owner", &env.alice).await;

    // bob saves a link frontend -> backend; alice saves docs -> backend.
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = put_repos(
        &env,
        env.docs,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Downgrading bob in backend invalidates rows linking to backend that bob saved.
    let resp = env
        .app
        .put(
            &format!("/api/projects/{}/members/{}", env.backend, env.bob_id),
            &json!({"role": "member"}),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let docs_rows = get_repos(&env, env.docs, &env.alice).await;
    assert_eq!(docs_rows[0]["status"], "invalid");
    assert!(docs_rows[0]["status_reason"]
        .as_str()
        .unwrap()
        .contains("owner"));
    let frontend_rows = get_repos(&env, env.frontend, &env.alice).await;
    assert_eq!(frontend_rows[0]["status"], "invalid");

    // Starting a project with an invalid related repo is refused.
    let resp = env
        .app
        .post(
            &format!("/api/projects/{}/start", env.docs),
            &json!({}),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["retMsg"].as_str().unwrap().contains("已失效"),
        "{body}"
    );

    // Re-saving the row (alice, owner of both) reactivates it.
    let (status, _) = put_repos(
        &env,
        env.docs,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        get_repos(&env, env.docs, &env.alice).await[0]["status"],
        "active"
    );

    // Removing bob from frontend invalidates nothing more (already invalid) but works.
    let resp = env
        .app
        .delete(
            &format!("/api/projects/{}/members/{}", env.frontend, env.bob_id),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(audit_actions(&env, env.docs).last().unwrap(), "update");
}

#[tokio::test]
async fn member_removal_invalidates_rows_in_that_project() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    add_member(&env.app, env.backend, env.bob_id, "owner", &env.alice).await;
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.bob,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let resp = env
        .app
        .delete(
            &format!("/api/projects/{}/members/{}", env.frontend, env.bob_id),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = get_repos(&env, env.frontend, &env.alice).await;
    assert_eq!(rows[0]["status"], "invalid");
    assert!(rows[0]["status_reason"].as_str().unwrap().contains("移出"));
}

#[tokio::test]
async fn start_with_valid_related_repo_renders_workflow() {
    let env = setup().await;
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "default_branch": "pre"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp = env
        .app
        .post(
            &format!("/api/projects/{}/start", env.frontend),
            &json!({}),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let workflow_path = std::path::Path::new(&env.app.db_path)
        .parent()
        .unwrap()
        .join(env.frontend.to_string())
        .join("WORKFLOW.md");
    let content = std::fs::read_to_string(workflow_path).unwrap();
    assert!(content.contains("    - alias: \"backend\"\n      slug: \"team/backend\"\n      role: secondary\n      default_branch: \"pre\"\n"));
    let _ = env
        .app
        .post(
            &format!("/api/projects/{}/stop", env.frontend),
            &json!({}),
            Some(&env.alice),
        )
        .await;
}

#[tokio::test]
async fn spawn_downgrades_write_row_not_granted_by_created_by() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "mode": "write"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Simulate a grant from someone other than the current created_by.
    let conn = rusqlite::Connection::open(&env.app.db_path).unwrap();
    conn.execute(
        "UPDATE project_repos SET write_granted_by = ?1 WHERE project_id = ?2",
        [env.bob_id, env.frontend],
    )
    .unwrap();

    let resp = env
        .app
        .post(
            &format!("/api/projects/{}/start", env.frontend),
            &json!({}),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let rows = get_repos(&env, env.frontend, &env.alice).await;
    assert_eq!(rows[0]["mode"], "read");
    assert_eq!(rows[0]["status"], "active");
    assert!(audit_actions(&env, env.frontend).contains(&"downgrade_write".to_string()));
    let _ = env
        .app
        .post(
            &format!("/api/projects/{}/stop", env.frontend),
            &json!({}),
            Some(&env.alice),
        )
        .await;
}

#[tokio::test]
async fn spawned_worker_env_is_whitelisted() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = tempfile::TempDir::new().unwrap();
    let env_out = bin_dir.path().join("env.txt");
    let script_path = bin_dir.path().join("fake-symphony.sh");
    {
        let mut script = std::fs::File::create(&script_path).unwrap();
        writeln!(
            script,
            "#!/bin/sh\n/usr/bin/env > '{}.tmp' && mv '{}.tmp' '{}'\nsleep 5\n",
            env_out.display(),
            env_out.display(),
            env_out.display()
        )
        .unwrap();
    }
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();

    // Parent env of the test process: one secret and one allow-listed extra.
    std::env::set_var("SYMPHONY_TEST_SECRET_TOKEN", "leak");
    std::env::set_var("KIRO_GATEWAY_API_KEY", "kiro-key");

    let app = common::TestApp::new_with_symphony_bin(script_path.to_str().unwrap()).await;
    let body = app
        .create_test_project("https://github.com/owner/env-whitelist", &app.admin_token)
        .await;
    let project_id = app.get_project_id(&body);
    let resp = app
        .post(
            &format!("/api/projects/{project_id}/start"),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    for _ in 0..200 {
        if env_out.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let dump = std::fs::read_to_string(&env_out).unwrap();
    let keys: Vec<&str> = dump.lines().filter_map(|l| l.split('=').next()).collect();
    assert!(keys.contains(&"PATH"));
    assert!(keys.contains(&"GITHUB_TOKEN"));
    assert!(keys.contains(&"SYMPHONY_SERVICE_INSTANCE_ID"));
    assert!(keys.contains(&"RUST_LOG"));
    assert!(dump.contains("KIRO_GATEWAY_API_KEY=kiro-key"));
    assert!(!keys.contains(&"SYMPHONY_TEST_SECRET_TOKEN"));
    assert!(!keys.contains(&"CARGO_PKG_NAME"));

    let _ = app
        .post(
            &format!("/api/projects/{project_id}/stop"),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
}

// ---------------------------------------------------------------------------
// Regressions found by verification round 1 (web-platform API & security).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_row_is_revalidated_on_identical_resave() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    add_member(&env.app, env.backend, env.bob_id, "owner", &env.alice).await;
    let row = json!([{"alias": "backend", "linked_project_id": env.backend}]);
    assert_eq!(
        put_repos(&env, env.frontend, row.clone(), &env.bob).await.0,
        StatusCode::OK
    );
    // bob loses ownership of backend -> row invalid.
    let resp = env
        .app
        .put(
            &format!("/api/projects/{}/members/{}", env.backend, env.bob_id),
            &json!({"role": "member"}),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        get_repos(&env, env.frontend, &env.alice).await[0]["status"],
        "invalid"
    );
    // Identical re-save by bob must NOT silently reactivate it.
    let (status, _) = put_repos(&env, env.frontend, row, &env.bob).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        get_repos(&env, env.frontend, &env.alice).await[0]["status"],
        "invalid"
    );
}

#[tokio::test]
async fn editing_non_source_fields_keeps_original_saver() {
    let env = setup().await;
    add_member(&env.app, env.frontend, env.bob_id, "owner", &env.alice).await;
    // alice saves a raw row (she has a token with Reporter on team/sdk).
    let raw = json!([{"alias": "sdk", "git_url": format!("{}/team/sdk.git", env.host), "default_branch": "main"}]);
    assert_eq!(
        put_repos(&env, env.frontend, raw, &env.alice).await.0,
        StatusCode::OK
    );
    // bob (no token) only changes checkout: allowed, but saver stays alice.
    let edited = json!([{"alias": "sdk", "git_url": format!("{}/team/sdk.git", env.host), "default_branch": "main", "checkout": "full"}]);
    assert_eq!(
        put_repos(&env, env.frontend, edited, &env.bob).await.0,
        StatusCode::OK
    );
    let conn = rusqlite::Connection::open(&env.app.db_path).unwrap();
    let saver: i64 = conn
        .query_row(
            "SELECT created_by FROM project_repos WHERE alias = 'sdk'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(saver, env.app.get_user_id("alice").await);
}

#[tokio::test]
async fn web_validation_matches_rust_parser_rules() {
    let env = setup().await;
    for alias in ["123", "-1", "quarantine", ".symphony"] {
        let (status, body) = put_repos(
            &env,
            env.frontend,
            json!([{"alias": alias, "linked_project_id": env.backend}]),
            &env.alice,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "alias {alias}: {body}");
    }
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "default_branch": "feat branch"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "x", "namespace": "team/..", "repo_name": "sdk", "default_branch": "main"}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Duplicate deps are not a cycle.
    let (status, body) = put_repos(
        &env,
        env.frontend,
        json!([
            {"alias": "backend", "linked_project_id": env.backend},
            {"alias": "docs", "linked_project_id": env.docs, "merge_after": ["backend", "backend"]}
        ]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn after_create_clone_into_root_conflicts_with_repos() {
    let env = setup().await;
    // Hook first, then repos -> rejected.
    let resp = env
        .app
        .put(
            &format!("/api/projects/{}", env.frontend),
            &json!({"hooks_after_create": "git clone http://h/a/b.git ."}),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Repos first, then hook -> rejected.
    assert_eq!(
        put_repos(
            &env,
            env.docs,
            json!([{"alias": "backend", "linked_project_id": env.backend}]),
            &env.alice
        )
        .await
        .0,
        StatusCode::OK
    );
    let resp = env
        .app
        .put(
            &format!("/api/projects/{}", env.docs),
            &json!({"hooks_after_create": "git clone http://h/a/b.git ."}),
            Some(&env.alice),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rendered_workflow_quotes_repo_fields() {
    let env = setup().await;
    let (status, _) = put_repos(
        &env,
        env.frontend,
        json!([{"alias": "backend", "linked_project_id": env.backend, "merge_after": ["primary"]}]),
        &env.alice,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let resp = env
        .app
        .get(
            &format!("/api/projects/{}/workflow", env.frontend),
            Some(&env.alice),
        )
        .await;
    let body: Value = resp.json().await.unwrap();
    let content = serde_json::to_string(&body["data"]).unwrap();
    // Every scalar a YAML parser could mistype is quoted (alias/slug/branch/deps).
    assert!(content.contains(r#"- alias: \"backend\""#), "{content}");
    assert!(content.contains(r#"slug: \"team/backend\""#), "{content}");
    assert!(
        content.contains(r#"merge_after: [\"primary\"]"#),
        "{content}"
    );
}
