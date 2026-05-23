use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ==================== Kanban Issue (card in the board) ====================

/// Issue card data as displayed in the kanban board.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KanbanIssue {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub labels: Vec<String>,
    pub author: PlatformUser,
    pub assignees: Vec<PlatformUser>,
    pub created_at: String,
    pub updated_at: String,
    pub web_url: String,
    /// Number of associated MRs (only populated for in_progress column)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mr_count: Option<u64>,
}

// ==================== Issue Detail ====================

/// Full issue detail returned by the detail endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueDetail {
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub state: String,
    pub labels: Vec<String>,
    pub author: PlatformUser,
    pub assignees: Vec<PlatformUser>,
    pub milestone: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub web_url: String,
    pub comment_count: u64,
    pub related_mrs: Vec<MergeRequestSummary>,
}

// ==================== Issue Summary (used in MR detail) ====================

/// Abbreviated issue info shown in MR detail's related_issues field.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IssueSummary {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub web_url: String,
}

// ==================== MR Summary (used in Issue detail) ====================

/// Abbreviated MR info shown in issue detail's related_mrs field.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MergeRequestSummary {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub author: PlatformUser,
    pub web_url: String,
}

// ==================== Platform User ====================

/// A user on the git platform (GitLab/GitHub).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PlatformUser {
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}
