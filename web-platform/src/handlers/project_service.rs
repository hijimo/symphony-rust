use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::Duration;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::{
    Project, ResponseData, ServiceLifecycleUpdate, ServiceStatus, ServiceStatusUpdate,
};
use crate::process_manager::{pid_verify, spawn, watcher, ProcessState};
use crate::repository::{NetworkProxyRepository, ProjectRepository};
use crate::AppState;

/// Lock acquisition timeout for per-project mutex.
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for waiting for process to exit after SIGTERM.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

fn web_instance_id() -> String {
    std::env::var("SYMPHONY_WEB_INSTANCE_ID")
        .unwrap_or_else(|_| format!("web-{}", std::process::id()))
}

fn service_workdir(workspace_root: &str, project_id: i64) -> String {
    std::path::PathBuf::from(workspace_root)
        .join(project_id.to_string())
        .to_string_lossy()
        .to_string()
}

fn service_cmdline_hash(symphony_bin: &str, workdir: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(symphony_bin.as_bytes());
    hasher.update([0]);
    hasher.update(b"WORKFLOW.md");
    hasher.update([0]);
    hasher.update(workdir.as_bytes());
    hex::encode(hasher.finalize())
}

fn next_service_identity(project: &Project) -> (i64, String) {
    let generation = project.service_generation + 1;
    (
        generation,
        format!("svc-{}-{}-{}", project.id, generation, Uuid::new_v4()),
    )
}

struct LifecycleStartInput<'a> {
    project: &'a Project,
    symphony_bin: &'a str,
    workspace_root: &'a str,
    last_lifecycle_op: &'a str,
    pid: u32,
    service_generation: i64,
    service_instance_id: String,
    proxy_config_version: String,
}

fn build_lifecycle_update(input: LifecycleStartInput<'_>) -> ServiceLifecycleUpdate {
    let web_instance_id = web_instance_id();
    let workdir = service_workdir(input.workspace_root, input.project.id);
    ServiceLifecycleUpdate {
        web_instance_id: web_instance_id.clone(),
        lifecycle_op_id: Uuid::new_v4().to_string(),
        service_owner_web_instance_id: web_instance_id,
        service_generation: input.service_generation,
        service_instance_id: input.service_instance_id,
        service_pgid: process_group_id(input.pid),
        service_session_id: process_session_id(input.pid),
        service_cmdline_hash: service_cmdline_hash(input.symphony_bin, &workdir),
        service_workdir: workdir,
        last_lifecycle_op: input.last_lifecycle_op.to_string(),
        service_proxy_config_version: input.proxy_config_version,
    }
}

fn build_stop_lifecycle_update(project: &Project, workspace_root: &str) -> ServiceLifecycleUpdate {
    let web_instance_id = project
        .web_instance_id
        .clone()
        .unwrap_or_else(web_instance_id);
    ServiceLifecycleUpdate {
        web_instance_id: web_instance_id.clone(),
        lifecycle_op_id: Uuid::new_v4().to_string(),
        service_owner_web_instance_id: web_instance_id,
        service_generation: project.service_generation,
        service_instance_id: project
            .service_instance_id
            .clone()
            .unwrap_or_else(|| format!("svc-{}-{}", project.id, project.service_generation)),
        service_pgid: None,
        service_session_id: None,
        service_cmdline_hash: project.service_cmdline_hash.clone().unwrap_or_default(),
        service_workdir: project
            .service_workdir
            .clone()
            .unwrap_or_else(|| service_workdir(workspace_root, project.id)),
        last_lifecycle_op: "stop".to_string(),
        service_proxy_config_version: project
            .service_proxy_config_version
            .clone()
            .unwrap_or_default(),
    }
}

#[cfg(unix)]
fn process_group_id(pid: u32) -> Option<i64> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    (pgid >= 0).then_some(pgid as i64)
}

#[cfg(not(unix))]
fn process_group_id(_pid: u32) -> Option<i64> {
    None
}

#[cfg(unix)]
fn process_session_id(pid: u32) -> Option<i64> {
    let sid = unsafe { libc::getsid(pid as libc::pid_t) };
    (sid >= 0).then_some(sid as i64)
}

#[cfg(not(unix))]
fn process_session_id(_pid: u32) -> Option<i64> {
    None
}

/// Service status response payload.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatusResponse {
    pub status: String,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub uptime_seconds: Option<i64>,
    pub restart_count: u32,
    pub error_message: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiagnosticsResponse {
    pub issues: Vec<serde_json::Value>,
    pub services: Vec<serde_json::Value>,
}

/// GET /api/projects/:id/diagnostics - Query recovery diagnostics for a project.
pub async fn get_diagnostics(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<ProjectDiagnosticsResponse>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;

    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;
    let global_proxy_config_version = state.repo.current_network_proxy_version().await?;
    let service_proxy_config_version = project.service_proxy_config_version.clone();
    let needs_proxy_restart = project.service_status == "running"
        && service_proxy_config_version.as_deref() != Some(global_proxy_config_version.as_str());

    let service = serde_json::json!({
        "project_id": project.id,
        "service_instance_id": project.service_instance_id,
        "service_generation": project.service_generation,
        "web_instance_id": project.web_instance_id,
        "lifecycle_op_id": project.lifecycle_op_id,
        "lifecycle_lease_expires_at": project.lifecycle_lease_expires_at,
        "service_owner_web_instance_id": project.service_owner_web_instance_id,
        "service_owner_lease_expires_at": project.service_owner_lease_expires_at,
        "service_owner_heartbeat_at": project.service_owner_heartbeat_at,
        "last_lifecycle_op": project.last_lifecycle_op,
        "status_code": project.service_status,
        "pid": project.service_pid,
        "pgid": project.service_pgid,
        "session_id": project.service_session_id,
        "cmdline_hash": project.service_cmdline_hash,
        "workdir": project.service_workdir,
        "restart_count": project.restart_count,
        "proxy": {
            "globalProxyConfigVersion": global_proxy_config_version,
            "serviceProxyConfigVersion": service_proxy_config_version,
            "needsRestart": needs_proxy_restart,
        },
        "updated_at": project.updated_at.to_string(),
        "next_action": null,
    });

    Ok(Json(ResponseData::success(ProjectDiagnosticsResponse {
        issues: Vec::new(),
        services: vec![service],
    })))
}

/// POST /api/projects/:id/start - Start the project service.
pub async fn start_service(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<ServiceStatusResponse>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    // Acquire per-project lock with timeout
    let _guard = state
        .process_manager
        .try_lock(project_id, LOCK_TIMEOUT)
        .await
        .ok_or_else(|| {
            WebPlatformError::Conflict(
                "Another operation is in progress for this project. Please try again.".to_string(),
            )
        })?;

    // Check current status
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    if project.service_status == "running" || project.service_status == "starting" {
        return Err(WebPlatformError::Conflict(
            "Service is already running or starting".to_string(),
        ));
    }

    // Update status to starting
    let status_update = ServiceStatusUpdate {
        status: ServiceStatus::Starting,
        pid: None,
        error_message: None,
    };
    state
        .repo
        .update_service_status(project_id, &status_update)
        .await?;

    let (service_generation, service_instance_id) = next_service_identity(&project);

    // Spawn the symphony process
    let spawn_result = match spawn::spawn_symphony(
        &project,
        &state.repo,
        &state.encryption_key,
        &state.symphony_bin,
        &state.workspace_root,
        &service_instance_id,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let status_update = ServiceStatusUpdate {
                status: ServiceStatus::Failed,
                pid: None,
                error_message: Some(format!("{}", e)),
            };
            let _ = state
                .repo
                .update_service_status(project_id, &status_update)
                .await;
            return Err(e);
        }
    };

    let now = Utc::now();
    let pid = spawn_result.pid;
    let proxy_config_version = spawn_result.proxy_config_version.clone();

    let lifecycle_update = build_lifecycle_update(LifecycleStartInput {
        project: &project,
        symphony_bin: &state.symphony_bin,
        workspace_root: &state.workspace_root,
        pid,
        service_generation,
        service_instance_id,
        last_lifecycle_op: "start",
        proxy_config_version,
    });
    state
        .repo
        .update_service_lifecycle(project_id, &lifecycle_update)
        .await?;

    // Update DB with running status
    let status_update = ServiceStatusUpdate {
        status: ServiceStatus::Running,
        pid: Some(pid as i64),
        error_message: None,
    };
    state
        .repo
        .update_service_status(project_id, &status_update)
        .await?;

    // Update process manager state
    let process_state = ProcessState {
        pid,
        started_at: now,
        status: ServiceStatus::Running,
        restart_count: 0,
    };
    state.process_manager.set_state(project_id, process_state);

    // Spawn health check watcher
    watcher::spawn_watcher(
        project_id,
        state.process_manager.clone(),
        state.repo.clone(),
        state.encryption_key,
        state.symphony_bin.clone(),
        state.workspace_root.clone(),
    );

    let response = ServiceStatusResponse {
        status: "running".to_string(),
        pid: Some(pid),
        started_at: Some(now.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        uptime_seconds: Some(0),
        restart_count: 0,
        error_message: None,
    };

    Ok(Json(ResponseData::success(response)))
}

/// POST /api/projects/:id/stop - Stop the project service.
pub async fn stop_service(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<ServiceStatusResponse>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    // Acquire per-project lock with timeout
    let _guard = state
        .process_manager
        .try_lock(project_id, LOCK_TIMEOUT)
        .await
        .ok_or_else(|| {
            WebPlatformError::Conflict(
                "Another operation is in progress for this project. Please try again.".to_string(),
            )
        })?;

    // Check current status
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    if project.service_status != "running" && project.service_status != "error" {
        return Err(WebPlatformError::Conflict(
            "Service is not running".to_string(),
        ));
    }

    // Update status to stopping
    let status_update = ServiceStatusUpdate {
        status: ServiceStatus::Stopping,
        pid: project.service_pid,
        error_message: None,
    };
    state
        .repo
        .update_service_status(project_id, &status_update)
        .await?;

    // Get PID and verify
    if let Some(pid) = project.service_pid {
        let pid = pid as u32;
        let start_time = project
            .last_started_at
            .map(|dt| dt.and_utc().timestamp())
            .unwrap_or(0);

        if pid_verify::verify_pid(pid, start_time) {
            // Send SIGTERM
            pid_verify::send_sigterm(pid);

            // Wait for exit with timeout
            if !pid_verify::wait_for_exit(pid, STOP_TIMEOUT).await {
                // Force kill
                tracing::warn!(
                    project_id,
                    pid,
                    "Process did not exit after SIGTERM, sending SIGKILL"
                );
                pid_verify::send_sigkill(pid);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    // Update status to stopped
    let lifecycle_update = build_stop_lifecycle_update(&project, &state.workspace_root);
    state
        .repo
        .update_service_lifecycle(project_id, &lifecycle_update)
        .await?;

    let status_update = ServiceStatusUpdate {
        status: ServiceStatus::Stopped,
        pid: None,
        error_message: None,
    };
    state
        .repo
        .update_service_status(project_id, &status_update)
        .await?;

    // Clean up process manager state
    state.process_manager.remove_state(project_id);

    let response = ServiceStatusResponse {
        status: "stopped".to_string(),
        pid: None,
        started_at: None,
        uptime_seconds: None,
        restart_count: 0,
        error_message: None,
    };

    Ok(Json(ResponseData::success(response)))
}

/// POST /api/projects/:id/restart - Restart the project service.
pub async fn restart_service(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<ServiceStatusResponse>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    // Acquire per-project lock with timeout
    let _guard = state
        .process_manager
        .try_lock(project_id, LOCK_TIMEOUT)
        .await
        .ok_or_else(|| {
            WebPlatformError::Conflict(
                "Another operation is in progress for this project. Please try again.".to_string(),
            )
        })?;

    // Check current status
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    if project.service_status != "running" && project.service_status != "error" {
        return Err(WebPlatformError::Conflict(
            "Service is not running, cannot restart".to_string(),
        ));
    }

    // Stop phase
    if let Some(pid) = project.service_pid {
        let pid = pid as u32;
        let start_time = project
            .last_started_at
            .map(|dt| dt.and_utc().timestamp())
            .unwrap_or(0);

        if pid_verify::verify_pid(pid, start_time) {
            pid_verify::send_sigterm(pid);
            if !pid_verify::wait_for_exit(pid, STOP_TIMEOUT).await {
                pid_verify::send_sigkill(pid);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    let (service_generation, service_instance_id) = next_service_identity(&project);

    // Start phase
    let spawn_result = match spawn::spawn_symphony(
        &project,
        &state.repo,
        &state.encryption_key,
        &state.symphony_bin,
        &state.workspace_root,
        &service_instance_id,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let status_update = ServiceStatusUpdate {
                status: ServiceStatus::Failed,
                pid: None,
                error_message: Some(format!("{}", e)),
            };
            let _ = state
                .repo
                .update_service_status(project_id, &status_update)
                .await;
            return Err(e);
        }
    };

    let now = Utc::now();
    let pid = spawn_result.pid;
    let proxy_config_version = spawn_result.proxy_config_version.clone();

    let lifecycle_update = build_lifecycle_update(LifecycleStartInput {
        project: &project,
        symphony_bin: &state.symphony_bin,
        workspace_root: &state.workspace_root,
        pid,
        service_generation,
        service_instance_id,
        last_lifecycle_op: "restart",
        proxy_config_version,
    });
    state
        .repo
        .update_service_lifecycle(project_id, &lifecycle_update)
        .await?;

    let status_update = ServiceStatusUpdate {
        status: ServiceStatus::Running,
        pid: Some(pid as i64),
        error_message: None,
    };
    state
        .repo
        .update_service_status(project_id, &status_update)
        .await?;

    // Reset restart count on manual restart
    let process_state = ProcessState {
        pid,
        started_at: now,
        status: ServiceStatus::Running,
        restart_count: 0,
    };
    state.process_manager.set_state(project_id, process_state);

    // Spawn new watcher
    watcher::spawn_watcher(
        project_id,
        state.process_manager.clone(),
        state.repo.clone(),
        state.encryption_key,
        state.symphony_bin.clone(),
        state.workspace_root.clone(),
    );

    let response = ServiceStatusResponse {
        status: "running".to_string(),
        pid: Some(pid),
        started_at: Some(now.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        uptime_seconds: Some(0),
        restart_count: 0,
        error_message: None,
    };

    Ok(Json(ResponseData::success(response)))
}

/// GET /api/projects/:id/status - Get service runtime status.
pub async fn get_service_status(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<ServiceStatusResponse>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;

    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Calculate uptime if running
    let (uptime_seconds, started_at) =
        if let Some(ref process_state) = state.process_manager.get_state(project_id) {
            if process_state.status == ServiceStatus::Running {
                let uptime = Utc::now()
                    .signed_duration_since(process_state.started_at)
                    .num_seconds();
                (
                    Some(uptime),
                    Some(
                        process_state
                            .started_at
                            .format("%Y-%m-%dT%H:%M:%SZ")
                            .to_string(),
                    ),
                )
            } else {
                (None, None)
            }
        } else {
            let started_at = project
                .last_started_at
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string());
            (None, started_at)
        };

    let restart_count = state
        .process_manager
        .get_state(project_id)
        .map(|s| s.restart_count)
        .unwrap_or(project.restart_count as u32);

    let response = ServiceStatusResponse {
        status: project.service_status,
        pid: project.service_pid.map(|p| p as u32),
        started_at,
        uptime_seconds,
        restart_count,
        error_message: project.error_message,
    };

    Ok(Json(ResponseData::success(response)))
}
