//! Runner state hand-offs pass display names ("Human Review", "In Progress")
//! while platform adapters key workflow states by normalized names
//! ("human_review"). The tracker must normalize before calling the platform.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use symphony_platform::config::platform::{IssueFilter, PlatformConfig, WorkflowConfig};
use symphony_platform::platform::github::GithubAdapter;
use symphony_platform::platform::Platform;
use symphony_platform::tracker::gitlab::GitlabTrackerAdapter;
use symphony_platform::tracker::Tracker;

fn config(base: &str) -> PlatformConfig {
    // Same shape as main.rs::build_workflow_states: normalized key -> label.
    let mut states = HashMap::new();
    states.insert("in_progress".to_string(), "In Progress".to_string());
    states.insert("human_review".to_string(), "Human Review".to_string());
    PlatformConfig {
        kind: "github".into(),
        api_token: "unused".into(),
        base_url: base.into(),
        owner: "acme".into(),
        repo: "app".into(),
        project_id: None,
        allow_custom_host: true,
        issue_filter: IssueFilter {
            labels: vec![],
            assignee: None,
            milestone: None,
        },
        workflow: WorkflowConfig {
            states,
            active_states: vec!["In Progress".into()],
            terminal_states: vec![],
        },
    }
}

#[tokio::test]
async fn test_set_workflow_state_accepts_display_names() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/app/issues/7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 1, "number": 7, "title": "t", "body": null,
            "html_url": "u", "state": "open", "assignee": null,
            "labels": [{"name": "Human Review"}],
            "created_at": "2026-11-13T00:00:00Z", "updated_at": "2026-11-13T00:00:00Z"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/app/issues/7/labels"))
        .and(body_json(json!({"labels": ["In Progress"]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/repos/acme/app/issues/7/labels/Human%20Review"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let platform: Arc<dyn Platform> =
        Arc::new(GithubAdapter::new_with_token(config(&server.uri()), "t").unwrap());
    let tracker = GitlabTrackerAdapter::new(platform, vec!["In Progress".into()], vec![]);
    tracker
        .set_workflow_state("7", "In Progress")
        .await
        .expect("display name resolves to the in_progress key");
}
