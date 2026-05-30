# Gitea Platform Adapter 实现任务

## 任务概述

为 Symphony 平台添加 Gitea 支持，使用户可以使用自托管 Gitea 实例作为 issue tracker，与现有的 GitHub 和 GitLab adapter 并列。

## 完成日期

2026-05-29

## 提交

`0a93aa6` — `feat: add Gitea platform adapter with full tracker support`

## 变更范围

| 模块 | 新建文件 | 修改文件 |
|------|----------|----------|
| rust-platform | 4 | 7 |
| web-platform | 3 | 10 |
| **总计** | **7** | **17** |

## 实现内容

### rust-platform（核心 adapter）

1. **`src/platform/gitea.rs`** — 完整 `Platform` trait 实现
   - 认证：`Authorization: token <TOKEN>`
   - 分页：自定义 `paginated_get()` 使用 `?limit=50&page=N`
   - Label 操作：ID-based，带 name→ID 缓存（lazy 加载 + 自动刷新 + 1000 条上限）
   - PR 过滤：`fetch_candidate_issues` 排除 `pull_request.is_some()` 的条目
   - Closed state 推断：复用 GitHub 的 `inferred_closed_issue_state()` 逻辑
   - 错误响应截断：body 超过 500 字符时截断，防止内部路径泄露

2. **`src/config/service_config.rs`** — `TrackerKind::Gitea`
   - 解析、默认端点（空，自托管无默认值）、环境变量 `GITEA_TOKEN`
   - `validate_for_dispatch()` 强制要求 `project_slug` 和 `endpoint`

3. **`src/config/validator.rs`** — 安全校验
   - `validate_gitea_specifics()`：owner/repo 只允许 `[a-zA-Z0-9._-]`，不允许空值
   - `is_private_or_reserved()`：SSRF 防护，拦截 RFC 1918、link-local、loopback、云 metadata IP

4. **`src/platform/http_client.rs`** — 共享 HTTP 层
   - `build_client_with_token()`：`"gitea"` arm
   - `list_labels()`：`"github" | "gitea"` 合并（路径相同）
   - `ensure_workflow_labels()`：`"github" | "gitea"` 合并

5. **`src/main.rs`**
   - `TrackerKind::Gitea` 初始化 arm
   - `build_platform_config()` 支持 `"gitea"` 走 owner/repo 解析
   - **Bug fix**：`build_workflow_states()` 连字符处理 `.replace(' ', "_")` → `.replace([' ', '-'], "_")`

### web-platform（API/模板/数据库）

1. **`src/git_url.rs`** — `Platform::Gitea` 枚举变体
2. **`src/templates/workflow_gitea.md`** — Agent workflow 模板
   - 安全 `gitea_api()` shell 封装（`--config <(...)` 隐藏 token、`--proto '=https'` 强制 HTTPS）
   - `transition_label()` / `get_label_id()` 辅助函数
3. **`src/templates/workflow_test_gitea.md`** — Test-engineer 模板
4. **`src/templates/mod.rs`** — 模板注册 + `platform_endpoint` 直接透传
5. **`src/process_manager/spawn.rs`** — `GITEA_TOKEN` 环境变量注入 + `platform_host` 处理
6. **`src/handlers/`** — `project_workflow.rs`、`projects.rs`、`user_profile.rs` 添加 Gitea arms
7. **`src/models/user.rs`** — `gitea_token` + `gitea_host` 字段
8. **`src/repository/`** — `upsert_config` / `get_config` 支持新字段
9. **`migrations/V011__gitea_token.sql`** — 数据库 schema 迁移

### 测试

| 测试文件 | 类型 | 数量 |
|----------|------|------|
| `tests/gitea_adapter_test.rs` | wiremock 单元测试 | 16 |
| `tests/real_workflow_gitea.rs` | E2E 集成测试（`#[ignore]`） | 7 |
| `tests/common/gitea_host.rs` | GitHost trait 实现 | — |
| `src/platform/gitea.rs` (内联) | 单元测试 | 5 |

**E2E 测试覆盖：**
- 认证验证（`GET /user`）
- Issue 获取 + 分页 + label 过滤
- 完整生命周期（创建 → label 转换 → workpad CRUD → 关闭）
- PR 创建
- State 归一化交叉匹配
- 模板渲染验证
- Codex agent 集成（启动 codex app-server，验证 agent 执行 label 转换 + workpad 创建 + 文件创建）

## 设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 实现方式 | 独立 adapter | 边界清晰，未来扩展方便 |
| State 管理 | Label-based | 与 GitHub/GitLab 一致 |
| Tracker 层 | 复用 `GitlabTrackerAdapter` | 该 wrapper 已支持任意 `Platform` 实现 |
| 分页 | 自定义 `paginated_get()` | Gitea 不识别 `per_page`，不能复用 `HttpClient::get_all_pages` |
| Label 操作 | ID-based + 缓存 | Gitea API 要求 label ID |
| 模板 CLI | `gitea_api()` shell 函数 | 无官方 CLI，安全封装 curl |
| `detect_platform()` | 不修改 | Gitea 自托管 hostname 不固定，通过 UI 显式选择 |

## 已知局限

1. SSRF 防护不防 DNS rebinding（hostname 解析到内网 IP）— 所有自托管平台的共同风险
2. `fetch_candidate_issues` 每个 active state label 单独请求（与 GitHub 一致，可后续优化为逗号分隔）
3. 前端 UI 不在此仓库中，需要在前端项目中添加 Gitea token/host 的输入表单

## 验证方式

```bash
# 编译
cargo build --workspace

# 单元测试
cargo test -p symphony-platform --test gitea_adapter_test

# E2E 测试（需要环境变量）
export GITEA_TOKEN='<token>'
export GITEA_BASE_URL='https://gitea.example.com/api/v1'
export TEST_REPO_NAME='owner/repo'
export E2E_PLATFORM='gitea'
cargo test -p symphony-platform --test real_workflow_gitea -- --ignored --nocapture
```

## 配置示例

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
