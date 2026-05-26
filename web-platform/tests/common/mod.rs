use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use reqwest::{Client, Response};
use serde::Serialize;
use tempfile::TempDir;
use tokio::net::TcpListener;

use web_platform::auth::password::hash_password;
use web_platform::auth::rate_limit::RateLimiter;
use web_platform::concurrency::ConcurrencyManager;
use web_platform::crypto;
use web_platform::db::init_pool;
use web_platform::process_manager::ProcessManager;
use web_platform::repository::{SqliteRepository, UserConfigRepository, UserRepository};
use web_platform::router::{create_router, create_router_with_static_dir};
use web_platform::services::cache::ApiCache;
use web_platform::{AppState, Phase3RateLimiter};

#[allow(dead_code)]
pub struct TestApp {
    pub addr: String,
    pub client: Client,
    pub admin_token: String,
    pub db_path: PathBuf,
    pub repo: SqliteRepository,
    pub process_manager: ProcessManager,
    _dir: TempDir,
}

#[allow(dead_code)]
impl TestApp {
    pub async fn new() -> Self {
        Self::new_with_symphony_bin("/usr/bin/false").await
    }

    pub async fn new_with_symphony_bin(symphony_bin: &str) -> Self {
        Self::new_with_symphony_bin_and_static_dir(symphony_bin, None).await
    }

    pub async fn new_with_static_dir(static_dir: PathBuf) -> Self {
        Self::new_with_symphony_bin_and_static_dir("/usr/bin/false", Some(static_dir)).await
    }

    async fn new_with_symphony_bin_and_static_dir(
        symphony_bin: &str,
        static_dir: Option<PathBuf>,
    ) -> Self {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let pool = init_pool(db_path.to_str().unwrap());
        let repo = SqliteRepository::new(pool);

        let admin_hash = hash_password("admin123").unwrap();
        let admin = repo
            .create_user("admin", &admin_hash, Some("Administrator"), "admin")
            .await
            .unwrap();
        let encryption_key = [0x42u8; 32];
        let github_token = crypto::encrypt("test-github-token", &encryption_key).unwrap();
        let gitlab_token = crypto::encrypt("test-gitlab-token", &encryption_key).unwrap();
        repo.upsert_config(
            admin.id,
            Some(&gitlab_token),
            Some("https://gitlab.com"),
            Some(&github_token),
        )
        .await
        .unwrap();
        let admin = repo.find_by_username("admin").await.unwrap().unwrap();
        let encrypted_github_token = crypto::encrypt("test-github-token", &[0x42u8; 32]).unwrap();
        repo.upsert_config(admin.id, None, None, Some(&encrypted_github_token))
            .await
            .unwrap();

        let process_manager = ProcessManager::new();
        let concurrency_manager = Arc::new(ConcurrencyManager::new(10));

        let state = AppState {
            repo,
            jwt_secret: "test-jwt-secret-key-at-least-32-characters-long".to_string(),
            encryption_key,
            token_blacklist: Arc::new(DashMap::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            process_manager: process_manager.clone(),
            api_cache: Arc::new(ApiCache::new(10, 3, 10000)),
            ai_service: None,
            phase3_rate_limiter: Arc::new(Phase3RateLimiter::new()),
            concurrency_manager: concurrency_manager.clone(),
            symphony_bin: symphony_bin.to_string(),
            workspace_root: dir.path().to_str().unwrap().to_string(),
            alert_manager: None,
        };

        let repo = state.repo.clone();
        let router = match static_dir {
            Some(static_dir) => create_router_with_static_dir(state, Some(static_dir)),
            None => create_router(state),
        };
        let app = router.into_make_service_with_connect_info::<SocketAddr>();

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
            db_path,
            repo,
            process_manager,
            _dir: dir,
        }
    }

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

    /// Create a test project and return the full response body.
    pub async fn create_test_project(&self, git_url: &str, token: &str) -> serde_json::Value {
        let resp = self
            .post(
                "/api/projects",
                &serde_json::json!({
                    "git_url": git_url
                }),
                Some(token),
            )
            .await;
        resp.json().await.unwrap()
    }

    /// Extract the project ID from a create-project response body.
    pub fn get_project_id(&self, response: &serde_json::Value) -> i64 {
        response["data"]["id"].as_i64().unwrap()
    }

    /// Get the user ID for a given username by searching the admin users list.
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
}
