use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::config::PlatformConfig;
use crate::error::PlatformError;
use crate::platform::http_client::HttpClient;
use crate::platform::{
    Capability, Comment, CommentId, CreatePrParams, FetchOptions, Issue, IssueId, Platform,
    PullRequest,
};

const MAX_PAGES: u32 = 10;
const PER_PAGE: u32 = 50;
const LABEL_CACHE_LIMIT: usize = 1000;

// ---------------------------------------------------------------------------
// Gitea API response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GiteaUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GiteaLabel {
    id: u64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct GiteaIssue {
    #[allow(dead_code)]
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    html_url: String,
    state: String,
    labels: Vec<GiteaLabel>,
    assignee: Option<GiteaUser>,
    created_at: String,
    updated_at: String,
    pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GiteaComment {
    id: u64,
    body: Option<String>,
    user: Option<GiteaUser>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct GiteaPullRequest {
    id: u64,
    number: u64,
    html_url: String,
    state: String,
}

// ---------------------------------------------------------------------------
// GiteaAdapter
// ---------------------------------------------------------------------------

pub struct GiteaAdapter {
    http: HttpClient,
    config: PlatformConfig,
    label_cache: RwLock<HashMap<String, u64>>,
}

impl GiteaAdapter {
    pub fn new(config: PlatformConfig) -> Result<Self, PlatformError> {
        let http = HttpClient::new(config.clone())?;
        Ok(Self {
            http,
            config,
            label_cache: RwLock::new(HashMap::new()),
        })
    }

    pub fn new_with_token(config: PlatformConfig, token: &str) -> Result<Self, PlatformError> {
        let http = HttpClient::new_with_resolved_token(config.clone(), token)?;
        Ok(Self {
            http,
            config,
            label_cache: RwLock::new(HashMap::new()),
        })
    }

    pub fn http_client(&self) -> &HttpClient {
        &self.http
    }

    fn repo_path(&self) -> String {
        format!("/repos/{}/{}", self.config.owner, self.config.repo)
    }

    async fn refresh_label_cache(&self) -> Result<(), PlatformError> {
        let path = format!("{}/labels", self.repo_path());
        let labels: Vec<GiteaLabel> = self.paginated_get(&path, &[]).await?;

        let mut cache = self.label_cache.write().await;
        *cache = labels
            .into_iter()
            .take(LABEL_CACHE_LIMIT)
            .map(|l| (l.name, l.id))
            .collect();
        Ok(())
    }

    async fn resolve_label_id(&self, name: &str) -> Result<u64, PlatformError> {
        // Try cache first
        {
            let cache = self.label_cache.read().await;
            if let Some(&id) = cache.get(name) {
                return Ok(id);
            }
        }

        // Cache miss — refresh once
        self.refresh_label_cache().await?;

        let cache = self.label_cache.read().await;
        cache.get(name).copied().ok_or_else(|| {
            PlatformError::NotFound(format!("Label '{}' not found in repository", name))
        })
    }

    /// Gitea-specific paginated GET using `?limit=N&page=N` parameters.
    async fn paginated_get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<T>, PlatformError> {
        let mut all_items: Vec<T> = Vec::new();
        let mut page = 1u32;

        loop {
            if page > MAX_PAGES {
                tracing::warn!(
                    path,
                    max_pages = MAX_PAGES,
                    "Reached max page limit, results may be truncated"
                );
                break;
            }

            let page_str = page.to_string();
            let limit_str = PER_PAGE.to_string();

            let mut query: Vec<(&str, &str)> = params.to_vec();
            query.push(("page", &page_str));
            query.push(("limit", &limit_str));

            let url = format!("{}{}", self.http.base_url(), path);
            let response = self
                .http
                .inner()
                .get(&url)
                .query(&query)
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
                let body = response.text().await.unwrap_or_default();
                let truncated = if body.len() > 500 { &body[..500] } else { &body };
                return Err(PlatformError::from_status(status.as_u16(), truncated));
            }

            let items: Vec<T> = response.json().await.map_err(|e| {
                tracing::error!(path, page, error = %e, "Failed to deserialize page response");
                PlatformError::Network(e)
            })?;

            let item_count = items.len();
            all_items.extend(items);

            if item_count < PER_PAGE as usize {
                break;
            }
            page += 1;
        }

        Ok(all_items)
    }

    fn convert_issue(&self, gi: GiteaIssue) -> Issue {
        let workflow_state = if gi.state.eq_ignore_ascii_case("closed") {
            Some(self.inferred_closed_issue_state())
        } else {
            self.extract_workflow_state(&gi.labels)
        };

        let labels: Vec<String> = gi.labels.iter().map(|l| l.name.clone()).collect();
        let branch_name = format!("symphony/issue-{}", gi.number);

        Issue {
            id: IssueId(gi.number),
            number: gi.number,
            title: gi.title,
            description: gi.body,
            url: gi.html_url,
            assignee: gi.assignee.map(|u| u.login),
            workflow_state,
            branch_name,
            priority: None,
            labels,
            blocked_by: Vec::new(),
            created_at: chrono::DateTime::parse_from_rfc3339(&gi.created_at)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            updated_at: chrono::DateTime::parse_from_rfc3339(&gi.updated_at)
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Utc)),
        }
    }

    fn extract_workflow_state(&self, labels: &[GiteaLabel]) -> Option<String> {
        for label in labels {
            for (key, value) in &self.config.workflow.states {
                if label.name == *value {
                    return Some(key.clone());
                }
            }
        }
        None
    }

    fn inferred_closed_issue_state(&self) -> String {
        if self
            .config
            .workflow
            .terminal_states
            .iter()
            .any(|s| s.eq_ignore_ascii_case("done"))
        {
            "done".to_string()
        } else {
            self.config
                .workflow
                .terminal_states
                .first()
                .map(|s| s.trim().to_lowercase().replace([' ', '-'], "_"))
                .unwrap_or_else(|| "done".to_string())
        }
    }

    fn all_workflow_labels(&self) -> Vec<String> {
        self.config.workflow.states.values().cloned().collect()
    }
}

#[async_trait]
impl Platform for GiteaAdapter {
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }

    async fn fetch_candidate_issues(
        &self,
        _opts: FetchOptions,
    ) -> Result<Vec<Issue>, PlatformError> {
        let path = format!("{}/issues", self.repo_path());

        let filter_labels: Vec<String> = self
            .config
            .workflow
            .active_states
            .iter()
            .filter_map(|state| {
                let normalized = state.to_lowercase().replace([' ', '-'], "_");
                self.config.workflow.states.get(&normalized).cloned()
            })
            .collect();

        let mut all_issues: Vec<GiteaIssue> = Vec::new();
        let mut seen_numbers: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for label in &filter_labels {
            let params: Vec<(&str, &str)> =
                vec![("labels", label.as_str()), ("state", "open"), ("type", "issues")];
            let gi_issues: Vec<GiteaIssue> = self.paginated_get(&path, &params).await?;
            for issue in gi_issues {
                if seen_numbers.insert(issue.number) {
                    all_issues.push(issue);
                }
            }
        }

        let issues: Vec<Issue> = all_issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(|i| self.convert_issue(i))
            .collect();

        tracing::debug!(count = issues.len(), "Fetched candidate issues from Gitea");
        Ok(issues)
    }

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
            let truncated = if body.len() > 500 { &body[..500] } else { &body };
            return Err(PlatformError::from_status(status.as_u16(), truncated));
        }

        let gi_issue: GiteaIssue = response.json().await?;
        Ok(self.convert_issue(gi_issue))
    }

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

    async fn get_workflow_state(&self, issue_id: IssueId) -> Result<Option<String>, PlatformError> {
        let issue = self.fetch_issue(issue_id).await?;
        Ok(issue.workflow_state)
    }

    async fn set_workflow_state(
        &self,
        issue_id: IssueId,
        state: &str,
    ) -> Result<(), PlatformError> {
        let target_label = self
            .config
            .workflow
            .states
            .get(state)
            .cloned()
            .ok_or_else(|| PlatformError::MissingState(state.to_string()))?;

        let issue = self.fetch_issue(issue_id).await?;
        let all_workflow_labels = self.all_workflow_labels();
        let current_workflow_labels: Vec<String> = issue
            .labels
            .iter()
            .filter(|l| all_workflow_labels.contains(l))
            .cloned()
            .collect();

        // Add target label first (ensures issue always has at least one workflow label)
        self.add_labels(issue_id, std::slice::from_ref(&target_label))
            .await?;

        // Remove stale workflow labels
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

    async fn add_labels(&self, issue_id: IssueId, labels: &[String]) -> Result<(), PlatformError> {
        if labels.is_empty() {
            return Ok(());
        }

        let mut label_ids = Vec::with_capacity(labels.len());
        for label in labels {
            let id = self.resolve_label_id(label).await?;
            label_ids.push(id);
        }

        let path = format!("{}/issues/{}/labels", self.repo_path(), issue_id.0);
        let url = format!("{}{}", self.http.base_url(), path);
        let body = serde_json::json!({ "labels": label_ids });

        let response = self
            .http
            .inner()
            .post(&url)
            .json(&body)
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
            let truncated = if body_text.len() > 500 {
                &body_text[..500]
            } else {
                &body_text
            };
            return Err(PlatformError::from_status(status.as_u16(), truncated));
        }

        tracing::debug!(
            issue_id = %issue_id,
            labels = ?labels,
            "Labels added successfully"
        );
        Ok(())
    }

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
            let label_id = match self.resolve_label_id(label).await {
                Ok(id) => id,
                Err(_) => {
                    // Label not found — treat as already removed
                    removed.push(label.clone());
                    continue;
                }
            };

            let path = format!(
                "{}/issues/{}/labels/{}",
                self.repo_path(),
                issue_id.0,
                label_id
            );
            let url = format!("{}{}", self.http.base_url(), path);

            let response = self.http.inner().delete(&url).send().await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() || status.as_u16() == 404 {
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
            let truncated = if body_text.len() > 500 {
                &body_text[..500]
            } else {
                &body_text
            };
            return Err(PlatformError::from_status(status.as_u16(), truncated));
        }

        let comment: GiteaComment = response.json().await?;
        Ok(CommentId(comment.id))
    }

    async fn update_comment(&self, comment_id: CommentId, body: &str) -> Result<(), PlatformError> {
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
            let truncated = if body_text.len() > 500 {
                &body_text[..500]
            } else {
                &body_text
            };
            return Err(PlatformError::from_status(status.as_u16(), truncated));
        }

        Ok(())
    }

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

    async fn list_comments(&self, issue_id: IssueId) -> Result<Vec<Comment>, PlatformError> {
        let path = format!("{}/issues/{}/comments", self.repo_path(), issue_id.0);
        let gi_comments: Vec<GiteaComment> = self.paginated_get(&path, &[]).await?;

        let comments: Vec<Comment> = gi_comments
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
                is_system: false,
            })
            .collect();

        Ok(comments)
    }

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

        let response = self
            .http
            .inner()
            .post(&url)
            .json(&body)
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
            let truncated = if body_text.len() > 500 {
                &body_text[..500]
            } else {
                &body_text
            };
            return Err(PlatformError::from_status(status.as_u16(), truncated));
        }

        let pr: GiteaPullRequest = response.json().await?;
        Ok(PullRequest {
            id: pr.id,
            number: pr.number,
            url: pr.html_url,
            state: pr.state,
        })
    }

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
            let truncated = if body.len() > 500 { &body[..500] } else { &body };
            return Err(PlatformError::from_status(status.as_u16(), truncated));
        }

        let user: GiteaUser = response.json().await?;
        tracing::info!(
            user = %user.login,
            "Gitea credentials validated successfully"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{IssueFilter, WorkflowConfig};

    pub fn test_config(base_url: &str) -> PlatformConfig {
        std::env::set_var("SYMPHONY_TEST_GITEA_TOKEN", "test-token-value");

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
            kind: "gitea".to_string(),
            api_token: "$SYMPHONY_TEST_GITEA_TOKEN".to_string(),
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
        let adapter = GiteaAdapter::new(config).unwrap();

        let labels = vec![
            GiteaLabel {
                id: 1,
                name: "bug".to_string(),
            },
            GiteaLabel {
                id: 2,
                name: "workflow::todo".to_string(),
            },
        ];

        let state = adapter.extract_workflow_state(&labels);
        assert_eq!(state, Some("todo".to_string()));
    }

    #[test]
    fn test_extract_workflow_state_none() {
        let config = test_config("http://localhost:8080");
        let adapter = GiteaAdapter::new(config).unwrap();

        let labels = vec![GiteaLabel {
            id: 1,
            name: "bug".to_string(),
        }];

        let state = adapter.extract_workflow_state(&labels);
        assert_eq!(state, None);
    }

    #[test]
    fn test_convert_issue_closed_overrides_label() {
        let config = test_config("http://localhost:8080");
        let adapter = GiteaAdapter::new(config).unwrap();

        let gi_issue = GiteaIssue {
            id: 100,
            number: 5,
            title: "Closed issue".to_string(),
            body: None,
            html_url: "https://gitea.example.com/testorg/testrepo/issues/5".to_string(),
            state: "closed".to_string(),
            assignee: None,
            labels: vec![GiteaLabel {
                id: 2,
                name: "workflow::in-progress".to_string(),
            }],
            created_at: "2024-01-15T10:00:00Z".to_string(),
            updated_at: "2024-01-16T12:00:00Z".to_string(),
            pull_request: None,
        };

        let issue = adapter.convert_issue(gi_issue);
        assert_eq!(issue.workflow_state, Some("done".to_string()));
    }

    #[test]
    fn test_repo_path() {
        let config = test_config("http://localhost:8080");
        let adapter = GiteaAdapter::new(config).unwrap();
        assert_eq!(adapter.repo_path(), "/repos/testorg/testrepo");
    }

    #[test]
    fn test_capabilities_empty() {
        let config = test_config("http://localhost:8080");
        let adapter = GiteaAdapter::new(config).unwrap();
        assert!(adapter.capabilities().is_empty());
    }
}
