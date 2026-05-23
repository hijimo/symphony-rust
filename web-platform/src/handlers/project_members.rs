use axum::{
    extract::{Path, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::handlers::issues::{get_user_platform_token, map_platform_error};
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::{
    AddMemberRequest, ProjectMember, ResponseData, SyncMember, SyncResult, UpdateMemberRoleRequest,
};
use crate::repository::{ProjectMemberRepository, ProjectRepository, UserRepository};
use crate::services::git_platform::create_platform_client;
use crate::AppState;

/// GET /api/projects/:id/members - List project members.
pub async fn list_members(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<Vec<ProjectMember>>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;

    let members = state.repo.list_members(project_id).await?;

    Ok(Json(ResponseData::success(members)))
}

/// POST /api/projects/:id/members - Add a member to the project.
pub async fn add_member(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(req): Json<AddMemberRequest>,
) -> Result<Json<ResponseData<ProjectMember>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    // Validate role
    let role = req.role.unwrap_or_else(|| "member".to_string());
    if role != "owner" && role != "member" {
        return Err(WebPlatformError::BadRequest(
            "role must be 'owner' or 'member'".to_string(),
        ));
    }

    // Verify user exists
    let _user = state
        .repo
        .find_by_id(req.user_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("User not found".to_string()))?;

    let member = state
        .repo
        .add_member(project_id, req.user_id, &role, None)
        .await?;

    Ok(Json(ResponseData::success(member)))
}

/// PUT /api/projects/:id/members/:user_id - Update a member's role.
pub async fn update_member_role(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, target_user_id)): Path<(i64, i64)>,
    Json(req): Json<UpdateMemberRoleRequest>,
) -> Result<Json<ResponseData<ProjectMember>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    let current_user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Cannot modify own role
    if current_user_id == target_user_id {
        return Err(WebPlatformError::Conflict(
            "Cannot modify your own role".to_string(),
        ));
    }

    // Validate role
    if req.role != "owner" && req.role != "member" {
        return Err(WebPlatformError::BadRequest(
            "role must be 'owner' or 'member'".to_string(),
        ));
    }

    // Check if target is currently an owner and we're downgrading
    let current_role = state
        .repo
        .get_member_role(project_id, target_user_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Member not found in project".to_string()))?;

    if current_role == "owner" && req.role == "member" {
        // Ensure at least one owner remains
        let owner_count = state.repo.count_owners(project_id).await?;
        if owner_count <= 1 {
            return Err(WebPlatformError::Conflict(
                "Cannot downgrade the last owner. Transfer ownership first.".to_string(),
            ));
        }
    }

    state
        .repo
        .update_member_role(project_id, target_user_id, &req.role)
        .await?;

    // Return updated member list entry
    let members = state.repo.list_members(project_id).await?;
    let member = members
        .into_iter()
        .find(|m| m.user_id == target_user_id)
        .ok_or_else(|| WebPlatformError::Internal("Member not found after update".to_string()))?;

    Ok(Json(ResponseData::success(member)))
}

/// DELETE /api/projects/:id/members/:user_id - Remove a member from the project.
pub async fn remove_member(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, target_user_id)): Path<(i64, i64)>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    let current_user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Cannot remove yourself
    if current_user_id == target_user_id {
        return Err(WebPlatformError::Conflict(
            "Cannot remove yourself from the project. Transfer ownership first.".to_string(),
        ));
    }

    // Check if target is an owner
    let target_role = state
        .repo
        .get_member_role(project_id, target_user_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Member not found in project".to_string()))?;

    if target_role == "owner" {
        let owner_count = state.repo.count_owners(project_id).await?;
        if owner_count <= 1 {
            return Err(WebPlatformError::Conflict(
                "Cannot remove the last owner.".to_string(),
            ));
        }
    }

    state.repo.remove_member(project_id, target_user_id).await?;

    Ok(Json(ResponseData::success(())))
}

/// POST /api/projects/:id/members/sync - Sync members from the platform.
pub async fn sync_members(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<SyncResult>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Get project to determine platform
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    let (platform_token, _) = get_user_platform_token(&state, user_id, &project).await?;

    let client = create_platform_client(&project.platform, project.platform_host.as_deref());
    let project_path = format!("{}/{}", project.namespace, project.repo_name);

    let platform_members_raw = client
        .list_members(&platform_token, &project_path)
        .await
        .map_err(map_platform_error)?;

    let platform_members: Vec<SyncMember> = platform_members_raw
        .into_iter()
        .map(|m| SyncMember {
            username: m.username,
            role: m.access_level,
            synced_from: project.platform.clone(),
        })
        .collect();

    let result = state
        .repo
        .sync_members(project_id, &platform_members)
        .await?;

    Ok(Json(ResponseData::success(result)))
}
