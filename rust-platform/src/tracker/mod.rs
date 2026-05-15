//! Issue Tracker Client — fetches and normalizes issues from Linear (or Platform adapters).
//!
//! Provides the `Tracker` trait for SPEC-compliant tracker operations and the
//! `LinearClient` implementation for Linear's GraphQL API.

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

    #[error("Linear API request failed: {source}")]
    ApiRequest {
        #[source]
        source: reqwest::Error,
    },

    #[error("Linear API returned status {status}: {body}")]
    ApiStatus { status: u16, body: String },

    #[error("Linear GraphQL errors: {errors:?}")]
    GraphqlErrors { errors: Vec<serde_json::Value> },

    #[error("unexpected Linear API payload: {detail}")]
    UnknownPayload { detail: String },

    #[error("missing endCursor in paginated response")]
    MissingEndCursor,
}
