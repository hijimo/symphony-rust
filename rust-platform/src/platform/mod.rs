pub mod cooldown_queue;
pub mod gitea;
pub mod github;
pub mod gitlab;
pub mod http_client;
pub mod issue;
pub mod memory;
pub mod retry;
pub mod workflow;

use async_trait::async_trait;

pub use issue::{
    Capability, Comment, CommentId, CommentMode, CreatePrParams, Dispatchable, FetchOptions, Issue,
    IssueId, PullRequest, PullRequestRef, RepoFile,
};
pub use memory::{make_test_issue, FaultConfig, MemoryAdapter, MemoryState};

use crate::error::PlatformError;

/// Unified platform adapter supporting GitHub/GitLab issue and PR operations.
/// Use `Arc<dyn Platform>` to share across multiple tokio tasks.
#[async_trait]
pub trait Platform: Send + Sync {
    // --- Capability discovery ---

    /// Returns the set of capabilities supported by this adapter.
    fn capabilities(&self) -> Vec<Capability>;

    // --- Issue operations ---

    /// Fetch issues matching the configured active workflow states.
    async fn fetch_candidate_issues(&self, opts: FetchOptions)
        -> Result<Vec<Issue>, PlatformError>;

    /// Fetch a single issue by its platform-native ID.
    async fn fetch_issue(&self, issue_id: IssueId) -> Result<Issue, PlatformError>;

    /// Fetch multiple issues by ID (for state revalidation).
    async fn fetch_issue_states_by_ids(&self, ids: &[IssueId])
        -> Result<Vec<Issue>, PlatformError>;

    // --- Workflow state (label-based) ---

    /// Get the current workflow state label for an issue.
    async fn get_workflow_state(&self, issue_id: IssueId) -> Result<Option<String>, PlatformError>;

    /// Set the workflow state by swapping labels (add new, remove old).
    async fn set_workflow_state(&self, issue_id: IssueId, state: &str)
        -> Result<(), PlatformError>;

    // --- Label operations (no-op on empty slice) ---

    /// Add labels to an issue.
    async fn add_labels(&self, issue_id: IssueId, labels: &[String]) -> Result<(), PlatformError>;

    /// Remove labels from an issue.
    async fn remove_labels(
        &self,
        issue_id: IssueId,
        labels: &[String],
    ) -> Result<(), PlatformError>;

    // --- Comment / Workpad ---

    /// Create a new comment on an issue.
    async fn create_comment(
        &self,
        issue_id: IssueId,
        body: &str,
    ) -> Result<CommentId, PlatformError>;

    /// Update an existing comment.
    async fn update_comment(&self, comment_id: CommentId, body: &str) -> Result<(), PlatformError>;

    /// Find the workpad comment (contains "## Codex Workpad") on an issue.
    async fn find_workpad_comment(
        &self,
        issue_id: IssueId,
    ) -> Result<Option<(CommentId, String)>, PlatformError>;

    /// List all comments on an issue.
    async fn list_comments(&self, issue_id: IssueId) -> Result<Vec<Comment>, PlatformError>;

    // --- PR/MR ---

    /// Create a pull request or merge request.
    async fn create_pull_request(
        &self,
        params: CreatePrParams,
    ) -> Result<PullRequest, PlatformError>;

    /// Find the open PR/MR whose source branch is `branch` in `repo_slug`.
    /// Returns `None` when there is no open PR/MR for the branch.
    async fn find_open_pull_request(
        &self,
        repo_slug: &str,
        branch: &str,
    ) -> Result<Option<PullRequestRef>, PlatformError>;

    /// List user comments on a PR/MR (system notes excluded).
    async fn list_pull_request_comments(
        &self,
        pr: &PullRequestRef,
    ) -> Result<Vec<Comment>, PlatformError>;

    /// Write a comment on a PR/MR identified by `marker`.
    ///
    /// The stored body always contains `marker` (appended when missing).
    /// `Upsert` updates the first comment containing the marker, `Append`
    /// always creates a new one.
    async fn upsert_pull_request_comment(
        &self,
        pr: &PullRequestRef,
        marker: &str,
        body: &str,
        mode: CommentMode,
    ) -> Result<CommentId, PlatformError>;

    // --- Repository contents ---

    /// List the files (not subdirectories) directly inside `path` on `git_ref`,
    /// sorted by name. Returns `None` when the directory does not exist, is
    /// empty, or `path` is a file.
    async fn list_directory(
        &self,
        repo_slug: &str,
        path: &str,
        git_ref: &str,
    ) -> Result<Option<Vec<RepoFile>>, PlatformError>;

    // --- Health check ---

    /// Validate that the configured credentials are valid.
    async fn validate_credentials(&self) -> Result<(), PlatformError>;
}

/// Return `body` with `marker` guaranteed to be present.
pub(crate) fn body_with_marker(body: &str, marker: &str) -> String {
    if marker.is_empty() || body.contains(marker) {
        body.to_string()
    } else {
        format!("{}\n\n{}", body.trim_end(), marker)
    }
}

/// Validate an `owner/repo` slug (GitHub, Gitea) before it is placed in a URL path.
pub(crate) fn split_owner_repo(slug: &str) -> Result<(&str, &str), PlatformError> {
    let valid_segment = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    match slug.split_once('/') {
        Some((owner, repo)) if valid_segment(owner) && valid_segment(repo) => Ok((owner, repo)),
        _ => Err(PlatformError::Unprocessable(format!(
            "invalid repository slug: {slug:?}"
        ))),
    }
}

/// Validate a repository-relative path and percent-encode each segment for use
/// in a URL path. A trailing `/` is ignored.
pub(crate) fn encode_repo_path(path: &str) -> Result<String, PlatformError> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    let invalid = || PlatformError::Unprocessable(format!("invalid repository path: {path:?}"));
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return Err(invalid());
    }
    let mut out = Vec::new();
    for seg in trimmed.split('/') {
        if seg.is_empty()
            || seg == "."
            || seg == ".."
            || seg
                .chars()
                .any(|c| c.is_control() || matches!(c, '?' | '#' | '\\'))
        {
            return Err(invalid());
        }
        out.push(urlencoding::encode(seg).into_owned());
    }
    Ok(out.join("/"))
}

/// Parse a GitHub/Gitea contents API response. A JSON object means `path`
/// was a file, so it is not a directory listing.
pub(crate) fn parse_contents_listing(value: serde_json::Value) -> Option<Vec<RepoFile>> {
    let items = value.as_array()?;
    let files = items
        .iter()
        .filter(|i| i.get("type").and_then(|t| t.as_str()) == Some("file"))
        .filter_map(|i| {
            Some(RepoFile {
                name: i.get("name")?.as_str()?.to_string(),
                path: i.get("path")?.as_str()?.to_string(),
                blob_sha: i.get("sha")?.as_str()?.to_string(),
            })
        })
        .collect();
    sorted_files(files)
}

/// Sort a directory listing by name; an empty listing becomes `None`.
pub(crate) fn sorted_files(mut files: Vec<RepoFile>) -> Option<Vec<RepoFile>> {
    if files.is_empty() {
        return None;
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Some(files)
}

/// Validate a GitLab project path (`group/sub/project`) before URL-encoding it.
pub(crate) fn validate_project_path(slug: &str) -> Result<(), PlatformError> {
    let ok = !slug.is_empty()
        && slug.split('/').all(|s| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        });
    if ok {
        Ok(())
    } else {
        Err(PlatformError::Unprocessable(format!(
            "invalid project path: {slug:?}"
        )))
    }
}

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn test_encode_repo_path() {
        assert_eq!(
            encode_repo_path("docs/changes/7").unwrap(),
            "docs/changes/7"
        );
        assert_eq!(
            encode_repo_path("docs/方案").unwrap(),
            "docs/%E6%96%B9%E6%A1%88"
        );
        assert_eq!(encode_repo_path("docs/").unwrap(), "docs");
        for bad in [
            "", "/", "/etc", "a/../b", "a//b", "./a", "a/b?x", "a#b", "a\\b",
        ] {
            assert!(encode_repo_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn test_sorted_files() {
        let f = |n: &str| RepoFile {
            name: n.to_string(),
            path: n.to_string(),
            blob_sha: String::new(),
        };
        assert_eq!(sorted_files(vec![]), None);
        assert_eq!(
            sorted_files(vec![f("b"), f("a")]).unwrap(),
            vec![f("a"), f("b")]
        );
    }

    #[test]
    fn test_body_with_marker_appends_once() {
        assert_eq!(body_with_marker("a\n", "<!-- m -->"), "a\n\n<!-- m -->");
        assert_eq!(
            body_with_marker("a <!-- m -->", "<!-- m -->"),
            "a <!-- m -->"
        );
        assert_eq!(body_with_marker("a", ""), "a");
    }

    #[test]
    fn test_split_owner_repo_accepts_and_rejects() {
        assert_eq!(split_owner_repo("acme/app.js").unwrap(), ("acme", "app.js"));
        for bad in [
            "acme", "a/b/c", "../x", "a/..", "a/b c", "/b", "a/", "a/b?x=1",
        ] {
            assert!(split_owner_repo(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn test_validate_project_path() {
        assert!(validate_project_path("group/sub/proj").is_ok());
        assert!(validate_project_path("proj").is_ok());
        for bad in ["", "a//b", "a/../b", "a/b c", "a/b#x"] {
            assert!(validate_project_path(bad).is_err(), "{bad}");
        }
    }
}
