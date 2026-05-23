use async_trait::async_trait;
use reqwest::{Client, Url};
use serde::Deserialize;
use std::time::Duration;

use crate::models::issue::PlatformUser;
use crate::models::kanban::{
    CreateIssueRequest, PlatformIssue, PlatformMergeRequest, PlatformReviewer,
};
use crate::services::git_platform::{
    GitPlatformClient, GitPlatformError, ListIssuesOptions, PlatformMember,
};

/// GitHub API client implementation.
pub struct GitHubClient {
    http: Client,
}

impl Default for GitHubClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubClient {
    pub fn new() -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Symphony-WebPlatform/0.3.0")
            .build()
            .expect("failed to build reqwest client");

        Self { http }
    }

    const BASE_URL: &'static str = "https://api.github.com";

    fn list_issues_url(
        project_path: &str,
        options: &ListIssuesOptions,
    ) -> Result<Url, GitPlatformError> {
        let mut url = Url::parse(&format!("{}/repos/{}/issues", Self::BASE_URL, project_path))
            .map_err(|e| GitPlatformError::RequestError(format!("Invalid GitHub URL: {}", e)))?;

        {
            let mut pairs = url.query_pairs_mut();
            pairs
                .append_pair("per_page", &options.limit.to_string())
                .append_pair("direction", "desc")
                .append_pair("sort", "created");

            let state = options.state.as_deref().unwrap_or("open");
            pairs.append_pair("state", if state == "opened" { "open" } else { state });

            if let Some(ref labels) = options.labels {
                if !labels.is_empty() {
                    pairs.append_pair("labels", &labels.join(","));
                }
            }

            if let Some(ref assignee) = options.assignee {
                pairs.append_pair("assignee", assignee);
            }

            if let Some(ref author) = options.author {
                pairs.append_pair("creator", author);
            }
        }

        Ok(url)
    }

    /// Map HTTP status codes to our error types.
    fn map_status_error(status: reqwest::StatusCode, body: &str) -> GitPlatformError {
        match status.as_u16() {
            401 | 403 => GitPlatformError::TokenInvalid(format!(
                "GitHub returned {}: {}",
                status,
                truncate_body(body)
            )),
            404 => GitPlatformError::NotFound(format!(
                "GitHub resource not found: {}",
                truncate_body(body)
            )),
            422 => GitPlatformError::RequestError(format!(
                "GitHub validation error: {}",
                truncate_body(body)
            )),
            429 => {
                GitPlatformError::ServiceUnavailable("GitHub API rate limit exceeded".to_string())
            }
            500..=599 => GitPlatformError::ServiceUnavailable(format!(
                "GitHub server error {}: {}",
                status,
                truncate_body(body)
            )),
            _ => GitPlatformError::RequestError(format!(
                "GitHub returned unexpected status {}: {}",
                status,
                truncate_body(body)
            )),
        }
    }

    /// Fetch reviews for a pull request.
    async fn fetch_reviews(
        &self,
        token: &str,
        project_path: &str,
        pr_number: u64,
    ) -> Result<Vec<GitHubReview>, GitPlatformError> {
        let url = format!(
            "{}/repos/{}/pulls/{}/reviews",
            Self::BASE_URL,
            project_path,
            pr_number
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                GitPlatformError::RequestError(format!("GitHub reviews request failed: {}", e))
            })?;

        if !response.status().is_success() {
            return Ok(Vec::new());
        }

        response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitHub reviews: {}", e))
        })
    }

    async fn fetch_pr_timeline_issue_iids(
        &self,
        token: &str,
        project_path: &str,
        pr_number: u64,
    ) -> Result<Vec<u64>, GitPlatformError> {
        let url = format!(
            "{}/repos/{}/issues/{}/timeline",
            Self::BASE_URL,
            project_path,
            pr_number
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitHub API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitHub request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let events: Vec<GitHubTimelineEvent> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitHub timeline events: {}", e))
        })?;

        let mut issue_iids = Vec::new();
        for event in &events {
            if event.event == "cross-referenced" {
                if let Some(ref source) = event.source {
                    if let Some(ref issue) = source.issue {
                        if issue.pull_request.is_none() {
                            issue_iids.push(issue.number);
                        }
                    }
                }
            }
        }

        issue_iids.sort_unstable();
        issue_iids.dedup();
        Ok(issue_iids)
    }

    fn extract_issue_references(text: Option<&str>) -> Vec<u64> {
        let Some(text) = text else {
            return Vec::new();
        };

        let re = regex::Regex::new(r"(?i)(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?|refs?)\s+(?:[\w.-]+/[\w.-]+)?#(\d+)|#(\d+)").unwrap();
        let mut issue_iids = Vec::new();

        for captures in re.captures_iter(text) {
            let number = captures.get(1).or_else(|| captures.get(2));
            if let Some(number) = number.and_then(|m| m.as_str().parse::<u64>().ok()) {
                issue_iids.push(number);
            }
        }

        issue_iids.sort_unstable();
        issue_iids.dedup();
        issue_iids
    }

    fn create_issue_body(req: &CreateIssueRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "title": req.title,
        });

        if let Some(ref desc) = req.description {
            body["body"] = serde_json::Value::String(desc.clone());
        }

        if !req.labels.is_empty() {
            body["labels"] = serde_json::json!(req.labels);
        }

        if let Some(ref assignee) = req.assignee {
            body["assignees"] = serde_json::json!([assignee]);
        }

        body
    }
}

#[async_trait]
impl GitPlatformClient for GitHubClient {
    async fn list_issues(
        &self,
        token: &str,
        project_path: &str,
        options: &ListIssuesOptions,
    ) -> Result<(Vec<PlatformIssue>, u64), GitPlatformError> {
        let url = Self::list_issues_url(project_path, options)?;

        let response = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitHub API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitHub request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let issues: Vec<GitHubIssue> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitHub issues response: {}", e))
        })?;

        // Filter out pull requests (GitHub returns PRs in the issues endpoint)
        let mut platform_issues: Vec<PlatformIssue> = issues
            .into_iter()
            .filter(|i| i.pull_request.is_none())
            .map(|i| i.into_platform_issue())
            .collect();

        // Client-side exclude_labels filter (GitHub doesn't support this natively)
        if let Some(ref exclude) = options.exclude_labels {
            if !exclude.is_empty() {
                platform_issues.retain(|issue| !issue.labels.iter().any(|l| exclude.contains(l)));
            }
        }

        // Client-side search filter
        if let Some(ref search) = options.search {
            let search_lower = search.to_lowercase();
            platform_issues.retain(|issue| issue.title.to_lowercase().contains(&search_lower));
        }

        let total_count = platform_issues.len() as u64;
        Ok((platform_issues, total_count))
    }

    async fn get_issue(
        &self,
        token: &str,
        project_path: &str,
        iid: u64,
    ) -> Result<PlatformIssue, GitPlatformError> {
        let url = format!("{}/repos/{}/issues/{}", Self::BASE_URL, project_path, iid);

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitHub API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitHub request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let issue: GitHubIssue = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitHub issue: {}", e))
        })?;

        Ok(issue.into_platform_issue())
    }

    async fn create_issue(
        &self,
        token: &str,
        project_path: &str,
        req: &CreateIssueRequest,
    ) -> Result<PlatformIssue, GitPlatformError> {
        let url = format!("{}/repos/{}/issues", Self::BASE_URL, project_path);
        let body = Self::create_issue_body(req);

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitHub API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitHub request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let issue: GitHubIssue = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!(
                "Failed to parse GitHub create issue response: {}",
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
        // Use Timeline Events API to find cross-referenced PRs
        let url = format!(
            "{}/repos/{}/issues/{}/timeline",
            Self::BASE_URL,
            project_path,
            issue_iid
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitHub API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitHub request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let events: Vec<GitHubTimelineEvent> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitHub timeline events: {}", e))
        })?;

        // Collect PR numbers from cross-reference events
        let mut pr_numbers: Vec<u64> = Vec::new();
        for event in &events {
            if event.event == "cross-referenced" {
                if let Some(ref source) = event.source {
                    if let Some(ref issue) = source.issue {
                        if issue.pull_request.is_some() {
                            pr_numbers.push(issue.number);
                        }
                    }
                }
            }
        }

        pr_numbers.sort_unstable();
        pr_numbers.dedup();

        // Fetch each PR's details (cap at 10 to avoid excessive API calls)
        let mut merge_requests = Vec::new();
        for pr_number in pr_numbers.iter().take(10) {
            match self
                .get_merge_request(token, project_path, *pr_number)
                .await
            {
                Ok(mr) => merge_requests.push(mr),
                Err(GitPlatformError::NotFound(_)) => continue,
                Err(e) => return Err(e),
            }
        }

        Ok(merge_requests)
    }

    async fn get_merge_request(
        &self,
        token: &str,
        project_path: &str,
        mr_iid: u64,
    ) -> Result<PlatformMergeRequest, GitPlatformError> {
        let url = format!("{}/repos/{}/pulls/{}", Self::BASE_URL, project_path, mr_iid);

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitHub API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitHub request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let pr: GitHubPullRequest = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!("Failed to parse GitHub pull request: {}", e))
        })?;

        let mut related_issue_iids = Self::extract_issue_references(Some(&pr.title));
        related_issue_iids.extend(Self::extract_issue_references(pr.body.as_deref()));
        related_issue_iids.extend(
            self.fetch_pr_timeline_issue_iids(token, project_path, mr_iid)
                .await
                .unwrap_or_default(),
        );
        related_issue_iids.sort_unstable();
        related_issue_iids.dedup();

        // Fetch reviews to determine review status
        let reviews = self
            .fetch_reviews(token, project_path, mr_iid)
            .await
            .unwrap_or_default();

        let mut platform_mr = pr.into_platform_mr(reviews);
        platform_mr.related_issue_iids = related_issue_iids;

        Ok(platform_mr)
    }

    async fn list_members(
        &self,
        token: &str,
        project_path: &str,
    ) -> Result<Vec<PlatformMember>, GitPlatformError> {
        let url = format!(
            "{}/repos/{}/collaborators?per_page=100",
            Self::BASE_URL,
            project_path
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    GitPlatformError::ServiceUnavailable("GitHub API request timed out".to_string())
                } else {
                    GitPlatformError::RequestError(format!("GitHub request failed: {}", e))
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::map_status_error(status, &body));
        }

        let collaborators: Vec<GitHubCollaborator> = response.json().await.map_err(|e| {
            GitPlatformError::RequestError(format!(
                "Failed to parse GitHub collaborators response: {}",
                e
            ))
        })?;

        Ok(collaborators
            .into_iter()
            .map(|c| {
                let role = if c.permissions.admin.unwrap_or(false) {
                    "owner".to_string()
                } else {
                    "member".to_string()
                };
                PlatformMember {
                    username: c.login,
                    access_level: role,
                }
            })
            .collect())
    }
}

// ==================== GitHub API response types ====================

#[derive(Debug, Deserialize)]
struct GitHubIssue {
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    labels: Vec<GitHubLabel>,
    user: GitHubUser,
    assignees: Option<Vec<GitHubUser>>,
    milestone: Option<GitHubMilestone>,
    created_at: String,
    updated_at: String,
    closed_at: Option<String>,
    html_url: String,
    comments: Option<u64>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GitHubLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GitHubUser {
    login: String,
    #[serde(default)]
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubCollaborator {
    login: String,
    permissions: GitHubPermissions,
}

#[derive(Debug, Deserialize)]
struct GitHubPermissions {
    admin: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GitHubMilestone {
    title: String,
}

#[derive(Debug, Deserialize)]
struct GitHubPullRequest {
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    user: GitHubUser,
    head: GitHubBranch,
    base: GitHubBranch,
    merged: Option<bool>,
    mergeable: Option<bool>,
    mergeable_state: Option<String>,
    requested_reviewers: Option<Vec<GitHubUser>>,
    additions: Option<u64>,
    deletions: Option<u64>,
    changed_files: Option<u64>,
    created_at: String,
    updated_at: String,
    merged_at: Option<String>,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct GitHubBranch {
    #[serde(rename = "ref")]
    ref_name: String,
}

#[derive(Debug, Deserialize)]
struct GitHubReview {
    user: GitHubUser,
    state: String,
}

#[derive(Debug, Deserialize)]
struct GitHubTimelineEvent {
    event: String,
    source: Option<GitHubTimelineSource>,
}

#[derive(Debug, Deserialize)]
struct GitHubTimelineSource {
    issue: Option<GitHubTimelineIssue>,
}

#[derive(Debug, Deserialize)]
struct GitHubTimelineIssue {
    number: u64,
    pull_request: Option<serde_json::Value>,
}

// ==================== Conversion helpers ====================

impl GitHubIssue {
    fn into_platform_issue(self) -> PlatformIssue {
        // GitHub uses "open"/"closed"; normalize to "opened"/"closed"
        let state = if self.state == "open" {
            "opened".to_string()
        } else {
            self.state
        };

        PlatformIssue {
            iid: self.number,
            title: self.title,
            description: self.body,
            state,
            labels: self.labels.into_iter().map(|l| l.name).collect(),
            author: self.user.into_platform_user(),
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
            web_url: self.html_url,
            comment_count: self.comments,
        }
    }
}

impl GitHubUser {
    fn into_platform_user(self) -> PlatformUser {
        PlatformUser {
            username: self.login,
            display_name: self.name,
            avatar_url: self.avatar_url,
        }
    }
}

impl GitHubPullRequest {
    fn into_platform_mr(self, reviews: Vec<GitHubReview>) -> PlatformMergeRequest {
        // Determine state
        let state = if self.merged == Some(true) {
            "merged".to_string()
        } else if self.state == "open" {
            "opened".to_string()
        } else {
            "closed".to_string()
        };

        // Determine merge_status
        let merge_status = match self.mergeable {
            Some(true) => Some("can_be_merged".to_string()),
            Some(false) => Some("cannot_be_merged".to_string()),
            None => match self.mergeable_state.as_deref() {
                Some("unknown") => Some("checking".to_string()),
                _ => Some("unchecked".to_string()),
            },
        };

        // Build reviewers from reviews + requested_reviewers
        let mut reviewers: Vec<PlatformReviewer> = reviews
            .into_iter()
            .map(|r| {
                let review_state = match r.state.as_str() {
                    "APPROVED" => "approved",
                    "CHANGES_REQUESTED" => "changes_requested",
                    _ => "pending",
                };
                PlatformReviewer {
                    user: r.user.into_platform_user(),
                    state: review_state.to_string(),
                }
            })
            .collect();

        // Add requested reviewers who haven't reviewed yet
        if let Some(requested) = self.requested_reviewers {
            for user in requested {
                let username = user.login.clone();
                if !reviewers.iter().any(|r| r.user.username == username) {
                    reviewers.push(PlatformReviewer {
                        user: user.into_platform_user(),
                        state: "pending".to_string(),
                    });
                }
            }
        }

        // Aggregate review_status
        let review_status = if reviewers.is_empty() {
            None
        } else if reviewers.iter().any(|r| r.state == "changes_requested") {
            Some("changes_requested".to_string())
        } else if reviewers.iter().all(|r| r.state == "approved") {
            Some("approved".to_string())
        } else {
            Some("pending".to_string())
        };

        PlatformMergeRequest {
            iid: self.number,
            title: self.title,
            description: self.body,
            state,
            author: self.user.into_platform_user(),
            source_branch: self.head.ref_name,
            target_branch: self.base.ref_name,
            ci_status: None,
            ci_web_url: None,
            review_status,
            reviewers,
            merge_status,
            related_issue_iids: Vec::new(),
            additions: self.additions,
            deletions: self.deletions,
            changed_files: self.changed_files,
            created_at: self.created_at,
            updated_at: self.updated_at,
            merged_at: self.merged_at,
            web_url: self.html_url,
        }
    }
}

/// Truncate a response body for error messages.
fn truncate_body(body: &str) -> &str {
    if body.len() > 200 {
        &body[..200]
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_issues_url_encodes_query_parameters() {
        let options = ListIssuesOptions {
            labels: Some(vec!["needs review".to_string(), "中文".to_string()]),
            exclude_labels: None,
            assignee: Some("alice+bob".to_string()),
            author: None,
            search: None,
            limit: 25,
            state: Some("opened".to_string()),
        };

        let url = GitHubClient::list_issues_url("owner/repo", &options).expect("url should parse");

        assert_eq!(
            url.as_str(),
            "https://api.github.com/repos/owner/repo/issues?per_page=25&direction=desc&sort=created&state=open&labels=needs+review%2C%E4%B8%AD%E6%96%87&assignee=alice%2Bbob"
        );
    }

    #[test]
    fn extract_issue_references_deduplicates_numbers() {
        let refs = GitHubClient::extract_issue_references(Some(
            "Fixes #12, resolves org/repo#34 and refs #12",
        ));

        assert_eq!(refs, vec![12, 34]);
    }

    #[test]
    fn create_issue_body_includes_labels_as_array() {
        let req = CreateIssueRequest {
            title: "Issue with labels".to_string(),
            description: Some("body".to_string()),
            labels: vec!["Todo".to_string(), "In Progress".to_string()],
            assignee: Some("alice".to_string()),
        };

        let body = GitHubClient::create_issue_body(&req);

        assert_eq!(body["title"], "Issue with labels");
        assert_eq!(body["body"], "body");
        assert_eq!(body["labels"], serde_json::json!(["Todo", "In Progress"]));
        assert_eq!(body["assignees"], serde_json::json!(["alice"]));
    }
}
