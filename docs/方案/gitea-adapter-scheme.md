# Gitea Platform Adapter 实现方案（v2）

## 背景

项目已有 GitHub 和 GitLab 的 Platform adapter 实现。现在需要添加 Gitea 支持，使用户可以用 Gitea 作为 issue tracker。Gitea 的 REST API 与 GitHub 高度相似（路径结构、认证方式、分页机制几乎一致），但作为独立 adapter 实现以保持清晰的边界和未来扩展性。

Workflow state 采用 label-based 方式（与 GitHub/GitLab 一致），复用现有的 `GitlabTrackerAdapter` wrapper 将 Platform 桥接到 Tracker trait。

## 架构决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 实现方式 | 独立 `GiteaAdapter` | 虽然与 GitHub 有重复代码，但边界清晰，未来 Gitea 特有功能更容易扩展 |
| State 管理 | Label-based | 与 GitHub/GitLab 一致，复用 `GitlabTrackerAdapter` wrapper |
| Tracker 层 | 复用 `GitlabTrackerAdapter` | 该 wrapper 已支持任意 `Platform` 实现，无需新建 |
| 认证方式 | `Authorization: token <TOKEN>` | Gitea 标准认证格式 |
| Label 操作 | ID-based + 启动时缓存 | Gitea API 要求 label ID，需维护 name→ID 映射 |
| 分页参数 | `?limit=` + `?page=` | Gitea 不识别 `per_page`，需要适配 HttpClient |
| 最低版本 | Gitea 1.12+ | 确保 API 兼容性 |

## 实现步骤

### 1. 新建 `rust-platform/src/platform/gitea.rs`

实现 `GiteaAdapter` struct，完整实现 `Platform` trait 的所有方法。

**结构参考** `platform/github.rs`，关键差异：

- Gitea 认证使用 `Authorization: token <TOKEN>` header（而非 Bearer）
- Gitea issue state 字段为 `"open"` / `"closed"`，与 GitHub 相同
- Gitea PR 创建端点为 `POST /repos/:owner/:repo/pulls`（同 GitHub）
- Gitea 分页使用 Link header + `?limit=N&page=N`（**非** `per_page`）
- Gitea label API 使用 ID（非 name），需要 name→ID 缓存
- Gitea API 路径需要 `/api/v1` 前缀，`base_url` 配置应为 `https://gitea.example.com/api/v1`

**API 响应类型**（私有 serde 结构体）：

```rust
#[derive(Debug, Deserialize)]
struct GiteaIssue {
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    html_url: String,
    state: String,           // "open" | "closed"
    labels: Vec<GiteaLabel>,
    assignee: Option<GiteaUser>,
    created_at: String,
    updated_at: String,
    pull_request: Option<serde_json::Value>,  // 过滤 PR
}

#[derive(Debug, Deserialize)]
struct GiteaLabel {
    id: u64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct GiteaUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GiteaComment {
    id: u64,
    body: Option<String>,
    user: Option<GiteaUser>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct GiteaPullRequest {
    id: u64,
    number: u64,
    html_url: String,
    state: String,
}
```

**Adapter 结构体**（采用 GitLab 模式，不冗余存储 config）：

```rust
pub struct GiteaAdapter {
    http: HttpClient,
    label_cache: tokio::sync::RwLock<HashMap<String, u64>>,  // name → id
}

impl GiteaAdapter {
    pub fn new(config: PlatformConfig) -> Result<Self, PlatformError> { ... }
    pub fn new_with_token(config: PlatformConfig, token: &str) -> Result<Self, PlatformError> { ... }
    pub fn http_client(&self) -> &HttpClient { &self.http }

    fn repo_path(&self) -> String {
        let config = self.http.config();
        format!("/repos/{}/{}", config.owner, config.repo)
    }

    /// 刷新 label name→ID 缓存
    async fn refresh_label_cache(&self) -> Result<(), PlatformError> { ... }

    /// 根据 label name 获取 ID，缓存未命中时自动刷新
    async fn resolve_label_id(&self, name: &str) -> Result<u64, PlatformError> { ... }
}
```

**Platform trait 方法实现**：

| 方法 | Gitea API 端点 | 备注 |
|------|---------------|------|
| `capabilities()` | — | 返回 `Vec::new()`（与 GitHub 保持一致） |
| `fetch_candidate_issues()` | `GET /repos/:owner/:repo/issues?state=open&type=issues&labels=...&limit=50&page=N` | 自行实现分页（不用 `get_all_pages`） |
| `fetch_issue()` | `GET /repos/:owner/:repo/issues/:number` | — |
| `fetch_issue_states_by_ids()` | 逐个 fetch | 同 GitHub 模式 |
| `get_workflow_state()` | 从 issue labels 中提取 | 同 GitHub 的 `extract_workflow_state` |
| `set_workflow_state()` | add label + remove old labels | add-then-remove 策略，使用 label ID |
| `add_labels()` | `POST /repos/:owner/:repo/issues/:number/labels` | body: `{"labels": [id1, id2]}`，通过 `resolve_label_id` 获取 ID |
| `remove_labels()` | `DELETE /repos/:owner/:repo/issues/:number/labels/:id` | 通过 `resolve_label_id` 获取 ID |
| `create_comment()` | `POST /repos/:owner/:repo/issues/:number/comments` | — |
| `update_comment()` | `PATCH /repos/:owner/:repo/issues/comments/:id` | — |
| `find_workpad_comment()` | `GET /repos/:owner/:repo/issues/:number/comments` | 查找含 "## Codex Workpad" 的 comment |
| `list_comments()` | `GET /repos/:owner/:repo/issues/:number/comments` | — |
| `create_pull_request()` | `POST /repos/:owner/:repo/pulls` | — |
| `validate_credentials()` | `GET /user` | 验证 token 有效性 |

**分页实现**：GiteaAdapter 自行实现分页逻辑（不使用 `HttpClient::get_all_pages`），使用 `?limit=50&page=N` 参数，通过 Link header 或返回数量 < limit 判断终止。保留 `MAX_PAGES=10` 安全上限。

**Label 缓存机制**：
- 启动时（`new` / `new_with_token`）不立即加载缓存
- 首次 `resolve_label_id` 调用时 lazy 加载
- 缓存未命中时自动刷新一次（处理运行时新建 label 的情况）
- 刷新后仍未找到则返回 `PlatformError::NotFound`
- 缓存上限 1000 条，超出时清空重建（防止恶意 Gitea 实例导致内存膨胀）

**Label 缓存与 `HttpClient::list_labels` 的关系**：

`crate::config::Label` 结构体只有 `name` 和 `color` 字段（无 `id`）。`HttpClient::list_labels()` 返回 `Vec<Label>` 足以让 `ensure_workflow_labels` 判断 label 是否存在并创建缺失的 label。但 `GiteaAdapter` 的 `add_labels`/`remove_labels` 需要 label ID。

因此 `GiteaAdapter` 的 `refresh_label_cache()` **不复用** `HttpClient::list_labels()`，而是自行调用 `GET /repos/:owner/:repo/labels` 并反序列化为内部的 `GiteaLabel`（含 `id: u64`），填充 `label_cache`。两者职责分离：
- `HttpClient::list_labels()` → 供 `ensure_workflow_labels` 使用（只需 name）
- `GiteaAdapter::refresh_label_cache()` → 供 `add_labels`/`remove_labels` 使用（需要 id）

**Closed issue state 推断**：与 GitHub 一致，当 `state == "closed"` 时使用 `inferred_closed_issue_state()` 逻辑。

**PR 过滤**：`fetch_candidate_issues` 中过滤掉 `pull_request.is_some()` 的条目。

### 2. 注册模块

文件：`rust-platform/src/platform/mod.rs`

```rust
pub mod gitea;  // 新增
```

### 3. 修改 `rust-platform/src/config/service_config.rs`

**3.1 TrackerKind 枚举**：

```rust
pub enum TrackerKind {
    Linear,
    GitHub,
    GitLab,
    Gitea,   // 新增
}
```

**3.2 `parse_tracker_kind()`**：

```rust
"gitea" => Ok(TrackerKind::Gitea),
```

**3.3 `default_endpoint_for_kind()`**：

```rust
TrackerKind::Gitea => String::new(),  // Gitea 是自托管的，无统一默认端点
```

**3.4 `resolve_tracker_api_key()` — 添加 canonical 环境变量**：

```rust
TrackerKind::Gitea => "GITEA_TOKEN",
```

**3.5 `validate_for_dispatch()` — 添加 Gitea 校验**：

```rust
// Gitea 和 GitHub 都需要 project_slug（owner/repo 格式）
if (self.tracker_kind == TrackerKind::Gitea || self.tracker_kind == TrackerKind::GitHub)
    && self.tracker_project_slug.is_empty()
{
    return Err(ServiceConfigError::ValidationFailed(
        "tracker.project_slug is required for GitHub/Gitea tracker (format: owner/repo)".to_string(),
    ));
}

// Gitea 必须配置 endpoint（自托管，无默认值）
if self.tracker_kind == TrackerKind::Gitea && self.tracker_endpoint.is_empty() {
    return Err(ServiceConfigError::ValidationFailed(
        "tracker.endpoint is required for Gitea (self-hosted, e.g. https://gitea.example.com/api/v1)".to_string(),
    ));
}
```

### 4. 修改 `rust-platform/src/main.rs`

**4.1 添加 Gitea 初始化分支**（`match service_config.tracker_kind` 块）：

```rust
TrackerKind::Gitea => {
    let platform_config = build_platform_config(&service_config, "gitea");

    match GiteaAdapter::new_with_token(platform_config, &service_config.tracker_api_key) {
        Ok(adapter) => {
            if let Err(e) = adapter.http_client().ensure_workflow_labels().await {
                tracing::warn!(error = %e, "Failed to verify workflow labels");
            }

            let tracker = Arc::new(GitlabTrackerAdapter::new(
                Arc::new(adapter),
                service_config.active_states.clone(),
                service_config.terminal_states.clone(),
            ));
            orchestrator.set_tracker(tracker);
            tracing::info!("Gitea tracker wired into orchestrator");
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to create Gitea adapter, dispatch disabled");
        }
    }
}
```

**4.2 `build_platform_config()` — Gitea 走 GitHub 的 owner/repo 解析路径**：

```rust
let (owner, repo, project_id) = if platform_kind == "github" || platform_kind == "gitea" {
    let mut parts = service_config.tracker_project_slug.splitn(2, '/');
    let owner = parts.next().unwrap_or_default().to_string();
    let repo = parts.next().unwrap_or_default().to_string();
    (owner, repo, None)
} else {
    // GitLab path
    ...
};
```

**4.3 修复 `build_workflow_states()` 连字符处理**（已知 bug，顺带修复）：

```rust
fn build_workflow_states(service_config: &ServiceConfig) -> HashMap<String, String> {
    let mut states = HashMap::new();
    for s in &service_config.active_states {
        states.insert(s.trim().to_lowercase().replace([' ', '-'], "_"), s.clone());
    }
    for s in &service_config.terminal_states {
        states.insert(s.trim().to_lowercase().replace([' ', '-'], "_"), s.clone());
    }
    states
}
```

### 5. 修改 `rust-platform/src/platform/http_client.rs`

**5.1 `build_client_with_token()` — 在 match 中新增 `"gitea"` arm**：

```rust
match config.kind.as_str() {
    "github" => {
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|_| PlatformError::InvalidToken)?);
        headers.insert("Accept", HeaderValue::from_static("application/vnd.github+json"));
    }
    "gitlab" => {
        headers.insert("PRIVATE-TOKEN", HeaderValue::from_str(token)
            .map_err(|_| PlatformError::InvalidToken)?);
    }
    "gitea" => {
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("token {}", token))
            .map_err(|_| PlatformError::InvalidToken)?);
        headers.insert("Accept", HeaderValue::from_static("application/json"));
    }
    _ => {}
}
```

**5.2 `list_labels()` — 添加 `"gitea"` 分支**：

Gitea 的 label 列表路径与 GitHub 相同：`GET /repos/:owner/:repo/labels`

```rust
"gitea" => {
    let path = format!("/repos/{}/{}/labels", self.config.owner, self.config.repo);
    // ... 同 GitHub 逻辑
}
```

**5.3 `ensure_workflow_labels()` — 添加 `"gitea"` 分支**：

Gitea 创建 label：`POST /repos/:owner/:repo/labels`，body: `{"name": "...", "color": "#..."}`

### 6. 修改 `rust-platform/src/config/validator.rs`

添加 `"gitea"` 为合法的 platform kind，确保 Gitea 配置经过完整的安全校验（HTTPS 强制、token 格式、workflow states）。

添加 owner/repo 格式校验：

```rust
fn validate_gitea_specifics(config: &PlatformConfig) -> Result<(), ValidationError> {
    // owner 和 repo 只允许 [a-zA-Z0-9._-]
    let valid_pattern = |s: &str| s.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !valid_pattern(&config.owner) || !valid_pattern(&config.repo) {
        return Err(ValidationError::InvalidValue {
            field: "project_slug".to_string(),
            detail: "owner and repo must contain only alphanumeric, dot, underscore, or hyphen".to_string(),
        });
    }
    Ok(())
}
```

### 7. 添加测试

#### 7.1 `rust-platform/tests/gitea_adapter_test.rs`（wiremock 测试）

测试用例：

**认证头验证**：
- 请求携带 `Authorization: token <TOKEN>`（非 Bearer）
- 请求携带 `Accept: application/json`

**Issue 操作**：
- `fetch_candidate_issues` — 正常获取 + PR 过滤
- `fetch_candidate_issues` — 分页（使用 `?limit=50&page=N`，非 `per_page`）
- `fetch_candidate_issues` — 150 条 issue 跨 3 页正确获取
- `fetch_issue` — 单个 issue 获取
- Closed issue state 推断

**Label 操作**：
- `add_labels` — 先查 label ID，再 POST ID 数组
- `add_labels` — label 不存在时返回 NotFound 错误
- `remove_labels` — 通过 ID 调用 DELETE
- Label 缓存命中（不重复请求 label 列表）
- Label 缓存未命中时自动刷新

**Comment 操作**：
- `create_comment` / `update_comment` / `list_comments`
- `find_workpad_comment` — 正确匹配 "## Codex Workpad"

**PR 创建**：
- `create_pull_request` — 正确调用 POST /repos/:owner/:repo/pulls

**凭证验证**：
- `validate_credentials` — 成功 / 401 失败

#### 7.2 State 归一化交叉测试（在 `tracker/gitlab.rs` 的 `#[cfg(test)]` 中添加）

```rust
#[tokio::test]
async fn gitea_tracker_state_normalization_cross_match() {
    // 模拟 GiteaAdapter 返回 workflow_state = "in_progress"
    // GitlabTrackerAdapter 的 active_states = ["In Progress"]
    // 验证 issue 被正确包含在 fetch_candidate_issues 结果中
}

#[tokio::test]
async fn gitea_tracker_hyphenated_state_matches() {
    // active_states = ["In-Progress"]
    // workflow_state = "in_progress"
    // 验证归一化后匹配
}
```

#### 7.3 `rust-platform/tests/common/gitea_host.rs`（测试辅助）

提供 `mock_gitea_issues_response()`、`mock_gitea_labels_response()` 等 fixture 函数。

#### 7.4 `rust-platform/tests/real_workflow_gitea.rs`（集成测试，`#[ignore]`）

使用 Docker Gitea 实例的 E2E 测试，验证完整的 issue → label → comment → PR 流程。

### 8. 新建 `web-platform/src/templates/workflow_gitea.md`

基于 `workflow_github.md` 模板微调生成。

#### 8.1 web-platform 代码注册（必须修改的文件）

| 文件 | 修改内容 |
|------|----------|
| `web-platform/src/git_url.rs` | 添加 `Platform::Gitea` 枚举变体 + `detect_platform()` 识别逻辑 |
| `web-platform/src/templates/mod.rs` | 添加 `GITEA_TEMPLATE` / `TEST_GITEA_TEMPLATE` 常量 + match arms（`get_default_template`、`get_test_template`、`render_template_string` 中的 `platform_endpoint` 构造） |
| `web-platform/src/process_manager/spawn.rs` | 添加 `Platform::Gitea` arm，注入 `GITEA_TOKEN` 环境变量 + `platform_host` 默认值处理 |
| `web-platform/src/handlers/project_workflow.rs` | 添加 `"gitea" => Platform::Gitea` match arms（3 处） |
| `web-platform/src/models/user.rs` | `UserConfig` 添加 `gitea_token: Option<String>` 和 `gitea_host: Option<String>` 字段 |
| `web-platform/src/handlers/user_profile.rs` | 暴露 `gitea_token` / `gitea_host` 字段的读写 API |
| `web-platform/migrations/` | **新建** — 添加 `gitea_token` 和 `gitea_host` 列到 user_configs 表 |
| `web-platform/src/templates/workflow_gitea.md` | **新建** — Agent workflow 模板 |
| `web-platform/src/templates/workflow_test_gitea.md` | **新建** — Test-engineer 模板 |

#### 8.2 `detect_platform()` 对 Gitea 的识别

Gitea 是自托管的，hostname 不固定。识别策略：
- **不修改 `detect_platform()` 的默认行为**——该函数仅用于从 git URL 推断平台类型，对未知 host 仍默认 GitLab
- Gitea 项目通过用户在 UI 中**显式选择** platform 类型创建，存储在 `project.platform` 字段中
- `spawn.rs` 和 `project_workflow.rs` 已经优先使用 `project.platform.as_str()` 做 match，不依赖 `detect_platform()`

#### 8.3 `platform_endpoint` 构造逻辑

```rust
Platform::Gitea => {
    // Gitea 的 endpoint 由用户完整配置（含 /api/v1），直接透传
    ctx.platform_host.clone()
}
```

#### 8.4 `platform_host` 默认值处理

`spawn.rs` 中 `platform_host` 的 fallback 需要添加 Gitea arm：

```rust
let platform_host = project
    .platform_host
    .clone()
    .unwrap_or_else(|| match &platform {
        Platform::GitLab => "https://gitlab.com".to_string(),
        Platform::GitHub => "https://github.com".to_string(),
        Platform::Gitea => {
            // Gitea 是自托管的，无默认值。此处不应到达（创建项目时强制要求填写）
            tracing::error!("Gitea project missing platform_host");
            String::new()
        }
    });
```

同时在项目创建逻辑中校验：当 `platform == "gitea"` 时，`platform_host` 必须非空。

#### 8.4 模板 CLI 策略：使用安全封装的 shell 函数

**问题**：直接使用 `curl -H "Authorization: token $GITEA_TOKEN"` 存在严重安全风险：
- Token 在 `ps aux` / `/proc/pid/cmdline` 中可见
- 模板变量未转义时存在命令注入风险
- 无 TLS 强制和重定向防护

**解决方案**：在模板顶部定义 `gitea_api()` 安全封装函数，所有 API 调用通过此函数执行：

```bash
## Prerequisite: Gitea API access

GITEA_ENDPOINT="{{platform_endpoint}}"

# Validate token and endpoint
if [ -z "$GITEA_TOKEN" ]; then
  echo "GITEA_TOKEN not set"; exit 1
fi

# Secure API wrapper — token never appears on command line
gitea_api() {
  local method="$1" path="$2"; shift 2
  curl -sf --proto '=https' --no-location --max-time 30 \
    --config <(printf 'header = "Authorization: token %s"\n' "$GITEA_TOKEN") \
    -X "$method" \
    -H "Content-Type: application/json" \
    "$@" \
    "${GITEA_ENDPOINT}${path}"
}

# Verify connectivity
gitea_api GET "/user" > /dev/null || { echo "Gitea API not reachable or token invalid"; exit 1; }
```

**安全保证**：
- `--config <(...)` — token 通过 process substitution 传递，不出现在命令行参数中
- `--proto '=https'` — 强制 HTTPS，拒绝 HTTP
- `--no-location` — 禁止跟随重定向（防止 token 泄露到第三方）
- `--max-time 30` — 防止挂起
- 所有 URL 路径通过变量引用（双引号包裹），防止注入

#### 8.5 Label 操作

使用 jq `--arg` 安全传参，避免 filter 注入：

```bash
# 获取 label ID（安全方式）
get_label_id() {
  local label_name="$1"
  gitea_api GET "/repos/${OWNER}/${REPO}/labels" | \
    jq -re --arg name "$label_name" '.[] | select(.name==$name) | .id'
}

# 状态转换：add-then-remove（先加后删，避免零 label 窗口）
transition_label() {
  local issue_number="$1" old_label="$2" new_label="$3"
  local new_id old_id

  new_id=$(get_label_id "$new_label") || { echo "Label '$new_label' not found"; return 1; }
  old_id=$(get_label_id "$old_label") || { echo "Label '$old_label' not found"; return 1; }

  # Add new label first
  gitea_api POST "/repos/${OWNER}/${REPO}/issues/${issue_number}/labels" \
    -d "{\"labels\":[${new_id}]}"
  # Then remove old label
  gitea_api DELETE "/repos/${OWNER}/${REPO}/issues/${issue_number}/labels/${old_id}"
}

# Example: move from "Todo" to "In Progress"
transition_label <number> "Todo" "In Progress"
```

**原子性说明**：Gitea 不支持单次 API 调用同时 add+remove label。采用 add-then-remove 顺序，确保 issue 始终至少有一个 workflow label（避免 reconciler 误判）。

#### 8.6 前置依赖检查

```bash
# 验证 jq 可用
command -v jq >/dev/null || { echo "jq not found — required for Gitea API parsing"; exit 1; }
command -v curl >/dev/null || { echo "curl not found"; exit 1; }
```

#### 8.7 模板 Guardrails（安全指令）

在模板中添加以下 agent 指令：
- "Never use `curl -k` or `--insecure` even if TLS errors occur — report as a blocker instead."
- "Never echo, log, or include `$GITEA_TOKEN` in workpad comments, issue comments, or any output."
- "Always use the `gitea_api()` wrapper for API calls. Never construct raw curl commands with inline tokens."

#### 8.8 与 GitHub/GitLab 模板的差异总结

| 差异点 | GitHub 模板 | GitLab 模板 | Gitea 模板 |
|--------|-------------|-------------|------------|
| CLI 工具 | `gh` | `glab` | `gitea_api()` shell 函数 |
| Front matter | `kind: github` | `kind: gitlab` + `endpoint` | `kind: gitea` + `endpoint` |
| Token 传递 | CLI 内部读取 env | CLI 内部读取 env | `--config <(...)` process substitution |
| Label 操作 | `gh issue edit --add-label --remove-label` | `glab issue update --label --unlabel` | `transition_label()` 函数（ID-based） |
| PR/MR 操作 | `gh pr create` | `glab mr create` | `gitea_api POST /repos/:owner/:repo/pulls` |
| Comment 操作 | `gh api` | `glab api` | `gitea_api GET/POST/PATCH/DELETE` |
| Rate limit | 5000 req/hour | 300 req/min | 自托管，取决于实例配置 |
| 术语 | PR | MR | PR |

### 9. 添加测试（web-platform 部分）

#### 9.1 `web-platform/src/templates/mod.rs` — 单元测试

```rust
#[test]
fn test_render_gitea_template() {
    let ctx = WorkflowTemplateContext {
        platform: Platform::Gitea,
        project_slug: "myorg/myrepo".to_string(),
        platform_host: "https://gitea.example.com/api/v1".to_string(),
        // ... 其他字段
    };
    let rendered = render_template(&ctx);

    // 核心断言
    assert!(rendered.contains("kind: gitea"));
    assert!(rendered.contains("project_slug: \"myorg/myrepo\""));
    assert!(rendered.contains("endpoint: \"https://gitea.example.com/api/v1\""));
    assert!(rendered.contains("max_concurrent_agents:"));
    assert!(rendered.contains("origin/main"));
    // 无残留占位符
    assert!(!rendered.contains("{{"));
    assert!(!rendered.contains("}}"));
}

#[test]
fn test_render_test_gitea_template() {
    let ctx = WorkflowTemplateContext {
        platform: Platform::Gitea,
        platform_host: "https://gitea.example.com/api/v1".to_string(),
        // ...
    };
    let rendered = render_test_template(&ctx);

    assert!(rendered.contains("kind: gitea"));
    assert!(rendered.contains("active_states:"));
    assert!(rendered.contains("Testing"));
    assert!(rendered.contains("test-engineer"));
    assert!(rendered.contains("FAIL-MINOR"));
    assert!(rendered.contains("FAIL-MAJOR"));
    // 无残留占位符
    assert!(!rendered.contains("{{"));
    assert!(!rendered.contains("}}"));
}

#[test]
fn test_get_default_template_gitea() {
    let tmpl = get_default_template(Platform::Gitea);
    assert!(!tmpl.is_empty());
    assert!(tmpl.contains("gitea"));
}

#[test]
fn test_get_test_template_gitea() {
    let tmpl = get_test_template(Platform::Gitea);
    assert!(!tmpl.is_empty());
    assert!(tmpl.contains("gitea"));
}
```

#### 9.2 `web-platform/tests/api_workflow.rs` — 集成测试

```rust
#[tokio::test]
async fn test_get_workflow_default_gitea() {
    // 创建项目时显式指定 platform="gitea" 和 platform_host
    let project = create_test_project_with_platform(
        "gitea",
        "https://gitea.example.com/api/v1",
        "myorg/myrepo",
    ).await;

    let resp = get_workflow(project.id).await;
    assert!(resp.contains("kind: gitea"));
    assert!(resp.contains("endpoint: \"https://gitea.example.com/api/v1\""));
}
```

注：需要扩展 `create_test_project` 辅助函数以支持显式指定 platform 类型（而非依赖 `detect_platform()`）。

#### 9.3 模板内容断言

- 验证 `workflow_gitea.md` 中不包含 `gh ` 或 `glab ` 命令（防止复制遗漏）
- 验证 `workflow_test_gitea.md` 中不包含 `gh ` 或 `glab ` 命令
- 验证 `gitea_api()` 函数定义存在
- 验证 `--proto '=https'` 安全标志存在
- 验证 `#!/usr/bin/env bash` shebang 或 bash 依赖说明存在

## 需要修改的文件清单

| 文件 | 操作 |
|------|------|
| **rust-platform** | |
| `rust-platform/src/platform/gitea.rs` | **新建** — GiteaAdapter 完整实现 |
| `rust-platform/src/platform/mod.rs` | 修改 — 添加 `pub mod gitea;` |
| `rust-platform/src/config/service_config.rs` | 修改 — TrackerKind 枚举 + 解析 + 默认端点 + 环境变量 + 校验 |
| `rust-platform/src/config/validator.rs` | 修改 — 添加 "gitea" 为合法 kind + owner/repo 格式校验 |
| `rust-platform/src/main.rs` | 修改 — 初始化分支 + build_platform_config + build_workflow_states 修复 |
| `rust-platform/src/platform/http_client.rs` | 修改 — 认证头 + list_labels + ensure_workflow_labels |
| `rust-platform/tests/gitea_adapter_test.rs` | **新建** — wiremock 测试 |
| `rust-platform/tests/common/gitea_host.rs` | **新建** — 测试辅助 |
| `rust-platform/tests/real_workflow_gitea.rs` | **新建** — E2E 集成测试（#[ignore]） |
| **web-platform** | |
| `web-platform/src/git_url.rs` | 修改 — 添加 `Platform::Gitea` 枚举 + 识别逻辑 |
| `web-platform/src/templates/mod.rs` | 修改 — 添加模板常量 + match arms + platform_endpoint 构造 |
| `web-platform/src/templates/workflow_gitea.md` | **新建** — Agent workflow 模板 |
| `web-platform/src/templates/workflow_test_gitea.md` | **新建** — Test-engineer 模板 |
| `web-platform/src/process_manager/spawn.rs` | 修改 — 添加 `Platform::Gitea` arm（GITEA_TOKEN 环境变量注入 + platform_host 默认值） |
| `web-platform/src/handlers/project_workflow.rs` | 修改 — 添加 `"gitea"` match arms（3 处） |
| `web-platform/src/models/user.rs` | 修改 — UserConfig 添加 `gitea_token` + `gitea_host` 字段 |
| `web-platform/src/handlers/user_profile.rs` | 修改 — 暴露 gitea_token/gitea_host 读写 API |
| `web-platform/migrations/` | **新建** — 添加 gitea_token、gitea_host 列到 user_configs 表 |

## WORKFLOW.md 配置示例

```yaml
tracker:
  kind: gitea
  endpoint: https://gitea.example.com/api/v1
  api_key: $GITEA_TOKEN
  project_slug: myorg/myrepo
  active_states:
    - Todo
    - In Progress
  terminal_states:
    - Done
    - Closed
```

## 安全考量

### 1. SSRF 防护（HIGH — 必须实现）

Gitea 是自托管的，`base_url` 可能指向内网。当前 `validate_base_url()` 在 `allow_custom_host: true` 时直接放行，无 IP 级别校验。

**实现方案**：在 `validator.rs` 中添加 `is_private_or_reserved()` 检查，即使 `allow_custom_host: true` 也必须执行：

```rust
use std::net::IpAddr;

fn is_private_or_reserved(url_str: &str) -> bool {
    let Ok(url) = url::Url::parse(url_str) else { return false };
    let Some(host) = url.host_str() else { return false };

    // 已知 metadata endpoints
    if host == "169.254.169.254" || host == "metadata.google.internal" {
        return true;
    }

    // 解析为 IP 时检查私有/保留段
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()           // 127.0.0.0/8
                || v4.is_private()         // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()      // 169.254.0.0/16
                || v4.is_unspecified()     // 0.0.0.0
            }
            IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unspecified()
            }
        };
    }

    false  // hostname 无法在配置时完全校验（DNS rebinding），但至少拦截明显的 IP
}
```

在 `validate_base_url()` 中调用：

```rust
pub fn validate_base_url(url: &str, allow_custom_host: bool) -> Result<(), ValidationError> {
    // ... 现有 HTTPS 检查 ...

    // 即使 allow_custom_host=true，也拦截私有/保留 IP
    if is_private_or_reserved(url) {
        return Err(ValidationError::InvalidValue {
            field: "endpoint".to_string(),
            detail: "endpoint must not point to private/reserved IP ranges (RFC 1918, link-local, loopback, cloud metadata)".to_string(),
        });
    }

    if allow_custom_host {
        return Ok(());
    }
    // ... 现有 ALLOWED_HOSTS 检查 ...
}
```

**局限性**：hostname-based SSRF（如 `https://internal.corp/api/v1`）无法在配置时完全阻止（DNS rebinding）。这是所有自托管平台的共同风险，不阻塞 Gitea 实现。

### 2. 认证头注入防护

使用 `HeaderValue::from_str(...).map_err(|_| PlatformError::InvalidToken)?` 确保 token 不含 CRLF。与现有 GitHub/GitLab 分支保持一致。

### 3. 路径遍历防护

校验 owner/repo 只含 `[a-zA-Z0-9._-]`，防止 `../../admin` 类攻击。在 `validator.rs` 的 `validate_gitea_specifics()` 中实现。

### 4. 错误信息泄露

自托管 Gitea 的错误响应可能含内部路径。在 `GiteaAdapter` 的错误处理中截断 response body：

```rust
let body_text = response.text().await.unwrap_or_default();
let truncated = if body_text.len() > 500 { &body_text[..500] } else { &body_text };
Err(PlatformError::from_status(status_code, truncated))
```

### 5. Rate limiting

解析 Gitea 的 `X-RateLimit-Remaining` / `X-RateLimit-Reset` header，避免耗尽自托管实例的限额。

## 验证方式

1. `cargo build` — 确认编译通过
2. `cargo test` — 确认现有测试不受影响
3. 新增的 wiremock 测试覆盖核心 API 交互（认证头、分页参数、label ID 解析）
4. State 归一化交叉测试通过
5. 可选：Docker Gitea 实例 E2E 测试

## 已知 bug 顺带修复

- `build_workflow_states()`（`main.rs`）：当前只处理空格→下划线，遗漏了连字符。修复为 `replace([' ', '-'], "_")`，与 `normalize_tracker_state()` 保持一致。
