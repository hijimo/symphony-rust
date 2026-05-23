use axum::{
    extract::{Path, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::crypto;
use crate::error::WebPlatformError;
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::middleware::project_access::require_project_member;
use crate::models::concurrency::{Contributor, ContributorsResponse};
use crate::models::ResponseData;
use crate::repository::{ProjectRepository, UserConfigRepository};
use crate::services::git_platform::{create_platform_client_with_proxy, ListIssuesOptions};
use crate::AppState;

/// GET /api/projects/:id/contributors
pub async fn get_contributors(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
) -> Result<Json<ResponseData<ContributorsResponse>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id".to_string()))?;

    require_project_member(&claims, project_id, &state.repo).await?;

    // Rate limit
    if let Err(retry_after) = state.phase3_rate_limiter.check("contributors", user_id, 30) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Get user's token
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
    }
    .ok_or_else(|| {
        WebPlatformError::BadRequest(format!(
            "请先在个人设置中配置 {} Token",
            if project.platform == "github" {
                "GitHub"
            } else {
                "GitLab"
            }
        ))
    })?;

    let token = crypto::decrypt(&encrypted_token, &state.encryption_key)
        .map_err(|_| WebPlatformError::Internal("Failed to decrypt token".to_string()))?;

    let host = match project.platform.as_str() {
        "github" => None,
        _ => user_config
            .gitlab_host
            .as_deref()
            .or(project.platform_host.as_deref()),
    };

    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let client = create_platform_client_with_proxy(&project.platform, host, Some(&proxy_config))
        .map_err(crate::handlers::issues::map_platform_error)?;
    let project_path = format!("{}/{}", project.namespace, project.repo_name);

    // Fetch recent issues to extract contributors
    let (issues, _total) = client
        .list_issues(
            &token,
            &project_path,
            &ListIssuesOptions {
                state: Some("all".to_string()),
                limit: 100,
                ..Default::default()
            },
        )
        .await
        .unwrap_or_default();

    // Aggregate contributors from issues
    let mut contributor_map: std::collections::HashMap<String, Contributor> =
        std::collections::HashMap::new();

    for issue in &issues {
        let entry = contributor_map
            .entry(issue.author.username.clone())
            .or_insert_with(|| Contributor {
                username: issue.author.username.clone(),
                display_name: issue.author.display_name.clone().unwrap_or_default(),
                avatar_url: issue.author.avatar_url.clone().unwrap_or_default(),
                recent_issue_count: 0,
                recent_mr_count: 0,
                is_bot: is_bot_username(&issue.author.username),
                logical_author: true,
            });
        entry.recent_issue_count += 1;
    }

    let mut contributors: Vec<Contributor> = contributor_map.into_values().collect();
    contributors.sort_by(|a, b| {
        (b.recent_issue_count + b.recent_mr_count).cmp(&(a.recent_issue_count + a.recent_mr_count))
    });

    Ok(Json(ResponseData::success(ContributorsResponse {
        contributors,
        scope: "last_100_items".to_string(),
    })))
}

fn is_bot_username(username: &str) -> bool {
    let lower = username.to_lowercase();
    lower.contains("bot") || lower.contains("symphony") || lower.contains("codex")
}
