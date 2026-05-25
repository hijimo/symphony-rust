use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::Deserialize;
use std::time::Duration;

use crate::models::issue::PlatformUser;
use crate::models::kanban::{
    CreateIssueRequest, PlatformIssue, PlatformMergeRequest, PlatformReviewer,
};
use crate::proxy::EffectiveProxyConfig;
use crate::services::git_platform::{
    CreateMergeRequest, GitPlatformClient, GitPlatformError, ListIssuesOptions, MergeRequestState,
    PlatformConflictCode, PlatformMember, PlatformValidationCode,
};

/// GitLab API client implementation.
pub struct GitLabClient {
    base_url: String,
    http: Client,
}

impl GitLabClient {
    pub fn new(base_url: String) -> Self {
        Self::new_with_proxy(base_url, None).expect("failed to build reqwest client")
    }

    pub fn new_with_proxy(
        base_url: String,
        proxy: Option<&EffectiveProxyConfig>,
    ) -> Result<Self, GitPlatformError> {
        let builder = match proxy {
            Some(proxy) => proxy
                .apply_to_reqwest_builder(Client::builder())
                .map_err(|e| GitPlatformError::RequestError(e.to_string()))?,
            None => Client::builder(),
        };
        let http = builder
            .timeout(Duration::from_secs(10))
            .user_agent("Symphony-WebPlatform/0.3.0")
            .build()
            .map_err(|e| GitPlatformError::RequestError(e.to_string()))?;

        Ok(Self { base_url, http })
    }

    fn list_issues_url(
        &self,
        project_path: &str,
        options: &ListIssuesOptions,
    ) -> Result<Url, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let mut url = Url::parse(&format!(
            "{}/api/v4/projects/{}/issues",
            self.base_url, encoded
        ))
        .map_err(|e| GitPlatformError::RequestError(format!("Invalid GitLab URL: {}", e)))?;

        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("per_page", &options.limit.to_string())
                .append_pair("order_by", "created_at")
                .append_pair("sort", "desc")
                .append_pair("state", options.state.as_deref().unwrap_or("opened"));

            if let Some(ref labels) = options.labels {
                if !labels.is_empty() {
                    pairs.append_pair("labels", &labels.join(","));
                }
            }

            if let Some(ref exclude) = options.exclude_labels {
                if !exclude.is_empty() {
                    pairs.append_pair("not[labels]", &exclude.join(","));
                }
            }

            if let Some(ref assignee) = options.assignee {
                pairs.append_pair("assignee_username", assignee);
            }

            if let Some(ref author) = options.author {
                pairs.append_pair("author_username", author);
            }

            if let Some(ref search) = options.search {
                pairs
                    .append_pair("search", search)
                    .append_pair("in", "title");
            }
        }

        Ok(url)
    }

    fn create_issue_body(req: &CreateIssueRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "title": req.title,
        });

        if let Some(ref desc) = req.description {
            body["description"] = serde_json::Value::String(desc.clone());
        }

        body["labels"] = serde_json::Value::String(req.labels_with_default_todo().join(","));

        if let Some(ref assignee) = req.assignee {
            body["assignee_username"] = serde_json::Value::String(assignee.clone());
        }

        body
    }

    fn create_merge_request_body(req: &CreateMergeRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "source_branch": req.source_branch,
            "target_branch": req.target_branch,
            "title": req.title,
        });

        if let Some(ref description) = req.description {
            body["description"] = serde_json::Value::String(description.clone());
        }
        if req.draft {
            body["title"] = serde_json::Value::String(format!("Draft: {}", req.title));
        }

        body
    }

    fn classify_create_error(status: reqwest::StatusCode, body: &str) -> GitPlatformError {
        let lower = body.to_ascii_lowercase();
        match status.as_u16() {
            400 | 409 | 422 => {
                if lower.contains("another open merge request already exists")
                    || lower.contains("merge request already exists")
                    || lower.contains("already exists")
                {
                    return GitPlatformError::Conflict {
                        code: PlatformConflictCode::ExistingOpenMergeRequest,
                        message: truncate_body(body).to_string(),
                    };
                }
                if lower.contains("no commits")
                    || lower.contains("must be different")
                    || lower.contains("no changes")
                {
                    return GitPlatformError::Validation {
                        code: PlatformValidationCode::NoCommits,
                        message: truncate_body(body).to_string(),
                    };
                }
                if lower.contains("source branch") && lower.contains("not exist") {
                    return GitPlatformError::Validation {
                        code: PlatformValidationCode::SourceBranchNotFound,
                        message: truncate_body(body).to_string(),
                    };
                }
                if lower.contains("target branch") && lower.contains("not exist") {
                    return GitPlatformError::Validation {
                        code: PlatformValidationCode::TargetBranchNotFound,
                        message: truncate_body(body).to_string(),
                    };
                }
                GitPlatformError::Validation {
                    code: PlatformValidationCode::Unknown,
                    message: truncate_body(body).to_string(),
                }
            }
            403 => GitPlatformError::Forbidden(format!(
                "GitLab returned {}: {}",
                status,
                truncate_body(body)
            )),
            _ => Self::map_status_error(status, body),
        }
    }

    async fn list_merge_requests_by_branches(
        &self,
        token: &str,
        project_path: &str,
        source_branch: &str,
        target_branch: &str,
        state: &str,
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let mut url = Url::parse(&format!(
            "{}/api/v4/projects/{}/merge_requests",
            self.base_url, encoded
        ))
        .map_err(|e| GitPlatformError::RequestError(format!("Invalid GitLab URL: {}", e)))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("state", state)
                .append_pair("source_branch", source_branch)
                .append_pair("target_branch", target_branch)
                .append_pair("order_by", "updated_at")
                .append_pair("sort", "desc")
                .append_pair("per_page", "20");
        }

        let response = self
            .http
            .get(url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let mrs: Vec<GitLabMergeRequest> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!(
                "Failed to parse GitLab merge requests response: {}",
                e
            ))
        })?;
        let mut result: Vec<_> = mrs
            .into_iter()
            .map(|mr| mr.into_platform_mr_for_project(project_path))
            .collect();
        sort_merge_requests(&mut result);
        Ok(result)
    }

    async fn fetch_closing_issue_iids(
        &self,
        token: &str,
        project_path: &str,
        mr_iid: u64,
    ) -> Result<Vec<u64>, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests/{}/closes_issues",
            self.base_url, encoded, mr_iid
        );

        let response = self
            .http
            .get(&url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let issues: Vec<GitLabClosingIssue> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!(
                "Failed to parse GitLab closes_issues response: {}",
                e
            ))
        })?;

        Ok(issues.into_iter().map(|issue| issue.iid).collect())
    }

    /// URL-encode the project path for GitLab API (namespace%2Frepo).
    fn encode_project_path(project_path: &str) -> String {
        project_path.replace('/', "%2F")
    }

    /// Map HTTP status codes to our error types.
    fn map_status_error(status: reqwest::StatusCode, body: &str) -> GitPlatformError {
        match status.as_u16() {
            401 | 403 => GitPlatformError::TokenInvalid(format!(
                "GitLab returned {}: {}",
                status,
                truncate_body(body)
            )),
            404 => GitPlatformError::NotFound(format!(
                "GitLab resource not found: {}",
                truncate_body(body)
            )),
            429 => {
                GitPlatformError::ServiceUnavailable("GitLab API rate limit exceeded".to_string())
            }
            500..=599 => GitPlatformError::ServiceUnavailable(format!(
                "GitLab server error {}: {}",
                status,
                truncate_body(body)
            )),
            _ => GitPlatformError::RequestError(format!(
                "GitLab returned unexpected status {}: {}",
                status,
                truncate_body(body)
            )),
        }
    }
}

#[async_trait]
impl GitPlatformClient for GitLabClient {
    async fn list_issues(
        &self,
        token: &str,
        project_path: &str,
        options: &ListIssuesOptions,
    ) -> Result<(Vec<PlatformIssue>, u64), GitPlatformError> {
        let url = self.list_issues_url(project_path, options)?;

        let response = self
            .http
            .get(url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        // Extract total count from header
        let total_count = response
            .headers()
            .get("x-total")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let issues: Vec<GitLabIssue> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitLab issues response: {}", e))
        })?;

        let platform_issues = issues
            .into_iter()
            .map(|i| i.into_platform_issue())
            .collect();
        Ok((platform_issues, total_count))
    }

    async fn get_issue(
        &self,
        token: &str,
        project_path: &str,
        iid: u64,
    ) -> Result<PlatformIssue, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let url = format!(
            "{}/api/v4/projects/{}/issues/{}",
            self.base_url, encoded, iid
        );

        let response = self
            .http
            .get(&url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let issue: GitLabIssue = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitLab issue: {}", e))
        })?;

        Ok(issue.into_platform_issue())
    }

    async fn create_issue(
        &self,
        token: &str,
        project_path: &str,
        req: &CreateIssueRequest,
    ) -> Result<PlatformIssue, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let url = format!("{}/api/v4/projects/{}/issues", self.base_url, encoded);

        let body = Self::create_issue_body(req);

        let response = self
            .http
            .post(&url)
            .header("PRIVATE-TOKEN", token)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let issue: GitLabIssue = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!(
                "Failed to parse GitLab create issue response: {}",
                e
            ))
        })?;

        Ok(issue.into_platform_issue())
    }

    async fn get_issue_merge_requests(
        &self,
        token: &str,
        project_path: &str,
        issue_iid: u64,
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let url = format!(
            "{}/api/v4/projects/{}/issues/{}/related_merge_requests",
            self.base_url, encoded, issue_iid
        );

        let response = self
            .http
            .get(&url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let mrs: Vec<GitLabMergeRequest> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!(
                "Failed to parse GitLab merge requests response: {}",
                e
            ))
        })?;

        Ok(mrs
            .into_iter()
            .map(|mr| mr.into_platform_mr_for_project(project_path))
            .collect())
    }

    async fn get_merge_request(
        &self,
        token: &str,
        project_path: &str,
        mr_iid: u64,
    ) -> Result<PlatformMergeRequest, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests/{}",
            self.base_url, encoded, mr_iid
        );

        let response = self
            .http
            .get(&url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let mr: GitLabMergeRequest = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitLab merge request: {}", e))
        })?;

        let mut platform_mr = mr.into_platform_mr_for_project(project_path);
        platform_mr.related_issue_iids = self
            .fetch_closing_issue_iids(token, project_path, mr_iid)
            .await
            .unwrap_or_default();

        Ok(platform_mr)
    }

    async fn create_merge_request(
        &self,
        token: &str,
        project_path: &str,
        req: &CreateMergeRequest,
    ) -> Result<PlatformMergeRequest, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let url = format!(
            "{}/api/v4/projects/{}/merge_requests",
            self.base_url, encoded
        );

        let response = self
            .http
            .post(&url)
            .header("PRIVATE-TOKEN", token)
            .json(&Self::create_merge_request_body(req))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::classify_create_error(status, &body));
        }

        let mr: GitLabMergeRequest = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitLab merge request: {}", e))
        })?;
        Ok(mr.into_platform_mr_for_project(project_path))
    }

    async fn find_open_merge_request_by_branches(
        &self,
        token: &str,
        project_path: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<Option<PlatformMergeRequest>, GitPlatformError> {
        let mrs = self
            .list_merge_requests_by_branches(
                token,
                project_path,
                source_branch,
                target_branch,
                "opened",
            )
            .await?;
        Ok(mrs.into_iter().next())
    }

    async fn find_merge_requests_by_branches(
        &self,
        token: &str,
        project_path: &str,
        source_branch: &str,
        target_branch: &str,
        states: &[MergeRequestState],
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError> {
        let mut result = Vec::new();
        if states.contains(&MergeRequestState::Open) {
            result.extend(
                self.list_merge_requests_by_branches(
                    token,
                    project_path,
                    source_branch,
                    target_branch,
                    "opened",
                )
                .await?,
            );
        }
        if states.contains(&MergeRequestState::Closed) {
            result.extend(
                self.list_merge_requests_by_branches(
                    token,
                    project_path,
                    source_branch,
                    target_branch,
                    "closed",
                )
                .await?,
            );
        }
        if states.contains(&MergeRequestState::Merged) {
            result.extend(
                self.list_merge_requests_by_branches(
                    token,
                    project_path,
                    source_branch,
                    target_branch,
                    "merged",
                )
                .await?,
            );
        }
        sort_merge_requests(&mut result);
        Ok(result)
    }

    async fn list_members(
        &self,
        token: &str,
        project_path: &str,
    ) -> Result<Vec<PlatformMember>, GitPlatformError> {
        let encoded = Self::encode_project_path(project_path);
        let url = format!(
            "{}/api/v4/projects/{}/members/all?per_page=100",
            self.base_url, encoded
        );

        let response = self
            .http
            .get(&url)
            .header("PRIVATE-TOKEN", token)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitLab API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitLab request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let members: Vec<GitLabMember> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!(
                "Failed to parse GitLab members response: {}",
                e
            ))
        })?;

        Ok(members
            .into_iter()
            .map(|m| {
                let role = if m.access_level >= 40 {
                    "owner".to_string()
                } else {
                    "member".to_string()
                };
                PlatformMember {
                    username: m.username,
                    access_level: role,
                }
            })
            .collect())
    }
}

// ==================== GitLab API response types ====================

#[derive(Debug, Deserialize)]
struct GitLabIssue {
    iid: u64,
    title: String,
    description: Option<String>,
    state: String,
    labels: Vec<String>,
    author: GitLabUser,
    assignees: Option<Vec<GitLabUser>>,
    milestone: Option<GitLabMilestone>,
    created_at: String,
    updated_at: String,
    closed_at: Option<String>,
    web_url: String,
    user_notes_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitLabUser {
    username: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabMilestone {
    title: String,
}

#[derive(Debug, Deserialize)]
struct GitLabMergeRequest {
    id: Option<u64>,
    iid: u64,
    title: String,
    description: Option<String>,
    state: String,
    author: GitLabUser,
    source_branch: String,
    target_branch: String,
    #[serde(default)]
    reviewers: Vec<GitLabUser>,
    merge_status: Option<String>,
    created_at: String,
    updated_at: String,
    merged_at: Option<String>,
    web_url: String,
    head_pipeline: Option<GitLabPipeline>,
    diff_stats: Option<GitLabDiffStats>,
}

#[derive(Debug, Deserialize)]
struct GitLabClosingIssue {
    iid: u64,
}

#[derive(Debug, Deserialize)]
struct GitLabMember {
    username: String,
    access_level: u32,
}

#[derive(Debug, Deserialize)]
struct GitLabPipeline {
    status: Option<String>,
    web_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitLabDiffStats {
    additions: Option<u64>,
    deletions: Option<u64>,
    total: Option<u64>,
}

// ==================== Conversion helpers ====================

impl GitLabIssue {
    fn into_platform_issue(self) -> PlatformIssue {
        PlatformIssue {
            iid: self.iid,
            title: self.title,
            description: self.description,
            state: self.state,
            labels: self.labels,
            author: self.author.into_platform_user(),
            assignees: self
                .assignees
                .unwrap_or_default()
                .into_iter()
                .map(|u| u.into_platform_user())
                .collect(),
            milestone: self.milestone.map(|m| m.title),
            created_at: self.created_at,
            updated_at: self.updated_at,
            closed_at: self.closed_at,
            web_url: self.web_url,
            comment_count: self.user_notes_count,
        }
    }
}

impl GitLabUser {
    fn into_platform_user(self) -> PlatformUser {
        PlatformUser {
            username: self.username,
            display_name: self.name,
            avatar_url: self.avatar_url,
        }
    }
}

impl GitLabMergeRequest {
    fn into_platform_mr_for_project(self, project_path: &str) -> PlatformMergeRequest {
        let ci_status = self.head_pipeline.as_ref().and_then(|p| p.status.clone());
        let ci_web_url = self.head_pipeline.as_ref().and_then(|p| p.web_url.clone());

        let reviewers = self
            .reviewers
            .into_iter()
            .map(|u| PlatformReviewer {
                user: u.into_platform_user(),
                state: "pending".to_string(),
            })
            .collect();

        let (additions, deletions, changed_files) = if let Some(stats) = self.diff_stats {
            (stats.additions, stats.deletions, stats.total)
        } else {
            (None, None, None)
        };

        PlatformMergeRequest {
            iid: self.iid,
            platform_node_id: self.id.map(|id| id.to_string()),
            title: self.title,
            description: self.description,
            state: self.state,
            author: self.author.into_platform_user(),
            source_project_path: Some(project_path.to_string()),
            target_project_path: Some(project_path.to_string()),
            source_branch: self.source_branch,
            target_branch: self.target_branch,
            ci_status,
            ci_web_url,
            review_status: None,
            reviewers,
            merge_status: self.merge_status,
            related_issue_iids: Vec::new(),
            additions,
            deletions,
            changed_files,
            created_at: self.created_at,
            updated_at: self.updated_at,
            merged_at: self.merged_at,
            web_url: self.web_url,
        }
    }
}

fn sort_merge_requests(mrs: &mut [PlatformMergeRequest]) {
    mrs.sort_by(|a, b| {
        let open_rank = |state: &str| if state == "opened" { 0 } else { 1 };
        open_rank(&a.state)
            .cmp(&open_rank(&b.state))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| b.iid.cmp(&a.iid))
    });
}

/// Truncate a response body for error messages.
fn truncate_body(body: &str) -> String {
    body.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_issues_url_encodes_query_parameters() {
        let client = GitLabClient::new("https://gitlab.example.com".to_string());
        let options = ListIssuesOptions {
            labels: Some(vec!["needs review".to_string(), "中文".to_string()]),
            exclude_labels: Some(vec!["symphony claimed".to_string()]),
            assignee: Some("alice+bob".to_string()),
            author: None,
            search: Some("foo & bar".to_string()),
            limit: 25,
            state: Some("opened".to_string()),
        };

        let url = client
            .list_issues_url("group/project", &options)
            .expect("url should parse");

        assert_eq!(
            url.as_str(),
            "https://gitlab.example.com/api/v4/projects/group%2Fproject/issues?per_page=25&order_by=created_at&sort=desc&state=opened&labels=needs+review%2C%E4%B8%AD%E6%96%87&not%5Blabels%5D=symphony+claimed&assignee_username=alice%2Bbob&search=foo+%26+bar&in=title"
        );
    }

    #[test]
    fn create_issue_body_adds_default_todo_label() {
        let req = CreateIssueRequest {
            title: "New issue".to_string(),
            description: None,
            labels: vec!["bug".to_string()],
            assignee: None,
        };

        let body = GitLabClient::create_issue_body(&req);

        assert_eq!(body["labels"], serde_json::json!("bug,Todo"));
    }

    #[test]
    fn create_issue_body_does_not_duplicate_todo_label() {
        let req = CreateIssueRequest {
            title: "New issue".to_string(),
            description: None,
            labels: vec!["Todo".to_string()],
            assignee: None,
        };

        let body = GitLabClient::create_issue_body(&req);

        assert_eq!(body["labels"], serde_json::json!("Todo"));
    }

    #[test]
    fn merge_request_conversion_sets_same_repo_project_paths() {
        let mr = GitLabMergeRequest {
            id: Some(123),
            iid: 17,
            title: "fix: login".to_string(),
            description: None,
            state: "opened".to_string(),
            author: GitLabUser {
                username: "alice".to_string(),
                name: None,
                avatar_url: None,
            },
            source_branch: "feature/login".to_string(),
            target_branch: "main".to_string(),
            reviewers: Vec::new(),
            merge_status: None,
            created_at: "2026-05-25T00:00:00Z".to_string(),
            updated_at: "2026-05-25T00:00:00Z".to_string(),
            merged_at: None,
            web_url: "https://gitlab.example.com/acme/app/-/merge_requests/17".to_string(),
            head_pipeline: None,
            diff_stats: None,
        };

        let platform_mr = mr.into_platform_mr_for_project("acme/app");

        assert_eq!(platform_mr.source_project_path.as_deref(), Some("acme/app"));
        assert_eq!(platform_mr.target_project_path.as_deref(), Some("acme/app"));
    }

    #[test]
    fn truncate_body_handles_multibyte_text() {
        let body = "错误".repeat(150);

        let truncated = truncate_body(&body);

        assert_eq!(truncated.chars().count(), 200);
        assert!(truncated.ends_with('误'));
    }
}
