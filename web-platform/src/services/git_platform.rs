use async_trait::async_trait;

use crate::models::kanban::{CreateIssueRequest, PlatformIssue, PlatformMergeRequest};
use crate::proxy::EffectiveProxyConfig;

/// Errors that can occur when calling the git platform API.
#[derive(Debug, thiserror::Error)]
pub enum GitPlatformError {
    /// The user's token is invalid or expired (401/403 from platform).
    #[error("Platform token is invalid or expired: {0}")]
    TokenInvalid(String),

    /// The requested resource was not found on the platform (404).
    #[error("Resource not found on platform: {0}")]
    NotFound(String),

    /// The platform API is unavailable or returned a server error (5xx, timeout).
    #[error("External platform service unavailable: {0}")]
    ServiceUnavailable(String),

    /// A request error (network, timeout, etc).
    #[error("Request failed: {0}")]
    RequestError(String),
}

/// A member returned from the platform API.
#[derive(Debug, Clone)]
pub struct PlatformMember {
    pub username: String,
    pub access_level: String,
}

/// Options for listing issues from the platform.
#[derive(Debug, Clone, Default)]
pub struct ListIssuesOptions {
    /// Only return issues with ALL of these labels.
    pub labels: Option<Vec<String>>,
    /// Exclude issues that have ANY of these labels.
    pub exclude_labels: Option<Vec<String>>,
    /// Filter by assignee username.
    pub assignee: Option<String>,
    /// Filter by author username (Phase 4).
    pub author: Option<String>,
    /// Search by title keyword.
    pub search: Option<String>,
    /// Maximum number of issues to return.
    pub limit: u32,
    /// Only return issues in this state (e.g., "opened").
    pub state: Option<String>,
}

/// Unified trait for interacting with GitLab or GitHub project APIs.
///
/// All methods take a `token` parameter which is the user's decrypted
/// platform access token. The `project_path` is the namespace/repo
/// identifier (e.g., "group/project" for GitLab, "owner/repo" for GitHub).
#[async_trait]
pub trait GitPlatformClient: Send + Sync {
    /// List issues matching the given options.
    async fn list_issues(
        &self,
        token: &str,
        project_path: &str,
        options: &ListIssuesOptions,
    ) -> Result<(Vec<PlatformIssue>, u64), GitPlatformError>;

    /// Get a single issue by its iid (GitLab) or number (GitHub).
    async fn get_issue(
        &self,
        token: &str,
        project_path: &str,
        iid: u64,
    ) -> Result<PlatformIssue, GitPlatformError>;

    /// Create a new issue.
    async fn create_issue(
        &self,
        token: &str,
        project_path: &str,
        req: &CreateIssueRequest,
    ) -> Result<PlatformIssue, GitPlatformError>;

    /// Get merge requests / pull requests associated with an issue.
    async fn get_issue_merge_requests(
        &self,
        token: &str,
        project_path: &str,
        issue_iid: u64,
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError>;

    /// Get a single merge request / pull request by its iid/number.
    async fn get_merge_request(
        &self,
        token: &str,
        project_path: &str,
        mr_iid: u64,
    ) -> Result<PlatformMergeRequest, GitPlatformError>;

    /// List project members from the platform.
    async fn list_members(
        &self,
        token: &str,
        project_path: &str,
    ) -> Result<Vec<PlatformMember>, GitPlatformError>;
}

/// Factory function to create the appropriate client based on platform type.
pub fn create_platform_client(
    platform: &str,
    platform_host: Option<&str>,
) -> Box<dyn GitPlatformClient> {
    create_platform_client_with_proxy(platform, platform_host, None)
        .expect("failed to create platform client")
}

/// Factory function to create the appropriate client with proxy configuration.
pub fn create_platform_client_with_proxy(
    platform: &str,
    platform_host: Option<&str>,
    proxy: Option<&EffectiveProxyConfig>,
) -> Result<Box<dyn GitPlatformClient>, GitPlatformError> {
    match platform {
        "github" => Ok(Box::new(
            super::github_client::GitHubClient::new_with_proxy(proxy)?,
        )),
        _ => {
            // Default to GitLab; use custom host if provided.
            let base_url = platform_host
                .map(|h| {
                    let host = h.trim_end_matches('/');
                    if host.starts_with("http://") || host.starts_with("https://") {
                        host.to_string()
                    } else {
                        format!("https://{}", host)
                    }
                })
                .unwrap_or_else(|| "https://gitlab.com".to_string());
            Ok(Box::new(
                super::gitlab_client::GitLabClient::new_with_proxy(base_url, proxy)?,
            ))
        }
    }
}
