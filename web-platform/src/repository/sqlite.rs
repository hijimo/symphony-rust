use std::collections::HashMap;

use async_trait::async_trait;
use chrono::NaiveDateTime;
use rusqlite::OptionalExtension;

use crate::db::DbPool;
use crate::error::{Result, WebPlatformError};
use crate::handlers::admin_config::SystemConfigItem;
use crate::models::alert::{
    AlertHistoryQuery, AlertHistoryRecord, AlertRule, InsertAlertHistory, NotificationChannelRow,
};
use crate::models::concurrency::ConcurrencySnapshot;
use crate::models::{
    NewProject, Project, ProjectMember, ProjectUpdate, ServiceStatusUpdate, SyncMember, SyncResult,
    TokenBlacklistEntry, User, UserConfig,
};
use crate::proxy::{ProxySecret, ProxySecretMutation, VERSION_KEY};
use crate::repository::{
    AlertRepository, ConcurrencyEventInput, ConcurrencyRepository, NetworkProxyRepository,
    ProjectListFilter, ProjectMemberRepository, ProjectRepository, SystemConfigRepository,
    TokenBlacklistRepository, UserConfigRepository, UserRepository,
};

#[derive(Clone)]
pub struct SqliteRepository {
    pool: DbPool,
}

impl SqliteRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub(crate) fn pool(&self) -> DbPool {
        self.pool.clone()
    }
}

fn parse_datetime(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .unwrap_or_else(|_| NaiveDateTime::default())
}

fn row_to_user(row: &rusqlite::Row) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        password_hash: row.get(2)?,
        display_name: row.get(3)?,
        role: row.get(4)?,
        deleted_at: row.get::<_, Option<String>>(5)?.map(|s| parse_datetime(&s)),
        created_at: parse_datetime(&row.get::<_, String>(6)?),
        updated_at: parse_datetime(&row.get::<_, String>(7)?),
    })
}

#[async_trait]
impl UserRepository for SqliteRepository {
    async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        display_name: Option<&str>,
        role: &str,
    ) -> Result<User> {
        let pool = self.pool.clone();
        let username = username.to_string();
        let password_hash = password_hash.to_string();
        let display_name = display_name.map(|s| s.to_string());
        let role = role.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO users (username, password_hash, display_name, role) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![username, password_hash, display_name, role],
            ).map_err(|e| match e {
                rusqlite::Error::SqliteFailure(ref err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    WebPlatformError::Conflict(format!("Username '{}' already exists", username))
                }
                _ => WebPlatformError::Database(e),
            })?;

            let id = conn.last_insert_rowid();
            let user = conn.query_row(
                "SELECT id, username, password_hash, display_name, role, deleted_at, created_at, updated_at FROM users WHERE id = ?1",
                [id],
                row_to_user,
            )?;
            Ok(user)
        }).await.unwrap()
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        let pool = self.pool.clone();
        let username = username.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn.query_row(
                "SELECT id, username, password_hash, display_name, role, deleted_at, created_at, updated_at FROM users WHERE username = ?1 AND deleted_at IS NULL",
                [&username],
                row_to_user,
            );
            match result {
                Ok(user) => Ok(Some(user)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        }).await.unwrap()
    }

    async fn find_by_id(&self, id: i64) -> Result<Option<User>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn.query_row(
                "SELECT id, username, password_hash, display_name, role, deleted_at, created_at, updated_at FROM users WHERE id = ?1 AND deleted_at IS NULL",
                [id],
                row_to_user,
            );
            match result {
                Ok(user) => Ok(Some(user)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        }).await.unwrap()
    }

    async fn list_users(
        &self,
        page_no: i64,
        page_size: i64,
        search: Option<&str>,
        role_filter: Option<&str>,
    ) -> Result<(Vec<User>, i64)> {
        let pool = self.pool.clone();
        let search = search.map(|s| s.to_string());
        let role_filter = role_filter.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let offset = (page_no - 1) * page_size;

            let mut conditions = vec!["deleted_at IS NULL".to_string()];
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref s) = search {
                conditions.push("(username LIKE ?1 OR display_name LIKE ?1)".to_string());
                params.push(Box::new(format!("%{}%", s)));
            }

            if let Some(ref role) = role_filter {
                let idx = params.len() + 1;
                conditions.push(format!("role = ?{}", idx));
                params.push(Box::new(role.to_string()));
            }

            let where_clause = conditions.join(" AND ");

            let count_sql = format!("SELECT COUNT(*) FROM users WHERE {}", where_clause);
            let total: i64 = conn.query_row(
                &count_sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                |row| row.get(0),
            )?;

            let query_sql = format!(
                "SELECT id, username, password_hash, display_name, role, deleted_at, created_at, updated_at FROM users WHERE {} ORDER BY id DESC LIMIT ?{} OFFSET ?{}",
                where_clause,
                params.len() + 1,
                params.len() + 2
            );
            params.push(Box::new(page_size));
            params.push(Box::new(offset));

            let mut stmt = conn.prepare(&query_sql)?;
            let users = stmt
                .query_map(
                    rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                    row_to_user,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            Ok((users, total))
        }).await.unwrap()
    }

    async fn update_display_name(&self, user_id: i64, display_name: &str) -> Result<()> {
        let pool = self.pool.clone();
        let display_name = display_name.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "UPDATE users SET display_name = ?1, updated_at = datetime('now') WHERE id = ?2 AND deleted_at IS NULL",
                rusqlite::params![display_name, user_id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("User not found".to_string()));
            }
            Ok(())
        }).await.unwrap()
    }

    async fn update_password(&self, user_id: i64, password_hash: &str) -> Result<()> {
        let pool = self.pool.clone();
        let password_hash = password_hash.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "UPDATE users SET password_hash = ?1, updated_at = datetime('now') WHERE id = ?2 AND deleted_at IS NULL",
                rusqlite::params![password_hash, user_id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("User not found".to_string()));
            }
            Ok(())
        }).await.unwrap()
    }

    async fn soft_delete(&self, user_id: i64) -> Result<()> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "UPDATE users SET deleted_at = datetime('now'), updated_at = datetime('now') WHERE id = ?1 AND deleted_at IS NULL",
                [user_id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("User not found".to_string()));
            }
            Ok(())
        }).await.unwrap()
    }
}

#[async_trait]
impl UserConfigRepository for SqliteRepository {
    async fn get_config(&self, user_id: i64) -> Result<Option<UserConfig>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn.query_row(
                "SELECT id, user_id, gitlab_token, gitlab_host, github_token, updated_at FROM user_configs WHERE user_id = ?1",
                [user_id],
                |row| {
                    Ok(UserConfig {
                        id: row.get(0)?,
                        user_id: row.get(1)?,
                        gitlab_token: row.get(2)?,
                        gitlab_host: row.get(3)?,
                        github_token: row.get(4)?,
                        updated_at: parse_datetime(&row.get::<_, String>(5)?),
                    })
                },
            );
            match result {
                Ok(config) => Ok(Some(config)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        }).await.unwrap()
    }

    async fn upsert_config(
        &self,
        user_id: i64,
        gitlab_token: Option<&str>,
        gitlab_host: Option<&str>,
        github_token: Option<&str>,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let gitlab_token = gitlab_token.map(|s| s.to_string());
        let gitlab_host = gitlab_host.map(|s| s.to_string());
        let github_token = github_token.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO user_configs (user_id, gitlab_token, gitlab_host, github_token)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(user_id) DO UPDATE SET
                    gitlab_token = COALESCE(?2, gitlab_token),
                    gitlab_host = COALESCE(?3, gitlab_host),
                    github_token = COALESCE(?4, github_token),
                    updated_at = datetime('now')",
                rusqlite::params![user_id, gitlab_token, gitlab_host, github_token],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }
}

#[async_trait]
impl TokenBlacklistRepository for SqliteRepository {
    async fn add_to_blacklist(&self, user_id: i64, reason: &str) -> Result<()> {
        let pool = self.pool.clone();
        let reason = reason.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO token_blacklist (user_id, reason) VALUES (?1, ?2)",
                rusqlite::params![user_id, reason],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn is_blacklisted(
        &self,
        user_id: i64,
        issued_before: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let timestamp = issued_before.format("%Y-%m-%d %H:%M:%S").to_string();
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM token_blacklist WHERE user_id = ?1 AND invalidated_at >= ?2",
                rusqlite::params![user_id, timestamp],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
        .await
        .unwrap()
    }

    async fn load_all(&self) -> Result<Vec<TokenBlacklistEntry>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut stmt = conn.prepare(
                "SELECT id, user_id, invalidated_at, reason FROM token_blacklist ORDER BY id DESC",
            )?;
            let entries = stmt
                .query_map([], |row| {
                    Ok(TokenBlacklistEntry {
                        id: row.get(0)?,
                        user_id: row.get(1)?,
                        invalidated_at: parse_datetime(&row.get::<_, String>(2)?),
                        reason: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(entries)
        })
        .await
        .unwrap()
    }
}

// ==================== Project Repository ====================

fn row_to_project(row: &rusqlite::Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        git_url: row.get(3)?,
        platform: row.get(4)?,
        platform_host: row.get(5)?,
        namespace: row.get(6)?,
        repo_name: row.get(7)?,
        default_branch: row.get(8)?,
        workflow_template: row.get(9)?,
        workflow_content: row.get(10)?,
        service_status: row.get(11)?,
        service_pid: row.get(12)?,
        max_concurrent_agents: row.get(13)?,
        auto_restart: row.get::<_, i64>(14)? != 0,
        restart_count: row.get(15)?,
        last_started_at: row
            .get::<_, Option<String>>(16)?
            .map(|s| parse_datetime(&s)),
        last_stopped_at: row
            .get::<_, Option<String>>(17)?
            .map(|s| parse_datetime(&s)),
        error_message: row.get(18)?,
        created_by: row.get(19)?,
        created_at: parse_datetime(&row.get::<_, String>(20)?),
        updated_at: parse_datetime(&row.get::<_, String>(21)?),
        member_count: None,
        my_role: None,
        hooks_after_create: row.get(22)?,
        hooks_before_remove: row.get(23)?,
        codex_command: row.get(24)?,
        codex_approval_policy: row.get(25)?,
        codex_sandbox: row.get(26)?,
        web_instance_id: row.get(27)?,
        lifecycle_op_id: row.get(28)?,
        lifecycle_lease_expires_at: row.get(29)?,
        service_owner_web_instance_id: row.get(30)?,
        service_owner_lease_expires_at: row.get(31)?,
        service_owner_heartbeat_at: row.get(32)?,
        service_generation: row.get(33)?,
        service_instance_id: row.get(34)?,
        service_pgid: row.get(35)?,
        service_session_id: row.get(36)?,
        service_cmdline_hash: row.get(37)?,
        service_workdir: row.get(38)?,
        last_lifecycle_op: row.get(39)?,
        service_proxy_config_version: row.get(40)?,
    })
}

const PROJECT_COLUMNS: &str = "id, name, description, git_url, platform, platform_host, namespace, repo_name, default_branch, workflow_template, workflow_content, service_status, service_pid, max_concurrent_agents, auto_restart, restart_count, last_started_at, last_stopped_at, error_message, created_by, created_at, updated_at, hooks_after_create, hooks_before_remove, codex_command, codex_approval_policy, codex_sandbox, web_instance_id, lifecycle_op_id, lifecycle_lease_expires_at, service_owner_web_instance_id, service_owner_lease_expires_at, service_owner_heartbeat_at, service_generation, service_instance_id, service_pgid, service_session_id, service_cmdline_hash, service_workdir, last_lifecycle_op, service_proxy_config_version";

#[async_trait]
impl ProjectRepository for SqliteRepository {
    async fn create_project(&self, project: &NewProject) -> Result<Project> {
        let pool = self.pool.clone();
        let project = project.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO projects (name, description, git_url, platform, platform_host, namespace, repo_name, default_branch, workflow_template, workflow_content, created_by)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    project.name,
                    project.description,
                    project.git_url,
                    project.platform,
                    project.platform_host,
                    project.namespace,
                    project.repo_name,
                    project.default_branch,
                    project.workflow_template,
                    project.workflow_content,
                    project.created_by,
                ],
            ).map_err(|e| match e {
                rusqlite::Error::SqliteFailure(ref err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    WebPlatformError::Conflict(format!(
                        "Project with git_url '{}' already exists",
                        project.git_url
                    ))
                }
                _ => WebPlatformError::Database(e),
            })?;

            let id = conn.last_insert_rowid();
            let sql = format!(
                "SELECT {} FROM projects WHERE id = ?1",
                PROJECT_COLUMNS
            );
            let p = conn.query_row(&sql, [id], row_to_project)?;
            Ok(p)
        })
        .await
        .unwrap()
    }

    async fn get_project(&self, id: i64) -> Result<Option<Project>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let sql = format!("SELECT {} FROM projects WHERE id = ?1", PROJECT_COLUMNS);
            let result = conn.query_row(&sql, [id], row_to_project);
            match result {
                Ok(p) => Ok(Some(p)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn list_projects_for_user(
        &self,
        filter: ProjectListFilter<'_>,
    ) -> Result<(Vec<Project>, i64)> {
        let pool = self.pool.clone();
        let user_id = filter.user_id;
        let is_admin = filter.is_admin;
        let page_no = filter.page_no;
        let page_size = filter.page_size;
        let platform = filter.platform.map(|s| s.to_string());
        let status = filter.status.map(|s| s.to_string());
        let search = filter.search.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let offset = (page_no - 1) * page_size;

            let mut conditions: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if !is_admin {
                let idx = params.len() + 1;
                conditions.push(format!(
                    "p.id IN (SELECT project_id FROM project_members WHERE user_id = ?{})",
                    idx
                ));
                params.push(Box::new(user_id));
            }

            if let Some(ref plat) = platform {
                let idx = params.len() + 1;
                conditions.push(format!("p.platform = ?{}", idx));
                params.push(Box::new(plat.clone()));
            }

            if let Some(ref st) = status {
                let idx = params.len() + 1;
                conditions.push(format!("p.service_status = ?{}", idx));
                params.push(Box::new(st.clone()));
            }

            if let Some(ref s) = search {
                let idx = params.len() + 1;
                conditions.push(format!(
                    "(p.name LIKE ?{} OR p.git_url LIKE ?{})",
                    idx, idx
                ));
                params.push(Box::new(format!("%{}%", s)));
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            // Count
            let count_sql = format!("SELECT COUNT(*) FROM projects p {}", where_clause);
            let total: i64 = conn.query_row(
                &count_sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                |row| row.get(0),
            )?;

            // Query with member_count subquery
            let query_sql = format!(
                "SELECT p.id, p.name, p.description, p.git_url, p.platform, p.platform_host, p.namespace, p.repo_name, p.default_branch, p.workflow_template, p.workflow_content, p.service_status, p.service_pid, p.max_concurrent_agents, p.auto_restart, p.restart_count, p.last_started_at, p.last_stopped_at, p.error_message, p.created_by, p.created_at, p.updated_at, p.hooks_after_create, p.hooks_before_remove, p.codex_command, p.codex_approval_policy, p.codex_sandbox, p.web_instance_id, p.lifecycle_op_id, p.lifecycle_lease_expires_at, p.service_owner_web_instance_id, p.service_owner_lease_expires_at, p.service_owner_heartbeat_at, p.service_generation, p.service_instance_id, p.service_pgid, p.service_session_id, p.service_cmdline_hash, p.service_workdir, p.last_lifecycle_op, p.service_proxy_config_version, \
                 (SELECT COUNT(*) FROM project_members pm WHERE pm.project_id = p.id) as member_count, \
                 (SELECT pm2.role FROM project_members pm2 WHERE pm2.project_id = p.id AND pm2.user_id = ?{}) as my_role \
                 FROM projects p {} ORDER BY p.id DESC LIMIT ?{} OFFSET ?{}",
                params.len() + 1,
                where_clause,
                params.len() + 2,
                params.len() + 3,
            );
            params.push(Box::new(user_id));
            params.push(Box::new(page_size));
            params.push(Box::new(offset));

            let mut stmt = conn.prepare(&query_sql)?;
            let projects = stmt
                .query_map(
                    rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                    |row| {
                        let mut p = row_to_project(row)?;
                        p.member_count = row.get(41)?;
                        p.my_role = row.get(42)?;
                        Ok(p)
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            Ok((projects, total))
        })
        .await
        .unwrap()
    }

    async fn list_running_projects_for_member(
        &self,
        user_id: i64,
        is_admin: bool,
        limit: u32,
    ) -> Result<(Vec<Project>, u64)> {
        let pool = self.pool.clone();
        let limit = limit.min(20) as i64;
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;

            let (count_sql, query_sql) = if is_admin {
                (
                    "SELECT COUNT(*) FROM projects WHERE service_status = 'running'".to_string(),
                    format!(
                        "SELECT {} FROM projects p WHERE p.service_status = 'running' ORDER BY p.updated_at DESC LIMIT ?1",
                        PROJECT_COLUMNS.split(", ").map(|c| format!("p.{}", c)).collect::<Vec<_>>().join(", ")
                    ),
                )
            } else {
                (
                    "SELECT COUNT(*) FROM projects p INNER JOIN project_members pm ON pm.project_id = p.id WHERE p.service_status = 'running' AND pm.user_id = ?1".to_string(),
                    format!(
                        "SELECT {} FROM projects p INNER JOIN project_members pm ON pm.project_id = p.id WHERE p.service_status = 'running' AND pm.user_id = ?1 ORDER BY p.updated_at DESC LIMIT ?2",
                        PROJECT_COLUMNS.split(", ").map(|c| format!("p.{}", c)).collect::<Vec<_>>().join(", ")
                    ),
                )
            };

            let total: i64 = if is_admin {
                conn.query_row(&count_sql, [], |row| row.get(0))?
            } else {
                conn.query_row(&count_sql, rusqlite::params![user_id], |row| row.get(0))?
            };

            let projects = if is_admin {
                let mut stmt = conn.prepare(&query_sql)?;
                let rows = stmt.query_map(rusqlite::params![limit], row_to_project)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            } else {
                let mut stmt = conn.prepare(&query_sql)?;
                let rows = stmt.query_map(rusqlite::params![user_id, limit], row_to_project)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };

            Ok((projects, total as u64))
        })
        .await
        .unwrap()
    }

    async fn update_project(&self, id: i64, updates: &ProjectUpdate) -> Result<()> {
        let pool = self.pool.clone();
        let updates = updates.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut set_clauses: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref name) = updates.name {
                let idx = params.len() + 1;
                set_clauses.push(format!("name = ?{}", idx));
                params.push(Box::new(name.clone()));
            }
            if let Some(ref desc) = updates.description {
                let idx = params.len() + 1;
                set_clauses.push(format!("description = ?{}", idx));
                params.push(Box::new(desc.clone()));
            }
            if let Some(ref branch) = updates.default_branch {
                let idx = params.len() + 1;
                set_clauses.push(format!("default_branch = ?{}", idx));
                params.push(Box::new(branch.clone()));
            }
            if let Some(max_agents) = updates.max_concurrent_agents {
                let idx = params.len() + 1;
                set_clauses.push(format!("max_concurrent_agents = ?{}", idx));
                params.push(Box::new(max_agents));
            }
            if let Some(auto_restart) = updates.auto_restart {
                let idx = params.len() + 1;
                set_clauses.push(format!("auto_restart = ?{}", idx));
                params.push(Box::new(auto_restart as i64));
            }
            if let Some(ref v) = updates.hooks_after_create {
                let idx = params.len() + 1;
                set_clauses.push(format!("hooks_after_create = ?{}", idx));
                params.push(Box::new(v.clone()));
            }
            if let Some(ref v) = updates.hooks_before_remove {
                let idx = params.len() + 1;
                set_clauses.push(format!("hooks_before_remove = ?{}", idx));
                params.push(Box::new(v.clone()));
            }
            if let Some(ref v) = updates.codex_command {
                let idx = params.len() + 1;
                set_clauses.push(format!("codex_command = ?{}", idx));
                params.push(Box::new(v.clone()));
            }
            if let Some(ref v) = updates.codex_approval_policy {
                let idx = params.len() + 1;
                set_clauses.push(format!("codex_approval_policy = ?{}", idx));
                params.push(Box::new(v.clone()));
            }
            if let Some(ref v) = updates.codex_sandbox {
                let idx = params.len() + 1;
                set_clauses.push(format!("codex_sandbox = ?{}", idx));
                params.push(Box::new(v.clone()));
            }

            if set_clauses.is_empty() {
                return Ok(());
            }

            set_clauses.push("updated_at = datetime('now')".to_string());
            let idx = params.len() + 1;
            let sql = format!(
                "UPDATE projects SET {} WHERE id = ?{}",
                set_clauses.join(", "),
                idx
            );
            params.push(Box::new(id));

            let rows = conn.execute(
                &sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("Project not found".to_string()));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn delete_project(&self, id: i64) -> Result<()> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute("DELETE FROM projects WHERE id = ?1", [id])?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("Project not found".to_string()));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn update_service_status(&self, id: i64, status: &ServiceStatusUpdate) -> Result<()> {
        let pool = self.pool.clone();
        let status_str = status.status.as_str().to_string();
        let pid = status.pid;
        let error_message = status.error_message.clone();
        let is_running = status.status == crate::models::ServiceStatus::Running;
        let is_stopped = status.status == crate::models::ServiceStatus::Stopped;
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut sql = String::from(
                "UPDATE projects SET service_status = ?1, service_pid = ?2, error_message = ?3, updated_at = datetime('now')",
            );
            if is_running {
                sql.push_str(", last_started_at = datetime('now')");
            }
            if is_stopped {
                sql.push_str(", last_stopped_at = datetime('now')");
            }
            sql.push_str(" WHERE id = ?4");

            let rows = conn.execute(
                &sql,
                rusqlite::params![status_str, pid, error_message, id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("Project not found".to_string()));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn update_service_lifecycle(
        &self,
        id: i64,
        lifecycle: &crate::models::ServiceLifecycleUpdate,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let lifecycle = lifecycle.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "UPDATE projects
                 SET web_instance_id = ?1,
                     lifecycle_op_id = ?2,
                     service_owner_web_instance_id = ?3,
                     service_owner_heartbeat_at = datetime('now'),
                     service_generation = ?4,
                     service_instance_id = ?5,
                     service_pgid = ?6,
                     service_session_id = ?7,
                     service_cmdline_hash = ?8,
                     service_workdir = ?9,
                     last_lifecycle_op = ?10,
                     service_proxy_config_version = ?11,
                     updated_at = datetime('now')
                 WHERE id = ?12",
                rusqlite::params![
                    lifecycle.web_instance_id,
                    lifecycle.lifecycle_op_id,
                    lifecycle.service_owner_web_instance_id,
                    lifecycle.service_generation,
                    lifecycle.service_instance_id,
                    lifecycle.service_pgid,
                    lifecycle.service_session_id,
                    lifecycle.service_cmdline_hash,
                    lifecycle.service_workdir,
                    lifecycle.last_lifecycle_op,
                    lifecycle.service_proxy_config_version,
                    id,
                ],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("Project not found".to_string()));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn update_workflow(&self, id: i64, template: &str, content: Option<&str>) -> Result<()> {
        let pool = self.pool.clone();
        let template = template.to_string();
        let content = content.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "UPDATE projects SET workflow_template = ?1, workflow_content = ?2, updated_at = datetime('now') WHERE id = ?3",
                rusqlite::params![template, content, id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound("Project not found".to_string()));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get_workflow_content(&self, id: i64) -> Result<Option<(String, Option<String>)>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn.query_row(
                "SELECT workflow_template, workflow_content FROM projects WHERE id = ?1",
                [id],
                |row| {
                    let template: String = row.get(0)?;
                    let content: Option<String> = row.get(1)?;
                    Ok((template, content))
                },
            );
            match result {
                Ok(data) => Ok(Some(data)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        })
        .await
        .unwrap()
    }
}

// ==================== Project Member Repository ====================

#[async_trait]
impl ProjectMemberRepository for SqliteRepository {
    async fn list_members(&self, project_id: i64) -> Result<Vec<ProjectMember>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut stmt = conn.prepare(
                "SELECT pm.user_id, u.username, u.display_name, pm.role, pm.synced_from, pm.created_at
                 FROM project_members pm
                 JOIN users u ON u.id = pm.user_id
                 WHERE pm.project_id = ?1 AND u.deleted_at IS NULL
                 ORDER BY pm.created_at ASC",
            )?;
            let members = stmt
                .query_map([project_id], |row| {
                    Ok(ProjectMember {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        display_name: row.get(2)?,
                        role: row.get(3)?,
                        synced_from: row.get(4)?,
                        created_at: parse_datetime(&row.get::<_, String>(5)?),
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(members)
        })
        .await
        .unwrap()
    }

    async fn add_member(
        &self,
        project_id: i64,
        user_id: i64,
        role: &str,
        synced_from: Option<&str>,
    ) -> Result<ProjectMember> {
        let pool = self.pool.clone();
        let role = role.to_string();
        let synced_from = synced_from.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO project_members (project_id, user_id, role, synced_from) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![project_id, user_id, role, synced_from],
            ).map_err(|e| match e {
                rusqlite::Error::SqliteFailure(ref err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    WebPlatformError::Conflict("User is already a member of this project".to_string())
                }
                _ => WebPlatformError::Database(e),
            })?;

            let member = conn.query_row(
                "SELECT pm.user_id, u.username, u.display_name, pm.role, pm.synced_from, pm.created_at
                 FROM project_members pm
                 JOIN users u ON u.id = pm.user_id
                 WHERE pm.project_id = ?1 AND pm.user_id = ?2",
                rusqlite::params![project_id, user_id],
                |row| {
                    Ok(ProjectMember {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        display_name: row.get(2)?,
                        role: row.get(3)?,
                        synced_from: row.get(4)?,
                        created_at: parse_datetime(&row.get::<_, String>(5)?),
                    })
                },
            )?;
            Ok(member)
        })
        .await
        .unwrap()
    }

    async fn update_member_role(&self, project_id: i64, user_id: i64, role: &str) -> Result<()> {
        let pool = self.pool.clone();
        let role = role.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "UPDATE project_members SET role = ?1 WHERE project_id = ?2 AND user_id = ?3",
                rusqlite::params![role, project_id, user_id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound(
                    "Member not found in project".to_string(),
                ));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn remove_member(&self, project_id: i64, user_id: i64) -> Result<()> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "DELETE FROM project_members WHERE project_id = ?1 AND user_id = ?2",
                rusqlite::params![project_id, user_id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound(
                    "Member not found in project".to_string(),
                ));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn is_member(&self, project_id: i64, user_id: i64) -> Result<bool> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM project_members WHERE project_id = ?1 AND user_id = ?2",
                rusqlite::params![project_id, user_id],
                |row| row.get(0),
            )?;
            Ok(count > 0)
        })
        .await
        .unwrap()
    }

    async fn get_member_role(&self, project_id: i64, user_id: i64) -> Result<Option<String>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn.query_row(
                "SELECT role FROM project_members WHERE project_id = ?1 AND user_id = ?2",
                rusqlite::params![project_id, user_id],
                |row| row.get::<_, String>(0),
            );
            match result {
                Ok(role) => Ok(Some(role)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn count_members(&self, project_id: i64) -> Result<i64> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM project_members WHERE project_id = ?1",
                [project_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap()
    }

    async fn count_owners(&self, project_id: i64) -> Result<i64> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM project_members WHERE project_id = ?1 AND role = 'owner'",
                [project_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap()
    }

    async fn sync_members(&self, project_id: i64, members: &[SyncMember]) -> Result<SyncResult> {
        let pool = self.pool.clone();
        let members = members.to_vec();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut added: u32 = 0;
            let mut skipped: u32 = 0;
            let mut unmatched: Vec<String> = Vec::new();

            for member in &members {
                // Try to find user by username
                let user_id: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM users WHERE username = ?1 AND deleted_at IS NULL",
                        [&member.username],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(WebPlatformError::Database)?;

                match user_id {
                    Some(uid) => {
                        // Check if already a member
                        let exists: i64 = conn.query_row(
                            "SELECT COUNT(*) FROM project_members WHERE project_id = ?1 AND user_id = ?2",
                            rusqlite::params![project_id, uid],
                            |row| row.get(0),
                        )?;

                        if exists > 0 {
                            skipped += 1;
                        } else {
                            conn.execute(
                                "INSERT INTO project_members (project_id, user_id, role, synced_from) VALUES (?1, ?2, ?3, ?4)",
                                rusqlite::params![project_id, uid, member.role, member.synced_from],
                            )?;
                            added += 1;
                        }
                    }
                    None => {
                        unmatched.push(member.username.clone());
                    }
                }
            }

            Ok(SyncResult {
                added,
                skipped,
                unmatched,
            })
        })
        .await
        .unwrap()
    }
}

#[async_trait]
impl ConcurrencyRepository for SqliteRepository {
    async fn record_concurrency_event(&self, input: ConcurrencyEventInput<'_>) -> Result<()> {
        let pool = self.pool.clone();
        let project_id = input.project_id;
        let issue_iid = input.issue_iid;
        let duration_seconds = input.duration_seconds;
        let event_type = input.event_type.to_string();
        let agent_id = input.agent_id.map(|s| s.to_string());
        let issue_title = input.issue_title.map(|s| s.to_string());
        let metadata_json = input.metadata_json.map(|s| s.to_string());

        tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            conn.execute(
                "INSERT INTO concurrency_events (project_id, event_type, agent_id, issue_iid, issue_title, duration_seconds, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![project_id, event_type, agent_id, issue_iid, issue_title, duration_seconds, metadata_json],
            ).map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| WebPlatformError::Internal(e.to_string()))?
    }

    async fn save_snapshot(
        &self,
        project_id: i64,
        active_agents: i64,
        queued_tasks: i64,
        agents_json: Option<&str>,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let agents_json = agents_json.map(|s| s.to_string());

        tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            conn.execute(
                "INSERT INTO concurrency_snapshots (project_id, active_agents, queued_tasks, agents_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))
                 ON CONFLICT(project_id) DO UPDATE SET
                   active_agents = excluded.active_agents,
                   queued_tasks = excluded.queued_tasks,
                   agents_json = excluded.agents_json,
                   updated_at = datetime('now')",
                rusqlite::params![project_id, active_agents, queued_tasks, agents_json],
            ).map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| WebPlatformError::Internal(e.to_string()))?
    }

    async fn load_snapshots(&self) -> Result<Vec<ConcurrencySnapshot>> {
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            let mut stmt = conn
                .prepare("SELECT project_id, active_agents, queued_tasks, agents_json, updated_at FROM concurrency_snapshots")
                .map_err(|e| WebPlatformError::Internal(e.to_string()))?;

            let snapshots = stmt
                .query_map([], |row| {
                    Ok(ConcurrencySnapshot {
                        project_id: row.get(0)?,
                        active_agents: row.get(1)?,
                        queued_tasks: row.get(2)?,
                        agents_json: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                })
                .map_err(|e| WebPlatformError::Internal(e.to_string()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| WebPlatformError::Internal(e.to_string()))?;

            Ok(snapshots)
        })
        .await
        .map_err(|e| WebPlatformError::Internal(e.to_string()))?
    }

    async fn get_today_stats(&self, project_id: i64) -> Result<(i64, i64, Option<i64>)> {
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get().map_err(|e| WebPlatformError::Internal(e.to_string()))?;
            let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

            let started: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM concurrency_events WHERE project_id = ?1 AND event_type = 'agent_started' AND created_at >= ?2",
                    rusqlite::params![project_id, format!("{} 00:00:00", today)],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let completed: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM concurrency_events WHERE project_id = ?1 AND event_type = 'agent_completed' AND created_at >= ?2",
                    rusqlite::params![project_id, format!("{} 00:00:00", today)],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let avg_duration: Option<i64> = conn
                .query_row(
                    "SELECT AVG(duration_seconds) FROM concurrency_events WHERE project_id = ?1 AND event_type = 'agent_completed' AND created_at >= ?2 AND duration_seconds IS NOT NULL",
                    rusqlite::params![project_id, format!("{} 00:00:00", today)],
                    |row| row.get(0),
                )
                .unwrap_or(None);

            Ok((started, completed, avg_duration))
        })
        .await
        .map_err(|e| WebPlatformError::Internal(e.to_string()))?
    }
}

// ==================== Alert Repository ====================

#[async_trait]
impl AlertRepository for SqliteRepository {
    async fn get_all_alert_rules(&self) -> Result<Vec<AlertRule>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut stmt = conn.prepare(
                "SELECT rule_id, name, description, severity, enabled, threshold_json, cooldown_seconds, updated_at FROM alert_rules ORDER BY rule_id",
            )?;
            let rules = stmt
                .query_map([], |row| {
                    let threshold_str: String = row.get(5)?;
                    let threshold: HashMap<String, serde_json::Value> =
                        serde_json::from_str(&threshold_str).unwrap_or_default();
                    Ok(AlertRule {
                        rule_id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        severity: row.get(3)?,
                        enabled: row.get::<_, i64>(4)? != 0,
                        threshold,
                        cooldown_seconds: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rules)
        })
        .await
        .unwrap()
    }

    async fn get_alert_rule(&self, rule_id: &str) -> Result<Option<AlertRule>> {
        let pool = self.pool.clone();
        let rule_id = rule_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn.query_row(
                "SELECT rule_id, name, description, severity, enabled, threshold_json, cooldown_seconds, updated_at FROM alert_rules WHERE rule_id = ?1",
                [&rule_id],
                |row| {
                    let threshold_str: String = row.get(5)?;
                    let threshold: HashMap<String, serde_json::Value> =
                        serde_json::from_str(&threshold_str).unwrap_or_default();
                    Ok(AlertRule {
                        rule_id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        severity: row.get(3)?,
                        enabled: row.get::<_, i64>(4)? != 0,
                        threshold,
                        cooldown_seconds: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                },
            );
            match result {
                Ok(rule) => Ok(Some(rule)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn update_alert_rule(
        &self,
        rule_id: &str,
        enabled: Option<bool>,
        threshold_json: Option<&str>,
        cooldown_seconds: Option<i64>,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let rule_id = rule_id.to_string();
        let threshold_json = threshold_json.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut set_clauses: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(en) = enabled {
                let idx = params.len() + 1;
                set_clauses.push(format!("enabled = ?{}", idx));
                params.push(Box::new(en as i64));
            }
            if let Some(ref tj) = threshold_json {
                let idx = params.len() + 1;
                set_clauses.push(format!("threshold_json = ?{}", idx));
                params.push(Box::new(tj.clone()));
            }
            if let Some(cd) = cooldown_seconds {
                let idx = params.len() + 1;
                set_clauses.push(format!("cooldown_seconds = ?{}", idx));
                params.push(Box::new(cd));
            }

            if set_clauses.is_empty() {
                return Ok(());
            }

            set_clauses.push("updated_at = datetime('now')".to_string());
            let idx = params.len() + 1;
            let sql = format!(
                "UPDATE alert_rules SET {} WHERE rule_id = ?{}",
                set_clauses.join(", "),
                idx
            );
            params.push(Box::new(rule_id.clone()));

            let rows = conn.execute(
                &sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound(format!(
                    "Alert rule '{}' not found",
                    rule_id
                )));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get_all_notification_channels(&self) -> Result<Vec<NotificationChannelRow>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut stmt = conn.prepare(
                "SELECT channel_id, name, channel_type, enabled, config_encrypted, severity_filter_json, last_test_at, last_test_success, created_at, updated_at FROM notification_channels ORDER BY channel_id",
            )?;
            let channels = stmt
                .query_map([], |row| {
                    Ok(NotificationChannelRow {
                        channel_id: row.get(0)?,
                        name: row.get(1)?,
                        channel_type: row.get(2)?,
                        enabled: row.get::<_, i64>(3)? != 0,
                        config_encrypted: row.get(4)?,
                        severity_filter_json: row.get(5)?,
                        last_test_at: row.get(6)?,
                        last_test_success: row.get::<_, Option<i64>>(7)?.map(|v| v != 0),
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(channels)
        })
        .await
        .unwrap()
    }

    async fn get_notification_channel(
        &self,
        channel_id: &str,
    ) -> Result<Option<NotificationChannelRow>> {
        let pool = self.pool.clone();
        let channel_id = channel_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn.query_row(
                "SELECT channel_id, name, channel_type, enabled, config_encrypted, severity_filter_json, last_test_at, last_test_success, created_at, updated_at FROM notification_channels WHERE channel_id = ?1",
                [&channel_id],
                |row| {
                    Ok(NotificationChannelRow {
                        channel_id: row.get(0)?,
                        name: row.get(1)?,
                        channel_type: row.get(2)?,
                        enabled: row.get::<_, i64>(3)? != 0,
                        config_encrypted: row.get(4)?,
                        severity_filter_json: row.get(5)?,
                        last_test_at: row.get(6)?,
                        last_test_success: row.get::<_, Option<i64>>(7)?.map(|v| v != 0),
                        created_at: row.get(8)?,
                        updated_at: row.get(9)?,
                    })
                },
            );
            match result {
                Ok(ch) => Ok(Some(ch)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(WebPlatformError::Database(e)),
            }
        })
        .await
        .unwrap()
    }

    async fn save_notification_channels(
        &self,
        channels: Vec<NotificationChannelRow>,
    ) -> Result<()> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            // Full replacement: delete all existing, then insert new ones
            conn.execute("DELETE FROM notification_channels", [])?;
            for ch in &channels {
                conn.execute(
                    "INSERT INTO notification_channels (channel_id, name, channel_type, enabled, config_encrypted, severity_filter_json, last_test_at, last_test_success, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    rusqlite::params![
                        ch.channel_id,
                        ch.name,
                        ch.channel_type,
                        ch.enabled as i64,
                        ch.config_encrypted,
                        ch.severity_filter_json,
                        ch.last_test_at,
                        ch.last_test_success.map(|b| b as i64),
                        ch.created_at,
                        ch.updated_at,
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn update_channel_test_result(
        &self,
        channel_id: &str,
        success: bool,
        tested_at: &str,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let channel_id = channel_id.to_string();
        let tested_at = tested_at.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "UPDATE notification_channels SET last_test_at = ?1, last_test_success = ?2, updated_at = datetime('now') WHERE channel_id = ?3",
                rusqlite::params![tested_at, success as i64, channel_id],
            )?;
            if rows == 0 {
                return Err(WebPlatformError::NotFound(format!(
                    "Notification channel '{}' not found",
                    channel_id
                )));
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn insert_alert_history(&self, record: &InsertAlertHistory) -> Result<i64> {
        let pool = self.pool.clone();
        let record = record.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO alert_history (rule_id, severity, project_id, title, message, context_json, fired_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    record.rule_id,
                    record.severity,
                    record.project_id,
                    record.title,
                    record.message,
                    record.context_json,
                    record.fired_at,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await
        .unwrap()
    }

    async fn update_alert_notification_status(
        &self,
        id: i64,
        channel: &str,
        status: &str,
        notified_at: &str,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let channel = channel.to_string();
        let status = status.to_string();
        let notified_at = notified_at.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "UPDATE alert_history SET notification_channel = ?1, notification_status = ?2, notified_at = ?3 WHERE id = ?4",
                rusqlite::params![channel, status, notified_at, id],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn query_alert_history(
        &self,
        query: &AlertHistoryQuery,
    ) -> Result<(Vec<AlertHistoryRecord>, i64)> {
        let pool = self.pool.clone();
        let page_no = query.effective_page_no();
        let page_size = query.effective_page_size();
        let severity = query.severity.clone();
        let rule_id = query.rule_id.clone();
        let project_id = query.project_id;
        let status = query.status.clone();
        let start_time = query.start_time.clone();
        let end_time = query.end_time.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let offset = (page_no - 1) * page_size;

            let mut conditions: Vec<String> = Vec::new();
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref sev) = severity {
                let idx = params.len() + 1;
                conditions.push(format!("ah.severity = ?{}", idx));
                params.push(Box::new(sev.clone()));
            }
            if let Some(ref rid) = rule_id {
                let idx = params.len() + 1;
                conditions.push(format!("ah.rule_id = ?{}", idx));
                params.push(Box::new(rid.clone()));
            }
            if let Some(pid) = project_id {
                let idx = params.len() + 1;
                conditions.push(format!("ah.project_id = ?{}", idx));
                params.push(Box::new(pid));
            }
            if let Some(ref st) = status {
                let idx = params.len() + 1;
                conditions.push(format!("ah.notification_status = ?{}", idx));
                params.push(Box::new(st.clone()));
            }
            if let Some(ref start) = start_time {
                let idx = params.len() + 1;
                conditions.push(format!("ah.fired_at >= ?{}", idx));
                params.push(Box::new(start.clone()));
            }
            if let Some(ref end) = end_time {
                let idx = params.len() + 1;
                conditions.push(format!("ah.fired_at <= ?{}", idx));
                params.push(Box::new(end.clone()));
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            // Count total
            let count_sql = format!(
                "SELECT COUNT(*) FROM alert_history ah {}",
                where_clause
            );
            let total: i64 = conn.query_row(
                &count_sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                |row| row.get(0),
            )?;

            // Query with LEFT JOIN to get project name
            let query_sql = format!(
                "SELECT ah.id, ah.rule_id, ah.severity, ah.project_id, p.name, ah.title, ah.message, ah.context_json, ah.fired_at, ah.resolved_at, ah.notified_at, ah.notification_channel, ah.notification_status \
                 FROM alert_history ah \
                 LEFT JOIN projects p ON p.id = ah.project_id \
                 {} \
                 ORDER BY ah.fired_at DESC \
                 LIMIT ?{} OFFSET ?{}",
                where_clause,
                params.len() + 1,
                params.len() + 2,
            );
            params.push(Box::new(page_size));
            params.push(Box::new(offset));

            let mut stmt = conn.prepare(&query_sql)?;
            let records = stmt
                .query_map(
                    rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                    |row| {
                        let context_json: Option<String> = row.get(7)?;
                        let context: Option<HashMap<String, String>> = context_json
                            .and_then(|s| serde_json::from_str(&s).ok());
                        Ok(AlertHistoryRecord {
                            id: row.get(0)?,
                            rule_id: row.get(1)?,
                            severity: row.get(2)?,
                            project_id: row.get(3)?,
                            project_name: row.get(4)?,
                            title: row.get(5)?,
                            message: row.get(6)?,
                            context,
                            fired_at: row.get(8)?,
                            resolved_at: row.get(9)?,
                            notified_at: row.get(10)?,
                            notification_channel: row.get(11)?,
                            notification_status: row.get(12)?,
                        })
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            Ok((records, total))
        })
        .await
        .unwrap()
    }

    async fn cleanup_alert_history(&self, retention_days: i64) -> Result<u64> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "DELETE FROM alert_history WHERE fired_at < datetime('now', ?1)",
                rusqlite::params![format!("-{} days", retention_days)],
            )?;
            Ok(rows as u64)
        })
        .await
        .unwrap()
    }

    async fn save_cooldown(
        &self,
        rule_id: &str,
        scope_key: &str,
        last_fired_at: &str,
        expires_at: &str,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let rule_id = rule_id.to_string();
        let scope_key = scope_key.to_string();
        let last_fired_at = last_fired_at.to_string();
        let expires_at = expires_at.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO alert_cooldowns (rule_id, scope_key, last_fired_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(rule_id, scope_key) DO UPDATE SET
                    last_fired_at = excluded.last_fired_at,
                    expires_at = excluded.expires_at",
                rusqlite::params![rule_id, scope_key, last_fired_at, expires_at],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn load_active_cooldowns(&self) -> Result<Vec<(String, String, String)>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut stmt = conn.prepare(
                "SELECT rule_id, scope_key, expires_at FROM alert_cooldowns WHERE expires_at > datetime('now')",
            )?;
            let entries = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(entries)
        })
        .await
        .unwrap()
    }

    async fn cleanup_expired_cooldowns(&self) -> Result<u64> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let rows = conn.execute(
                "DELETE FROM alert_cooldowns WHERE expires_at <= datetime('now')",
                [],
            )?;
            Ok(rows as u64)
        })
        .await
        .unwrap()
    }
}

#[async_trait]
impl SystemConfigRepository for SqliteRepository {
    async fn list_system_configs(&self) -> Result<Vec<SystemConfigItem>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let mut stmt = conn.prepare(
                "SELECT key, value, description, updated_at FROM system_configs ORDER BY key ASC",
            )?;
            let configs = stmt
                .query_map([], |row| {
                    Ok(SystemConfigItem {
                        key: row.get(0)?,
                        value: row.get(1)?,
                        description: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(configs)
        })
        .await
        .unwrap()
    }

    async fn update_system_configs(&self, configs: &[(&str, &str)]) -> Result<()> {
        let pool = self.pool.clone();
        let configs: Vec<(String, String)> = configs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            for (key, value) in &configs {
                let rows = conn.execute(
                    "UPDATE system_configs SET value = ?1, updated_at = datetime('now') WHERE key = ?2",
                    rusqlite::params![value, key],
                )?;
                if rows == 0 {
                    return Err(WebPlatformError::NotFound(format!(
                        "Config key '{}' not found",
                        key
                    )));
                }
            }
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn get_system_stats(&self) -> Result<(i64, i64, i64)> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let total_projects: i64 =
                conn.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;
            let running_services: i64 = conn.query_row(
                "SELECT COUNT(*) FROM projects WHERE service_status = 'running'",
                [],
                |row| row.get(0),
            )?;
            let total_users: i64 = conn.query_row(
                "SELECT COUNT(*) FROM users WHERE deleted_at IS NULL",
                [],
                |row| row.get(0),
            )?;
            Ok((total_projects, running_services, total_users))
        })
        .await
        .unwrap()
    }
}

#[async_trait]
impl NetworkProxyRepository for SqliteRepository {
    async fn get_proxy_secret(&self, key: &str) -> Result<Option<ProxySecret>> {
        let pool = self.pool.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let result = conn
                .query_row(
                    "SELECT key, encrypted_value, kind, updated_at FROM secret_configs WHERE key = ?1",
                    [&key],
                    |row| {
                        Ok(ProxySecret {
                            key: row.get(0)?,
                            encrypted_value: row.get(1)?,
                            kind: row.get(2)?,
                            updated_at: row.get(3)?,
                        })
                    },
                )
                .optional()?;
            Ok(result)
        })
        .await
        .unwrap()
    }

    async fn upsert_proxy_secret(
        &self,
        key: &str,
        encrypted_value: &str,
        kind: &str,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let key = key.to_string();
        let encrypted_value = encrypted_value.to_string();
        let kind = kind.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute(
                "INSERT INTO secret_configs (key, encrypted_value, kind)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    encrypted_value = excluded.encrypted_value,
                    kind = excluded.kind,
                    updated_at = datetime('now')",
                rusqlite::params![key, encrypted_value, kind],
            )?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn delete_proxy_secret(&self, key: &str) -> Result<()> {
        let pool = self.pool.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            conn.execute("DELETE FROM secret_configs WHERE key = ?1", [&key])?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn update_network_proxy_config(
        &self,
        expected_version: &str,
        configs: Vec<(String, String)>,
        secret_mutations: Vec<ProxySecretMutation>,
    ) -> Result<()> {
        let pool = self.pool.clone();
        let expected_version = expected_version.to_string();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

            let current_version: Option<String> = tx
                .query_row(
                    "SELECT value FROM system_configs WHERE key = ?1",
                    [VERSION_KEY],
                    |row| row.get(0),
                )
                .optional()?;
            let current_version = current_version.unwrap_or_else(|| "1".to_string());
            if current_version != expected_version {
                return Err(WebPlatformError::Conflict(
                    "network proxy config version conflict".to_string(),
                ));
            }

            for mutation in secret_mutations {
                if let Some(encrypted_value) = mutation.encrypted_value {
                    tx.execute(
                        "INSERT INTO secret_configs (key, encrypted_value, kind)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT(key) DO UPDATE SET
                            encrypted_value = excluded.encrypted_value,
                            kind = excluded.kind,
                            updated_at = datetime('now')",
                        rusqlite::params![mutation.key, encrypted_value, mutation.kind],
                    )?;
                } else {
                    tx.execute("DELETE FROM secret_configs WHERE key = ?1", [mutation.key])?;
                }
            }

            for (key, value) in configs {
                tx.execute(
                    "INSERT INTO system_configs (key, value, updated_at)
                     VALUES (?1, ?2, datetime('now'))
                     ON CONFLICT(key) DO UPDATE SET
                        value = excluded.value,
                        updated_at = datetime('now')",
                    rusqlite::params![key, value],
                )?;
            }

            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap()
    }

    async fn count_running_services_with_stale_proxy_version(
        &self,
        current_version: &str,
    ) -> Result<i64> {
        let pool = self.pool.clone();
        let current_version = current_version.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let count = conn.query_row(
                "SELECT COUNT(*)
                 FROM projects
                 WHERE service_status = 'running'
                   AND COALESCE(service_proxy_config_version, '') <> ?1",
                [current_version],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap()
    }

    async fn current_network_proxy_version(&self) -> Result<String> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.get()?;
            let version = conn
                .query_row(
                    "SELECT value FROM system_configs WHERE key = ?1",
                    [VERSION_KEY],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or_else(|| "1".to_string());
            Ok(version)
        })
        .await
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_pool;
    use tempfile::TempDir;

    fn setup_test_repo() -> (SqliteRepository, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = init_pool(db_path.to_str().unwrap());
        let repo = SqliteRepository::new(pool);
        (repo, dir)
    }

    #[tokio::test]
    async fn test_create_user_success() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("testuser", "hash123", Some("Test User"), "user")
            .await
            .unwrap();
        assert!(user.id > 0);
        assert_eq!(user.username, "testuser");
        assert_eq!(user.display_name, Some("Test User".to_string()));
        assert_eq!(user.role, "user");
        assert!(user.deleted_at.is_none());
    }

    #[tokio::test]
    async fn test_create_user_duplicate_username_fails() {
        let (repo, _dir) = setup_test_repo();
        repo.create_user("dup_user", "hash1", None, "user")
            .await
            .unwrap();
        let result = repo.create_user("dup_user", "hash2", None, "user").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            WebPlatformError::Conflict(msg) => {
                assert!(msg.contains("dup_user"));
            }
            other => panic!("expected Conflict, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_find_by_username_exists() {
        let (repo, _dir) = setup_test_repo();
        repo.create_user("findme", "hash", None, "user")
            .await
            .unwrap();
        let found = repo.find_by_username("findme").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, "findme");
    }

    #[tokio::test]
    async fn test_find_by_username_not_exists() {
        let (repo, _dir) = setup_test_repo();
        let found = repo.find_by_username("ghost").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_by_id_exists() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("byid", "hash", None, "user")
            .await
            .unwrap();
        let found = repo.find_by_id(user.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, user.id);
    }

    #[tokio::test]
    async fn test_find_by_id_not_exists() {
        let (repo, _dir) = setup_test_repo();
        let found = repo.find_by_id(9999).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_find_by_id_soft_deleted_returns_none() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("todelete", "hash", None, "user")
            .await
            .unwrap();
        repo.soft_delete(user.id).await.unwrap();
        let found = repo.find_by_id(user.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_list_users_pagination() {
        let (repo, _dir) = setup_test_repo();
        for i in 0..10 {
            repo.create_user(&format!("user{}", i), "hash", None, "user")
                .await
                .unwrap();
        }
        let (users, total) = repo.list_users(1, 3, None, None).await.unwrap();
        assert_eq!(users.len(), 3);
        assert_eq!(total, 10);

        let (users_p2, _) = repo.list_users(2, 3, None, None).await.unwrap();
        assert_eq!(users_p2.len(), 3);
    }

    #[tokio::test]
    async fn test_list_users_search_filter() {
        let (repo, _dir) = setup_test_repo();
        repo.create_user("alice", "hash", Some("Alice Smith"), "user")
            .await
            .unwrap();
        repo.create_user("bob", "hash", Some("Bob Jones"), "user")
            .await
            .unwrap();
        repo.create_user("charlie", "hash", Some("Charlie Smith"), "admin")
            .await
            .unwrap();

        let (users, total) = repo.list_users(1, 20, Some("alice"), None).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(users[0].username, "alice");
    }

    #[tokio::test]
    async fn test_list_users_role_filter() {
        let (repo, _dir) = setup_test_repo();
        repo.create_user("admin1", "hash", None, "admin")
            .await
            .unwrap();
        repo.create_user("user1", "hash", None, "user")
            .await
            .unwrap();
        repo.create_user("user2", "hash", None, "user")
            .await
            .unwrap();

        let (users, total) = repo.list_users(1, 20, None, Some("admin")).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(users[0].role, "admin");
    }

    #[tokio::test]
    async fn test_list_users_excludes_deleted() {
        let (repo, _dir) = setup_test_repo();
        let u1 = repo
            .create_user("active", "hash", None, "user")
            .await
            .unwrap();
        let u2 = repo
            .create_user("deleted", "hash", None, "user")
            .await
            .unwrap();
        repo.soft_delete(u2.id).await.unwrap();

        let (users, total) = repo.list_users(1, 20, None, None).await.unwrap();
        assert_eq!(total, 1);
        assert_eq!(users[0].id, u1.id);
    }

    #[tokio::test]
    async fn test_update_display_name_success() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("updname", "hash", Some("Old Name"), "user")
            .await
            .unwrap();
        repo.update_display_name(user.id, "New Name").await.unwrap();
        let found = repo.find_by_id(user.id).await.unwrap().unwrap();
        assert_eq!(found.display_name, Some("New Name".to_string()));
    }

    #[tokio::test]
    async fn test_update_display_name_not_found() {
        let (repo, _dir) = setup_test_repo();
        let result = repo.update_display_name(9999, "Name").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_password_success() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("updpass", "old_hash", None, "user")
            .await
            .unwrap();
        repo.update_password(user.id, "new_hash").await.unwrap();
        let found = repo.find_by_id(user.id).await.unwrap().unwrap();
        assert_eq!(found.password_hash, "new_hash");
    }

    #[tokio::test]
    async fn test_soft_delete_sets_deleted_at() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("softdel", "hash", None, "user")
            .await
            .unwrap();
        repo.soft_delete(user.id).await.unwrap();
        let found = repo.find_by_username("softdel").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_soft_delete_nonexistent_fails() {
        let (repo, _dir) = setup_test_repo();
        let result = repo.soft_delete(9999).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_upsert_config_create() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("cfguser", "hash", None, "user")
            .await
            .unwrap();
        repo.upsert_config(user.id, Some("glpat-xxx"), Some("https://gitlab.com"), None)
            .await
            .unwrap();
        let config = repo.get_config(user.id).await.unwrap().unwrap();
        assert_eq!(config.gitlab_token, Some("glpat-xxx".to_string()));
        assert_eq!(config.gitlab_host, Some("https://gitlab.com".to_string()));
        assert_eq!(config.github_token, None);
    }

    #[tokio::test]
    async fn test_upsert_config_update() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("cfgupd", "hash", None, "user")
            .await
            .unwrap();
        repo.upsert_config(user.id, Some("token1"), None, None)
            .await
            .unwrap();
        repo.upsert_config(user.id, Some("token2"), None, None)
            .await
            .unwrap();
        let config = repo.get_config(user.id).await.unwrap().unwrap();
        assert_eq!(config.gitlab_token, Some("token2".to_string()));
    }

    #[tokio::test]
    async fn test_get_config_not_exists() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("nocfg", "hash", None, "user")
            .await
            .unwrap();
        let config = repo.get_config(user.id).await.unwrap();
        assert!(config.is_none());
    }

    #[tokio::test]
    async fn test_add_to_blacklist_success() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("bluser", "hash", None, "user")
            .await
            .unwrap();
        let result = repo.add_to_blacklist(user.id, "password_changed").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_is_blacklisted_correct() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("blcheck", "hash", None, "user")
            .await
            .unwrap();

        let before = chrono::Utc::now();
        let is_bl = repo.is_blacklisted(user.id, before).await.unwrap();
        assert!(!is_bl);

        repo.add_to_blacklist(user.id, "test").await.unwrap();

        let is_bl_after = repo.is_blacklisted(user.id, before).await.unwrap();
        assert!(is_bl_after);
    }

    #[tokio::test]
    async fn test_load_all_returns_entries() {
        let (repo, _dir) = setup_test_repo();
        let user = repo
            .create_user("loadall", "hash", None, "user")
            .await
            .unwrap();
        repo.add_to_blacklist(user.id, "reason1").await.unwrap();
        repo.add_to_blacklist(user.id, "reason2").await.unwrap();

        let entries = repo.load_all().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].user_id, user.id);
    }
}
