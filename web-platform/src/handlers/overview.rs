use axum::{
    extract::{Query, State},
    Json,
};
use tokio::time::{timeout, Duration};

use crate::auth::jwt::Claims;
use crate::crypto;
use crate::error::WebPlatformError;
use crate::handlers::kanban::{fetch_issues_internal, fetch_prs_internal};
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::models::{
    InProgressColumn, KanbanQuery, OverviewIssuesResponse, OverviewPrsResponse, OverviewQuery,
    PrColumn, ProjectIssuesEntry, ProjectMeta, ProjectPrsEntry, ResponseData, TodoColumn,
};
use crate::repository::{ProjectRepository, UserConfigRepository};
use crate::services::git_platform::create_platform_client_with_proxy;
use crate::AppState;

const PER_PROJECT_TIMEOUT: Duration = Duration::from_secs(10);
const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(15);

fn normalize_platform_host(platform: &str, platform_host: Option<&str>) -> String {
    match platform {
        "github" => "github.com".to_string(),
        _ => platform_host
            .unwrap_or("gitlab.com")
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_lowercase(),
    }
}

fn project_meta(project: &crate::models::Project) -> ProjectMeta {
    ProjectMeta {
        project_id: project.id,
        project_name: project.name.clone(),
        platform: project.platform.clone(),
        namespace: project.namespace.clone(),
        repo_name: project.repo_name.clone(),
    }
}

/// GET /api/overview/kanban/issues
pub async fn get_overview_issues(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<ResponseData<OverviewIssuesResponse>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    if let Err(retry_after) = state.phase3_rate_limiter.check("overview", user_id, 15) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    let is_admin = claims.role == "admin";
    let max_projects = query.effective_max_projects();
    let todo_limit = query.effective_todo_limit();

    let (projects, total) = state
        .repo
        .list_running_projects_for_member(user_id, is_admin, max_projects)
        .await?;

    if projects.is_empty() {
        return Ok(Json(ResponseData::success(OverviewIssuesResponse {
            projects: Vec::new(),
            total_running_projects: 0,
            has_more: false,
        })));
    }

    let user_config = state.repo.get_config(user_id).await?;
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;

    // Decrypt tokens lazily per platform
    let github_token = user_config
        .as_ref()
        .and_then(|c| c.github_token.as_ref())
        .and_then(|t| crypto::decrypt(t, &state.encryption_key).ok());
    let gitlab_token = user_config
        .as_ref()
        .and_then(|c| c.gitlab_token.as_ref())
        .and_then(|t| crypto::decrypt(t, &state.encryption_key).ok());

    let project_futures: Vec<_> = projects
        .iter()
        .map(|project| {
            let state = &state;
            let proxy_config = &proxy_config;
            let github_token = &github_token;
            let gitlab_token = &gitlab_token;
            let meta = project_meta(project);

            async move {
                let token = match project.platform.as_str() {
                    "github" => github_token.as_deref(),
                    _ => gitlab_token.as_deref(),
                };

                let token = match token {
                    Some(t) => t,
                    None => {
                        return ProjectIssuesEntry {
                            meta,
                            todo: TodoColumn {
                                issues: Vec::new(),
                                total_count: 0,
                                has_more: false,
                            },
                            in_progress: InProgressColumn {
                                issues: Vec::new(),
                                total_count: 0,
                            },
                            testing: None,
                            error: Some("no_token".to_string()),
                        };
                    }
                };

                let host = normalize_platform_host(
                    &project.platform,
                    project.platform_host.as_deref(),
                );
                let sem = state.platform_host_semaphores.get(&host);
                let _permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        return ProjectIssuesEntry {
                            meta,
                            todo: TodoColumn {
                                issues: Vec::new(),
                                total_count: 0,
                                has_more: false,
                            },
                            in_progress: InProgressColumn {
                                issues: Vec::new(),
                                total_count: 0,
                            },
                            testing: None,
                            error: Some("semaphore_closed".to_string()),
                        };
                    }
                };

                // Check per-project cache
                let cache_key = format!("{}:{}:kanban:issues:overview", user_id, project.id);
                if let Some(cached_json) = state.api_cache.get(&cache_key) {
                    if let Ok(entry) = serde_json::from_str::<ProjectIssuesEntry>(&cached_json) {
                        return entry;
                    }
                }

                let client = match create_platform_client_with_proxy(
                    &project.platform,
                    project.platform_host.as_deref(),
                    Some(proxy_config),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        return ProjectIssuesEntry {
                            meta,
                            todo: TodoColumn {
                                issues: Vec::new(),
                                total_count: 0,
                                has_more: false,
                            },
                            in_progress: InProgressColumn {
                                issues: Vec::new(),
                                total_count: 0,
                            },
                            testing: None,
                            error: Some(format!("client_error: {}", e)),
                        };
                    }
                };

                let project_path = format!("{}/{}", project.namespace, project.repo_name);
                let kanban_query = KanbanQuery {
                    todo_limit: Some(todo_limit),
                    assignee: None,
                    labels: None,
                    search: None,
                    no_cache: None,
                    author: None,
                };

                let result = timeout(
                    PER_PROJECT_TIMEOUT,
                    fetch_issues_internal(
                        client.as_ref(),
                        token,
                        &project_path,
                        &kanban_query,
                        false, // skip mr_count in overview
                    ),
                )
                .await;

                let entry = match result {
                    Ok(Ok(issues_result)) => ProjectIssuesEntry {
                        meta,
                        todo: issues_result.todo,
                        in_progress: issues_result.in_progress,
                        testing: None,
                        error: None,
                    },
                    Ok(Err(e)) => ProjectIssuesEntry {
                        meta,
                        todo: TodoColumn {
                            issues: Vec::new(),
                            total_count: 0,
                            has_more: false,
                        },
                        in_progress: InProgressColumn {
                            issues: Vec::new(),
                            total_count: 0,
                        },
                        testing: None,
                        error: Some(e.to_string()),
                    },
                    Err(_) => ProjectIssuesEntry {
                        meta,
                        todo: TodoColumn {
                            issues: Vec::new(),
                            total_count: 0,
                            has_more: false,
                        },
                        in_progress: InProgressColumn {
                            issues: Vec::new(),
                            total_count: 0,
                        },
                        testing: None,
                        error: Some("timeout".to_string()),
                    },
                };

                // Cache successful results
                if entry.error.is_none() {
                    if let Ok(json) = serde_json::to_string(&entry) {
                        state.api_cache.set(cache_key, json, false);
                    }
                }

                entry
            }
        })
        .collect();

    let entries = match timeout(ENDPOINT_TIMEOUT, futures::future::join_all(project_futures)).await
    {
        Ok(results) => results,
        Err(_) => {
            return Ok(Json(ResponseData::success(OverviewIssuesResponse {
                projects: Vec::new(),
                total_running_projects: total,
                has_more: total > max_projects as u64,
            })));
        }
    };

    Ok(Json(ResponseData::success(OverviewIssuesResponse {
        projects: entries,
        total_running_projects: total,
        has_more: total > max_projects as u64,
    })))
}

/// GET /api/overview/kanban/prs
pub async fn get_overview_prs(
    State(state): State<AppState>,
    claims: axum::Extension<Claims>,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<ResponseData<OverviewPrsResponse>>, WebPlatformError> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| WebPlatformError::Internal("invalid user id in token".to_string()))?;

    if let Err(retry_after) = state.phase3_rate_limiter.check("overview", user_id, 15) {
        return Err(WebPlatformError::RateLimited(retry_after));
    }

    let is_admin = claims.role == "admin";
    let max_projects = query.effective_max_projects();

    let (projects, total) = state
        .repo
        .list_running_projects_for_member(user_id, is_admin, max_projects)
        .await?;

    if projects.is_empty() {
        return Ok(Json(ResponseData::success(OverviewPrsResponse {
            projects: Vec::new(),
            total_running_projects: 0,
            has_more: false,
        })));
    }

    let user_config = state.repo.get_config(user_id).await?;
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;

    let github_token = user_config
        .as_ref()
        .and_then(|c| c.github_token.as_ref())
        .and_then(|t| crypto::decrypt(t, &state.encryption_key).ok());
    let gitlab_token = user_config
        .as_ref()
        .and_then(|c| c.gitlab_token.as_ref())
        .and_then(|t| crypto::decrypt(t, &state.encryption_key).ok());

    let project_futures: Vec<_> = projects
        .iter()
        .map(|project| {
            let state = &state;
            let proxy_config = &proxy_config;
            let github_token = &github_token;
            let gitlab_token = &gitlab_token;
            let meta = project_meta(project);

            async move {
                let token = match project.platform.as_str() {
                    "github" => github_token.as_deref(),
                    _ => gitlab_token.as_deref(),
                };

                let token = match token {
                    Some(t) => t,
                    None => {
                        return ProjectPrsEntry {
                            meta,
                            pr: PrColumn {
                                merge_requests: Vec::new(),
                                total_count: 0,
                                error: None,
                            },
                            error: Some("no_token".to_string()),
                        };
                    }
                };

                let host = normalize_platform_host(
                    &project.platform,
                    project.platform_host.as_deref(),
                );
                let sem = state.platform_host_semaphores.get(&host);
                let _permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => {
                        return ProjectPrsEntry {
                            meta,
                            pr: PrColumn {
                                merge_requests: Vec::new(),
                                total_count: 0,
                                error: None,
                            },
                            error: Some("semaphore_closed".to_string()),
                        };
                    }
                };

                // Check per-project cache
                let cache_key = format!("{}:{}:kanban:prs:0", user_id, project.id);
                if let Some(cached_json) = state.api_cache.get(&cache_key) {
                    if let Ok(prs_data) =
                        serde_json::from_str::<crate::models::KanbanPrsData>(&cached_json)
                    {
                        return ProjectPrsEntry {
                            meta,
                            pr: prs_data.pr,
                            error: None,
                        };
                    }
                }

                let client = match create_platform_client_with_proxy(
                    &project.platform,
                    project.platform_host.as_deref(),
                    Some(proxy_config),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        return ProjectPrsEntry {
                            meta,
                            pr: PrColumn {
                                merge_requests: Vec::new(),
                                total_count: 0,
                                error: None,
                            },
                            error: Some(format!("client_error: {}", e)),
                        };
                    }
                };

                let project_path = format!("{}/{}", project.namespace, project.repo_name);

                let result = timeout(
                    PER_PROJECT_TIMEOUT,
                    fetch_prs_internal(client.as_ref(), token, &project_path),
                )
                .await;

                let entry = match result {
                    Ok(Ok(pr_column)) => ProjectPrsEntry {
                        meta,
                        pr: pr_column,
                        error: None,
                    },
                    Ok(Err(e)) => ProjectPrsEntry {
                        meta,
                        pr: PrColumn {
                            merge_requests: Vec::new(),
                            total_count: 0,
                            error: None,
                        },
                        error: Some(e.to_string()),
                    },
                    Err(_) => ProjectPrsEntry {
                        meta,
                        pr: PrColumn {
                            merge_requests: Vec::new(),
                            total_count: 0,
                            error: None,
                        },
                        error: Some("timeout".to_string()),
                    },
                };

                // Cache successful results (reuse same key as single-project prs)
                if entry.error.is_none() && entry.pr.error.is_none() {
                    let prs_data = crate::models::KanbanPrsData {
                        pr: entry.pr.clone(),
                        platform: project.platform.clone(),
                        cached: false,
                        cached_at: None,
                    };
                    if let Ok(json) = serde_json::to_string(&prs_data) {
                        state.api_cache.set(cache_key, json, false);
                    }
                }

                entry
            }
        })
        .collect();

    let entries = match timeout(ENDPOINT_TIMEOUT, futures::future::join_all(project_futures)).await
    {
        Ok(results) => results,
        Err(_) => {
            return Ok(Json(ResponseData::success(OverviewPrsResponse {
                projects: Vec::new(),
                total_running_projects: total,
                has_more: total > max_projects as u64,
            })));
        }
    };

    Ok(Json(ResponseData::success(OverviewPrsResponse {
        projects: entries,
        total_running_projects: total,
        has_more: total > max_projects as u64,
    })))
}
