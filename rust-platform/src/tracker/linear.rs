//! Linear GraphQL tracker client implementation.
//!
//! Implements the `Tracker` trait for Linear's GraphQL API, handling pagination,
//! normalization, and error mapping per SPEC Section 11.

use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

use super::{BlockerRef, Tracker, TrackerError, TrackerIssue};

/// Default page size for paginated Linear queries.
const DEFAULT_PAGE_SIZE: usize = 50;

/// Default network timeout in milliseconds.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Linear GraphQL tracker client.
pub struct LinearClient {
    endpoint: String,
    api_key: String,
    project_slug: String,
    http: Client,
    page_size: usize,
    timeout: Duration,
    /// Active states to filter on when fetching candidates (server-side filtering).
    active_states: Vec<String>,
}

impl LinearClient {
    /// Create a new LinearClient with the given configuration.
    pub fn new(
        endpoint: String,
        api_key: String,
        project_slug: String,
    ) -> Result<Self, TrackerError> {
        if api_key.is_empty() {
            return Err(TrackerError::MissingApiKey);
        }
        if project_slug.is_empty() {
            return Err(TrackerError::MissingProjectSlug);
        }

        let http = Client::builder()
            .timeout(Duration::from_millis(DEFAULT_TIMEOUT_MS))
            .build()
            .map_err(|e| TrackerError::ApiRequest { source: e })?;

        Ok(Self {
            endpoint,
            api_key,
            project_slug,
            http,
            page_size: DEFAULT_PAGE_SIZE,
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
            active_states: Vec::new(),
        })
    }

    /// Override the page size (for testing).
    pub fn with_page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size;
        self
    }

    /// Override the timeout (for testing).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the active states for server-side filtering in fetch_candidate_issues.
    pub fn with_active_states(mut self, states: Vec<String>) -> Self {
        self.active_states = states;
        self
    }

    /// Execute a GraphQL query against the Linear API.
    async fn execute_graphql(&self, query: &str, variables: Value) -> Result<Value, TrackerError> {
        let body = json!({
            "query": query,
            "variables": variables,
        });

        let response = self
            .http
            .post(&self.endpoint)
            .header("Authorization", &self.api_key)
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| TrackerError::ApiRequest { source: e })?;

        let status = response.status().as_u16();
        if status != 200 {
            let body_text = response.text().await.unwrap_or_default();
            return Err(TrackerError::ApiStatus {
                status,
                body: body_text,
            });
        }

        let json_response: Value = response
            .json()
            .await
            .map_err(|e| TrackerError::ApiRequest { source: e })?;

        // Check for GraphQL-level errors
        if let Some(errors) = json_response.get("errors") {
            if let Some(arr) = errors.as_array() {
                if !arr.is_empty() {
                    return Err(TrackerError::GraphqlErrors {
                        errors: arr.clone(),
                    });
                }
            }
        }

        Ok(json_response)
    }

    /// Fetch candidate issues with pagination using project slugId filter.
    async fn fetch_issues_paginated(
        &self,
        query: &str,
        variables: Value,
        data_path: &[&str],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        let mut all_issues = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut vars = variables.clone();
            if let Some(ref c) = cursor {
                vars.as_object_mut()
                    .unwrap()
                    .insert("after".to_string(), json!(c));
            }
            vars.as_object_mut()
                .unwrap()
                .insert("first".to_string(), json!(self.page_size));

            let response = self.execute_graphql(query, vars).await?;

            // Navigate to the data path
            let mut data = response
                .get("data")
                .ok_or_else(|| TrackerError::UnknownPayload {
                    detail: "missing 'data' field in response".into(),
                })?;

            for key in data_path {
                data = data.get(*key).ok_or_else(|| TrackerError::UnknownPayload {
                    detail: format!("missing '{}' in response data path", key),
                })?;
            }

            // Extract nodes
            let nodes = data
                .get("nodes")
                .and_then(|n| n.as_array())
                .ok_or_else(|| TrackerError::UnknownPayload {
                    detail: "missing 'nodes' array".into(),
                })?;

            for node in nodes {
                if let Some(issue) = normalize_linear_issue(node) {
                    all_issues.push(issue);
                }
            }

            // Check pagination
            let page_info = data
                .get("pageInfo")
                .ok_or_else(|| TrackerError::UnknownPayload {
                    detail: "missing 'pageInfo'".into(),
                })?;

            let has_next = page_info
                .get("hasNextPage")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if !has_next {
                break;
            }

            cursor = page_info
                .get("endCursor")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            if cursor.is_none() {
                return Err(TrackerError::MissingEndCursor);
            }
        }

        Ok(all_issues)
    }
}

#[async_trait]
impl Tracker for LinearClient {
    async fn fetch_candidate_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        // If active_states are configured, use server-side state filtering
        if !self.active_states.is_empty() {
            let query = r#"
                query CandidateIssues($projectSlug: String!, $states: [String!]!, $first: Int!, $after: String) {
                    issues(
                        filter: {
                            project: { slugId: { eq: $projectSlug } }
                            state: { name: { in: $states } }
                        }
                        first: $first
                        after: $after
                    ) {
                        nodes {
                            id
                            identifier
                            title
                            description
                            priority
                            state { name }
                            branchName
                            url
                            labels { nodes { name } }
                            relations { nodes { type relatedIssue { id identifier state { name } } } }
                            inverseRelations { nodes { type issue { id identifier state { name } } } }
                            createdAt
                            updatedAt
                        }
                        pageInfo {
                            hasNextPage
                            endCursor
                        }
                    }
                }
            "#;

            let variables = json!({
                "projectSlug": self.project_slug,
                "states": self.active_states,
            });

            return self
                .fetch_issues_paginated(query, variables, &["issues"])
                .await;
        }

        // Fallback: fetch all issues in the project (no state filter)
        let query = r#"
            query CandidateIssues($projectSlug: String!, $first: Int!, $after: String) {
                issues(
                    filter: {
                        project: { slugId: { eq: $projectSlug } }
                    }
                    first: $first
                    after: $after
                ) {
                    nodes {
                        id
                        identifier
                        title
                        description
                        priority
                        state { name }
                        branchName
                        url
                        labels { nodes { name } }
                        relations { nodes { type relatedIssue { id identifier state { name } } } }
                        inverseRelations { nodes { type issue { id identifier state { name } } } }
                        createdAt
                        updatedAt
                    }
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                }
            }
        "#;

        let variables = json!({
            "projectSlug": self.project_slug,
        });

        self.fetch_issues_paginated(query, variables, &["issues"])
            .await
    }

    async fn fetch_issues_by_states(
        &self,
        states: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        let query = r#"
            query IssuesByStates($projectSlug: String!, $states: [String!]!, $first: Int!, $after: String) {
                issues(
                    filter: {
                        project: { slugId: { eq: $projectSlug } }
                        state: { name: { in: $states } }
                    }
                    first: $first
                    after: $after
                ) {
                    nodes {
                        id
                        identifier
                        title
                        description
                        priority
                        state { name }
                        branchName
                        url
                        labels { nodes { name } }
                        relations { nodes { type relatedIssue { id identifier state { name } } } }
                        inverseRelations { nodes { type issue { id identifier state { name } } } }
                        createdAt
                        updatedAt
                    }
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                }
            }
        "#;

        let variables = json!({
            "projectSlug": self.project_slug,
            "states": states,
        });

        self.fetch_issues_paginated(query, variables, &["issues"])
            .await
    }

    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        // Linear supports fetching by ID list
        let query = r#"
            query IssuesByIds($ids: [ID!]!, $first: Int!, $after: String) {
                issues(
                    filter: {
                        id: { in: $ids }
                    }
                    first: $first
                    after: $after
                ) {
                    nodes {
                        id
                        identifier
                        title
                        description
                        priority
                        state { name }
                        branchName
                        url
                        labels { nodes { name } }
                        relations { nodes { type relatedIssue { id identifier state { name } } } }
                        inverseRelations { nodes { type issue { id identifier state { name } } } }
                        createdAt
                        updatedAt
                    }
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                }
            }
        "#;

        let variables = json!({
            "ids": ids,
        });

        self.fetch_issues_paginated(query, variables, &["issues"])
            .await
    }
}

/// Normalize a raw Linear issue JSON node into a `TrackerIssue`.
///
/// Applies SPEC Section 11.3 normalization rules:
/// - labels -> lowercase
/// - blocked_by -> from inverse relations where type == "blocks"
/// - priority -> integer only (non-integers become None)
/// - timestamps -> ISO-8601 parse
fn normalize_linear_issue(node: &Value) -> Option<TrackerIssue> {
    let id = node.get("id")?.as_str()?.to_string();
    let identifier = node.get("identifier")?.as_str()?.to_string();
    let title = node.get("title")?.as_str()?.to_string();

    let description = node
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Priority: integer only
    let priority = node.get("priority").and_then(|v| {
        if let Some(n) = v.as_i64() {
            Some(n as i32)
        } else {
            None
        }
    });

    let state = node
        .get("state")
        .and_then(|s| s.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("Unknown")
        .to_string();

    let branch_name = node
        .get("branchName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let url = node
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Labels: normalize to lowercase
    let labels = node
        .get("labels")
        .and_then(|l| l.get("nodes"))
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|label| label.get("name").and_then(|n| n.as_str()))
                .map(|s| s.to_lowercase())
                .collect()
        })
        .unwrap_or_default();

    // blocked_by: from inverse relations where type == "blocks"
    let blocked_by = extract_blockers(node);

    let created_at = node
        .get("createdAt")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    let updated_at = node
        .get("updatedAt")
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    Some(TrackerIssue {
        id,
        identifier,
        title,
        description,
        priority,
        state,
        branch_name,
        url,
        labels,
        blocked_by,
        created_at,
        updated_at,
    })
}

/// Extract blocker references from inverse relations where type == "blocks".
///
/// In Linear's data model, if issue A blocks issue B, then B's inverseRelations
/// will contain a relation with type "blocks" pointing to A.
fn extract_blockers(node: &Value) -> Vec<BlockerRef> {
    let mut blockers = Vec::new();

    // Check inverseRelations (issues that block this one)
    if let Some(inverse) = node
        .get("inverseRelations")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
    {
        for rel in inverse {
            let rel_type = rel.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if rel_type == "blocks" {
                if let Some(issue) = rel.get("issue") {
                    blockers.push(BlockerRef {
                        id: issue
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        identifier: issue
                            .get("identifier")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        state: issue
                            .get("state")
                            .and_then(|s| s.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string()),
                    });
                }
            }
        }
    }

    // Also check relations where relatedIssue blocks this one
    if let Some(relations) = node
        .get("relations")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
    {
        for rel in relations {
            let rel_type = rel.get("type").and_then(|t| t.as_str()).unwrap_or("");
            // "is_blocked_by" type means the relatedIssue blocks this issue
            if rel_type == "is_blocked_by" {
                if let Some(issue) = rel.get("relatedIssue") {
                    blockers.push(BlockerRef {
                        id: issue
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        identifier: issue
                            .get("identifier")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        state: issue
                            .get("state")
                            .and_then(|s| s.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string()),
                    });
                }
            }
        }
    }

    blockers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_linear_issue_basic() {
        let node = json!({
            "id": "uuid-123",
            "identifier": "PROJ-42",
            "title": "Fix the bug",
            "description": "Something is broken",
            "priority": 2,
            "state": { "name": "In Progress" },
            "branchName": "fix/proj-42",
            "url": "https://linear.app/team/issue/PROJ-42",
            "labels": { "nodes": [{ "name": "Bug" }, { "name": "URGENT" }] },
            "relations": { "nodes": [] },
            "inverseRelations": { "nodes": [] },
            "createdAt": "2024-01-15T10:00:00Z",
            "updatedAt": "2024-01-16T12:00:00Z"
        });

        let issue = normalize_linear_issue(&node).unwrap();
        assert_eq!(issue.id, "uuid-123");
        assert_eq!(issue.identifier, "PROJ-42");
        assert_eq!(issue.title, "Fix the bug");
        assert_eq!(issue.priority, Some(2));
        assert_eq!(issue.state, "In Progress");
        assert_eq!(issue.labels, vec!["bug", "urgent"]); // lowercase
        assert!(issue.blocked_by.is_empty());
    }

    #[test]
    fn test_normalize_linear_issue_with_blockers() {
        let node = json!({
            "id": "uuid-456",
            "identifier": "PROJ-43",
            "title": "Blocked task",
            "priority": null,
            "state": { "name": "Todo" },
            "labels": { "nodes": [] },
            "relations": { "nodes": [] },
            "inverseRelations": {
                "nodes": [{
                    "type": "blocks",
                    "issue": {
                        "id": "uuid-789",
                        "identifier": "PROJ-40",
                        "state": { "name": "In Progress" }
                    }
                }]
            },
            "createdAt": "2024-01-10T08:00:00Z",
            "updatedAt": null
        });

        let issue = normalize_linear_issue(&node).unwrap();
        assert_eq!(issue.priority, None);
        assert_eq!(issue.blocked_by.len(), 1);
        assert_eq!(issue.blocked_by[0].id.as_deref(), Some("uuid-789"));
        assert_eq!(issue.blocked_by[0].identifier.as_deref(), Some("PROJ-40"));
        assert_eq!(issue.blocked_by[0].state.as_deref(), Some("In Progress"));
    }

    #[test]
    fn test_normalize_linear_issue_missing_required_fields() {
        // Missing id
        let node = json!({
            "identifier": "PROJ-1",
            "title": "No ID"
        });
        assert!(normalize_linear_issue(&node).is_none());

        // Missing identifier
        let node = json!({
            "id": "uuid-1",
            "title": "No identifier"
        });
        assert!(normalize_linear_issue(&node).is_none());
    }

    #[test]
    fn test_normalize_priority_non_integer() {
        let node = json!({
            "id": "uuid-1",
            "identifier": "PROJ-1",
            "title": "Test",
            "priority": "high",
            "state": { "name": "Todo" },
            "labels": { "nodes": [] },
            "relations": { "nodes": [] },
            "inverseRelations": { "nodes": [] },
            "createdAt": null,
            "updatedAt": null
        });

        let issue = normalize_linear_issue(&node).unwrap();
        assert_eq!(issue.priority, None); // non-integer becomes None
    }
}
