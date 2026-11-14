//! Gate approver lists (P2 design §4 B6).

mod sdlc_support;

use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};
use web_platform::auth::password::hash_password;
use web_platform::repository::{ProjectMemberRepository, UserRepository};

struct User {
    id: i64,
    token: String,
}

async fn member(app: &SdlcApp, name: &str, role: &str) -> User {
    let hash = hash_password("pw123456").unwrap();
    let u = app
        .repo
        .create_user(name, &hash, Some(name), "user")
        .await
        .unwrap();
    app.repo
        .add_member(app.project_id, u.id, role, None)
        .await
        .unwrap();
    let login: Value = app
        .client
        .post(app.url("/api/auth/login"))
        .json(&json!({"username": name, "password": "pw123456"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    User {
        id: u.id,
        token: login["data"]["token"].as_str().unwrap().to_string(),
    }
}

async fn call_as(
    app: &SdlcApp,
    user: &User,
    method: reqwest::Method,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let resp = app
        .client
        .request(method, app.url(path))
        .bearer_auth(&user.token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let st = StatusCode::from_u16(resp.status().as_u16()).unwrap();
    (st, resp.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn gate_approver_lists_restrict_decisions() {
    let app = SdlcApp::new().await;
    let alice = member(&app, "alice", "owner").await;
    let bob = member(&app, "bob", "member").await;
    let carol = member(&app, "carol", "member").await;
    let url = format!("/api/projects/{}/gate-approvers", app.project_id);

    let (st, v) = call_as(&app, &bob, reqwest::Method::GET, &url, json!(null)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"], json!({"G1": [], "G2": [], "G3": []}));

    let body = json!({"G1": [carol.id], "G2": [], "G3": []});
    let (st, _) = call_as(&app, &bob, reqwest::Method::PUT, &url, body.clone()).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "only owners edit the lists");
    let (st, v) = call_as(
        &app,
        &alice,
        reqwest::Method::PUT,
        &url,
        json!({"G1": [9999]}),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    let (st, v) = call_as(&app, &alice, reqwest::Method::PUT, &url, body).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["data"]["G1"][0]["userId"], carol.id, "{v}");
    assert_eq!(v["data"]["G1"][0]["username"], "carol");
    assert_eq!(v["data"]["G2"], json!([]));

    let (st, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "Gate", "initialContent": INTENT_OK}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    let base = format!("{}/{iid}", app.base());
    let (st, v) = app
        .post(&format!("{base}/docs/intent/submit"), json!({}))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let (_, v) = app.get(&base).await;
    assert_eq!(
        v["data"]["gateApprovers"]["G1"][0]["username"], "carol",
        "{v}"
    );

    let decision = format!("{base}/docs/intent/decision");
    for body in [
        json!({"action": "approve"}),
        json!({"action": "request_changes", "comment": "more"}),
        json!({"action": "reject", "comment": "no"}),
    ] {
        let (st, v) = call_as(&app, &bob, reqwest::Method::POST, &decision, body).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
        assert!(
            v["retMsg"].as_str().unwrap().contains("仅 carol 可审批 G1"),
            "{v}"
        );
    }
    assert_eq!(app.wait_flow(iid, |_| true).await["state"], "intent_review");

    let (st, v) = call_as(
        &app,
        &carol,
        reqwest::Method::POST,
        &decision,
        json!({"action": "approve"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn admins_and_unconfigured_gates_are_not_restricted() {
    let app = SdlcApp::new().await;
    let alice = member(&app, "alice", "owner").await;
    let bob = member(&app, "bob", "member").await;
    let url = format!("/api/projects/{}/gate-approvers", app.project_id);

    // G2 restricted, G1 open: bob may approve the intent.
    let (st, _) = call_as(
        &app,
        &alice,
        reqwest::Method::PUT,
        &url,
        json!({"G2": [alice.id]}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, v) = app
        .post(
            &app.base(),
            json!({"track": "full", "title": "Open", "initialContent": INTENT_OK}),
        )
        .await;
    let iid = v["data"]["flow"]["issueIid"].as_i64().unwrap();
    app.post(
        &format!("{}/{iid}/docs/intent/submit", app.base()),
        json!({}),
    )
    .await;
    let (st, v) = call_as(
        &app,
        &bob,
        reqwest::Method::POST,
        &format!("{}/{iid}/docs/intent/decision", app.base()),
        json!({"action": "approve"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");

    // Admin (not on the G2 list) can still decide.
    app.wait_flow(iid, |f| SdlcApp::doc_status(f, "spec") == "in_review")
        .await;
    let (st, v) = app
        .post(
            &format!("{}/{iid}/docs/spec/decision", app.base()),
            json!({"action": "request_changes", "comment": "admin note"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
}
