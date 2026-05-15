use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Platform-native issue ID (newtype to prevent misuse with CommentId).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IssueId(pub u64);

/// Platform-native comment/note ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommentId(pub u64);

impl fmt::Display for IssueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for CommentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Options for fetching candidate issues.
#[derive(Debug, Clone, Default)]
pub struct FetchOptions {
    pub page_size: Option<u32>,
    pub sort: Option<String>,
    pub direction: Option<String>,
}

/// Standardized issue representation across GitHub and GitLab.
#[derive(Debug, Clone, Serialize)]
pub struct Issue {
    pub id: IssueId,
    pub number: u64,
    pub title: String,
    pub description: Option<String>,
    pub url: String,
    pub assignee: Option<String>,
    pub workflow_state: Option<String>,
    pub branch_name: String,
    pub priority: Option<u8>,
    pub labels: Vec<String>,
    pub blocked_by: Vec<IssueId>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// A comment or note on an issue.
#[derive(Debug, Clone)]
pub struct Comment {
    pub id: CommentId,
    pub body: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    /// GitLab distinguishes system notes from user notes.
    pub is_system: bool,
}

/// Parameters for creating a pull request or merge request.
#[derive(Debug, Clone)]
pub struct CreatePrParams {
    pub title: String,
    pub body: String,
    /// Source branch.
    pub head: String,
    /// Target branch.
    pub base: String,
    pub draft: bool,
}

/// Standardized pull request / merge request representation.
#[derive(Debug, Clone, Serialize)]
pub struct PullRequest {
    pub id: u64,
    pub number: u64,
    pub url: String,
    /// "open" | "closed" | "merged"
    pub state: String,
}

/// Platform capabilities that may differ between GitHub and GitLab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// GitLab supports atomic label add+remove in a single PUT.
    AtomicLabels,
    /// GitLab uses merge requests instead of pull requests.
    MergeRequest,
    /// Platform supports webhook-based event delivery.
    Webhook,
}

/// Trait for unified sorting/filtering of issues from different sources.
pub trait Dispatchable: Send + Sync {
    fn id(&self) -> IssueId;
    fn state(&self) -> Option<&str>;
    fn priority(&self) -> Option<u8>;
    fn labels(&self) -> &[String];
    fn created_at(&self) -> Option<DateTime<Utc>>;
    fn is_blocked(&self) -> bool;
}

impl Dispatchable for Issue {
    fn id(&self) -> IssueId {
        self.id
    }

    fn state(&self) -> Option<&str> {
        self.workflow_state.as_deref()
    }

    fn priority(&self) -> Option<u8> {
        self.priority
    }

    fn labels(&self) -> &[String] {
        &self.labels
    }

    fn created_at(&self) -> Option<DateTime<Utc>> {
        self.created_at
    }

    fn is_blocked(&self) -> bool {
        !self.blocked_by.is_empty()
    }
}
