use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Service status for a project's Symphony process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ServiceStatus {
    Running,
    Stopped,
    Starting,
    Stopping,
    Error,
    Failed,
}

impl ServiceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServiceStatus::Running => "running",
            ServiceStatus::Stopped => "stopped",
            ServiceStatus::Starting => "starting",
            ServiceStatus::Stopping => "stopping",
            ServiceStatus::Error => "error",
            ServiceStatus::Failed => "failed",
        }
    }

    pub fn parse_or_stopped(s: &str) -> Self {
        match s {
            "running" => ServiceStatus::Running,
            "stopped" => ServiceStatus::Stopped,
            "starting" => ServiceStatus::Starting,
            "stopping" => ServiceStatus::Stopping,
            "error" => ServiceStatus::Error,
            "failed" => ServiceStatus::Failed,
            _ => ServiceStatus::Stopped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceStatus;

    #[test]
    fn service_status_parse_or_stopped_defaults_unknown_values_to_stopped() {
        assert_eq!(
            ServiceStatus::parse_or_stopped("running"),
            ServiceStatus::Running
        );
        assert_eq!(
            ServiceStatus::parse_or_stopped("stopped"),
            ServiceStatus::Stopped
        );
        assert_eq!(
            ServiceStatus::parse_or_stopped("starting"),
            ServiceStatus::Starting
        );
        assert_eq!(
            ServiceStatus::parse_or_stopped("stopping"),
            ServiceStatus::Stopping
        );
        assert_eq!(
            ServiceStatus::parse_or_stopped("error"),
            ServiceStatus::Error
        );
        assert_eq!(
            ServiceStatus::parse_or_stopped("failed"),
            ServiceStatus::Failed
        );
        assert_eq!(
            ServiceStatus::parse_or_stopped("unknown"),
            ServiceStatus::Stopped
        );
    }
}

/// Full project entity as stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub git_url: String,
    pub platform: String,
    pub platform_host: Option<String>,
    pub namespace: String,
    pub repo_name: String,
    pub default_branch: String,
    pub workflow_template: String,
    #[serde(skip_serializing)]
    #[schema(ignore)]
    pub workflow_content: Option<String>,
    pub service_status: String,
    pub service_pid: Option<i64>,
    pub max_concurrent_agents: i64,
    pub auto_restart: bool,
    pub restart_count: i64,
    #[schema(value_type = Option<String>)]
    pub last_started_at: Option<NaiveDateTime>,
    #[schema(value_type = Option<String>)]
    pub last_stopped_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub created_by: Option<i64>,
    #[schema(value_type = String)]
    pub created_at: NaiveDateTime,
    #[schema(value_type = String)]
    pub updated_at: NaiveDateTime,
    /// Computed: number of members in this project
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_count: Option<i64>,
    /// Computed: current user's role in this project
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_role: Option<String>,
    // Workflow hooks/codex configuration
    pub hooks_after_create: Option<String>,
    pub hooks_before_remove: Option<String>,
    pub codex_command: Option<String>,
    pub codex_approval_policy: Option<String>,
    pub codex_sandbox: Option<String>,
    // Resume/recovery lifecycle fencing fields.
    pub web_instance_id: Option<String>,
    pub lifecycle_op_id: Option<String>,
    pub lifecycle_lease_expires_at: Option<String>,
    pub service_owner_web_instance_id: Option<String>,
    pub service_owner_lease_expires_at: Option<String>,
    pub service_owner_heartbeat_at: Option<String>,
    pub service_generation: i64,
    pub service_instance_id: Option<String>,
    pub service_pgid: Option<i64>,
    pub service_session_id: Option<i64>,
    pub service_cmdline_hash: Option<String>,
    pub service_workdir: Option<String>,
    pub last_lifecycle_op: Option<String>,
    pub service_proxy_config_version: Option<String>,
    // Testing agent configuration
    pub testing_enabled: bool,
    pub testing_max_attempts: i64,
    pub testing_max_turns: i64,
    pub testing_skip_labels: Option<String>,
    pub testing_allowed_commands: Option<String>,
    pub testing_service_status: String,
    pub testing_service_pid: Option<i64>,
    pub testing_service_instance_id: Option<String>,
    pub testing_service_generation: i64,
}

/// Data required to create a new project.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NewProject {
    pub name: String,
    pub description: Option<String>,
    pub git_url: String,
    pub platform: String,
    pub platform_host: Option<String>,
    pub namespace: String,
    pub repo_name: String,
    pub default_branch: String,
    pub workflow_template: String,
    pub workflow_content: Option<String>,
    pub created_by: i64,
}

/// Fields that can be updated on a project.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ProjectUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub default_branch: Option<String>,
    pub max_concurrent_agents: Option<i64>,
    pub auto_restart: Option<bool>,
    pub hooks_after_create: Option<String>,
    pub hooks_before_remove: Option<String>,
    pub codex_command: Option<String>,
    pub codex_approval_policy: Option<String>,
    pub codex_sandbox: Option<String>,
    pub testing_enabled: Option<bool>,
    pub testing_max_attempts: Option<i64>,
    pub testing_max_turns: Option<i64>,
    pub testing_skip_labels: Option<String>,
    pub testing_allowed_commands: Option<String>,
}

/// Service status update payload.
#[derive(Debug, Clone)]
pub struct ServiceStatusUpdate {
    pub status: ServiceStatus,
    pub pid: Option<i64>,
    pub error_message: Option<String>,
}

/// Service lifecycle metadata written when a Symphony process is launched.
#[derive(Debug, Clone)]
pub struct ServiceLifecycleUpdate {
    pub web_instance_id: String,
    pub lifecycle_op_id: String,
    pub service_owner_web_instance_id: String,
    pub service_generation: i64,
    pub service_instance_id: String,
    pub service_pgid: Option<i64>,
    pub service_session_id: Option<i64>,
    pub service_cmdline_hash: String,
    pub service_workdir: String,
    pub last_lifecycle_op: String,
    pub service_proxy_config_version: String,
}

/// Project member entity (joined with user info).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectMember {
    pub user_id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,
    pub synced_from: Option<String>,
    #[schema(value_type = String)]
    pub created_at: NaiveDateTime,
}

/// Data for syncing a member from an external platform.
#[derive(Debug, Clone)]
pub struct SyncMember {
    pub username: String,
    pub role: String,
    pub synced_from: String,
}

/// Result of a platform member sync operation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SyncResult {
    pub added: u32,
    pub skipped: u32,
    pub unmatched: Vec<String>,
}

// ==================== Request/Response types for API handlers ====================

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateProjectRequest {
    pub git_url: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub default_branch: Option<String>,
    pub workflow_template: Option<String>,
    pub workflow_content: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub default_branch: Option<String>,
    pub max_concurrent_agents: Option<i64>,
    pub auto_restart: Option<bool>,
    pub hooks_after_create: Option<String>,
    pub hooks_before_remove: Option<String>,
    pub codex_command: Option<String>,
    pub codex_approval_policy: Option<String>,
    pub codex_sandbox: Option<String>,
    pub testing_enabled: Option<bool>,
    pub testing_max_attempts: Option<i64>,
    pub testing_max_turns: Option<i64>,
    pub testing_skip_labels: Option<String>,
    pub testing_allowed_commands: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AddMemberRequest {
    pub user_id: i64,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMemberRoleRequest {
    pub role: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateWorkflowRequest {
    pub template_mode: String,
    pub content: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WorkflowResponse {
    pub template_mode: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectListQuery {
    pub page_no: Option<i64>,
    pub page_size: Option<i64>,
    pub platform: Option<String>,
    pub status: Option<String>,
    pub search: Option<String>,
}
