use axum::{
    extract::{Path, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::crypto;
use crate::error::WebPlatformError;
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::middleware::project_access::require_project_member;
use crate::models::issue::{IssueDetail, MergeRequestSummary};
use crate::models::kanban::{CreateIssueApiRequest, CreateIssueRequest};
use crate::models::ResponseData;
use crate::repository::{ProjectRepository, UserConfigRepository};
use crate::services::git_platform::{create_platform_client_with_proxy, GitPlatformError};
use crate::AppState;

/// POST /api/projects/:id/issues
///
/// Create a new issue in the project's GitLab/GitHub repository.
pub async fn create_issue(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Json(req): Json<CreateIssueApiRequest>,
) -> Result<Json<ResponseData<IssueDetail>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Check project membership
    require_project_member(&claims, project_id, &state.repo).await?;

    // Rate limit: 20/min/user for issue creation
    if let Err(retry_after) = state.phase3_rate_limiter.check("issues", user_id, 20) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    // Validate request
    if req.title.is_empty() || req.title.len() > 200 {
        return Err(WebPlatformError::BadRequest(
            "title must be 1-200 characters".to_string(),
        ));
    }
    if let Some(ref desc) = req.description {
        if desc.len() > 65536 {
            return Err(WebPlatformError::BadRequest(
                "description must be at most 65536 characters".to_string(),
            ));
        }
    }
    if let Some(ref labels) = req.labels {
        if labels.len() > 20 {
            return Err(WebPlatformError::BadRequest(
                "labels must have at most 20 items".to_string(),
            ));
        }
        for label in labels {
            if label.len() > 100 {
                return Err(WebPlatformError::BadRequest(
                    "each label must be at most 100 characters".to_string(),
                ));
            }
        }
    }
    if let Some(ref assignee) = req.assignee {
        if assignee.len() > 100 {
            return Err(WebPlatformError::BadRequest(
                "assignee must be at most 100 characters".to_string(),
            ));
        }
    }

    // Get project info
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Get user's platform token
    let (platform_token, _) = get_user_platform_token(&state, user_id, &project).await?;

    // Build project path
    let project_path = format!("{}/{}", project.namespace, project.repo_name);

    // Create platform client
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let client = create_platform_client_with_proxy(
        &project.platform,
        project.platform_host.as_deref(),
        Some(&proxy_config),
    )
    .map_err(map_platform_error)?;

    // Build the create request
    let create_req = CreateIssueRequest {
        title: req.title,
        description: req.description,
        labels: req.labels.unwrap_or_default(),
        assignee: req.assignee,
    };

    // Call platform API to create the issue
    let platform_issue = client
        .create_issue(&platform_token, &project_path, &create_req)
        .await
        .map_err(map_platform_error)?;

    // Invalidate kanban cache for this project/user
    let cache_prefix = format!("{}:{}:kanban:", user_id, project_id);
    state.api_cache.invalidate_prefix(&cache_prefix);

    // Convert to IssueDetail response
    let issue_detail = IssueDetail {
        iid: platform_issue.iid,
        title: platform_issue.title,
        description: platform_issue.description,
        state: platform_issue.state,
        labels: platform_issue.labels,
        author: platform_issue.author,
        assignees: platform_issue.assignees,
        milestone: platform_issue.milestone,
        created_at: platform_issue.created_at,
        updated_at: platform_issue.updated_at,
        closed_at: platform_issue.closed_at,
        web_url: platform_issue.web_url,
        comment_count: platform_issue.comment_count.unwrap_or(0),
        related_mrs: vec![],
    };

    Ok(Json(ResponseData::success(issue_detail)))
}

/// GET /api/projects/:id/issues/:iid
///
/// Get detailed information about a specific issue.
pub async fn get_issue(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, u64)>,
) -> Result<Json<ResponseData<IssueDetail>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Check project membership
    require_project_member(&claims, project_id, &state.repo).await?;

    // Rate limit: 60/min/user for GET endpoints
    if let Err(retry_after) = state.phase3_rate_limiter.check("issue_detail", user_id, 60) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    // Get project info
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Get user's platform token
    let (platform_token, _) = get_user_platform_token(&state, user_id, &project).await?;

    // Build project path
    let project_path = format!("{}/{}", project.namespace, project.repo_name);

    // Check cache
    let cache_key = format!("{}:{}:issue:{}:detail", user_id, project_id, iid);
    if let Some(cached_json) = state.api_cache.get(&cache_key) {
        if let Ok(issue_detail) = serde_json::from_str::<IssueDetail>(&cached_json) {
            return Ok(Json(ResponseData::success(issue_detail)));
        }
    }

    // Create platform client
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let client = create_platform_client_with_proxy(
        &project.platform,
        project.platform_host.as_deref(),
        Some(&proxy_config),
    )
    .map_err(map_platform_error)?;

    // Fetch issue detail
    let platform_issue = client
        .get_issue(&platform_token, &project_path, iid)
        .await
        .map_err(map_platform_error)?;

    // Fetch related MRs
    let related_mrs = client
        .get_issue_merge_requests(&platform_token, &project_path, iid)
        .await
        .unwrap_or_default();

    let mr_summaries: Vec<MergeRequestSummary> = related_mrs
        .into_iter()
        .map(|mr| MergeRequestSummary {
            iid: mr.iid,
            title: mr.title,
            state: mr.state,
            author: mr.author,
            web_url: mr.web_url,
        })
        .collect();

    let issue_detail = IssueDetail {
        iid: platform_issue.iid,
        title: platform_issue.title,
        description: platform_issue.description,
        state: platform_issue.state,
        labels: platform_issue.labels,
        author: platform_issue.author,
        assignees: platform_issue.assignees,
        milestone: platform_issue.milestone,
        created_at: platform_issue.created_at,
        updated_at: platform_issue.updated_at,
        closed_at: platform_issue.closed_at,
        web_url: platform_issue.web_url,
        comment_count: platform_issue.comment_count.unwrap_or(0),
        related_mrs: mr_summaries,
    };

    // Cache the result (5s TTL)
    if let Ok(json) = serde_json::to_string(&issue_detail) {
        state
            .api_cache
            .set_with_ttl(cache_key, json, std::time::Duration::from_secs(5));
    }

    Ok(Json(ResponseData::success(issue_detail)))
}

/// Helper: get the user's decrypted platform token for the given project.
/// Returns (token, platform_type).
pub(crate) async fn get_user_platform_token(
    state: &AppState,
    user_id: i64,
    project: &crate::models::Project,
) -> Result<(String, String), WebPlatformError> {
    let user_config = state.repo.get_config(user_id).await?.ok_or_else(|| {
        WebPlatformError::BadRequest(format!(
            "请先在个人设置中配置 {} Token",
            if project.platform == "github" {
                "GitHub"
            } else {
                "GitLab"
            }
        ))
    })?;

    let encrypted_token = match project.platform.as_str() {
        "github" => user_config.github_token,
        _ => user_config.gitlab_token,
    };

    let encrypted_token = encrypted_token.ok_or_else(|| {
        WebPlatformError::BadRequest(format!(
            "请先在个人设置中配置 {} Token",
            if project.platform == "github" {
                "GitHub"
            } else {
                "GitLab"
            }
        ))
    })?;

    let platform_token = crypto::decrypt(&encrypted_token, &state.encryption_key)?;
    Ok((platform_token, project.platform.clone()))
}

/// Map GitPlatformError to WebPlatformError.
pub(crate) fn map_platform_error(err: GitPlatformError) -> WebPlatformError {
    match err {
        GitPlatformError::TokenInvalid(msg) => WebPlatformError::TokenInvalid(msg),
        GitPlatformError::Forbidden(_) => WebPlatformError::Forbidden,
        GitPlatformError::NotFound(msg) => WebPlatformError::NotFound(msg),
        GitPlatformError::Validation { message, .. } => WebPlatformError::BadRequest(message),
        GitPlatformError::Conflict { message, .. } => WebPlatformError::Conflict(message),
        GitPlatformError::ServiceUnavailable(msg) => WebPlatformError::ExternalService(msg),
        GitPlatformError::RequestError(msg) => WebPlatformError::ExternalService(msg),
    }
}
