use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth::jwt::{invalidate_user_tokens, Claims};
use crate::auth::password::hash_password;
use crate::error::WebPlatformError;
use crate::models::{PaginationData, ResponseData};
use crate::repository::UserRepository;
use crate::AppState;

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListUsersQuery {
    #[param(default = 1)]
    pub page_no: Option<i64>,
    #[param(default = 20)]
    pub page_size: Option<i64>,
    pub search: Option<String>,
    pub role: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/users",
    params(ListUsersQuery),
    responses(
        (status = 200, description = "User list"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_users(
    State(state): State<AppState>,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<ResponseData<PaginationData<UserInfo>>>, WebPlatformError> {
    let page_no = query.page_no.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    let (users, total) = state
        .repo
        .list_users(
            page_no,
            page_size,
            query.search.as_deref(),
            query.role.as_deref(),
        )
        .await?;

    let items: Vec<UserInfo> = users
        .into_iter()
        .map(|u| UserInfo {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            role: u.role,
            created_at: u.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            updated_at: u.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        })
        .collect();

    Ok(Json(ResponseData::success(PaginationData::new(
        items, total, page_no, page_size,
    ))))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    pub role: String,
}

fn validate_username(username: &str) -> Result<(), WebPlatformError> {
    if username.len() < 3 || username.len() > 32 {
        return Err(WebPlatformError::BadRequest(
            "username must be 3-32 characters".to_string(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(WebPlatformError::BadRequest(
            "username can only contain letters, digits, and underscores".to_string(),
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), WebPlatformError> {
    if password.len() < 6 || password.len() > 128 {
        return Err(WebPlatformError::BadRequest(
            "password must be 6-128 characters".to_string(),
        ));
    }
    Ok(())
}

#[utoipa::path(
    post,
    path = "/api/admin/users",
    request_body = CreateUserRequest,
    responses(
        (status = 200, description = "User created"),
        (status = 409, description = "Username already exists"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_user(
    State(state): State<AppState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    validate_username(&req.username)?;
    validate_password(&req.password)?;

    if req.role != "admin" && req.role != "user" {
        return Err(WebPlatformError::BadRequest(
            "role must be 'admin' or 'user'".to_string(),
        ));
    }

    let password_hash = hash_password(&req.password)?;

    state
        .repo
        .create_user(
            &req.username,
            &password_hash,
            req.display_name.as_deref(),
            &req.role,
        )
        .await?;

    Ok(Json(ResponseData::success(())))
}

#[utoipa::path(
    delete,
    path = "/api/admin/users/{id}",
    params(("id" = i64, Path, description = "User ID")),
    responses(
        (status = 200, description = "User deleted"),
        (status = 404, description = "User not found"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_user(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    let current_user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    if current_user_id == id {
        return Err(WebPlatformError::BadRequest(
            "cannot delete yourself".to_string(),
        ));
    }

    state.repo.soft_delete(id).await?;

    Ok(Json(ResponseData::success(())))
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    pub new_password: String,
}

#[utoipa::path(
    put,
    path = "/api/admin/users/{id}/reset-password",
    params(("id" = i64, Path, description = "User ID")),
    request_body = ResetPasswordRequest,
    responses(
        (status = 200, description = "Password reset"),
        (status = 404, description = "User not found"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn reset_password(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(id): Path<i64>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    let current_user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    if current_user_id == id {
        return Err(WebPlatformError::BadRequest(
            "cannot reset your own password here, use change password instead".to_string(),
        ));
    }

    validate_password(&req.new_password)?;

    let new_hash = hash_password(&req.new_password)?;
    state.repo.update_password(id, &new_hash).await?;

    invalidate_user_tokens(id, &state.token_blacklist, &state.repo).await;

    Ok(Json(ResponseData::success(())))
}
