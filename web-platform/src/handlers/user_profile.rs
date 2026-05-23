use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::jwt::Claims;
use crate::crypto;
use crate::error::WebPlatformError;
use crate::models::ResponseData;
use crate::repository::{UserConfigRepository, UserRepository};
use crate::AppState;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,
    pub created_at: String,
}

#[utoipa::path(
    get,
    path = "/api/user/profile",
    responses(
        (status = 200, description = "User profile"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_profile(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
) -> Result<Json<ResponseData<UserProfile>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    let user = state
        .repo
        .find_by_id(user_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("user not found".to_string()))?;

    Ok(Json(ResponseData::success(UserProfile {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        role: user.role,
        created_at: user.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
    })))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileRequest {
    pub display_name: String,
}

#[utoipa::path(
    put,
    path = "/api/user/profile",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Profile updated"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_profile(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    state
        .repo
        .update_display_name(user_id, &req.display_name)
        .await?;

    Ok(Json(ResponseData::success(())))
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserConfigResponse {
    pub has_gitlab_token: bool,
    pub gitlab_host: Option<String>,
    pub has_github_token: bool,
}

#[utoipa::path(
    get,
    path = "/api/user/config",
    responses(
        (status = 200, description = "User config"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_config(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
) -> Result<Json<ResponseData<UserConfigResponse>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    let config = state.repo.get_config(user_id).await?;

    match config {
        Some(c) => Ok(Json(ResponseData::success(UserConfigResponse {
            has_gitlab_token: c.gitlab_token.is_some(),
            gitlab_host: c.gitlab_host,
            has_github_token: c.github_token.is_some(),
        }))),
        None => Ok(Json(ResponseData::success(UserConfigResponse {
            has_gitlab_token: false,
            gitlab_host: None,
            has_github_token: false,
        }))),
    }
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigRequest {
    pub gitlab_token: Option<String>,
    pub gitlab_host: Option<String>,
    pub github_token: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/user/config",
    request_body = UpdateConfigRequest,
    responses(
        (status = 200, description = "Config updated"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_config(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    let encrypted_gitlab = req
        .gitlab_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| crypto::encrypt(t, &state.encryption_key))
        .transpose()?;

    let encrypted_github = req
        .github_token
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| crypto::encrypt(t, &state.encryption_key))
        .transpose()?;

    state
        .repo
        .upsert_config(
            user_id,
            encrypted_gitlab.as_deref(),
            req.gitlab_host.as_deref(),
            encrypted_github.as_deref(),
        )
        .await?;

    Ok(Json(ResponseData::success(())))
}
