use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::repository::{ProjectMemberRepository, ProjectRepository, SqliteRepository};

/// Check that the current user has access to the project (is a member or admin).
/// Returns the user's role in the project context.
pub async fn require_project_member(
    claims: &Claims,
    project_id: i64,
    repo: &SqliteRepository,
) -> Result<String, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Admin can access any project
    if claims.role == "admin" {
        // Verify project exists
        repo.get_project(project_id)
            .await?
            .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;
        return Ok("admin".to_string());
    }

    // Verify project exists
    repo.get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Check membership
    let role = repo
        .get_member_role(project_id, user_id)
        .await?
        .ok_or(WebPlatformError::Forbidden)?;

    Ok(role)
}

/// Check that the current user is a project owner or system admin.
/// Returns the user's effective role.
pub async fn require_project_owner(
    claims: &Claims,
    project_id: i64,
    repo: &SqliteRepository,
) -> Result<String, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Admin can do anything
    if claims.role == "admin" {
        // Verify project exists
        repo.get_project(project_id)
            .await?
            .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;
        return Ok("admin".to_string());
    }

    // Verify project exists
    repo.get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Check membership and role
    let role = repo
        .get_member_role(project_id, user_id)
        .await?
        .ok_or(WebPlatformError::Forbidden)?;

    if role != "owner" {
        return Err(WebPlatformError::Forbidden);
    }

    Ok(role)
}
