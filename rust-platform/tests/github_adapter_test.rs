//! GitHub adapter unit tests using wiremock for HTTP mocking.
//!
//! Tests the GitHub adapter's interaction with the GitHub REST API,
//! including issue fetching, label operations, comments, and pagination.

#![allow(dead_code)]

mod common;

use common::{
    mock_add_labels_response, mock_comments_response, mock_create_comment_response,
    mock_github_issues_response, mock_github_user_response, test_config,
};
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// =============================================================================
// Helper: Simple HTTP client that mirrors the GitHub adapter's behavior
// =============================================================================

/// Minimal GitHub adapter for testing HTTP interactions.
/// In production, this would be `symphony_platform::platform::github::GithubAdapter`.
struct GithubTestClient {
    client: reqwest::Client,
    base_url: String,
    owner: String,
    repo: String,
    token: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GithubIssue {
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    html_url: String,
    labels: Vec<GithubLabel>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GithubLabel {
    id: u64,
    name: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GithubComment {
    id: u64,
    body: String,
    user: GithubUser,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct GithubUser {
    login: String,
}

#[derive(Debug)]
enum AdapterError {
    Http(u16),
    NotFound(String),
    Network(String),
    PartialLabelUpdate {
        added: Vec<String>,
        failed: Vec<String>,
    },
}

impl GithubTestClient {
    fn new(config: &common::TestPlatformConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: config.base_url.clone(),
            owner: config.owner.clone(),
            repo: config.repo.clone(),
            token: config.api_token.clone(),
        }
    }

    async fn fetch_candidate_issues(&self) -> Result<Vec<GithubIssue>, AdapterError> {
        let url = format!(
            "{}/repos/{}/{}/issues",
            self.base_url, self.owner, self.repo
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .query(&[("state", "open"), ("per_page", "100")])
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        if status == 404 {
            return Err(AdapterError::NotFound("Issues not found".to_string()));
        }
        if !resp.status().is_success() {
            return Err(AdapterError::Http(status));
        }

        let issues: Vec<GithubIssue> = resp
            .json()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;
        Ok(issues)
    }

    async fn fetch_issue(&self, number: u64) -> Result<GithubIssue, AdapterError> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.base_url, self.owner, self.repo, number
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let status = resp.status().as_u16();
        if status == 404 {
            return Err(AdapterError::NotFound(format!("Issue #{} not found", number)));
        }
        if !resp.status().is_success() {
            return Err(AdapterError::Http(status));
        }

        resp.json()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))
    }

    async fn add_labels(&self, number: u64, labels: &[String]) -> Result<(), AdapterError> {
        if labels.is_empty() {
            return Ok(());
        }

        let url = format!(
            "{}/repos/{}/{}/issues/{}/labels",
            self.base_url, self.owner, self.repo, number
        );

        let body = json!({ "labels": labels });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AdapterError::Http(resp.status().as_u16()));
        }
        Ok(())
    }

    async fn remove_labels(
        &self,
        number: u64,
        labels: &[String],
    ) -> Result<(), AdapterError> {
        if labels.is_empty() {
            return Ok(());
        }

        let mut failed = Vec::new();
        let mut succeeded = Vec::new();

        for label in labels {
            let url = format!(
                "{}/repos/{}/{}/issues/{}/labels/{}",
                self.base_url, self.owner, self.repo, number, label
            );

            let resp = self
                .client
                .delete(&url)
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(|e| AdapterError::Network(e.to_string()))?;

            if resp.status().is_success() {
                succeeded.push(label.clone());
            } else {
                failed.push(label.clone());
            }
        }

        if !failed.is_empty() {
            return Err(AdapterError::PartialLabelUpdate {
                added: succeeded,
                failed,
            });
        }
        Ok(())
    }

    async fn set_workflow_state(
        &self,
        number: u64,
        new_state_label: &str,
        old_state_labels: &[String],
    ) -> Result<(), AdapterError> {
        // Add new label first (compensating transaction pattern)
        self.add_labels(number, &[new_state_label.to_string()])
            .await?;

        // Then remove old labels
        let stale: Vec<String> = old_state_labels
            .iter()
            .filter(|l| *l != new_state_label)
            .cloned()
            .collect();
        if !stale.is_empty() {
            // Partial failure is acceptable here (logged, not fatal)
            let _ = self.remove_labels(number, &stale).await;
        }
        Ok(())
    }

    async fn create_comment(&self, number: u64, body: &str) -> Result<u64, AdapterError> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            self.base_url, self.owner, self.repo, number
        );

        let payload = json!({ "body": body });

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AdapterError::Http(resp.status().as_u16()));
        }

        let comment: GithubComment = resp
            .json()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;
        Ok(comment.id)
    }

    async fn find_workpad_comment(
        &self,
        number: u64,
    ) -> Result<Option<(u64, String)>, AdapterError> {
        let url = format!(
            "{}/repos/{}/{}/issues/{}/comments",
            self.base_url, self.owner, self.repo, number
        );

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AdapterError::Http(resp.status().as_u16()));
        }

        let comments: Vec<GithubComment> = resp
            .json()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        let workpad = comments
            .into_iter()
            .find(|c| c.body.contains("## Codex Workpad"));

        Ok(workpad.map(|c| (c.id, c.body)))
    }

    async fn validate_credentials(&self) -> Result<(), AdapterError> {
        let url = format!("{}/user", self.base_url);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| AdapterError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AdapterError::Http(resp.status().as_u16()));
        }
        Ok(())
    }

    async fn fetch_all_pages(&self, path_suffix: &str) -> Result<Vec<GithubIssue>, AdapterError> {
        let mut all_issues = Vec::new();
        let mut page = 1u32;

        loop {
            let url = format!(
                "{}/repos/{}/{}/{}",
                self.base_url, self.owner, self.repo, path_suffix
            );

            let resp = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .query(&[
                    ("state", "open"),
                    ("per_page", "2"),
                    ("page", &page.to_string()),
                ])
                .send()
                .await
                .map_err(|e| AdapterError::Network(e.to_string()))?;

            if !resp.status().is_success() {
                return Err(AdapterError::Http(resp.status().as_u16()));
            }

            let has_next = resp
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.contains("rel=\"next\""))
                .unwrap_or(false);

            let issues: Vec<GithubIssue> = resp
                .json()
                .await
                .map_err(|e| AdapterError::Network(e.to_string()))?;
            all_issues.extend(issues);

            if !has_next || page >= 10 {
                break;
            }
            page += 1;
        }

        Ok(all_issues)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[tokio::test]
async fn test_fetch_candidate_issues() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-org/test-repo/issues"))
        .and(header("Authorization", "Bearer test-token-12345"))
        .and(query_param("state", "open"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(mock_github_issues_response()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let issues = client.fetch_candidate_issues().await.unwrap();
    assert_eq!(issues.len(), 2);

    assert_eq!(issues[0].number, 42);
    assert_eq!(issues[0].title, "Implement user authentication");
    assert!(issues[0].labels.iter().any(|l| l.name == "workflow::todo"));

    assert_eq!(issues[1].number, 43);
    assert_eq!(issues[1].title, "Fix database connection pooling");
    assert!(issues[1]
        .labels
        .iter()
        .any(|l| l.name == "workflow::in-progress"));
}

#[tokio::test]
async fn test_fetch_issue_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-org/test-repo/issues/999"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({
                "message": "Not Found",
                "documentation_url": "https://docs.github.com/rest"
            })),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let result = client.fetch_issue(999).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AdapterError::NotFound(msg) => {
            assert!(msg.contains("999"));
        }
        other => panic!("Expected NotFound, got {:?}", other),
    }
}

#[tokio::test]
async fn test_add_labels() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repos/test-org/test-repo/issues/42/labels"))
        .and(header("Authorization", "Bearer test-token-12345"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(mock_add_labels_response()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let labels = vec!["workflow::todo".to_string(), "bug".to_string()];
    let result = client.add_labels(42, &labels).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_remove_labels_partial_failure() {
    let mock_server = MockServer::start().await;

    // First label removal succeeds
    Mock::given(method("DELETE"))
        .and(path("/repos/test-org/test-repo/issues/42/labels/workflow::backlog"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Second label removal fails with 404 (label not found on issue)
    Mock::given(method("DELETE"))
        .and(path("/repos/test-org/test-repo/issues/42/labels/workflow::rework"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(json!({
                "message": "Label does not exist"
            })),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let labels = vec![
        "workflow::backlog".to_string(),
        "workflow::rework".to_string(),
    ];
    let result = client.remove_labels(42, &labels).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        AdapterError::PartialLabelUpdate { added, failed } => {
            assert_eq!(added, vec!["workflow::backlog"]);
            assert_eq!(failed, vec!["workflow::rework"]);
        }
        other => panic!("Expected PartialLabelUpdate, got {:?}", other),
    }
}

#[tokio::test]
async fn test_set_workflow_state() {
    let mock_server = MockServer::start().await;

    // Step 1: Add new label (should happen first)
    Mock::given(method("POST"))
        .and(path("/repos/test-org/test-repo/issues/42/labels"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([
                {"id": 3, "name": "workflow::in-progress", "color": "fbca04"}
            ])),
        )
        .expect(1)
        .named("add_new_label")
        .mount(&mock_server)
        .await;

    // Step 2: Remove old label (should happen after add)
    Mock::given(method("DELETE"))
        .and(path("/repos/test-org/test-repo/issues/42/labels/workflow::todo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .named("remove_old_label")
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let result = client
        .set_workflow_state(
            42,
            "workflow::in-progress",
            &["workflow::todo".to_string()],
        )
        .await;

    assert!(result.is_ok());
    // wiremock will verify both mocks were called exactly once
}

#[tokio::test]
async fn test_create_comment() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/repos/test-org/test-repo/issues/42/comments"))
        .and(header("Authorization", "Bearer test-token-12345"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(mock_create_comment_response()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let comment_id = client
        .create_comment(42, "Test comment body")
        .await
        .unwrap();
    assert_eq!(comment_id, 6001);
}

#[tokio::test]
async fn test_find_workpad_comment() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/test-org/test-repo/issues/42/comments"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(mock_comments_response()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let result = client.find_workpad_comment(42).await.unwrap();
    assert!(result.is_some());

    let (id, body) = result.unwrap();
    assert_eq!(id, 5002);
    assert!(body.contains("## Codex Workpad"));
    assert!(body.contains("Step 1: Analyze requirements"));
}

#[tokio::test]
async fn test_find_workpad_comment_not_present() {
    let mock_server = MockServer::start().await;

    // Return comments without a workpad
    Mock::given(method("GET"))
        .and(path("/repos/test-org/test-repo/issues/42/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "id": 5001,
                "body": "Just a regular comment",
                "user": {"login": "bob"},
                "created_at": "2025-01-10T11:00:00Z",
                "updated_at": "2025-01-10T11:00:00Z"
            }
        ])))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let result = client.find_workpad_comment(42).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_validate_credentials() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("Authorization", "Bearer test-token-12345"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(mock_github_user_response()),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let result = client.validate_credentials().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_validate_credentials_invalid_token() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({
                "message": "Bad credentials",
                "documentation_url": "https://docs.github.com/rest"
            })),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let result = client.validate_credentials().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        AdapterError::Http(status) => assert_eq!(status, 401),
        other => panic!("Expected Http(401), got {:?}", other),
    }
}

#[tokio::test]
async fn test_pagination() {
    let mock_server = MockServer::start().await;

    // Page 1: returns 2 issues with Link header pointing to page 2
    let page1_issues = json!([
        {
            "id": 1001,
            "number": 1,
            "title": "Issue 1",
            "body": null,
            "html_url": "https://github.com/test-org/test-repo/issues/1",
            "labels": [{"id": 1, "name": "workflow::todo"}]
        },
        {
            "id": 1002,
            "number": 2,
            "title": "Issue 2",
            "body": null,
            "html_url": "https://github.com/test-org/test-repo/issues/2",
            "labels": [{"id": 2, "name": "workflow::todo"}]
        }
    ]);

    Mock::given(method("GET"))
        .and(path("/repos/test-org/test-repo/issues"))
        .and(query_param("page", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(page1_issues)
                .append_header(
                    "link",
                    &format!(
                        "<{}/repos/test-org/test-repo/issues?page=2&per_page=2>; rel=\"next\", <{}/repos/test-org/test-repo/issues?page=2&per_page=2>; rel=\"last\"",
                        mock_server.uri(),
                        mock_server.uri()
                    ),
                ),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    // Page 2: returns 1 issue, no Link header (last page)
    let page2_issues = json!([
        {
            "id": 1003,
            "number": 3,
            "title": "Issue 3",
            "body": null,
            "html_url": "https://github.com/test-org/test-repo/issues/3",
            "labels": [{"id": 3, "name": "workflow::rework"}]
        }
    ]);

    Mock::given(method("GET"))
        .and(path("/repos/test-org/test-repo/issues"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(page2_issues))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let issues = client.fetch_all_pages("issues").await.unwrap();
    assert_eq!(issues.len(), 3);
    assert_eq!(issues[0].title, "Issue 1");
    assert_eq!(issues[1].title, "Issue 2");
    assert_eq!(issues[2].title, "Issue 3");
}

#[tokio::test]
async fn test_empty_labels_is_noop() {
    let mock_server = MockServer::start().await;

    // No mocks registered — if the client makes any request, wiremock will return 404
    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    // Empty labels should be a no-op (no HTTP request made)
    let result = client.add_labels(42, &[]).await;
    assert!(result.is_ok());

    let result = client.remove_labels(42, &[]).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_authorization_header_sent() {
    let mock_server = MockServer::start().await;

    // Only match requests with the correct Authorization header
    Mock::given(method("GET"))
        .and(path("/repos/test-org/test-repo/issues"))
        .and(header("Authorization", "Bearer test-token-12345"))
        .and(header("Accept", "application/vnd.github+json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = test_config(&mock_server.uri());
    let client = GithubTestClient::new(&config);

    let issues = client.fetch_candidate_issues().await.unwrap();
    assert_eq!(issues.len(), 0);
}
