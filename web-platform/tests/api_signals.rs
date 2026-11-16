//! External signal ingestion (P3 design §3 S1): ingest keys, Bearer and
//! Sentry-signature auth, limits, fingerprint de-duplication, redaction.

mod sdlc_support;

use hmac::{Hmac, KeyInit, Mac};
use reqwest::StatusCode;
use sdlc_support::*;
use serde_json::{json, Value};
use sha2::Sha256;

fn keys_url(app: &SdlcApp) -> String {
    format!("/api/projects/{}/signal-keys", app.project_id)
}

fn ingest_url(app: &SdlcApp, project: i64, source: &str) -> String {
    app.url(&format!("/api/ingest/{project}/{source}"))
}

async fn new_key(app: &SdlcApp, source: &str) -> (i64, String) {
    let (st, v) = app
        .post(
            &keys_url(app),
            json!({"source": source, "name": format!("{source} key")}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let secret = v["data"]["secret"].as_str().unwrap().to_string();
    assert!(secret.starts_with("sig_"), "{v}");
    (v["data"]["id"].as_i64().unwrap(), secret)
}

async fn push(app: &SdlcApp, token: &str, body: &Value) -> (StatusCode, Value) {
    let r = app
        .client
        .post(ingest_url(app, app.project_id, "generic"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .unwrap();
    let st = StatusCode::from_u16(r.status().as_u16()).unwrap();
    (st, r.json().await.unwrap_or(Value::Null))
}

async fn signals(app: &SdlcApp) -> Vec<Value> {
    let (st, v) = app
        .get(&format!("/api/projects/{}/signals", app.project_id))
        .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    v["data"].as_array().unwrap().clone()
}

#[tokio::test]
async fn keys_are_shown_once_and_can_be_revoked() {
    let app = SdlcApp::new().await;
    let (id, secret) = new_key(&app, "generic").await;
    let (st, v) = app.get(&keys_url(&app)).await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let k = &v["data"][0];
    assert_eq!(k["id"], id);
    assert_eq!(k["source"], "generic");
    assert!(
        k.get("secret").is_none() && !v.to_string().contains(&secret),
        "{v}"
    );

    let (st, v) = app
        .post(&keys_url(&app), json!({"source": "pagerduty", "name": "x"}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");

    let body = json!({"title": "Disk full", "fingerprint": "disk-1"});
    assert_eq!(push(&app, &secret, &body).await.0, StatusCode::ACCEPTED);
    let (st, _) = app
        .call(
            reqwest::Method::DELETE,
            &format!("{}/{id}", keys_url(&app)),
            None,
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(push(&app, &secret, &body).await.0, StatusCode::UNAUTHORIZED);
    let (_, v) = app.get(&keys_url(&app)).await;
    assert!(v["data"][0]["revokedAt"].is_string(), "{v}");
}

#[tokio::test]
async fn bearer_auth_is_bound_to_project_and_source() {
    let app = SdlcApp::new().await;
    let (_, secret) = new_key(&app, "generic").await;
    let (_, am) = new_key(&app, "alertmanager").await;
    let body = json!({"title": "t", "fingerprint": "f"});

    assert_eq!(
        push(&app, "sig_wrong", &body).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        push(&app, &am, &body).await.0,
        StatusCode::UNAUTHORIZED,
        "other source"
    );
    let r = app
        .client
        .post(ingest_url(&app, app.project_id + 999, "generic"))
        .bearer_auth(&secret)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status().as_u16(),
        401,
        "unknown project looks like a bad key"
    );
    let r = app
        .client
        .post(ingest_url(&app, app.project_id, "generic"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 401, "no credentials");

    let (st, v) = push(&app, &secret, &json!({"fingerprint": "f"})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "title is required: {v}");
    let big = "x".repeat(300 * 1024);
    let (st, _) = push(
        &app,
        &secret,
        &json!({"title": "t", "fingerprint": "f", "message": big}),
    )
    .await;
    assert_eq!(st, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(signals(&app).await.is_empty());
}

#[tokio::test]
async fn same_fingerprint_is_deduplicated_and_content_redacted() {
    let app = SdlcApp::new().await;
    let (_, secret) = new_key(&app, "generic").await;
    let first = json!({
        "title": "Checkout <b>500</b>", "fingerprint": "checkout-500", "severity": "critical",
        "message": "token glpat-abcdefghijklmnop1234 leaked", "url": "https://grafana.local/d/1",
        "details": {"host": "web-1"}
    });
    let (st, v) = push(&app, &secret, &first).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    assert_eq!(v["data"], json!({"accepted": 1, "created": 1}));

    let again = json!({"title": "Checkout 500", "fingerprint": "checkout-500", "message": "second", "resolved": true});
    let (_, v) = push(&app, &secret, &again).await;
    assert_eq!(v["data"], json!({"accepted": 1, "created": 0}));
    push(
        &app,
        &secret,
        &json!({"title": "Other", "fingerprint": "other"}),
    )
    .await;

    let list = signals(&app).await;
    assert_eq!(list.len(), 2, "{list:?}");
    let s = list
        .iter()
        .find(|s| s["fingerprint"] == "checkout-500")
        .unwrap();
    assert_eq!(s["count"], 2);
    assert_eq!(s["status"], "new");
    assert_eq!(s["resolved"], true);
    assert_eq!(s["source"], "generic");
    assert_eq!(
        s["severity"], "critical",
        "kept from the first event when omitted"
    );
    assert_eq!(s["title"], "Checkout 500", "{s}");
    assert_eq!(s["evidence"]["message"], "second");
    let other = list.iter().find(|s| s["fingerprint"] == "other").unwrap();
    assert_eq!(other["severity"], "warning", "default severity");

    // The first event's message was redacted before it was stored.
    let db = rusqlite::Connection::open(app.dir.path().join("test.db")).unwrap();
    let stored: String = db
        .query_row(
            "SELECT group_concat(evidence_json || title, '') FROM signals",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!stored.contains("glpat-"), "{stored}");
}

#[tokio::test]
async fn first_event_redaction_and_html_stripping() {
    let app = SdlcApp::new().await;
    let (_, secret) = new_key(&app, "generic").await;
    push(
        &app,
        &secret,
        &json!({"title": "<script>x</script>Login fails", "fingerprint": "login",
                "message": "-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY----- then ghp_0123456789abcdefghij"}),
    )
    .await;
    let s = &signals(&app).await[0];
    assert_eq!(s["title"], "xLogin fails", "{s}");
    let msg = s["evidence"]["message"].as_str().unwrap();
    assert!(!msg.contains("MIIE") && !msg.contains("ghp_"), "{msg}");
    assert!(msg.contains("[已脱敏]"), "{msg}");
}

#[tokio::test]
async fn ingest_is_rate_limited_per_key() {
    let app = SdlcApp::new().await;
    let (_, secret) = new_key(&app, "generic").await;
    let body = json!({"title": "t", "fingerprint": "f"});
    for _ in 0..60 {
        assert_eq!(push(&app, &secret, &body).await.0, StatusCode::ACCEPTED);
    }
    assert_eq!(
        push(&app, &secret, &body).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let (_, other) = new_key(&app, "generic").await;
    assert_eq!(
        push(&app, &other, &body).await.0,
        StatusCode::ACCEPTED,
        "other key unaffected"
    );
}

#[tokio::test]
async fn sentry_requests_are_verified_by_signature() {
    let app = SdlcApp::new().await;
    let (_, secret) = new_key(&app, "sentry").await;
    let body = br#"{"action":"created","data":{"issue":{"id":"1","title":"x"}}}"#.to_vec();
    let sign = |key: &str| {
        let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
        mac.update(&body);
        hex::encode(mac.finalize().into_bytes())
    };
    let send = |sig: String| {
        app.client
            .post(ingest_url(&app, app.project_id, "sentry"))
            .header("Sentry-Hook-Resource", "issue")
            .header("Sentry-Hook-Signature", sig)
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
    };
    assert_eq!(send(sign("wrong")).await.unwrap().status().as_u16(), 401);
    // Authenticated; parsing Sentry payloads arrives with S2.
    let r = send(sign(&secret)).await.unwrap();
    assert_ne!(r.status().as_u16(), 401);
}
