//! track:fast whitelist and track upgrades (P2 design §5 A1).

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::json;

fn labels(app: &SdlcApp, iid: i64) -> Vec<String> {
    app.gitlab.issue(iid as u64).labels
}

#[tokio::test]
async fn fast_track_requires_a_whitelisted_label() {
    let app = SdlcApp::new().await;
    let plain = app.gitlab.add_issue("Plain", &["bug"]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "fast", "issueIid": plain}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["retMsg"].as_str().unwrap().contains("hotfix"), "{v}");

    let hot = app.gitlab.add_issue("Hot", &["hotfix", "doc:plan"]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "fast", "issueIid": hot}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let l = labels(&app, hot);
    assert!(l.contains(&"track:fast".to_string()), "{l:?}");
    assert!(l.contains(&"Todo".to_string()), "{l:?}");
    assert!(!l.iter().any(|x| x.starts_with("doc:")), "{l:?}");
    // No document flow is created for fast issues.
    let (st, _) = app.get(&format!("{}/{hot}", app.base())).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn fast_issue_upgrades_to_plan_until_claimed() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Hot", &["hotfix"]) as i64;
    app.post(&app.base(), json!({"track": "fast", "issueIid": iid}))
        .await;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let l = labels(&app, iid);
    assert!(l.contains(&"track:plan".to_string()), "{l:?}");
    assert!(!l.contains(&"track:fast".to_string()), "{l:?}");
    assert!(!l.contains(&"Todo".to_string()), "{l:?}");

    // Once the orchestrator claimed an issue its track can no longer change.
    let claimed = app.gitlab.add_issue(
        "Claimed",
        &["hotfix", "track:fast", "Todo", "symphony-claimed"],
    ) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": claimed}))
        .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(labels(&app, claimed).contains(&"symphony-claimed".to_string()));
}

#[tokio::test]
async fn plan_flow_upgrades_to_full_and_never_downgrades() {
    let app = SdlcApp::new().await;
    let iid = app.gitlab.add_issue("Up", &[]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "plan", "issueIid": iid}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let base = format!("{}/{iid}", app.base());

    let (st, v) = app
        .post(&format!("{base}/upgrade"), json!({"to": "full"}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["flow"]["track"], "full", "{v}");
    assert_eq!(v["data"]["state"], "intent", "{v}");
    let docs: Vec<&str> = v["data"]["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["docType"].as_str().unwrap())
        .collect();
    assert!(docs.contains(&"intent"), "{docs:?}");
    let intent = read_branch_file(
        &app.bare,
        &format!("symphony/docs-{iid}"),
        &format!("docs/changes/{iid}/intent.md"),
    );
    assert!(intent.is_some(), "intent.md committed on the docs branch");
    let l = app.wait_label(iid, "track:full").await;
    assert!(!l.contains(&"track:plan".to_string()), "{l:?}");
    assert!(l.contains(&"doc:intent".to_string()), "{l:?}");

    let (st, v) = app
        .post(&format!("{base}/upgrade"), json!({"to": "plan"}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
}

#[tokio::test]
async fn fast_track_labels_come_from_project_settings() {
    let app = SdlcApp::new().await;
    let (st, v) = app
        .put(
            &format!("/api/projects/{}", app.project_id),
            json!({"fast_track_labels": "typo, chore"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["fastTrackLabels"], "typo, chore");

    let hot = app.gitlab.add_issue("Hot", &["hotfix"]) as i64;
    let (st, _) = app
        .post(&app.base(), json!({"track": "fast", "issueIid": hot}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "hotfix no longer whitelisted");
    let typo = app.gitlab.add_issue("Typo", &["Typo"]) as i64;
    let (st, v) = app
        .post(&app.base(), json!({"track": "fast", "issueIid": typo}))
        .await;
    assert_eq!(st, StatusCode::OK, "case-insensitive match: {v}");

    let (_, k) = app
        .get(&format!("/api/projects/{}/kanban/stages", app.project_id))
        .await;
    assert_eq!(k["data"]["fastTrackLabels"], json!(["typo", "chore"]));
}
