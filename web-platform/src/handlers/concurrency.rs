use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, Sse},
    Json,
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::require_project_member;
use crate::models::concurrency::{
    ConcurrencyConfigResponse, ConcurrencyStatus, ProjectConcurrencyDetail, SseTicketResponse,
    UpdateConcurrencyConfigRequest, UpdateProjectConcurrencyRequest,
};
use crate::models::ProjectConcurrencyInfo;
use crate::models::ResponseData;
use crate::process_manager::ProcessManager;
use crate::repository::{ConcurrencyRepository, ProjectMemberRepository, ProjectRepository};
use crate::AppState;

async fn get_live_concurrency_status(
    state: &AppState,
) -> Result<ConcurrencyStatus, WebPlatformError> {
    let global_max = state
        .concurrency_manager
        .global_max
        .load(std::sync::atomic::Ordering::Relaxed);

    let process_entries: Vec<(i64, crate::process_manager::ProcessState)> = state
        .process_manager
        .processes
        .iter()
        .map(|entry| (*entry.key(), entry.value().clone()))
        .collect();

    let mut projects = Vec::new();
    for (project_id, process_state) in process_entries {
        if !ProcessManager::is_active_process(&process_state) {
            continue;
        }

        if let Some(project) = state.repo.get_project(project_id).await? {
            projects.push(ProjectConcurrencyInfo {
                project_id,
                project_name: project.name,
                active_agents: 1,
                max_agents: Some(project.max_concurrent_agents),
                queued_tasks: 0,
                service_status: project.service_status,
            });
        }
    }

    let global_active = state.process_manager.active_process_count();
    state
        .concurrency_manager
        .global_active
        .store(global_active, std::sync::atomic::Ordering::Relaxed);

    let utilization_percent = if global_max > 0 {
        (global_active as f64 / global_max as f64) * 100.0
    } else {
        0.0
    };

    let data_freshness_seconds = state
        .concurrency_manager
        .get_status()
        .data_freshness_seconds;

    Ok(ConcurrencyStatus {
        global_max,
        global_active,
        utilization_percent,
        projects,
        data_freshness_seconds,
    })
}

/// GET /api/admin/concurrency
pub async fn get_global_concurrency(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
) -> Result<Json<ResponseData<ConcurrencyStatus>>, WebPlatformError> {
    let status = get_live_concurrency_status(&state).await?;
    Ok(Json(ResponseData::success(status)))
}

/// PUT /api/admin/concurrency/config
pub async fn update_concurrency_config(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
    Json(req): Json<UpdateConcurrencyConfigRequest>,
) -> Result<Json<ResponseData<ConcurrencyConfigResponse>>, WebPlatformError> {
    let new_max = req
        .global_max
        .ok_or_else(|| WebPlatformError::BadRequest("global_max is required".to_string()))?;

    if !(1..=100).contains(&new_max) {
        return Err(WebPlatformError::BadRequest(
            "global_max must be between 1 and 100".to_string(),
        ));
    }

    // Optimistic locking check
    if let Some(expected) = req.expected_previous {
        let current = state
            .concurrency_manager
            .global_max
            .load(std::sync::atomic::Ordering::Relaxed);
        if current != expected {
            return Err(WebPlatformError::Conflict(format!(
                "Expected previous value {}, but current is {}",
                expected, current
            )));
        }
    }

    let previous = state.concurrency_manager.update_global_max(new_max);

    Ok(Json(ResponseData::success(ConcurrencyConfigResponse {
        global_max: new_max,
        previous_value: previous,
    })))
}

/// GET /api/projects/:id/concurrency
pub async fn get_project_concurrency(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<ProjectConcurrencyDetail>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;

    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    let info = if state
        .process_manager
        .get_state(project_id)
        .as_ref()
        .is_some_and(ProcessManager::is_active_process)
    {
        Some(ProjectConcurrencyInfo {
            project_id,
            project_name: project.name.clone(),
            active_agents: 1,
            max_agents: Some(project.max_concurrent_agents),
            queued_tasks: 0,
            service_status: project.service_status.clone(),
        })
    } else {
        Some(ProjectConcurrencyInfo {
            project_id,
            project_name: project.name.clone(),
            active_agents: 0,
            max_agents: Some(project.max_concurrent_agents),
            queued_tasks: 0,
            service_status: project.service_status.clone(),
        })
    };
    let (today_started, today_completed, avg_duration) =
        state.repo.get_today_stats(project_id).await?;

    let detail = ProjectConcurrencyDetail {
        project_id,
        project_name: project.name,
        active_agents: info.as_ref().map(|i| i.active_agents).unwrap_or(0),
        max_agents: info.as_ref().and_then(|i| i.max_agents),
        queued_tasks: info.as_ref().map(|i| i.queued_tasks).unwrap_or(0),
        today_started,
        today_completed,
        avg_duration_seconds: avg_duration,
    };

    Ok(Json(ResponseData::success(detail)))
}

/// PUT /api/projects/:id/concurrency
pub async fn update_project_concurrency(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(req): Json<UpdateProjectConcurrencyRequest>,
) -> Result<Json<ResponseData<serde_json::Value>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;

    // Check role is owner
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;
    let role = state.repo.get_member_role(project_id, user_id).await?;
    if role.as_deref() != Some("owner") && claims.role != "admin" {
        return Err(WebPlatformError::Forbidden);
    }

    let global_max = state
        .concurrency_manager
        .global_max
        .load(std::sync::atomic::Ordering::Relaxed);
    if req.max_agents > global_max {
        return Err(WebPlatformError::BadRequest(format!(
            "Project max_agents ({}) cannot exceed global max ({})",
            req.max_agents, global_max
        )));
    }

    if req.max_agents < 0 {
        return Err(WebPlatformError::BadRequest(
            "max_agents must be non-negative".to_string(),
        ));
    }

    let limit = if req.max_agents == 0 {
        None
    } else {
        Some(req.max_agents)
    };
    state
        .concurrency_manager
        .set_project_limit(project_id, limit);

    Ok(Json(ResponseData::success(serde_json::json!({
        "project_id": project_id,
        "max_agents": req.max_agents
    }))))
}

/// POST /api/admin/concurrency/events/ticket
pub async fn create_sse_ticket(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
) -> Result<Json<ResponseData<SseTicketResponse>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    let ticket = state.concurrency_manager.generate_ticket(user_id);
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(30);

    Ok(Json(ResponseData::success(SseTicketResponse {
        ticket,
        expires_at,
    })))
}

#[derive(Debug, serde::Deserialize)]
pub struct SseQuery {
    pub ticket: String,
}

/// GET /api/admin/concurrency/events?ticket=xxx
pub async fn concurrency_events_sse(
    State(state): State<AppState>,
    Query(query): Query<SseQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, WebPlatformError> {
    // Validate one-time ticket
    let _user_id = state
        .concurrency_manager
        .validate_ticket(&query.ticket)
        .ok_or(WebPlatformError::Unauthorized)?;

    let rx = state.concurrency_manager.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|result| match result {
            Ok(event) => {
                let json = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None,
        })
        .throttle(Duration::from_millis(200));

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}
