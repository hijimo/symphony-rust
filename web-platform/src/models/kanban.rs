use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub use super::issue::{KanbanIssue, PlatformUser};
pub use super::merge_request::KanbanMergeRequest;

// ==================== Kanban Response Structures ====================

/// The full kanban board data with three columns.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KanbanData {
    pub todo: TodoColumn,
    pub in_progress: InProgressColumn,
    pub pr: PrColumn,
    /// Which platform this project uses (gitlab or github)
    pub platform: String,
    /// Whether this response was served from cache
    pub cached: bool,
    /// When the cache entry was created (only present if cached=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_at: Option<String>,
}

/// The "todo" column: open issues without the symphony-claimed label.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TodoColumn {
    pub issues: Vec<KanbanIssue>,
    pub total_count: u64,
    pub has_more: bool,
}

/// The "in progress" column: open issues with the symphony-claimed label.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct InProgressColumn {
    pub issues: Vec<KanbanIssue>,
    pub total_count: u64,
}

/// The "PR" column: merge requests associated with in-progress issues.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PrColumn {
    pub merge_requests: Vec<KanbanMergeRequest>,
    pub total_count: u64,
}

// ==================== Query Parameters ====================

/// Query parameters for the kanban endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct KanbanQuery {
    pub todo_limit: Option<u32>,
    pub assignee: Option<String>,
    pub labels: Option<String>,
    pub search: Option<String>,
    pub no_cache: Option<bool>,
    /// Phase 4: Filter by issue author username
    pub author: Option<String>,
}

impl KanbanQuery {
    /// Parse comma-separated labels into a Vec.
    pub fn parsed_labels(&self) -> Option<Vec<String>> {
        self.labels.as_ref().map(|l| {
            l.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
    }

    /// Get the todo limit, clamped to 1..=100, default 50.
    pub fn effective_todo_limit(&self) -> u32 {
        self.todo_limit.unwrap_or(50).clamp(1, 100)
    }
}

// ==================== AI Generation ====================

/// Request body for AI issue generation.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AIGenerateRequest {
    pub prompt: String,
    pub title: Option<String>,
    pub context: Option<String>,
}

/// SSE event types for AI generation streaming.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SseEvent {
    #[serde(rename = "chunk")]
    Chunk { content: String },
    #[serde(rename = "done")]
    Done { content: String },
    #[serde(rename = "error")]
    Error {
        error: String,
        #[serde(rename = "retCode")]
        ret_code: String,
    },
}

// ==================== Create Issue Request ====================

/// Request body for creating an issue via the platform API.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateIssueApiRequest {
    pub title: String,
    pub description: Option<String>,
    pub labels: Option<Vec<String>>,
    pub assignee: Option<String>,
}

// ==================== Platform-agnostic types used by the trait ====================

/// A platform issue as returned by GitLab/GitHub, normalized to our schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformIssue {
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
    pub comment_count: Option<u64>,
}

/// A platform merge request as returned by GitLab/GitHub, normalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformMergeRequest {
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
    pub reviewers: Vec<PlatformReviewer>,
    pub merge_status: Option<String>,
    pub related_issue_iids: Vec<u64>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub changed_files: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub web_url: String,
}

/// A reviewer on a merge request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformReviewer {
    pub user: PlatformUser,
    pub state: String,
}

/// Request to create an issue on the platform.
#[derive(Debug, Clone)]
pub struct CreateIssueRequest {
    pub title: String,
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub assignee: Option<String>,
}
