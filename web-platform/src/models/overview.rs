use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{InProgressColumn, PrColumn, TestingColumn, TodoColumn};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectMeta {
    pub project_id: i64,
    pub project_name: String,
    pub platform: String,
    pub namespace: String,
    pub repo_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectIssuesEntry {
    #[serde(flatten)]
    pub meta: ProjectMeta,
    pub todo: TodoColumn,
    pub in_progress: InProgressColumn,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testing: Option<TestingColumn>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectPrsEntry {
    #[serde(flatten)]
    pub meta: ProjectMeta,
    pub pr: PrColumn,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OverviewIssuesResponse {
    pub projects: Vec<ProjectIssuesEntry>,
    pub total_running_projects: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OverviewPrsResponse {
    pub projects: Vec<ProjectPrsEntry>,
    pub total_running_projects: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OverviewQuery {
    pub max_projects: Option<u32>,
    pub todo_limit: Option<u32>,
}

impl OverviewQuery {
    pub fn effective_max_projects(&self) -> u32 {
        self.max_projects.unwrap_or(10).clamp(1, 20)
    }

    pub fn effective_todo_limit(&self) -> u32 {
        self.todo_limit.unwrap_or(5).clamp(1, 10)
    }
}
