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
use crate::models::ResponseData;
use crate::repository::{ConcurrencyRepository, ProjectMemberRepository, ProjectRepository};
use crate::AppState;

/// GET /api/admin/concurrency
pub async fn get_global_concurrency(
    State(state): State<AppState>,
    _claims: axum::Extension<Claims>,
) -> Result<Json<ResponseData<ConcurrencyStatus>>, WebPlatformError> {
    let status = state.concurrency_manager.get_status();
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

    if new_max < 1 || new_max > 100 {
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

    let info = state.concurrency_manager.get_project_status(project_id);
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
