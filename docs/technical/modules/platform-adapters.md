# Platform 适配器开发指南

源文件：`rust-platform/src/platform/`

---

## Platform trait 接口定义

`Platform` trait 定义了与代码托管平台（GitHub、GitLab）交互的标准接口（`platform/mod.rs`）：

```rust
#[async_trait]
pub trait Platform: Send + Sync {
    // --- 能力发现 ---
    fn capabilities(&self) -> Vec<Capability>;

    // --- Issue 操作 ---
    async fn fetch_candidate_issues(&self, opts: FetchOptions) -> Result<Vec<Issue>, PlatformError>;
    async fn fetch_issue(&self, issue_id: IssueId) -> Result<Issue, PlatformError>;
    async fn fetch_issue_states_by_ids(&self, ids: &[IssueId]) -> Result<Vec<Issue>, PlatformError>;

    // --- 工作流状态（基于 label） ---
    async fn get_workflow_state(&self, issue_id: IssueId) -> Result<Option<String>, PlatformError>;
    async fn set_workflow_state(&self, issue_id: IssueId, state: &str) -> Result<(), PlatformError>;

    // --- Label 操作 ---
    async fn add_labels(&self, issue_id: IssueId, labels: &[String]) -> Result<(), PlatformError>;
    async fn remove_labels(&self, issue_id: IssueId, labels: &[String]) -> Result<(), PlatformError>;

    // --- 评论 / Workpad ---
    async fn create_comment(&self, issue_id: IssueId, body: &str) -> Result<CommentId, PlatformError>;
    async fn update_comment(&self, comment_id: CommentId, body: &str) -> Result<(), PlatformError>;
    async fn find_workpad_comment(&self, issue_id: IssueId) -> Result<Option<(CommentId, String)>, PlatformError>;
    async fn list_comments(&self, issue_id: IssueId) -> Result<Vec<Comment>, PlatformError>;

    // --- PR/MR ---
    async fn create_pull_request(&self, params: CreatePrParams) -> Result<PullRequest, PlatformError>;

    // --- 健康检查 ---
    async fn validate_credentials(&self) -> Result<(), PlatformError>;
}
```

---

## 已有实现

### GithubAdapter（`platform/github.rs`）

使用 GitHub REST API v3：

- Issue 状态通过 label 模拟（`workflow_labels` 配置）
- `set_workflow_state`：先 add 新状态 label，再 remove 旧状态 label（add-then-remove 策略）
- PR 创建：`POST /repos/{owner}/{repo}/pulls`
- 支持 `AtomicLabels`、`MergeRequest` capability

### GitlabAdapter（`platform/gitlab.rs`）

使用 GitLab REST API v4：

- Issue 状态通过 label 模拟
- `set_workflow_state`：原子 PUT 更新 label 列表（一次请求替换所有 label）
- MR 创建：`POST /projects/{id}/merge_requests`
- 支持私有部署（通过 `GITLAB_HOST` 环境变量）

### MemoryAdapter（`platform/memory.rs`）

内存实现，用于测试：

- 所有数据存储在 `MemoryState`（`Arc<Mutex<...>>`）中
- 支持 `FaultConfig` 故障注入（模拟 API 失败、延迟等）
- 无网络请求，适合单元测试和集成测试

---

## 新增 Platform 步骤

以新增 Gitea Platform 为例：

**1. 实现 Platform trait**

在 `rust-platform/src/platform/gitea.rs` 中：

```rust
pub struct GiteaAdapter {
    client: HttpClient,
    base_url: String,
    token: String,
    owner: String,
    repo: String,
    workflow_labels: Vec<String>,
}

#[async_trait]
impl Platform for GiteaAdapter {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::MergeRequest]
    }

    async fn fetch_candidate_issues(
        &self,
        opts: FetchOptions,
    ) -> Result<Vec<Issue>, PlatformError> {
        // 调用 Gitea API，过滤 workflow_labels
    }

    // ... 实现其他方法
}
```

**2. 注册到配置**

在 `rust-platform/src/config/service_config.rs` 中添加 Gitea 平台类型，并在 `main.rs` 的平台初始化逻辑中添加对应分支。

**3. 处理认证和分页**

使用 `platform/http_client.rs` 提供的共享 HTTP 客户端层，它已内置：

- Token 认证头注入
- 自动分页（Link header 解析）
- 超时配置
- 代理支持

---

## HTTP 客户端共享层（`platform/http_client.rs`）

`HttpClient` 封装了 `reqwest::Client`，提供：

- **认证**：自动注入 `Authorization: Bearer <token>` 或 `PRIVATE-TOKEN: <token>`
- **分页**：解析 `Link: <url>; rel="next"` header，自动翻页
- **超时**：可配置的请求超时
- **代理**：继承进程环境变量中的代理配置
- **重试**：`platform/retry.rs` 提供指数退避重试逻辑

---

## 工作流状态管理

Platform 使用 label 模拟工作流状态（因为 GitHub/GitLab Issue 没有原生状态字段）。

### GitHub — add-then-remove 策略

```
set_workflow_state(issue_id, "In Progress"):
    1. add_labels(issue_id, ["In Progress"])
    2. 获取当前所有 workflow_labels
    3. remove_labels(issue_id, 当前 workflow_labels 中除 "In Progress" 外的所有标签)
```

两步操作非原子，存在短暂的多标签状态，但实践中影响可忽略。

### GitLab — 原子 PUT 策略

```
set_workflow_state(issue_id, "In Progress"):
    1. 获取 Issue 当前所有 labels
    2. 移除所有 workflow_labels
    3. 添加 "In Progress"
    4. PUT /issues/{id} with { labels: [...新标签列表] }
```

单次 API 调用原子替换，无中间状态。

---

## CooldownQueue（`platform/cooldown_queue.rs`）

`CooldownQueue` 管理 per-resource 的速率限制冷却时间：

- 每个资源（Issue ID、Label 等）有独立的冷却计时器
- 操作后设置冷却时间，冷却期内跳过重复操作
- 用于避免对同一 Issue 频繁发送 API 请求

---

## Capability 枚举

```rust
pub enum Capability {
    AtomicLabels,   // 支持原子 label 更新（GitLab）
    MergeRequest,   // 支持创建 MR/PR
    Webhook,        // 支持 Webhook 推送（未来扩展）
}
```

在实现 `capabilities()` 时，只声明平台实际支持的能力。调用方通过 `capabilities()` 检查后再调用对应方法。

---

## 错误处理

`PlatformError` 是所有 Platform 实现共用的错误类型：

```rust
pub enum PlatformError {
    // 认证失败
    Unauthorized,
    // 资源不存在
    NotFound(String),
    // API 请求失败
    ApiRequest(reqwest::Error),
    // API 返回非预期状态码
    ApiStatus { status: u16, body: String },
    // 能力不支持
    UnsupportedCapability(String),
    // 其他内部错误
    Internal(String),
}
```

错误消息不要硬编码平台名称（如不要写 "GitHub API returned 404"），使用通用描述，因为同一错误变体会被多个平台实现共用。

---

## 测试方法

### MemoryAdapter 单元测试

```rust
#[tokio::test]
async fn test_set_workflow_state() {
    let state = Arc::new(Mutex::new(MemoryState::default()));
    let adapter = MemoryAdapter::new(state.clone());

    // 创建测试 Issue
    let issue = make_test_issue("1", "TEST-1", "In Progress");
    state.lock().await.issues.insert("1".to_string(), issue);

    // 设置工作流状态
    adapter.set_workflow_state(IssueId("1".to_string()), "Done").await.unwrap();

    // 验证状态已更新
    let updated = adapter.fetch_issue(IssueId("1".to_string())).await.unwrap();
    assert_eq!(updated.state, "Done");
}
```

### FaultConfig 故障注入

```rust
#[tokio::test]
async fn test_handles_api_failure() {
    let fault_config = FaultConfig {
        fail_fetch: true,           // 让 fetch 操作失败
        fail_after_n_calls: Some(2), // 前 2 次成功，之后失败
        ..Default::default()
    };

    let adapter = MemoryAdapter::with_faults(fault_config);
    // 测试 orchestrator 在 API 失败时的行为
}
```

### wiremock HTTP Mock 测试

```rust
#[tokio::test]
async fn test_github_fetch_issues() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/repos/org/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "number": 1, "title": "Test", "labels": [{"name": "In Progress"}] }
        ])))
        .mount(&mock_server)
        .await;

    let adapter = GithubAdapter::new(&mock_server.uri(), "token", "org", "repo", vec![]);
    let issues = adapter.fetch_candidate_issues(FetchOptions::default()).await.unwrap();
    assert_eq!(issues.len(), 1);
}
```
