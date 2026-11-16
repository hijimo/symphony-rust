//! `GET /api/admin/org-config`, `POST /api/admin/org-config/sync` (P3 §4 O1),
//! `GET|PUT /api/projects/{id}/org-config` (O4).

use axum::extract::{Path, State};
use axum::{Extension, Json};
use serde::Deserialize;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::ResponseData;
use crate::sdlc::org_config::{ProjectOrgConfig, SyncStatus};
use crate::AppState;

type ProjectResult = Result<Json<ResponseData<ProjectOrgConfig>>, WebPlatformError>;

pub async fn get_project_org_config(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
) -> ProjectResult {
    let role = require_project_member(&claims, project_id, &state.repo).await?;
    let can_edit = role == "owner" || role == "admin";
    Ok(Json(ResponseData::success(
        state
            .sdlc
            .org_config
            .project_state(&state.repo, project_id, can_edit)
            .await?,
    )))
}

#[derive(Debug, Deserialize)]
pub struct PinRequest {
    pub pin: Option<String>,
}

pub async fn put_project_org_config(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(req): Json<PinRequest>,
) -> ProjectResult {
    require_project_owner(&claims, project_id, &state.repo).await?;
    let org = &state.sdlc.org_config;
    org.set_pin(&state.repo, project_id, req.pin.as_deref())
        .await?;
    Ok(Json(ResponseData::success(
        org.project_state(&state.repo, project_id, true).await?,
    )))
}

pub async fn get_org_config(
    State(state): State<AppState>,
) -> Result<Json<ResponseData<SyncStatus>>, WebPlatformError> {
    Ok(Json(ResponseData::success(
        state.sdlc.org_config.status(&state.repo).await?,
    )))
}

pub async fn sync_org_config(
    State(state): State<AppState>,
) -> Result<Json<ResponseData<SyncStatus>>, WebPlatformError> {
    Ok(Json(ResponseData::success(
        state.sdlc.org_config.sync(&state.repo).await?,
    )))
}
