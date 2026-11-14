//! Document-flow metrics (P2 design §4 B5).

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};

async fn metrics(app: &SdlcApp) -> Value {
    let (st, v) = app
        .get(&format!(
            "/api/projects/{}/sdlc/metrics?days=30",
            app.project_id
        ))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"].clone()
}

async fn start_full(app: &SdlcApp, title: &str) -> i64 {
    let (st, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": title, "initialContent": INTENT_OK}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let (st, v) = app
        .post(
            &format!("{}/{iid}/docs/intent/submit", app.base()),
            json!({}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    iid
}

async fn decide(app: &SdlcApp, iid: i64, doc: &str, body: Value) {
    let (st, v) = app
        .post(&format!("{}/{iid}/docs/{doc}/decision", app.base()), body)
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

fn close(app: &SdlcApp, iid: i64, labels: &[&str]) {
    let mut m = app.gitlab.issues.lock().unwrap();
    let i = m.get_mut(&(iid as u64)).unwrap();
    i.state = "closed".into();
    i.labels.extend(labels.iter().map(|l| l.to_string()));
}

#[tokio::test]
async fn empty_project_has_no_rates() {
    let app = SdlcApp::new().await;
    let m = metrics(&app).await;
    assert_eq!(m["days"], 30);
    assert!(m["intentSurvival"]["rate"].is_null(), "{m}");
    assert_eq!(m["designCycle"]["count"], 0);
    assert!(m["designCycle"]["medianHours"].is_null());
    assert_eq!(m["rework"]["count"], 0);
    assert!(m["firstPassMerge"]["rate"].is_null());
    assert!(m["planConformance"]["rate"].is_null());
    assert_eq!(m["draftTokens"]["total"], 0);
}

#[tokio::test]
async fn metrics_follow_gates_rework_labels_and_tokens() {
    let app = SdlcApp::new().await;

    // Accepted intent → spec → plan, then the approved spec is edited.
    let a = start_full(&app, "Accepted").await;
    decide(&app, a, "intent", json!({"action": "approve"})).await;
    app.wait_flow(a, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    decide(&app, a, "spec", json!({"action": "approve"})).await;
    app.wait_flow(a, |f| SdlcApp::doc_status(f, "plan") == "in_review")
        .await;
    let base = format!("{}/{a}", app.base());
    let path = format!("docs/changes/{a}/spec.md");
    let (_, cur) = app
        .get(&format!("{base}/file?path={}", urlencode(&path)))
        .await;
    let text = format!(
        "{}\nExtra requirement.\n",
        cur["data"]["content"].as_str().unwrap()
    );
    let (st, v) = app
        .put(
            &format!("{base}/file"),
            json!({"path": path, "content": text, "baseVersion": cur["data"]["version"], "commit": true}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // Rejected intent.
    let r = start_full(&app, "Rejected").await;
    decide(
        &app,
        r,
        "intent",
        json!({"action": "reject", "comment": "out of scope"}),
    )
    .await;

    // Closed implementation issues with plan-only flows.
    let clean = app.gitlab.add_issue("Clean", &[]) as i64;
    let deviated = app.gitlab.add_issue("Deviated", &[]) as i64;
    for iid in [clean, deviated] {
        let (st, v) = app
            .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
            .await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }
    close(&app, clean, &[]);
    close(&app, deviated, &["plan-deviation"]);
    app.gitlab.label_events.lock().unwrap().push((
        deviated as u64,
        "Rework".into(),
        "2026-11-10T08:00:00Z".into(),
    ));

    let m = metrics(&app).await;
    assert_eq!(
        m["intentSurvival"],
        json!({"accepted": 1, "rejected": 1, "rate": 0.5}),
        "{m}"
    );
    assert_eq!(m["designCycle"]["count"], 1, "{m}");
    assert!(m["designCycle"]["medianHours"].as_f64().unwrap() >= 0.0);
    assert!(m["designCycle"]["p90Hours"].as_f64().unwrap() >= 0.0);
    assert_eq!(m["rework"], json!({"count": 1, "flows": 1}), "{m}");
    assert_eq!(m["firstPassMerge"]["closed"], 2, "{m}");
    assert_eq!(m["firstPassMerge"]["firstPass"], 1);
    assert_eq!(m["firstPassMerge"]["rate"], 0.5);
    assert_eq!(
        m["planConformance"],
        json!({"closed": 2, "conforming": 1, "rate": 0.5}),
        "{m}"
    );
    let tokens = m["draftTokens"]["total"].as_i64().unwrap();
    assert!(
        tokens >= 240 && tokens % 120 == 0,
        "spec + plan drafts: {m}"
    );
}

fn urlencode(s: &str) -> String {
    s.replace('/', "%2F")
}
