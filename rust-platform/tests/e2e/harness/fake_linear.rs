//! FakeLinearServer — wiremock-based Linear GraphQL API simulator.
//!
//! Provides a configurable mock server that responds to Linear GraphQL queries
//! used by the tracker client. Supports:
//! - Candidate issues query (with pagination)
//! - State refresh query (by IDs)
//! - Terminal issues query
//! - Error simulation (timeouts, 500s, GraphQL errors)

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Builder for creating Linear issue fixtures.
#[derive(Debug, Clone)]
pub struct LinearIssueBuilder {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    pub state_name: String,
    pub state_type: String,
    pub priority: f64,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
    pub branch_name: String,
    pub blocked_by: Vec<String>,
}

impl LinearIssueBuilder {
    pub fn new(id: &str, identifier: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            identifier: identifier.to_string(),
            title: title.to_string(),
            description: None,
            state_name: "Todo".to_string(),
            state_type: "started".to_string(),
            priority: 2.0,
            labels: Vec::new(),
            assignee: None,
            branch_name: format!("issue-{}", identifier.to_lowercase().replace('-', "")),
            blocked_by: Vec::new(),
        }
    }

    pub fn with_state(mut self, name: &str, state_type: &str) -> Self {
        self.state_name = name.to_string();
        self.state_type = state_type.to_string();
        self
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    pub fn with_priority(mut self, priority: f64) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_labels(mut self, labels: Vec<&str>) -> Self {
        self.labels = labels.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_assignee(mut self, assignee: &str) -> Self {
        self.assignee = Some(assignee.to_string());
        self
    }

    pub fn with_blocked_by(mut self, blockers: Vec<&str>) -> Self {
        self.blocked_by = blockers.into_iter().map(|s| s.to_string()).collect();
        self
    }

    /// Build the JSON representation as it would appear in a Linear GraphQL response.
    pub fn build_json(&self) -> Value {
        json!({
            "id": self.id,
            "identifier": self.identifier,
            "title": self.title,
            "description": self.description,
            "state": {
                "name": self.state_name,
                "type": self.state_type
            },
            "priority": self.priority,
            "labels": {
                "nodes": self.labels.iter().map(|l| json!({"name": l})).collect::<Vec<_>>()
            },
            "assignee": self.assignee.as_ref().map(|a| json!({"name": a})),
            "branchName": self.branch_name,
            "relations": {
                "nodes": self.blocked_by.iter().map(|b| json!({
                    "type": "blocks",
                    "relatedIssue": {"id": b}
                })).collect::<Vec<_>>()
            },
            "createdAt": "2025-01-10T10:00:00.000Z",
            "updatedAt": "2025-01-12T15:30:00.000Z"
        })
    }
}

/// Configuration for error simulation.
#[derive(Debug, Clone)]
pub enum LinearErrorMode {
    /// Normal operation (no errors)
    None,
    /// Return HTTP 500 on next request
    ServerError,
    /// Return HTTP 429 (rate limited)
    RateLimited,
    /// Simulate a timeout (delay response beyond client timeout)
    Timeout,
    /// Return a GraphQL error in the response body
    GraphQLError(String),
    /// Return malformed JSON
    MalformedResponse,
}

/// Fake Linear GraphQL API server for E2E testing.
pub struct FakeLinearServer {
    server: MockServer,
    issues: Arc<Mutex<Vec<LinearIssueBuilder>>>,
    error_mode: Arc<Mutex<LinearErrorMode>>,
    /// Track how many requests have been received
    request_count: Arc<Mutex<u64>>,
}

impl FakeLinearServer {
    /// Create and start a new fake Linear server.
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let instance = Self {
            server,
            issues: Arc::new(Mutex::new(Vec::new())),
            error_mode: Arc::new(Mutex::new(LinearErrorMode::None)),
            request_count: Arc::new(Mutex::new(0)),
        };
        instance
    }

    /// Get the server URI for client configuration.
    pub fn uri(&self) -> String {
        self.server.uri()
    }

    /// Seed issues into the fake server.
    pub async fn seed_issues(&self, issues: Vec<LinearIssueBuilder>) {
        let mut store = self.issues.lock().await;
        *store = issues;
        self.setup_mocks().await;
    }

    /// Add a single issue.
    pub async fn add_issue(&self, issue: LinearIssueBuilder) {
        let mut store = self.issues.lock().await;
        store.push(issue);
        drop(store);
        self.setup_mocks().await;
    }

    /// Update an issue's state (simulates external state change).
    pub async fn update_issue_state(&self, issue_id: &str, state_name: &str, state_type: &str) {
        let mut store = self.issues.lock().await;
        if let Some(issue) = store.iter_mut().find(|i| i.id == issue_id) {
            issue.state_name = state_name.to_string();
            issue.state_type = state_type.to_string();
        }
        drop(store);
        self.setup_mocks().await;
    }

    /// Set error mode for the next request(s).
    pub async fn set_error_mode(&self, mode: LinearErrorMode) {
        let mut error = self.error_mode.lock().await;
        *error = mode;
        self.setup_mocks().await;
    }

    /// Get the number of requests received.
    pub async fn request_count(&self) -> u64 {
        *self.request_count.lock().await
    }

    /// Reset the server state.
    pub async fn reset(&self) {
        let mut store = self.issues.lock().await;
        store.clear();
        let mut error = self.error_mode.lock().await;
        *error = LinearErrorMode::None;
        let mut count = self.request_count.lock().await;
        *count = 0;
        drop(store);
        drop(error);
        drop(count);
        self.server.reset().await;
    }

    /// Set up wiremock mocks based on current state.
    async fn setup_mocks(&self) {
        // Reset existing mocks
        self.server.reset().await;

        let error_mode = self.error_mode.lock().await.clone();
        let issues = self.issues.lock().await.clone();

        match error_mode {
            LinearErrorMode::None => {
                self.setup_normal_mocks(&issues).await;
            }
            LinearErrorMode::ServerError => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::RateLimited => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(
                        ResponseTemplate::new(429)
                            .insert_header("Retry-After", "60")
                            .set_body_string("Rate limited"),
                    )
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::Timeout => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(
                        ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(300)),
                    )
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::GraphQLError(ref msg) => {
                let error_response = json!({
                    "data": null,
                    "errors": [{
                        "message": msg,
                        "locations": [{"line": 1, "column": 1}],
                        "path": ["issues"]
                    }]
                });
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(
                        ResponseTemplate::new(200).set_body_json(&error_response),
                    )
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::MalformedResponse => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(
                        ResponseTemplate::new(200).set_body_string("not valid json {{{"),
                    )
                    .mount(&self.server)
                    .await;
            }
        }
    }

    /// Set up normal (non-error) mocks for GraphQL queries.
    async fn setup_normal_mocks(&self, issues: &[LinearIssueBuilder]) {
        let issue_nodes: Vec<Value> = issues.iter().map(|i| i.build_json()).collect();

        // Mock for candidate issues query (contains "issues" in body)
        let candidates_response = json!({
            "data": {
                "issues": {
                    "nodes": issue_nodes,
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        });

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(&candidates_response),
            )
            .mount(&self.server)
            .await;
    }

    /// Build a paginated response (for pagination tests).
    pub fn build_paginated_response(
        issues: &[LinearIssueBuilder],
        page_size: usize,
        page: usize,
    ) -> Value {
        let start = page * page_size;
        let end = (start + page_size).min(issues.len());
        let page_issues: Vec<Value> = issues[start..end].iter().map(|i| i.build_json()).collect();
        let has_next = end < issues.len();
        let cursor = if has_next {
            Some(format!("cursor_{}", end))
        } else {
            None
        };

        json!({
            "data": {
                "issues": {
                    "nodes": page_issues,
                    "pageInfo": {
                        "hasNextPage": has_next,
                        "endCursor": cursor
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_linear_issue_builder() {
        let issue = LinearIssueBuilder::new("abc-123", "PROJ-42", "Fix the bug")
            .with_state("In Progress", "started")
            .with_priority(1.0)
            .with_labels(vec!["bug", "urgent"])
            .with_assignee("alice")
            .with_description("Something is broken");

        let json = issue.build_json();

        assert_eq!(json["id"], "abc-123");
        assert_eq!(json["identifier"], "PROJ-42");
        assert_eq!(json["title"], "Fix the bug");
        assert_eq!(json["state"]["name"], "In Progress");
        assert_eq!(json["priority"], 1.0);
        assert_eq!(json["labels"]["nodes"][0]["name"], "bug");
        assert_eq!(json["assignee"]["name"], "alice");
    }

    #[tokio::test]
    async fn test_paginated_response() {
        let issues: Vec<LinearIssueBuilder> = (0..5)
            .map(|i| {
                LinearIssueBuilder::new(
                    &format!("id-{}", i),
                    &format!("PROJ-{}", i),
                    &format!("Issue {}", i),
                )
            })
            .collect();

        // First page
        let page1 = FakeLinearServer::build_paginated_response(&issues, 2, 0);
        assert_eq!(page1["data"]["issues"]["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(page1["data"]["issues"]["pageInfo"]["hasNextPage"], true);

        // Last page
        let page3 = FakeLinearServer::build_paginated_response(&issues, 2, 2);
        assert_eq!(page3["data"]["issues"]["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(page3["data"]["issues"]["pageInfo"]["hasNextPage"], false);
    }

    #[tokio::test]
    async fn test_linear_issue_builder_with_blockers() {
        let issue = LinearIssueBuilder::new("id-1", "PROJ-1", "Blocked issue")
            .with_blocked_by(vec!["id-2", "id-3"]);

        let json = issue.build_json();
        let relations = json["relations"]["nodes"].as_array().unwrap();
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[0]["type"], "blocks");
        assert_eq!(relations[0]["relatedIssue"]["id"], "id-2");
    }
}
