use axum::{
    extract::{Path, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::git_url::Platform;
use crate::middleware::project_access::{require_project_member, require_project_owner};
use crate::models::{ResponseData, UpdateWorkflowRequest, WorkflowResponse};
use crate::repository::ProjectRepository;
use crate::templates::{self, WorkflowTemplateContext};
use crate::AppState;

/// GET /api/projects/:id/workflow - Get the project's WORKFLOW.md content.
pub async fn get_workflow(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<WorkflowResponse>>, WebPlatformError> {
    require_project_member(&claims, project_id, &state.repo).await?;

    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    let content = if project.workflow_template == "custom" {
        project.workflow_content.unwrap_or_default()
    } else {
        // Render default template with project variables
        let platform = match project.platform.as_str() {
            "github" => Platform::GitHub,
            "gitea" => Platform::Gitea,
            _ => Platform::GitLab,
        };
        let ctx = WorkflowTemplateContext {
            platform,
            project_slug: format!("{}/{}", project.namespace, project.repo_name),
            platform_host: project
                .platform_host
                .unwrap_or_else(|| "gitlab.com".to_string()),
            workspace_root: format!("~/symphony-workspaces/{}", project.id),
            max_concurrent_agents: project.max_concurrent_agents,
            default_branch: project.default_branch,
            hooks_after_create: project.hooks_after_create,
            hooks_before_remove: project.hooks_before_remove,
            codex_command: project.codex_command,
            codex_approval_policy: project.codex_approval_policy,
            codex_sandbox: project.codex_sandbox,
            testing_max_turns: Some(project.testing_max_turns),
            testing_skip_labels: project.testing_skip_labels,
        };
        templates::render_template(&ctx)
    };

    let response = WorkflowResponse {
        template_mode: project.workflow_template,
        content,
    };

    Ok(Json(ResponseData::success(response)))
}

/// PUT /api/projects/:id/workflow - Update the project's WORKFLOW.md configuration.
pub async fn update_workflow(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(req): Json<UpdateWorkflowRequest>,
) -> Result<Json<ResponseData<WorkflowResponse>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    // Validate template_mode
    if req.template_mode != "default" && req.template_mode != "custom" {
        return Err(WebPlatformError::BadRequest(
            "template_mode must be 'default' or 'custom'".to_string(),
        ));
    }

    let content = if req.template_mode == "custom" {
        match req.content {
            Some(ref c) if !c.is_empty() => Some(c.as_str()),
            _ => {
                return Err(WebPlatformError::BadRequest(
                    "content is required when template_mode is 'custom'".to_string(),
                ));
            }
        }
    } else {
        None
    };

    state
        .repo
        .update_workflow(project_id, &req.template_mode, content)
        .await?;

    // Return the updated workflow
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    let rendered_content = if req.template_mode == "custom" {
        content.unwrap_or_default().to_string()
    } else {
        let platform = match project.platform.as_str() {
            "github" => Platform::GitHub,
            "gitea" => Platform::Gitea,
            _ => Platform::GitLab,
        };
        let ctx = WorkflowTemplateContext {
            platform,
            project_slug: format!("{}/{}", project.namespace, project.repo_name),
            platform_host: project
                .platform_host
                .unwrap_or_else(|| "gitlab.com".to_string()),
            workspace_root: format!("~/symphony-workspaces/{}", project.id),
            max_concurrent_agents: project.max_concurrent_agents,
            default_branch: project.default_branch,
            hooks_after_create: project.hooks_after_create,
            hooks_before_remove: project.hooks_before_remove,
            codex_command: project.codex_command,
            codex_approval_policy: project.codex_approval_policy,
            codex_sandbox: project.codex_sandbox,
            testing_max_turns: Some(project.testing_max_turns),
            testing_skip_labels: project.testing_skip_labels,
        };
        templates::render_template(&ctx)
    };

    let response = WorkflowResponse {
        template_mode: req.template_mode,
        content: rendered_content,
    };

    Ok(Json(ResponseData::success(response)))
}

/// POST /api/projects/:id/workflow/reset - Reset workflow to default template.
pub async fn reset_workflow(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<WorkflowResponse>>, WebPlatformError> {
    require_project_owner(&claims, project_id, &state.repo).await?;

    // Reset to default (clear custom content)
    state
        .repo
        .update_workflow(project_id, "default", None)
        .await?;

    // Return rendered default template
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    let platform = match project.platform.as_str() {
        "github" => Platform::GitHub,
        _ => Platform::GitLab,
    };
    let ctx = WorkflowTemplateContext {
        platform,
        project_slug: format!("{}/{}", project.namespace, project.repo_name),
        platform_host: project
            .platform_host
            .unwrap_or_else(|| "gitlab.com".to_string()),
        workspace_root: format!("~/symphony-workspaces/{}", project.id),
        max_concurrent_agents: project.max_concurrent_agents,
        default_branch: project.default_branch,
        hooks_after_create: project.hooks_after_create,
        hooks_before_remove: project.hooks_before_remove,
        codex_command: project.codex_command,
        codex_approval_policy: project.codex_approval_policy,
        codex_sandbox: project.codex_sandbox,
        testing_max_turns: Some(project.testing_max_turns),
        testing_skip_labels: project.testing_skip_labels,
    };
    let content = templates::render_template(&ctx);

    let response = WorkflowResponse {
        template_mode: "default".to_string(),
        content,
    };

    Ok(Json(ResponseData::success(response)))
}
