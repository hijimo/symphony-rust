//! Issue Tracker Client — fetches and normalizes issues from Linear (or Platform adapters).
//!
//! Provides the `Tracker` trait for SPEC-compliant tracker operations and the
//! `LinearClient` implementation for Linear's GraphQL API.

pub mod gitlab;
pub mod linear;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Normalized issue model (SPEC Section 4.1.1).
///
/// This is the canonical issue representation used by the orchestrator,
/// prompt engine, and observability layer. It is distinct from the
/// platform-specific `Issue` in `crate::platform::issue`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerIssue {
    /// Stable tracker-internal ID (Linear UUID / GitHub number as string).
    pub id: String,
    /// Human-readable identifier (e.g. "ABC-123").
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    /// Priority: lower numbers are higher priority; None sorts last.
    pub priority: Option<i32>,
    /// Current tracker state name.
    pub state: String,
    pub branch_name: Option<String>,
    pub url: Option<String>,
    /// Labels (normalized to lowercase).
    pub labels: Vec<String>,
    pub blocked_by: Vec<BlockerRef>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// Blocker reference for dependency tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}

/// SPEC-native Tracker trait (SPEC Section 11.1).
///
/// Implementations provide the three required operations for issue tracking:
/// candidate fetch, state-based fetch, and ID-based state refresh.
#[async_trait]
pub trait Tracker: Send + Sync {
    /// Fetch candidate issues in configured active states (SPEC Section 11.1.1).
    async fn fetch_candidate_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError>;

    /// Fetch issues by state names — used for startup terminal cleanup (SPEC Section 11.1.2).
    async fn fetch_issues_by_states(
        &self,
        states: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError>;

    /// Fetch current states for specific issue IDs — used for reconciliation (SPEC Section 11.1.3).
    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError>;

    /// Set the workflow state label on an issue (e.g., transition to "Done").
    /// Used by the orchestrator to persist terminal state after issue closure.
    /// Default implementation is a no-op for trackers that don't support label writes.
    async fn set_workflow_state(&self, _issue_id: &str, _state: &str) -> Result<(), TrackerError> {
        Ok(())
    }

    /// Whether workpad / label operations below are supported.
    fn supports_workpad(&self) -> bool {
        false
    }

    /// Read the workpad comment: `(comment_id, body)`.
    async fn fetch_workpad(&self, _issue_id: &str) -> Result<Option<(u64, String)>, TrackerError> {
        Err(TrackerError::Unsupported("workpad".to_string()))
    }

    /// Replace the body of the workpad comment.
    async fn update_workpad(&self, _comment_id: u64, _body: &str) -> Result<(), TrackerError> {
        Err(TrackerError::Unsupported("workpad".to_string()))
    }

    /// Add plain (non-workflow) labels.
    async fn add_labels(&self, _issue_id: &str, _labels: &[String]) -> Result<(), TrackerError> {
        Err(TrackerError::Unsupported("labels".to_string()))
    }

    /// Remove plain (non-workflow) labels.
    async fn remove_labels(&self, _issue_id: &str, _labels: &[String]) -> Result<(), TrackerError> {
        Err(TrackerError::Unsupported("labels".to_string()))
    }

    /// List files directly inside `path` of repository `slug` on `git_ref`
    /// (empty = default branch). `None` when the directory does not exist.
    async fn list_repo_directory(
        &self,
        _slug: &str,
        _path: &str,
        _git_ref: &str,
    ) -> Result<Option<Vec<crate::platform::RepoFile>>, TrackerError> {
        Err(TrackerError::Unsupported("repository listing".to_string()))
    }

    /// Open PR/MR for `branch` in repository `slug`.
    async fn find_open_pull_request(
        &self,
        _slug: &str,
        _branch: &str,
    ) -> Result<Option<crate::platform::PullRequestRef>, TrackerError> {
        Err(TrackerError::Unsupported("pull requests".to_string()))
    }

    /// Write a marker-identified PR/MR comment (see `Platform::upsert_pull_request_comment`).
    async fn upsert_pull_request_comment(
        &self,
        _pr: &crate::platform::PullRequestRef,
        _marker: &str,
        _body: &str,
        _mode: crate::platform::CommentMode,
    ) -> Result<(), TrackerError> {
        Err(TrackerError::Unsupported("pull requests".to_string()))
    }
}

/// Unified issue source routing (Linear tracker vs Platform adapter).
pub enum IssueSource {
    Linear(std::sync::Arc<dyn Tracker>),
    Platform(std::sync::Arc<dyn crate::platform::Platform>),
}

/// Tracker error categories (SPEC Section 11.4).
#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("unsupported tracker kind: {0}")]
    UnsupportedTrackerKind(String),

    #[error("missing tracker API key")]
    MissingApiKey,

    #[error("missing tracker project slug")]
    MissingProjectSlug,

    #[error("Tracker API request failed: {source}")]
    ApiRequest {
        #[source]
        source: reqwest::Error,
    },

    #[error("Tracker API returned status {status}: {body}")]
    ApiStatus { status: u16, body: String },

    #[error("Tracker GraphQL errors: {errors:?}")]
    GraphqlErrors { errors: Vec<serde_json::Value> },

    #[error("unexpected Tracker API payload: {detail}")]
    UnknownPayload { detail: String },

    #[error("missing endCursor in paginated response")]
    MissingEndCursor,

    #[error("operation not supported by this tracker: {0}")]
    Unsupported(String),
}
