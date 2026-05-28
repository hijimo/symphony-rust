use crate::error::Result;
use crate::handlers::admin_config::SystemConfigItem;
use crate::models::alert::{
    AlertHistoryQuery, AlertHistoryRecord, AlertRule, InsertAlertHistory, NotificationChannelRow,
};
use crate::models::concurrency::ConcurrencySnapshot;
use crate::models::{
    NewProject, Project, ProjectMember, ProjectUpdate, ServiceLifecycleUpdate, ServiceStatusUpdate,
    SyncMember, SyncResult, TokenBlacklistEntry, User, UserConfig,
};
use crate::proxy::{ProxySecret, ProxySecretMutation};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy)]
pub struct ProjectListFilter<'a> {
    pub user_id: i64,
    pub is_admin: bool,
    pub page_no: i64,
    pub page_size: i64,
    pub platform: Option<&'a str>,
    pub status: Option<&'a str>,
    pub search: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct ConcurrencyEventInput<'a> {
    pub project_id: i64,
    pub event_type: &'a str,
    pub agent_id: Option<&'a str>,
    pub issue_iid: Option<i64>,
    pub issue_title: Option<&'a str>,
    pub duration_seconds: Option<i64>,
    pub metadata_json: Option<&'a str>,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        display_name: Option<&str>,
        role: &str,
    ) -> Result<User>;
    async fn find_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn find_by_id(&self, id: i64) -> Result<Option<User>>;
    async fn list_users(
        &self,
        page_no: i64,
        page_size: i64,
        search: Option<&str>,
        role_filter: Option<&str>,
    ) -> Result<(Vec<User>, i64)>;
    async fn update_display_name(&self, user_id: i64, display_name: &str) -> Result<()>;
    async fn update_password(&self, user_id: i64, password_hash: &str) -> Result<()>;
    async fn soft_delete(&self, user_id: i64) -> Result<()>;
}

#[async_trait]
pub trait UserConfigRepository: Send + Sync {
    async fn get_config(&self, user_id: i64) -> Result<Option<UserConfig>>;
    async fn upsert_config(
        &self,
        user_id: i64,
        gitlab_token: Option<&str>,
        gitlab_host: Option<&str>,
        github_token: Option<&str>,
    ) -> Result<()>;
}

#[async_trait]
pub trait TokenBlacklistRepository: Send + Sync {
    async fn add_to_blacklist(&self, user_id: i64, reason: &str) -> Result<()>;
    async fn is_blacklisted(
        &self,
        user_id: i64,
        issued_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool>;
    async fn load_all(&self) -> Result<Vec<TokenBlacklistEntry>>;
}

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn create_project(&self, project: &NewProject) -> Result<Project>;
    async fn get_project(&self, id: i64) -> Result<Option<Project>>;
    async fn list_projects_for_user(
        &self,
        filter: ProjectListFilter<'_>,
    ) -> Result<(Vec<Project>, i64)>;
    /// List running projects that the user is a member of (SQL-level JOIN).
    async fn list_running_projects_for_member(
        &self,
        user_id: i64,
        is_admin: bool,
        limit: u32,
    ) -> Result<(Vec<Project>, u64)>;
    async fn update_project(&self, id: i64, updates: &ProjectUpdate) -> Result<()>;
    async fn delete_project(&self, id: i64) -> Result<()>;
    async fn update_service_status(&self, id: i64, status: &ServiceStatusUpdate) -> Result<()>;
    async fn update_testing_service_status(
        &self,
        id: i64,
        status: &ServiceStatusUpdate,
    ) -> Result<()>;
    async fn update_service_lifecycle(
        &self,
        id: i64,
        lifecycle: &ServiceLifecycleUpdate,
    ) -> Result<()>;
    async fn update_workflow(&self, id: i64, template: &str, content: Option<&str>) -> Result<()>;
    async fn get_workflow_content(&self, id: i64) -> Result<Option<(String, Option<String>)>>;
}

#[async_trait]
pub trait ProjectMemberRepository: Send + Sync {
    async fn list_members(&self, project_id: i64) -> Result<Vec<ProjectMember>>;
    async fn add_member(
        &self,
        project_id: i64,
        user_id: i64,
        role: &str,
        synced_from: Option<&str>,
    ) -> Result<ProjectMember>;
    async fn update_member_role(&self, project_id: i64, user_id: i64, role: &str) -> Result<()>;
    async fn remove_member(&self, project_id: i64, user_id: i64) -> Result<()>;
    async fn is_member(&self, project_id: i64, user_id: i64) -> Result<bool>;
    async fn get_member_role(&self, project_id: i64, user_id: i64) -> Result<Option<String>>;
    async fn count_members(&self, project_id: i64) -> Result<i64>;
    async fn count_owners(&self, project_id: i64) -> Result<i64>;
    async fn sync_members(&self, project_id: i64, members: &[SyncMember]) -> Result<SyncResult>;
}

#[async_trait]
pub trait ConcurrencyRepository: Send + Sync {
    async fn record_concurrency_event(&self, input: ConcurrencyEventInput<'_>) -> Result<()>;

    async fn save_snapshot(
        &self,
        project_id: i64,
        active_agents: i64,
        queued_tasks: i64,
        agents_json: Option<&str>,
    ) -> Result<()>;

    async fn load_snapshots(&self) -> Result<Vec<ConcurrencySnapshot>>;

    async fn get_today_stats(&self, project_id: i64) -> Result<(i64, i64, Option<i64>)>;
}

#[async_trait]
pub trait AlertRepository: Send + Sync {
    // --- Alert Rules ---

    /// Get all alert rules.
    async fn get_all_alert_rules(&self) -> Result<Vec<AlertRule>>;

    /// Get a specific alert rule by ID.
    async fn get_alert_rule(&self, rule_id: &str) -> Result<Option<AlertRule>>;

    /// Update an alert rule (partial update).
    async fn update_alert_rule(
        &self,
        rule_id: &str,
        enabled: Option<bool>,
        threshold_json: Option<&str>,
        cooldown_seconds: Option<i64>,
    ) -> Result<()>;

    // --- Notification Channels ---

    /// Get all notification channel rows.
    async fn get_all_notification_channels(&self) -> Result<Vec<NotificationChannelRow>>;

    /// Get a specific notification channel by ID.
    async fn get_notification_channel(
        &self,
        channel_id: &str,
    ) -> Result<Option<NotificationChannelRow>>;

    /// Save notification channels (full replacement).
    async fn save_notification_channels(&self, channels: Vec<NotificationChannelRow>)
        -> Result<()>;

    /// Update channel test result fields.
    async fn update_channel_test_result(
        &self,
        channel_id: &str,
        success: bool,
        tested_at: &str,
    ) -> Result<()>;

    // --- Alert History ---

    /// Insert an alert history record, returning the new row ID.
    async fn insert_alert_history(&self, record: &InsertAlertHistory) -> Result<i64>;

    /// Update the notification status of an alert history record.
    async fn update_alert_notification_status(
        &self,
        id: i64,
        channel: &str,
        status: &str,
        notified_at: &str,
    ) -> Result<()>;

    /// Query alert history with pagination and filters.
    async fn query_alert_history(
        &self,
        query: &AlertHistoryQuery,
    ) -> Result<(Vec<AlertHistoryRecord>, i64)>;

    /// Clean up alert history older than retention_days.
    async fn cleanup_alert_history(&self, retention_days: i64) -> Result<u64>;

    // --- Cooldown Persistence ---

    /// Save a cooldown entry (upsert).
    async fn save_cooldown(
        &self,
        rule_id: &str,
        scope_key: &str,
        last_fired_at: &str,
        expires_at: &str,
    ) -> Result<()>;

    /// Load all active (non-expired) cooldown entries.
    async fn load_active_cooldowns(&self) -> Result<Vec<(String, String, String)>>;

    /// Delete expired cooldown entries, returning the count removed.
    async fn cleanup_expired_cooldowns(&self) -> Result<u64>;
}

#[async_trait]
pub trait SystemConfigRepository: Send + Sync {
    /// List all system config entries.
    async fn list_system_configs(&self) -> Result<Vec<SystemConfigItem>>;

    /// Update multiple system config entries (upsert).
    async fn update_system_configs(&self, configs: &[(&str, &str)]) -> Result<()>;

    /// Get global stats: (total_projects, running_services, total_users).
    async fn get_system_stats(&self) -> Result<(i64, i64, i64)>;
}

#[async_trait]
pub trait NetworkProxyRepository: Send + Sync {
    async fn get_proxy_secret(&self, key: &str) -> Result<Option<ProxySecret>>;
    async fn upsert_proxy_secret(&self, key: &str, encrypted_value: &str, kind: &str)
        -> Result<()>;
    async fn delete_proxy_secret(&self, key: &str) -> Result<()>;
    async fn update_network_proxy_config(
        &self,
        expected_version: &str,
        configs: Vec<(String, String)>,
        secret_mutations: Vec<ProxySecretMutation>,
    ) -> Result<()>;
    async fn count_running_services_with_stale_proxy_version(
        &self,
        current_version: &str,
    ) -> Result<i64>;
    async fn current_network_proxy_version(&self) -> Result<String>;
}
