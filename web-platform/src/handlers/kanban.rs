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
    InProgressColumn, KanbanData, KanbanQuery, PlatformIssue, PrColumn, ResponseData, TodoColumn,
};
use crate::repository::{ProjectRepository, UserConfigRepository};
use crate::services::git_platform::{
    create_platform_client_with_proxy, ListIssuesOptions, ListMergeRequestsOptions,
};
use crate::AppState;

const IN_PROGRESS_ISSUE_LABELS: &[&str] = &["symphony-claimed", "In Progree", "Merging", "Rework"];

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

/// GET /api/projects/:id/kanban
///
/// Fetches the three-column kanban board data:
/// - todo: open issues without an in-progress workflow label
/// - in_progress: open issues with any in-progress workflow label
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
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let client = create_platform_client_with_proxy(
        &project.platform,
        project.platform_host.as_deref(),
        Some(&proxy_config),
    )
    .map_err(map_platform_error)?;

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

    // Fetch in-progress issues once per workflow label because platform label filters require
    // all requested labels rather than any one of them.
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
            limit: 100, // Fetch all in-progress (usually limited)
            state: Some("opened".to_string()),
            ..Default::default()
        };

        let (issues, _) = client
            .list_issues(&platform_token, &project_path, &in_progress_options)
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
    }

    let pr_options = ListMergeRequestsOptions {
        limit: 100,
        state: Some("opened".to_string()),
    };
    let (all_mrs, pr_error) = match client
        .list_merge_requests(&platform_token, &project_path, &pr_options)
        .await
    {
        Ok(mrs) => (
            mrs.into_iter()
                .filter(|mr| is_pending_merge_request_state(&mr.state))
                .map(|mr| KanbanMergeRequest {
                    iid: mr.iid,
                    title: mr.title,
                    state: normalize_merge_request_state(&mr.state),
                    repository: project_path.clone(),
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
                .collect(),
            None,
        ),
        Err(err) => (Vec::new(), Some(format!("PR/MR 数据加载失败：{}", err))),
    };
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
            error: pr_error,
        },
        platform: project.platform.clone(),
        cached: false,
        cached_at: None,
    };

    // Store in cache
    if kanban_data.pr.error.is_none() {
        if let Ok(json) = serde_json::to_string(&kanban_data) {
            state.api_cache.set(cache_key, json, false);
        }
    }

    Ok(Json(ResponseData::success(kanban_data)))
}

fn is_pending_merge_request_state(state: &str) -> bool {
    matches!(
        state.trim().to_ascii_lowercase().as_str(),
        "opened" | "open"
    )
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
    use crate::models::kanban::PlatformIssue;
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

        assert!(labels.contains(&"In Progree"));
        assert!(labels.contains(&"Merging"));
        assert!(labels.contains(&"Rework"));
    }

    #[test]
    fn issue_with_any_workflow_label_is_in_progress() {
        for label in ["In Progree", "Merging", "Rework"] {
            let issue = platform_issue_with_labels(vec!["bug", label]);

            assert!(
                issue_has_in_progress_label(&issue),
                "expected {label} to classify issue as in_progress"
            );
        }
    }

    #[test]
    fn issue_without_processing_label_is_not_in_progress() {
        let issue = platform_issue_with_labels(vec!["Todo", "bug"]);

        assert!(!issue_has_in_progress_label(&issue));
    }
}
