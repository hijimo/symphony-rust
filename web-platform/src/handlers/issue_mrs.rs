use axum::{
    extract::{Path, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::handlers::issues::{get_user_platform_token, map_platform_error};
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::middleware::project_access::require_project_member;
use crate::models::issue::MergeRequestSummary;
use crate::models::ResponseData;
use crate::repository::ProjectRepository;
use crate::services::git_platform::create_platform_client_with_proxy;
use crate::AppState;

/// GET /api/projects/:id/issues/:iid/mrs
///
/// Get the list of MRs/PRs associated with a specific issue.
pub async fn get_issue_mrs(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, u64)>,
) -> Result<Json<ResponseData<Vec<MergeRequestSummary>>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Check project membership
    require_project_member(&claims, project_id, &state.repo).await?;

    // Rate limit: 60/min/user for GET endpoints
    if let Err(retry_after) = state.phase3_rate_limiter.check("issue_mrs", user_id, 60) {
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
    let cache_key = format!("{}:{}:issue:{}:mrs", user_id, project_id, iid);
    if let Some(cached_json) = state.api_cache.get(&cache_key) {
        if let Ok(mrs) = serde_json::from_str::<Vec<MergeRequestSummary>>(&cached_json) {
            return Ok(Json(ResponseData::success(mrs)));
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

    // Fetch related MRs
    let platform_mrs = client
        .get_issue_merge_requests(&platform_token, &project_path, iid)
        .await
        .map_err(map_platform_error)?;

    let mr_summaries: Vec<MergeRequestSummary> = platform_mrs
        .into_iter()
        .map(|mr| MergeRequestSummary {
            iid: mr.iid,
            title: mr.title,
            state: mr.state,
            author: mr.author,
            web_url: mr.web_url,
        })
        .collect();

    // Cache the result (5s TTL)
    if let Ok(json) = serde_json::to_string(&mr_summaries) {
        state
            .api_cache
            .set_with_ttl(cache_key, json, std::time::Duration::from_secs(5));
    }

    Ok(Json(ResponseData::success(mr_summaries)))
}
