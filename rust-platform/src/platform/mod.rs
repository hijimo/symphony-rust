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
    Capability, Comment, CommentId, CreatePrParams, Dispatchable, FetchOptions, Issue, IssueId,
    PullRequest,
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

    // --- Health check ---

    /// Validate that the configured credentials are valid.
    async fn validate_credentials(&self) -> Result<(), PlatformError>;
}
