mod common;

use chrono::Utc;
use reqwest::StatusCode;
use serde_json::json;
use web_platform::models::ServiceStatus;
use web_platform::process_manager::ProcessState;
use web_platform::repository::ProjectRepository;

#[tokio::test]
async fn test_global_concurrency_counts_running_child_processes() {
    let app = common::TestApp::new().await;
    let mut project_ids = Vec::new();

    for idx in 0..3 {
        let create_body = app
            .create_test_project(
                &format!("https://github.com/owner/concurrency-proj-{}", idx),
                &app.admin_token,
            )
            .await;
        let project_id = app.get_project_id(&create_body);
        project_ids.push(project_id);

        app.repo
            .update_service_status(
                project_id,
                &web_platform::models::ServiceStatusUpdate {
                    status: ServiceStatus::Running,
                    pid: Some(10_000 + idx),
                    error_message: None,
                },
            )
            .await
            .unwrap();

        app.process_manager.set_state(
            project_id,
            ProcessState {
                pid: (10_000 + idx) as u32,
                started_at: Utc::now(),
                status: ServiceStatus::Running,
                restart_count: 0,
            },
        );
    }

    let resp = app
        .get("/api/admin/concurrency", Some(&app.admin_token))
        .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["global_active"], json!(3));
    assert_eq!(body["data"]["global_max"], json!(10));
    assert_eq!(body["data"]["projects"].as_array().unwrap().len(), 3);

    app.process_manager.remove_state(project_ids[0]);

    let resp = app
        .get("/api/admin/concurrency", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["global_active"], json!(2));

    app.process_manager.set_state(
        project_ids[1],
        ProcessState {
            pid: 0,
            started_at: Utc::now(),
            status: ServiceStatus::Failed,
            restart_count: 0,
        },
    );

    let resp = app
        .get("/api/admin/concurrency", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["global_active"], json!(1));
}
