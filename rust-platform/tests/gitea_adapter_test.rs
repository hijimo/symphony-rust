//! Gitea adapter unit tests using wiremock for HTTP mocking.
//!
//! Tests the Gitea adapter's interaction with the Gitea REST API,
//! including authentication, issue fetching, label operations (ID-based),
//! comments, pagination, and credential validation.

use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use symphony_platform::config::platform::{IssueFilter, PlatformConfig, WorkflowConfig};
use symphony_platform::platform::gitea::GiteaAdapter;
use symphony_platform::platform::{FetchOptions, IssueId, Platform};

use std::collections::HashMap;

// =============================================================================
// Helpers
// =============================================================================

fn test_platform_config(base_url: &str) -> PlatformConfig {
    let mut states = HashMap::new();
    states.insert("todo".to_string(), "Todo".to_string());
    states.insert("in_progress".to_string(), "In Progress".to_string());
    states.insert("done".to_string(), "Done".to_string());

    PlatformConfig {
        kind: "gitea".to_string(),
        api_token: "test-gitea-token".to_string(),
        base_url: base_url.to_string(),
        owner: "testorg".to_string(),
        repo: "testrepo".to_string(),
        project_id: None,
        allow_custom_host: true,
        issue_filter: IssueFilter {
            labels: vec!["Todo".to_string(), "In Progress".to_string()],
            assignee: None,
            milestone: None,
        },
        workflow: WorkflowConfig {
            states,
            active_states: vec!["todo".to_string(), "in_progress".to_string()],
            terminal_states: vec!["done".to_string()],
        },
    }
}

fn mock_gitea_issue(number: u64, title: &str, labels: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "id": number * 100,
        "number": number,
        "title": title,
        "body": "Issue body",
        "html_url": format!("https://gitea.example.com/testorg/testrepo/issues/{}", number),
        "state": "open",
        "labels": labels,
        "assignee": {"login": "dev1"},
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-16T12:00:00Z",
        "pull_request": null
    })
}

fn mock_label(id: u64, name: &str) -> serde_json::Value {
    json!({"id": id, "name": name})
}

// =============================================================================
// Authentication tests
// =============================================================================

#[tokio::test]
async fn test_gitea_auth_header_uses_token_format() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("Authorization", "token test-gitea-token"))
        .and(header("Accept", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "testuser"})))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    adapter.validate_credentials().await.unwrap();
}

#[tokio::test]
async fn test_validate_credentials_401_returns_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "bad-token").unwrap();
    let result = adapter.validate_credentials().await;
    assert!(result.is_err());
}

// =============================================================================
// Issue fetching tests
// =============================================================================

#[tokio::test]
async fn test_fetch_candidate_issues_filters_prs() {
    let mock_server = MockServer::start().await;

    let issue_with_pr = json!({
        "id": 200,
        "number": 2,
        "title": "This is a PR",
        "body": null,
        "html_url": "https://gitea.example.com/testorg/testrepo/issues/2",
        "state": "open",
        "labels": [mock_label(10, "Todo")],
        "assignee": null,
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-16T12:00:00Z",
        "pull_request": {"merged": false}
    });

    let real_issue = mock_gitea_issue(1, "Real issue", vec![mock_label(10, "Todo")]);

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "Todo"))
        .and(query_param("state", "open"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([real_issue, issue_with_pr])),
        )
        .mount(&mock_server)
        .await;

    // In Progress label query returns empty
    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "In Progress"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let issues = adapter.fetch_candidate_issues(FetchOptions::default()).await.unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].title, "Real issue");
}

#[tokio::test]
async fn test_fetch_candidate_issues_deduplicates() {
    let mock_server = MockServer::start().await;

    let issue = mock_gitea_issue(
        1,
        "Dual label issue",
        vec![mock_label(10, "Todo"), mock_label(11, "In Progress")],
    );

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "Todo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([issue.clone()])))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "In Progress"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([issue])))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let issues = adapter.fetch_candidate_issues(FetchOptions::default()).await.unwrap();

    assert_eq!(issues.len(), 1);
}

#[tokio::test]
async fn test_fetch_candidate_issues_pagination() {
    let mock_server = MockServer::start().await;

    // Page 1: 50 issues (full page → triggers page 2 fetch)
    let page1: Vec<serde_json::Value> = (1..=50)
        .map(|i| mock_gitea_issue(i, &format!("Issue {}", i), vec![mock_label(10, "Todo")]))
        .collect();

    // Page 2: 10 issues (less than limit → last page)
    let page2: Vec<serde_json::Value> = (51..=60)
        .map(|i| mock_gitea_issue(i, &format!("Issue {}", i), vec![mock_label(10, "Todo")]))
        .collect();

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "Todo"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(page1)))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "Todo"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(page2)))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "In Progress"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let issues = adapter.fetch_candidate_issues(FetchOptions::default()).await.unwrap();

    assert_eq!(issues.len(), 60);
}

#[tokio::test]
async fn test_fetch_issue_single() {
    let mock_server = MockServer::start().await;

    let issue = mock_gitea_issue(42, "Single issue", vec![mock_label(10, "Todo")]);

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let issue = adapter.fetch_issue(IssueId(42)).await.unwrap();

    assert_eq!(issue.number, 42);
    assert_eq!(issue.title, "Single issue");
    assert_eq!(issue.workflow_state, Some("todo".to_string()));
}

#[tokio::test]
async fn test_closed_issue_infers_done_state() {
    let mock_server = MockServer::start().await;

    let issue = json!({
        "id": 500,
        "number": 5,
        "title": "Closed issue",
        "body": null,
        "html_url": "https://gitea.example.com/testorg/testrepo/issues/5",
        "state": "closed",
        "labels": [mock_label(11, "In Progress")],
        "assignee": null,
        "created_at": "2024-01-15T10:00:00Z",
        "updated_at": "2024-01-16T12:00:00Z",
        "pull_request": null
    });

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues/5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(issue))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let issue = adapter.fetch_issue(IssueId(5)).await.unwrap();

    assert_eq!(issue.workflow_state, Some("done".to_string()));
}

// =============================================================================
// Label operations tests (ID-based)
// =============================================================================

#[tokio::test]
async fn test_add_labels_resolves_ids() {
    let mock_server = MockServer::start().await;

    // Label cache fetch
    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "name": "Todo"},
            {"id": 11, "name": "In Progress"},
            {"id": 12, "name": "Done"}
        ])))
        .mount(&mock_server)
        .await;

    // Add labels with IDs
    Mock::given(method("POST"))
        .and(path("/repos/testorg/testrepo/issues/1/labels"))
        .and(body_json(json!({"labels": [11]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{"id": 11, "name": "In Progress"}])))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    adapter
        .add_labels(IssueId(1), &["In Progress".to_string()])
        .await
        .unwrap();
}

#[tokio::test]
async fn test_add_labels_not_found_returns_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "name": "Todo"}
        ])))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let result = adapter
        .add_labels(IssueId(1), &["NonExistent".to_string()])
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_remove_labels_uses_id_in_path() {
    let mock_server = MockServer::start().await;

    // Label cache
    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "name": "Todo"},
            {"id": 11, "name": "In Progress"}
        ])))
        .mount(&mock_server)
        .await;

    // DELETE with label ID in path
    Mock::given(method("DELETE"))
        .and(path("/repos/testorg/testrepo/issues/1/labels/10"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    adapter
        .remove_labels(IssueId(1), &["Todo".to_string()])
        .await
        .unwrap();
}

#[tokio::test]
async fn test_label_cache_hit_no_refetch() {
    let mock_server = MockServer::start().await;

    // Label cache — should only be called once
    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 10, "name": "Todo"},
            {"id": 11, "name": "In Progress"}
        ])))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/testorg/testrepo/issues/1/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/testorg/testrepo/issues/2/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();

    // Two add_labels calls — cache should be populated on first, reused on second
    adapter
        .add_labels(IssueId(1), &["Todo".to_string()])
        .await
        .unwrap();
    adapter
        .add_labels(IssueId(2), &["In Progress".to_string()])
        .await
        .unwrap();
}

// =============================================================================
// Comment tests
// =============================================================================

#[tokio::test]
async fn test_create_comment() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repos/testorg/testrepo/issues/1/comments"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({
                "id": 999,
                "body": "Hello",
                "user": {"login": "bot"},
                "created_at": "2024-01-15T10:00:00Z"
            })),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let comment_id = adapter.create_comment(IssueId(1), "Hello").await.unwrap();

    assert_eq!(comment_id.0, 999);
}

#[tokio::test]
async fn test_find_workpad_comment() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues/1/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": 1, "body": "Regular comment", "user": {"login": "dev"}, "created_at": "2024-01-15T10:00:00Z"},
            {"id": 2, "body": "## Codex Workpad\n\nPlan here", "user": {"login": "bot"}, "created_at": "2024-01-15T11:00:00Z"}
        ])))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    let result = adapter.find_workpad_comment(IssueId(1)).await.unwrap();

    assert!(result.is_some());
    let (id, body) = result.unwrap();
    assert_eq!(id.0, 2);
    assert!(body.contains("## Codex Workpad"));
}

#[tokio::test]
async fn test_update_comment() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PATCH"))
        .and(path("/repos/testorg/testrepo/issues/comments/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 42,
            "body": "Updated",
            "user": {"login": "bot"},
            "created_at": "2024-01-15T10:00:00Z"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    adapter
        .update_comment(symphony_platform::platform::CommentId(42), "Updated")
        .await
        .unwrap();
}

// =============================================================================
// PR creation test
// =============================================================================

#[tokio::test]
async fn test_create_pull_request() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repos/testorg/testrepo/pulls"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": 100,
            "number": 5,
            "html_url": "https://gitea.example.com/testorg/testrepo/pulls/5",
            "state": "open"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();

    let pr = adapter
        .create_pull_request(symphony_platform::platform::CreatePrParams {
            title: "Fix bug".to_string(),
            body: "Closes #1".to_string(),
            head: "fix/issue-1".to_string(),
            base: "main".to_string(),
            draft: false,
        })
        .await
        .unwrap();

    assert_eq!(pr.number, 5);
    assert_eq!(pr.state, "open");
}

// =============================================================================
// Pagination parameter test
// =============================================================================

#[tokio::test]
async fn test_pagination_uses_limit_not_per_page() {
    let mock_server = MockServer::start().await;

    // Verify that Gitea uses `limit` parameter (not `per_page`)
    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("limit", "50"))
        .and(query_param("page", "1"))
        .and(query_param("labels", "Todo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/testorg/testrepo/issues"))
        .and(query_param("labels", "In Progress"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&mock_server)
        .await;

    let config = test_platform_config(&mock_server.uri());
    let adapter = GiteaAdapter::new_with_token(config, "test-gitea-token").unwrap();
    adapter.fetch_candidate_issues(FetchOptions::default()).await.unwrap();
}
