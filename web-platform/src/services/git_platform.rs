use async_trait::async_trait;

use serde::Serialize;

use crate::models::kanban::{CreateIssueRequest, PlatformIssue, PlatformMergeRequest};
use crate::proxy::EffectiveProxyConfig;

/// Errors that can occur when calling the git platform API.
#[derive(Debug, thiserror::Error)]
pub enum GitPlatformError {
    /// The user's token is invalid or expired (401/403 from platform).
    #[error("Platform token is invalid or expired: {0}")]
    TokenInvalid(String),

    /// The token is valid but does not have enough permissions.
    #[error("Platform access forbidden: {0}")]
    Forbidden(String),

    /// The requested resource was not found on the platform (404).
    #[error("Resource not found on platform: {0}")]
    NotFound(String),

    /// Platform rejected the request with a modeled validation error.
    #[error("Platform validation error {code:?}: {message}")]
    Validation {
        code: PlatformValidationCode,
        message: String,
    },

    /// Platform rejected the request because of an existing or policy conflict.
    #[error("Platform conflict {code:?}: {message}")]
    Conflict {
        code: PlatformConflictCode,
        message: String,
    },

    /// The platform API is unavailable or returned a server error (5xx, timeout).
    #[error("External platform service unavailable: {0}")]
    ServiceUnavailable(String),

    /// A request error (network, timeout, etc).
    #[error("Request failed: {0}")]
    RequestError(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformValidationCode {
    SourceBranchNotFound,
    TargetBranchNotFound,
    NoCommits,
    InvalidTitle,
    UnsupportedFork,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformConflictCode {
    ExistingOpenMergeRequest,
    ExistingClosedOrMergedMergeRequest,
    BranchProtectionOrPolicy,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateMergeRequest {
    pub source_branch: String,
    pub target_branch: String,
    pub title: String,
    pub description: Option<String>,
    pub draft: bool,
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

/// Options for listing merge requests / pull requests from the platform.
#[derive(Debug, Clone, Default)]
pub struct ListMergeRequestsOptions {
    /// Maximum number of merge requests / pull requests to return.
    pub limit: u32,
    /// Only return merge requests / pull requests in this state.
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

    /// List project-level merge requests / pull requests.
    async fn list_merge_requests(
        &self,
        token: &str,
        project_path: &str,
        options: &ListMergeRequestsOptions,
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError>;

    /// Get a single merge request / pull request by its iid/number.
    async fn get_merge_request(
        &self,
        token: &str,
        project_path: &str,
        mr_iid: u64,
    ) -> Result<PlatformMergeRequest, GitPlatformError>;

    /// Create a merge request / pull request.
    async fn create_merge_request(
        &self,
        token: &str,
        project_path: &str,
        req: &CreateMergeRequest,
    ) -> Result<PlatformMergeRequest, GitPlatformError>;

    /// Find an open merge request / pull request by source and target branches.
    async fn find_open_merge_request_by_branches(
        &self,
        token: &str,
        project_path: &str,
        source_branch: &str,
        target_branch: &str,
    ) -> Result<Option<PlatformMergeRequest>, GitPlatformError>;

    /// Find merge requests / pull requests by source, target, and states.
    async fn find_merge_requests_by_branches(
        &self,
        token: &str,
        project_path: &str,
        source_branch: &str,
        target_branch: &str,
        states: &[MergeRequestState],
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError>;

    /// List project members from the platform.
    async fn list_members(
        &self,
        token: &str,
        project_path: &str,
    ) -> Result<Vec<PlatformMember>, GitPlatformError>;

    // ----- AI-native SDLC document flow (default: unsupported) -----

    /// Replace the given labels on an issue: add `add`, remove `remove`.
    async fn update_issue_labels(
        &self,
        _token: &str,
        _project_path: &str,
        _iid: u64,
        _add: &[String],
        _remove: &[String],
    ) -> Result<(), GitPlatformError> {
        Err(unsupported("update_issue_labels"))
    }

    /// Post a comment on an issue.
    async fn add_issue_comment(
        &self,
        _token: &str,
        _project_path: &str,
        _iid: u64,
        _body: &str,
    ) -> Result<(), GitPlatformError> {
        Err(unsupported("add_issue_comment"))
    }

    /// Post a comment on a merge request / pull request.
    async fn add_merge_request_comment(
        &self,
        _token: &str,
        _project_path: &str,
        _mr_iid: u64,
        _body: &str,
    ) -> Result<(), GitPlatformError> {
        Err(unsupported("add_merge_request_comment"))
    }

    /// Close an issue.
    async fn close_issue(
        &self,
        _token: &str,
        _project_path: &str,
        _iid: u64,
    ) -> Result<(), GitPlatformError> {
        Err(unsupported("close_issue"))
    }

    /// Mark a merge request ready (`draft=false`) or draft.
    async fn set_merge_request_draft(
        &self,
        _token: &str,
        _project_path: &str,
        _mr_iid: u64,
        _draft: bool,
    ) -> Result<(), GitPlatformError> {
        Err(unsupported("set_merge_request_draft"))
    }

    /// Close a merge request without merging.
    async fn close_merge_request(
        &self,
        _token: &str,
        _project_path: &str,
        _mr_iid: u64,
    ) -> Result<(), GitPlatformError> {
        Err(unsupported("close_merge_request"))
    }

    /// Merge with a merge commit (no squash). Returns the merge commit SHA.
    async fn merge_merge_request(
        &self,
        _token: &str,
        _project_path: &str,
        _mr_iid: u64,
    ) -> Result<Option<String>, GitPlatformError> {
        Err(unsupported("merge_merge_request"))
    }

    /// Whether the token's user may merge into the target branch (§10.2 预检).
    async fn can_merge(&self, _token: &str, _project_path: &str) -> Result<bool, GitPlatformError> {
        Err(unsupported("can_merge"))
    }

    /// CI state and the platform's reason the MR cannot merge yet
    /// (P2 design §5 A3).
    async fn merge_readiness(
        &self,
        _token: &str,
        _project_path: &str,
        _mr_iid: u64,
    ) -> Result<MergeReadiness, GitPlatformError> {
        Err(unsupported("merge_readiness"))
    }
}

/// Aggregated CI state of an MR's head commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiState {
    /// No pipeline / checks.
    None,
    Pending,
    Success,
    Failed,
}

/// Why the platform does not (yet) allow merging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeBlocker {
    NotApproved,
    DiscussionsUnresolved,
    Conflict,
    /// Branch must be updated with the target branch.
    Behind,
    Draft,
    /// Required checks not finished / not green.
    CiMustPass,
    /// Branch protection without a more specific reason (GitHub `blocked`).
    Blocked,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReadiness {
    pub ci: CiState,
    pub ci_url: Option<String>,
    pub blocker: Option<MergeBlocker>,
}

impl MergeBlocker {
    /// Hint shown to the person who approved G3.
    pub fn hint(&self) -> String {
        match self {
            Self::NotApproved => "平台要求另一位成员审批，请在 MR 上完成审批".into(),
            Self::DiscussionsUnresolved => "MR 上还有未解决的讨论".into(),
            Self::Conflict => "文档分支与目标分支存在冲突".into(),
            Self::Behind => "文档分支落后于目标分支，需要先更新".into(),
            Self::Draft => "MR 仍是 Draft".into(),
            Self::CiMustPass => "平台要求 CI 通过后才能合并".into(),
            Self::Blocked => "分支保护规则阻止合并（可能需要审批或必需检查）".into(),
            Self::Other(s) => format!("平台返回的合并状态为 {s}"),
        }
    }
}

/// GitLab pipeline status → CI state.
pub fn gitlab_ci_state(status: Option<&str>) -> CiState {
    match status {
        None => CiState::None,
        Some("success" | "skipped" | "manual") => CiState::Success,
        Some("failed" | "canceled" | "cancelled") => CiState::Failed,
        Some(_) => CiState::Pending,
    }
}

/// GitLab `detailed_merge_status` → blocker (`None` = mergeable or still
/// being computed).
pub fn gitlab_blocker(detailed: Option<&str>) -> Option<MergeBlocker> {
    match detailed? {
        "mergeable" | "checking" | "unchecked" | "preparing" | "approvals_syncing" => None,
        "not_approved" | "requested_changes" => Some(MergeBlocker::NotApproved),
        "discussions_not_resolved" => Some(MergeBlocker::DiscussionsUnresolved),
        "conflict" | "broken_status" => Some(MergeBlocker::Conflict),
        "need_rebase" => Some(MergeBlocker::Behind),
        "draft_status" => Some(MergeBlocker::Draft),
        "ci_must_pass" | "ci_still_running" => Some(MergeBlocker::CiMustPass),
        other => Some(MergeBlocker::Other(other.to_string())),
    }
}

/// GitHub combined status + check runs → CI state.
pub fn github_ci_state(combined: &serde_json::Value, check_runs: &serde_json::Value) -> CiState {
    let mut any = false;
    let mut pending = false;
    let mut failed = false;
    if combined["total_count"].as_u64().unwrap_or(0) > 0 {
        any = true;
        match combined["state"].as_str() {
            Some("pending") => pending = true,
            Some("failure" | "error") => failed = true,
            _ => {}
        }
    }
    for run in check_runs["check_runs"].as_array().into_iter().flatten() {
        any = true;
        if run["status"].as_str() != Some("completed") {
            pending = true;
        } else if matches!(
            run["conclusion"].as_str(),
            Some("failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure")
        ) {
            failed = true;
        }
    }
    if failed {
        CiState::Failed
    } else if pending {
        CiState::Pending
    } else if any {
        CiState::Success
    } else {
        CiState::None
    }
}

/// GitHub `mergeable_state` → blocker.
pub fn github_blocker(mergeable_state: Option<&str>) -> Option<MergeBlocker> {
    match mergeable_state? {
        "clean" | "has_hooks" | "unstable" | "unknown" => None,
        "blocked" => Some(MergeBlocker::Blocked),
        "behind" => Some(MergeBlocker::Behind),
        "dirty" => Some(MergeBlocker::Conflict),
        "draft" => Some(MergeBlocker::Draft),
        other => Some(MergeBlocker::Other(other.to_string())),
    }
}

fn unsupported(op: &str) -> GitPlatformError {
    GitPlatformError::RequestError(format!("platform does not support {op}"))
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

#[cfg(test)]
mod readiness_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_gitlab_mappings() {
        assert_eq!(gitlab_ci_state(None), CiState::None);
        for p in [
            "created",
            "pending",
            "running",
            "preparing",
            "scheduled",
            "waiting_for_resource",
        ] {
            assert_eq!(gitlab_ci_state(Some(p)), CiState::Pending, "{p}");
        }
        assert_eq!(gitlab_ci_state(Some("success")), CiState::Success);
        assert_eq!(gitlab_ci_state(Some("manual")), CiState::Success);
        assert_eq!(gitlab_ci_state(Some("failed")), CiState::Failed);
        assert_eq!(gitlab_ci_state(Some("canceled")), CiState::Failed);

        assert_eq!(gitlab_blocker(None), None);
        assert_eq!(gitlab_blocker(Some("mergeable")), None);
        assert_eq!(gitlab_blocker(Some("checking")), None);
        assert_eq!(
            gitlab_blocker(Some("not_approved")),
            Some(MergeBlocker::NotApproved)
        );
        assert_eq!(
            gitlab_blocker(Some("discussions_not_resolved")),
            Some(MergeBlocker::DiscussionsUnresolved)
        );
        assert_eq!(
            gitlab_blocker(Some("conflict")),
            Some(MergeBlocker::Conflict)
        );
        assert_eq!(
            gitlab_blocker(Some("need_rebase")),
            Some(MergeBlocker::Behind)
        );
        assert_eq!(
            gitlab_blocker(Some("ci_must_pass")),
            Some(MergeBlocker::CiMustPass)
        );
        assert_eq!(
            gitlab_blocker(Some("jira_association_missing")),
            Some(MergeBlocker::Other("jira_association_missing".into()))
        );
    }

    #[test]
    fn test_github_ci_state_combines_statuses_and_check_runs() {
        let none = json!({"state": "pending", "total_count": 0});
        assert_eq!(
            github_ci_state(&none, &json!({"check_runs": []})),
            CiState::None
        );
        assert_eq!(
            github_ci_state(&json!({"state": "success", "total_count": 1}), &json!({})),
            CiState::Success
        );
        let running = json!({"check_runs": [
            {"status": "completed", "conclusion": "success"},
            {"status": "in_progress", "conclusion": null}
        ]});
        assert_eq!(github_ci_state(&none, &running), CiState::Pending);
        let failed = json!({"check_runs": [
            {"status": "in_progress", "conclusion": null},
            {"status": "completed", "conclusion": "failure"}
        ]});
        assert_eq!(github_ci_state(&none, &failed), CiState::Failed);
        assert_eq!(
            github_ci_state(&json!({"state": "error", "total_count": 2}), &json!({})),
            CiState::Failed
        );
        assert_eq!(
            github_ci_state(&serde_json::Value::Null, &serde_json::Value::Null),
            CiState::None
        );
    }

    #[test]
    fn test_github_blocker_and_hints() {
        assert_eq!(github_blocker(Some("clean")), None);
        assert_eq!(github_blocker(Some("unstable")), None);
        assert_eq!(github_blocker(Some("blocked")), Some(MergeBlocker::Blocked));
        assert_eq!(github_blocker(Some("behind")), Some(MergeBlocker::Behind));
        assert_eq!(github_blocker(Some("dirty")), Some(MergeBlocker::Conflict));
        assert_eq!(github_blocker(Some("draft")), Some(MergeBlocker::Draft));
        assert!(MergeBlocker::NotApproved.hint().contains("另一位成员审批"));
        assert!(MergeBlocker::Other("x".into()).hint().contains("x"));
    }
}
