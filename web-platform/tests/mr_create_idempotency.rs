use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use axum::http::StatusCode;
use tempfile::TempDir;
use web_platform::auth::password::hash_password;
use web_platform::db::init_pool;
use web_platform::models::kanban::{CreateIssueRequest, PlatformIssue, PlatformMergeRequest};
use web_platform::models::merge_request::CreateMergeRequestApiRequest;
use web_platform::models::{NewProject, Project};
use web_platform::repository::{ProjectRepository, SqliteRepository, UserRepository};
use web_platform::services::git_platform::{
    CreateMergeRequest, GitPlatformClient, GitPlatformError, ListIssuesOptions,
    ListMergeRequestsOptions, MergeRequestState, PlatformMember, PlatformValidationCode,
};
use web_platform::services::mr_create::create_merge_request_idempotent;

fn platform_mr(
    iid: u64,
    source_branch: &str,
    target_branch: &str,
    state: &str,
) -> PlatformMergeRequest {
    PlatformMergeRequest {
        iid,
        title: "fix: login".to_string(),
        description: Some("Closes #123".to_string()),
        state: state.to_string(),
        author: web_platform::models::issue::PlatformUser {
            username: "alice".to_string(),
            display_name: None,
            avatar_url: None,
        },
        source_branch: source_branch.to_string(),
        target_branch: target_branch.to_string(),
        ci_status: None,
        ci_web_url: None,
        review_status: None,
        reviewers: Vec::new(),
        merge_status: None,
        related_issue_iids: Vec::new(),
        additions: None,
        deletions: None,
        changed_files: None,
        created_at: "2026-05-25T00:00:00Z".to_string(),
        updated_at: "2026-05-25T00:00:00Z".to_string(),
        merged_at: None,
        web_url: format!("https://github.com/acme/app/pull/{iid}"),
        platform_node_id: Some(format!("PR_{iid}")),
        source_project_path: Some("acme/app".to_string()),
        target_project_path: Some("acme/app".to_string()),
    }
}

#[derive(Default)]
struct FakePlatformClient {
    created: AtomicUsize,
    open: Mutex<Option<PlatformMergeRequest>>,
    closed: Mutex<Vec<PlatformMergeRequest>>,
    create_delay_ms: AtomicUsize,
    create_error: Mutex<Option<String>>,
}

#[async_trait]
impl GitPlatformClient for FakePlatformClient {
    async fn list_issues(
        &self,
        _token: &str,
        _project_path: &str,
        _options: &ListIssuesOptions,
    ) -> Result<(Vec<PlatformIssue>, u64), GitPlatformError> {
        unimplemented!("not used by MR create tests")
    }

    async fn get_issue(
        &self,
        _token: &str,
        _project_path: &str,
        _iid: u64,
    ) -> Result<PlatformIssue, GitPlatformError> {
        unimplemented!("not used by MR create tests")
    }

    async fn create_issue(
        &self,
        _token: &str,
        _project_path: &str,
        _req: &CreateIssueRequest,
    ) -> Result<PlatformIssue, GitPlatformError> {
        unimplemented!("not used by MR create tests")
    }

    async fn get_issue_merge_requests(
        &self,
        _token: &str,
        _project_path: &str,
        _issue_iid: u64,
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError> {
        unimplemented!("not used by MR create tests")
    }

    async fn list_merge_requests(
        &self,
        _token: &str,
        _project_path: &str,
        _options: &ListMergeRequestsOptions,
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError> {
        unimplemented!("not used by MR create tests")
    }

    async fn get_merge_request(
        &self,
        _token: &str,
        _project_path: &str,
        _mr_iid: u64,
    ) -> Result<PlatformMergeRequest, GitPlatformError> {
        unimplemented!("not used by MR create tests")
    }

    async fn list_members(
        &self,
        _token: &str,
        _project_path: &str,
    ) -> Result<Vec<PlatformMember>, GitPlatformError> {
        unimplemented!("not used by MR create tests")
    }

    async fn create_merge_request(
        &self,
        _token: &str,
        _project_path: &str,
        req: &CreateMergeRequest,
    ) -> Result<PlatformMergeRequest, GitPlatformError> {
        self.created.fetch_add(1, Ordering::SeqCst);
        if let Some(message) = self.create_error.lock().unwrap().clone() {
            return Err(GitPlatformError::Validation {
                code: PlatformValidationCode::SourceBranchNotFound,
                message,
            });
        }

        let delay_ms = self.create_delay_ms.load(Ordering::SeqCst).max(25);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms as u64)).await;
        let mr = platform_mr(17, &req.source_branch, &req.target_branch, "opened");
        *self.open.lock().unwrap() = Some(mr.clone());
        Ok(mr)
    }

    async fn find_open_merge_request_by_branches(
        &self,
        _token: &str,
        _project_path: &str,
        _source_branch: &str,
        _target_branch: &str,
    ) -> Result<Option<PlatformMergeRequest>, GitPlatformError> {
        Ok(self.open.lock().unwrap().clone())
    }

    async fn find_merge_requests_by_branches(
        &self,
        _token: &str,
        _project_path: &str,
        _source_branch: &str,
        _target_branch: &str,
        states: &[MergeRequestState],
    ) -> Result<Vec<PlatformMergeRequest>, GitPlatformError> {
        assert!(states.contains(&MergeRequestState::Closed));
        assert!(states.contains(&MergeRequestState::Merged));
        Ok(self.closed.lock().unwrap().clone())
    }
}

async fn test_repo_and_project() -> (TempDir, SqliteRepository, Project, i64) {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("test.db");
    let repo = SqliteRepository::new(init_pool(db_path.to_str().unwrap()));

    let user = repo
        .create_user(
            "alice",
            &hash_password("password123").unwrap(),
            Some("Alice"),
            "admin",
        )
        .await
        .unwrap();

    let project = repo
        .create_project(&NewProject {
            name: "app".to_string(),
            description: None,
            git_url: "https://github.com/acme/app.git".to_string(),
            platform: "github".to_string(),
            platform_host: None,
            namespace: "acme".to_string(),
            repo_name: "app".to_string(),
            default_branch: "main".to_string(),
            workflow_template: "github".to_string(),
            workflow_content: None,
            created_by: user.id,
        })
        .await
        .unwrap();

    (dir, repo, project, user.id)
}

fn expire_operation_lock_and_lease(dir: &TempDir) {
    let db_path = dir.path().join("test.db");
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "UPDATE merge_request_create_operations
         SET locked_until = datetime('now', '-10 minutes'),
             create_lease_expires_at = datetime('now', '-9 minutes')",
        [],
    )
    .unwrap();
}

fn force_operation_to_stale_active_lease(dir: &TempDir) {
    let db_path = dir.path().join("test.db");
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.execute(
        "UPDATE merge_request_create_operations
         SET status = 'active',
             locked_until = datetime('now', '-10 minutes'),
             create_lease_token = 'stale-token',
             create_lease_expires_at = datetime('now', '-9 minutes'),
             platform_iid = NULL,
             platform_node_id = NULL,
             web_url = NULL",
        [],
    )
    .unwrap();
}

fn request(title: &str) -> CreateMergeRequestApiRequest {
    CreateMergeRequestApiRequest {
        source_branch: "codex/issue-123-login-fix".to_string(),
        target_branch: None,
        title: title.to_string(),
        description: Some("   ".to_string()),
        purpose_type: Some("issue_delivery".to_string()),
        purpose_id: Some("123".to_string()),
        draft: None,
    }
}

#[tokio::test]
async fn same_idempotency_key_replays_success_and_rejects_hash_mismatch() {
    let (_dir, repo, project, user_id) = test_repo_and_project().await;
    let client = Arc::new(FakePlatformClient::default());

    let first = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stable-key-1",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(first.http_status, StatusCode::OK);
    assert_eq!(first.body["data"]["idempotency_status"], "created");
    assert_eq!(client.created.load(Ordering::SeqCst), 1);

    let replay = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stable-key-1",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap();
    assert_eq!(replay.http_status, StatusCode::OK);
    assert_eq!(replay.body["data"]["idempotency_status"], "replayed");
    assert_eq!(replay.body["data"]["iid"], 17);
    assert_eq!(client.created.load(Ordering::SeqCst), 1);

    let mismatch = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stable-key-1",
        request("fix: different title"),
        client.as_ref(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        mismatch.to_string(),
        "Idempotency-Key was reused with a different request"
    );
}

#[tokio::test]
async fn different_keys_for_same_business_reuse_existing_open_pr() {
    let (_dir, repo, project, user_id) = test_repo_and_project().await;
    let client = Arc::new(FakePlatformClient::default());

    create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stable-key-1",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap();

    let reused = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stable-key-2",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap();

    assert_eq!(reused.http_status, StatusCode::OK);
    assert_eq!(reused.body["data"]["idempotency_status"], "reused_open");
    assert_eq!(reused.body["data"]["iid"], 17);
    assert_eq!(client.created.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn final_error_replay_uses_same_api_error_body_as_first_failure() {
    let (_dir, repo, project, user_id) = test_repo_and_project().await;
    let client = Arc::new(FakePlatformClient::default());
    *client.create_error.lock().unwrap() = Some("source branch missing".to_string());

    let first = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stable-error-key",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap_err();
    assert_eq!(first.to_string(), "source branch missing");

    let replay = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stable-error-key",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap();

    assert_eq!(replay.http_status, StatusCode::BAD_REQUEST);
    assert_eq!(replay.body["success"], false);
    assert_eq!(replay.body["retCode"], "BIZ_001");
    assert_eq!(replay.body["retMsg"], "source branch missing");
    assert_eq!(client.created.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expired_create_lease_does_not_start_second_platform_create() {
    let (dir, repo, project, user_id) = test_repo_and_project().await;
    let client = Arc::new(FakePlatformClient::default());
    client.create_delay_ms.store(500, Ordering::SeqCst);

    let repo_for_first = repo.clone();
    let project_for_first = project.clone();
    let client_for_first = client.clone();
    let first = tokio::spawn(async move {
        create_merge_request_idempotent(
            &repo_for_first,
            &project_for_first,
            user_id,
            "token",
            "slow-key-1",
            request("fix: login"),
            client_for_first.as_ref(),
        )
        .await
    });

    while client.created.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    expire_operation_lock_and_lease(&dir);

    let second = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "slow-key-2",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap();

    assert_eq!(second.body["data"]["idempotency_status"], "in_progress");
    assert_eq!(client.created.load(Ordering::SeqCst), 1);

    let first_response = first.await.unwrap().unwrap();
    assert_eq!(first_response.body["data"]["idempotency_status"], "created");
    assert_eq!(client.created.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stale_expired_lease_without_active_create_can_retry_create() {
    let (dir, repo, project, user_id) = test_repo_and_project().await;
    let client = Arc::new(FakePlatformClient::default());
    *client.create_error.lock().unwrap() = Some("source branch missing".to_string());

    let first = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stale-key-1",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap_err();
    assert_eq!(first.to_string(), "source branch missing");
    assert_eq!(client.created.load(Ordering::SeqCst), 1);

    *client.create_error.lock().unwrap() = None;
    force_operation_to_stale_active_lease(&dir);

    let recovered = create_merge_request_idempotent(
        &repo,
        &project,
        user_id,
        "token",
        "stale-key-2",
        request("fix: login"),
        client.as_ref(),
    )
    .await
    .unwrap();

    assert_eq!(recovered.http_status, StatusCode::OK);
    assert_eq!(recovered.body["data"]["idempotency_status"], "created");
    assert_eq!(client.created.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn same_key_concurrent_requests_do_not_leak_unique_constraint_errors() {
    let (_dir, repo, project, user_id) = test_repo_and_project().await;
    let client = Arc::new(FakePlatformClient::default());
    let mut tasks = Vec::new();

    for _ in 0..24 {
        let repo = repo.clone();
        let project = project.clone();
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            create_merge_request_idempotent(
                &repo,
                &project,
                user_id,
                "token",
                "shared-key",
                request("fix: login"),
                client.as_ref(),
            )
            .await
        }));
    }

    let mut success_count = 0;
    for task in tasks {
        let response = task.await.unwrap().unwrap();
        assert_eq!(response.http_status, StatusCode::OK);
        success_count += 1;
    }

    assert_eq!(success_count, 24);
    assert_eq!(client.created.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn different_keys_same_business_concurrent_requests_create_once() {
    let (_dir, repo, project, user_id) = test_repo_and_project().await;
    let client = Arc::new(FakePlatformClient::default());
    let mut tasks = Vec::new();

    for idx in 0..24 {
        let repo = repo.clone();
        let project = project.clone();
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            create_merge_request_idempotent(
                &repo,
                &project,
                user_id,
                "token",
                &format!("business-key-{idx}"),
                request("fix: login"),
                client.as_ref(),
            )
            .await
        }));
    }

    for task in tasks {
        let response = task.await.unwrap().unwrap();
        assert_eq!(response.http_status, StatusCode::OK);
    }

    assert_eq!(client.created.load(Ordering::SeqCst), 1);
}
