# 后端测试框架设计

## 1. 测试工具选型

| 层级 | 工具 | 用途 |
|------|------|------|
| 单元测试 | Rust 内置 `#[cfg(test)]` + `#[tokio::test]` | 模块内部逻辑测试 |
| Mock | `mockall` 0.13 | trait mock，隔离依赖 |
| HTTP Mock | `wiremock` 0.6 | 外部 HTTP 服务模拟 |
| 集成测试 | `tests/` 目录 + `axum::test` | 跨模块集成验证 |
| HTTP 测试客户端 | `axum-test` 或 `reqwest` + 内存服务器 | API 端点测试 |
| 数据库 | `rusqlite` + `tempfile` | 临时 SQLite 实例 |
| 迁移 | `refinery` | 测试迁移正确性 |
| 断言 | `assert_matches` + `serde_json::json!` | 结构化断言 |
| 覆盖率 | `cargo-llvm-cov` | 代码覆盖率报告 |
| 性能基准 | `criterion` | 关键路径性能回归 |
| 快照测试 | `insta` | API 响应结构快照 |

## 2. 测试目录结构

```
web-management/
├── src/
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── jwt.rs          # JWT 生成/验证
│   │   ├── password.rs     # 密码哈希/验证
│   │   ├── rate_limit.rs   # 登录限流
│   │   └── tests.rs        # 单元测试（#[cfg(test)] mod tests）
│   ├── repository/
│   │   ├── mod.rs
│   │   ├── user_repo.rs
│   │   ├── config_repo.rs
│   │   └── tests.rs        # Repository 单元测试
│   ├── handler/
│   │   ├── mod.rs
│   │   ├── auth_handler.rs
│   │   ├── user_handler.rs
│   │   ├── admin_handler.rs
│   │   └── tests.rs        # Handler 单元测试（mock repo）
│   ├── middleware/
│   │   ├── mod.rs
│   │   ├── jwt_auth.rs
│   │   ├── role_check.rs
│   │   └── tests.rs
│   ├── model/
│   │   └── ...
│   └── lib.rs
├── tests/                   # 集成测试 + E2E 测试
│   ├── common/
│   │   ├── mod.rs           # 公共测试基础设施
│   │   ├── test_app.rs      # TestApp 封装
│   │   ├── test_db.rs       # TestDB 封装
│   │   └── fixtures.rs      # 测试数据工厂
│   ├── integration/
│   │   ├── mod.rs
│   │   ├── db_migration_test.rs
│   │   ├── user_repo_test.rs
│   │   ├── config_repo_test.rs
│   │   └── auth_integration_test.rs
│   ├── api/                 # 接口测试（每个端点独立文件）
│   │   ├── mod.rs
│   │   ├── auth_login_test.rs
│   │   ├── auth_password_test.rs
│   │   ├── user_profile_test.rs
│   │   ├── user_config_test.rs
│   │   ├── admin_users_test.rs
│   │   └── health_test.rs
│   └── e2e/
│       ├── mod.rs
│       ├── user_flow_test.rs
│       ├── admin_flow_test.rs
│       └── error_flow_test.rs
├── migrations/
│   ├── V001__create_users.sql
│   ├── V002__create_user_configs.sql
│   └── ...
└── Cargo.toml
```

## 3. 测试辅助工具

### 3.1 TestDB — 临时数据库封装

```rust
use refinery::embed_migrations;
use rusqlite::Connection;
use tempfile::TempDir;

embed_migrations!("migrations");

pub struct TestDB {
    pub conn: Connection,
    _dir: TempDir, // 保持 TempDir 存活，drop 时自动清理
}

impl TestDB {
    /// 创建临时 SQLite 数据库并运行所有迁移
    pub fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let mut conn = Connection::open(&db_path).unwrap();
        migrations::runner().run(&mut conn).unwrap();
        Self { conn, _dir: dir }
    }

    /// 创建带种子数据的数据库
    pub fn with_seed(seed_fn: impl FnOnce(&Connection)) -> Self {
        let db = Self::new();
        seed_fn(&db.conn);
        db
    }
}
```

### 3.2 TestApp — 完整应用封装

```rust
use axum::Router;
use axum_test::TestServer;

pub struct TestApp {
    pub server: TestServer,
    pub db: TestDB,
    pub admin_token: String,
    pub user_token: String,
}

impl TestApp {
    /// 启动完整应用（含数据库、路由、中间件）
    pub async fn new() -> Self {
        let db = TestDB::new();
        let app = build_app(db.pool()).await;
        let server = TestServer::new(app).unwrap();

        // 创建默认管理员并获取 token
        let admin_token = Self::create_admin_and_login(&server).await;
        let user_token = Self::create_user_and_login(&server, &admin_token).await;

        Self { server, db, admin_token, user_token }
    }

    /// 获取带认证头的请求
    pub fn auth_get(&self, path: &str, token: &str) -> RequestBuilder {
        self.server
            .get(path)
            .add_header("Authorization", format!("Bearer {}", token))
    }

    /// 获取带认证头的 POST 请求
    pub fn auth_post(&self, path: &str, token: &str) -> RequestBuilder {
        self.server
            .post(path)
            .add_header("Authorization", format!("Bearer {}", token))
    }
}
```

### 3.3 Fixtures — 测试数据工厂

```rust
pub struct UserFixture;

impl UserFixture {
    pub fn admin() -> CreateUserRequest {
        CreateUserRequest {
            username: "admin".to_string(),
            password: "Admin@123456".to_string(),
            display_name: Some("Administrator".to_string()),
            role: "admin".to_string(),
        }
    }

    pub fn regular_user(suffix: &str) -> CreateUserRequest {
        CreateUserRequest {
            username: format!("user_{}", suffix),
            password: "User@123456".to_string(),
            display_name: Some(format!("User {}", suffix)),
            role: "user".to_string(),
        }
    }

    pub fn invalid_username() -> CreateUserRequest {
        CreateUserRequest {
            username: "".to_string(), // 空用户名
            password: "Pass@123".to_string(),
            display_name: None,
            role: "user".to_string(),
        }
    }
}

pub struct TokenFixture;

impl TokenFixture {
    pub fn expired() -> String {
        // 生成一个已过期的 JWT token
        generate_jwt("test_user", Duration::from_secs(0))
    }

    pub fn invalid_signature() -> String {
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.invalid_sig".to_string()
    }

    pub fn none_algorithm() -> String {
        // alg: none 攻击向量
        "eyJhbGciOiJub25lIn0.eyJzdWIiOiJhZG1pbiJ9.".to_string()
    }
}
```

### 3.4 断言辅助宏

```rust
/// 断言 HTTP 响应状态码和 JSON body 结构
macro_rules! assert_api_error {
    ($response:expr, $status:expr, $error_code:expr) => {
        assert_eq!($response.status(), $status);
        let body: serde_json::Value = $response.json().await;
        assert_eq!(body["error"]["code"], $error_code);
    };
}

/// 断言分页响应结构
macro_rules! assert_paginated {
    ($response:expr, $total:expr) => {
        let body: serde_json::Value = $response.json().await;
        assert!(body["data"].is_array());
        assert_eq!(body["pagination"]["total"], $total);
    };
}
```

## 4. Mock 策略

### 4.1 Repository 层 Mock

使用 `mockall` 为 Repository trait 生成 mock：

```rust
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn find_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn find_by_id(&self, id: i64) -> Result<Option<User>>;
    async fn create(&self, req: &CreateUserRequest) -> Result<User>;
    async fn update(&self, id: i64, req: &UpdateUserRequest) -> Result<User>;
    async fn soft_delete(&self, id: i64) -> Result<()>;
    async fn list(&self, params: &ListParams) -> Result<Vec<User>>;
}
```

Handler 单元测试中注入 MockUserRepository：

```rust
#[tokio::test]
async fn test_login_success() {
    let mut mock_repo = MockUserRepository::new();
    mock_repo
        .expect_find_by_username()
        .with(eq("admin"))
        .returning(|_| Ok(Some(User { /* ... */ })));

    let handler = AuthHandler::new(Arc::new(mock_repo));
    let result = handler.login(LoginRequest { /* ... */ }).await;
    assert!(result.is_ok());
}
```

### 4.2 时间 Mock

Rate limit 测试需要控制时间：

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> chrono::DateTime<chrono::Utc>;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> { chrono::Utc::now() }
}

pub struct MockClock {
    pub current: std::sync::Mutex<chrono::DateTime<chrono::Utc>>,
}
impl MockClock {
    pub fn advance(&self, duration: chrono::Duration) {
        let mut t = self.current.lock().unwrap();
        *t = *t + duration;
    }
}
```

### 4.3 不 Mock 的部分

以下组件在集成测试中使用真实实现：
- SQLite 数据库（使用 tempfile 临时文件）
- 密码哈希（argon2，使用快速参数）
- JWT 签名/验证

## 5. 测试分层执行策略

```
┌─────────────────────────────────────────────────────────┐
│  Layer 1: 单元测试 (< 1s)                                │
│  cargo test --lib                                        │
│  - 纯逻辑测试，全部 mock                                  │
│  - 目标覆盖率: 90%+                                      │
├─────────────────────────────────────────────────────────┤
│  Layer 2: 集成测试 (< 10s)                               │
│  cargo test --test integration_*                         │
│  - 真实 SQLite + Repository                              │
│  - 目标覆盖率: 85%+                                      │
├─────────────────────────────────────────────────────────┤
│  Layer 3: API 测试 (< 30s)                               │
│  cargo test --test api_*                                 │
│  - 完整 HTTP 链路                                        │
│  - 每个端点的完整测试矩阵                                  │
├─────────────────────────────────────────────────────────┤
│  Layer 4: E2E 测试 (< 60s)                               │
│  cargo test --test e2e_*                                 │
│  - 多步骤用户流程                                         │
│  - 状态跨请求保持                                         │
└─────────────────────────────────────────────────────────┘
```

## 6. CI 集成方案

### 6.1 GitHub Actions Workflow

```yaml
name: Web Management Tests

on:
  pull_request:
    paths:
      - 'web-management/**'
  push:
    branches: [main]
    paths:
      - 'web-management/**'

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: web-management
      - name: Run unit tests
        working-directory: web-management
        run: cargo test --lib

  integration-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: web-management
      - name: Run integration tests
        working-directory: web-management
        run: cargo test --test 'integration_*'

  api-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: web-management
      - name: Run API tests
        working-directory: web-management
        run: cargo test --test 'api_*'

  e2e-tests:
    runs-on: ubuntu-latest
    needs: [integration-tests, api-tests]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: web-management
      - name: Run E2E tests
        working-directory: web-management
        run: cargo test --test 'e2e_*'

  coverage:
    runs-on: ubuntu-latest
    needs: [e2e-tests]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: taiki-e/install-action@cargo-llvm-cov
      - name: Generate coverage
        working-directory: web-management
        run: |
          cargo llvm-cov --all-features --workspace --lcov --output-path lcov.info
      - name: Upload coverage
        uses: codecov/codecov-action@v4
        with:
          files: web-management/lcov.info
          fail_ci_if_error: true
```

### 6.2 覆盖率门禁

| 模块 | 最低覆盖率 |
|------|-----------|
| auth/ | 95% |
| repository/ | 90% |
| handler/ | 85% |
| middleware/ | 90% |
| 整体 | 85% |

### 6.3 测试命名规范

```
test_{module}_{scenario}_{expected_result}
```

示例：
- `test_login_valid_credentials_returns_token`
- `test_login_wrong_password_returns_401`
- `test_admin_create_user_duplicate_username_returns_409`

## 7. 测试环境配置

### 7.1 Cargo.toml dev-dependencies

```toml
[dev-dependencies]
tokio-test = "0.4"
axum-test = "16"
mockall = "0.13"
wiremock = "0.6"
assert_matches = "1"
tempfile = "3"
serde_json = "1"
insta = { version = "1", features = ["json"] }
cargo-llvm-cov = "0.6"
fake = { version = "3", features = ["derive"] }
rand = "0.8"
```

### 7.2 测试专用配置

```rust
/// 测试环境使用快速 argon2 参数（降低 CI 耗时）
#[cfg(test)]
pub fn test_password_config() -> argon2::Config<'static> {
    argon2::Config {
        mem_cost: 1024,    // 生产环境: 65536
        time_cost: 1,      // 生产环境: 3
        lanes: 1,          // 生产环境: 4
        ..Default::default()
    }
}

/// 测试环境 JWT 密钥
#[cfg(test)]
pub const TEST_JWT_SECRET: &str = "test-secret-key-for-unit-tests-only";
```

## 8. 测试数据管理

### 8.1 原则

- 每个测试独立创建所需数据，不依赖共享状态
- 使用 `TempDir` 确保数据库文件自动清理
- 测试间完全隔离，可并行执行

### 8.2 数据库种子

```rust
pub fn seed_admin(conn: &Connection) -> User {
    let hash = hash_password_fast("Admin@123456");
    conn.execute(
        "INSERT INTO users (username, password_hash, display_name, role) VALUES (?1, ?2, ?3, ?4)",
        params!["admin", hash, "Administrator", "admin"],
    ).unwrap();
    // 返回创建的用户
    find_user_by_username(conn, "admin").unwrap()
}

pub fn seed_users(conn: &Connection, count: usize) -> Vec<User> {
    (0..count).map(|i| {
        let hash = hash_password_fast("User@123456");
        conn.execute(
            "INSERT INTO users (username, password_hash, display_name, role) VALUES (?1, ?2, ?3, ?4)",
            params![format!("user_{}", i), hash, format!("User {}", i), "user"],
        ).unwrap();
        find_user_by_username(conn, &format!("user_{}", i)).unwrap()
    }).collect()
}
```
