use axum::{
    extract::{Path, Query, State},
    Json,
};

use crate::auth::jwt::Claims;
use crate::crypto;
use crate::error::WebPlatformError;
use crate::handlers::issues::map_platform_error;
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::middleware::project_access::require_project_member;
use crate::models::issue::KanbanIssue;
use crate::models::merge_request::KanbanMergeRequest;
use crate::models::{
    InProgressColumn, KanbanData, KanbanIssuesData, KanbanPrsData, KanbanPrsQuery, KanbanQuery,
    PlatformIssue, PrColumn, ResponseData, TodoColumn,
};
use crate::repository::{ProjectRepository, UserConfigRepository};
use crate::services::git_platform::{
    create_platform_client_with_proxy, GitPlatformClient, ListIssuesOptions,
    ListMergeRequestsOptions,
};
use crate::AppState;

const IN_PROGRESS_ISSUE_LABELS: &[&str] =
    &["symphony-claimed", "In Progress", "Merging", "Rework"];

fn in_progress_issue_labels() -> &'static [&'static str] {
    IN_PROGRESS_ISSUE_LABELS
}

fn issue_has_in_progress_label(issue: &PlatformIssue) -> bool {
    issue
        .labels
        .iter()
        .any(|label| in_progress_issue_labels().contains(&label.as_str()))
}

fn in_progress_issue_label_strings() -> Vec<String> {
    in_progress_issue_labels()
        .iter()
        .map(|label| (*label).to_string())
        .collect()
}

fn add_required_label(labels: Option<&[String]>, required_label: &str) -> Vec<String> {
    let mut combined = labels.map_or_else(Vec::new, ToOwned::to_owned);
    if !combined.iter().any(|label| label == required_label) {
        combined.push(required_label.to_string());
    }
    combined
}

fn dedupe_platform_issues(issues: Vec<PlatformIssue>) -> Vec<PlatformIssue> {
    let mut seen = std::collections::HashSet::new();
    issues
        .into_iter()
        .filter(|issue| seen.insert(issue.iid))
        .collect()
}

// ==================== Shared Context ====================

struct ProjectContext {
    platform: String,
    project_path: String,
    platform_token: String,
    client: Box<dyn GitPlatformClient>,
}

async fn resolve_project_context(
    state: &AppState,
    claims: &Claims,
    user_id: i64,
    project_id: i64,
) -> Result<ProjectContext, WebPlatformError> {
    require_project_member(claims, project_id, &state.repo).await?;

    let project = state
        .repo
        .get_project(project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;

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
    let project_path = format!("{}/{}", project.namespace, project.repo_name);

    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let client = create_platform_client_with_proxy(
        &project.platform,
        project.platform_host.as_deref(),
        Some(&proxy_config),
    )
    .map_err(map_platform_error)?;

    Ok(ProjectContext {
        platform: project.platform,
        project_path,
        platform_token,
        client,
    })
}

// ==================== Internal Fetch Logic ====================

pub(crate) struct IssuesResult {
    pub todo: TodoColumn,
    pub in_progress: InProgressColumn,
}

pub(crate) async fn fetch_issues_internal(
    client: &dyn GitPlatformClient,
    token: &str,
    project_path: &str,
    query: &KanbanQuery,
    include_mr_count: bool,
) -> Result<IssuesResult, WebPlatformError> {
    let todo_limit = query.effective_todo_limit();
    let parsed_labels = query.parsed_labels();

    // Fetch todo issues (without in-progress workflow labels)
    let mut todo_options = ListIssuesOptions {
        exclude_labels: Some(in_progress_issue_label_strings()),
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

    let (todo_issues, todo_total) = client
        .list_issues(token, project_path, &todo_options)
        .await
        .map_err(map_platform_error)?;

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

    // Fetch in-progress issues once per workflow label
    let mut in_progress_issue_groups = Vec::new();
    for in_progress_label in in_progress_issue_labels() {
        let in_progress_options = ListIssuesOptions {
            labels: Some(add_required_label(
                parsed_labels.as_deref(),
                in_progress_label,
            )),
            assignee: query.assignee.clone(),
            author: query.author.clone(),
            search: query.search.clone(),
            limit: 100,
            state: Some("opened".to_string()),
            ..Default::default()
        };

        let (issues, _) = client
            .list_issues(token, project_path, &in_progress_options)
            .await
            .map_err(map_platform_error)?;
        in_progress_issue_groups.extend(issues);
    }

    let in_progress_issues = dedupe_platform_issues(
        in_progress_issue_groups
            .into_iter()
            .filter(issue_has_in_progress_label)
            .collect(),
    );
    let in_progress_total = in_progress_issues.len() as u64;

    // Build in-progress kanban issues, optionally with mr_count
    let in_progress_kanban_issues = if include_mr_count {
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
        let mut mr_futures = Vec::new();

        for issue in &in_progress_issues {
            let sem = semaphore.clone();
            let tok = token.to_string();
            let path = project_path.to_string();
            let iid = issue.iid;
            let client_ref = client;

            mr_futures.push(async move {
                let _permit = sem.acquire().await.unwrap();
                client_ref
                    .get_issue_merge_requests(&tok, &path, iid)
                    .await
                    .unwrap_or_default()
            });
        }

        let mr_results = futures::future::join_all(mr_futures).await;

        in_progress_issues
            .into_iter()
            .zip(mr_results)
            .map(|(issue, mrs)| KanbanIssue {
                iid: issue.iid,
                title: issue.title,
                state: issue.state,
                labels: issue.labels,
                author: issue.author,
                assignees: issue.assignees,
                created_at: issue.created_at,
                updated_at: issue.updated_at,
                web_url: issue.web_url,
                mr_count: Some(mrs.len() as u64),
            })
            .collect()
    } else {
        in_progress_issues
            .into_iter()
            .map(|issue| KanbanIssue {
                iid: issue.iid,
                title: issue.title,
                state: issue.state,
                labels: issue.labels,
                author: issue.author,
                assignees: issue.assignees,
                created_at: issue.created_at,
                updated_at: issue.updated_at,
                web_url: issue.web_url,
                mr_count: None,
            })
            .collect()
    };

    Ok(IssuesResult {
        todo: TodoColumn {
            has_more: todo_total > todo_limit as u64,
            issues: todo_kanban_issues,
            total_count: todo_total,
        },
        in_progress: InProgressColumn {
            issues: in_progress_kanban_issues,
            total_count: in_progress_total,
        },
    })
}

pub(crate) fn build_pr_column(
    merge_requests: Vec<crate::models::PlatformMergeRequest>,
    project_path: &str,
) -> PrColumn {
    let mrs: Vec<KanbanMergeRequest> = sort_pending_merge_requests(merge_requests)
        .into_iter()
        .map(|mr| KanbanMergeRequest {
            iid: mr.iid,
            title: mr.title,
            state: normalize_merge_request_state(&mr.state),
            repository: project_path.to_string(),
            author: mr.author,
            source_branch: mr.source_branch,
            target_branch: mr.target_branch,
            ci_status: mr.ci_status,
            review_status: mr.review_status,
            related_issue_iids: mr.related_issue_iids,
            created_at: mr.created_at,
            updated_at: mr.updated_at,
            web_url: mr.web_url,
        })
        .collect();
    let total_count = mrs.len() as u64;
    PrColumn {
        merge_requests: mrs,
        total_count,
        error: None,
    }
}

pub(crate) async fn fetch_prs_internal(
    client: &dyn GitPlatformClient,
    token: &str,
    project_path: &str,
) -> Result<PrColumn, WebPlatformError> {
    let pr_options = ListMergeRequestsOptions {
        limit: 100,
        state: Some("opened".to_string()),
    };
    match client
        .list_merge_requests(token, project_path, &pr_options)
        .await
    {
        Ok(mrs) => Ok(build_pr_column(mrs, project_path)),
        Err(err) => Ok(PrColumn {
            merge_requests: Vec::new(),
            total_count: 0,
            error: Some(format!("PR/MR 数据加载失败：{}", err)),
        }),
    }
}

// ==================== Cache Helpers ====================

fn set_cached_at(state: &AppState, cache_key: &str, cached_at_field: &mut Option<String>) {
    if let Some(cached_at) = state.api_cache.get_cached_at(cache_key) {
        let elapsed = cached_at.elapsed();
        let cached_time =
            chrono::Utc::now() - chrono::Duration::from_std(elapsed).unwrap_or_default();
        *cached_at_field = Some(cached_time.format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }
}

// ==================== Public Handlers ====================

/// GET /api/projects/:id/kanban/issues
pub async fn get_kanban_issues(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<KanbanQuery>,
) -> Result<Json<ResponseData<KanbanIssuesData>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    if let Err(retry_after) = state.phase3_rate_limiter.check("kanban", user_id, 30) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    let ctx = resolve_project_context(&state, &claims, user_id, project_id).await?;

    let cache_key = format!(
        "{}:{}:kanban:issues:{}",
        user_id,
        project_id,
        query_hash(&query)
    );

    if query.no_cache != Some(true) {
        if let Some(cached_json) = state.api_cache.get(&cache_key) {
            if let Ok(mut data) = serde_json::from_str::<KanbanIssuesData>(&cached_json) {
                data.cached = true;
                set_cached_at(&state, &cache_key, &mut data.cached_at);
                return Ok(Json(ResponseData::success(data)));
            }
        }
    }

    let result = fetch_issues_internal(
        ctx.client.as_ref(),
        &ctx.platform_token,
        &ctx.project_path,
        &query,
        true,
    )
    .await?;

    let data = KanbanIssuesData {
        todo: result.todo,
        in_progress: result.in_progress,
        platform: ctx.platform,
        cached: false,
        cached_at: None,
    };

    if let Ok(json) = serde_json::to_string(&data) {
        state.api_cache.set(cache_key, json, false);
    }

    Ok(Json(ResponseData::success(data)))
}

/// GET /api/projects/:id/kanban/prs
pub async fn get_kanban_prs(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Path(project_id): Path<i64>,
    Query(query): Query<KanbanPrsQuery>,
) -> Result<Json<ResponseData<KanbanPrsData>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    if let Err(retry_after) = state.phase3_rate_limiter.check("kanban", user_id, 30) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    let ctx = resolve_project_context(&state, &claims, user_id, project_id).await?;

    let cache_key = format!("{}:{}:kanban:prs:0", user_id, project_id);

    if query.no_cache != Some(true) {
        if let Some(cached_json) = state.api_cache.get(&cache_key) {
            if let Ok(mut data) = serde_json::from_str::<KanbanPrsData>(&cached_json) {
                data.cached = true;
                set_cached_at(&state, &cache_key, &mut data.cached_at);
                return Ok(Json(ResponseData::success(data)));
            }
        }
    }

    let pr_column =
        fetch_prs_internal(ctx.client.as_ref(), &ctx.platform_token, &ctx.project_path).await?;

    let data = KanbanPrsData {
        pr: pr_column,
        platform: ctx.platform,
        cached: false,
        cached_at: None,
    };

    if data.pr.error.is_none() {
        if let Ok(json) = serde_json::to_string(&data) {
            state.api_cache.set(cache_key, json, false);
        }
    }

    Ok(Json(ResponseData::success(data)))
}

/// GET /api/projects/:id/kanban (compatibility shim)
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

    if let Err(retry_after) = state.phase3_rate_limiter.check("kanban", user_id, 30) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    let ctx = resolve_project_context(&state, &claims, user_id, project_id).await?;

    let cache_key = format!("{}:{}:kanban:{}", user_id, project_id, query_hash(&query));

    if query.no_cache != Some(true) {
        if let Some(cached_json) = state.api_cache.get(&cache_key) {
            if let Ok(mut kanban_data) = serde_json::from_str::<KanbanData>(&cached_json) {
                kanban_data.cached = true;
                set_cached_at(&state, &cache_key, &mut kanban_data.cached_at);
                return Ok(Json(ResponseData::success(kanban_data)));
            }
        }
    }

    let issues_result = fetch_issues_internal(
        ctx.client.as_ref(),
        &ctx.platform_token,
        &ctx.project_path,
        &query,
        true,
    )
    .await?;

    let pr_column =
        fetch_prs_internal(ctx.client.as_ref(), &ctx.platform_token, &ctx.project_path).await?;

    let kanban_data = KanbanData {
        todo: issues_result.todo,
        in_progress: issues_result.in_progress,
        pr: pr_column,
        platform: ctx.platform,
        cached: false,
        cached_at: None,
    };

    if kanban_data.pr.error.is_none() {
        if let Ok(json) = serde_json::to_string(&kanban_data) {
            state.api_cache.set(cache_key, json, false);
        }
    }

    Ok(Json(ResponseData::success(kanban_data)))
}

// ==================== Utility Functions ====================

fn is_pending_merge_request_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "opened" | "open"
    )
}

fn sort_pending_merge_requests(
    mut merge_requests: Vec<crate::models::PlatformMergeRequest>,
) -> Vec<crate::models::PlatformMergeRequest> {
    merge_requests.retain(|mr| is_pending_merge_request_state(&mr.state));
    merge_requests.sort_by(|a, b| {
        merge_request_timestamp(&b.updated_at)
            .cmp(&merge_request_timestamp(&a.updated_at))
            .then_with(|| {
                merge_request_timestamp(&b.created_at).cmp(&merge_request_timestamp(&a.created_at))
            })
            .then_with(|| b.iid.cmp(&a.iid))
    });
    merge_requests
}

fn merge_request_timestamp(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .unwrap_or_default()
}

fn normalize_merge_request_state(state: &str) -> String {
    if state.trim().eq_ignore_ascii_case("open") {
        "opened".to_string()
    } else {
        state.to_string()
    }
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
    query.author.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::kanban::{PlatformIssue, PlatformMergeRequest};
    use crate::models::PlatformUser;

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

    #[test]
    fn pending_merge_request_state_excludes_terminal_states() {
        assert!(is_pending_merge_request_state("opened"));
        assert!(is_pending_merge_request_state("open"));

        for state in ["merged", "closed", "rejected", "declined"] {
            assert!(
                !is_pending_merge_request_state(state),
                "{state} should not be treated as pending"
            );
        }
    }

    fn platform_merge_request(
        iid: u64,
        title: &str,
        state: &str,
        updated_at: &str,
        created_at: &str,
    ) -> PlatformMergeRequest {
        PlatformMergeRequest {
            iid,
            platform_node_id: None,
            title: title.to_string(),
            description: None,
            state: state.to_string(),
            author: PlatformUser {
                username: "alice".to_string(),
                display_name: Some("Alice".to_string()),
                avatar_url: None,
            },
            source_project_path: None,
            target_project_path: None,
            source_branch: format!("feature/{iid}"),
            target_branch: "main".to_string(),
            ci_status: None,
            ci_web_url: None,
            review_status: None,
            reviewers: Vec::new(),
            merge_status: None,
            related_issue_iids: Vec::new(),
            additions: None,
            deletions: None,
            changed_files: None,
            created_at: created_at.to_string(),
            updated_at: updated_at.to_string(),
            merged_at: None,
            web_url: format!("https://example.test/pulls/{iid}"),
        }
    }

    #[test]
    fn pending_merge_requests_are_filtered_and_stably_sorted() {
        let sorted = sort_pending_merge_requests(vec![
            platform_merge_request(
                3,
                "closed terminal",
                "closed",
                "2026-05-24T11:00:00Z",
                "2026-05-24T08:00:00Z",
            ),
            platform_merge_request(
                4,
                "older pending",
                "opened",
                "2026-05-24T10:00:00Z",
                "2026-05-24T09:00:00Z",
            ),
            platform_merge_request(
                2,
                "merged terminal",
                "merged",
                "2026-05-24T12:00:00Z",
                "2026-05-24T07:00:00Z",
            ),
            platform_merge_request(
                1,
                "newer pending",
                "open",
                "2026-05-24T10:00:00Z",
                "2026-05-24T09:30:00Z",
            ),
        ]);

        assert_eq!(
            sorted.into_iter().map(|mr| mr.iid).collect::<Vec<_>>(),
            vec![1, 4]
        );
    }

    fn platform_issue_with_labels(labels: Vec<&str>) -> PlatformIssue {
        PlatformIssue {
            iid: 1,
            title: "Issue".to_string(),
            description: None,
            state: "opened".to_string(),
            labels: labels.into_iter().map(str::to_string).collect(),
            author: PlatformUser {
                username: "alice".to_string(),
                display_name: Some("Alice".to_string()),
                avatar_url: None,
            },
            assignees: Vec::new(),
            milestone: None,
            created_at: "2026-05-24T00:00:00Z".to_string(),
            updated_at: "2026-05-24T00:00:00Z".to_string(),
            closed_at: None,
            web_url: "https://example.com/issues/1".to_string(),
            comment_count: None,
        }
    }

    #[test]
    fn in_progress_label_set_includes_issue_workflow_labels() {
        let labels = in_progress_issue_labels();

        assert!(labels.contains(&"In Progress"));
        assert!(labels.contains(&"Merging"));
        assert!(labels.contains(&"Rework"));
    }

    #[test]
    fn issue_with_any_workflow_label_is_in_progress() {
        for label in ["In Progress", "Merging", "Rework"] {
            let issue = platform_issue_with_labels(vec!["bug", label]);

            assert!(
                issue_has_in_progress_label(&issue),
                "expected {label} to classify issue as in_progress"
            );
        }
    }

    #[test]
    fn issue_with_todo_and_processing_label_is_in_progress() {
        for label in ["In Progress", "Merging", "Rework"] {
            let issue = platform_issue_with_labels(vec!["Todo", "bug", label]);

            assert!(
                issue_has_in_progress_label(&issue),
                "expected {label} to take priority over Todo when classifying issue"
            );
        }
    }

    #[test]
    fn issue_without_processing_label_is_not_in_progress() {
        let issue = platform_issue_with_labels(vec!["Todo", "bug"]);

        assert!(!issue_has_in_progress_label(&issue));
    }
}
