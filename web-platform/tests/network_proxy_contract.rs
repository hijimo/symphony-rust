mod common;

use common::TestApp;
use serde_json::json;

fn insert_system_config(db_path: &std::path::Path, key: &str, value: &str) {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO system_configs (key, value, description, updated_at)
         VALUES (?1, ?2, 'test', datetime('now'))",
        rusqlite::params![key, value],
    )
    .unwrap();
}

fn insert_proxy_secret(db_path: &std::path::Path, key: &str, encrypted_value: &str) {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO secret_configs (key, encrypted_value, kind, updated_at)
         VALUES (?1, ?2, 'proxy_url', datetime('now'))",
        rusqlite::params![key, encrypted_value],
    )
    .unwrap();
}

fn insert_corrupt_proxy_secret(db_path: &std::path::Path, key: &str) {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO secret_configs (key, encrypted_value, kind, updated_at)
         VALUES (?1, 'not-valid-ciphertext', 'network_proxy_http', datetime('now'))",
        [key],
    )
    .unwrap();
}

fn mark_project_running_with_proxy_version(
    db_path: &std::path::Path,
    project_id: i64,
    proxy_version: Option<&str>,
) {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "UPDATE projects
         SET service_status = 'running',
             service_pid = 4242,
             service_proxy_config_version = ?1
         WHERE id = ?2",
        rusqlite::params![proxy_version, project_id],
    )
    .unwrap();
}

#[tokio::test]
async fn get_admin_config_filters_network_proxy_namespace() {
    let app = TestApp::new().await;
    insert_system_config(&app.db_path, "network_proxy.mode", "manual");
    insert_system_config(
        &app.db_path,
        "network_proxy.http_url",
        "http://user:password@proxy.example.com:8080",
    );

    let resp = app.get("/api/admin/config", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    let configs = body["data"].as_array().unwrap();
    assert!(
        configs
            .iter()
            .all(|item| !item["key"].as_str().unwrap().starts_with("network_proxy.")),
        "network proxy keys must not be exposed through generic config API: {configs:?}"
    );
}

#[tokio::test]
async fn put_admin_config_rejects_network_proxy_namespace() {
    let app = TestApp::new().await;
    insert_system_config(&app.db_path, "network_proxy.mode", "inherit_env");

    let resp = app
        .put(
            "/api/admin/config",
            &json!({
                "configs": [{
                    "key": "network_proxy.mode",
                    "value": "disabled"
                }]
            }),
            Some(&app.admin_token),
        )
        .await;

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn put_admin_config_rejects_unknown_config_keys() {
    let app = TestApp::new().await;

    let resp = app
        .put(
            "/api/admin/config",
            &json!({
                "configs": [{
                    "key": "unknown_config_key",
                    "value": "unexpected"
                }]
            }),
            Some(&app.admin_token),
        )
        .await;

    assert_eq!(resp.status(), 404);

    let resp = app.get("/api/admin/config", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let configs = body["data"].as_array().unwrap();
    assert!(configs
        .iter()
        .all(|item| item["key"].as_str().unwrap() != "unknown_config_key"));
}

#[tokio::test]
async fn get_network_proxy_returns_default_structured_config() {
    let app = TestApp::new().await;

    let resp = app
        .get("/api/admin/network-proxy", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["mode"], "inherit_env");
    assert_eq!(body["data"]["version"], "1");
    assert_eq!(body["data"]["httpProxy"]["configured"], false);
    assert_eq!(body["data"]["httpsProxy"]["configured"], false);
    assert_eq!(body["data"]["allProxy"]["configured"], false);
}

#[tokio::test]
async fn put_network_proxy_noop_does_not_bump_version_or_restart_count() {
    let app = TestApp::new().await;
    let project = app
        .create_test_project("https://github.com/owner/proxy-noop", &app.admin_token)
        .await;
    mark_project_running_with_proxy_version(&app.db_path, app.get_project_id(&project), Some("1"));

    let resp = app
        .put(
            "/api/admin/network-proxy",
            &json!({
                "expectedVersion": "1",
                "mode": "inherit_env",
                "httpProxy": { "action": "clear" },
                "httpsProxy": { "action": "clear" },
                "allProxy": { "action": "clear" },
                "noProxy": "",
                "autoBypassLocal": true
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["version"], "1");
    assert_eq!(body["data"]["needsRestartProjectCount"], 0);
}

#[tokio::test]
async fn put_network_proxy_stores_and_returns_only_redacted_secret() {
    let app = TestApp::new().await;

    let resp = app
        .put(
            "/api/admin/network-proxy",
            &json!({
                "expectedVersion": "1",
                "mode": "manual",
                "httpProxy": {
                    "action": "set",
                    "value": "http://user:password@proxy.example.com:8080?token=secret"
                },
                "httpsProxy": { "action": "clear" },
                "allProxy": { "action": "clear" },
                "noProxy": "localhost,127.0.0.1,::1,.example.com,10.0.0.0/8",
                "autoBypassLocal": true
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["mode"], "manual");
    assert_eq!(body["data"]["version"], "2");
    assert_eq!(body["data"]["httpProxy"]["configured"], true);

    let display = body["data"]["httpProxy"]["displayValue"].as_str().unwrap();
    assert!(display.contains("proxy.example.com:8080"));
    assert!(!display.contains("password"));
    assert!(!display.contains("token=secret"));
}

#[tokio::test]
async fn put_network_proxy_rejects_masked_secret_placeholder() {
    let app = TestApp::new().await;

    let resp = app
        .put(
            "/api/admin/network-proxy",
            &json!({
                "expectedVersion": "1",
                "mode": "manual",
                "httpProxy": {
                    "action": "set",
                    "value": "http://u***r:***@proxy.example.com:8080"
                },
                "httpsProxy": { "action": "clear" },
                "allProxy": { "action": "clear" },
                "noProxy": "localhost,127.0.0.1,::1",
                "autoBypassLocal": true
            }),
            Some(&app.admin_token),
        )
        .await;

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn put_network_proxy_rejects_keep_for_missing_secret() {
    let app = TestApp::new().await;

    let resp = app
        .put(
            "/api/admin/network-proxy",
            &json!({
                "expectedVersion": "1",
                "mode": "disabled",
                "httpProxy": { "action": "keep" },
                "httpsProxy": { "action": "clear" },
                "allProxy": { "action": "clear" },
                "noProxy": "",
                "autoBypassLocal": true
            }),
            Some(&app.admin_token),
        )
        .await;

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn draft_proxy_test_validates_draft_without_persisting_it() {
    let app = TestApp::new().await;

    let resp = app
        .post(
            "/api/admin/network-proxy/test",
            &json!({
                "targetId": "github",
                "useDraftConfig": true,
                "draftConfig": {
                    "expectedVersion": "stale-but-ignored-for-draft-test",
                    "mode": "manual",
                    "httpProxy": {
                        "action": "set",
                        "value": "http://proxy.example.com:8080"
                    },
                    "httpsProxy": { "action": "clear" },
                    "allProxy": { "action": "clear" },
                    "noProxy": "localhost:3000",
                    "autoBypassLocal": true
                }
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"], "validation_failed");
    assert!(body["data"]["message"]
        .as_str()
        .unwrap()
        .contains("NO_PROXY"));

    let resp = app
        .get("/api/admin/network-proxy", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["version"], "1");
    assert_eq!(body["data"]["mode"], "inherit_env");
    assert_eq!(body["data"]["httpProxy"]["configured"], false);
}

#[tokio::test]
async fn draft_proxy_test_rejects_custom_url_fields_inside_draft_config() {
    let app = TestApp::new().await;

    let resp = app
        .post(
            "/api/admin/network-proxy/test",
            &json!({
                "targetId": "github",
                "useDraftConfig": true,
                "draftConfig": {
                    "expectedVersion": "1",
                    "mode": "disabled",
                    "httpProxy": { "action": "clear" },
                    "httpsProxy": { "action": "clear" },
                    "allProxy": { "action": "clear" },
                    "noProxy": "",
                    "autoBypassLocal": true,
                    "targetUrl": "http://127.0.0.1:1"
                }
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"], "validation_failed");
    assert!(body["data"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown field"));
}

#[tokio::test]
async fn proxy_test_rejects_custom_url_fields() {
    let app = TestApp::new().await;

    let resp = app
        .post(
            "/api/admin/network-proxy/test",
            &json!({
                "targetId": "github",
                "targetUrl": "http://127.0.0.1:1",
                "useDraftConfig": false
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"], "validation_failed");
}

#[tokio::test]
async fn proxy_test_rejects_unknown_target_id() {
    let app = TestApp::new().await;

    let resp = app
        .post(
            "/api/admin/network-proxy/test",
            &json!({
                "targetId": "internal-service",
                "useDraftConfig": false
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"], "validation_failed");
}

#[tokio::test]
async fn proxy_test_returns_validation_failed_when_stored_config_is_corrupt() {
    let app = TestApp::new().await;
    insert_system_config(&app.db_path, "network_proxy.mode", "manual");
    insert_proxy_secret(
        &app.db_path,
        "network_proxy.http_url",
        "not-valid-encrypted-proxy-secret",
    );

    let resp = app
        .post(
            "/api/admin/network-proxy/test",
            &json!({
                "targetId": "github",
                "useDraftConfig": false
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"], "validation_failed");
}

#[tokio::test]
async fn corrupted_manual_secret_fail_closed_for_get_effective_and_test() {
    let app = TestApp::new().await;
    insert_system_config(&app.db_path, "network_proxy.mode", "manual");
    insert_system_config(&app.db_path, "network_proxy.version", "7");
    insert_corrupt_proxy_secret(&app.db_path, "network_proxy.http_url");

    let resp = app
        .get("/api/admin/network-proxy", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["mode"], "disabled");
    assert_eq!(body["data"]["source"], "fallback_disabled");
    assert!(body["data"]["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning["blocking"] == true));

    let resp = app
        .get("/api/admin/network-proxy/effective", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["mode"], "disabled");
    assert_eq!(body["data"]["version"], "7");
    assert_eq!(body["data"]["source"], "fallback_disabled");

    let resp = app
        .post(
            "/api/admin/network-proxy/test",
            &json!({
                "targetId": "github",
                "useDraftConfig": false
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"], "validation_failed");
}

#[tokio::test]
async fn restart_count_only_counts_running_projects_with_stale_proxy_version() {
    let app = TestApp::new().await;
    insert_system_config(&app.db_path, "network_proxy.version", "5");

    let fresh = app
        .create_test_project("https://github.com/owner/proxy-fresh", &app.admin_token)
        .await;
    let stale = app
        .create_test_project("https://github.com/owner/proxy-stale", &app.admin_token)
        .await;
    let stopped_stale = app
        .create_test_project(
            "https://github.com/owner/proxy-stopped-stale",
            &app.admin_token,
        )
        .await;

    mark_project_running_with_proxy_version(&app.db_path, app.get_project_id(&fresh), Some("5"));
    mark_project_running_with_proxy_version(&app.db_path, app.get_project_id(&stale), Some("4"));
    mark_project_running_with_proxy_version(
        &app.db_path,
        app.get_project_id(&stopped_stale),
        Some("4"),
    );
    let conn = rusqlite::Connection::open(&app.db_path).unwrap();
    conn.execute(
        "UPDATE projects SET service_status = 'stopped', service_pid = NULL WHERE id = ?1",
        [app.get_project_id(&stopped_stale)],
    )
    .unwrap();

    let resp = app
        .get("/api/admin/network-proxy", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["needsRestartProjectCount"], 1);
}

#[tokio::test]
async fn project_diagnostics_include_redacted_proxy_version_state() {
    let app = TestApp::new().await;
    insert_system_config(&app.db_path, "network_proxy.version", "9");
    let project = app
        .create_test_project(
            "https://github.com/owner/proxy-diagnostics",
            &app.admin_token,
        )
        .await;
    let project_id = app.get_project_id(&project);
    mark_project_running_with_proxy_version(&app.db_path, project_id, Some("8"));

    let resp = app
        .get(
            &format!("/api/projects/{project_id}/diagnostics"),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let proxy = &body["data"]["services"][0]["proxy"];
    assert_eq!(proxy["globalProxyConfigVersion"], "9");
    assert_eq!(proxy["serviceProxyConfigVersion"], "8");
    assert_eq!(proxy["needsRestart"], true);
    assert!(proxy.get("httpProxy").is_none());
    assert!(proxy.get("httpsProxy").is_none());
    assert!(proxy.get("allProxy").is_none());
}
