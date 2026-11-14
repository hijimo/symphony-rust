//! Stage board details (P2 design §5 A4): label/document mismatch, AI
//! running for implementation, Human Review waiting time and the pending
//! merge sub-state.

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};
use web_platform::services::runtime_state::{
    Instance, RunningIssue, RuntimeSnapshot, RuntimeTokens,
};

async fn board(app: &SdlcApp) -> Value {
    let (st, v) = app
        .get(&format!("/api/projects/{}/kanban/stages", app.project_id))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"].clone()
}

fn card(board: &Value, iid: i64) -> Value {
    board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["issues"].as_array().unwrap().iter())
        .find(|i| i["iid"] == iid)
        .cloned()
        .unwrap_or_else(|| panic!("card #{iid} missing: {board}"))
}

#[tokio::test]
async fn implementation_cards_show_ai_running_and_review_wait() {
    let app = SdlcApp::new().await;
    let running = app.gitlab.add_issue("Running", &["In Progress"]) as i64;
    let review = app.gitlab.add_issue("Review", &["Human Review"]) as i64;
    app.gitlab.label_events.lock().unwrap().extend([
        (
            review as u64,
            "In Progress".to_string(),
            "2026-11-13T07:00:00Z".to_string(),
        ),
        (
            review as u64,
            "Human Review".to_string(),
            "2026-11-13T08:00:00Z".to_string(),
        ),
    ]);
    app.runtime.record(
        app.project_id,
        Instance::Dev,
        RuntimeSnapshot {
            fetched_at: chrono::Utc::now(),
            stale: false,
            running: vec![RunningIssue {
                issue_id: running.to_string(),
                issue_identifier: format!("#{running}"),
                state: "In Progress".into(),
                turn_count: 2,
                started_at: "2026-11-13T09:00:00Z".into(),
                last_event_at: None,
                tokens: RuntimeTokens::default(),
            }],
            retrying: vec![],
        },
    );

    let b = board(&app).await;
    let r = card(&b, running);
    assert_eq!(r["aiRunning"]["since"], "2026-11-13T09:00:00Z", "{r}");
    assert_eq!(r["aiRunning"]["instance"], "dev");
    let h = card(&b, review);
    assert!(h["aiRunning"].is_null());
    assert_eq!(h["humanReviewSince"], "2026-11-13T08:00:00Z", "{h}");
    assert_eq!(h["placement"]["waitingHuman"], true);
}

#[tokio::test]
async fn document_cards_flag_label_mismatch() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Docs", &[]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let c = card(&board(&app).await, iid);
    assert_eq!(c["placement"]["anomaly"], false, "{c}");
    assert!(c["placement"]["anomalyReason"].is_null());

    // A person moves the issue to Todo by hand during the document phase.
    {
        let mut m = app.gitlab.issues.lock().unwrap();
        let i = m.get_mut(&(iid as u64)).unwrap();
        i.labels.retain(|l| !l.starts_with("doc:"));
        i.labels.push("Todo".into());
    }
    let c = card(&board(&app).await, iid);
    assert_eq!(c["placement"]["anomaly"], true, "{c}");
    assert!(
        c["placement"]["anomalyReason"]
            .as_str()
            .unwrap()
            .contains("按 track:fast 处理"),
        "{c}"
    );
}

#[tokio::test]
async fn overview_stage_board_merges_running_projects() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Global", &["In Progress"]) as i64;

    // Stopped projects are not part of the overview.
    let (st, v) = app.get("/api/overview/kanban/stages").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["projects"].as_array().unwrap().len(), 0, "{v}");

    rusqlite::Connection::open(app.dir.path().join("test.db"))
        .unwrap()
        .execute(
            "UPDATE projects SET service_status = 'running' WHERE id = ?1",
            [app.project_id],
        )
        .unwrap();
    let (st, v) = app.get("/api/overview/kanban/stages").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let d = &v["data"];
    assert_eq!(d["projects"][0]["projectId"], app.project_id, "{v}");
    assert!(d["projects"][0]["error"].is_null(), "{v}");
    let c = card(d, iid);
    assert_eq!(c["projectId"], app.project_id, "{c}");
    assert!(c["projectName"].is_string(), "{c}");
    assert_eq!(c["placement"]["column"], "build", "{c}");
}
