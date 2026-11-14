//! Docs MR merge waits for CI, explains rejections and reports the project
//! token state (P2 design §5 A3). Separate binary: it sets the CI poll
//! interval through the environment.

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};

fn file_path(iid: i64, doc: &str) -> String {
    format!("docs/changes/{iid}/{doc}.md")
}

async fn plan_in_review(app: &SdlcApp, title: &str) -> (i64, String) {
    std::env::set_var("SYMPHONY_DOCS_MERGE_CI_POLL_SECS", "1");
    let iid = app.gitlab.add_issue(title, &[]) as i64;
    app.post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    let base = format!("{}/{iid}", app.base());
    let (_, cur) = app
        .get(&format!(
            "{base}/file?path={}",
            urlencoding::encode(&file_path(iid, "plan"))
        ))
        .await;
    let cur = &cur["data"];
    let fm_end = cur["content"].as_str().unwrap().find("\n---\n").unwrap() + 5;
    let text = format!(
        "{}# Plan\n\n## Files\n- src/main.rs\n\n## Steps\n1\n\n## Tests\n- t\n\n## Risks\n- r\n\n## Alternatives considered\n- n\n",
        &cur["content"].as_str().unwrap()[..fm_end]
    );
    app.put(
        &format!("{base}/file"),
        json!({"path": file_path(iid, "plan"), "content": text, "baseVersion": cur["version"], "commit": true}),
    )
    .await;
    let (st, v) = app
        .post(&format!("{base}/docs/plan/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    (iid, base)
}

async fn approve(app: &SdlcApp, base: &str) -> Value {
    let (st, v) = app
        .post(
            &format!("{base}/docs/plan/decision"),
            json!({"action": "approve"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"].clone()
}

#[tokio::test]
async fn merge_waits_for_running_pipeline_then_merges() {
    let app = SdlcApp::new().await;
    let (iid, base) = plan_in_review(&app, "CI wait").await;
    *app.gitlab.pipeline_status.lock().unwrap() = Some("running".into());

    let flow = approve(&app, &base).await;
    assert_eq!(flow["state"], "plan_approved_pending_merge", "{flow}");
    assert!(
        flow["flow"]["mergeWaitingSince"].is_string(),
        "waiting for CI: {flow}"
    );
    assert!(flow["flow"]["mergeError"].is_null());
    assert!(
        app.gitlab.merge_tokens.lock().unwrap().is_empty(),
        "not merged yet"
    );

    *app.gitlab.pipeline_status.lock().unwrap() = Some("success".into());
    let flow = app.wait_flow(iid, |f| f["state"] == "todo").await;
    assert!(flow["flow"]["mergeWaitingSince"].is_null(), "{flow}");
    assert!(flow["flow"]["mergeCommitSha"].is_string());
}

#[tokio::test]
async fn failed_pipeline_is_reported_without_merging() {
    let app = SdlcApp::new().await;
    let (_iid, base) = plan_in_review(&app, "CI failed").await;
    *app.gitlab.pipeline_status.lock().unwrap() = Some("failed".into());

    let flow = approve(&app, &base).await;
    assert_eq!(flow["state"], "plan_approved_pending_merge");
    let err = flow["flow"]["mergeError"].as_str().unwrap();
    assert!(err.contains("CI 未通过"), "{err}");
    assert!(err.contains("重试合并"), "{err}");
    assert!(app.gitlab.merge_tokens.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rejected_merge_explains_missing_approval() {
    let app = SdlcApp::new().await;
    let (_iid, base) = plan_in_review(&app, "Approval").await;
    *app.gitlab.fail_merge.lock().unwrap() = true;
    *app.gitlab.detailed_merge_status.lock().unwrap() = Some("not_approved".into());

    let flow = approve(&app, &base).await;
    let err = flow["flow"]["mergeError"].as_str().unwrap();
    assert!(err.contains("平台要求另一位成员审批"), "{err}");
    assert!(err.contains("重试合并"), "{err}");
}

#[tokio::test]
async fn token_status_reports_invalid_project_token() {
    let app = SdlcApp::new().await;
    let url = format!("/api/projects/{}/token-status", app.project_id);
    let (st, v) = app.get(&url).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["ok"], true, "{v}");

    *app.gitlab.reject_token.lock().unwrap() = true;
    let (_, v) = app.get(&format!("{url}?refresh=true")).await;
    assert_eq!(v["data"]["ok"], false, "{v}");
    assert!(
        v["data"]["reason"]
            .as_str()
            .unwrap()
            .contains("项目 Token 不可用"),
        "{v}"
    );
}
