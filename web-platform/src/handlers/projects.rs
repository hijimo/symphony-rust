use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Serialize;
use utoipa::ToSchema;

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::git_url::{parse_git_url, Platform};
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::{
    CreateProjectRequest, NewProject, PaginationData, Project, ProjectListQuery, ProjectUpdate,
    ResponseData, UpdateProjectRequest,
};
use crate::repository::{ProjectListFilter, ProjectMemberRepository, ProjectRepository};
use crate::AppState;

/// Response type for a single project (with computed fields).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResponse {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub git_url: String,
    pub platform: String,
    pub platform_host: Option<String>,
    pub namespace: String,
    pub repo_name: String,
    pub default_branch: String,
    pub workflow_template: String,
    pub service_status: String,
    pub service_pid: Option<i64>,
    pub max_concurrent_agents: i64,
    pub auto_restart: bool,
    pub member_count: i64,
    pub my_role: Option<String>,
    pub created_by: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

impl ProjectResponse {
    pub fn from_project(project: Project, member_count: i64, my_role: Option<String>) -> Self {
        Self {
            id: project.id,
            name: project.name,
            description: project.description,
            git_url: project.git_url,
            platform: project.platform,
            platform_host: project.platform_host,
            namespace: project.namespace,
            repo_name: project.repo_name,
            default_branch: project.default_branch,
            workflow_template: project.workflow_template,
            service_status: project.service_status,
            service_pid: project.service_pid,
            max_concurrent_agents: project.max_concurrent_agents,
            auto_restart: project.auto_restart,
            member_count,
            my_role,
            created_by: project.created_by,
            created_at: project.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            updated_at: project.updated_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        }
    }

    pub fn from_project_with_computed(project: Project) -> Self {
        let member_count = project.member_count.unwrap_or(0);
        let my_role = project.my_role.clone();
        Self::from_project(project, member_count, my_role)
    }
}

/// GET /api/projects - List projects visible to the current user.
pub async fn list_projects(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Query(query): Query<ProjectListQuery>,
) -> Result<Json<ResponseData<PaginationData<ProjectResponse>>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    let is_admin = claims.role == "admin";
    let page_no = query.page_no.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    let (projects, total) = state
        .repo
        .list_projects_for_user(ProjectListFilter {
            user_id,
            is_admin,
            page_no,
            page_size,
            platform: query.platform.as_deref(),
            status: query.status.as_deref(),
            search: query.search.as_deref(),
        })
        .await?;

    let items: Vec<ProjectResponse> = projects
        .into_iter()
        .map(|p| {
            let mut resp = ProjectResponse::from_project_with_computed(p);
            if is_admin && resp.my_role.is_none() {
                resp.my_role = Some("admin".to_string());
            }
            resp
        })
        .collect();

    Ok(Json(ResponseData::success(PaginationData::new(
        items, total, page_no, page_size,
    ))))
}

/// POST /api/projects - Create a new project.
pub async fn create_project(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ResponseData<ProjectResponse>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Validate git_url
    if req.git_url.len() < 10 || req.git_url.len() > 500 {
        return Err(WebPlatformError::BadRequest(
            "git_url must be 10-500 characters".to_string(),
        ));
    }

    // Parse the git URL
    let parsed = parse_git_url(&req.git_url)
        .map_err(|e| WebPlatformError::BadRequest(format!("Invalid git URL: {}", e)))?;

    // Determine project name
    let name = req.name.unwrap_or_else(|| parsed.repo_name.clone());

    if name.is_empty() || name.len() > 100 {
        return Err(WebPlatformError::BadRequest(
            "name must be 1-100 characters".to_string(),
        ));
    }

    // Validate workflow template
    let workflow_template = req
        .workflow_template
        .unwrap_or_else(|| "default".to_string());
    if workflow_template != "default" && workflow_template != "custom" {
        return Err(WebPlatformError::BadRequest(
            "workflow_template must be 'default' or 'custom'".to_string(),
        ));
    }

    let workflow_content = if workflow_template == "custom" {
        match req.workflow_content {
            Some(ref content) if !content.is_empty() => Some(content.clone()),
            _ => {
                return Err(WebPlatformError::BadRequest(
                    "workflow_content is required when workflow_template is 'custom'".to_string(),
                ));
            }
        }
    } else {
        None
    };

    let default_branch = req.default_branch.unwrap_or_else(|| "main".to_string());

    let platform_host = match parsed.platform {
        Platform::GitHub => None,
        Platform::GitLab => {
            if parsed.host.contains("github.com") {
                None
            } else {
                // Store the full base URL including scheme for custom GitLab instances
                let scheme = if req.git_url.starts_with("http://") {
                    "http"
                } else {
                    "https"
                };
                Some(format!("{}://{}", scheme, parsed.host))
            }
        }
        Platform::Gitea => {
            let scheme = if req.git_url.starts_with("http://") {
                "http"
            } else {
                "https"
            };
            Some(format!("{}://{}", scheme, parsed.host))
        }
    };

    let new_project = NewProject {
        name,
        description: req.description,
        git_url: parsed.normalized_url,
        platform: parsed.platform.to_string(),
        platform_host,
        namespace: parsed.namespace,
        repo_name: parsed.repo_name,
        default_branch,
        workflow_template,
        workflow_content,
        created_by: user_id,
    };

    let project = state.repo.create_project(&new_project).await?;
    let project_id = project.id;

    // Add creator as owner
    let _ = state
        .repo
        .add_member(project_id, user_id, "owner", None)
        .await;

    // Fetch updated project with member count
    let member_count = state.repo.count_members(project_id).await.unwrap_or(1);
    let resp = ProjectResponse::from_project(project, member_count, Some("owner".to_string()));

    Ok(Json(ResponseData::success(resp)))
}

/// GET /api/projects/:id - Get project details.
pub async fn get_project(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<ResponseData<ProjectResponse>>, WebPlatformError> {
    let my_role = require_project_member(&claims, id, &state.repo).await?;

    let project = state
        .repo
        .get_project(id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    let member_count = state.repo.count_members(id).await.unwrap_or(0);
    let resp = ProjectResponse::from_project(project, member_count, Some(my_role));

    Ok(Json(ResponseData::success(resp)))
}

/// PUT /api/projects/:id - Update project configuration.
pub async fn update_project(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Json<ResponseData<ProjectResponse>>, WebPlatformError> {
    let my_role = require_project_owner(&claims, id, &state.repo).await?;

    // Validate fields
    if let Some(ref name) = req.name {
        if name.is_empty() || name.len() > 100 {
            return Err(WebPlatformError::BadRequest(
                "name must be 1-100 characters".to_string(),
            ));
        }
    }
    if let Some(ref desc) = req.description {
        if desc.len() > 500 {
            return Err(WebPlatformError::BadRequest(
                "description must be at most 500 characters".to_string(),
            ));
        }
    }
    if let Some(ref branch) = req.default_branch {
        if branch.is_empty() || branch.len() > 100 {
            return Err(WebPlatformError::BadRequest(
                "default_branch must be 1-100 characters".to_string(),
            ));
        }
    }
    if let Some(max_agents) = req.max_concurrent_agents {
        if !(1..=20).contains(&max_agents) {
            return Err(WebPlatformError::BadRequest(
                "max_concurrent_agents must be 1-20".to_string(),
            ));
        }
    }
    if let Some(v) = req.testing_max_turns {
        if !(5..=30).contains(&v) {
            return Err(WebPlatformError::BadRequest(
                "testing_max_turns must be 5-30".to_string(),
            ));
        }
    }

    let updates = ProjectUpdate {
        name: req.name,
        description: req.description,
        default_branch: req.default_branch,
        max_concurrent_agents: req.max_concurrent_agents,
        auto_restart: req.auto_restart,
        hooks_after_create: req.hooks_after_create,
        hooks_before_remove: req.hooks_before_remove,
        codex_command: req.codex_command,
        codex_approval_policy: req.codex_approval_policy,
        codex_sandbox: req.codex_sandbox,
        testing_enabled: req.testing_enabled,
        testing_max_turns: req.testing_max_turns,
    };

    state.repo.update_project(id, &updates).await?;

    // Return updated project
    let project = state
        .repo
        .get_project(id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    let member_count = state.repo.count_members(id).await.unwrap_or(0);
    let resp = ProjectResponse::from_project(project, member_count, Some(my_role));

    Ok(Json(ResponseData::success(resp)))
}

/// DELETE /api/projects/:id - Delete a project.
pub async fn delete_project(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(id): Path<i64>,
) -> Result<Json<ResponseData<()>>, WebPlatformError> {
    require_project_owner(&claims, id, &state.repo).await?;

    // Check service status - must be stopped
    let project = state
        .repo
        .get_project(id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    if project.service_status != "stopped" && project.service_status != "failed" {
        return Err(WebPlatformError::Conflict(
            "Cannot delete project while service is running. Please stop the service first."
                .to_string(),
        ));
    }

    state.repo.delete_project(id).await?;

    // Clean up process manager state
    state.process_manager.remove_state(id);

    Ok(Json(ResponseData::success(())))
}
