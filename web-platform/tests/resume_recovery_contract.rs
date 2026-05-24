//! Contract tests for the resume/recovery Web lifecycle design.
//!
//! These tests are written before the implementation so the missing DB/API
//! contract is visible as RED test output.

mod common;

use std::collections::HashSet;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use reqwest::StatusCode;
use tempfile::TempDir;
use web_platform::db::init_pool;

#[test]
fn project_service_lifecycle_schema_has_generation_owner_and_fencing_columns() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let pool = init_pool(db_path.to_str().unwrap());
    let conn = pool.get().unwrap();

    let mut stmt = conn.prepare("PRAGMA table_info(projects)").unwrap();
    let columns: HashSet<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();

    let required = [
        "web_instance_id",
        "lifecycle_op_id",
        "lifecycle_lease_expires_at",
        "service_owner_web_instance_id",
        "service_owner_lease_expires_at",
        "service_owner_heartbeat_at",
        "service_generation",
        "service_instance_id",
        "service_pgid",
        "service_session_id",
        "service_cmdline_hash",
        "service_workdir",
        "last_lifecycle_op",
    ];

    for column in required {
        assert!(
            columns.contains(column),
            "projects table must include lifecycle fencing column `{column}`"
        );
    }
}

#[tokio::test]
async fn project_diagnostics_endpoint_exposes_issue_and_service_recovery_state() {
    let app = common::TestApp::new().await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/resume-recovery-diagnostics",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);
    {
        let conn = rusqlite::Connection::open(&app.db_path).unwrap();
        conn.execute(
            "UPDATE projects
             SET web_instance_id = 'web-1',
                 lifecycle_op_id = 'op-1',
                 service_owner_web_instance_id = 'owner-1',
                 service_generation = 7,
                 service_instance_id = 'svc-1',
                 service_pgid = 4242,
                 service_session_id = 4343,
                 service_cmdline_hash = 'cmdhash',
                 service_workdir = '/tmp/workdir',
                 last_lifecycle_op = 'start'
             WHERE id = ?1",
            [project_id],
        )
        .unwrap();
    }

    let resp = app
        .get(
            &format!("/api/projects/{project_id}/diagnostics"),
            Some(&app.admin_token),
        )
        .await;

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "diagnostics API must be queryable; scattered logs are not a sufficient recovery surface"
    );

    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["data"]["issues"].is_array(),
        "diagnostics payload must expose per-issue recovery diagnostics"
    );
    assert!(
        body["data"]["services"].is_array(),
        "diagnostics payload must expose per-service lifecycle diagnostics"
    );
    assert_eq!(body["data"]["services"][0]["service_instance_id"], "svc-1");
    assert_eq!(body["data"]["services"][0]["service_generation"], 7);
    assert_eq!(body["data"]["services"][0]["web_instance_id"], "web-1");
    assert_eq!(body["data"]["services"][0]["pgid"], 4242);
    assert_eq!(body["data"]["services"][0]["session_id"], 4343);
    assert_eq!(body["data"]["services"][0]["cmdline_hash"], "cmdhash");
    assert_eq!(body["data"]["services"][0]["workdir"], "/tmp/workdir");
}

#[tokio::test]
async fn start_service_persists_lifecycle_identity_and_exports_it_to_child() {
    let bin_dir = TempDir::new().unwrap();
    let env_out = bin_dir.path().join("service-id.txt");
    let script_path = bin_dir.path().join("fake-symphony.sh");
    {
        let mut script = std::fs::File::create(&script_path).unwrap();
        writeln!(
            script,
            "#!/bin/sh\nprintf '%s' \"$SYMPHONY_SERVICE_INSTANCE_ID\" > '{}'\nsleep 5\n",
            env_out.display()
        )
        .unwrap();
    }
    let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms).unwrap();

    let app = common::TestApp::new_with_symphony_bin(script_path.to_str().unwrap()).await;
    let create_body = app
        .create_test_project(
            "https://github.com/owner/resume-recovery-start",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&create_body);

    let resp = app
        .post(
            &format!("/api/projects/{project_id}/start"),
            &serde_json::json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let diagnostics = app
        .get(
            &format!("/api/projects/{project_id}/diagnostics"),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(diagnostics.status(), StatusCode::OK);
    let body: serde_json::Value = diagnostics.json().await.unwrap();
    let service = &body["data"]["services"][0];
    let service_instance_id = service["service_instance_id"]
        .as_str()
        .expect("service_instance_id must be persisted");

    assert!(
        service_instance_id.starts_with("svc-"),
        "service_instance_id must be a web-generated service identity"
    );
    assert_eq!(service["service_generation"], 1);
    assert_eq!(service["last_lifecycle_op"], "start");
    assert!(service["pgid"].as_i64().is_some());
    assert!(service["session_id"].as_i64().is_some());
    assert!(service["cmdline_hash"]
        .as_str()
        .is_some_and(|s| !s.is_empty()));
    assert!(service["workdir"].as_str().is_some_and(|s| !s.is_empty()));

    for _ in 0..100 {
        if env_out.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        env_out.exists(),
        "fake symphony child must write exported service identity"
    );
    let exported_id = std::fs::read_to_string(&env_out).unwrap();
    assert_eq!(
        exported_id, service_instance_id,
        "child process must receive the same service identity persisted in diagnostics"
    );

    let _ = app
        .post(
            &format!("/api/projects/{project_id}/stop"),
            &serde_json::json!({}),
            Some(&app.admin_token),
        )
        .await;
}
