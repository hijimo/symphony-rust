use axum::http::StatusCode;
use chrono::{Duration, Utc};
use dashmap::DashMap;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use uuid::Uuid;

use crate::error::WebPlatformError;
use crate::handlers::issues::{get_user_platform_token, map_platform_error};
use crate::handlers::network_proxy::load_effective_proxy_config;
use crate::models::kanban::PlatformMergeRequest;
use crate::models::merge_request::CreateMergeRequestApiRequest;
use crate::models::Project;
use crate::repository::{ProjectRepository, SqliteRepository};
use crate::services::git_platform::{
    create_platform_client_with_proxy, CreateMergeRequest, GitPlatformClient, GitPlatformError,
    MergeRequestState, PlatformConflictCode, PlatformValidationCode,
};
use crate::AppState;

const LOCK_TTL_SECONDS: i64 = 120;
const CREATE_LEASE_SECONDS: i64 = 90;

static ACTIVE_CREATE_OPERATIONS: OnceLock<DashMap<String, ()>> = OnceLock::new();

#[derive(Debug)]
pub struct CreateMergeRequestServiceResponse {
    pub http_status: StatusCode,
    pub body: Value,
}

#[derive(Debug, Clone)]
struct NormalizedCreateMrRequest {
    source_branch: String,
    target_branch: String,
    title: String,
    description: Option<String>,
    purpose_type: String,
    purpose_id: String,
    draft: bool,
}

#[derive(Debug)]
struct Registration {
    request_id: i64,
    operation_id: i64,
    owns_operation_lock: bool,
    replay: Option<CreateMergeRequestServiceResponse>,
}

struct RegisterInput<'a> {
    repo: &'a SqliteRepository,
    project: &'a Project,
    user_id: i64,
    idempotency_key: &'a str,
    request_hash: &'a str,
    business_key: &'a str,
    business_key_json: &'a str,
    project_path: &'a str,
    req: &'a NormalizedCreateMrRequest,
}

#[derive(Clone)]
struct RegisterOwned {
    repo: SqliteRepository,
    project_id: i64,
    platform: String,
    user_id: i64,
    idempotency_key: String,
    request_hash: String,
    business_key: String,
    business_key_json: String,
    project_path: String,
    source_branch: String,
    target_branch: String,
    purpose_type: String,
    purpose_id: String,
}

struct CreateExecution<'a> {
    repo: &'a SqliteRepository,
    project: &'a Project,
    user_id: i64,
    platform_token: &'a str,
    client: &'a dyn GitPlatformClient,
    project_path: &'a str,
    req: &'a NormalizedCreateMrRequest,
}

struct CreateLease {
    active_key: String,
}

struct SuccessInput<'a> {
    repo: &'a SqliteRepository,
    project_id: i64,
    user_id: i64,
    request_id: i64,
    operation_id: i64,
    mr: &'a PlatformMergeRequest,
    req: &'a NormalizedCreateMrRequest,
    idempotency_status: &'a str,
}

struct ExistingRequestRow {
    id: i64,
    request_hash: String,
    operation_id: Option<i64>,
    response_status: String,
    http_status: i64,
    response_json: Option<String>,
}

struct ReconcileOperation {
    id: i64,
    project_id: i64,
    source_branch: String,
    target_branch: String,
}

#[derive(Debug, Serialize)]
struct RequestHashInput<'a> {
    version: u8,
    operation: &'a str,
    project_id: i64,
    platform: &'a str,
    project_path: &'a str,
    source_project_path: &'a str,
    source_branch: &'a str,
    target_branch: &'a str,
    title: &'a str,
    description: &'a Option<String>,
    purpose_type: &'a str,
    purpose_id: &'a str,
    draft: bool,
}

#[derive(Debug, Serialize)]
struct BusinessKeyInput<'a> {
    version: u8,
    operation: &'a str,
    project_id: i64,
    platform: &'a str,
    project_path: &'a str,
    source_project_path: &'a str,
    source_branch: &'a str,
    target_branch: &'a str,
    purpose_type: &'a str,
    purpose_id: &'a str,
}

pub async fn create_merge_request_idempotent(
    repo: &SqliteRepository,
    project: &Project,
    user_id: i64,
    platform_token: &str,
    idempotency_key: &str,
    req: CreateMergeRequestApiRequest,
    client: &dyn GitPlatformClient,
) -> Result<CreateMergeRequestServiceResponse, WebPlatformError> {
    for attempt in 0..8 {
        let result = create_merge_request_idempotent_once(
            repo,
            project,
            user_id,
            platform_token,
            idempotency_key,
            req.clone(),
            client,
        )
        .await;

        match result {
            Ok(response) => return Ok(response),
            Err(err) if is_retryable_sqlite_error(&err) && attempt < 7 => {
                tokio::time::sleep(std::time::Duration::from_millis(10 * (attempt + 1))).await;
            }
            Err(err) => return Err(err),
        }
    }

    unreachable!("bounded create retry loop always returns")
}

async fn create_merge_request_idempotent_once(
    repo: &SqliteRepository,
    project: &Project,
    user_id: i64,
    platform_token: &str,
    idempotency_key: &str,
    req: CreateMergeRequestApiRequest,
    client: &dyn GitPlatformClient,
) -> Result<CreateMergeRequestServiceResponse, WebPlatformError> {
    let key = idempotency_key.trim();
    if key.is_empty() {
        return Err(WebPlatformError::BadRequest(
            "Idempotency-Key is required".to_string(),
        ));
    }

    let project_path = format!("{}/{}", project.namespace, project.repo_name);
    let normalized = normalize_request(req, &project.default_branch)?;
    let business_json = business_key_json(project, &project_path, &normalized)?;
    let business_key = sha256_hex(&business_json);
    let request_hash = request_hash(project, &project_path, &normalized)?;

    let registration = register_request(RegisterInput {
        repo,
        project,
        user_id,
        idempotency_key: key,
        request_hash: &request_hash,
        business_key: &business_key,
        business_key_json: &business_json,
        project_path: &project_path,
        req: &normalized,
    })
    .await?;

    if let Some(replay) = registration.replay {
        return Ok(replay);
    }

    reconcile_or_create(
        CreateExecution {
            repo,
            project,
            user_id,
            platform_token,
            client,
            project_path: &project_path,
            req: &normalized,
        },
        registration,
    )
    .await
}

pub fn spawn_merge_request_reconciler(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Err(err) = reconcile_pending_operations(&state).await {
                tracing::warn!("MR create reconciliation failed: {}", err);
            }
        }
    });
}

pub async fn reconcile_pending_operations(state: &AppState) -> Result<(), WebPlatformError> {
    let operations = load_reconcile_operations(&state.repo).await?;
    for operation in operations {
        if let Err(err) = reconcile_operation(state, operation).await {
            tracing::warn!("MR create operation reconciliation skipped: {}", err);
        }
    }
    Ok(())
}

fn normalize_request(
    req: CreateMergeRequestApiRequest,
    default_branch: &str,
) -> Result<NormalizedCreateMrRequest, WebPlatformError> {
    let source_branch = req.source_branch.trim().to_string();
    if source_branch.is_empty() {
        return Err(WebPlatformError::BadRequest(
            "source_branch is required".to_string(),
        ));
    }

    let target_branch = req.target_branch.unwrap_or_default().trim().to_string();
    let target_branch = if target_branch.is_empty() {
        default_branch.trim().to_string()
    } else {
        target_branch
    };
    if target_branch.is_empty() {
        return Err(WebPlatformError::BadRequest(
            "target_branch is required".to_string(),
        ));
    }

    let title = req.title.trim().to_string();
    if title.is_empty() {
        return Err(WebPlatformError::BadRequest(
            "title is required".to_string(),
        ));
    }
    if title.chars().count() > 200 {
        return Err(WebPlatformError::BadRequest(
            "title must be at most 200 characters".to_string(),
        ));
    }

    let description = req.description.and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    });

    let purpose_type = req
        .purpose_type
        .unwrap_or_else(|| "manual".to_string())
        .trim()
        .to_string();
    if !matches!(
        purpose_type.as_str(),
        "manual" | "issue_delivery" | "agent_handoff"
    ) {
        return Err(WebPlatformError::BadRequest(
            "purpose_type must be manual, issue_delivery, or agent_handoff".to_string(),
        ));
    }

    Ok(NormalizedCreateMrRequest {
        source_branch,
        target_branch,
        title,
        description,
        purpose_type,
        purpose_id: req.purpose_id.unwrap_or_default().trim().to_string(),
        draft: req.draft.unwrap_or(false),
    })
}

fn request_hash(
    project: &Project,
    project_path: &str,
    req: &NormalizedCreateMrRequest,
) -> Result<String, WebPlatformError> {
    let source_project_path = project_path;
    let value = RequestHashInput {
        version: 1,
        operation: "create_merge_request",
        project_id: project.id,
        platform: &project.platform,
        project_path,
        source_project_path,
        source_branch: &req.source_branch,
        target_branch: &req.target_branch,
        title: &req.title,
        description: &req.description,
        purpose_type: &req.purpose_type,
        purpose_id: &req.purpose_id,
        draft: req.draft,
    };
    let json = serde_json::to_string(&value).map_err(|e| {
        WebPlatformError::Internal(format!("failed to serialize request hash: {e}"))
    })?;
    Ok(sha256_hex(&json))
}

fn business_key_json(
    project: &Project,
    project_path: &str,
    req: &NormalizedCreateMrRequest,
) -> Result<String, WebPlatformError> {
    let value = BusinessKeyInput {
        version: 1,
        operation: "create_merge_request",
        project_id: project.id,
        platform: &project.platform,
        project_path,
        source_project_path: project_path,
        source_branch: &req.source_branch,
        target_branch: &req.target_branch,
        purpose_type: &req.purpose_type,
        purpose_id: &req.purpose_id,
    };
    serde_json::to_string(&value)
        .map_err(|e| WebPlatformError::Internal(format!("failed to serialize business key: {e}")))
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

async fn load_reconcile_operations(
    repo: &SqliteRepository,
) -> Result<Vec<ReconcileOperation>, WebPlatformError> {
    let pool = repo.pool();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, project_id, source_branch, target_branch
             FROM merge_request_create_operations
             WHERE (status = 'active' AND locked_until < datetime('now'))
                OR (status = 'failed_retryable' AND updated_at < datetime('now', '-1 minute'))
             ORDER BY updated_at ASC
             LIMIT 50",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ReconcileOperation {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    source_branch: row.get(2)?,
                    target_branch: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok::<_, WebPlatformError>(rows)
    })
    .await
    .unwrap()
}

async fn reconcile_operation(
    state: &AppState,
    operation: ReconcileOperation,
) -> Result<(), WebPlatformError> {
    let project = state
        .repo
        .get_project(operation.project_id)
        .await?
        .ok_or_else(|| WebPlatformError::NotFound("Project not found".to_string()))?;
    let Some((request_id, user_id)) = select_reconcile_request(&state.repo, operation.id).await?
    else {
        return Ok(());
    };

    let (platform_token, _) = get_user_platform_token(state, user_id, &project).await?;
    let proxy_config = load_effective_proxy_config(&state.repo, &state.encryption_key).await?;
    let client = create_platform_client_with_proxy(
        &project.platform,
        project.platform_host.as_deref(),
        Some(&proxy_config),
    )
    .map_err(map_platform_error)?;
    let project_path = format!("{}/{}", project.namespace, project.repo_name);
    let req = NormalizedCreateMrRequest {
        source_branch: operation.source_branch,
        target_branch: operation.target_branch,
        title: String::new(),
        description: None,
        purpose_type: "manual".to_string(),
        purpose_id: String::new(),
        draft: false,
    };

    if let Some(open) = client
        .find_open_merge_request_by_branches(
            &platform_token,
            &project_path,
            &req.source_branch,
            &req.target_branch,
        )
        .await
        .map_err(map_precheck_error)?
    {
        save_success(SuccessInput {
            repo: &state.repo,
            project_id: project.id,
            user_id,
            request_id,
            operation_id: operation.id,
            mr: &open,
            req: &req,
            idempotency_status: "reconciled",
        })
        .await?;
        state.api_cache.invalidate_project(project.id);
        return Ok(());
    }

    let closed_or_merged = client
        .find_merge_requests_by_branches(
            &platform_token,
            &project_path,
            &req.source_branch,
            &req.target_branch,
            &[MergeRequestState::Closed, MergeRequestState::Merged],
        )
        .await
        .map_err(map_precheck_error)?;
    if !closed_or_merged.is_empty() {
        mark_failed_final(
            &state.repo,
            request_id,
            operation.id,
            "closed_or_merged",
            "Only closed or merged PR/MR exists for this branch pair; create a new source branch",
            StatusCode::CONFLICT,
            "BIZ_003",
        )
        .await?;
    }

    Ok(())
}

async fn select_reconcile_request(
    repo: &SqliteRepository,
    operation_id: i64,
) -> Result<Option<(i64, i64)>, WebPlatformError> {
    let pool = repo.pool();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        let result = conn
            .query_row(
                "SELECT r.id, r.user_id
                 FROM idempotency_requests r
                 LEFT JOIN merge_request_create_operations o ON o.id = r.operation_id
                 WHERE r.operation_id = ?1
                 ORDER BY CASE WHEN r.id = o.lock_owner_request_id THEN 0 ELSE 1 END,
                          r.updated_at DESC
                 LIMIT 1",
                [operation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok::<_, WebPlatformError>(result)
    })
    .await
    .unwrap()
}

async fn register_request(input: RegisterInput<'_>) -> Result<Registration, WebPlatformError> {
    let owned = RegisterOwned {
        repo: input.repo.clone(),
        project_id: input.project.id,
        platform: input.project.platform.clone(),
        user_id: input.user_id,
        idempotency_key: input.idempotency_key.to_string(),
        request_hash: input.request_hash.to_string(),
        business_key: input.business_key.to_string(),
        business_key_json: input.business_key_json.to_string(),
        project_path: input.project_path.to_string(),
        source_branch: input.req.source_branch.clone(),
        target_branch: input.req.target_branch.clone(),
        purpose_type: input.req.purpose_type.clone(),
        purpose_id: input.req.purpose_id.clone(),
    };

    for attempt in 0..8 {
        let attempt_input = owned.clone();
        let result = tokio::task::spawn_blocking(move || register_request_once(attempt_input))
            .await
            .unwrap();
        match result {
            Ok(registration) => return Ok(registration),
            Err(err) if is_retryable_sqlite_error(&err) && attempt < 7 => {
                tokio::time::sleep(std::time::Duration::from_millis(10 * (attempt + 1))).await;
            }
            Err(err) => return Err(err),
        }
    }

    unreachable!("bounded registration retry loop always returns")
}

fn register_request_once(input: RegisterOwned) -> Result<Registration, WebPlatformError> {
    let pool = input.repo.pool();
    let mut conn = pool.get()?;
    let tx = conn.transaction()?;
    let now = utc_sql_now();
    let locked_until = utc_sql_after(LOCK_TTL_SECONDS);

    let existing_request: Option<ExistingRequestRow> = tx
        .query_row(
            "SELECT id, request_hash, operation_id, response_status, http_status, response_json
                 FROM idempotency_requests
                 WHERE project_id = ?1 AND user_id = ?2 AND idempotency_key = ?3",
            params![input.project_id, input.user_id, input.idempotency_key],
            |row| {
                Ok(ExistingRequestRow {
                    id: row.get(0)?,
                    request_hash: row.get(1)?,
                    operation_id: row.get(2)?,
                    response_status: row.get(3)?,
                    http_status: row.get(4)?,
                    response_json: row.get(5)?,
                })
            },
        )
        .optional()?;

    let (request_id, existing_operation_id, replay) = if let Some(existing) = existing_request {
        if existing.request_hash != input.request_hash {
            return Err(WebPlatformError::Conflict(
                "Idempotency-Key was reused with a different request".to_string(),
            ));
        }
        let replay = if matches!(
            existing.response_status.as_str(),
            "succeeded" | "failed_final"
        ) {
            existing.response_json.and_then(|raw| {
                let mut body = serde_json::from_str::<Value>(&raw).ok()?;
                if let Some(data) = body.get_mut("data").and_then(Value::as_object_mut) {
                    data.insert(
                        "idempotency_status".to_string(),
                        Value::String("replayed".to_string()),
                    );
                }
                Some(CreateMergeRequestServiceResponse {
                    http_status: StatusCode::from_u16(existing.http_status as u16).ok()?,
                    body,
                })
            })
        } else {
            None
        };
        (existing.id, existing.operation_id, replay)
    } else {
        tx.execute(
                "INSERT INTO idempotency_requests
                     (project_id, user_id, idempotency_key, request_hash, response_status, http_status)
                     VALUES (?1, ?2, ?3, ?4, 'in_progress', 200)",
                params![
                    input.project_id,
                    input.user_id,
                    input.idempotency_key,
                    input.request_hash
                ],
            )?;
        (tx.last_insert_rowid(), None, None)
    };

    if let Some(replay) = replay {
        tx.commit()?;
        return Ok(Registration {
            request_id,
            operation_id: existing_operation_id.unwrap_or_default(),
            owns_operation_lock: false,
            replay: Some(replay),
        });
    }

    let operation: Option<(i64, String, Option<i64>, String)> = tx
        .query_row(
            "SELECT id, status, lock_owner_request_id, locked_until
                 FROM merge_request_create_operations
                 WHERE project_id = ?1
                   AND business_key = ?2
                   AND status IN ('active', 'succeeded_open', 'failed_retryable')
                 ORDER BY id DESC
                 LIMIT 1",
            params![input.project_id, input.business_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let (operation_id, owns_operation_lock) =
        if let Some((operation_id, status, lock_owner, locked_until_existing)) = operation {
            tx.execute(
                "UPDATE idempotency_requests
                     SET operation_id = ?1, updated_at = datetime('now')
                     WHERE id = ?2",
                params![operation_id, request_id],
            )?;

            let can_take_lock = status == "failed_retryable"
                || locked_until_existing < now
                || lock_owner == Some(request_id);
            if can_take_lock && status != "succeeded_open" {
                tx.execute(
                    "UPDATE merge_request_create_operations
                         SET status = 'active',
                             lock_owner_request_id = ?1,
                             locked_until = ?2,
                             updated_at = datetime('now')
                         WHERE id = ?3",
                    params![request_id, locked_until, operation_id],
                )?;
                (operation_id, true)
            } else {
                (operation_id, lock_owner == Some(request_id))
            }
        } else {
            tx.execute(
                "INSERT INTO merge_request_create_operations
                     (project_id, platform, project_path, source_project_path, business_key,
                      business_key_json, source_branch, target_branch, purpose_type, purpose_id,
                      status, lock_owner_request_id, locked_until)
                     VALUES (?1, ?2, ?3, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?11)",
                params![
                    input.project_id,
                    input.platform,
                    input.project_path,
                    input.business_key,
                    input.business_key_json,
                    input.source_branch,
                    input.target_branch,
                    input.purpose_type,
                    input.purpose_id,
                    request_id,
                    locked_until
                ],
            )?;
            let operation_id = tx.last_insert_rowid();
            tx.execute(
                "UPDATE idempotency_requests
                     SET operation_id = ?1, updated_at = datetime('now')
                     WHERE id = ?2",
                params![operation_id, request_id],
            )?;
            (operation_id, true)
        };

    tx.commit()?;
    Ok(Registration {
        request_id,
        operation_id,
        owns_operation_lock,
        replay: None,
    })
}

fn is_retryable_sqlite_error(err: &WebPlatformError) -> bool {
    matches!(
        err,
        WebPlatformError::Database(rusqlite::Error::SqliteFailure(sqlite_err, _))
            if matches!(
                sqlite_err.code,
                rusqlite::ErrorCode::DatabaseBusy
                    | rusqlite::ErrorCode::DatabaseLocked
                    | rusqlite::ErrorCode::ConstraintViolation
            )
    )
}

async fn reconcile_or_create(
    execution: CreateExecution<'_>,
    registration: Registration,
) -> Result<CreateMergeRequestServiceResponse, WebPlatformError> {
    let repo = execution.repo;
    let project = execution.project;
    let user_id = execution.user_id;
    let platform_token = execution.platform_token;
    let client = execution.client;
    let project_path = execution.project_path;
    let req = execution.req;

    if let Some(open) = client
        .find_open_merge_request_by_branches(
            platform_token,
            project_path,
            &req.source_branch,
            &req.target_branch,
        )
        .await
        .map_err(map_precheck_error)?
    {
        let status = if registration.owns_operation_lock {
            "reconciled"
        } else {
            "reused_open"
        };
        return save_success(SuccessInput {
            repo,
            project_id: project.id,
            user_id,
            request_id: registration.request_id,
            operation_id: registration.operation_id,
            mr: &open,
            req,
            idempotency_status: status,
        })
        .await;
    }

    let closed_or_merged = client
        .find_merge_requests_by_branches(
            platform_token,
            project_path,
            &req.source_branch,
            &req.target_branch,
            &[MergeRequestState::Closed, MergeRequestState::Merged],
        )
        .await
        .map_err(map_precheck_error)?;

    if !closed_or_merged.is_empty() {
        mark_failed_final(
            repo,
            registration.request_id,
            registration.operation_id,
            "closed_or_merged",
            "Only closed or merged PR/MR exists for this branch pair; create a new source branch",
            StatusCode::CONFLICT,
            "BIZ_003",
        )
        .await?;
        return Err(WebPlatformError::Conflict(
            "Only closed or merged PR/MR exists for this branch pair; create a new source branch"
                .to_string(),
        ));
    }

    if !registration.owns_operation_lock {
        return save_in_progress(
            repo,
            registration.request_id,
            registration.operation_id,
            req,
        )
        .await;
    }

    let Some(_lease) = acquire_create_lease(repo, registration.operation_id).await? else {
        return save_in_progress(
            repo,
            registration.request_id,
            registration.operation_id,
            req,
        )
        .await;
    };

    let create_req = CreateMergeRequest {
        source_branch: req.source_branch.clone(),
        target_branch: req.target_branch.clone(),
        title: req.title.clone(),
        description: req.description.clone(),
        draft: req.draft,
    };

    match client
        .create_merge_request(platform_token, project_path, &create_req)
        .await
    {
        Ok(created) => {
            save_success(SuccessInput {
                repo,
                project_id: project.id,
                user_id,
                request_id: registration.request_id,
                operation_id: registration.operation_id,
                mr: &created,
                req,
                idempotency_status: "created",
            })
            .await
        }
        Err(GitPlatformError::Conflict {
            code: PlatformConflictCode::ExistingOpenMergeRequest,
            ..
        }) => {
            if let Some(open) = client
                .find_open_merge_request_by_branches(
                    platform_token,
                    project_path,
                    &req.source_branch,
                    &req.target_branch,
                )
                .await
                .map_err(map_precheck_error)?
            {
                save_success(SuccessInput {
                    repo,
                    project_id: project.id,
                    user_id,
                    request_id: registration.request_id,
                    operation_id: registration.operation_id,
                    mr: &open,
                    req,
                    idempotency_status: "reconciled",
                })
                .await
            } else {
                mark_retryable(
                    repo,
                    registration.operation_id,
                    "platform_conflict_without_open_pr",
                    "Platform reported an existing PR/MR but reconciliation found none",
                )
                .await?;
                Err(WebPlatformError::ExternalService(
                    "Platform result is unclear; retry with the same Idempotency-Key".to_string(),
                ))
            }
        }
        Err(
            err @ (GitPlatformError::ServiceUnavailable(_) | GitPlatformError::RequestError(_)),
        ) => {
            if let Some(open) = client
                .find_open_merge_request_by_branches(
                    platform_token,
                    project_path,
                    &req.source_branch,
                    &req.target_branch,
                )
                .await
                .ok()
                .flatten()
            {
                return save_success(SuccessInput {
                    repo,
                    project_id: project.id,
                    user_id,
                    request_id: registration.request_id,
                    operation_id: registration.operation_id,
                    mr: &open,
                    req,
                    idempotency_status: "reconciled",
                })
                .await;
            }
            mark_retryable(
                repo,
                registration.operation_id,
                "platform_retryable",
                &err.to_string(),
            )
            .await?;
            Err(WebPlatformError::ExternalService(
                "Platform request failed; retry with the same Idempotency-Key".to_string(),
            ))
        }
        Err(err) => {
            let mapped = map_final_platform_error(&err);
            let response_message = error_response_message(&mapped);
            mark_failed_final(
                repo,
                registration.request_id,
                registration.operation_id,
                "platform_final",
                &response_message,
                error_status_code(&mapped),
                error_ret_code(&mapped),
            )
            .await?;
            Err(mapped)
        }
    }
}

fn map_precheck_error(err: GitPlatformError) -> WebPlatformError {
    match err {
        GitPlatformError::TokenInvalid(message) => WebPlatformError::TokenInvalid(message),
        GitPlatformError::Forbidden(_) => WebPlatformError::Forbidden,
        GitPlatformError::ServiceUnavailable(message) | GitPlatformError::RequestError(message) => {
            WebPlatformError::ExternalService(message)
        }
        GitPlatformError::NotFound(message) => WebPlatformError::NotFound(message),
        GitPlatformError::Validation { message, .. }
        | GitPlatformError::Conflict { message, .. } => WebPlatformError::BadRequest(message),
    }
}

fn map_final_platform_error(err: &GitPlatformError) -> WebPlatformError {
    match err {
        GitPlatformError::TokenInvalid(message) => WebPlatformError::TokenInvalid(message.clone()),
        GitPlatformError::Forbidden(_) => WebPlatformError::Forbidden,
        GitPlatformError::NotFound(message) => WebPlatformError::NotFound(message.clone()),
        GitPlatformError::Validation { code, message } => match code {
            PlatformValidationCode::NoCommits => WebPlatformError::Conflict(message.clone()),
            _ => WebPlatformError::BadRequest(message.clone()),
        },
        GitPlatformError::Conflict { code, message } => match code {
            PlatformConflictCode::ExistingOpenMergeRequest => {
                WebPlatformError::Conflict(message.clone())
            }
            _ => WebPlatformError::Conflict(message.clone()),
        },
        GitPlatformError::ServiceUnavailable(message) | GitPlatformError::RequestError(message) => {
            WebPlatformError::ExternalService(message.clone())
        }
    }
}

fn error_status_code(err: &WebPlatformError) -> StatusCode {
    match err {
        WebPlatformError::TokenInvalid(_) => StatusCode::BAD_REQUEST,
        WebPlatformError::Forbidden => StatusCode::FORBIDDEN,
        WebPlatformError::NotFound(_) => StatusCode::NOT_FOUND,
        WebPlatformError::BadRequest(_) => StatusCode::BAD_REQUEST,
        WebPlatformError::Conflict(_) => StatusCode::CONFLICT,
        WebPlatformError::ExternalService(_) => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn error_ret_code(err: &WebPlatformError) -> &'static str {
    match err {
        WebPlatformError::TokenInvalid(_) => "TOKEN_001",
        WebPlatformError::Forbidden => "AUTH_002",
        WebPlatformError::NotFound(_) => "BIZ_002",
        WebPlatformError::BadRequest(_) => "BIZ_001",
        WebPlatformError::Conflict(_) => "BIZ_003",
        WebPlatformError::ExternalService(_) => "EXT_001",
        _ => "SYS_001",
    }
}

fn error_response_message(err: &WebPlatformError) -> String {
    match err {
        WebPlatformError::TokenInvalid(message)
        | WebPlatformError::NotFound(message)
        | WebPlatformError::BadRequest(message)
        | WebPlatformError::Conflict(message)
        | WebPlatformError::ExternalService(message) => message.clone(),
        WebPlatformError::Forbidden => err.to_string(),
        _ => "Internal server error".to_string(),
    }
}

fn active_create_operations() -> &'static DashMap<String, ()> {
    ACTIVE_CREATE_OPERATIONS.get_or_init(DashMap::new)
}

impl Drop for CreateLease {
    fn drop(&mut self) {
        active_create_operations().remove(&self.active_key);
    }
}

fn create_operation_active_key(
    conn: &rusqlite::Connection,
    operation_id: i64,
) -> Result<String, rusqlite::Error> {
    let database_path = conn
        .query_row("PRAGMA database_list", [], |row| row.get::<_, String>(2))
        .unwrap_or_default();
    if database_path.is_empty() {
        Ok(format!("operation:{operation_id}"))
    } else {
        Ok(format!("{database_path}:{operation_id}"))
    }
}

async fn acquire_create_lease(
    repo: &SqliteRepository,
    operation_id: i64,
) -> Result<Option<CreateLease>, WebPlatformError> {
    let pool = repo.pool();
    let acquired = tokio::task::spawn_blocking(move || {
        let mut conn = pool.get()?;
        let active_key = create_operation_active_key(&conn, operation_id)?;
        let tx = conn.transaction()?;
        let now = utc_sql_now();
        let existing: Option<Option<String>> = tx
            .query_row(
                "SELECT create_lease_expires_at
             FROM merge_request_create_operations
             WHERE id = ?1",
                [operation_id],
                |row| row.get(0),
            )
            .optional()?;

        let Some(create_lease_expires_at) = existing else {
            tx.commit()?;
            return Ok::<Option<String>, WebPlatformError>(None);
        };

        if create_lease_expires_at
            .as_deref()
            .is_some_and(|expires_at| expires_at > now.as_str())
            || active_create_operations().contains_key(&active_key)
        {
            tx.commit()?;
            return Ok::<Option<String>, WebPlatformError>(None);
        }

        let token = Uuid::new_v4().to_string();
        let rows = tx.execute(
            "UPDATE merge_request_create_operations
             SET create_lease_token = ?1,
                 create_lease_expires_at = ?2,
                 creation_started_at = datetime('now'),
                 updated_at = datetime('now')
             WHERE id = ?3
               AND (create_lease_expires_at IS NULL OR create_lease_expires_at <= ?4)",
            params![
                token,
                utc_sql_after(CREATE_LEASE_SECONDS),
                operation_id,
                now
            ],
        )?;
        tx.commit()?;
        if rows == 0 {
            Ok::<Option<String>, WebPlatformError>(None)
        } else {
            Ok::<Option<String>, WebPlatformError>(Some(active_key))
        }
    })
    .await
    .unwrap()?;

    Ok(acquired.map(|active_key| {
        active_create_operations().insert(active_key.clone(), ());
        CreateLease { active_key }
    }))
}

async fn save_success(
    input: SuccessInput<'_>,
) -> Result<CreateMergeRequestServiceResponse, WebPlatformError> {
    let body = success_body(json!({
        "operation_id": input.operation_id,
        "iid": input.mr.iid,
        "state": input.mr.state,
        "source_branch": input.req.source_branch,
        "target_branch": input.req.target_branch,
        "web_url": input.mr.web_url,
        "idempotency_status": input.idempotency_status,
    }));
    let body_raw = serde_json::to_string(&body)
        .map_err(|e| WebPlatformError::Internal(format!("failed to serialize response: {e}")))?;
    let pool = input.repo.pool();
    let platform_node_id = input.mr.platform_node_id.clone();
    let web_url = input.mr.web_url.clone();
    let platform_iid = input.mr.iid as i64;
    let operation_id = input.operation_id;
    let request_id = input.request_id;
    let project_id = input.project_id;
    let user_id = input.user_id;
    let mr_iid = input.mr.iid;

    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE merge_request_create_operations
             SET status = 'succeeded_open',
                 platform_iid = ?1,
                 platform_node_id = ?2,
                 web_url = ?3,
                 create_lease_token = NULL,
                 create_lease_expires_at = NULL,
                 last_error_code = NULL,
                 last_error_message = NULL,
                 updated_at = datetime('now')
             WHERE id = ?4",
            params![platform_iid, platform_node_id, web_url, operation_id],
        )?;
        tx.execute(
            "UPDATE idempotency_requests
             SET response_status = 'succeeded',
                 http_status = 200,
                 response_json = ?1,
                 updated_at = datetime('now')
             WHERE id = ?2",
            params![body_raw, request_id],
        )?;
        tx.commit()?;
        Ok::<_, WebPlatformError>(())
    })
    .await
    .unwrap()?;

    invalidate_project_mr_cache(input.repo, project_id, user_id, mr_iid);

    Ok(CreateMergeRequestServiceResponse {
        http_status: StatusCode::OK,
        body,
    })
}

async fn save_in_progress(
    repo: &SqliteRepository,
    request_id: i64,
    operation_id: i64,
    req: &NormalizedCreateMrRequest,
) -> Result<CreateMergeRequestServiceResponse, WebPlatformError> {
    let body = success_body(json!({
        "operation_id": operation_id,
        "iid": Value::Null,
        "state": "creating",
        "source_branch": req.source_branch,
        "target_branch": req.target_branch,
        "web_url": Value::Null,
        "idempotency_status": "in_progress",
        "retry_after_seconds": 2,
    }));
    let body_raw = serde_json::to_string(&body)
        .map_err(|e| WebPlatformError::Internal(format!("failed to serialize response: {e}")))?;
    let pool = repo.pool();
    tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        conn.execute(
            "UPDATE idempotency_requests
             SET response_status = 'in_progress',
                 http_status = 200,
                 response_json = ?1,
                 updated_at = datetime('now')
             WHERE id = ?2",
            params![body_raw, request_id],
        )?;
        Ok::<_, WebPlatformError>(())
    })
    .await
    .unwrap()?;

    Ok(CreateMergeRequestServiceResponse {
        http_status: StatusCode::OK,
        body,
    })
}

async fn mark_retryable(
    repo: &SqliteRepository,
    operation_id: i64,
    code: &str,
    message: &str,
) -> Result<(), WebPlatformError> {
    let pool = repo.pool();
    let code = code.to_string();
    let message = truncate_error(message);
    tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        conn.execute(
            "UPDATE merge_request_create_operations
             SET status = 'failed_retryable',
                 last_error_code = ?1,
                 last_error_message = ?2,
                 create_lease_token = NULL,
                 create_lease_expires_at = NULL,
                 updated_at = datetime('now')
             WHERE id = ?3",
            params![code, message, operation_id],
        )?;
        Ok::<_, WebPlatformError>(())
    })
    .await
    .unwrap()
}

async fn mark_failed_final(
    repo: &SqliteRepository,
    request_id: i64,
    operation_id: i64,
    code: &str,
    message: &str,
    http_status: StatusCode,
    ret_code: &str,
) -> Result<(), WebPlatformError> {
    let pool = repo.pool();
    let code = code.to_string();
    let message = truncate_error(message);
    let error_body = json!({
        "data": Value::Null,
        "success": false,
        "retCode": ret_code,
        "retMsg": message,
        "showType": 1,
    });
    let error_body_raw = serde_json::to_string(&error_body)
        .map_err(|e| WebPlatformError::Internal(format!("failed to serialize response: {e}")))?;
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get()?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE merge_request_create_operations
             SET status = 'failed_final',
                 last_error_code = ?1,
                 last_error_message = ?2,
                 create_lease_token = NULL,
                 create_lease_expires_at = NULL,
                 updated_at = datetime('now')
             WHERE id = ?3",
            params![code, message, operation_id],
        )?;
        tx.execute(
            "UPDATE idempotency_requests
             SET response_status = 'failed_final',
                 http_status = ?1,
                 response_json = ?2,
                 updated_at = datetime('now')
             WHERE id = ?3",
            params![http_status.as_u16() as i64, error_body_raw, request_id],
        )?;
        tx.commit()?;
        Ok::<_, WebPlatformError>(())
    })
    .await
    .unwrap()
}

fn success_body(data: Value) -> Value {
    json!({
        "data": data,
        "success": true,
        "retCode": "0",
        "retMsg": "ok",
    })
}

fn utc_sql_now() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn utc_sql_after(seconds: i64) -> String {
    (Utc::now() + Duration::seconds(seconds))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn truncate_error(message: &str) -> String {
    message.chars().take(500).collect()
}

fn invalidate_project_mr_cache(
    _repo: &SqliteRepository,
    _project_id: i64,
    _user_id: i64,
    _mr_iid: u64,
) {
    // ApiCache invalidation is handled by the HTTP layer where AppState is available.
}
