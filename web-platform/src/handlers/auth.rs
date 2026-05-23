use axum::{
    extract::{ConnectInfo, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use utoipa::ToSchema;

use crate::auth::jwt::{generate_token, invalidate_user_tokens, Claims};
use crate::auth::password::{hash_password, verify_password};
use crate::error::WebPlatformError;
use crate::models::ResponseData;
use crate::repository::UserRepository;
use crate::AppState;

#[derive(Deserialize, ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: String,
    pub user: LoginUser,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginUser {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Login successful"),
        (status = 401, description = "Invalid credentials"),
        (status = 429, description = "Rate limited"),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    connect_info: ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<ResponseData<LoginResponse>>, WebPlatformError> {
    if req.username.is_empty() || req.password.is_empty() {
        return Err(WebPlatformError::BadRequest(
            "username and password are required".to_string(),
        ));
    }

    let ip = connect_info.0.ip().to_string();
    state.rate_limiter.check_rate_limit(&req.username, &ip)?;

    let user = state
        .repo
        .find_by_username(&req.username)
        .await?
        .ok_or(WebPlatformError::InvalidCredentials)?;

    if user.deleted_at.is_some() {
        return Err(WebPlatformError::InvalidCredentials);
    }

    let valid = verify_password(&req.password, &user.password_hash)?;
    if !valid {
        return Err(WebPlatformError::InvalidCredentials);
    }

    let (token, expires_at) =
        generate_token(user.id, &user.username, &user.role, &state.jwt_secret)?;

    Ok(Json(ResponseData::success(LoginResponse {
        token,
        expires_at: expires_at.to_rfc3339(),
        user: LoginUser {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            role: user.role,
        },
    })))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[utoipa::path(
    put,
    path = "/api/auth/password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 200, description = "Password changed"),
        (status = 401, description = "Invalid old password"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn change_password(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    if req.new_password.len() < 6 || req.new_password.len() > 128 {
        return Err(WebPlatformError::BadRequest(
            "password must be 6-128 characters".to_string(),
        ));
    }

    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    let user = state
        .repo
        .find_by_id(user_id)
        .await?
        .ok_or(WebPlatformError::Unauthorized)?;

    let valid = verify_password(&req.old_password, &user.password_hash)?;
    if !valid {
        return Err(WebPlatformError::Unauthorized);
    }

    let new_hash = hash_password(&req.new_password)?;
    state.repo.update_password(user_id, &new_hash).await?;

    invalidate_user_tokens(user_id, &state.token_blacklist, &state.repo).await;

    Ok(Json(ResponseData::success(())))
}
