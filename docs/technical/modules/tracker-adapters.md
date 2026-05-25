# Tracker 适配器开发指南

源文件：`rust-platform/src/tracker/`

---

## Tracker trait 接口定义

`Tracker` trait 定义了 orchestrator 与 Issue 追踪系统交互的标准接口（`tracker/mod.rs`）：

```rust
#[async_trait]
pub trait Tracker: Send + Sync {
    /// 获取所有处于 active_states 的候选 Issue（调度用）
    async fn fetch_candidate_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError>;

    /// 按状态名称批量获取 Issue（启动时终态清理用）
    async fn fetch_issues_by_states(
        &self,
        states: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError>;

    /// 按 Issue ID 批量获取当前状态（协调器 Part B 用）
    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError>;

    /// 设置 Issue 的工作流状态标签（可选，默认 no-op）
    async fn set_workflow_state(
        &self,
        issue_id: &str,
        state: &str,
    ) -> Result<(), TrackerError> {
        Ok(())
    }
}
```

---

## TrackerIssue 模型

`TrackerIssue` 是 orchestrator 使用的规范化 Issue 表示：

```rust
pub struct TrackerIssue {
    /// 追踪器内部稳定 ID（Linear UUID / GitHub issue number 字符串）
    pub id: String,
    /// 人类可读标识（如 "ABC-123"、"#42"）
    pub identifier: String,
    pub title: String,
    pub description: Option<String>,
    /// 优先级：数值越小越高，None 排最后
    pub priority: Option<i32>,
    /// 当前状态（state_key 形式，如 "in_progress"）
    pub state: String,
    pub branch_name: Option<String>,
    pub url: Option<String>,
    /// 标签列表（归一化为小写）
    pub labels: Vec<String>,
    /// 阻塞依赖列表
    pub blocked_by: Vec<BlockerRef>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

pub struct BlockerRef {
    pub id: Option<String>,
    pub identifier: Option<String>,
    pub state: Option<String>,
}
```

---

## 已有实现

### LinearClient（`tracker/linear.rs`）

直接实现 `Tracker` trait，使用 Linear GraphQL API：

- 通过 GraphQL 查询获取指定状态的 Issue
- 支持分页（`endCursor` 游标）
- 状态通过 `normalize_tracker_state()` 归一化

### GitlabTrackerAdapter（`tracker/gitlab.rs`）

桥接 `Platform` trait 到 `Tracker` trait：

- 内部持有 `Arc<dyn Platform>`（GitlabAdapter）
- 将 Platform 的 `fetch_candidate_issues` 结果转换为 `TrackerIssue`
- 状态通过 `normalize_tracker_state()` 归一化后返回

---

## 新增 Tracker 步骤

以新增 Jira Tracker 为例：

**1. 实现 Tracker trait**

在 `rust-platform/src/tracker/jira.rs` 中：

```rust
pub struct JiraTracker {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    project_key: String,
    active_states: Vec<String>,
}

#[async_trait]
impl Tracker for JiraTracker {
    async fn fetch_candidate_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        // 调用 Jira REST API，过滤 active_states
        // 将结果转换为 TrackerIssue，调用 normalize_tracker_state()
    }

    async fn fetch_issues_by_states(
        &self,
        states: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        // 按状态查询
    }

    async fn fetch_issue_states_by_ids(
        &self,
        ids: &[String],
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        // 按 ID 批量查询当前状态
    }
}
```

**2. 添加 TrackerKind 枚举值**

在 `rust-platform/src/config/service_config.rs` 中：

```rust
pub enum TrackerKind {
    Linear,
    GitHub,
    GitLab,
    Jira,   // 新增
}
```

**3. 注册到 main.rs tracker 初始化**

在 `rust-platform/src/main.rs` 中的 tracker 构建逻辑中添加 Jira 分支：

```rust
TrackerKind::Jira => {
    Arc::new(JiraTracker::new(&config)) as Arc<dyn Tracker>
}
```

**4. 配置 WORKFLOW.md**

```yaml
tracker:
  kind: jira
  endpoint: https://your-org.atlassian.net
  api_key: $JIRA_API_TOKEN
  project_slug: "PROJ"
  active_states:
    - In Progress
    - In Review
```

---

## 状态归一化要求

这是实现新 Tracker 时最容易出错的地方，必须严格遵守。

**规则**：`normalize_tracker_state()` 的行为必须与 `normalize_state()`（`models/mod.rs`）完全一致：

```
trim → lowercase → 空格替换为下划线 → 连字符替换为下划线
```

示例：

| 原始状态 | 归一化结果 |
|----------|------------|
| `"In Progress"` | `"in_progress"` |
| `"in-progress"` | `"in_progress"` |
| `" Todo "` | `"todo"` |
| `"Human Review"` | `"human_review"` |

**背景**：`GitlabTrackerAdapter` 返回的 `TrackerIssue.state` 是 state_key 形式（如 `"in_progress"`），而 `ServiceConfig.active_states` 保存的是原始形式（如 `"In Progress"`）。reconciler 的 `normalize_state` 必须处理空格→下划线转换，否则会误判 Issue 不在 active 状态而终止正在运行的 worker。

---

## 错误处理

`TrackerError` 是所有 Tracker 实现共用的错误类型，**不要在错误消息中硬编码平台名称**：

```rust
pub enum TrackerError {
    UnsupportedTrackerKind(String),
    MissingApiKey,
    MissingProjectSlug,
    ApiRequest { source: reqwest::Error },
    ApiStatus { status: u16, body: String },   // 通用，不写 "Jira API"
    GraphqlErrors { errors: Vec<serde_json::Value> },
    UnknownPayload { detail: String },
    MissingEndCursor,
}
```

---

## 测试方法

**HTTP Mock 测试**（使用 `wiremock`）：

```rust
#[tokio::test]
async fn test_fetch_candidates_normalizes_state() {
    let mock_server = MockServer::start().await;

    // 模拟 Tracker API 返回 "In Progress" 状态
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "issues": {
                    "nodes": [{
                        "id": "issue-1",
                        "identifier": "ABC-1",
                        "title": "Test",
                        "state": { "name": "In Progress" },
                        "priority": 1,
                    }]
                }
            }
        })))
        .mount(&mock_server)
        .await;

    let tracker = LinearClient::new(&mock_server.uri(), "test-key", "PROJ", active_states);
    let issues = tracker.fetch_candidate_issues().await.unwrap();

    // 验证状态已归一化
    assert_eq!(issues[0].state, "in_progress");
}
```

**归一化交叉匹配测试**：

```rust
#[test]
fn test_state_normalization_cross_match() {
    // active_states 用原始形式（来自 WORKFLOW.md）
    let active_states = vec!["In Progress".to_string(), "Todo".to_string()];

    // tracker 返回 state_key 形式
    let tracker_state = "in_progress";

    // 验证归一化后能正确匹配
    let normalized_active: Vec<String> = active_states
        .iter()
        .map(|s| normalize_state(s))
        .collect();

    assert!(normalized_active.contains(&normalize_tracker_state(tracker_state)));
}
```
