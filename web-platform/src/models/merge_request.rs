use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::issue::{IssueSummary, PlatformUser};

// ==================== Kanban MR (card in the PR column) ====================

/// MR/PR card data as displayed in the kanban board's PR column.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KanbanMergeRequest {
    pub iid: u64,
    pub title: String,
    pub state: String,
    pub author: PlatformUser,
    pub source_branch: String,
    pub target_branch: String,
    pub ci_status: Option<String>,
    pub review_status: Option<String>,
    pub related_issue_iids: Vec<u64>,
    pub created_at: String,
    pub updated_at: String,
    pub web_url: String,
}

// ==================== MR/PR Detail ====================

/// Full merge request detail returned by the detail endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MergeRequestDetail {
    pub iid: u64,
    pub title: String,
    pub description: Option<String>,
    pub state: String,
    pub author: PlatformUser,
    pub source_branch: String,
    pub target_branch: String,
    pub ci_status: Option<String>,
    pub ci_web_url: Option<String>,
    pub review_status: Option<String>,
    pub reviewers: Vec<Reviewer>,
    pub merge_status: Option<String>,
    pub related_issues: Vec<IssueSummary>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub changed_files: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub web_url: String,
}

// ==================== Reviewer ====================

/// A reviewer and their review state on a merge request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Reviewer {
    pub user: PlatformUser,
    pub state: String,
}

// ==================== MR/PR Creation ====================

/// Request body for creating a merge request / pull request.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMergeRequestApiRequest {
    pub source_branch: String,
    pub target_branch: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub purpose_type: Option<String>,
    pub purpose_id: Option<String>,
    pub draft: Option<bool>,
}
