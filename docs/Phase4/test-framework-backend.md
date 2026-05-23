# Phase 4 后端测试框架设计

## 概述

本文档定义 Phase 4（协作与控制）后端的完整测试策略，覆盖单元测试、集成测试、API 契约测试和 E2E 测试。

## GitLab CI 信息

- GitLab URL: http://gitlab.jushuitan-inc.com:8081/
- 项目: /zimei10525/symphony_e2e_test_repo
- GITLAB_TOKEN: gitlab-token-example

---

## 1. 单元测试

### 1.1 目录结构与约定

```
web-platform/src/
├── concurrency/
│   ├── mod.rs          # 内含 #[cfg(test)] mod tests
│   └── manager.rs      # 内含 #[cfg(test)] mod tests
├── handlers/
│   ├── concurrency.rs  # handler 逻辑测试
│   └── token_validation.rs
└── ...
```

约定：
- 每个模块文件底部添加 `#[cfg(test)] mod tests { ... }`
- 测试函数命名：`test_<功能>_<场景>_<预期结果>`
- 使用 `mockall` crate mock repository traits

### 1.2 Mock 策略

```rust
use mockall::automock;

#[automock]
#[async_trait]
pub trait ConcurrencyRepository: Send + Sync {
    async fn record_event(&self, event: &ConcurrencyEvent) -> Result<()>;
    async fn save_snapshot(&self, snapshot: &ConcurrencySnapshot) -> Result<()>;
    async fn load_snapshots(&self) -> Result<Vec<ConcurrencySnapshot>>;
    async fn get_config(&self, key: &str) -> Result<Option<String>>;
    async fn update_config(&self, key: &str, value: &str) -> Result<()>;
}
```

### 1.3 ConcurrencyManager 单元测试示例

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn test_add_agent_increments_global_count() {
        let manager = ConcurrencyManager::new(10);
        manager.report_project_agents(1, 3);
        assert_eq!(manager.global_active.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn test_global_limit_reached_returns_error() {
        let manager = ConcurrencyManager::new(5);
        manager.report_project_agents(1, 5);
        assert!(manager.check_can_schedule(1).is_err());
    }

    #[tokio::test]
    async fn test_project_limit_respected() {
        let manager = ConcurrencyManager::new(10);
        manager.set_project_limit(1, 2);
        manager.report_project_agents(1, 2);
        assert!(manager.check_can_schedule_project(1).is_err());
    }

    #[tokio::test]
    async fn test_remove_project_decrements_global() {
        let manager = ConcurrencyManager::new(10);
        manager.report_project_agents(1, 3);
        manager.report_project_agents(2, 2);
        assert_eq!(manager.global_active.load(Ordering::Relaxed), 5);
        manager.report_project_agents(1, 0);
        assert_eq!(manager.global_active.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn test_sse_ticket_generation_and_validation() {
        let manager = ConcurrencyManager::new(10);
        let ticket = manager.generate_ticket(42);
        assert!(manager.validate_ticket(&ticket).is_some());
        // Second use should fail (one-time)
        assert!(manager.validate_ticket(&ticket).is_none());
    }

    #[tokio::test]
    async fn test_sse_ticket_expires_after_30s() {
        let manager = ConcurrencyManager::new(10);
        let ticket = manager.generate_ticket_with_expiry(42, chrono::Duration::seconds(-1));
        assert!(manager.validate_ticket(&ticket).is_none());
    }
}
```

### 1.4 Token 验证时序测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_validate_token_minimum_response_time() {
        let start = Instant::now();
        // 模拟一个立即返回的 platform client
        let mock_client = MockGitPlatformClient::new();
        mock_client.expect_validate_token()
            .returning(|_| Ok(TokenValidationResult::Valid { username: "test".into() }));

        let result = validate_token_with_timing(mock_client, "fake-token", "gitlab").await;
        let elapsed = start.elapsed();

        assert!(elapsed >= Duration::from_millis(500), "Response must take at least 500ms");
        assert!(result.is_ok());
    }
}
```

---

## 2. 集成测试

### 2.1 目录结构

```
web-platform/tests/
├── common/
│   ├── mod.rs          # 共享 test helpers
│   ├── db.rs           # 测试数据库 setup/teardown
│   ├── auth.rs         # JWT 生成 helper
│   └── fixtures.rs     # 测试数据工厂
├── phase4_concurrency.rs
├── phase4_token_isolation.rs
└── phase4_author_filter.rs
```

### 2.2 测试数据库 Helper

```rust
// tests/common/db.rs
use web_platform::db;
use web_platform::repository::SqliteRepository;

pub async fn setup_test_db() -> SqliteRepository {
    let repo = SqliteRepository::new_in_memory().await.unwrap();
    repo.run_migrations().await.unwrap();
    seed_test_data(&repo).await;
    repo
}

async fn seed_test_data(repo: &SqliteRepository) {
    // 创建 admin 用户
    repo.create_user("admin", &hash_password("admin123"), Some("Admin"), "admin").await.unwrap();
    // 创建普通用户
    repo.create_user("user1", &hash_password("user123"), Some("User One"), "user").await.unwrap();
    repo.create_user("user2", &hash_password("user123"), Some("User Two"), "user").await.unwrap();
    // 创建项目
    repo.create_project(&NewProject {
        name: "test-project".into(),
        git_url: "https://gitlab.com/test/repo".into(),
        platform: "gitlab".into(),
        ..Default::default()
    }).await.unwrap();
}
```

### 2.3 JWT Helper

```rust
// tests/common/auth.rs
use web_platform::auth::jwt::{Claims, create_token};

pub fn admin_token(secret: &str) -> String {
    create_token(1, "admin", "admin", secret).unwrap()
}

pub fn user_token(user_id: i64, secret: &str) -> String {
    create_token(user_id, &format!("user{}", user_id), "user", secret).unwrap()
}
```

### 2.4 集成测试示例

```rust
// tests/phase4_concurrency.rs
mod common;

use axum::http::StatusCode;
use axum_test::TestServer;
use web_platform::router::create_router;

#[tokio::test]
async fn test_get_global_concurrency_as_admin() {
    let (state, server) = common::setup_server().await;
    let token = common::auth::admin_token(&state.jwt_secret);

    let resp = server
        .get("/api/admin/concurrency")
        .add_header("Authorization", &format!("Bearer {}", token))
        .await;

    assert_eq!(resp.status_code(), StatusCode::OK);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["success"], true);
    assert!(body["data"]["global_max"].is_number());
    assert!(body["data"]["global_active"].is_number());
}

#[tokio::test]
async fn test_get_global_concurrency_as_user_denied() {
    let (state, server) = common::setup_server().await;
    let token = common::auth::user_token(2, &state.jwt_secret);

    let resp = server
        .get("/api/admin/concurrency")
        .add_header("Authorization", &format!("Bearer {}", token))
        .await;

    assert_eq!(resp.status_code(), StatusCode::FORBIDDEN);
}
```

---

## 3. API 契约测试

### 3.1 端点测试矩阵

#### POST /api/user/config/validate-token

| 场景 | 输入 | 预期状态码 | 预期 retCode |
|------|------|-----------|-------------|
| 有效 GitLab Token | `{"platform":"gitlab","token":"valid"}` | 200 | 0 |
| 无效 Token | `{"platform":"gitlab","token":"invalid"}` | 200 | 0 (data.valid=false) |
| 缺少 platform 字段 | `{"token":"xxx"}` | 400 | BIZ_001 |
| 未认证 | 无 Authorization header | 401 | AUTH_001 |
| 速率限制（第4次/分钟） | 连续4次请求 | 429 | EXT_002 |
| 响应时间 >= 500ms | 任意请求 | 200 | - (验证时间) |

#### GET /api/admin/concurrency

| 场景 | 条件 | 预期状态码 | 预期 retCode |
|------|------|-----------|-------------|
| Admin 正常获取 | admin token | 200 | 0 |
| 非 admin 拒绝 | user token | 403 | AUTH_002 |
| 无 token | 无 header | 401 | AUTH_001 |
| 有运行中实例 | 模拟 agent 活跃 | 200 | data.global_active > 0 |
| 无运行实例 | 初始状态 | 200 | data.global_active = 0 |

#### PUT /api/admin/concurrency/config

| 场景 | 输入 | 预期状态码 | 预期 retCode |
|------|------|-----------|-------------|
| 正常更新 | `{"global_max":10}` | 200 | 0 |
| 值过小 | `{"global_max":0}` | 400 | BIZ_001 |
| 值过大 | `{"global_max":1000}` | 400 | BIZ_001 |
| 乐观锁冲突 | `{"global_max":10,"expected_previous":5}` 但当前=8 | 409 | BIZ_003 |
| 非 admin | user token | 403 | AUTH_002 |

#### GET /api/projects/:id/concurrency

| 场景 | 条件 | 预期状态码 | 预期 retCode |
|------|------|-----------|-------------|
| 项目成员获取 | member token | 200 | 0 |
| 非成员拒绝 | non-member token | 403 | AUTH_002 |
| 项目不存在 | id=99999 | 404 | BIZ_002 |
| Admin 可访问任意项目 | admin token | 200 | 0 |

#### PUT /api/projects/:id/concurrency

| 场景 | 输入 | 预期状态码 | 预期 retCode |
|------|------|-----------|-------------|
| Owner 更新 | `{"max_agents":3}` | 200 | 0 |
| Member 拒绝 | member token | 403 | AUTH_002 |
| 超过全局限制 | `{"max_agents":100}` (全局max=10) | 400 | BIZ_001 |

#### GET /api/projects/:id/contributors

| 场景 | 条件 | 预期状态码 | 预期 retCode |
|------|------|-----------|-------------|
| 正常获取 | member token, 有 issues | 200 | 0 |
| 空项目 | 无 issues/MRs | 200 | data.contributors=[] |
| Token 未配置 | 用户无 platform token | 400 | TOKEN_001 |
| 缓存命中 | 30s 内重复请求 | 200 | data.cached=true |

#### GET /api/projects/:id/kanban?author=xxx

| 场景 | 条件 | 预期状态码 | 说明 |
|------|------|-----------|------|
| 按作者过滤 | `?author=john` | 200 | 只返回 john 的 issues |
| 作者不存在 | `?author=nobody` | 200 | 返回空列表 |
| 与其他过滤组合 | `?author=john&labels=bug` | 200 | 两个条件 AND |
| 缓存 key 包含 author | 不同 author 不共享缓存 | 200 | 验证隔离 |

#### POST /api/admin/concurrency/events/ticket

| 场景 | 条件 | 预期状态码 | 说明 |
|------|------|-----------|------|
| Admin 获取 ticket | admin token | 200 | 返回 ticket + expires_at |
| 非 admin 拒绝 | user token | 403 | AUTH_002 |
| Ticket 单次使用 | 使用后再次验证 | - | 第二次无效 |
| Ticket 30s 过期 | 等待过期后使用 | - | 连接被拒 |

### 3.2 API 测试代码示例

```rust
// tests/phase4_api_contracts.rs
mod common;

use serde_json::json;

#[tokio::test]
async fn test_validate_token_happy_path() {
    let (state, server) = common::setup_server().await;
    let token = common::auth::user_token(2, &state.jwt_secret);

    let resp = server
        .post("/api/user/config/validate-token")
        .add_header("Authorization", &format!("Bearer {}", token))
        .json(&json!({
            "platform": "gitlab",
            "token": "gitlab-token-example"
        }))
        .await;

    assert_eq!(resp.status_code(), 200);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["success"], true);
    assert!(body["data"]["valid"].is_boolean());
}

#[tokio::test]
async fn test_validate_token_rate_limit() {
    let (state, server) = common::setup_server().await;
    let token = common::auth::user_token(2, &state.jwt_secret);

    // 发送 3 次（限制为 3/min）
    for _ in 0..3 {
        server.post("/api/user/config/validate-token")
            .add_header("Authorization", &format!("Bearer {}", token))
            .json(&json!({"platform": "gitlab", "token": "test"}))
            .await;
    }

    // 第 4 次应被限流
    let resp = server
        .post("/api/user/config/validate-token")
        .add_header("Authorization", &format!("Bearer {}", token))
        .json(&json!({"platform": "gitlab", "token": "test"}))
        .await;

    assert_eq!(resp.status_code(), 429);
}

#[tokio::test]
async fn test_concurrency_config_optimistic_lock() {
    let (state, server) = common::setup_server().await;
    let token = common::auth::admin_token(&state.jwt_secret);

    // 设置初始值为 5
    server.put("/api/admin/concurrency/config")
        .add_header("Authorization", &format!("Bearer {}", token))
        .json(&json!({"global_max": 5}))
        .await;

    // 尝试用错误的 expected_previous 更新
    let resp = server
        .put("/api/admin/concurrency/config")
        .add_header("Authorization", &format!("Bearer {}", token))
        .json(&json!({"global_max": 10, "expected_previous": 3}))
        .await;

    assert_eq!(resp.status_code(), 409);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["retCode"], "BIZ_003");
}
```

---

## 4. E2E 测试

### 4.1 多用户 Token 隔离场景

```rust
#[tokio::test]
async fn test_e2e_multi_user_token_isolation() {
    let (state, server) = common::setup_server().await;

    // User1 配置 GitLab Token
    let user1_token = common::auth::user_token(2, &state.jwt_secret);
    server.put("/api/user/config")
        .add_header("Authorization", &format!("Bearer {}", user1_token))
        .json(&json!({"gitlab_token": "user1-gitlab-token", "gitlab_host": "gitlab.com"}))
        .await;

    // User2 配置不同的 Token
    let user2_token = common::auth::user_token(3, &state.jwt_secret);
    server.put("/api/user/config")
        .add_header("Authorization", &format!("Bearer {}", user2_token))
        .json(&json!({"gitlab_token": "user2-gitlab-token", "gitlab_host": "gitlab.com"}))
        .await;

    // 验证 User1 看板使用 User1 的 Token（通过 mock 验证调用参数）
    // 验证 User2 看板使用 User2 的 Token
    // 验证服务启动使用 Owner Token
}
```

### 4.2 并行控制强制执行

```rust
#[tokio::test]
async fn test_e2e_concurrency_limit_enforcement() {
    let (state, server) = common::setup_server().await;
    let admin_token = common::auth::admin_token(&state.jwt_secret);

    // 设置全局限制为 2
    server.put("/api/admin/concurrency/config")
        .add_header("Authorization", &format!("Bearer {}", admin_token))
        .json(&json!({"global_max": 2}))
        .await;

    // 模拟 2 个 agent 活跃
    state.concurrency_manager.report_project_agents(1, 2);

    // 尝试启动新服务应被拒绝
    let resp = server.post("/api/projects/1/start")
        .add_header("Authorization", &format!("Bearer {}", admin_token))
        .await;

    assert_eq!(resp.status_code(), 429);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["retCode"], "CONCURRENCY_001");
}
```

---

## 5. CI Pipeline

### 5.1 .gitlab-ci.yml

```yaml
stages:
  - lint
  - test
  - integration
  - api-test
  - e2e

variables:
  CARGO_HOME: ${CI_PROJECT_DIR}/.cargo
  RUSTFLAGS: "-D warnings"
  DATABASE_URL: "sqlite://:memory:"
  JWT_SECRET: "test-secret-at-least-32-characters-long"
  ENCRYPTION_KEY: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

cache:
  key: ${CI_COMMIT_REF_SLUG}
  paths:
    - .cargo/
    - target/

# ==================== Lint ====================
clippy:
  stage: lint
  image: rust:1.78
  script:
    - rustup component add clippy
    - cargo clippy -p web-platform --all-targets -- -D warnings
  rules:
    - changes:
        - web-platform/**/*

fmt-check:
  stage: lint
  image: rust:1.78
  script:
    - rustup component add rustfmt
    - cargo fmt -p web-platform -- --check
  rules:
    - changes:
        - web-platform/**/*

# ==================== Unit Tests ====================
unit-tests:
  stage: test
  image: rust:1.78
  script:
    - cargo test -p web-platform --lib -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml

# ==================== Integration Tests ====================
integration-tests:
  stage: integration
  image: rust:1.78
  script:
    - cargo test -p web-platform --test '*' -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml

# ==================== API Contract Tests ====================
api-tests:
  stage: api-test
  image: rust:1.78
  script:
    - cargo test -p web-platform --test phase4_api_contracts -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml

# ==================== E2E Tests ====================
e2e-tests:
  stage: e2e
  image: rust:1.78
  script:
    - cargo test -p web-platform --test phase4_e2e -- --nocapture
  artifacts:
    when: always
    reports:
      junit: target/nextest/ci/junit.xml

# ==================== Coverage ====================
coverage:
  stage: test
  image: rust:1.78
  script:
    - cargo install cargo-tarpaulin
    - cargo tarpaulin -p web-platform --out xml --output-dir coverage/
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
```

### 5.2 覆盖率目标

| 模块 | 目标覆盖率 |
|------|-----------|
| concurrency/ | >= 85% |
| handlers/concurrency.rs | >= 80% |
| handlers/token_validation.rs | >= 90% |
| repository (concurrency 部分) | >= 75% |

### 5.3 本地运行

```bash
# 运行所有 Phase 4 测试
cargo test -p web-platform -- phase4

# 运行特定测试文件
cargo test -p web-platform --test phase4_concurrency

# 带覆盖率
cargo tarpaulin -p web-platform --out html
```
