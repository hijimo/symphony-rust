use axum::{
    extract::{Path, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::error::WebPlatformError;
use crate::handlers::issues::{get_user_platform_token, map_platform_error};
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::middleware::project_access::require_project_member;
use crate::models::issue::IssueSummary;
use crate::models::merge_request::{MergeRequestDetail, Reviewer};
use crate::models::ResponseData;
use crate::repository::ProjectRepository;
use crate::services::git_platform::create_platform_client_with_proxy;
use crate::AppState;

/// GET /api/projects/:id/mrs/:iid
///
/// Get detailed information about a specific merge request / pull request.
pub async fn get_merge_request(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path((project_id, iid)): Path<(i64, u64)>,
) -> Result<Json<ResponseData<MergeRequestDetail>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Check project membership
    require_project_member(&claims, project_id, &state.repo).await?;

    // Rate limit: 60/min/user for GET endpoints
    if let Err(retry_after) = state.phase3_rate_limiter.check("mr_detail", user_id, 60) {
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
    let cache_key = format!("{}:{}:mr:{}:detail", user_id, project_id, iid);
    if let Some(cached_json) = state.api_cache.get(&cache_key) {
        if let Ok(mr_detail) = serde_json::from_str::<MergeRequestDetail>(&cached_json) {
            return Ok(Json(ResponseData::success(mr_detail)));
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

    // Fetch MR detail
    let platform_mr = client
        .get_merge_request(&platform_token, &project_path, iid)
        .await
        .map_err(map_platform_error)?;

    // Convert related_issue_iids to IssueSummary (we'd need to fetch each, but for now
    // we construct minimal summaries from the iids)
    let related_issues: Vec<IssueSummary> = platform_mr
        .related_issue_iids
        .iter()
        .map(|&issue_iid| IssueSummary {
            iid: issue_iid,
            title: String::new(), // Will be populated by a follow-up fetch if needed
            state: "opened".to_string(),
            web_url: String::new(),
        })
        .collect();

    // If we have issue iids, try to fetch their details in parallel
    let related_issues = if !platform_mr.related_issue_iids.is_empty() {
        let mut issue_futures = Vec::new();
        for &issue_iid in &platform_mr.related_issue_iids {
            let token = platform_token.clone();
            let path = project_path.clone();
            let client_ref = &client;
            issue_futures
                .push(async move { client_ref.get_issue(&token, &path, issue_iid).await.ok() });
        }
        let results = futures::future::join_all(issue_futures).await;
        results
            .into_iter()
            .flatten()
            .map(|issue| IssueSummary {
                iid: issue.iid,
                title: issue.title,
                state: issue.state,
                web_url: issue.web_url,
            })
            .collect()
    } else {
        related_issues
    };

    let mr_detail = MergeRequestDetail {
        iid: platform_mr.iid,
        title: platform_mr.title,
        description: platform_mr.description,
        state: platform_mr.state,
        author: platform_mr.author,
        source_branch: platform_mr.source_branch,
        target_branch: platform_mr.target_branch,
        ci_status: platform_mr.ci_status,
        ci_web_url: platform_mr.ci_web_url,
        review_status: platform_mr.review_status,
        reviewers: platform_mr
            .reviewers
            .into_iter()
            .map(|r| Reviewer {
                user: r.user,
                state: r.state,
            })
            .collect(),
        merge_status: platform_mr.merge_status,
        related_issues,
        additions: platform_mr.additions,
        deletions: platform_mr.deletions,
        changed_files: platform_mr.changed_files,
        created_at: platform_mr.created_at,
        updated_at: platform_mr.updated_at,
        merged_at: platform_mr.merged_at,
        web_url: platform_mr.web_url,
    };

    // Cache the result (5s TTL)
    if let Ok(json) = serde_json::to_string(&mr_detail) {
        state
            .api_cache
            .set_with_ttl(cache_key, json, std::time::Duration::from_secs(5));
    }

    Ok(Json(ResponseData::success(mr_detail)))
}
