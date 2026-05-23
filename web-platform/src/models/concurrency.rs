use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ==================== Concurrency Status ====================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConcurrencyStatus {
    pub global_max: i64,
    pub global_active: i64,
    pub utilization_percent: f64,
    pub projects: Vec<ProjectConcurrencyInfo>,
    pub data_freshness_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectConcurrencyInfo {
    pub project_id: i64,
    pub project_name: String,
    pub active_agents: i64,
    pub max_agents: Option<i64>,
    pub queued_tasks: i64,
    pub service_status: String,
}

// ==================== Concurrency Config ====================

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateConcurrencyConfigRequest {
    pub global_max: Option<i64>,
    pub expected_previous: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConcurrencyConfigResponse {
    pub global_max: i64,
    pub previous_value: i64,
}

// ==================== Project Concurrency ====================

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateProjectConcurrencyRequest {
    pub max_agents: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectConcurrencyDetail {
    pub project_id: i64,
    pub project_name: String,
    pub active_agents: i64,
    pub max_agents: Option<i64>,
    pub queued_tasks: i64,
    pub today_started: i64,
    pub today_completed: i64,
    pub avg_duration_seconds: Option<i64>,
}

// ==================== SSE Events ====================

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConcurrencyEvent {
    AgentStarted {
        project_id: i64,
        project_name: String,
        agent_id: String,
        issue_iid: Option<u64>,
        active_agents: i64,
        global_active: i64,
    },
    AgentCompleted {
        project_id: i64,
        project_name: String,
        agent_id: String,
        issue_iid: Option<u64>,
        duration_seconds: i64,
        active_agents: i64,
        global_active: i64,
    },
    ThrottleOn {
        project_id: i64,
        project_name: String,
        reason: String,
        global_active: i64,
        global_max: i64,
    },
    ThrottleOff {
        project_id: i64,
        project_name: String,
        global_active: i64,
        global_max: i64,
    },
    Snapshot {
        global_active: i64,
        global_max: i64,
        projects: Vec<ProjectConcurrencyInfo>,
    },
}

// ==================== SSE Ticket ====================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SseTicketResponse {
    pub ticket: String,
    #[schema(value_type = String)]
    pub expires_at: DateTime<Utc>,
}

// ==================== Contributors ====================

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ContributorsResponse {
    pub contributors: Vec<Contributor>,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Contributor {
    pub username: String,
    pub display_name: String,
    pub avatar_url: String,
    pub recent_issue_count: u64,
    pub recent_mr_count: u64,
    pub is_bot: bool,
    pub logical_author: bool,
}

// ==================== Token Validation ====================

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ValidateTokenRequest {
    pub platform: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ValidateTokenResponse {
    pub valid: bool,
    pub username: Option<String>,
    pub scopes: Vec<String>,
    pub error: Option<String>,
}

// ==================== Concurrency DB Models ====================

#[derive(Debug, Clone)]
pub struct ConcurrencyEventRecord {
    pub id: i64,
    pub project_id: i64,
    pub event_type: String,
    pub agent_id: Option<String>,
    pub issue_iid: Option<i64>,
    pub issue_title: Option<String>,
    pub duration_seconds: Option<i64>,
    pub metadata_json: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ConcurrencySnapshot {
    pub project_id: i64,
    pub active_agents: i64,
    pub queued_tasks: i64,
    pub agents_json: Option<String>,
    pub updated_at: String,
}
