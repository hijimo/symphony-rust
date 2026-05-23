# Phase 3 Backend Test Framework - Kanban API Testing

## 1. Test Directory Structure

```
web-platform/
├── src/
│   └── ... (production code)
├── tests/
│   ├── common/
│   │   ├── mod.rs                  # TestApp + shared helpers (extended)
│   │   ├── fixtures.rs             # Test data factories
│   │   ├── mock_gitlab.rs          # GitLab API mock server
│   │   └── mock_github.rs          # GitHub API mock server
│   ├── unit/
│   │   ├── git_api_client.rs       # Git platform API client unit tests
│   │   ├── kanban_transform.rs     # Data transformation logic tests
│   │   ├── cache_layer.rs          # Cache TTL and eviction tests
│   │   └── ai_prompt_builder.rs    # AI prompt construction tests
│   ├── integration/
│   │   ├── kanban_cache.rs         # Cache integration with real TTL
│   │   ├── gitlab_client.rs        # GitLab client with mock server
│   │   ├── github_client.rs        # GitHub client with mock server
│   │   └── ai_service.rs           # AI service integration tests
│   ├── api/
│   │   ├── api_kanban.rs           # GET /api/projects/:id/kanban
│   │   ├── api_issues.rs           # POST/GET issue endpoints
│   │   ├── api_issue_ai.rs         # AI generation endpoint (SSE)
│   │   ├── api_issue_mrs.rs        # Issue-MR association endpoint
│   │   └── api_mr_detail.rs        # MR detail endpoint
│   ├── e2e/
│   │   ├── kanban_lifecycle.rs     # Full kanban workflow E2E
│   │   ├── multi_user_kanban.rs    # Multi-user concurrent access
│   │   └── external_api_e2e.rs     # Real GitLab/GitHub API E2E
│   ├── api_auth.rs                 # (existing) Auth API tests
│   ├── api_admin_users.rs          # (existing) Admin user tests
│   ├── api_user_profile.rs         # (existing) Profile tests
│   ├── api_projects.rs             # (existing) Project tests
│   ├── api_members.rs              # (existing) Member tests
│   ├── api_service.rs              # (existing) Service tests
│   ├── api_workflow.rs             # (existing) Workflow tests
│   └── e2e.rs                      # (existing) E2E tests
```

---

## 2. Unit Tests

### 2.1 Test Strategy Per Module

| Module | Focus | Mocking Strategy |
|--------|-------|-----------------|
| `handlers/kanban.rs` | Request parsing, response formatting, error mapping | Mock repository + mock git client trait |
| `handlers/issues.rs` | Issue creation validation, AI request building | Mock git client trait |
| `repository/` | SQL correctness (tested at integration level) | Real SQLite (tempfile) |
| `auth/` | JWT validation, permission checks | Existing patterns (no mock needed) |
| `git_client/gitlab.rs` | HTTP request construction, response parsing | Mock HTTP responses |
| `git_client/github.rs` | HTTP request construction, response parsing | Mock HTTP responses |
| `cache/kanban_cache.rs` | TTL logic, eviction, singleflight dedup | Time mocking |
| `ai/issue_generator.rs` | Prompt construction, SSE stream parsing | Mock HTTP client |

### 2.2 Trait-Based Mocking Strategy

Phase 3 introduces external API dependencies (GitLab/GitHub). We use trait-based abstraction for testability:

```rust
// src/git_client/traits.rs
use async_trait::async_trait;
use crate::error::Result;
use crate::models::kanban::{KanbanIssue, MergeRequest, IssueDetail, MrDetail};

#[async_trait]
pub trait GitPlatformClient: Send + Sync {
    /// Fetch open issues (pending column)
    async fn list_issues(
        &self,
        token: &str,
        namespace: &str,
        repo: &str,
        state: &str,
        labels: Option<&str>,
        page: u32,
        per_page: u32,
    ) -> Result<Vec<KanbanIssue>>;

    /// Fetch a single issue detail
    async fn get_issue(
        &self,
        token: &str,
        namespace: &str,
        repo: &str,
        iid: u64,
    ) -> Result<IssueDetail>;

    /// Create a new issue
    async fn create_issue(
        &self,
        token: &str,
        namespace: &str,
        repo: &str,
        title: &str,
        description: &str,
        labels: &[String],
    ) -> Result<KanbanIssue>;

    /// Get MRs/PRs related to an issue
    async fn get_issue_merge_requests(
        &self,
        token: &str,
        namespace: &str,
        repo: &str,
        issue_iid: u64,
    ) -> Result<Vec<MergeRequest>>;

    /// Get MR/PR detail
    async fn get_merge_request(
        &self,
        token: &str,
        namespace: &str,
        repo: &str,
        mr_iid: u64,
    ) -> Result<MrDetail>;
}
```

Unit test example using mock:

```rust
// tests/unit/git_api_client.rs
use mockall::predicate::*;
use mockall::mock;

mock! {
    pub GitClient {}

    #[async_trait]
    impl GitPlatformClient for GitClient {
        async fn list_issues(
            &self,
            token: &str,
            namespace: &str,
            repo: &str,
            state: &str,
            labels: Option<&str>,
            page: u32,
            per_page: u32,
        ) -> Result<Vec<KanbanIssue>>;

        async fn get_issue(
            &self,
            token: &str,
            namespace: &str,
            repo: &str,
            iid: u64,
        ) -> Result<IssueDetail>;

        // ... other methods
    }
}

#[tokio::test]
async fn test_list_issues_constructs_correct_request() {
    let mut mock = MockGitClient::new();
    mock.expect_list_issues()
        .with(
            eq("glpat-xxx"),
            eq("group"),
            eq("repo"),
            eq("opened"),
            eq(None),
            eq(1),
            eq(50),
        )
        .times(1)
        .returning(|_, _, _, _, _, _, _| Ok(vec![]));

    let result = mock
        .list_issues("glpat-xxx", "group", "repo", "opened", None, 1, 50)
        .await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}
```

### 2.3 Cache Unit Tests

```rust
// tests/unit/cache_layer.rs
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_cache_returns_fresh_data_within_ttl() {
    let cache = KanbanCache::new(Duration::from_secs(10));
    let key = CacheKey::new(1, "pending");

    cache.set(&key, vec![mock_issue(1)]).await;

    let result = cache.get(&key).await;
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 1);
}

#[tokio::test]
async fn test_cache_expires_after_ttl() {
    let cache = KanbanCache::new(Duration::from_millis(50));
    let key = CacheKey::new(1, "pending");

    cache.set(&key, vec![mock_issue(1)]).await;
    sleep(Duration::from_millis(100)).await;

    let result = cache.get(&key).await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_cache_singleflight_deduplication() {
    let cache = KanbanCache::new(Duration::from_secs(10));
    let call_count = Arc::new(AtomicU32::new(0));

    // Simulate 10 concurrent requests for the same key
    let mut handles = vec![];
    for _ in 0..10 {
        let cache = cache.clone();
        let count = call_count.clone();
        handles.push(tokio::spawn(async move {
            cache.get_or_fetch(CacheKey::new(1, "pending"), || async {
                count.fetch_add(1, Ordering::SeqCst);
                sleep(Duration::from_millis(50)).await;
                Ok(vec![mock_issue(1)])
            }).await
        }));
    }

    let results: Vec<_> = futures::future::join_all(handles).await;
    // Only one actual fetch should have occurred
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    // All 10 requests should get the same result
    for r in results {
        assert!(r.unwrap().is_ok());
    }
}
```

### 2.4 Coverage Targets

| Module | Target Coverage | Rationale |
|--------|----------------|-----------|
| `git_client/` | 90%+ | Critical external integration, many edge cases |
| `handlers/kanban.rs` | 85%+ | Request/response mapping, error paths |
| `cache/` | 95%+ | Correctness critical for data freshness |
| `ai/` | 80%+ | SSE streaming has complex state |
| Overall Phase 3 | 85%+ | Balanced coverage with focus on critical paths |

---

## 3. Integration Tests

### 3.1 Database Integration Tests

Phase 3 kanban data is fetched from external APIs, not stored locally. Database integration tests focus on:
- Project lookup (existing)
- User config retrieval (token access for API calls)
- Cache metadata storage (if persistent cache is added later)

```rust
// tests/integration/kanban_cache.rs
#[tokio::test]
async fn test_user_token_retrieval_for_api_calls() {
    let app = common::TestApp::new().await;

    // Configure user with GitLab token
    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": "gitlab-token-example",
            "gitlabHost": "https://gitlab.example.com"
        }),
        Some(&app.admin_token),
    ).await;

    // Verify token is retrievable for API calls
    let config = app.get("/api/user/config", Some(&app.admin_token)).await;
    let body: serde_json::Value = config.json().await.unwrap();
    assert_eq!(body["data"]["hasGitlabToken"], true);
    assert_eq!(body["data"]["gitlabHost"], "https://gitlab.example.com");
}
```

### 3.2 GitLab/GitHub Client Integration Tests (Mock Server)

```rust
// tests/integration/gitlab_client.rs
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path, header};

#[tokio::test]
async fn test_gitlab_list_issues_integration() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v4/projects/group%2Frepo/issues"))
        .and(header("PRIVATE-TOKEN", "glpat-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "iid": 1,
                "title": "Fix login bug",
                "state": "opened",
                "labels": [],
                "author": {"username": "dev1", "avatar_url": "https://..."},
                "created_at": "2026-05-01T10:00:00Z",
                "updated_at": "2026-05-20T10:00:00Z",
                "web_url": "https://gitlab.example.com/group/repo/-/issues/1"
            }
        ])))
        .mount(&mock_server)
        .await;

    let client = GitLabClient::new(&mock_server.uri());
    let issues = client
        .list_issues("glpat-test", "group", "repo", "opened", None, 1, 50)
        .await
        .unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].iid, 1);
    assert_eq!(issues[0].title, "Fix login bug");
}

#[tokio::test]
async fn test_gitlab_get_issue_merge_requests() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v4/projects/group%2Frepo/issues/1/related_merge_requests"))
        .and(header("PRIVATE-TOKEN", "glpat-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "iid": 10,
                "title": "Fix: resolve login bug",
                "state": "merged",
                "author": {"username": "dev1"},
                "source_branch": "fix/login-bug",
                "target_branch": "main",
                "web_url": "https://gitlab.example.com/group/repo/-/merge_requests/10"
            }
        ])))
        .mount(&mock_server)
        .await;

    let client = GitLabClient::new(&mock_server.uri());
    let mrs = client
        .get_issue_merge_requests("glpat-test", "group", "repo", 1)
        .await
        .unwrap();

    assert_eq!(mrs.len(), 1);
    assert_eq!(mrs[0].iid, 10);
    assert_eq!(mrs[0].state, "merged");
}

#[tokio::test]
async fn test_gitlab_api_returns_401_unauthorized() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v4/projects/group%2Frepo/issues"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "message": "401 Unauthorized"
        })))
        .mount(&mock_server)
        .await;

    let client = GitLabClient::new(&mock_server.uri());
    let result = client
        .list_issues("invalid-token", "group", "repo", "opened", None, 1, 50)
        .await;

    assert!(result.is_err());
    // Should map to our internal error type
    match result.unwrap_err() {
        AppError::ExternalApiUnauthorized(_) => {}
        other => panic!("Expected ExternalApiUnauthorized, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_gitlab_api_returns_rate_limit() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v4/projects/group%2Frepo/issues"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("RateLimit-Reset", "1716300000")
        )
        .mount(&mock_server)
        .await;

    let client = GitLabClient::new(&mock_server.uri());
    let result = client
        .list_issues("glpat-test", "group", "repo", "opened", None, 1, 50)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        AppError::ExternalApiRateLimit { retry_after } => {
            assert!(retry_after > 0);
        }
        other => panic!("Expected ExternalApiRateLimit, got: {:?}", other),
    }
}
```

### 3.3 GitHub Client Integration Tests (Mock Server)

```rust
// tests/integration/github_client.rs
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path, header};

#[tokio::test]
async fn test_github_list_issues_integration() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues"))
        .and(header("Authorization", "Bearer ghp-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "number": 42,
                "title": "Add dark mode",
                "state": "open",
                "labels": [{"name": "enhancement"}],
                "user": {"login": "dev1", "avatar_url": "https://..."},
                "created_at": "2026-05-01T10:00:00Z",
                "updated_at": "2026-05-20T10:00:00Z",
                "html_url": "https://github.com/owner/repo/issues/42"
            }
        ])))
        .mount(&mock_server)
        .await;

    let client = GitHubClient::new(&mock_server.uri());
    let issues = client
        .list_issues("ghp-test-token", "owner", "repo", "open", None, 1, 50)
        .await
        .unwrap();

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].iid, 42);
    assert_eq!(issues[0].title, "Add dark mode");
}

#[tokio::test]
async fn test_github_get_issue_pull_requests_via_timeline() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/owner/repo/issues/42/timeline"))
        .and(header("Authorization", "Bearer ghp-test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "event": "cross-referenced",
                "source": {
                    "type": "issue",
                    "issue": {
                        "number": 100,
                        "title": "feat: add dark mode support",
                        "state": "open",
                        "pull_request": {
                            "url": "https://api.github.com/repos/owner/repo/pulls/100"
                        },
                        "user": {"login": "dev1"}
                    }
                }
            }
        ])))
        .mount(&mock_server)
        .await;

    let client = GitHubClient::new(&mock_server.uri());
    let mrs = client
        .get_issue_merge_requests("ghp-test-token", "owner", "repo", 42)
        .await
        .unwrap();

    assert_eq!(mrs.len(), 1);
    assert_eq!(mrs[0].iid, 100);
}
```

### 3.4 AI Service Integration Tests

```rust
// tests/integration/ai_service.rs
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path, header, body_partial_json};

#[tokio::test]
async fn test_ai_generate_issue_success() {
    let mock_server = MockServer::start().await;

    // Mock Azure OpenAI SSE response
    let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"# Issue Title\\n\\n\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"Description of the issue.\"}}]}\n\n\
                    data: [DONE]\n\n";

    Mock::given(method("POST"))
        .and(path("/openai/deployments/gpt-55/chat/completions"))
        .and(header("api-key", "test-api-key"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Type", "text/event-stream")
                .set_body_string(sse_body)
        )
        .mount(&mock_server)
        .await;

    let ai_service = AiIssueGenerator::new(&mock_server.uri(), "test-api-key");
    let mut stream = ai_service
        .generate_issue("Build a login page with OAuth support", "web-app")
        .await
        .unwrap();

    let mut content = String::new();
    while let Some(chunk) = stream.next().await {
        content.push_str(&chunk.unwrap());
    }

    assert!(content.contains("Issue Title"));
    assert!(content.contains("Description"));
}

#[tokio::test]
async fn test_ai_service_timeout() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/openai/deployments/gpt-55/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_delay(Duration::from_secs(60)))
        .mount(&mock_server)
        .await;

    let ai_service = AiIssueGenerator::with_timeout(
        &mock_server.uri(),
        "test-api-key",
        Duration::from_millis(100),
    );

    let result = ai_service
        .generate_issue("test prompt", "repo")
        .await;

    assert!(result.is_err());
}
```

---

## 4. E2E Tests

### 4.1 Full System Tests with Real GitLab API

These tests use the actual GitLab instance and are gated behind a feature flag or environment variable. They run in CI with real credentials.

```rust
// tests/e2e/external_api_e2e.rs
//! E2E tests that hit real GitLab/GitHub APIs.
//! Run with: cargo test --test external_api_e2e -- --ignored
//! Requires: GITLAB_TOKEN, GITLAB_HOST, TEST_PROJECT_ID env vars

mod common;

use reqwest::StatusCode;
use serde_json::json;

/// Real GitLab instance for E2E testing
const GITLAB_HOST: &str = "http://gitlab.jushuitan-inc.com:8081";
const TEST_NAMESPACE: &str = "zimei10525";
const TEST_REPO: &str = "symphony_e2e_test_repo";

fn gitlab_token() -> String {
    std::env::var("GITLAB_TOKEN")
        .unwrap_or_else(|_| "<your-gitlab-token>".to_string())
}

#[tokio::test]
#[ignore] // Only run in CI or manually
async fn test_e2e_kanban_with_real_gitlab() {
    let app = common::TestApp::new().await;

    // Setup: create project pointing to real GitLab repo
    let project_id = app.create_project(
        &format!("{}/{}/{}", GITLAB_HOST, TEST_NAMESPACE, TEST_REPO),
        &app.admin_token,
    ).await;

    // Configure user's GitLab token
    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": gitlab_token(),
            "gitlabHost": GITLAB_HOST
        }),
        Some(&app.admin_token),
    ).await;

    // Fetch kanban data
    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert!(body["data"]["pending"].is_array());
    assert!(body["data"]["in_progress"].is_array());
    assert!(body["data"]["done"].is_array());
}

#[tokio::test]
#[ignore]
async fn test_e2e_create_issue_real_gitlab() {
    let app = common::TestApp::new().await;

    let project_id = app.create_project(
        &format!("{}/{}/{}", GITLAB_HOST, TEST_NAMESPACE, TEST_REPO),
        &app.admin_token,
    ).await;

    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": gitlab_token(),
            "gitlabHost": GITLAB_HOST
        }),
        Some(&app.admin_token),
    ).await;

    // Create a test issue
    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({
                "title": "[E2E Test] Automated test issue",
                "description": "This issue was created by automated E2E tests. Safe to close.",
                "labels": ["e2e-test"]
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert!(body["data"]["iid"].as_u64().unwrap() > 0);
    assert_eq!(body["data"]["title"], "[E2E Test] Automated test issue");
}

#[tokio::test]
#[ignore]
async fn test_e2e_issue_detail_real_gitlab() {
    let app = common::TestApp::new().await;

    let project_id = app.create_project(
        &format!("{}/{}/{}", GITLAB_HOST, TEST_NAMESPACE, TEST_REPO),
        &app.admin_token,
    ).await;

    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": gitlab_token(),
            "gitlabHost": GITLAB_HOST
        }),
        Some(&app.admin_token),
    ).await;

    // Create an issue first, then fetch its detail
    let create_resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({
                "title": "[E2E Test] Detail test",
                "description": "Testing issue detail endpoint"
            }),
            Some(&app.admin_token),
        )
        .await;
    let create_body: serde_json::Value = create_resp.json().await.unwrap();
    let issue_iid = create_body["data"]["iid"].as_u64().unwrap();

    // Fetch detail
    let resp = app
        .get(
            &format!("/api/projects/{}/issues/{}", project_id, issue_iid),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["iid"], issue_iid);
    assert_eq!(body["data"]["title"], "[E2E Test] Detail test");
    assert!(body["data"]["description"].as_str().is_some());
    assert!(body["data"]["author"].is_object());
}
```

### 4.2 Service Lifecycle Tests

```rust
// tests/e2e/kanban_lifecycle.rs
mod common;

use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn test_kanban_full_lifecycle() {
    let app = common::TestApp::new_with_mock_git_server().await;

    // 1. Create project
    let project_id = app.create_project(
        "https://gitlab.example.com/group/repo",
        &app.admin_token,
    ).await;

    // 2. Configure token
    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": "glpat-mock-token",
            "gitlabHost": &app.mock_gitlab_url
        }),
        Some(&app.admin_token),
    ).await;

    // 3. Fetch kanban (should return mock data)
    let kanban_resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(kanban_resp.status(), StatusCode::OK);
    let kanban: serde_json::Value = kanban_resp.json().await.unwrap();
    assert!(kanban["data"]["pending"].as_array().unwrap().len() > 0);

    // 4. Create an issue
    let issue_resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({
                "title": "New feature request",
                "description": "Implement dark mode"
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(issue_resp.status(), StatusCode::OK);
    let issue: serde_json::Value = issue_resp.json().await.unwrap();
    let issue_iid = issue["data"]["iid"].as_u64().unwrap();

    // 5. Get issue detail
    let detail_resp = app
        .get(
            &format!("/api/projects/{}/issues/{}", project_id, issue_iid),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(detail_resp.status(), StatusCode::OK);

    // 6. Get issue's MRs (should be empty for new issue)
    let mrs_resp = app
        .get(
            &format!("/api/projects/{}/issues/{}/mrs", project_id, issue_iid),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(mrs_resp.status(), StatusCode::OK);
    let mrs: serde_json::Value = mrs_resp.json().await.unwrap();
    assert_eq!(mrs["data"].as_array().unwrap().len(), 0);
}
```

### 4.3 Multi-User Scenario Tests

```rust
// tests/e2e/multi_user_kanban.rs
mod common;

use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn test_multi_user_kanban_token_isolation() {
    let app = common::TestApp::new_with_mock_git_server().await;

    // Create two users with different tokens
    app.create_test_user("user_a", "Pass123456", "user").await;
    app.create_test_user("user_b", "Pass123456", "user").await;
    let token_a = app.login_get_token("user_a", "Pass123456").await;
    let token_b = app.login_get_token("user_b", "Pass123456").await;

    // Create project and add both as members
    let project_id = app.create_project(
        "https://gitlab.example.com/group/shared-repo",
        &app.admin_token,
    ).await;

    let user_a_id = app.get_user_id("user_a").await;
    let user_b_id = app.get_user_id("user_b").await;
    app.add_project_member(project_id, user_a_id, "member", &app.admin_token).await;
    app.add_project_member(project_id, user_b_id, "member", &app.admin_token).await;

    // Each user configures their own GitLab token
    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": "glpat-user-a-token",
            "gitlabHost": &app.mock_gitlab_url
        }),
        Some(&token_a),
    ).await;

    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": "glpat-user-b-token",
            "gitlabHost": &app.mock_gitlab_url
        }),
        Some(&token_b),
    ).await;

    // Both users can access kanban
    let resp_a = app
        .get(&format!("/api/projects/{}/kanban", project_id), Some(&token_a))
        .await;
    let resp_b = app
        .get(&format!("/api/projects/{}/kanban", project_id), Some(&token_b))
        .await;

    assert_eq!(resp_a.status(), StatusCode::OK);
    assert_eq!(resp_b.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_non_member_cannot_access_kanban() {
    let app = common::TestApp::new_with_mock_git_server().await;

    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let project_id = app.create_project(
        "https://gitlab.example.com/group/private-repo",
        &app.admin_token,
    ).await;

    // Outsider tries to access kanban
    let resp = app
        .get(&format!("/api/projects/{}/kanban", project_id), Some(&outsider_token))
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");
}

#[tokio::test]
async fn test_concurrent_kanban_access_performance() {
    let app = common::TestApp::new_with_mock_git_server().await;

    let project_id = app.create_project(
        "https://gitlab.example.com/group/perf-repo",
        &app.admin_token,
    ).await;

    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": "glpat-perf-token",
            "gitlabHost": &app.mock_gitlab_url
        }),
        Some(&app.admin_token),
    ).await;

    // Simulate 20 concurrent kanban requests
    let start = std::time::Instant::now();
    let mut handles = vec![];
    for _ in 0..20 {
        let client = app.client.clone();
        let url = app.url(&format!("/api/projects/{}/kanban", project_id));
        let token = app.admin_token.clone();
        handles.push(tokio::spawn(async move {
            client
                .get(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
                .unwrap()
        }));
    }

    let responses: Vec<_> = futures::future::join_all(handles).await;
    let elapsed = start.elapsed();

    // All should succeed
    for r in &responses {
        assert_eq!(r.as_ref().unwrap().status(), StatusCode::OK);
    }

    // Should complete within 2 seconds (cache + singleflight)
    assert!(elapsed.as_secs() < 2, "Concurrent access took too long: {:?}", elapsed);
}
```

---

## 5. Interface Tests (Per-API Endpoint)

### 5.1 GET /api/projects/:id/kanban

Fetches the three-column kanban board data (pending, in_progress, done).

```rust
// tests/api/api_kanban.rs
mod common;

use reqwest::StatusCode;
use serde_json::json;

// --- Happy Path ---

#[tokio::test]
async fn test_get_kanban_success() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["retCode"], "0");

    // Verify three-column structure
    assert!(body["data"]["pending"].is_array());
    assert!(body["data"]["in_progress"].is_array());
    assert!(body["data"]["done"].is_array());

    // Verify issue structure in pending column
    let pending = body["data"]["pending"].as_array().unwrap();
    if !pending.is_empty() {
        let issue = &pending[0];
        assert!(issue["iid"].is_number());
        assert!(issue["title"].is_string());
        assert!(issue["author"].is_object());
        assert!(issue["author"]["username"].is_string());
        assert!(issue["created_at"].is_string());
        assert!(issue["web_url"].is_string());
    }
}

#[tokio::test]
async fn test_get_kanban_empty_project() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token_empty_repo(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["pending"].as_array().unwrap().len(), 0);
    assert_eq!(body["data"]["in_progress"].as_array().unwrap().len(), 0);
    assert_eq!(body["data"]["done"].as_array().unwrap().len(), 0);
}

// --- Authentication Failures ---

#[tokio::test]
async fn test_get_kanban_no_auth() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(&format!("/api/projects/{}/kanban", project_id), None)
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_001");
}

#[tokio::test]
async fn test_get_kanban_expired_token() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;
    let expired_token = app.generate_expired_token();

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&expired_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// --- Authorization Failures ---

#[tokio::test]
async fn test_get_kanban_non_member() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");
}

// --- Invalid Parameters ---

#[tokio::test]
async fn test_get_kanban_invalid_project_id() {
    let app = common::TestApp::new_with_mock_git_server().await;

    let resp = app
        .get("/api/projects/99999/kanban", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_002");
}

#[tokio::test]
async fn test_get_kanban_non_numeric_project_id() {
    let app = common::TestApp::new_with_mock_git_server().await;

    let resp = app
        .get("/api/projects/abc/kanban", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- External Service Failures ---

#[tokio::test]
async fn test_get_kanban_gitlab_api_down() {
    let app = common::TestApp::new_with_failing_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    // Should return 502 or graceful degradation with cached data
    assert!(
        resp.status() == StatusCode::BAD_GATEWAY
            || resp.status() == StatusCode::OK
    );
}

#[tokio::test]
async fn test_get_kanban_gitlab_token_invalid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_invalid_git_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "EXT_001");
}

#[tokio::test]
async fn test_get_kanban_gitlab_rate_limited() {
    let app = common::TestApp::new_with_rate_limited_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

// --- Cache Behavior ---

#[tokio::test]
async fn test_get_kanban_cache_hit() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    // First request - populates cache
    let resp1 = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp1.status(), StatusCode::OK);

    // Second request within TTL - cache hit
    let start = std::time::Instant::now();
    let resp2 = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    let elapsed = start.elapsed();
    assert_eq!(resp2.status(), StatusCode::OK);
    assert!(elapsed.as_millis() < 50); // Cache hit should be fast
}

#[tokio::test]
async fn test_get_kanban_user_no_git_token_configured() {
    let app = common::TestApp::new_with_mock_git_server().await;

    // Create project but do NOT configure git token
    let project_id = app.create_project(
        "https://gitlab.example.com/group/repo",
        &app.admin_token,
    ).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/kanban", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_004");
}
```

### 5.2 POST /api/projects/:id/issues

Creates a new issue on the remote Git platform.

```rust
// tests/api/api_issues.rs
mod common;

use reqwest::StatusCode;
use serde_json::json;

// --- Happy Path ---

#[tokio::test]
async fn test_create_issue_success() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({
                "title": "New feature: dark mode",
                "description": "Implement dark mode for the application",
                "labels": ["enhancement", "ui"]
            }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert!(body["data"]["iid"].as_u64().unwrap() > 0);
    assert_eq!(body["data"]["title"], "New feature: dark mode");
    assert!(body["data"]["web_url"].as_str().is_some());
}

#[tokio::test]
async fn test_create_issue_minimal_fields() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "title": "Bug fix needed" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["title"], "Bug fix needed");
}

// --- Authentication Failures ---

#[tokio::test]
async fn test_create_issue_no_auth() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "title": "test" }),
            None,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// --- Authorization Failures ---

#[tokio::test]
async fn test_create_issue_non_member() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "title": "Unauthorized issue" }),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "AUTH_002");
}

// --- Invalid Parameters ---

#[tokio::test]
async fn test_create_issue_empty_title() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "title": "" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_001");
}

#[tokio::test]
async fn test_create_issue_missing_title() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "description": "No title" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_issue_title_too_long() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let long_title = "x".repeat(1000);
    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "title": long_title }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- External Service Failures ---

#[tokio::test]
async fn test_create_issue_gitlab_api_down() {
    let app = common::TestApp::new_with_failing_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "title": "Will fail" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn test_create_issue_git_token_expired() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_invalid_git_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues", project_id),
            &json!({ "title": "Token expired" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "EXT_001");
}
```

### 5.3 POST /api/projects/:id/issues/ai-generate

AI-assisted issue content generation via SSE streaming.

```rust
// tests/api/api_issue_ai.rs
mod common;

use reqwest::StatusCode;
use serde_json::json;
use futures::StreamExt;

// --- Happy Path ---

#[tokio::test]
async fn test_ai_generate_issue_success_sse() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app.client
        .post(app.url(&format!("/api/projects/{}/issues/ai-generate", project_id)))
        .header("Authorization", format!("Bearer {}", app.admin_token))
        .header("Accept", "text/event-stream")
        .json(&json!({
            "prompt": "Create an issue for implementing user authentication with OAuth2"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );

    // Read SSE stream
    let mut stream = resp.bytes_stream();
    let mut full_content = String::new();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.unwrap();
        let text = String::from_utf8_lossy(&bytes);
        if text.contains("data: [DONE]") {
            break;
        }
        if text.starts_with("data: ") {
            let data: serde_json::Value = serde_json::from_str(&text[6..]).unwrap_or_default();
            if let Some(content) = data["content"].as_str() {
                full_content.push_str(content);
            }
        }
    }

    assert!(!full_content.is_empty());
    // AI should generate structured issue content
    assert!(full_content.contains('#') || full_content.len() > 20);
}

// --- Authentication Failures ---

#[tokio::test]
async fn test_ai_generate_no_auth() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": "test" }),
            None,
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// --- Authorization Failures ---

#[tokio::test]
async fn test_ai_generate_non_member() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": "test" }),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// --- Invalid Parameters ---

#[tokio::test]
async fn test_ai_generate_empty_prompt() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": "" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "BIZ_001");
}

#[tokio::test]
async fn test_ai_generate_missing_prompt() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({}),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_ai_generate_prompt_too_long() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    let long_prompt = "x".repeat(10000);
    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": long_prompt }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- Rate Limiting ---

#[tokio::test]
async fn test_ai_generate_rate_limited() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    // Send multiple rapid requests to trigger rate limit
    let mut last_status = StatusCode::OK;
    for _ in 0..20 {
        let resp = app
            .post(
                &format!("/api/projects/{}/issues/ai-generate", project_id),
                &json!({ "prompt": "test prompt" }),
                Some(&app.admin_token),
            )
            .await;
        last_status = resp.status();
        if last_status == StatusCode::TOO_MANY_REQUESTS {
            break;
        }
    }
    assert_eq!(last_status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_ai_generate_rate_limit_per_user() {
    let app = common::TestApp::new_with_mock_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    // Create second user as member
    app.create_test_user("user2", "Pass123456", "user").await;
    let user2_id = app.get_user_id("user2").await;
    app.add_project_member(project_id, user2_id, "member", &app.admin_token).await;
    let user2_token = app.login_get_token("user2", "Pass123456").await;

    // Exhaust rate limit for admin
    for _ in 0..20 {
        app.post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": "test" }),
            Some(&app.admin_token),
        ).await;
    }

    // user2 should still be able to make requests (per-user limit)
    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": "test from user2" }),
            Some(&user2_token),
        )
        .await;
    assert_ne!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

// --- External Service Failures ---

#[tokio::test]
async fn test_ai_generate_ai_service_down() {
    let app = common::TestApp::new_with_failing_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": "test" }),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "EXT_002"); // AI service unavailable
}

#[tokio::test]
async fn test_ai_generate_ai_service_timeout() {
    let app = common::TestApp::new_with_slow_ai_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .post(
            &format!("/api/projects/{}/issues/ai-generate", project_id),
            &json!({ "prompt": "test" }),
            Some(&app.admin_token),
        )
        .await;
    // Should timeout gracefully
    assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
}
```

### 5.4 GET /api/projects/:id/issues/:iid

Fetches detailed information about a single issue.

```rust
// tests/api/api_issues.rs (get issue detail section)

// --- Happy Path ---

#[tokio::test]
async fn test_get_issue_detail_success() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["iid"], 1);
    assert!(body["data"]["title"].is_string());
    assert!(body["data"]["description"].is_string());
    assert!(body["data"]["state"].is_string());
    assert!(body["data"]["author"].is_object());
    assert!(body["data"]["labels"].is_array());
    assert!(body["data"]["created_at"].is_string());
    assert!(body["data"]["updated_at"].is_string());
    assert!(body["data"]["web_url"].is_string());
}

// --- Authentication Failures ---

#[tokio::test]
async fn test_get_issue_detail_no_auth() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(&format!("/api/projects/{}/issues/1", project_id), None)
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// --- Authorization Failures ---

#[tokio::test]
async fn test_get_issue_detail_non_member() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1", project_id),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// --- Invalid Parameters ---

#[tokio::test]
async fn test_get_issue_detail_not_found() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/99999", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_issue_detail_invalid_iid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/abc", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_issue_detail_zero_iid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/0", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- External Service Failures ---

#[tokio::test]
async fn test_get_issue_detail_gitlab_down() {
    let app = common::TestApp::new_with_failing_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn test_get_issue_detail_git_token_invalid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_invalid_git_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "EXT_001");
}
```

### 5.5 GET /api/projects/:id/issues/:iid/mrs

Fetches merge requests/pull requests associated with a specific issue.

```rust
// tests/api/api_issue_mrs.rs
mod common;

use reqwest::StatusCode;
use serde_json::json;

// --- Happy Path ---

#[tokio::test]
async fn test_get_issue_mrs_success() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    // Issue #1 has associated MRs in mock
    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1/mrs", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert!(body["data"].is_array());

    let mrs = body["data"].as_array().unwrap();
    if !mrs.is_empty() {
        let mr = &mrs[0];
        assert!(mr["iid"].is_number());
        assert!(mr["title"].is_string());
        assert!(mr["state"].is_string());
        assert!(mr["author"].is_object());
        assert!(mr["source_branch"].is_string());
        assert!(mr["target_branch"].is_string());
        assert!(mr["web_url"].is_string());
    }
}

#[tokio::test]
async fn test_get_issue_mrs_empty() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    // Issue #999 has no associated MRs in mock
    let resp = app
        .get(
            &format!("/api/projects/{}/issues/999/mrs", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"].as_array().unwrap().len(), 0);
}

// --- Authentication Failures ---

#[tokio::test]
async fn test_get_issue_mrs_no_auth() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(&format!("/api/projects/{}/issues/1/mrs", project_id), None)
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// --- Authorization Failures ---

#[tokio::test]
async fn test_get_issue_mrs_non_member() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1/mrs", project_id),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// --- Invalid Parameters ---

#[tokio::test]
async fn test_get_issue_mrs_invalid_issue_iid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/abc/mrs", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_issue_mrs_project_not_found() {
    let app = common::TestApp::new_with_mock_git_server().await;

    let resp = app
        .get("/api/projects/99999/issues/1/mrs", Some(&app.admin_token))
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- External Service Failures ---

#[tokio::test]
async fn test_get_issue_mrs_gitlab_down() {
    let app = common::TestApp::new_with_failing_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1/mrs", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn test_get_issue_mrs_git_token_invalid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_invalid_git_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/issues/1/mrs", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
```

### 5.6 GET /api/projects/:id/mrs/:iid

Fetches detailed information about a merge request/pull request, including associated issues.

```rust
// tests/api/api_mr_detail.rs
mod common;

use reqwest::StatusCode;
use serde_json::json;

// --- Happy Path ---

#[tokio::test]
async fn test_get_mr_detail_success() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/mrs/10", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["data"]["iid"], 10);
    assert!(body["data"]["title"].is_string());
    assert!(body["data"]["state"].is_string());
    assert!(body["data"]["author"].is_object());
    assert!(body["data"]["source_branch"].is_string());
    assert!(body["data"]["target_branch"].is_string());
    assert!(body["data"]["web_url"].is_string());
    // Should include related issues
    assert!(body["data"]["related_issues"].is_array());
}

#[tokio::test]
async fn test_get_mr_detail_with_ci_status() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/mrs/10", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = resp.json().await.unwrap();
    // CI/pipeline status should be included if available
    if body["data"]["pipeline"].is_object() {
        assert!(body["data"]["pipeline"]["status"].is_string());
    }
}

// --- Authentication Failures ---

#[tokio::test]
async fn test_get_mr_detail_no_auth() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(&format!("/api/projects/{}/mrs/10", project_id), None)
        .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// --- Authorization Failures ---

#[tokio::test]
async fn test_get_mr_detail_non_member() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    app.create_test_user("outsider", "Pass123456", "user").await;
    let outsider_token = app.login_get_token("outsider", "Pass123456").await;

    let resp = app
        .get(
            &format!("/api/projects/{}/mrs/10", project_id),
            Some(&outsider_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// --- Invalid Parameters ---

#[tokio::test]
async fn test_get_mr_detail_not_found() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/mrs/99999", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_mr_detail_invalid_iid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/mrs/abc", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// --- External Service Failures ---

#[tokio::test]
async fn test_get_mr_detail_gitlab_down() {
    let app = common::TestApp::new_with_failing_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/mrs/10", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn test_get_mr_detail_git_token_invalid() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_invalid_git_token(&app).await;

    let resp = app
        .get(
            &format!("/api/projects/{}/mrs/10", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["retCode"], "EXT_001");
}

// --- Cache Behavior ---

#[tokio::test]
async fn test_get_mr_detail_cache_behavior() {
    let app = common::TestApp::new_with_mock_git_server().await;
    let project_id = setup_project_with_token(&app).await;

    // First request
    let resp1 = app
        .get(
            &format!("/api/projects/{}/mrs/10", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp1.status(), StatusCode::OK);

    // Second request should be cached
    let start = std::time::Instant::now();
    let resp2 = app
        .get(
            &format!("/api/projects/{}/mrs/10", project_id),
            Some(&app.admin_token),
        )
        .await;
    assert_eq!(resp2.status(), StatusCode::OK);
    assert!(start.elapsed().as_millis() < 50);
}
```

---

## 6. CI/CD Integration

### 6.1 GitLab CI Pipeline Configuration

```yaml
# .gitlab-ci.yml
stages:
  - lint
  - unit-test
  - integration-test
  - api-test
  - e2e-test
  - coverage-report

variables:
  CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  RUSTFLAGS: "-D warnings"
  # Test environment
  GITLAB_TOKEN: ${GITLAB_E2E_TOKEN}
  GITLAB_HOST: "http://gitlab.jushuitan-inc.com:8081"
  TEST_NAMESPACE: "zimei10525"
  TEST_REPO: "symphony_e2e_test_repo"
  # AI service (mock in CI, real in nightly)
  AI_SERVICE_URL: "http://localhost:8090"
  AI_API_KEY: "test-key"

cache:
  key: ${CI_COMMIT_REF_SLUG}
  paths:
    - .cargo/
    - target/

# --- Stage 1: Lint ---
lint:
  stage: lint
  script:
    - rustup component add clippy rustfmt
    - cargo fmt --check
    - cargo clippy --all-targets --all-features -- -D warnings
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
    - if: $CI_COMMIT_BRANCH == "main"

# --- Stage 2: Unit Tests ---
unit-tests:
  stage: unit-test
  script:
    - cargo test --lib --all-features -- --nocapture
    - cargo test --test unit -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
    - if: $CI_COMMIT_BRANCH == "main"

# --- Stage 3: Integration Tests ---
integration-tests:
  stage: integration-test
  services:
    - name: wiremock/wiremock:latest
      alias: mock-server
  script:
    - cargo test --test integration -- --nocapture
  variables:
    MOCK_SERVER_URL: "http://mock-server:8080"
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
    - if: $CI_COMMIT_BRANCH == "main"

# --- Stage 4: API Tests ---
api-tests:
  stage: api-test
  script:
    - cargo test --test api -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
    - if: $CI_COMMIT_BRANCH == "main"

# --- Stage 5: E2E Tests ---
e2e-tests-mock:
  stage: e2e-test
  script:
    - cargo test --test e2e -- --nocapture
    - cargo test --test kanban_lifecycle -- --nocapture
    - cargo test --test multi_user_kanban -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
    - if: $CI_COMMIT_BRANCH == "main"

e2e-tests-real-api:
  stage: e2e-test
  script:
    - cargo test --test external_api_e2e -- --ignored --nocapture
  variables:
    GITLAB_TOKEN: ${GITLAB_E2E_TOKEN}
  rules:
    # Only run on main branch or nightly schedule
    - if: $CI_COMMIT_BRANCH == "main"
    - if: $CI_PIPELINE_SOURCE == "schedule"
  allow_failure: true  # External API tests may flake

# --- Stage 6: Coverage ---
coverage:
  stage: coverage-report
  script:
    - cargo install cargo-tarpaulin || true
    - cargo tarpaulin --out xml --output-dir coverage/ --skip-clean
      --exclude-files "tests/*" "src/main.rs"
      --all-features
  coverage: '/^\d+.\d+% coverage/'
  artifacts:
    paths:
      - coverage/
    reports:
      coverage_report:
        coverage_format: cobertura
        path: coverage/cobertura.xml
  rules:
    - if: $CI_COMMIT_BRANCH == "main"
    - if: $CI_PIPELINE_SOURCE == "schedule"
```

### 6.2 Test Stages and Dependencies

```
┌─────────┐    ┌────────────┐    ┌──────────────────┐    ┌───────────┐    ┌──────────┐    ┌──────────┐
│  Lint   │───>│ Unit Tests │───>│ Integration Tests│───>│ API Tests │───>│ E2E Tests│───>│ Coverage │
└─────────┘    └────────────┘    └──────────────────┘    └───────────┘    └──────────┘    └──────────┘
   ~30s            ~60s                ~90s                   ~120s           ~180s           ~120s
```

| Stage | Duration | Failure Impact | Retry Policy |
|-------|----------|---------------|--------------|
| Lint | ~30s | Blocks all | No retry |
| Unit Tests | ~60s | Blocks integration+ | No retry |
| Integration Tests | ~90s | Blocks API+ | 1 retry |
| API Tests | ~120s | Blocks E2E | 1 retry |
| E2E Tests (mock) | ~180s | Blocks coverage | 1 retry |
| E2E Tests (real API) | ~300s | Non-blocking | 2 retries, allow_failure |
| Coverage | ~120s | Non-blocking | No retry |

### 6.3 Artifact Collection

```yaml
# Test results are collected as JUnit XML for GitLab integration
artifacts:
  when: always
  paths:
    - target/nextest/ci/junit.xml
    - coverage/
  reports:
    junit: target/nextest/ci/junit.xml
    coverage_report:
      coverage_format: cobertura
      path: coverage/cobertura.xml
```

### 6.4 Coverage Reporting

Coverage thresholds enforced in CI:

| Module | Minimum Coverage | Target |
|--------|-----------------|--------|
| `git_client/` | 85% | 90% |
| `handlers/kanban.rs` | 80% | 85% |
| `cache/` | 90% | 95% |
| `ai/` | 75% | 80% |
| Overall | 80% | 85% |

```yaml
# Coverage gate in CI
coverage-gate:
  stage: coverage-report
  script:
    - |
      COVERAGE=$(cargo tarpaulin --print-rust-flags 2>&1 | grep -oP '\d+\.\d+(?=% coverage)')
      if (( $(echo "$COVERAGE < 80.0" | bc -l) )); then
        echo "Coverage $COVERAGE% is below 80% threshold"
        exit 1
      fi
  rules:
    - if: $CI_PIPELINE_SOURCE == "merge_request_event"
```

---

## 7. Test Infrastructure

### 7.1 Test Fixtures and Factories

```rust
// tests/common/fixtures.rs
use serde_json::json;

/// Factory for creating mock kanban issues
pub fn mock_kanban_issue(iid: u64) -> serde_json::Value {
    json!({
        "iid": iid,
        "title": format!("Test Issue #{}", iid),
        "state": "opened",
        "labels": ["bug"],
        "author": {
            "username": "testuser",
            "avatar_url": "https://example.com/avatar.png"
        },
        "created_at": "2026-05-01T10:00:00Z",
        "updated_at": "2026-05-20T10:00:00Z",
        "web_url": format!("https://gitlab.example.com/group/repo/-/issues/{}", iid)
    })
}

/// Factory for creating mock merge requests
pub fn mock_merge_request(iid: u64) -> serde_json::Value {
    json!({
        "iid": iid,
        "title": format!("MR !{}: Fix issue", iid),
        "state": "opened",
        "author": {
            "username": "developer",
            "avatar_url": "https://example.com/dev-avatar.png"
        },
        "source_branch": format!("fix/issue-{}", iid),
        "target_branch": "main",
        "web_url": format!("https://gitlab.example.com/group/repo/-/merge_requests/{}", iid),
        "pipeline": {
            "status": "success",
            "web_url": "https://gitlab.example.com/group/repo/-/pipelines/123"
        }
    })
}

/// Factory for creating mock MR detail with related issues
pub fn mock_mr_detail(iid: u64, related_issue_iids: &[u64]) -> serde_json::Value {
    let related_issues: Vec<_> = related_issue_iids
        .iter()
        .map(|&issue_iid| json!({
            "iid": issue_iid,
            "title": format!("Related Issue #{}", issue_iid),
            "state": "opened"
        }))
        .collect();

    json!({
        "iid": iid,
        "title": format!("MR !{}: Implementation", iid),
        "description": "Implements the feature as described in the related issues.",
        "state": "opened",
        "author": {
            "username": "developer",
            "avatar_url": "https://example.com/dev-avatar.png"
        },
        "source_branch": "feature/implementation",
        "target_branch": "main",
        "web_url": format!("https://gitlab.example.com/group/repo/-/merge_requests/{}", iid),
        "pipeline": {
            "status": "success",
            "web_url": "https://gitlab.example.com/group/repo/-/pipelines/456"
        },
        "related_issues": related_issues,
        "changes_count": 15,
        "diff_stats": {
            "additions": 120,
            "deletions": 30
        }
    })
}

/// Factory for AI SSE response chunks
pub fn mock_ai_sse_response(content: &str) -> String {
    let chunks: Vec<&str> = content.split_whitespace().collect();
    let mut sse = String::new();
    for chunk in chunks {
        sse.push_str(&format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
            chunk
        ));
    }
    sse.push_str("data: [DONE]\n\n");
    sse
}
```

### 7.2 Mock Server Setup for GitLab/GitHub APIs

```rust
// tests/common/mock_gitlab.rs
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path_regex, header, query_param};
use serde_json::json;

pub struct MockGitLabServer {
    pub server: MockServer,
    pub url: String,
}

impl MockGitLabServer {
    pub async fn new() -> Self {
        let server = MockServer::start().await;
        let url = server.uri();
        Self { server, url }
    }

    /// Setup standard kanban-related mocks
    pub async fn setup_kanban_mocks(&self) {
        // List issues (pending - no symphony-claimed label)
        Mock::given(method("GET"))
            .and(path_regex(r"/api/v4/projects/.+/issues"))
            .and(query_param("state", "opened"))
            .and(header_exists("PRIVATE-TOKEN"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                fixtures::mock_kanban_issue(1),
                fixtures::mock_kanban_issue(2),
                fixtures::mock_kanban_issue(3),
            ])))
            .mount(&self.server)
            .await;

        // List issues (in_progress - with symphony-claimed label)
        Mock::given(method("GET"))
            .and(path_regex(r"/api/v4/projects/.+/issues"))
            .and(query_param("labels", "symphony-claimed"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                fixtures::mock_kanban_issue(4),
            ])))
            .mount(&self.server)
            .await;

        // Get single issue detail
        Mock::given(method("GET"))
            .and(path_regex(r"/api/v4/projects/.+/issues/\d+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "iid": 1,
                "title": "Test Issue #1",
                "description": "Detailed description of the issue",
                "state": "opened",
                "labels": ["bug"],
                "author": {"username": "testuser", "avatar_url": "https://..."},
                "created_at": "2026-05-01T10:00:00Z",
                "updated_at": "2026-05-20T10:00:00Z",
                "web_url": "https://gitlab.example.com/group/repo/-/issues/1"
            })))
            .mount(&self.server)
            .await;

        // Get issue related MRs
        Mock::given(method("GET"))
            .and(path_regex(r"/api/v4/projects/.+/issues/\d+/related_merge_requests"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                fixtures::mock_merge_request(10),
            ])))
            .mount(&self.server)
            .await;

        // Get MR detail
        Mock::given(method("GET"))
            .and(path_regex(r"/api/v4/projects/.+/merge_requests/\d+$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                fixtures::mock_mr_detail(10, &[1, 2])
            ))
            .mount(&self.server)
            .await;

        // Create issue
        Mock::given(method("POST"))
            .and(path_regex(r"/api/v4/projects/.+/issues"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "iid": 100,
                "title": "Created Issue",
                "state": "opened",
                "author": {"username": "admin"},
                "web_url": "https://gitlab.example.com/group/repo/-/issues/100"
            })))
            .mount(&self.server)
            .await;
    }

    /// Setup mocks that simulate API failures
    pub async fn setup_failure_mocks(&self) {
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "message": "Internal Server Error"
            })))
            .mount(&self.server)
            .await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&self.server)
            .await;
    }

    /// Setup mocks that simulate rate limiting
    pub async fn setup_rate_limit_mocks(&self) {
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("RateLimit-Remaining", "0")
                    .insert_header("RateLimit-Reset", "1716300000")
                    .set_body_json(json!({"message": "429 Too Many Requests"}))
            )
            .mount(&self.server)
            .await;
    }

    /// Setup mocks that simulate unauthorized (invalid token)
    pub async fn setup_unauthorized_mocks(&self) {
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401).set_body_json(json!({
                "message": "401 Unauthorized"
            })))
            .mount(&self.server)
            .await;
    }
}
```

### 7.3 Extended TestApp for Phase 3

```rust
// tests/common/mod.rs (additions for Phase 3)

impl TestApp {
    /// Create TestApp with a mock GitLab server for kanban tests
    pub async fn new_with_mock_git_server() -> Self {
        let mock_gitlab = MockGitLabServer::new().await;
        mock_gitlab.setup_kanban_mocks().await;

        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = init_pool(db_path.to_str().unwrap());
        let repo = SqliteRepository::new(pool);

        let admin_hash = hash_password("admin123").unwrap();
        repo.create_user("admin", &admin_hash, Some("Administrator"), "admin")
            .await
            .unwrap();

        let state = AppState {
            repo: repo.clone(),
            jwt_secret: "test-jwt-secret-key-at-least-32-characters-long".to_string(),
            encryption_key: [0x42u8; 32],
            token_blacklist: Arc::new(DashMap::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            process_manager: ProcessManager::new(),
            // Phase 3: inject mock git server URL
            git_api_base_url: Some(mock_gitlab.url.clone()),
        };

        // ... (same server setup as new())

        Self {
            addr: base_url,
            client,
            admin_token,
            _dir: dir,
            mock_gitlab_url: mock_gitlab.url,
            _mock_gitlab: mock_gitlab, // keep alive
        }
    }

    /// Create TestApp with a failing git server (simulates API down)
    pub async fn new_with_failing_git_server() -> Self {
        let mock_gitlab = MockGitLabServer::new().await;
        mock_gitlab.setup_failure_mocks().await;
        // ... same as above but with failure mocks
        todo!()
    }

    /// Create TestApp with rate-limited git server
    pub async fn new_with_rate_limited_git_server() -> Self {
        let mock_gitlab = MockGitLabServer::new().await;
        mock_gitlab.setup_rate_limit_mocks().await;
        // ... same as above but with rate limit mocks
        todo!()
    }

    /// Create TestApp with mock AI server
    pub async fn new_with_mock_ai_server() -> Self {
        // Setup both mock git server and mock AI server
        todo!()
    }
}

/// Helper: setup a project with a valid git token configured
async fn setup_project_with_token(app: &TestApp) -> i64 {
    let project_id = app.create_project(
        "https://gitlab.example.com/group/repo",
        &app.admin_token,
    ).await;

    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": "gitlab-token-example",
            "gitlabHost": &app.mock_gitlab_url
        }),
        Some(&app.admin_token),
    ).await;

    project_id
}

/// Helper: setup a project with an invalid git token
async fn setup_project_with_invalid_git_token(app: &TestApp) -> i64 {
    let project_id = app.create_project(
        "https://gitlab.example.com/group/repo",
        &app.admin_token,
    ).await;

    app.put(
        "/api/user/config",
        &json!({
            "gitlabToken": "gitlab-token-example",
            "gitlabHost": &app.mock_gitlab_url
        }),
        Some(&app.admin_token),
    ).await;

    project_id
}
```

### 7.4 Environment Configuration

```rust
// tests/common/config.rs

/// Test environment configuration
pub struct TestConfig {
    /// Base URL for mock GitLab server (integration/API tests)
    pub mock_gitlab_url: Option<String>,
    /// Base URL for mock GitHub server (integration/API tests)
    pub mock_github_url: Option<String>,
    /// Base URL for mock AI server
    pub mock_ai_url: Option<String>,
    /// Real GitLab token for E2E tests (from env)
    pub real_gitlab_token: Option<String>,
    /// Real GitLab host for E2E tests
    pub real_gitlab_host: Option<String>,
    /// Cache TTL for tests (shorter than production)
    pub cache_ttl_ms: u64,
    /// AI rate limit for tests (lower than production)
    pub ai_rate_limit_per_minute: u32,
}

impl TestConfig {
    pub fn from_env() -> Self {
        Self {
            mock_gitlab_url: None, // Set by test setup
            mock_github_url: None,
            mock_ai_url: None,
            real_gitlab_token: std::env::var("GITLAB_TOKEN").ok(),
            real_gitlab_host: std::env::var("GITLAB_HOST").ok(),
            cache_ttl_ms: 100, // 100ms for fast test execution
            ai_rate_limit_per_minute: 5, // Low limit for rate limit testing
        }
    }

    pub fn has_real_gitlab(&self) -> bool {
        self.real_gitlab_token.is_some() && self.real_gitlab_host.is_some()
    }
}
```

### 7.5 Test Database Management

```rust
// Each test gets an isolated SQLite database via TempDir
// This is already the pattern used in existing tests.
// Key principles:
//
// 1. Each test creates its own TempDir with a fresh database
// 2. Database migrations run automatically on init_pool()
// 3. No shared state between tests (full isolation)
// 4. TempDir cleanup is automatic when TestApp is dropped
//
// For Phase 3, the database is primarily used for:
// - User authentication and token storage
// - Project metadata (git URL, platform, namespace)
// - Member relationships (for authorization checks)
// - User config (encrypted git tokens)
//
// Kanban data itself is NOT stored in the database (fetched from external APIs).
```

### 7.6 New Dependencies for Testing

```toml
# Cargo.toml [dev-dependencies] additions for Phase 3
[dev-dependencies]
tokio-test = "0.4"
tempfile = "3"
reqwest = { version = "0.12", features = ["json", "stream"] }
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
wiremock = "0.6"           # HTTP mock server
mockall = "0.13"           # Trait mocking
futures = "0.3"            # Stream utilities for SSE testing
tokio-stream = "0.1"      # Stream adapters
assert_json_diff = "2"    # JSON comparison in assertions
```

---

## 8. Error Code Reference

Phase 3 introduces new error codes for external service interactions:

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `AUTH_001` | 401 | Missing or invalid JWT token |
| `AUTH_002` | 403 | Insufficient permissions (non-member, non-owner) |
| `BIZ_001` | 400 | Invalid request parameters (empty title, etc.) |
| `BIZ_002` | 404 | Resource not found (project, issue) |
| `BIZ_003` | 409 | Conflict (duplicate, running service) |
| `BIZ_004` | 422 | Git token not configured for user |
| `EXT_001` | 422 | External API authentication failure (invalid git token) |
| `EXT_002` | 502 | External AI service unavailable |
| `EXT_003` | 502 | External Git API unavailable |
| `EXT_004` | 429 | External API rate limit exceeded |
| `EXT_005` | 504 | External service timeout |

---

## 9. Test Execution Commands

```bash
# Run all tests
cargo test --all-features

# Run only unit tests
cargo test --lib
cargo test --test unit

# Run only integration tests
cargo test --test integration

# Run only API tests (Phase 3 kanban)
cargo test --test api_kanban
cargo test --test api_issues
cargo test --test api_issue_ai
cargo test --test api_issue_mrs
cargo test --test api_mr_detail

# Run E2E tests (mock servers)
cargo test --test kanban_lifecycle
cargo test --test multi_user_kanban

# Run E2E tests with real APIs (requires env vars)
GITLAB_TOKEN=<your-gitlab-token> cargo test --test external_api_e2e -- --ignored

# Run with coverage
cargo tarpaulin --out html --skip-clean

# Run specific test by name
cargo test test_get_kanban_success -- --nocapture

# Run tests matching a pattern
cargo test kanban -- --nocapture
cargo test ai_generate -- --nocapture
```

---

## 10. Summary: Test Case Count by Endpoint

| Endpoint | Happy Path | Auth | Authz | Params | External | Cache/Rate | Total |
|----------|-----------|------|-------|--------|----------|------------|-------|
| `GET /kanban` | 2 | 2 | 1 | 2 | 3 | 2 | **12** |
| `POST /issues` | 2 | 1 | 1 | 3 | 2 | 0 | **9** |
| `POST /issues/ai-generate` | 1 | 1 | 1 | 3 | 2 | 2 | **10** |
| `GET /issues/:iid` | 1 | 1 | 1 | 3 | 2 | 0 | **8** |
| `GET /issues/:iid/mrs` | 2 | 1 | 1 | 2 | 2 | 0 | **8** |
| `GET /mrs/:iid` | 2 | 1 | 1 | 2 | 2 | 1 | **9** |
| **Total** | **10** | **7** | **6** | **15** | **13** | **5** | **56** |

Additional tests:
- Unit tests: ~25 test cases
- Integration tests: ~20 test cases
- E2E tests: ~15 test cases
- **Grand total: ~116 test cases for Phase 3**
