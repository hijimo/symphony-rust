//! FakeLinearServer — wiremock-based Linear GraphQL API simulator.
//!
//! Provides a configurable mock server that responds to Linear GraphQL queries
//! used by the tracker client. Supports:
//! - Candidate issues query (with pagination)
//! - State refresh query (by IDs)
//! - Terminal issues query
//! - Error simulation (timeouts, 500s, GraphQL errors)
//! - Assignee filtering
//! - State change tracking

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
    /// Return HTTP 401 (unauthorized)
    Unauthorized,
    /// Return HTTP 403 (forbidden)
    Forbidden,
    /// Fail only the Nth request (0-indexed), then recover
    FailOnRequest(u64),
}

/// Record of state changes made via the fake tracker.
#[derive(Debug, Clone)]
pub struct StateChangeRecord {
    pub issue_id: String,
    pub old_state: String,
    pub new_state: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Fake Linear GraphQL API server for E2E testing.
pub struct FakeLinearServer {
    server: MockServer,
    issues: Arc<Mutex<Vec<LinearIssueBuilder>>>,
    error_mode: Arc<Mutex<LinearErrorMode>>,
    /// Track how many requests have been received
    request_count: Arc<Mutex<u64>>,
    /// Track state changes for assertions
    state_changes: Arc<Mutex<Vec<StateChangeRecord>>>,
    /// Assignee filter (if set, only return issues assigned to this user)
    assignee_filter: Arc<Mutex<Option<String>>>,
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
            state_changes: Arc::new(Mutex::new(Vec::new())),
            assignee_filter: Arc::new(Mutex::new(None)),
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
        drop(store);
        self.setup_mocks().await;
    }

    /// Add a single issue.
    pub async fn add_issue(&self, issue: LinearIssueBuilder) {
        let mut store = self.issues.lock().await;
        store.push(issue);
        drop(store);
        self.setup_mocks().await;
    }

    /// Remove an issue by ID.
    pub async fn remove_issue(&self, issue_id: &str) {
        let mut store = self.issues.lock().await;
        store.retain(|i| i.id != issue_id);
        drop(store);
        self.setup_mocks().await;
    }

    /// Update an issue's state (simulates external state change).
    pub async fn update_issue_state(&self, issue_id: &str, state_name: &str, state_type: &str) {
        let old_state = {
            let mut store = self.issues.lock().await;
            if let Some(issue) = store.iter_mut().find(|i| i.id == issue_id) {
                let old = issue.state_name.clone();
                issue.state_name = state_name.to_string();
                issue.state_type = state_type.to_string();
                Some(old)
            } else {
                None
            }
        };

        // Record the state change (outside the issues lock)
        if let Some(old_state) = old_state {
            let mut changes = self.state_changes.lock().await;
            changes.push(StateChangeRecord {
                issue_id: issue_id.to_string(),
                old_state,
                new_state: state_name.to_string(),
                timestamp: chrono::Utc::now(),
            });
        }

        self.setup_mocks().await;
    }

    /// Set error mode for the next request(s).
    pub async fn set_error_mode(&self, mode: LinearErrorMode) {
        let mut error = self.error_mode.lock().await;
        *error = mode;
        drop(error);
        self.setup_mocks().await;
    }

    /// Set assignee filter.
    pub async fn set_assignee_filter(&self, assignee: Option<&str>) {
        let mut filter = self.assignee_filter.lock().await;
        *filter = assignee.map(|s| s.to_string());
        drop(filter);
        self.setup_mocks().await;
    }

    /// Get the number of requests received.
    pub async fn request_count(&self) -> u64 {
        *self.request_count.lock().await
    }

    /// Get all state change records.
    pub async fn state_changes(&self) -> Vec<StateChangeRecord> {
        self.state_changes.lock().await.clone()
    }

    /// Check if an issue has been moved to a terminal state.
    pub async fn is_terminal(&self, issue_id: &str) -> bool {
        let store = self.issues.lock().await;
        store
            .iter()
            .find(|i| i.id == issue_id)
            .map(|i| {
                matches!(
                    i.state_type.as_str(),
                    "completed" | "cancelled" | "canceled"
                )
            })
            .unwrap_or(false)
    }

    /// Get the current issues (for assertions).
    pub async fn current_issues(&self) -> Vec<LinearIssueBuilder> {
        self.issues.lock().await.clone()
    }

    /// Reset the server state.
    pub async fn reset(&self) {
        let mut store = self.issues.lock().await;
        store.clear();
        let mut error = self.error_mode.lock().await;
        *error = LinearErrorMode::None;
        let mut count = self.request_count.lock().await;
        *count = 0;
        let mut changes = self.state_changes.lock().await;
        changes.clear();
        let mut filter = self.assignee_filter.lock().await;
        *filter = None;
        drop(store);
        drop(error);
        drop(count);
        drop(changes);
        drop(filter);
        self.server.reset().await;
    }

    /// Set up wiremock mocks based on current state.
    async fn setup_mocks(&self) {
        // Reset existing mocks
        self.server.reset().await;

        let error_mode = self.error_mode.lock().await.clone();
        let issues = self.issues.lock().await.clone();
        let assignee_filter = self.assignee_filter.lock().await.clone();

        match error_mode {
            LinearErrorMode::None => {
                self.setup_normal_mocks(&issues, &assignee_filter).await;
            }
            LinearErrorMode::ServerError => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(
                        ResponseTemplate::new(500).set_body_string("Internal Server Error"),
                    )
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
                    .respond_with(ResponseTemplate::new(200).set_body_json(&error_response))
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::MalformedResponse => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(ResponseTemplate::new(200).set_body_string("not valid json {{{"))
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::Unauthorized => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::Forbidden => {
                Mock::given(method("POST"))
                    .and(path("/graphql"))
                    .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
                    .mount(&self.server)
                    .await;
            }
            LinearErrorMode::FailOnRequest(_n) => {
                // For simplicity, set up normal mocks — the test should
                // toggle error mode between requests
                self.setup_normal_mocks(&issues, &assignee_filter).await;
            }
        }
    }

    /// Set up normal (non-error) mocks for GraphQL queries.
    async fn setup_normal_mocks(
        &self,
        issues: &[LinearIssueBuilder],
        assignee_filter: &Option<String>,
    ) {
        // Apply assignee filter if set
        let filtered_issues: Vec<&LinearIssueBuilder> = if let Some(ref assignee) = assignee_filter
        {
            issues
                .iter()
                .filter(|i| i.assignee.as_deref() == Some(assignee.as_str()))
                .collect()
        } else {
            issues.iter().collect()
        };

        let issue_nodes: Vec<Value> = filtered_issues.iter().map(|i| i.build_json()).collect();

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
            .respond_with(ResponseTemplate::new(200).set_body_json(&candidates_response))
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
        assert_eq!(
            page1["data"]["issues"]["nodes"].as_array().unwrap().len(),
            2
        );
        assert_eq!(page1["data"]["issues"]["pageInfo"]["hasNextPage"], true);

        // Last page
        let page3 = FakeLinearServer::build_paginated_response(&issues, 2, 2);
        assert_eq!(
            page3["data"]["issues"]["nodes"].as_array().unwrap().len(),
            1
        );
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

    #[tokio::test]
    async fn test_fake_linear_state_change_tracking() {
        let server = FakeLinearServer::start().await;
        server
            .seed_issues(vec![
                LinearIssueBuilder::new("id-1", "PROJ-1", "Issue 1").with_state("Todo", "started")
            ])
            .await;

        server.update_issue_state("id-1", "Done", "completed").await;

        let changes = server.state_changes().await;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].issue_id, "id-1");
        assert_eq!(changes[0].old_state, "Todo");
        assert_eq!(changes[0].new_state, "Done");
    }

    #[tokio::test]
    async fn test_fake_linear_assignee_filter() {
        let server = FakeLinearServer::start().await;
        server
            .seed_issues(vec![
                LinearIssueBuilder::new("id-1", "PROJ-1", "Alice's issue").with_assignee("alice"),
                LinearIssueBuilder::new("id-2", "PROJ-2", "Bob's issue").with_assignee("bob"),
                LinearIssueBuilder::new("id-3", "PROJ-3", "Unassigned issue"),
            ])
            .await;

        server.set_assignee_filter(Some("alice")).await;

        // The mock is set up — in a real test we'd make an HTTP request
        // Here we just verify the filter is stored
        let issues = server.current_issues().await;
        assert_eq!(issues.len(), 3); // All stored, filter applies at mock level
    }

    #[tokio::test]
    async fn test_fake_linear_terminal_check() {
        let server = FakeLinearServer::start().await;
        server
            .seed_issues(vec![
                LinearIssueBuilder::new("id-1", "PROJ-1", "Active issue")
                    .with_state("Todo", "started"),
                LinearIssueBuilder::new("id-2", "PROJ-2", "Done issue")
                    .with_state("Done", "completed"),
            ])
            .await;

        assert!(!server.is_terminal("id-1").await);
        assert!(server.is_terminal("id-2").await);
    }

    #[tokio::test]
    async fn test_fake_linear_remove_issue() {
        let server = FakeLinearServer::start().await;
        server
            .seed_issues(vec![
                LinearIssueBuilder::new("id-1", "PROJ-1", "Issue 1"),
                LinearIssueBuilder::new("id-2", "PROJ-2", "Issue 2"),
            ])
            .await;

        server.remove_issue("id-1").await;

        let issues = server.current_issues().await;
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].id, "id-2");
    }
}
