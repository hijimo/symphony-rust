//! `GET /api/admin/org-config`, `POST /api/admin/org-config/sync` (P3 §4 O1).

use axum::extract::State;
use axum::Json;

use crate::error::WebPlatformError;
use crate::models::ResponseData;
use crate::sdlc::org_config::SyncStatus;
use crate::AppState;

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
