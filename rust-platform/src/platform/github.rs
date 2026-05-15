//! GitHub platform adapter implementation.
//!
//! Implements the `Platform` trait for GitHub's REST API, providing:
//! - Issue fetching with label-based filtering and auto-pagination
//! - Label-based workflow state management (add-then-remove strategy)
//! - Comment CRUD and workpad discovery
//! - Pull request creation
//! - Credential validation
//!
//! All HTTP calls go through the shared `HttpClient` which handles
//! authentication, pagination, rate-limit awareness, and timeouts.

use async_trait::async_trait;
use serde::Deserialize;

use crate::config::PlatformConfig;
use crate::error::PlatformError;
use crate::platform::http_client::HttpClient;
use crate::platform::{
    Capability, Comment, CommentId, CreatePrParams, FetchOptions, Issue, IssueId, Platform,
    PullRequest,
};

// ---------------------------------------------------------------------------
// GitHub API response types (serde deserialization)
// ---------------------------------------------------------------------------

/// GitHub user object (subset of fields we need).
#[derive(Debug, Deserialize)]
struct GhUser {
    login: String,
}

/// GitHub label object as returned by the API.
#[derive(Debug, Deserialize)]
struct GhLabel {
    name: String,
}

/// GitHub issue object as returned by the list/get endpoints.
#[derive(Debug, Deserialize)]
struct GhIssue {
    #[allow(dead_code)]
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    html_url: String,
    assignee: Option<GhUser>,
    labels: Vec<GhLabel>,
    created_at: String,
    updated_at: String,
    /// GitHub returns PRs in the issues endpoint; we filter them out.
    pull_request: Option<serde_json::Value>,
}

/// GitHub comment object.
#[derive(Debug, Deserialize)]
struct GhComment {
    id: u64,
    body: Option<String>,
    user: Option<GhUser>,
    created_at: String,
}

/// GitHub pull request object (response from POST /repos/:owner/:repo/pulls).
#[derive(Debug, Deserialize)]
struct GhPullRequest {
    id: u64,
    number: u64,
    html_url: String,
    state: String,
}

// ---------------------------------------------------------------------------
// GithubAdapter
// ---------------------------------------------------------------------------

/// GitHub platform adapter implementing the `Platform` trait.
///
/// Uses the GitHub REST API v3 (application/vnd.github+json) for all operations.
/// Authentication is via Bearer token (PAT or GitHub App installation token).
pub struct GithubAdapter {
    http: HttpClient,
    config: PlatformConfig,
}

impl GithubAdapter {
    /// Creates a new GitHub adapter from the given platform configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed (e.g., invalid token).
    pub fn new(config: PlatformConfig) -> Result<Self, PlatformError> {
        let http = HttpClient::new(config.clone())?;
        Ok(Self { http, config })
    }

    /// Constructs the repo path prefix: `/repos/:owner/:repo`
    fn repo_path(&self) -> String {
        format!("/repos/{}/{}", self.config.owner, self.config.repo)
    }

    /// Converts a GitHub issue API response into our standardized `Issue` type.
    fn convert_issue(&self, gh: GhIssue) -> Issue {
        let workflow_state = self.extract_workflow_state(&gh.labels);
        let labels: Vec<String> = gh.labels.iter().map(|l| l.name.clone()).collect();
        let branch_name = format!("symphony/issue-{}", gh.number);

        Issue {
            id: IssueId(gh.number),
            number: gh.number,
            title: gh.title,
            description: gh.body,
            url: gh.html_url,
            assignee: gh.assignee.map(|u| u.login),
            workflow_state,
            branch_name,
            priority: None,
            labels,
            blocked_by: Vec::new(),
            created_at: chrono::DateTime::parse_from_rfc3339(&gh.created_at)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            updated_at: chrono::DateTime::parse_from_rfc3339(&gh.updated_at)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)),
        }
    }

    /// Extracts the workflow state from issue labels.
    ///
    /// Looks for labels that match any value in the workflow states config.
    /// Returns the internal state key (e.g., "todo") rather than the label name.
    fn extract_workflow_state(&self, labels: &[GhLabel]) -> Option<String> {
        for label in labels {
            for (key, value) in &self.config.workflow.states {
                if label.name == *value {
                    return Some(key.clone());
                }
            }
        }
        None
    }

    /// Returns all workflow label values from config (e.g., "workflow::todo").
    fn all_workflow_labels(&self) -> Vec<String> {
        self.config.workflow.states.values().cloned().collect()
    }
}

#[async_trait]
impl Platform for GithubAdapter {
    // --- Capability discovery ---

    fn capabilities(&self) -> Vec<Capability> {
        // GitHub does not support atomic label operations (add+remove in one call)
        Vec::new()
    }

    // --- Issue operations ---

    /// Fetches candidate issues matching the configured active workflow state labels.
    ///
    /// GitHub API: `GET /repos/:owner/:repo/issues?labels=...&state=open`
    ///
    /// Uses auto-pagination via `get_all_pages`. Filters out pull requests
    /// (GitHub returns PRs in the issues endpoint).
    async fn fetch_candidate_issues(
        &self,
        _opts: FetchOptions,
    ) -> Result<Vec<Issue>, PlatformError> {
        let path = format!("{}/issues", self.repo_path());

        // Build the labels filter from active_states config
        let filter_labels: Vec<String> = self
            .config
            .workflow
            .active_states
            .iter()
            .filter_map(|state_key| self.config.workflow.states.get(state_key))
            .cloned()
            .collect();

        let labels_param = filter_labels.join(",");
        let params: Vec<(&str, &str)> = vec![("labels", &labels_param), ("state", "open")];

        let gh_issues: Vec<GhIssue> = self.http.get_all_pages(&path, &params).await?;

        // Filter out pull requests (GitHub includes them in the issues endpoint)
        let issues: Vec<Issue> = gh_issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(|i| self.convert_issue(i))
            .collect();

        tracing::debug!(count = issues.len(), "Fetched candidate issues from GitHub");
        Ok(issues)
    }

    /// Fetches a single issue by its number (used as IssueId for GitHub).
    ///
    /// GitHub API: `GET /repos/:owner/:repo/issues/:number`
    async fn fetch_issue(&self, issue_id: IssueId) -> Result<Issue, PlatformError> {
        let path = format!("{}/issues/{}", self.repo_path(), issue_id.0);
        let url = format!("{}{}", self.http.base_url(), path);

        let response = self.http.inner().get(&url).send().await.map_err(|e| {
            if e.is_timeout() {
                PlatformError::Timeout
            } else if e.is_connect() {
                PlatformError::ConnectionRefused
            } else {
                PlatformError::Network(e)
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &body));
        }

        let gh_issue: GhIssue = response.json().await?;
        Ok(self.convert_issue(gh_issue))
    }

    /// Fetches multiple issues by ID (batch call to fetch_issue).
    ///
    /// GitHub does not have a batch endpoint, so this calls fetch_issue for each ID.
    /// Partial failures are logged — successfully fetched issues are returned.
    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[IssueId],
    ) -> Result<Vec<Issue>, PlatformError> {
        let mut results = Vec::with_capacity(ids.len());
        for &id in ids {
            match self.fetch_issue(id).await {
                Ok(issue) => results.push(issue),
                Err(e) => {
                    tracing::warn!(
                        issue_id = %id,
                        error = %e,
                        "Failed to fetch issue state, skipping"
                    );
                }
            }
        }
        Ok(results)
    }

    // --- Workflow state (label-based) ---

    /// Gets the current workflow state for an issue by inspecting its labels.
    ///
    /// Returns the internal state key (e.g., "todo") if a matching workflow label
    /// is found, or None if the issue has no workflow label.
    async fn get_workflow_state(
        &self,
        issue_id: IssueId,
    ) -> Result<Option<String>, PlatformError> {
        let issue = self.fetch_issue(issue_id).await?;
        Ok(issue.workflow_state)
    }

    /// Sets the workflow state using the "add-then-remove" strategy.
    ///
    /// 1. Add the target workflow label
    /// 2. Remove all other workflow labels
    ///
    /// This ensures the issue never enters a "ghost" state (no workflow label).
    /// If removal of old labels partially fails, a warning is logged but the
    /// operation is considered successful (the target label is already applied).
    async fn set_workflow_state(
        &self,
        issue_id: IssueId,
        state: &str,
    ) -> Result<(), PlatformError> {
        // Resolve the target label name from the state key
        let target_label = self
            .config
            .workflow
            .states
            .get(state)
            .cloned()
            .ok_or_else(|| PlatformError::MissingState(state.to_string()))?;

        // Get current workflow labels on the issue
        let issue = self.fetch_issue(issue_id).await?;
        let all_workflow_labels = self.all_workflow_labels();
        let current_workflow_labels: Vec<String> = issue
            .labels
            .iter()
            .filter(|l| all_workflow_labels.contains(l))
            .cloned()
            .collect();

        // Step 1: Add the target label (ensures issue always has at least one workflow label)
        self.add_labels(issue_id, &[target_label.clone()]).await?;

        // Step 2: Remove stale workflow labels
        let stale: Vec<String> = current_workflow_labels
            .into_iter()
            .filter(|l| l != &target_label)
            .collect();

        if !stale.is_empty() {
            if let Err(e) = self.remove_labels(issue_id, &stale).await {
                tracing::warn!(
                    issue_id = %issue_id,
                    error = %e,
                    "Failed to remove stale workflow labels (target label already applied)"
                );
            }
        }

        Ok(())
    }

    // --- Label operations ---

    /// Adds labels to an issue.
    ///
    /// GitHub API: `POST /repos/:owner/:repo/issues/:number/labels`
    /// Body: `{"labels": ["label1", "label2"]}`
    ///
    /// No-op if the labels slice is empty.
    async fn add_labels(
        &self,
        issue_id: IssueId,
        labels: &[String],
    ) -> Result<(), PlatformError> {
        if labels.is_empty() {
            return Ok(());
        }

        let path = format!("{}/issues/{}/labels", self.repo_path(), issue_id.0);
        let url = format!("{}{}", self.http.base_url(), path);
        let body = serde_json::json!({ "labels": labels });

        let response = self.http.inner().post(&url).json(&body).send().await.map_err(|e| {
            if e.is_timeout() {
                PlatformError::Timeout
            } else {
                PlatformError::Network(e)
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &body_text));
        }

        tracing::debug!(
            issue_id = %issue_id,
            labels = ?labels,
            "Labels added successfully"
        );
        Ok(())
    }

    /// Removes labels from an issue one at a time.
    ///
    /// GitHub API: `DELETE /repos/:owner/:repo/issues/:number/labels/:name`
    ///
    /// Each label is removed individually. If some removals fail, returns a
    /// `PartialLabelUpdate` error listing which labels were successfully removed
    /// and which failed.
    ///
    /// No-op if the labels slice is empty.
    async fn remove_labels(
        &self,
        issue_id: IssueId,
        labels: &[String],
    ) -> Result<(), PlatformError> {
        if labels.is_empty() {
            return Ok(());
        }

        let mut removed = Vec::new();
        let mut failed = Vec::new();

        for label in labels {
            // URL-encode the label name to handle special characters (e.g., "::")
            let encoded_label = urlencoding::encode(label);
            let path = format!(
                "{}/issues/{}/labels/{}",
                self.repo_path(),
                issue_id.0,
                encoded_label
            );
            let url = format!("{}{}", self.http.base_url(), path);

            let response = self.http.inner().delete(&url).send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() || status.as_u16() == 404 {
                        // 404 means label wasn't on the issue — treat as success
                        removed.push(label.clone());
                    } else {
                        tracing::warn!(
                            issue_id = %issue_id,
                            label = %label,
                            status = status.as_u16(),
                            "Failed to remove label"
                        );
                        failed.push(label.clone());
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        issue_id = %issue_id,
                        label = %label,
                        error = %e,
                        "Network error removing label"
                    );
                    failed.push(label.clone());
                }
            }
        }

        if failed.is_empty() {
            Ok(())
        } else {
            Err(PlatformError::PartialLabelUpdate {
                added: removed,
                failed,
            })
        }
    }

    // --- Comment / Workpad ---

    /// Creates a comment on an issue.
    ///
    /// GitHub API: `POST /repos/:owner/:repo/issues/:number/comments`
    /// Body: `{"body": "comment text"}`
    async fn create_comment(
        &self,
        issue_id: IssueId,
        body: &str,
    ) -> Result<CommentId, PlatformError> {
        let path = format!("{}/issues/{}/comments", self.repo_path(), issue_id.0);
        let url = format!("{}{}", self.http.base_url(), path);
        let request_body = serde_json::json!({ "body": body });

        let response = self
            .http
            .inner()
            .post(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    PlatformError::Timeout
                } else {
                    PlatformError::Network(e)
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &body_text));
        }

        let comment: GhComment = response.json().await?;
        Ok(CommentId(comment.id))
    }

    /// Updates an existing comment.
    ///
    /// GitHub API: `PATCH /repos/:owner/:repo/issues/comments/:id`
    /// Body: `{"body": "updated text"}`
    async fn update_comment(
        &self,
        comment_id: CommentId,
        body: &str,
    ) -> Result<(), PlatformError> {
        let path = format!("{}/issues/comments/{}", self.repo_path(), comment_id.0);
        let url = format!("{}{}", self.http.base_url(), path);
        let request_body = serde_json::json!({ "body": body });

        let response = self
            .http
            .inner()
            .patch(&url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    PlatformError::Timeout
                } else {
                    PlatformError::Network(e)
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &body_text));
        }

        Ok(())
    }

    /// Finds the workpad comment on an issue.
    ///
    /// Lists all comments and returns the first one containing "## Codex Workpad".
    /// Returns both the comment ID and its full body text.
    async fn find_workpad_comment(
        &self,
        issue_id: IssueId,
    ) -> Result<Option<(CommentId, String)>, PlatformError> {
        let comments = self.list_comments(issue_id).await?;

        for comment in comments {
            if comment.body.contains("## Codex Workpad") {
                return Ok(Some((comment.id, comment.body)));
            }
        }

        Ok(None)
    }

    /// Lists all comments on an issue.
    ///
    /// GitHub API: `GET /repos/:owner/:repo/issues/:number/comments`
    /// Uses auto-pagination to fetch all comments.
    async fn list_comments(&self, issue_id: IssueId) -> Result<Vec<Comment>, PlatformError> {
        let path = format!("{}/issues/{}/comments", self.repo_path(), issue_id.0);
        let gh_comments: Vec<GhComment> = self.http.get_all_pages(&path, &[]).await?;

        let comments: Vec<Comment> = gh_comments
            .into_iter()
            .map(|c| Comment {
                id: CommentId(c.id),
                body: c.body.unwrap_or_default(),
                author: c
                    .user
                    .map(|u| u.login)
                    .unwrap_or_else(|| "unknown".to_string()),
                created_at: chrono::DateTime::parse_from_rfc3339(&c.created_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                is_system: false, // GitHub doesn't have system comments
            })
            .collect();

        Ok(comments)
    }

    // --- PR/MR ---

    /// Creates a pull request.
    ///
    /// GitHub API: `POST /repos/:owner/:repo/pulls`
    /// Body: `{"title": "...", "body": "...", "head": "...", "base": "...", "draft": bool}`
    async fn create_pull_request(
        &self,
        params: CreatePrParams,
    ) -> Result<PullRequest, PlatformError> {
        let path = format!("{}/pulls", self.repo_path());
        let url = format!("{}{}", self.http.base_url(), path);
        let body = serde_json::json!({
            "title": params.title,
            "body": params.body,
            "head": params.head,
            "base": params.base,
            "draft": params.draft,
        });

        let response = self.http.inner().post(&url).json(&body).send().await.map_err(|e| {
            if e.is_timeout() {
                PlatformError::Timeout
            } else {
                PlatformError::Network(e)
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &body_text));
        }

        let pr: GhPullRequest = response.json().await?;
        Ok(PullRequest {
            id: pr.id,
            number: pr.number,
            url: pr.html_url,
            state: pr.state,
        })
    }

    // --- Health check ---

    /// Validates credentials by calling `GET /user`.
    ///
    /// Logs the authenticated user's login on success.
    async fn validate_credentials(&self) -> Result<(), PlatformError> {
        let url = format!("{}/user", self.http.base_url());

        let response = self.http.inner().get(&url).send().await.map_err(|e| {
            if e.is_timeout() {
                PlatformError::Timeout
            } else if e.is_connect() {
                PlatformError::ConnectionRefused
            } else {
                PlatformError::Network(e)
            }
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &body));
        }

        let user: GhUser = response.json().await?;
        tracing::info!(
            user = %user.login,
            "GitHub credentials validated successfully"
        );

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{IssueFilter, WorkflowConfig};
    use std::collections::HashMap;

    /// Creates a test PlatformConfig pointing at the given base_url.
    /// Sets a test env var for the token.
    pub fn test_config(base_url: &str) -> PlatformConfig {
        std::env::set_var("SYMPHONY_TEST_GH_TOKEN", "test-token-value");

        let mut states = HashMap::new();
        states.insert("backlog".to_string(), "workflow::backlog".to_string());
        states.insert("todo".to_string(), "workflow::todo".to_string());
        states.insert(
            "in_progress".to_string(),
            "workflow::in-progress".to_string(),
        );
        states.insert("rework".to_string(), "workflow::rework".to_string());
        states.insert(
            "human_review".to_string(),
            "workflow::human-review".to_string(),
        );
        states.insert("merging".to_string(), "workflow::merging".to_string());
        states.insert("done".to_string(), "workflow::done".to_string());

        PlatformConfig {
            kind: "github".to_string(),
            api_token: "$SYMPHONY_TEST_GH_TOKEN".to_string(),
            base_url: base_url.to_string(),
            owner: "testorg".to_string(),
            repo: "testrepo".to_string(),
            project_id: None,
            allow_custom_host: true,
            issue_filter: IssueFilter {
                labels: vec![
                    "workflow::todo".to_string(),
                    "workflow::in-progress".to_string(),
                    "workflow::rework".to_string(),
                ],
                assignee: None,
                milestone: None,
            },
            workflow: WorkflowConfig {
                states,
                active_states: vec![
                    "todo".to_string(),
                    "in_progress".to_string(),
                    "rework".to_string(),
                ],
                terminal_states: vec!["done".to_string()],
            },
        }
    }

    #[test]
    fn test_extract_workflow_state_found() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();

        let labels = vec![
            GhLabel {
                name: "bug".to_string(),
            },
            GhLabel {
                name: "workflow::todo".to_string(),
            },
            GhLabel {
                name: "priority::high".to_string(),
            },
        ];

        let state = adapter.extract_workflow_state(&labels);
        assert_eq!(state, Some("todo".to_string()));
    }

    #[test]
    fn test_extract_workflow_state_in_progress() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();

        let labels = vec![GhLabel {
            name: "workflow::in-progress".to_string(),
        }];

        let state = adapter.extract_workflow_state(&labels);
        assert_eq!(state, Some("in_progress".to_string()));
    }

    #[test]
    fn test_extract_workflow_state_none() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();

        let labels = vec![
            GhLabel {
                name: "bug".to_string(),
            },
            GhLabel {
                name: "enhancement".to_string(),
            },
        ];

        let state = adapter.extract_workflow_state(&labels);
        assert_eq!(state, None);
    }

    #[test]
    fn test_convert_issue_basic() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();

        let gh_issue = GhIssue {
            id: 12345,
            number: 42,
            title: "Fix the bug".to_string(),
            body: Some("Description here".to_string()),
            html_url: "https://github.com/testorg/testrepo/issues/42".to_string(),
            assignee: Some(GhUser {
                login: "dev1".to_string(),
            }),
            labels: vec![
                GhLabel {
                    name: "workflow::in-progress".to_string(),
                },
                GhLabel {
                    name: "bug".to_string(),
                },
            ],
            created_at: "2024-01-15T10:00:00Z".to_string(),
            updated_at: "2024-01-16T12:00:00Z".to_string(),
            pull_request: None,
        };

        let issue = adapter.convert_issue(gh_issue);
        assert_eq!(issue.id, IssueId(42));
        assert_eq!(issue.number, 42);
        assert_eq!(issue.title, "Fix the bug");
        assert_eq!(issue.description, Some("Description here".to_string()));
        assert_eq!(issue.assignee, Some("dev1".to_string()));
        assert_eq!(issue.workflow_state, Some("in_progress".to_string()));
        assert_eq!(issue.labels.len(), 2);
        assert!(issue.labels.contains(&"workflow::in-progress".to_string()));
        assert!(issue.labels.contains(&"bug".to_string()));
        assert_eq!(issue.branch_name, "symphony/issue-42");
        assert!(issue.created_at.is_some());
        assert!(issue.updated_at.is_some());
        assert!(issue.blocked_by.is_empty());
    }

    #[test]
    fn test_convert_issue_no_assignee() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();

        let gh_issue = GhIssue {
            id: 99,
            number: 7,
            title: "No assignee".to_string(),
            body: None,
            html_url: "https://github.com/testorg/testrepo/issues/7".to_string(),
            assignee: None,
            labels: vec![],
            created_at: "2024-03-01T08:00:00Z".to_string(),
            updated_at: "2024-03-01T08:00:00Z".to_string(),
            pull_request: None,
        };

        let issue = adapter.convert_issue(gh_issue);
        assert_eq!(issue.assignee, None);
        assert_eq!(issue.description, None);
        assert_eq!(issue.workflow_state, None);
    }

    #[test]
    fn test_capabilities_empty() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();
        assert!(adapter.capabilities().is_empty());
    }

    #[test]
    fn test_all_workflow_labels() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();
        let labels = adapter.all_workflow_labels();
        assert!(labels.contains(&"workflow::todo".to_string()));
        assert!(labels.contains(&"workflow::in-progress".to_string()));
        assert!(labels.contains(&"workflow::rework".to_string()));
        assert!(labels.contains(&"workflow::human-review".to_string()));
        assert!(labels.contains(&"workflow::done".to_string()));
        assert!(labels.contains(&"workflow::backlog".to_string()));
        assert!(labels.contains(&"workflow::merging".to_string()));
        assert_eq!(labels.len(), 7);
    }

    #[test]
    fn test_repo_path() {
        let config = test_config("http://localhost:8080");
        let adapter = GithubAdapter::new(config).unwrap();
        assert_eq!(adapter.repo_path(), "/repos/testorg/testrepo");
    }
}
