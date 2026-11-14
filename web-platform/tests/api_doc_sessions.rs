//! Drafting session quota, queueing and token usage (P2 design §5 A6).

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};

fn doc<'a>(flow: &'a Value, d: &str) -> &'a Value {
    flow["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["docType"] == d)
        .unwrap_or_else(|| panic!("{d} missing: {flow}"))
}

async fn start_plan(app: &SdlcApp, title: &str) -> i64 {
    let iid = app.gitlab.add_issue(title, &[]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    // Plan-only flows are not drafted automatically; ask for an AI draft.
    let (st, v) = app
        .post(
            &format!("{}/{iid}/docs/plan/draft/retry", app.base()),
            json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    iid
}

#[tokio::test]
async fn draft_token_usage_is_recorded_per_document() {
    let app = SdlcApp::new().await;
    let iid = start_plan(&app, "Tokens").await;
    let flow = app
        .wait_flow(iid, |f| SdlcApp::doc_status(f, "plan") == "in_review")
        .await;
    let u = &flow["tokenUsage"]["plan"];
    let total = u["total"].as_i64().unwrap_or(0);
    assert!(total >= 120 && total % 120 == 0, "{flow}");
    let turns = total / 120;
    assert_eq!(u["input"], 100 * turns, "{u}");
    assert_eq!(u["cachedInput"], 10 * turns);
    assert_eq!(u["output"], 20 * turns);
    assert_eq!(u["reasoning"], 5 * turns);
    assert!(flow["tokenUsage"]["intent"].is_null());
}

#[tokio::test]
async fn drafts_queue_when_sessions_are_full() {
    let app = SdlcApp::with(Opts {
        max_sessions: 1,
        ..Default::default()
    })
    .await;
    let hold = app.dir.path().join("hold");
    std::fs::write(&hold, "").unwrap();

    let a = start_plan(&app, "First").await;
    app.wait_flow(a, |f| doc(f, "plan")["draftState"] == "running")
        .await;
    let b = start_plan(&app, "Second").await;
    let flow = app
        .wait_flow(b, |f| doc(f, "plan")["draftState"] == "queued")
        .await;
    assert_eq!(flow["queuedStages"], json!(["plan"]), "{flow}");

    let (st, v) = app.get("/api/admin/concurrency").await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(
        v["data"]["doc_sessions"],
        json!({"active": 1, "max": 1, "queued": 1}),
        "{v}"
    );

    let (_, k) = app
        .get(&format!("/api/projects/{}/kanban/stages", app.project_id))
        .await;
    let card = k["data"]["columns"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| c["issues"].as_array().unwrap().iter())
        .find(|i| i["iid"] == b)
        .cloned()
        .unwrap();
    assert_eq!(
        card["flow"]["documents"][0]["draftState"], "queued",
        "{card}"
    );

    std::fs::remove_file(&hold).unwrap();
    for iid in [a, b] {
        app.wait_flow(iid, |f| SdlcApp::doc_status(f, "plan") == "in_review")
            .await;
    }
    let (_, v) = app.get("/api/admin/concurrency").await;
    assert_eq!(v["data"]["doc_sessions"]["queued"], 0, "{v}");
}
