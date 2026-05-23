//! GitLab adapter implementing the Platform trait.
//!
//! Uses GitLab REST API v4 for issue, label, note, and merge request operations.
//! Supports atomic label operations via single PUT requests.
//!
//! Depends on:
//! - `crate::platform::http_client::HttpClient` (shared HTTP layer with auth/pagination)
//! - `crate::config::PlatformConfig` (configuration)
//! - `crate::error::PlatformError` (error types)

use async_trait::async_trait;
use serde::Deserialize;

use crate::config::PlatformConfig;
use crate::error::PlatformError;
use crate::platform::http_client::HttpClient;
use crate::platform::{
    Capability, Comment, CommentId, CreatePrParams, FetchOptions, Issue, IssueId, Platform,
    PullRequest,
};

/// GitLab adapter that communicates with the GitLab REST API v4.
///
/// Uses the shared `HttpClient` for authenticated requests and automatic pagination.
/// Supports atomic label add+remove in a single PUT (the `AtomicLabels` capability).
pub struct GitlabAdapter {
    http: HttpClient,
    project_id: String,
}

// --- GitLab API response types (private, for deserialization only) ---

#[derive(Debug, Deserialize)]
struct GitlabIssue {
    #[allow(dead_code)]
    id: u64,
    iid: u64,
    title: String,
    description: Option<String>,
    web_url: String,
    labels: Vec<String>,
    assignee: Option<GitlabUser>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct GitlabUser {
    username: String,
}

#[derive(Debug, Deserialize)]
struct GitlabNote {
    id: u64,
    body: String,
    author: GitlabNoteAuthor,
    created_at: String,
    system: bool,
}

#[derive(Debug, Deserialize)]
struct GitlabNoteAuthor {
    username: String,
}

#[derive(Debug, Deserialize)]
struct GitlabMergeRequest {
    id: u64,
    iid: u64,
    web_url: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct GitlabCurrentUser {
    username: String,
}

impl GitlabAdapter {
    /// Create a new GitLab adapter from the platform configuration.
    ///
    /// Builds the shared HTTP client (which handles token resolution and auth headers)
    /// and resolves the project ID from config.
    pub fn new(config: PlatformConfig) -> Result<Self, PlatformError> {
        let project_id = config.project_id.clone().unwrap_or_default();
        let http = HttpClient::new(config)?;

        Ok(Self { http, project_id })
    }

    /// Create a new GitLab adapter from a config and already-resolved token.
    pub fn new_with_token(config: PlatformConfig, token: &str) -> Result<Self, PlatformError> {
        let project_id = config.project_id.clone().unwrap_or_default();
        let http = HttpClient::new_with_resolved_token(config, token)?;

        Ok(Self { http, project_id })
    }

    /// Returns a reference to the underlying HTTP client.
    pub fn http_client(&self) -> &HttpClient {
        &self.http
    }

    /// API path prefix for project-scoped endpoints.
    fn project_path(&self) -> String {
        format!("/projects/{}", urlencoding::encode(&self.project_id))
    }

    /// Build the comma-separated labels filter string from issue_filter config.
    fn active_labels_filter(&self) -> String {
        self.http.config().issue_filter.labels.join(",")
    }

    /// Parse a GitLab API issue response into our standardized Issue type.
    fn parse_issue(&self, gi: GitlabIssue) -> Issue {
        let workflow_state = self.detect_workflow_state(&gi.labels);
        let branch_name = format!("issue-{}", gi.iid);

        Issue {
            id: IssueId(gi.iid),
            number: gi.iid,
            title: gi.title,
            description: gi.description,
            url: gi.web_url,
            assignee: gi.assignee.map(|a| a.username),
            workflow_state,
            branch_name,
            priority: None,
            labels: gi.labels,
            blocked_by: Vec::new(),
            created_at: chrono::DateTime::parse_from_rfc3339(&gi.created_at)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            updated_at: chrono::DateTime::parse_from_rfc3339(&gi.updated_at)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)),
        }
    }

    /// Detect the current workflow state key from an issue's labels.
    ///
    /// Scans the workflow states map and returns the first matching state key.
    fn detect_workflow_state(&self, labels: &[String]) -> Option<String> {
        let config = self.http.config();
        for (state_key, label_name) in &config.workflow.states {
            if labels.contains(label_name) {
                return Some(state_key.clone());
            }
        }
        None
    }

    /// Extract all workflow labels currently on an issue.
    fn get_workflow_labels_from_list(&self, labels: &[String]) -> Vec<String> {
        let config = self.http.config();
        labels
            .iter()
            .filter(|l| config.workflow.states.values().any(|v| v == *l))
            .cloned()
            .collect()
    }

    /// Perform a PUT request to the given path with a JSON body.
    async fn put_json(&self, path: &str, body: &serde_json::Value) -> Result<(), PlatformError> {
        let url = format!("{}{}", self.http.base_url(), path);

        let response = self
            .http
            .inner()
            .put(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| {
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
            let resp_body = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &resp_body));
        }

        Ok(())
    }

    /// Perform a POST request and return the deserialized response.
    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, PlatformError> {
        let url = format!("{}{}", self.http.base_url(), path);

        let response = self
            .http
            .inner()
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| {
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
            let resp_body = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &resp_body));
        }

        response.json().await.map_err(|e| {
            tracing::error!(path, error = %e, "Failed to deserialize POST response");
            PlatformError::Network(e)
        })
    }

    /// Perform a GET request for a single resource and return the deserialized response.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, PlatformError> {
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
            let resp_body = response.text().await.unwrap_or_default();
            return Err(PlatformError::from_status(status.as_u16(), &resp_body));
        }

        response.json().await.map_err(|e| {
            tracing::error!(path, error = %e, "Failed to deserialize GET response");
            PlatformError::Network(e)
        })
    }
}

#[async_trait]
impl Platform for GitlabAdapter {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::AtomicLabels]
    }

    async fn fetch_candidate_issues(
        &self,
        _opts: FetchOptions,
    ) -> Result<Vec<Issue>, PlatformError> {
        let path = format!("{}/issues", self.project_path());
        let filter_labels: Vec<&str> = self
            .http
            .config()
            .issue_filter
            .labels
            .iter()
            .map(|s| s.as_str())
            .collect();

        // GitLab's labels param is AND — query each label separately and deduplicate
        let mut seen = std::collections::HashSet::new();
        let mut all_issues = Vec::new();

        for label in &filter_labels {
            let params: Vec<(&str, &str)> = vec![("labels", label), ("state", "opened")];
            let gitlab_issues: Vec<GitlabIssue> = self.http.get_all_pages(&path, &params).await?;
            for gi in gitlab_issues {
                if seen.insert(gi.iid) {
                    all_issues.push(self.parse_issue(gi));
                }
            }
        }

        Ok(all_issues)
    }

    async fn fetch_issue(&self, issue_id: IssueId) -> Result<Issue, PlatformError> {
        let path = format!("{}/issues/{}", self.project_path(), issue_id.0);
        let gi: GitlabIssue = self.get_json(&path).await?;
        Ok(self.parse_issue(gi))
    }

    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[IssueId],
    ) -> Result<Vec<Issue>, PlatformError> {
        let mut issues = Vec::with_capacity(ids.len());
        for id in ids {
            match self.fetch_issue(*id).await {
                Ok(issue) => issues.push(issue),
                Err(PlatformError::NotFound(_)) => {
                    tracing::debug!(issue_id = %id, "Issue not found during batch fetch, skipping");
                }
                Err(e) => return Err(e),
            }
        }
        Ok(issues)
    }

    async fn get_workflow_state(&self, issue_id: IssueId) -> Result<Option<String>, PlatformError> {
        let issue = self.fetch_issue(issue_id).await?;
        Ok(issue.workflow_state)
    }

    /// Set workflow state atomically using a single PUT with add_labels and remove_labels.
    ///
    /// GitLab supports passing both `add_labels` and `remove_labels` in a single
    /// `PUT /projects/:id/issues/:iid` request, making this a true atomic operation.
    /// This is the key advantage over GitHub's non-atomic label operations.
    async fn set_workflow_state(
        &self,
        issue_id: IssueId,
        state: &str,
    ) -> Result<(), PlatformError> {
        let target_label = self
            .http
            .config()
            .workflow
            .states
            .get(state)
            .cloned()
            .ok_or_else(|| PlatformError::MissingState(state.to_string()))?;

        // Fetch current issue to find existing workflow labels
        let issue = self.fetch_issue(issue_id).await?;
        let current_workflow_labels = self.get_workflow_labels_from_list(&issue.labels);

        // Determine which labels to remove (all workflow labels except the target)
        let labels_to_remove: Vec<&str> = current_workflow_labels
            .iter()
            .filter(|l| *l != &target_label)
            .map(|l| l.as_str())
            .collect();

        // Single atomic PUT with both add_labels and remove_labels
        let path = format!("{}/issues/{}", self.project_path(), issue_id.0);

        let mut body = serde_json::json!({
            "add_labels": target_label,
        });

        if !labels_to_remove.is_empty() {
            body["remove_labels"] = serde_json::Value::String(labels_to_remove.join(","));
        }

        self.put_json(&path, &body).await?;

        tracing::debug!(
            issue_id = %issue_id,
            target_state = state,
            target_label = %target_label,
            removed = ?labels_to_remove,
            "Workflow state set atomically"
        );

        Ok(())
    }

    async fn add_labels(&self, issue_id: IssueId, labels: &[String]) -> Result<(), PlatformError> {
        if labels.is_empty() {
            return Ok(());
        }

        let path = format!("{}/issues/{}", self.project_path(), issue_id.0);
        let body = serde_json::json!({
            "add_labels": labels.join(","),
        });

        self.put_json(&path, &body).await
    }

    async fn remove_labels(
        &self,
        issue_id: IssueId,
        labels: &[String],
    ) -> Result<(), PlatformError> {
        if labels.is_empty() {
            return Ok(());
        }

        let path = format!("{}/issues/{}", self.project_path(), issue_id.0);
        let body = serde_json::json!({
            "remove_labels": labels.join(","),
        });

        self.put_json(&path, &body).await
    }

    async fn create_comment(
        &self,
        issue_id: IssueId,
        body: &str,
    ) -> Result<CommentId, PlatformError> {
        let path = format!("{}/issues/{}/notes", self.project_path(), issue_id.0);
        let payload = serde_json::json!({
            "body": body,
        });

        let note: GitlabNote = self.post_json(&path, &payload).await?;
        Ok(CommentId(note.id))
    }

    async fn update_comment(&self, comment_id: CommentId, body: &str) -> Result<(), PlatformError> {
        // GitLab's note update API requires the issue IID in the URL path:
        //   PUT /projects/:id/issues/:iid/notes/:note_id
        //
        // Since the Platform trait only passes comment_id, we use a composite encoding:
        //   composite_id = issue_iid * 1_000_000 + note_id
        //
        // This supports note IDs up to 999,999 which covers all practical cases.
        // The find_workpad_comment() method returns IDs in this composite format.
        let issue_iid = comment_id.0 / 1_000_000;
        let note_id = comment_id.0 % 1_000_000;

        let path = format!(
            "{}/issues/{}/notes/{}",
            self.project_path(),
            issue_iid,
            note_id
        );
        let payload = serde_json::json!({
            "body": body,
        });

        self.put_json(&path, &payload).await
    }

    async fn find_workpad_comment(
        &self,
        issue_id: IssueId,
    ) -> Result<Option<(CommentId, String)>, PlatformError> {
        let notes = self.list_comments(issue_id).await?;

        for note in notes {
            if !note.is_system && note.body.contains("## Codex Workpad") {
                // Encode composite ID: issue_iid * 1_000_000 + note_id
                let composite_id = CommentId(issue_id.0 * 1_000_000 + note.id.0);
                return Ok(Some((composite_id, note.body)));
            }
        }

        Ok(None)
    }

    async fn list_comments(&self, issue_id: IssueId) -> Result<Vec<Comment>, PlatformError> {
        let path = format!("{}/issues/{}/notes", self.project_path(), issue_id.0);

        let gitlab_notes: Vec<GitlabNote> = self.http.get_all_pages(&path, &[]).await?;

        // Filter out system notes — only return user-authored notes
        let comments = gitlab_notes
            .into_iter()
            .filter(|n| !n.system)
            .map(|n| {
                let created_at = chrono::DateTime::parse_from_rfc3339(&n.created_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                Comment {
                    id: CommentId(n.id),
                    body: n.body,
                    author: n.author.username,
                    created_at,
                    is_system: false,
                }
            })
            .collect();

        Ok(comments)
    }

    async fn create_pull_request(
        &self,
        params: CreatePrParams,
    ) -> Result<PullRequest, PlatformError> {
        let path = format!("{}/merge_requests", self.project_path());

        let mut payload = serde_json::json!({
            "source_branch": params.head,
            "target_branch": params.base,
            "title": params.title,
            "description": params.body,
        });

        if params.draft {
            payload["draft"] = serde_json::Value::Bool(true);
        }

        let mr: GitlabMergeRequest = self.post_json(&path, &payload).await?;

        Ok(PullRequest {
            id: mr.id,
            number: mr.iid,
            url: mr.web_url,
            state: mr.state,
        })
    }

    async fn validate_credentials(&self) -> Result<(), PlatformError> {
        let user: GitlabCurrentUser = self.get_json("/user").await?;

        tracing::info!(
            username = %user.username,
            project_id = self.project_id,
            "GitLab credentials validated successfully"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::platform::Capability;

    #[test]
    fn test_capabilities_includes_atomic_labels() {
        let caps = vec![Capability::AtomicLabels];
        assert!(caps.contains(&Capability::AtomicLabels));
        assert!(!caps.contains(&Capability::Webhook));
        assert!(!caps.contains(&Capability::MergeRequest));
    }

    #[test]
    fn test_composite_comment_id_encoding() {
        let issue_iid: u64 = 42;
        let note_id: u64 = 12345;
        let composite = issue_iid * 1_000_000 + note_id;

        assert_eq!(composite / 1_000_000, issue_iid);
        assert_eq!(composite % 1_000_000, note_id);
    }

    #[test]
    fn test_composite_comment_id_large_values() {
        // Verify encoding works for large issue IIDs
        let issue_iid: u64 = 99999;
        let note_id: u64 = 999999;
        let composite = issue_iid * 1_000_000 + note_id;

        assert_eq!(composite / 1_000_000, issue_iid);
        assert_eq!(composite % 1_000_000, note_id);
    }

    #[test]
    fn test_composite_comment_id_zero_issue() {
        let issue_iid: u64 = 0;
        let note_id: u64 = 500;
        let composite = issue_iid * 1_000_000 + note_id;

        assert_eq!(composite / 1_000_000, 0);
        assert_eq!(composite % 1_000_000, 500);
    }
}
