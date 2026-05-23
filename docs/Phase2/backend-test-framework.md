# Phase 2 Backend Test Framework - Complete Design

## 1. Test Directory Structure

```
web-platform/
├── src/
│   └── ... (production code)
├── tests/
│   ├── common/
│   │   ├── mod.rs              # TestApp + shared helpers
│   │   ├── fixtures.rs         # Test data factories
│   │   └── mock_process.rs     # Mock subprocess spawner
│   ├── unit/
│   │   ├── git_url_parser.rs   # Git URL parsing unit tests
│   │   ├── workflow_template.rs # Template rendering tests
│   │   ├── pid_verify.rs       # PID verification logic tests
│   │   └── service_status.rs   # State machine transition tests
│   ├── integration/
│   │   ├── repo_projects.rs    # Project repository CRUD
│   │   ├── repo_members.rs     # Member repository CRUD
│   │   ├── auth_project.rs     # Project-level auth middleware
│   │   └── process_manager.rs  # Process manager with mocks
│   ├── api/
│   │   ├── api_projects.rs     # Project CRUD endpoints
│   │   ├── api_members.rs      # Member management endpoints
│   │   ├── api_service.rs      # Service control endpoints
│   │   └── api_workflow.rs     # Workflow template endpoints
│   ├── e2e/
│   │   ├── project_lifecycle.rs # Full project lifecycle
│   │   ├── member_flow.rs      # Member management flow
│   │   └── concurrent_ops.rs   # Concurrent operation tests
│   ├── api_auth.rs             # (existing) Auth API tests
│   ├── api_admin_users.rs      # (existing) Admin user tests
│   ├── api_user_profile.rs     # (existing) Profile tests
│   └── e2e.rs                  # (existing) E2E tests
```

---

## 2. Test Infrastructure Code

### 2.1 Extended TestApp (tests/common/mod.rs)

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use reqwest::{Client, Response};
use serde::Serialize;
use tempfile::TempDir;
use tokio::net::TcpListener;

use web_platform::auth::password::hash_password;
use web_platform::auth::rate_limit::RateLimiter;
use web_platform::db::init_pool;
use web_platform::repository::{SqliteRepository, UserRepository};
use web_platform::router::create_router;
use web_platform::AppState;

pub mod fixtures;
pub mod mock_process;

pub struct TestApp {
    pub addr: String,
    pub client: Client,
    pub admin_token: String,
    pub repo: SqliteRepository,
    _dir: TempDir,
}

impl TestApp {
    pub async fn new() -> Self {
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
        };

        let app = create_router(state).into_make_service_with_connect_info::<SocketAddr>();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = Client::new();

        let login_resp = client
            .post(format!("{}/api/auth/login", base_url))
            .json(&serde_json::json!({
                "username": "admin",
                "password": "admin123"
            }))
            .send()
            .await
            .unwrap();

        let login_body: serde_json::Value = login_resp.json().await.unwrap();
        let admin_token = login_body["data"]["token"].as_str().unwrap().to_string();

        Self {
            addr: base_url,
            client,
            admin_token,
            repo,
            _dir: dir,
        }
    }

    // --- HTTP helpers ---

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.addr, path)
    }

    pub async fn get(&self, path: &str, token: Option<&str>) -> Response {
        let mut req = self.client.get(self.url(path));
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }
        req.send().await.unwrap()
    }

    pub async fn post<T: Serialize>(&self, path: &str, body: &T, token: Option<&str>) -> Response {
        let mut req = self.client.post(self.url(path)).json(body);
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }
        req.send().await.unwrap()
    }

    pub async fn put<T: Serialize>(&self, path: &str, body: &T, token: Option<&str>) -> Response {
        let mut req = self.client.put(self.url(path)).json(body);
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }
        req.send().await.unwrap()
    }

    pub async fn delete(&self, path: &str, token: Option<&str>) -> Response {
        let mut req = self.client.delete(self.url(path));
        if let Some(t) = token {
            req = req.header("Authorization", format!("Bearer {}", t));
        }
        req.send().await.unwrap()
    }

    // --- Auth helpers ---

    pub async fn login(&self, username: &str, password: &str) -> Response {
        self.post(
            "/api/auth/login",
            &serde_json::json!({
                "username": username,
                "password": password
            }),
            None,
        )
        .await
    }

    pub async fn login_get_token(&self, username: &str, password: &str) -> String {
        let resp = self.login(username, password).await;
        let body: serde_json::Value = resp.json().await.unwrap();
        body["data"]["token"].as_str().unwrap().to_string()
    }

    pub async fn create_test_user(&self, username: &str, password: &str, role: &str) {
        self.post(
            "/api/admin/users",
            &serde_json::json!({
                "username": username,
                "password": password,
                "role": role
            }),
            Some(&self.admin_token),
        )
        .await;
    }

    /// Create a user and return their token
    pub async fn create_user_get_token(&self, username: &str, password: &str, role: &str) -> String {
        self.create_test_user(username, password, role).await;
        self.login_get_token(username, password).await
    }

    /// Get user ID by username (via admin list endpoint)
    pub async fn get_user_id(&self, username: &str) -> i64 {
        let resp = self
            .get(
                &format!("/api/admin/users?search={}", username),
                Some(&self.admin_token),
            )
            .await;
        let body: serde_json::Value = resp.json().await.unwrap();
        body["data"]["records"][0]["id"].as_i64().unwrap()
    }

    // --- Project helpers ---

    /// Create a project and return its ID
    pub async fn create_project(&self, git_url: &str, token: &str) -> i64 {
        let resp = self
            .post(
                "/api/projects",
                &serde_json::json!({
                    "git_url": git_url
                }),
                Some(token),
            )
            .await;
        let body: serde_json::Value = resp.json().await.unwrap();
        body["data"]["id"].as_i64().unwrap()
    }

    /// Create a project with full options
    pub async fn create_project_full(
        &self,
        git_url: &str,
        name: Option<&str>,
        description: Option<&str>,
        token: &str,
    ) -> Response {
        let mut payload = serde_json::json!({ "git_url": git_url });
        if let Some(n) = name {
            payload["name"] = serde_json::json!(n);
        }
        if let Some(d) = description {
            payload["description"] = serde_json::json!(d);
        }
        self.post("/api/projects", &payload, Some(token)).await
    }

    /// Add a member to a project
    pub async fn add_project_member(
        &self,
        project_id: i64,
        user_id: i64,
        role: &str,
        token: &str,
    ) -> Response {
        self.post(
            &format!("/api/projects/{}/members", project_id),
            &serde_json::json!({
                "user_id": user_id,
                "role": role
            }),
            Some(token),
        )
        .await
    }

    /// Generate an expired JWT token for testing
    pub fn generate_expired_token(&self) -> String {
        use chrono::{Duration, Utc};
        use jsonwebtoken::{encode, EncodingKey, Header};
        use web_platform::auth::jwt::Claims;

        let now = Utc::now();
        let claims = Claims {
            sub: "999".to_string(),
            username: "expired_user".to_string(),
            role: "user".to_string(),
            iat: (now - Duration::hours(2)).timestamp(),
            exp: (now - Duration::hours(1)).timestamp(),
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(
                "test-jwt-secret-key-at-least-32-characters-long".as_bytes(),
            ),
        )
        .unwrap()
    }

    /// Generate a token with a specific role (for permission testing)
    pub fn generate_token_for_role(&self, user_id: i64, username: &str, role: &str) -> String {
        use web_platform::auth::jwt::generate_token;
        let (token, _) = generate_token(
            user_id,
            username,
            role,
            "test-jwt-secret-key-at-least-32-characters-long",
        )
        .unwrap();
        token
    }
}
```

### 2.2 Test Data Fixtures (tests/common/fixtures.rs)

```rust
use serde_json::Value;

/// Standard Git URLs for testing
pub mod git_urls {
    pub const GITHUB_HTTPS: &str = "https://github.com/owner/my-repo";
    pub const GITHUB_SSH: &str = "git@github.com:owner/my-repo.git";
    pub const GITLAB_HTTPS: &str = "https://gitlab.com/group/project";
    pub const GITLAB_SSH: &str = "git@gitlab.com:group/project.git";
    pub const GITLAB_SUBGROUP: &str = "https://gitlab.com/group/sub/project";
    pub const GITLAB_CUSTOM: &str = "https://gitlab.example.com/team/repo";
    pub const GITLAB_CUSTOM_SSH: &str = "git@gitlab.example.com:team/repo.git";
    pub const GITLAB_WITH_DOT_GIT: &str = "https://gitlab.com/group/project.git";
    pub const GITLAB_TRAILING_SLASH: &str = "https://gitlab.com/group/project/";
    pub const INVALID_URL: &str = "not-a-valid-url";
    pub const EMPTY_URL: &str = "";
    pub const HTTP_ONLY: &str = "http://github.com/owner/repo";
    pub const MISSING_REPO: &str = "https://github.com/owner";
    pub const JUST_DOMAIN: &str = "https://github.com";
}

/// Create project request payloads
pub fn create_project_payload(git_url: &str) -> Value {
    serde_json::json!({
        "git_url": git_url
    })
}

pub fn create_project_full_payload(
    git_url: &str,
    name: &str,
    description: &str,
    branch: &str,
) -> Value {
    serde_json::json!({
        "git_url": git_url,
        "name": name,
        "description": description,
        "default_branch": branch
    })
}

pub fn update_project_payload(name: &str, description: &str) -> Value {
    serde_json::json!({
        "name": name,
        "description": description
    })
}

pub fn add_member_payload(user_id: i64, role: &str) -> Value {
    serde_json::json!({
        "user_id": user_id,
        "role": role
    })
}

pub fn update_member_role_payload(role: &str) -> Value {
    serde_json::json!({
        "role": role
    })
}

pub fn update_workflow_payload(content: &str) -> Value {
    serde_json::json!({
        "template_mode": "custom",
        "content": content
    })
}

/// Test user credentials
pub struct TestUser {
    pub username: &'static str,
    pub password: &'static str,
    pub role: &'static str,
}

pub const ADMIN_USER: TestUser = TestUser {
    username: "admin",
    password: "admin123",
    role: "admin",
};

pub const OWNER_USER: TestUser = TestUser {
    username: "project_owner",
    password: "OwnerPass123",
    role: "user",
};

pub const MEMBER_USER: TestUser = TestUser {
    username: "project_member",
    password: "MemberPass123",
    role: "user",
};

pub const NON_MEMBER_USER: TestUser = TestUser {
    username: "outsider",
    password: "OutsiderPass123",
    role: "user",
};
```

### 2.3 Mock Process Spawner (tests/common/mock_process.rs)

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Records of process operations for assertion
#[derive(Debug, Clone, Default)]
pub struct ProcessLog {
    pub spawned: Vec<SpawnRecord>,
    pub killed: Vec<KillRecord>,
}

#[derive(Debug, Clone)]
pub struct SpawnRecord {
    pub project_id: i64,
    pub command: String,
    pub env_vars: HashMap<String, String>,
    pub working_dir: String,
}

#[derive(Debug, Clone)]
pub struct KillRecord {
    pub pid: u32,
    pub signal: &'static str, // "SIGTERM" or "SIGKILL"
}

/// Mock process spawner that doesn't actually spawn processes
pub struct MockProcessSpawner {
    pub log: Arc<Mutex<ProcessLog>>,
    pub next_pid: Arc<Mutex<u32>>,
    /// If set, spawn will return this error
    pub spawn_error: Arc<Mutex<Option<String>>>,
    /// If set, the "process" will "exit" after this many health checks
    pub exit_after_checks: Arc<Mutex<Option<u32>>>,
}

impl MockProcessSpawner {
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(ProcessLog::default())),
            next_pid: Arc::new(Mutex::new(10000)),
            spawn_error: Arc::new(Mutex::new(None)),
            exit_after_checks: Arc::new(Mutex::new(None)),
        }
    }

    pub fn spawn(&self, project_id: i64, command: &str, env: HashMap<String, String>, cwd: &str) -> Result<u32, String> {
        if let Some(err) = self.spawn_error.lock().unwrap().as_ref() {
            return Err(err.clone());
        }

        let mut pid = self.next_pid.lock().unwrap();
        let current_pid = *pid;
        *pid += 1;

        self.log.lock().unwrap().spawned.push(SpawnRecord {
            project_id,
            command: command.to_string(),
            env_vars: env,
            working_dir: cwd.to_string(),
        });

        Ok(current_pid)
    }

    pub fn kill(&self, pid: u32, signal: &'static str) {
        self.log.lock().unwrap().killed.push(KillRecord { pid, signal });
    }

    pub fn get_log(&self) -> ProcessLog {
        self.log.lock().unwrap().clone()
    }

    pub fn set_spawn_error(&self, err: Option<String>) {
        *self.spawn_error.lock().unwrap() = err;
    }
}

/// Mock PID verifier - always returns the configured result
pub struct MockPidVerifier {
    pub valid_pids: Arc<Mutex<Vec<u32>>>,
}

impl MockPidVerifier {
    pub fn new() -> Self {
        Self {
            valid_pids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn set_valid(&self, pid: u32) {
        self.valid_pids.lock().unwrap().push(pid);
    }

    pub fn verify(&self, pid: u32) -> bool {
        self.valid_pids.lock().unwrap().contains(&pid)
    }
}
```

---

## 3. Complete Test Cases

### 3.1 Unit Tests

#### 3.1.1 Git URL Parser (tests/unit/git_url_parser.rs)

```rust
#[cfg(test)]
mod tests {
    use web_platform::git_url::{parse_git_url, Platform, ParsedGitUrl, GitUrlError};

    // --- HTTPS format ---

    #[test]
    fn parse_github_https_standard() {
        let result = parse_git_url("https://github.com/owner/my-repo").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.host, "github.com");
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "my-repo");
        assert_eq!(result.normalized_url, "https://github.com/owner/my-repo");
    }

    #[test]
    fn parse_gitlab_https_standard() {
        let result = parse_git_url("https://gitlab.com/group/project").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "gitlab.com");
        assert_eq!(result.namespace, "group");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn parse_gitlab_https_subgroup() {
        let result = parse_git_url("https://gitlab.com/group/sub/project").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.namespace, "group/sub");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn parse_gitlab_https_deep_subgroup() {
        let result = parse_git_url("https://gitlab.com/a/b/c/project").unwrap();
        assert_eq!(result.namespace, "a/b/c");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn parse_custom_gitlab_domain() {
        let result = parse_git_url("https://gitlab.example.com/team/repo").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "gitlab.example.com");
        assert_eq!(result.namespace, "team");
        assert_eq!(result.repo_name, "repo");
    }

    #[test]
    fn parse_custom_domain_defaults_to_gitlab() {
        let result = parse_git_url("https://git.mycompany.com/team/repo").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "git.mycompany.com");
    }

    // --- SSH format ---

    #[test]
    fn parse_github_ssh() {
        let result = parse_git_url("git@github.com:owner/my-repo.git").unwrap();
        assert_eq!(result.platform, Platform::GitHub);
        assert_eq!(result.host, "github.com");
        assert_eq!(result.namespace, "owner");
        assert_eq!(result.repo_name, "my-repo");
    }

    #[test]
    fn parse_gitlab_ssh() {
        let result = parse_git_url("git@gitlab.com:group/project.git").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.namespace, "group");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn parse_gitlab_ssh_subgroup() {
        let result = parse_git_url("git@gitlab.com:group/sub/project.git").unwrap();
        assert_eq!(result.namespace, "group/sub");
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn parse_custom_gitlab_ssh() {
        let result = parse_git_url("git@gitlab.example.com:team/repo.git").unwrap();
        assert_eq!(result.platform, Platform::GitLab);
        assert_eq!(result.host, "gitlab.example.com");
    }

    // --- Edge cases ---

    #[test]
    fn parse_url_with_dot_git_suffix() {
        let result = parse_git_url("https://gitlab.com/group/project.git").unwrap();
        assert_eq!(result.repo_name, "project"); // .git stripped
    }

    #[test]
    fn parse_url_with_trailing_slash() {
        let result = parse_git_url("https://gitlab.com/group/project/").unwrap();
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn parse_url_with_dot_git_and_trailing_slash() {
        let result = parse_git_url("https://gitlab.com/group/project.git/").unwrap();
        assert_eq!(result.repo_name, "project");
    }

    #[test]
    fn parse_ssh_without_dot_git() {
        let result = parse_git_url("git@github.com:owner/repo").unwrap();
        assert_eq!(result.repo_name, "repo");
    }

    #[test]
    fn parse_http_upgrades_to_https() {
        let result = parse_git_url("http://github.com/owner/repo").unwrap();
        assert!(result.normalized_url.starts_with("https://"));
    }

    // --- Error cases ---

    #[test]
    fn parse_empty_url_returns_error() {
        let result = parse_git_url("");
        assert!(matches!(result, Err(GitUrlError::Empty)));
    }

    #[test]
    fn parse_invalid_url_returns_error() {
        let result = parse_git_url("not-a-valid-url");
        assert!(result.is_err());
    }

    #[test]
    fn parse_url_missing_repo_path() {
        let result = parse_git_url("https://github.com/owner");
        assert!(result.is_err());
    }

    #[test]
    fn parse_url_just_domain() {
        let result = parse_git_url("https://github.com");
        assert!(result.is_err());
    }

    #[test]
    fn parse_url_with_port() {
        let result = parse_git_url("https://gitlab.example.com:8443/team/repo").unwrap();
        assert_eq!(result.host, "gitlab.example.com:8443");
        assert_eq!(result.namespace, "team");
        assert_eq!(result.repo_name, "repo");
    }

    #[test]
    fn parse_ssh_with_custom_port() {
        // ssh://git@gitlab.example.com:2222/team/repo.git
        let result = parse_git_url("ssh://git@gitlab.example.com:2222/team/repo.git").unwrap();
        assert_eq!(result.namespace, "team");
        assert_eq!(result.repo_name, "repo");
    }

    // --- Normalization ---

    #[test]
    fn normalized_url_is_https_without_dot_git() {
        let result = parse_git_url("git@github.com:owner/repo.git").unwrap();
        assert_eq!(result.normalized_url, "https://github.com/owner/repo");
    }

    #[test]
    fn normalized_url_preserves_subgroups() {
        let result = parse_git_url("git@gitlab.com:a/b/c/repo.git").unwrap();
        assert_eq!(result.normalized_url, "https://gitlab.com/a/b/c/repo");
    }
}
```

#### 3.1.2 Workflow Template Rendering (tests/unit/workflow_template.rs)

```rust
#[cfg(test)]
mod tests {
    use web_platform::templates::{render_workflow, WorkflowTemplateContext};

    #[test]
    fn render_github_template_contains_platform() {
        let ctx = WorkflowTemplateContext {
            platform: "github".to_string(),
            project_slug: "owner/repo".to_string(),
            workspace_root: "/tmp/workspaces/1".to_string(),
            max_concurrent_agents: 2,
            default_branch: "main".to_string(),
        };
        let result = render_workflow("github", &ctx).unwrap();
        assert!(result.contains("github"));
        assert!(result.contains("owner/repo"));
    }

    #[test]
    fn render_gitlab_template_contains_platform() {
        let ctx = WorkflowTemplateContext {
            platform: "gitlab".to_string(),
            project_slug: "group/project".to_string(),
            workspace_root: "/tmp/workspaces/2".to_string(),
            max_concurrent_agents: 3,
            default_branch: "develop".to_string(),
        };
        let result = render_workflow("gitlab", &ctx).unwrap();
        assert!(result.contains("gitlab"));
        assert!(result.contains("group/project"));
    }

    #[test]
    fn render_template_substitutes_workspace_root() {
        let ctx = WorkflowTemplateContext {
            platform: "github".to_string(),
            project_slug: "owner/repo".to_string(),
            workspace_root: "/home/user/workspaces/42".to_string(),
            max_concurrent_agents: 2,
            default_branch: "main".to_string(),
        };
        let result = render_workflow("github", &ctx).unwrap();
        assert!(result.contains("/home/user/workspaces/42"));
    }

    #[test]
    fn render_template_substitutes_max_agents() {
        let ctx = WorkflowTemplateContext {
            platform: "github".to_string(),
            project_slug: "owner/repo".to_string(),
            workspace_root: "/tmp/ws".to_string(),
            max_concurrent_agents: 5,
            default_branch: "main".to_string(),
        };
        let result = render_workflow("github", &ctx).unwrap();
        assert!(result.contains("5"));
    }

    #[test]
    fn render_template_substitutes_default_branch() {
        let ctx = WorkflowTemplateContext {
            platform: "gitlab".to_string(),
            project_slug: "group/repo".to_string(),
            workspace_root: "/tmp/ws".to_string(),
            max_concurrent_agents: 2,
            default_branch: "develop".to_string(),
        };
        let result = render_workflow("gitlab", &ctx).unwrap();
        assert!(result.contains("develop"));
    }

    #[test]
    fn render_unknown_template_returns_error() {
        let ctx = WorkflowTemplateContext {
            platform: "bitbucket".to_string(),
            project_slug: "team/repo".to_string(),
            workspace_root: "/tmp/ws".to_string(),
            max_concurrent_agents: 2,
            default_branch: "main".to_string(),
        };
        let result = render_workflow("bitbucket", &ctx);
        assert!(result.is_err());
    }
}
```

#### 3.1.3 PID Verification (tests/unit/pid_verify.rs)

```rust
#[cfg(test)]
mod tests {
    use web_platform::process_manager::pid_verify::verify_pid;
    use chrono::Utc;

    #[test]
    fn verify_nonexistent_pid_returns_false() {
        // PID 999999 should not exist
        let result = verify_pid(999999, Utc::now());
        assert!(!result);
    }

    #[test]
    fn verify_pid_zero_returns_false() {
        let result = verify_pid(0, Utc::now());
        assert!(!result);
    }

    #[test]
    fn verify_current_process_pid_returns_false() {
        // Current process is not symphony-rust, so cmdline check should fail
        let pid = std::process::id();
        let result = verify_pid(pid, Utc::now());
        assert!(!result); // Not a symphony-rust process
    }

    #[test]
    fn verify_pid_with_very_old_start_time_returns_false() {
        use chrono::Duration;
        // Even if PID exists, start time mismatch should fail
        let pid = std::process::id();
        let old_time = Utc::now() - Duration::days(365);
        let result = verify_pid(pid, old_time);
        assert!(!result);
    }
}
```

#### 3.1.4 Service Status State Machine (tests/unit/service_status.rs)

```rust
#[cfg(test)]
mod tests {
    use web_platform::process_manager::ServiceStatus;

    #[test]
    fn stopped_can_transition_to_starting() {
        assert!(ServiceStatus::Stopped.can_transition_to(&ServiceStatus::Starting));
    }

    #[test]
    fn starting_can_transition_to_running() {
        assert!(ServiceStatus::Starting.can_transition_to(&ServiceStatus::Running));
    }

    #[test]
    fn starting_can_transition_to_error() {
        assert!(ServiceStatus::Starting.can_transition_to(&ServiceStatus::Error("fail".into())));
    }

    #[test]
    fn running_can_transition_to_stopping() {
        assert!(ServiceStatus::Running.can_transition_to(&ServiceStatus::Stopping));
    }

    #[test]
    fn stopping_can_transition_to_stopped() {
        assert!(ServiceStatus::Stopping.can_transition_to(&ServiceStatus::Stopped));
    }

    #[test]
    fn running_cannot_transition_to_starting() {
        assert!(!ServiceStatus::Running.can_transition_to(&ServiceStatus::Starting));
    }

    #[test]
    fn stopped_cannot_transition_to_running_directly() {
        // Must go through Starting first
        assert!(!ServiceStatus::Stopped.can_transition_to(&ServiceStatus::Running));
    }

    #[test]
    fn error_can_transition_to_starting_for_retry() {
        assert!(ServiceStatus::Error("prev".into()).can_transition_to(&ServiceStatus::Starting));
    }

    #[test]
    fn failed_cannot_transition_to_starting() {
        // Failed is terminal - requires manual intervention
        assert!(!ServiceStatus::Failed.can_transition_to(&ServiceStatus::Starting));
    }

    #[test]
    fn running_can_transition_to_error_on_crash() {
        assert!(ServiceStatus::Running.can_transition_to(&ServiceStatus::Error("crashed".into())));
    }

    #[test]
    fn error_can_transition_to_failed_after_max_retries() {
        assert!(ServiceStatus::Error("x".into()).can_transition_to(&ServiceStatus::Failed));
    }
}
```

### 3.2 Integration Tests

#### 3.2.1 Project Repository (tests/integration/repo_projects.rs)

```rust
#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use web_platform::db::init_pool;
    use web_platform::repository::{SqliteRepository, ProjectRepository, UserRepository};
    use web_platform::auth::password::hash_password;

    async fn setup() -> (SqliteRepository, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = init_pool(db_path.to_str().unwrap());
        let repo = SqliteRepository::new(pool);
        // Create a test user for created_by
        let hash = hash_password("pass").unwrap();
        repo.create_user("testuser", &hash, None, "user").await.unwrap();
        (repo, dir)
    }

    #[tokio::test]
    async fn create_project_returns_id() {
        let (repo, _dir) = setup().await;
        let project = repo.create_project(&NewProject {
            name: "test-project".into(),
            git_url: "https://github.com/owner/repo".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "repo".into(),
            default_branch: "main".into(),
            created_by: 1,
        }).await.unwrap();
        assert!(project.id > 0);
        assert_eq!(project.name, "test-project");
        assert_eq!(project.service_status, "stopped");
    }

    #[tokio::test]
    async fn create_project_duplicate_url_returns_error() {
        let (repo, _dir) = setup().await;
        let p = NewProject {
            name: "p1".into(),
            git_url: "https://github.com/owner/repo".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "repo".into(),
            default_branch: "main".into(),
            created_by: 1,
        };
        repo.create_project(&p).await.unwrap();
        let result = repo.create_project(&p).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_project_by_id() {
        let (repo, _dir) = setup().await;
        let created = repo.create_project(&NewProject {
            name: "get-test".into(),
            git_url: "https://github.com/owner/get-test".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "get-test".into(),
            default_branch: "main".into(),
            created_by: 1,
        }).await.unwrap();

        let found = repo.get_project(created.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "get-test");
    }

    #[tokio::test]
    async fn get_nonexistent_project_returns_none() {
        let (repo, _dir) = setup().await;
        let found = repo.get_project(99999).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn list_projects_for_admin_returns_all() {
        let (repo, _dir) = setup().await;
        for i in 0..3 {
            repo.create_project(&NewProject {
                name: format!("proj-{}", i),
                git_url: format!("https://github.com/owner/proj-{}", i),
                platform: "github".into(),
                platform_host: Some("github.com".into()),
                namespace: "owner".into(),
                repo_name: format!("proj-{}", i),
                default_branch: "main".into(),
                created_by: 1,
            }).await.unwrap();
        }
        let projects = repo.list_projects_for_user(1, true).await.unwrap();
        assert_eq!(projects.len(), 3);
    }

    #[tokio::test]
    async fn list_projects_for_user_filters_by_membership() {
        let (repo, _dir) = setup().await;
        let hash = hash_password("pass").unwrap();
        let user2 = repo.create_user("user2", &hash, None, "user").await.unwrap();

        // Create project owned by user 1
        let p = repo.create_project(&NewProject {
            name: "owned".into(),
            git_url: "https://github.com/owner/owned".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "owned".into(),
            default_branch: "main".into(),
            created_by: 1,
        }).await.unwrap();

        // user2 is not a member - should see nothing
        let projects = repo.list_projects_for_user(user2.id, false).await.unwrap();
        assert_eq!(projects.len(), 0);
    }

    #[tokio::test]
    async fn update_project_changes_fields() {
        let (repo, _dir) = setup().await;
        let p = repo.create_project(&NewProject {
            name: "original".into(),
            git_url: "https://github.com/owner/update-test".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "update-test".into(),
            default_branch: "main".into(),
            created_by: 1,
        }).await.unwrap();

        repo.update_project(p.id, &ProjectUpdate {
            name: Some("updated".into()),
            description: Some("new desc".into()),
            ..Default::default()
        }).await.unwrap();

        let updated = repo.get_project(p.id).await.unwrap().unwrap();
        assert_eq!(updated.name, "updated");
        assert_eq!(updated.description.unwrap(), "new desc");
    }

    #[tokio::test]
    async fn delete_project_removes_from_list() {
        let (repo, _dir) = setup().await;
        let p = repo.create_project(&NewProject {
            name: "to-delete".into(),
            git_url: "https://github.com/owner/to-delete".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "to-delete".into(),
            default_branch: "main".into(),
            created_by: 1,
        }).await.unwrap();

        repo.delete_project(p.id).await.unwrap();
        let found = repo.get_project(p.id).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn update_service_status() {
        let (repo, _dir) = setup().await;
        let p = repo.create_project(&NewProject {
            name: "status-test".into(),
            git_url: "https://github.com/owner/status-test".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "status-test".into(),
            default_branch: "main".into(),
            created_by: 1,
        }).await.unwrap();

        repo.update_service_status(p.id, &ServiceStatus {
            status: "running".into(),
            pid: Some(12345),
        }).await.unwrap();

        let updated = repo.get_project(p.id).await.unwrap().unwrap();
        assert_eq!(updated.service_status, "running");
        assert_eq!(updated.service_pid, Some(12345));
    }

    #[tokio::test]
    async fn delete_project_cascades_to_members() {
        let (repo, _dir) = setup().await;
        let p = repo.create_project(&NewProject {
            name: "cascade-test".into(),
            git_url: "https://github.com/owner/cascade".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "cascade".into(),
            default_branch: "main".into(),
            created_by: 1,
        }).await.unwrap();

        // Add a member
        repo.add_member(p.id, 1, "owner").await.unwrap();

        // Delete project
        repo.delete_project(p.id).await.unwrap();

        // Members should be gone (CASCADE)
        let members = repo.list_members(p.id).await.unwrap();
        assert_eq!(members.len(), 0);
    }
}
```

#### 3.2.2 Member Repository (tests/integration/repo_members.rs)

```rust
#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use web_platform::db::init_pool;
    use web_platform::repository::*;
    use web_platform::auth::password::hash_password;

    async fn setup() -> (SqliteRepository, i64, i64, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = init_pool(db_path.to_str().unwrap());
        let repo = SqliteRepository::new(pool);

        let hash = hash_password("pass").unwrap();
        let user1 = repo.create_user("owner1", &hash, None, "user").await.unwrap();
        let user2 = repo.create_user("member1", &hash, None, "user").await.unwrap();

        let project = repo.create_project(&NewProject {
            name: "member-test".into(),
            git_url: "https://github.com/owner/member-test".into(),
            platform: "github".into(),
            platform_host: Some("github.com".into()),
            namespace: "owner".into(),
            repo_name: "member-test".into(),
            default_branch: "main".into(),
            created_by: user1.id,
        }).await.unwrap();

        (repo, project.id, user2.id, dir)
    }

    #[tokio::test]
    async fn add_member_success() {
        let (repo, project_id, user_id, _dir) = setup().await;
        repo.add_member(project_id, user_id, "member").await.unwrap();
        let is_member = repo.is_member(project_id, user_id).await.unwrap();
        assert!(is_member);
    }

    #[tokio::test]
    async fn add_member_duplicate_returns_error() {
        let (repo, project_id, user_id, _dir) = setup().await;
        repo.add_member(project_id, user_id, "member").await.unwrap();
        let result = repo.add_member(project_id, user_id, "member").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn remove_member_success() {
        let (repo, project_id, user_id, _dir) = setup().await;
        repo.add_member(project_id, user_id, "member").await.unwrap();
        repo.remove_member(project_id, user_id).await.unwrap();
        let is_member = repo.is_member(project_id, user_id).await.unwrap();
        assert!(!is_member);
    }

    #[tokio::test]
    async fn update_member_role() {
        let (repo, project_id, user_id, _dir) = setup().await;
        repo.add_member(project_id, user_id, "member").await.unwrap();
        repo.update_member_role(project_id, user_id, "owner").await.unwrap();
        let role = repo.get_member_role(project_id, user_id).await.unwrap();
        assert_eq!(role, Some("owner".to_string()));
    }

    #[tokio::test]
    async fn get_member_role_non_member_returns_none() {
        let (repo, project_id, _, _dir) = setup().await;
        let role = repo.get_member_role(project_id, 99999).await.unwrap();
        assert_eq!(role, None);
    }

    #[tokio::test]
    async fn is_member_returns_false_for_non_member() {
        let (repo, project_id, _, _dir) = setup().await;
        let is_member = repo.is_member(project_id, 99999).await.unwrap();
        assert!(!is_member);
    }

    #[tokio::test]
    async fn list_members_returns_all() {
        let (repo, project_id, user_id, _dir) = setup().await;
        let hash = hash_password("pass").unwrap();
        let user3 = repo.create_user("member2", &hash, None, "user").await.unwrap();

        repo.add_member(project_id, user_id, "member").await.unwrap();
        repo.add_member(project_id, user3.id, "member").await.unwrap();

        let members = repo.list_members(project_id).await.unwrap();
        assert_eq!(members.len(), 2);
    }

    #[tokio::test]
    async fn sync_members_adds_new_and_skips_existing() {
        let (repo, project_id, user_id, _dir) = setup().await;
        repo.add_member(project_id, user_id, "member").await.unwrap();

        let hash = hash_password("pass").unwrap();
        let user3 = repo.create_user("sync_user", &hash, None, "user").await.unwrap();

        let sync_members = vec![
            SyncMember { user_id: user_id, username: "member1".into() },  // existing
            SyncMember { user_id: user3.id, username: "sync_user".into() }, // new
        ];

        let result = repo.sync_members(project_id, &sync_members).await.unwrap();
        assert_eq!(result.added, 1);
        assert_eq!(result.skipped, 1);
    }
}
```

#### 3.2.3 Auth Middleware with Project Permissions (tests/integration/auth_project.rs)

```rust
#[cfg(test)]
mod tests {
    use crate::common::TestApp;
    use reqwest::StatusCode;

    #[tokio::test]
    async fn owner_can_access_project() {
        let app = TestApp::new().await;
        let owner_token = app.create_user_get_token("owner1", "Pass123", "user").await;
        let project_id = app.create_project("https://github.com/test/auth-proj", &owner_token).await;

        let resp = app.get(&format!("/api/projects/{}", project_id), Some(&owner_token)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn member_can_access_project() {
        let app = TestApp::new().await;
        let owner_token = app.create_user_get_token("owner2", "Pass123", "user").await;
        let project_id = app.create_project("https://github.com/test/member-access", &owner_token).await;

        app.create_test_user("member2", "Pass123", "user").await;
        let member_id = app.get_user_id("member2").await;
        app.add_project_member(project_id, member_id, "member", &owner_token).await;

        let member_token = app.login_get_token("member2", "Pass123").await;
        let resp = app.get(&format!("/api/projects/{}", project_id), Some(&member_token)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn non_member_cannot_access_project() {
        let app = TestApp::new().await;
        let owner_token = app.create_user_get_token("owner3", "Pass123", "user").await;
        let project_id = app.create_project("https://github.com/test/no-access", &owner_token).await;

        let outsider_token = app.create_user_get_token("outsider3", "Pass123", "user").await;
        let resp = app.get(&format!("/api/projects/{}", project_id), Some(&outsider_token)).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_can_access_any_project() {
        let app = TestApp::new().await;
        let owner_token = app.create_user_get_token("owner4", "Pass123", "user").await;
        let project_id = app.create_project("https://github.com/test/admin-access", &owner_token).await;

        // Admin (from TestApp::new) can access without being a member
        let resp = app.get(&format!("/api/projects/{}", project_id), Some(&app.admin_token)).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn member_cannot_delete_project() {
        let app = TestApp::new().await;
        let owner_token = app.create_user_get_token("owner5", "Pass123", "user").await;
        let project_id = app.create_project("https://github.com/test/no-delete", &owner_token).await;

        app.create_test_user("member5", "Pass123", "user").await;
        let member_id = app.get_user_id("member5").await;
        app.add_project_member(project_id, member_id, "member", &owner_token).await;

        let member_token = app.login_get_token("member5", "Pass123").await;
        let resp = app.delete(&format!("/api/projects/{}", project_id), Some(&member_token)).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
```

#### 3.2.4 Process Manager Integration (tests/integration/process_manager.rs)

```rust
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::common::mock_process::{MockProcessSpawner, MockPidVerifier};
    use web_platform::process_manager::{ProcessManager, ServiceStatus};

    #[tokio::test]
    async fn start_service_spawns_process_and_records_pid() {
        let spawner = MockProcessSpawner::new();
        let verifier = MockPidVerifier::new();
        let pm = ProcessManager::new_with_mocks(spawner.clone(), verifier);

        let result = pm.start_service(1, "/path/to/workflow.md", HashMap::new()).await;
        assert!(result.is_ok());

        let log = spawner.get_log();
        assert_eq!(log.spawned.len(), 1);
        assert_eq!(log.spawned[0].project_id, 1);

        let state = pm.get_status(1).await.unwrap();
        assert_eq!(state.status, ServiceStatus::Running);
    }

    #[tokio::test]
    async fn start_already_running_returns_conflict() {
        let spawner = MockProcessSpawner::new();
        let verifier = MockPidVerifier::new();
        let pm = ProcessManager::new_with_mocks(spawner.clone(), verifier);

        pm.start_service(1, "/path/to/workflow.md", HashMap::new()).await.unwrap();
        let result = pm.start_service(1, "/path/to/workflow.md", HashMap::new()).await;
        assert!(result.is_err()); // Conflict
    }

    #[tokio::test]
    async fn stop_service_sends_sigterm() {
        let spawner = MockProcessSpawner::new();
        let verifier = MockPidVerifier::new();
        let pm = ProcessManager::new_with_mocks(spawner.clone(), verifier.clone());

        pm.start_service(1, "/path/to/workflow.md", HashMap::new()).await.unwrap();
        let state = pm.get_status(1).await.unwrap();
        verifier.set_valid(state.pid);

        pm.stop_service(1).await.unwrap();

        let log = spawner.get_log();
        assert_eq!(log.killed.len(), 1);
        assert_eq!(log.killed[0].signal, "SIGTERM");

        let final_state = pm.get_status(1).await.unwrap();
        assert_eq!(final_state.status, ServiceStatus::Stopped);
    }

    #[tokio::test]
    async fn stop_already_stopped_is_idempotent() {
        let spawner = MockProcessSpawner::new();
        let verifier = MockPidVerifier::new();
        let pm = ProcessManager::new_with_mocks(spawner, verifier);

        // Never started - stop should be OK (idempotent)
        let result = pm.stop_service(1).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn restart_stops_then_starts() {
        let spawner = MockProcessSpawner::new();
        let verifier = MockPidVerifier::new();
        let pm = ProcessManager::new_with_mocks(spawner.clone(), verifier.clone());

        pm.start_service(1, "/path/to/workflow.md", HashMap::new()).await.unwrap();
        let state = pm.get_status(1).await.unwrap();
        verifier.set_valid(state.pid);

        pm.restart_service(1, "/path/to/workflow.md", HashMap::new()).await.unwrap();

        let log = spawner.get_log();
        assert_eq!(log.spawned.len(), 2); // started twice
        assert_eq!(log.killed.len(), 1);  // stopped once
    }

    #[tokio::test]
    async fn concurrent_start_stop_uses_mutex() {
        let spawner = MockProcessSpawner::new();
        let verifier = MockPidVerifier::new();
        let pm = ProcessManager::new_with_mocks(spawner.clone(), verifier.clone());

        // Start service first
        pm.start_service(1, "/path/to/workflow.md", HashMap::new()).await.unwrap();
        let state = pm.get_status(1).await.unwrap();
        verifier.set_valid(state.pid);

        // Concurrent stop + start should not race
        let pm1 = pm.clone();
        let pm2 = pm.clone();
        let (r1, r2) = tokio::join!(
            pm1.stop_service(1),
            pm2.start_service(1, "/path/to/workflow.md", HashMap::new()),
        );

        // One should succeed, one should get conflict or succeed after the other
        // The key assertion: no panic, no data corruption
        assert!(r1.is_ok() || r2.is_ok());
    }

    #[tokio::test]
    async fn spawn_failure_sets_error_status() {
        let spawner = MockProcessSpawner::new();
        spawner.set_spawn_error(Some("binary not found".into()));
        let verifier = MockPidVerifier::new();
        let pm = ProcessManager::new_with_mocks(spawner, verifier);

        let result = pm.start_service(1, "/path/to/workflow.md", HashMap::new()).await;
        assert!(result.is_err());

        let state = pm.get_status(1).await;
        // Should be in Error or Stopped state
        match state {
            Some(s) => assert!(matches!(s.status, ServiceStatus::Error(_) | ServiceStatus::Stopped)),
            None => {} // Never registered is also acceptable
        }
    }
}
```

---

### 3.3 API/Interface Tests

#### 3.3.1 Project CRUD Endpoints (tests/api/api_projects.rs)

```rust
mod common;

use reqwest::StatusCode;
use crate::common::TestApp;
use crate::common::fixtures::git_urls;

// ============================================================
// POST /api/projects - Create Project
// ============================================================

#[tokio::test]
async fn create_project_happy_path() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("creator1", "Pass123", "user").await;

    let resp = app.post(
        "/api/projects",
        &serde_json::json!({
            "git_url": "https://github.com/owner/new-project"
        }),
        Some(&token),
    ).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["success"].as_bool().unwrap());
    assert!(body["data"]["id"].as_i64().unwrap() > 0);
    assert_eq!(body["data"]["platform"].as_str().unwrap(), "github");
    assert_eq!(body["data"]["namespace"].as_str().unwrap(), "owner");
    assert_eq!(body["data"]["repo_name"].as_str().unwrap(), "new-project");
    assert_eq!(body["data"]["service_status"].as_str().unwrap(), "stopped");
}

#[tokio::test]
async fn create_project_with_full_options() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("creator2", "Pass123", "user").await;

    let resp = app.post(
        "/api/projects",
        &serde_json::json!({
            "git_url": "https://gitlab.com/group/full-opts",
            "name": "Custom Name",
            "description": "A test project",
            "default_branch": "develop"
        }),
        Some(&token),
    ).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["name"].as_str().unwrap(), "Custom Name");
    assert_eq!(body["data"]["default_branch"].as_str().unwrap(), "develop");
}

#[tokio::test]
async fn create_project_no_token_returns_401() {
    let app = TestApp::new().await;
    let resp = app.post(
        "/api/projects",
        &serde_json::json!({ "git_url": "https://github.com/owner/repo" }),
        None,
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_project_expired_token_returns_401() {
    let app = TestApp::new().await;
    let expired = app.generate_expired_token();
    let resp = app.post(
        "/api/projects",
        &serde_json::json!({ "git_url": "https://github.com/owner/repo" }),
        Some(&expired),
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_project_missing_git_url_returns_400() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("creator3", "Pass123", "user").await;
    let resp = app.post(
        "/api/projects",
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_project_invalid_url_returns_400() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("creator4", "Pass123", "user").await;
    let resp = app.post(
        "/api/projects",
        &serde_json::json!({ "git_url": "not-a-valid-url" }),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_project_duplicate_url_returns_409() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("creator5", "Pass123", "user").await;

    app.post(
        "/api/projects",
        &serde_json::json!({ "git_url": "https://github.com/owner/dup-test" }),
        Some(&token),
    ).await;

    let resp = app.post(
        "/api/projects",
        &serde_json::json!({ "git_url": "https://github.com/owner/dup-test" }),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn create_project_creator_becomes_owner() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("creator6", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/owner-test", &token).await;

    let resp = app.get(
        &format!("/api/projects/{}/members", project_id),
        Some(&token),
    ).await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let members = body["data"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["role"].as_str().unwrap(), "owner");
}

// ============================================================
// GET /api/projects - List Projects
// ============================================================

#[tokio::test]
async fn list_projects_returns_user_projects_only() {
    let app = TestApp::new().await;
    let user1_token = app.create_user_get_token("lister1", "Pass123", "user").await;
    let user2_token = app.create_user_get_token("lister2", "Pass123", "user").await;

    app.create_project("https://github.com/owner/list-proj1", &user1_token).await;
    app.create_project("https://github.com/owner/list-proj2", &user2_token).await;

    let resp = app.get("/api/projects", Some(&user1_token)).await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
}

#[tokio::test]
async fn list_projects_admin_sees_all() {
    let app = TestApp::new().await;
    let user_token = app.create_user_get_token("lister3", "Pass123", "user").await;
    app.create_project("https://github.com/owner/admin-list1", &user_token).await;
    app.create_project("https://github.com/owner/admin-list2", &user_token).await;

    let resp = app.get("/api/projects", Some(&app.admin_token)).await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    assert!(records.len() >= 2);
}

#[tokio::test]
async fn list_projects_pagination() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("paginator", "Pass123", "user").await;
    for i in 0..5 {
        app.create_project(&format!("https://github.com/owner/page-{}", i), &token).await;
    }

    let resp = app.get("/api/projects?pageNo=1&pageSize=2", Some(&token)).await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(body["data"]["totalCount"].as_i64().unwrap(), 5);
    assert_eq!(body["data"]["pages"].as_i64().unwrap(), 3);
}

#[tokio::test]
async fn list_projects_filter_by_platform() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("filterer", "Pass123", "user").await;
    app.create_project("https://github.com/owner/gh-proj", &token).await;
    app.create_project("https://gitlab.com/group/gl-proj", &token).await;

    let resp = app.get("/api/projects?platform=github", Some(&token)).await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["platform"].as_str().unwrap(), "github");
}

#[tokio::test]
async fn list_projects_search_by_name() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("searcher", "Pass123", "user").await;
    app.create_project("https://github.com/owner/alpha-project", &token).await;
    app.create_project("https://github.com/owner/beta-project", &token).await;

    let resp = app.get("/api/projects?search=alpha", Some(&token)).await;
    let body: serde_json::Value = resp.json().await.unwrap();
    let records = body["data"]["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
}

#[tokio::test]
async fn list_projects_no_token_returns_401() {
    let app = TestApp::new().await;
    let resp = app.get("/api/projects", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================
// GET /api/projects/:id - Project Detail
// ============================================================

#[tokio::test]
async fn get_project_detail_happy_path() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("detail1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/detail-proj", &token).await;

    let resp = app.get(&format!("/api/projects/{}", project_id), Some(&token)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["id"].as_i64().unwrap(), project_id);
}

#[tokio::test]
async fn get_project_nonexistent_returns_404() {
    let app = TestApp::new().await;
    let resp = app.get("/api/projects/99999", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_project_non_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("detail_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/private-proj", &owner_token).await;

    let outsider_token = app.create_user_get_token("detail_outsider", "Pass123", "user").await;
    let resp = app.get(&format!("/api/projects/{}", project_id), Some(&outsider_token)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================
// PUT /api/projects/:id - Update Project
// ============================================================

#[tokio::test]
async fn update_project_owner_success() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("updater1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/update-proj", &token).await;

    let resp = app.put(
        &format!("/api/projects/{}", project_id),
        &serde_json::json!({
            "name": "Updated Name",
            "description": "Updated description"
        }),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn update_project_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("up_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/up-member", &owner_token).await;

    app.create_test_user("up_member", "Pass123", "user").await;
    let member_id = app.get_user_id("up_member").await;
    app.add_project_member(project_id, member_id, "member", &owner_token).await;

    let member_token = app.login_get_token("up_member", "Pass123").await;
    let resp = app.put(
        &format!("/api/projects/{}", project_id),
        &serde_json::json!({ "name": "Hacked" }),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_project_admin_success() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("up_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/admin-update", &owner_token).await;

    let resp = app.put(
        &format!("/api/projects/{}", project_id),
        &serde_json::json!({ "name": "Admin Updated" }),
        Some(&app.admin_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================
// DELETE /api/projects/:id - Delete Project
// ============================================================

#[tokio::test]
async fn delete_project_owner_success() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("deleter1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/del-proj", &token).await;

    let resp = app.delete(&format!("/api/projects/{}", project_id), Some(&token)).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify deleted
    let get_resp = app.get(&format!("/api/projects/{}", project_id), Some(&token)).await;
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_project_running_service_returns_409() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("deleter2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/del-running", &token).await;

    // Start service (mock)
    app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    // Try to delete while running
    let resp = app.delete(&format!("/api/projects/{}", project_id), Some(&token)).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn delete_project_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("del_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/del-forbidden", &owner_token).await;

    app.create_test_user("del_member", "Pass123", "user").await;
    let member_id = app.get_user_id("del_member").await;
    app.add_project_member(project_id, member_id, "member", &owner_token).await;

    let member_token = app.login_get_token("del_member", "Pass123").await;
    let resp = app.delete(&format!("/api/projects/{}", project_id), Some(&member_token)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn delete_project_nonexistent_returns_404() {
    let app = TestApp::new().await;
    let resp = app.delete("/api/projects/99999", Some(&app.admin_token)).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
```

#### 3.3.2 Member Management Endpoints (tests/api/api_members.rs)

```rust
mod common;

use reqwest::StatusCode;
use crate::common::TestApp;

// ============================================================
// GET /api/projects/:id/members - List Members
// ============================================================

#[tokio::test]
async fn list_members_happy_path() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("mem_owner1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/mem-list", &owner_token).await;

    let resp = app.get(
        &format!("/api/projects/{}/members", project_id),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let members = body["data"].as_array().unwrap();
    assert_eq!(members.len(), 1); // owner auto-added
    assert_eq!(members[0]["role"].as_str().unwrap(), "owner");
}

#[tokio::test]
async fn list_members_non_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("mem_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/mem-list-403", &owner_token).await;

    let outsider_token = app.create_user_get_token("mem_outsider", "Pass123", "user").await;
    let resp = app.get(
        &format!("/api/projects/{}/members", project_id),
        Some(&outsider_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_members_no_token_returns_401() {
    let app = TestApp::new().await;
    let resp = app.get("/api/projects/1/members", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================
// POST /api/projects/:id/members - Add Member
// ============================================================

#[tokio::test]
async fn add_member_happy_path() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("add_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/add-mem", &owner_token).await;

    app.create_test_user("new_member", "Pass123", "user").await;
    let new_member_id = app.get_user_id("new_member").await;

    let resp = app.post(
        &format!("/api/projects/{}/members", project_id),
        &serde_json::json!({
            "user_id": new_member_id,
            "role": "member"
        }),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn add_member_duplicate_returns_409() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("dup_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/dup-mem", &owner_token).await;

    app.create_test_user("dup_member", "Pass123", "user").await;
    let member_id = app.get_user_id("dup_member").await;

    app.add_project_member(project_id, member_id, "member", &owner_token).await;

    let resp = app.post(
        &format!("/api/projects/{}/members", project_id),
        &serde_json::json!({ "user_id": member_id, "role": "member" }),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn add_member_nonexistent_user_returns_404() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("add_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/add-nouser", &owner_token).await;

    let resp = app.post(
        &format!("/api/projects/{}/members", project_id),
        &serde_json::json!({ "user_id": 99999, "role": "member" }),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn add_member_by_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("add_owner3", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/add-forbidden", &owner_token).await;

    app.create_test_user("existing_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("existing_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    app.create_test_user("target_mem", "Pass123", "user").await;
    let target_id = app.get_user_id("target_mem").await;

    let member_token = app.login_get_token("existing_mem", "Pass123").await;
    let resp = app.post(
        &format!("/api/projects/{}/members", project_id),
        &serde_json::json!({ "user_id": target_id, "role": "member" }),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn add_member_invalid_role_returns_400() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("add_owner4", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/add-badrole", &owner_token).await;

    app.create_test_user("role_target", "Pass123", "user").await;
    let target_id = app.get_user_id("role_target").await;

    let resp = app.post(
        &format!("/api/projects/{}/members", project_id),
        &serde_json::json!({ "user_id": target_id, "role": "superadmin" }),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ============================================================
// PUT /api/projects/:id/members/:userId - Update Member Role
// ============================================================

#[tokio::test]
async fn update_member_role_happy_path() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("role_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/role-update", &owner_token).await;

    app.create_test_user("role_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("role_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let resp = app.put(
        &format!("/api/projects/{}/members/{}", project_id, mem_id),
        &serde_json::json!({ "role": "owner" }),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn update_member_role_non_member_target_returns_404() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("role_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/role-404", &owner_token).await;

    let resp = app.put(
        &format!("/api/projects/{}/members/99999", project_id),
        &serde_json::json!({ "role": "owner" }),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_member_role_by_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("role_owner3", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/role-403", &owner_token).await;

    app.create_test_user("role_mem2", "Pass123", "user").await;
    let mem_id = app.get_user_id("role_mem2").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("role_mem2", "Pass123").await;
    let owner_id = app.get_user_id("role_owner3").await;

    let resp = app.put(
        &format!("/api/projects/{}/members/{}", project_id, owner_id),
        &serde_json::json!({ "role": "member" }),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================
// DELETE /api/projects/:id/members/:userId - Remove Member
// ============================================================

#[tokio::test]
async fn remove_member_happy_path() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("rm_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/rm-mem", &owner_token).await;

    app.create_test_user("rm_target", "Pass123", "user").await;
    let target_id = app.get_user_id("rm_target").await;
    app.add_project_member(project_id, target_id, "member", &owner_token).await;

    let resp = app.delete(
        &format!("/api/projects/{}/members/{}", project_id, target_id),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn remove_last_owner_returns_400() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("last_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/last-owner", &owner_token).await;
    let owner_id = app.get_user_id("last_owner").await;

    // Try to remove the only owner
    let resp = app.delete(
        &format!("/api/projects/{}/members/{}", project_id, owner_id),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn remove_member_by_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("rm_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/rm-403", &owner_token).await;

    app.create_test_user("rm_mem1", "Pass123", "user").await;
    let mem1_id = app.get_user_id("rm_mem1").await;
    app.add_project_member(project_id, mem1_id, "member", &owner_token).await;

    app.create_test_user("rm_mem2", "Pass123", "user").await;
    let mem2_id = app.get_user_id("rm_mem2").await;
    app.add_project_member(project_id, mem2_id, "member", &owner_token).await;

    let mem1_token = app.login_get_token("rm_mem1", "Pass123").await;
    let resp = app.delete(
        &format!("/api/projects/{}/members/{}", project_id, mem2_id),
        Some(&mem1_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================
// POST /api/projects/:id/members/sync - Sync Members
// ============================================================

#[tokio::test]
async fn sync_members_happy_path() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("sync_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/sync-proj", &owner_token).await;

    // Configure user token for platform access
    app.put(
        "/api/user/config",
        &serde_json::json!({
            "githubToken": "ghp_test_token"
        }),
        Some(&owner_token),
    ).await;

    let resp = app.post(
        &format!("/api/projects/{}/members/sync", project_id),
        &serde_json::json!({}),
        Some(&owner_token),
    ).await;
    // May return 200 with sync results or 502 if platform unreachable in test
    assert!(resp.status() == StatusCode::OK || resp.status() == StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn sync_members_no_token_configured_returns_400() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("sync_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/sync-notoken", &owner_token).await;

    let resp = app.post(
        &format!("/api/projects/{}/members/sync", project_id),
        &serde_json::json!({}),
        Some(&owner_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sync_members_by_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("sync_owner3", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/sync-403", &owner_token).await;

    app.create_test_user("sync_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("sync_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("sync_mem", "Pass123").await;
    let resp = app.post(
        &format!("/api/projects/{}/members/sync", project_id),
        &serde_json::json!({}),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
```

#### 3.3.3 Service Control Endpoints (tests/api/api_service.rs)

```rust
mod common;

use reqwest::StatusCode;
use crate::common::TestApp;

// ============================================================
// POST /api/projects/:id/start - Start Service
// ============================================================

#[tokio::test]
async fn start_service_happy_path() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_owner1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-start", &token).await;

    let resp = app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn start_service_already_running_returns_409() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-conflict", &token).await;

    app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    let resp = app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn start_service_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("svc_owner3", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-mem-start", &owner_token).await;

    app.create_test_user("svc_mem1", "Pass123", "user").await;
    let mem_id = app.get_user_id("svc_mem1").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("svc_mem1", "Pass123").await;
    let resp = app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn start_service_no_token_returns_401() {
    let app = TestApp::new().await;
    let resp = app.post(
        "/api/projects/1/start",
        &serde_json::json!({}),
        None,
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn start_service_nonexistent_project_returns_404() {
    let app = TestApp::new().await;
    let resp = app.post(
        "/api/projects/99999/start",
        &serde_json::json!({}),
        Some(&app.admin_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ============================================================
// POST /api/projects/:id/stop - Stop Service
// ============================================================

#[tokio::test]
async fn stop_service_happy_path() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_stop1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-stop", &token).await;

    // Start first
    app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    let resp = app.post(
        &format!("/api/projects/{}/stop", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn stop_service_already_stopped_is_idempotent() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_stop2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-stop-idem", &token).await;

    let resp = app.post(
        &format!("/api/projects/{}/stop", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn stop_service_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("svc_stop_own", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-stop-403", &owner_token).await;

    app.create_test_user("svc_stop_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("svc_stop_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("svc_stop_mem", "Pass123").await;
    let resp = app.post(
        &format!("/api/projects/{}/stop", project_id),
        &serde_json::json!({}),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================
// POST /api/projects/:id/restart - Restart Service
// ============================================================

#[tokio::test]
async fn restart_service_happy_path() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_restart1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-restart", &token).await;

    // Start first
    app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    let resp = app.post(
        &format!("/api/projects/{}/restart", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn restart_stopped_service_starts_it() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_restart2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-restart-cold", &token).await;

    let resp = app.post(
        &format!("/api/projects/{}/restart", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify it's now running
    let status_resp = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&token),
    ).await;
    let body: serde_json::Value = status_resp.json().await.unwrap();
    assert_eq!(body["data"]["status"].as_str().unwrap(), "running");
}

#[tokio::test]
async fn restart_service_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("svc_restart_own", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-restart-403", &owner_token).await;

    app.create_test_user("svc_restart_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("svc_restart_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("svc_restart_mem", "Pass123").await;
    let resp = app.post(
        &format!("/api/projects/{}/restart", project_id),
        &serde_json::json!({}),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ============================================================
// GET /api/projects/:id/status - Service Status
// ============================================================

#[tokio::test]
async fn get_service_status_stopped() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_status1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-status", &token).await;

    let resp = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"].as_str().unwrap(), "stopped");
    assert!(body["data"]["pid"].is_null());
}

#[tokio::test]
async fn get_service_status_running() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("svc_status2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-status-run", &token).await;

    app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    let resp = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["status"].as_str().unwrap(), "running");
    assert!(body["data"]["pid"].as_i64().unwrap() > 0);
    assert!(body["data"]["started_at"].is_string());
}

#[tokio::test]
async fn get_service_status_member_can_view() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("svc_status_own", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-status-mem", &owner_token).await;

    app.create_test_user("svc_status_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("svc_status_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("svc_status_mem", "Pass123").await;
    let resp = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_service_status_non_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("svc_status_own2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/svc-status-403", &owner_token).await;

    let outsider_token = app.create_user_get_token("svc_status_out", "Pass123", "user").await;
    let resp = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&outsider_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
```

#### 3.3.4 Workflow Template Endpoints (tests/api/api_workflow.rs)

```rust
mod common;

use reqwest::StatusCode;
use crate::common::TestApp;

// ============================================================
// GET /api/projects/:id/workflow - Get Workflow
// ============================================================

#[tokio::test]
async fn get_workflow_default_template() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("wf_owner1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-get", &token).await;

    let resp = app.get(
        &format!("/api/projects/{}/workflow", project_id),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["data"]["template_mode"].as_str().unwrap(), "default");
    assert!(!body["data"]["content"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn get_workflow_non_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("wf_owner2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-403", &owner_token).await;

    let outsider_token = app.create_user_get_token("wf_outsider", "Pass123", "user").await;
    let resp = app.get(
        &format!("/api/projects/{}/workflow", project_id),
        Some(&outsider_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn get_workflow_no_token_returns_401() {
    let app = TestApp::new().await;
    let resp = app.get("/api/projects/1/workflow", None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ============================================================
// PUT /api/projects/:id/workflow - Update Workflow
// ============================================================

#[tokio::test]
async fn update_workflow_to_custom() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("wf_up1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-custom", &token).await;

    let custom_content = "# Custom Workflow\n\nDo things differently.";
    let resp = app.put(
        &format!("/api/projects/{}/workflow", project_id),
        &serde_json::json!({
            "template_mode": "custom",
            "content": custom_content
        }),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify
    let get_resp = app.get(
        &format!("/api/projects/{}/workflow", project_id),
        Some(&token),
    ).await;
    let body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(body["data"]["template_mode"].as_str().unwrap(), "custom");
    assert_eq!(body["data"]["content"].as_str().unwrap(), custom_content);
}

#[tokio::test]
async fn update_workflow_empty_content_returns_400() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("wf_up2", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-empty", &token).await;

    let resp = app.put(
        &format!("/api/projects/{}/workflow", project_id),
        &serde_json::json!({
            "template_mode": "custom",
            "content": ""
        }),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_workflow_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("wf_up_own", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-mem-403", &owner_token).await;

    app.create_test_user("wf_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("wf_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("wf_mem", "Pass123").await;
    let resp = app.put(
        &format!("/api/projects/{}/workflow", project_id),
        &serde_json::json!({
            "template_mode": "custom",
            "content": "hacked"
        }),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn update_workflow_missing_fields_returns_400() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("wf_up3", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-missing", &token).await;

    let resp = app.put(
        &format!("/api/projects/{}/workflow", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ============================================================
// POST /api/projects/:id/workflow/reset - Reset Workflow
// ============================================================

#[tokio::test]
async fn reset_workflow_to_default() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("wf_reset1", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-reset", &token).await;

    // First set custom
    app.put(
        &format!("/api/projects/{}/workflow", project_id),
        &serde_json::json!({
            "template_mode": "custom",
            "content": "custom stuff"
        }),
        Some(&token),
    ).await;

    // Reset
    let resp = app.post(
        &format!("/api/projects/{}/workflow/reset", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify back to default
    let get_resp = app.get(
        &format!("/api/projects/{}/workflow", project_id),
        Some(&token),
    ).await;
    let body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(body["data"]["template_mode"].as_str().unwrap(), "default");
}

#[tokio::test]
async fn reset_workflow_member_returns_403() {
    let app = TestApp::new().await;
    let owner_token = app.create_user_get_token("wf_reset_own", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/wf-reset-403", &owner_token).await;

    app.create_test_user("wf_reset_mem", "Pass123", "user").await;
    let mem_id = app.get_user_id("wf_reset_mem").await;
    app.add_project_member(project_id, mem_id, "member", &owner_token).await;

    let member_token = app.login_get_token("wf_reset_mem", "Pass123").await;
    let resp = app.post(
        &format!("/api/projects/{}/workflow/reset", project_id),
        &serde_json::json!({}),
        Some(&member_token),
    ).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn reset_workflow_no_token_returns_401() {
    let app = TestApp::new().await;
    let resp = app.post(
        "/api/projects/1/workflow/reset",
        &serde_json::json!({}),
        None,
    ).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

### 3.4 E2E Tests

#### 3.4.1 Full Project Lifecycle (tests/e2e/project_lifecycle.rs)

```rust
mod common;

use reqwest::StatusCode;
use crate::common::TestApp;

#[tokio::test]
async fn test_e2e_project_full_lifecycle() {
    let app = TestApp::new().await;

    // 1. Create user and login
    let token = app.create_user_get_token("lifecycle_user", "Pass123", "user").await;

    // 2. Create project
    let create_resp = app.post(
        "/api/projects",
        &serde_json::json!({
            "git_url": "https://github.com/owner/lifecycle-project",
            "name": "Lifecycle Test",
            "description": "Full lifecycle test"
        }),
        Some(&token),
    ).await;
    assert_eq!(create_resp.status(), StatusCode::OK);
    let body: serde_json::Value = create_resp.json().await.unwrap();
    let project_id = body["data"]["id"].as_i64().unwrap();

    // 3. Verify project appears in list
    let list_resp = app.get("/api/projects", Some(&token)).await;
    let list_body: serde_json::Value = list_resp.json().await.unwrap();
    let records = list_body["data"]["records"].as_array().unwrap();
    assert!(records.iter().any(|r| r["id"].as_i64().unwrap() == project_id));

    // 4. Update project configuration
    let update_resp = app.put(
        &format!("/api/projects/{}", project_id),
        &serde_json::json!({
            "max_concurrent_agents": 3,
            "auto_restart": true
        }),
        Some(&token),
    ).await;
    assert_eq!(update_resp.status(), StatusCode::OK);

    // 5. Start service
    let start_resp = app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(start_resp.status(), StatusCode::OK);

    // 6. Check status is running
    let status_resp = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&token),
    ).await;
    let status_body: serde_json::Value = status_resp.json().await.unwrap();
    assert_eq!(status_body["data"]["status"].as_str().unwrap(), "running");

    // 7. Stop service
    let stop_resp = app.post(
        &format!("/api/projects/{}/stop", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;
    assert_eq!(stop_resp.status(), StatusCode::OK);

    // 8. Verify stopped
    let status_resp2 = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&token),
    ).await;
    let status_body2: serde_json::Value = status_resp2.json().await.unwrap();
    assert_eq!(status_body2["data"]["status"].as_str().unwrap(), "stopped");

    // 9. Delete project
    let del_resp = app.delete(
        &format!("/api/projects/{}", project_id),
        Some(&token),
    ).await;
    assert_eq!(del_resp.status(), StatusCode::OK);

    // 10. Verify deleted
    let get_resp = app.get(
        &format!("/api/projects/{}", project_id),
        Some(&token),
    ).await;
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_e2e_cannot_delete_running_project() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("nodelete_user", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/nodelete-proj", &token).await;

    // Start service
    app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    // Try delete - should fail
    let del_resp = app.delete(
        &format!("/api/projects/{}", project_id),
        Some(&token),
    ).await;
    assert_eq!(del_resp.status(), StatusCode::CONFLICT);

    // Stop first, then delete
    app.post(
        &format!("/api/projects/{}/stop", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    let del_resp2 = app.delete(
        &format!("/api/projects/{}", project_id),
        Some(&token),
    ).await;
    assert_eq!(del_resp2.status(), StatusCode::OK);
}
```

#### 3.4.2 Member Management Flow (tests/e2e/member_flow.rs)

```rust
mod common;

use reqwest::StatusCode;
use crate::common::TestApp;

#[tokio::test]
async fn test_e2e_member_management_flow() {
    let app = TestApp::new().await;

    // Setup: owner creates project
    let owner_token = app.create_user_get_token("mem_flow_owner", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/mem-flow", &owner_token).await;

    // Create users to add as members
    app.create_test_user("mem_flow_user1", "Pass123", "user").await;
    app.create_test_user("mem_flow_user2", "Pass123", "user").await;
    let user1_id = app.get_user_id("mem_flow_user1").await;
    let user2_id = app.get_user_id("mem_flow_user2").await;

    // 1. Add member
    let add_resp = app.post(
        &format!("/api/projects/{}/members", project_id),
        &serde_json::json!({ "user_id": user1_id, "role": "member" }),
        Some(&owner_token),
    ).await;
    assert_eq!(add_resp.status(), StatusCode::OK);

    // 2. Verify member can see project
    let user1_token = app.login_get_token("mem_flow_user1", "Pass123").await;
    let proj_resp = app.get(
        &format!("/api/projects/{}", project_id),
        Some(&user1_token),
    ).await;
    assert_eq!(proj_resp.status(), StatusCode::OK);

    // 3. Change role to owner
    let role_resp = app.put(
        &format!("/api/projects/{}/members/{}", project_id, user1_id),
        &serde_json::json!({ "role": "owner" }),
        Some(&owner_token),
    ).await;
    assert_eq!(role_resp.status(), StatusCode::OK);

    // 4. New owner can now add members
    let add_resp2 = app.post(
        &format!("/api/projects/{}/members", project_id),
        &serde_json::json!({ "user_id": user2_id, "role": "member" }),
        Some(&user1_token),
    ).await;
    assert_eq!(add_resp2.status(), StatusCode::OK);

    // 5. Verify member list
    let list_resp = app.get(
        &format!("/api/projects/{}/members", project_id),
        Some(&owner_token),
    ).await;
    let body: serde_json::Value = list_resp.json().await.unwrap();
    let members = body["data"].as_array().unwrap();
    assert_eq!(members.len(), 3); // original owner + user1 + user2

    // 6. Remove member
    let rm_resp = app.delete(
        &format!("/api/projects/{}/members/{}", project_id, user2_id),
        Some(&owner_token),
    ).await;
    assert_eq!(rm_resp.status(), StatusCode::OK);

    // 7. Removed member cannot access project
    let user2_token = app.login_get_token("mem_flow_user2", "Pass123").await;
    let access_resp = app.get(
        &format!("/api/projects/{}", project_id),
        Some(&user2_token),
    ).await;
    assert_eq!(access_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_e2e_member_visibility_isolation() {
    let app = TestApp::new().await;

    // Two users create their own projects
    let user1_token = app.create_user_get_token("iso_user1", "Pass123", "user").await;
    let user2_token = app.create_user_get_token("iso_user2", "Pass123", "user").await;

    let proj1_id = app.create_project("https://github.com/owner/iso-proj1", &user1_token).await;
    let proj2_id = app.create_project("https://github.com/owner/iso-proj2", &user2_token).await;

    // User1 can only see their project
    let list1 = app.get("/api/projects", Some(&user1_token)).await;
    let body1: serde_json::Value = list1.json().await.unwrap();
    let records1 = body1["data"]["records"].as_array().unwrap();
    assert_eq!(records1.len(), 1);
    assert_eq!(records1[0]["id"].as_i64().unwrap(), proj1_id);

    // User2 can only see their project
    let list2 = app.get("/api/projects", Some(&user2_token)).await;
    let body2: serde_json::Value = list2.json().await.unwrap();
    let records2 = body2["data"]["records"].as_array().unwrap();
    assert_eq!(records2.len(), 1);
    assert_eq!(records2[0]["id"].as_i64().unwrap(), proj2_id);

    // Admin sees both
    let list_admin = app.get("/api/projects", Some(&app.admin_token)).await;
    let body_admin: serde_json::Value = list_admin.json().await.unwrap();
    let records_admin = body_admin["data"]["records"].as_array().unwrap();
    assert!(records_admin.len() >= 2);
}
```

#### 3.4.3 Concurrent Operations (tests/e2e/concurrent_ops.rs)

```rust
mod common;

use reqwest::StatusCode;
use crate::common::TestApp;

#[tokio::test]
async fn test_e2e_concurrent_start_stop_mutex() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("conc_user", "Pass123", "user").await;
    let project_id = app.create_project("https://github.com/owner/conc-proj", &token).await;

    // Start service
    app.post(
        &format!("/api/projects/{}/start", project_id),
        &serde_json::json!({}),
        Some(&token),
    ).await;

    // Concurrent stop and start - should not panic or corrupt state
    let app_clone = app.client.clone();
    let url_stop = app.url(&format!("/api/projects/{}/stop", project_id));
    let url_start = app.url(&format!("/api/projects/{}/start", project_id));
    let token_clone = token.clone();

    let (r1, r2) = tokio::join!(
        async {
            app_clone
                .post(&url_stop)
                .header("Authorization", format!("Bearer {}", token_clone))
                .json(&serde_json::json!({}))
                .send()
                .await
                .unwrap()
        },
        async {
            app_clone
                .post(&url_start)
                .header("Authorization", format!("Bearer {}", token))
                .json(&serde_json::json!({}))
                .send()
                .await
                .unwrap()
        },
    );

    // At least one should succeed, the other may get conflict
    let statuses = vec![r1.status(), r2.status()];
    assert!(
        statuses.contains(&StatusCode::OK) || statuses.contains(&StatusCode::CONFLICT),
        "Expected at least one OK or CONFLICT, got: {:?}", statuses
    );

    // Final state should be consistent (either running or stopped, not corrupted)
    let status_resp = app.get(
        &format!("/api/projects/{}/status", project_id),
        Some(&token),
    ).await;
    let body: serde_json::Value = status_resp.json().await.unwrap();
    let status = body["data"]["status"].as_str().unwrap();
    assert!(status == "running" || status == "stopped" || status == "starting" || status == "stopping");
}

#[tokio::test]
async fn test_e2e_concurrent_project_creation_unique_urls() {
    let app = TestApp::new().await;
    let token = app.create_user_get_token("conc_create", "Pass123", "user").await;

    let url = app.url("/api/projects");
    let client = app.client.clone();
    let token_clone = token.clone();

    // Try to create same project concurrently
    let (r1, r2) = tokio::join!(
        async {
            client
                .post(&url)
                .header("Authorization", format!("Bearer {}", token_clone))
                .json(&serde_json::json!({ "git_url": "https://github.com/owner/race-proj" }))
                .send()
                .await
                .unwrap()
        },
        async {
            client
                .post(&url)
                .header("Authorization", format!("Bearer {}", token))
                .json(&serde_json::json!({ "git_url": "https://github.com/owner/race-proj" }))
                .send()
                .await
                .unwrap()
        },
    );

    // One should succeed (200), one should get conflict (409)
    let statuses = vec![r1.status(), r2.status()];
    assert!(statuses.contains(&StatusCode::OK));
    assert!(statuses.contains(&StatusCode::CONFLICT));
}
```

---

## 4. GitLab CI Pipeline Configuration

### .gitlab-ci.yml

```yaml
stages:
  - lint
  - test
  - report

variables:
  CARGO_HOME: "${CI_PROJECT_DIR}/.cargo"
  RUSTFLAGS: "-D warnings"

cache:
  key: "${CI_COMMIT_REF_SLUG}"
  paths:
    - .cargo/bin/
    - .cargo/registry/index/
    - .cargo/registry/cache/
    - .cargo/git/db/
    - target/

# ============================================================
# Lint Stage
# ============================================================

clippy:
  stage: lint
  image: rust:1.77
  script:
    - rustup component add clippy
    - cargo clippy -p web-platform --all-targets -- -D warnings
  rules:
    - changes:
        - web-platform/**/*

fmt-check:
  stage: lint
  image: rust:1.77
  script:
    - rustup component add rustfmt
    - cargo fmt -p web-platform -- --check
  rules:
    - changes:
        - web-platform/**/*

# ============================================================
# Test Stage
# ============================================================

unit-tests:
  stage: test
  image: rust:1.77
  script:
    - cargo test -p web-platform --lib -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/test-results/unit.xml
  rules:
    - changes:
        - web-platform/**/*

integration-tests:
  stage: test
  image: rust:1.77
  script:
    - cargo test -p web-platform --test 'integration_*' -- --nocapture --test-threads=1
  artifacts:
    when: always
    reports:
      junit: target/test-results/integration.xml
  rules:
    - changes:
        - web-platform/**/*

api-tests:
  stage: test
  image: rust:1.77
  script:
    - cargo test -p web-platform --test 'api_*' -- --nocapture --test-threads=4
  artifacts:
    when: always
    reports:
      junit: target/test-results/api.xml
  rules:
    - changes:
        - web-platform/**/*

e2e-tests:
  stage: test
  image: rust:1.77
  script:
    - cargo test -p web-platform --test 'e2e*' -- --nocapture --test-threads=1
  artifacts:
    when: always
    reports:
      junit: target/test-results/e2e.xml
  rules:
    - changes:
        - web-platform/**/*

# ============================================================
# Report Stage
# ============================================================

coverage:
  stage: report
  image: rust:1.77
  script:
    - cargo install cargo-tarpaulin || true
    - cargo tarpaulin -p web-platform --out xml --output-dir target/coverage/
  coverage: '/^\d+.\d+% coverage/'
  artifacts:
    reports:
      coverage_report:
        coverage_format: cobertura
        path: target/coverage/cobertura.xml
  rules:
    - changes:
        - web-platform/**/*
  allow_failure: true
```

---

## 5. Test Execution Commands

### Run All Tests

```bash
# All tests (unit + integration + API + E2E)
cargo test -p web-platform

# With output
cargo test -p web-platform -- --nocapture
```

### Run by Category

```bash
# Unit tests only (in-module tests)
cargo test -p web-platform --lib

# Integration tests
cargo test -p web-platform --test repo_projects
cargo test -p web-platform --test repo_members
cargo test -p web-platform --test auth_project
cargo test -p web-platform --test process_manager

# API tests
cargo test -p web-platform --test api_projects
cargo test -p web-platform --test api_members
cargo test -p web-platform --test api_service
cargo test -p web-platform --test api_workflow

# E2E tests
cargo test -p web-platform --test project_lifecycle
cargo test -p web-platform --test member_flow
cargo test -p web-platform --test concurrent_ops
```

### Run Specific Test

```bash
# Single test by name
cargo test -p web-platform create_project_happy_path

# Tests matching pattern
cargo test -p web-platform -- --test-threads=1 "start_service"
```

### Sequential Execution (for tests with shared state)

```bash
# E2E and integration tests should run sequentially
cargo test -p web-platform --test e2e -- --test-threads=1
```

### Coverage Report

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage
cargo tarpaulin -p web-platform --out html --output-dir target/coverage/

# Open report
open target/coverage/tarpaulin-report.html
```

---

## 6. Test Summary Matrix

| Category | File | Test Count | Parallelizable |
|----------|------|-----------|----------------|
| Unit: Git URL | git_url_parser.rs | 22 | Yes |
| Unit: Workflow | workflow_template.rs | 6 | Yes |
| Unit: PID | pid_verify.rs | 4 | Yes |
| Unit: Status | service_status.rs | 11 | Yes |
| Integration: Projects | repo_projects.rs | 9 | No (shared DB) |
| Integration: Members | repo_members.rs | 8 | No (shared DB) |
| Integration: Auth | auth_project.rs | 5 | Yes (per-test DB) |
| Integration: Process | process_manager.rs | 7 | Yes (mocked) |
| API: Projects | api_projects.rs | 18 | Yes (per-test server) |
| API: Members | api_members.rs | 14 | Yes (per-test server) |
| API: Service | api_service.rs | 14 | Yes (per-test server) |
| API: Workflow | api_workflow.rs | 9 | Yes (per-test server) |
| E2E: Lifecycle | project_lifecycle.rs | 2 | No |
| E2E: Members | member_flow.rs | 2 | No |
| E2E: Concurrent | concurrent_ops.rs | 2 | No |
| **Total** | | **133** | |

---

## 7. Key Design Decisions

1. **Per-test database isolation**: Each `TestApp::new()` creates a fresh in-memory SQLite database via `TempDir`. This eliminates test interdependence and allows parallel execution of API tests.

2. **Real HTTP server per test**: API tests spin up a real Axum server on a random port. This tests the full middleware stack (auth, rate limiting, CORS) rather than just handler logic.

3. **Mock process spawner**: Process management tests use `MockProcessSpawner` to avoid spawning real subprocesses. This makes tests fast, deterministic, and CI-friendly.

4. **Layered test strategy**: Unit tests validate pure logic (parsing, state machines). Integration tests validate repository operations against real SQLite. API tests validate the full HTTP contract. E2E tests validate multi-step business flows.

5. **Explicit permission matrix testing**: Every endpoint is tested with all relevant permission levels (admin, owner, member, non-member, unauthenticated) to ensure the authorization model is correct.

6. **GitLab CI with caching**: The pipeline caches Cargo dependencies between runs and splits tests into parallel jobs by category for faster feedback.
