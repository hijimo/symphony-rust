use axum::{
    extract::{Path, State},
    Json,
};
use chrono::Utc;
use serde::Serialize;
use std::time::Duration;
use utoipa::ToSchema;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::{ResponseData, ServiceStatus, ServiceStatusUpdate};
use crate::process_manager::{pid_verify, spawn, watcher, ProcessState};
use crate::repository::ProjectRepository;
use crate::AppState;

/// Lock acquisition timeout for per-project mutex.
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for waiting for process to exit after SIGTERM.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

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

    // Spawn the symphony process
    let spawn_result = match spawn::spawn_symphony(
        &project,
        &state.repo,
        &state.encryption_key,
        &state.symphony_bin,
        &state.workspace_root,
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

    // Start phase
    let spawn_result = match spawn::spawn_symphony(
        &project,
        &state.repo,
        &state.encryption_key,
        &state.symphony_bin,
        &state.workspace_root,
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
