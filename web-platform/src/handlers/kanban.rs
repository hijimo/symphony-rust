use axum::{
    extract::{Path, Query, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::crypto;
use crate::error::WebPlatformError;
use crate::handlers::issues::map_platform_error;
use crate::middleware::project_access::require_project_member;
use crate::models::issue::KanbanIssue;
use crate::models::merge_request::KanbanMergeRequest;
use crate::models::{
    InProgressColumn, KanbanData, KanbanQuery, PrColumn, ResponseData, TodoColumn,
};
use crate::repository::{ProjectRepository, UserConfigRepository};
use crate::services::git_platform::{create_platform_client, ListIssuesOptions};
use crate::AppState;

/// GET /api/projects/:id/kanban
///
/// Fetches the three-column kanban board data:
/// - todo: open issues without `symphony-claimed` label
/// - in_progress: open issues with `symphony-claimed` label
/// - pr: MRs associated with in-progress issues
pub async fn get_kanban(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<KanbanQuery>,
) -> Result<Json<ResponseData<KanbanData>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    // Check project membership
    require_project_member(&claims, project_id, &state.repo).await?;

    // Rate limit check: 30/min/user for kanban
    if let Err(retry_after) = state.phase3_rate_limiter.check("kanban", user_id, 30) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    // Get project info to determine platform
    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

    // Get user's platform token
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

    // Decrypt the token
    let platform_token = crypto::decrypt(&encrypted_token, &state.encryption_key)?;

    // Build the project path (namespace/repo_name)
    let project_path = format!("{}/{}", project.namespace, project.repo_name);

    // Check cache first
    let cache_key = format!("{}:{}:kanban:{}", user_id, project_id, query_hash(&query));

    if query.no_cache != Some(true) {
        if let Some(cached_json) = state.api_cache.get(&cache_key) {
            if let Ok(mut kanban_data) = serde_json::from_str::<KanbanData>(&cached_json) {
                kanban_data.cached = true;
                // Set cached_at from the cache entry creation time
                if let Some(cached_at) = state.api_cache.get_cached_at(&cache_key) {
                    let elapsed = cached_at.elapsed();
                    let cached_time = chrono::Utc::now()
                        - chrono::Duration::from_std(elapsed).unwrap_or_default();
                    kanban_data.cached_at =
                        Some(cached_time.format("%Y-%m-%dT%H:%M:%SZ").to_string());
                }
                return Ok(Json(ResponseData::success(kanban_data)));
            }
        }
    }

    // Create platform client
    let client = create_platform_client(&project.platform, project.platform_host.as_deref());

    let todo_limit = query.effective_todo_limit();
    let parsed_labels = query.parsed_labels();

    // Fetch todo issues (without symphony-claimed label)
    let mut todo_options = ListIssuesOptions {
        exclude_labels: Some(vec!["symphony-claimed".to_string()]),
        assignee: query.assignee.clone(),
        author: query.author.clone(),
        search: query.search.clone(),
        limit: todo_limit,
        state: Some("opened".to_string()),
        ..Default::default()
    };
    if let Some(ref labels) = parsed_labels {
        todo_options.labels = Some(labels.clone());
    }

    let todo_result = client
        .list_issues(&platform_token, &project_path, &todo_options)
        .await
        .map_err(map_platform_error)?;

    let (todo_issues, todo_total) = todo_result;
    let todo_kanban_issues: Vec<KanbanIssue> = todo_issues
        .into_iter()
        .map(|i| KanbanIssue {
            iid: i.iid,
            title: i.title,
            state: i.state,
            labels: i.labels,
            author: i.author,
            assignees: i.assignees,
            created_at: i.created_at,
            updated_at: i.updated_at,
            web_url: i.web_url,
            mr_count: None,
        })
        .collect();

    // Fetch in-progress issues (with symphony-claimed label)
    let mut in_progress_options = ListIssuesOptions {
        labels: Some(vec!["symphony-claimed".to_string()]),
        assignee: query.assignee.clone(),
        author: query.author.clone(),
        search: query.search.clone(),
        limit: 100, // Fetch all in-progress (usually limited)
        state: Some("opened".to_string()),
        ..Default::default()
    };
    if let Some(ref labels) = parsed_labels {
        let mut combined = labels.clone();
        combined.push("symphony-claimed".to_string());
        in_progress_options.labels = Some(combined);
    }

    let in_progress_result = client
        .list_issues(&platform_token, &project_path, &in_progress_options)
        .await
        .map_err(map_platform_error)?;

    let (in_progress_issues, in_progress_total) = in_progress_result;

    // Fetch related MRs for in-progress issues (parallel, max 10 concurrent)
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
    let mut mr_futures = Vec::new();

    for issue in &in_progress_issues {
        let sem = semaphore.clone();
        let token = platform_token.clone();
        let path = project_path.clone();
        let iid = issue.iid;
        let client_ref = &client;

        mr_futures.push(async move {
            let _permit = sem.acquire().await.unwrap();
            client_ref
                .get_issue_merge_requests(&token, &path, iid)
                .await
                .unwrap_or_default()
        });
    }

    let mr_results = futures::future::join_all(mr_futures).await;

    // Build in-progress kanban issues with mr_count
    let mut in_progress_kanban_issues: Vec<KanbanIssue> = Vec::new();
    let mut all_mrs: Vec<KanbanMergeRequest> = Vec::new();

    for (issue, mrs) in in_progress_issues.into_iter().zip(mr_results) {
        let mr_count = mrs.len() as u64;
        in_progress_kanban_issues.push(KanbanIssue {
            iid: issue.iid,
            title: issue.title,
            state: issue.state,
            labels: issue.labels,
            author: issue.author,
            assignees: issue.assignees,
            created_at: issue.created_at,
            updated_at: issue.updated_at,
            web_url: issue.web_url,
            mr_count: Some(mr_count),
        });

        for mr in mrs {
            // Avoid duplicates (same MR might be linked to multiple issues)
            if !all_mrs.iter().any(|existing| existing.iid == mr.iid) {
                all_mrs.push(KanbanMergeRequest {
                    iid: mr.iid,
                    title: mr.title,
                    state: mr.state,
                    author: mr.author,
                    source_branch: mr.source_branch,
                    target_branch: mr.target_branch,
                    ci_status: mr.ci_status,
                    review_status: mr.review_status,
                    related_issue_iids: mr.related_issue_iids,
                    created_at: mr.created_at,
                    updated_at: mr.updated_at,
                    web_url: mr.web_url,
                });
            }
        }
    }

    let mr_total = all_mrs.len() as u64;

    let kanban_data = KanbanData {
        todo: TodoColumn {
            has_more: todo_total > todo_limit as u64,
            issues: todo_kanban_issues,
            total_count: todo_total,
        },
        in_progress: InProgressColumn {
            issues: in_progress_kanban_issues,
            total_count: in_progress_total,
        },
        pr: PrColumn {
            merge_requests: all_mrs,
            total_count: mr_total,
        },
        platform: project.platform.clone(),
        cached: false,
        cached_at: None,
    };

    // Store in cache
    if let Ok(json) = serde_json::to_string(&kanban_data) {
        state.api_cache.set(cache_key, json, false);
    }

    Ok(Json(ResponseData::success(kanban_data)))
}

/// Generate a simple hash of the query parameters for cache key differentiation.
fn query_hash(query: &KanbanQuery) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    query.todo_limit.hash(&mut hasher);
    query.assignee.hash(&mut hasher);
    query.labels.hash(&mut hasher);
    query.search.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_hash_ignores_no_cache_flag() {
        let base = KanbanQuery {
            todo_limit: Some(20),
            assignee: Some("alice".to_string()),
            labels: Some("bug,needs review".to_string()),
            search: Some("中文 query".to_string()),
            no_cache: None,
            author: None,
        };
        let no_cache = KanbanQuery {
            no_cache: Some(true),
            ..base.clone()
        };

        assert_eq!(query_hash(&base), query_hash(&no_cache));
    }
}
