# Symphony Web 管理平台方案

## 1. 产品概述

基于现有 Symphony 服务（issue → codex → PR 全流程），构建一个 Web 管理系统，提供项目管理、看板视图、并行控制和多人协作能力。

### 核心目标

- 在 Web 上创建 Issue 并追踪进度
- 看板视图展示 Issue 全生命周期（待处理 → 处理中 → PR）
- 管理多个项目及其 Symphony 服务实例
- 控制 Codex 并行数
- 多人协作，各自使用自己的 GitLab/GitHub Token

## 2. 系统架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Web Management Platform                       │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────┐    ┌──────────────────────────────────────────┐   │
│  │  Frontend    │    │           Backend (Axum)                  │   │
│  │  (React)     │◄──►│  ┌────────┐ ┌────────┐ ┌────────────┐   │   │
│  └──────────────┘    │  │ Auth   │ │Project │ │  Kanban    │   │   │
│                      │  │ Module │ │Manager │ │  Service   │   │   │
│                      │  └────────┘ └────────┘ └────────────┘   │   │
│                      │  ┌────────┐ ┌────────┐ ┌────────────┐   │   │
│                      │  │ User   │ │Service │ │ Concurrency│   │   │
│                      │  │ Module │ │Control │ │  Control   │   │   │
│                      │  └────────┘ └────────┘ └────────────┘   │   │
│                      └──────────────────────────────────────────┘   │
│                                       │                              │
│                      ┌────────────────┼────────────────┐            │
│                      ▼                ▼                ▼             │
│              ┌──────────┐    ┌──────────────┐   ┌──────────┐       │
│              │  SQLite  │    │ Symphony     │   │ GitLab/  │       │
│              │  (本地DB) │    │ Instances    │   │ GitHub   │       │
│              └──────────┘    │ (子进程管理)  │   │ API      │       │
│                              └──────────────┘   └──────────┘       │
└─────────────────────────────────────────────────────────────────────┘
```

### 部署模型

```
Web Platform (单进程)
├── HTTP Server (Axum) — 提供 API + 静态前端
├── Process Manager — 管理多个 Symphony 实例
│   ├── symphony-rust --workflow ./projects/repo-a/WORKFLOW.md
│   ├── symphony-rust --workflow ./projects/repo-b/WORKFLOW.md
│   └── ...
└── SQLite — 持久化项目/用户/配置
```

每个项目对应一个独立的 `symphony-rust` 子进程，由 Web 平台统一管理生命周期。

## 3. 数据库设计 (SQLite)

### 3.1 用户表

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    role TEXT NOT NULL DEFAULT 'user',  -- 'admin' | 'user'
    deleted_at TEXT,                    -- 软删除标记，非 NULL 表示已删除
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_users_deleted_at ON users(deleted_at);
```

### 3.2 用户配置表

```sql
CREATE TABLE user_configs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL UNIQUE REFERENCES users(id),
    gitlab_token TEXT,           -- 加密存储
    gitlab_host TEXT,            -- 自定义 GitLab 实例地址
    github_token TEXT,           -- 加密存储
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### 3.3 项目表

```sql
CREATE TABLE projects (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    description TEXT,
    git_url TEXT NOT NULL UNIQUE,
    platform TEXT NOT NULL,      -- 'gitlab' | 'github'
    platform_host TEXT,          -- GitLab 自定义域名，GitHub 为 null
    namespace TEXT NOT NULL,     -- owner/group
    repo_name TEXT NOT NULL,
    default_branch TEXT DEFAULT 'main',
    workflow_template TEXT NOT NULL DEFAULT 'default', -- 'default' | 'custom'
    workflow_content TEXT,       -- 自定义 WORKFLOW.md 内容，为 NULL 时使用平台默认模板
    service_status TEXT NOT NULL DEFAULT 'stopped',  -- 'running' | 'stopped' | 'error'
    service_pid INTEGER,
    created_by INTEGER REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_projects_service_status ON projects(service_status);
CREATE INDEX idx_projects_platform ON projects(platform);
```

### 3.4 项目成员表

```sql
CREATE TABLE project_members (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id),
    role TEXT NOT NULL DEFAULT 'member',  -- 'owner' | 'member'
    synced_from TEXT,                     -- 'gitlab' | 'github' | NULL(手动添加)
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(project_id, user_id)
);

CREATE INDEX idx_project_members_user ON project_members(user_id);
CREATE INDEX idx_project_members_project ON project_members(project_id);
```

### 3.5 系统配置表

```sql
CREATE TABLE system_configs (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    description TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 默认配置
INSERT INTO system_configs (key, value, description) VALUES
('max_concurrent_codex', '5', '全局最大 Codex 并行数'),
('kanban_pending_limit', '50', '看板待处理 Issue 显示数量'),
('kanban_done_days', '7', '看板已完成 Issue 回溯天数');
```

## 4. 看板设计

### 4.1 三列看板

| 列 | 数据来源 | 说明 |
|----|----------|------|
| 待处理 | GitLab/GitHub API 获取 open issues（无 `symphony-claimed` label） | 默认前 50 条 |
| 处理中 | GitLab/GitHub API 获取带 `symphony-claimed` label 的 issues | Codex 回复 issue 即标记 |
| PR | GitLab/GitHub API 获取关联的 MR/PR | 展示 PR 状态及关联 issues |

### 4.2 Issue-PR 关联关系获取

**GitLab**：直接支持

```
GET /projects/:id/issues/:issue_iid/related_merge_requests
```

GitLab 原生提供 issue 与 MR 的关联查询，返回所有通过 `Closes #N`、`Fixes #N` 等关键字关联的 MR。

**GitHub**：通过 Timeline Events

```
GET /repos/{owner}/{repo}/issues/{issue_number}/timeline
```

过滤 `cross-referenced_event` 类型事件中引用的 Pull Request。或使用 GraphQL：

```graphql
query {
  repository(owner: "owner", name: "repo") {
    issue(number: 123) {
      timelineItems(itemTypes: [CROSS_REFERENCED_EVENT]) {
        nodes {
          ... on CrossReferencedEvent {
            source {
              ... on PullRequest { number title state url }
            }
          }
        }
      }
    }
  }
}
```

**结论**：两个平台均支持获取 Issue-PR 关联关系，可以实现看板需求。

### 4.3 PR 详情视图

PR 卡片点击展开后显示：
- PR 基本信息（标题、状态、作者、分支）
- 所有关联的 Issue 列表
- Review 状态
- CI 状态

### 4.4 数据获取策略

看板数据直接在线从 GitLab/GitHub API 获取，不做本地缓存持久化：
- 用户打开看板时实时请求
- 前端可做短时缓存（30s）避免频繁刷新
- 使用用户自己的 Token 调用 API（确保权限隔离）

## 5. 项目管理

### 5.1 创建项目流程

```
用户输入 Git URL
    │
    ▼
解析 URL → 识别平台(gitlab/github) + namespace + repo_name
    │
    ▼
使用用户 Token 调用 API → 获取项目描述/README（验证地址有效性）
    │
    ▼
写入 SQLite → 生成 WORKFLOW.md 配置
    │
    ▼
项目创建完成（服务默认停止状态）
```

**URL 解析规则**：

| 输入格式 | 平台 | namespace | repo |
|----------|------|-----------|------|
| `https://gitlab.com/group/project` | gitlab | group | project |
| `https://gitlab.example.com/group/sub/project` | gitlab | group/sub | project |
| `https://github.com/owner/repo` | github | owner | repo |
| `git@gitlab.com:group/project.git` | gitlab | group | project |
| `git@github.com:owner/repo.git` | github | owner | repo |

### 5.2 服务管理

| 操作 | 前置条件 | 行为 |
|------|----------|------|
| 启动服务 | 项目已创建，全局并行未满 | 启动 symphony-rust 子进程 |
| 停止服务 | 服务运行中 | 发送 SIGTERM，等待优雅关闭 |
| 重启服务 | 服务运行中 | 停止 → 启动 |
| 删除项目 | 服务已停止 | 删除数据库记录 + 清理配置 |

### 5.3 服务状态监控

Web 平台定期（每 10s）检查子进程存活状态：
- 进程存在且响应 health check → `running`
- 进程不存在 → `stopped`
- 进程存在但 health check 失败 → `error`

### 5.4 WORKFLOW.md 配置

Symphony 子进程启动时需要 WORKFLOW.md 文件来定义工作流程。平台内置两套默认模板，并支持用户自定义。

**默认模板**：

| 平台 | 模板来源 |
|------|---------|
| GitHub | 内置 `WORKFLOW.md.github` 模板（使用 `gh` CLI） |
| GitLab | 内置 `WORKFLOW.md.gitlab` 模板（使用 `glab` CLI） |

**模板关键配置项**（创建项目时根据项目信息自动填充）：

```yaml
tracker:
  kind: github | gitlab
  project_slug: "{namespace}/{repo_name}"
workspace:
  root: ~/symphony-workspaces/{project_id}
agent:
  max_concurrent_agents: {由并行控制分配}
```

**自定义 WORKFLOW.md**：

- 项目创建时默认使用平台对应的内置模板
- 管理员可在项目设置中切换为"自定义模式"，编辑完整的 WORKFLOW.md 内容
- 自定义内容存储在 `projects.workflow_content` 字段
- 切换回"默认模式"时恢复使用内置模板

**子进程启动时的 Token 注入**：

Web 平台在启动 Symphony 子进程时，从数据库实时读取项目 owner 的 Token（解密），通过 `tokio::process::Command::env()` 临时注入到子进程环境变量中。Token 仅存在于子进程的内存空间，不写入磁盘文件。

```rust
// 伪代码：启动子进程时临时注入 Token
let token = decrypt_token(owner.gitlab_token)?;  // 从 DB 读取并解密

Command::new("symphony-rust")
    .arg("--workflow")
    .arg(&workflow_path)
    .env("GITLAB_TOKEN", &token)  // 临时注入，仅子进程可见
    .env("GITLAB_HOST", &project.platform_host)
    .spawn()?;
// token 变量在此作用域结束后即被 drop，不持久化
```

注入时机与生命周期：
- **注入时机**：每次执行 `POST /api/projects/:id/start` 启动服务时
- **Token 来源**：实时从 SQLite 读取项目 owner 的 `user_configs` 并解密
- **作用域**：仅子进程环境变量，父进程和其他子进程不可见
- **更新机制**：owner 在 Web 上更新 Token 后，需重启服务才能生效（重启时会读取最新 Token）
- **安全性**：Token 不落盘、不写配置文件，子进程退出后环境变量随之销毁

### 5.5 项目成员管理

项目可见性基于成员关系控制：用户只能看到自己所属的项目。

**成员角色**：

| 角色 | 权限 |
|------|------|
| owner | 项目全部操作（启停服务、修改配置、管理成员、删除项目） |
| member | 查看项目、看板操作、创建 Issue |

**成员来源**：

1. **手动添加**：owner 在项目设置中添加平台已注册用户
2. **平台同步**（可选）：从 GitLab/GitHub 同步项目成员
   - GitLab：`GET /projects/:id/members`
   - GitHub：`GET /repos/{owner}/{repo}/collaborators`
   - 同步的成员标记 `synced_from` 字段，便于区分来源
   - 同步为手动触发（非自动），避免 Token 权限不足时静默失败

**平台成员同步匹配规则**：

同步时通过 `username` 匹配本系统已注册用户：
- 平台返回的成员 username 与本系统 `users.username` 匹配成功 → 自动添加为项目 member
- 匹配失败（平台成员未在本系统注册）→ 跳过，不自动创建用户
- 同步完成后，前端展示匹配结果：已关联 N 人，未匹配 M 人（附未匹配的 username 列表，提示管理员创建对应账号）
- 已存在的成员关系不会被重复创建（UNIQUE 约束保证幂等）
- 同步不会删除已有成员（增量添加，不自动移除）

**注意**：admin 角色用户可查看和管理所有项目，不受成员关系限制。

## 6. 并行控制

### 6.1 全局并行限制

```
系统配置: max_concurrent_codex = 5（默认）

分配策略:
- 每个项目的 Symphony 实例共享全局 Codex 额度
- Web 平台在启动/调度时检查当前总并行数
- 超出限制时排队等待
```

### 6.2 实现方式

Web 平台作为中央调度器，通过以下方式控制并行：

1. 每个 Symphony 实例的 `max_concurrent_agents` 由 Web 平台动态分配
2. Web 平台汇总所有实例的活跃 Agent 数（通过各实例的 HTTP API `/api/v1/state`）
3. 当总数达到上限时，暂停新任务调度

### 6.3 用户可配置项

- 全局最大并行数（系统配置）
- 单项目最大并行数（项目级配置，可选）

## 7. 多人协作

### 7.1 用户体系

| 角色 | 权限 |
|------|------|
| admin | 用户管理、系统配置、所有项目管理、看板 |
| user | 个人配置、所属项目的看板查看、Issue 创建 |

**项目可见性**：user 角色只能看到自己作为成员的项目（见 5.5 项目成员管理）。admin 角色可查看所有项目。

### 7.2 Token 隔离

每个用户配置自己的 GitLab/GitHub Token：
- 看板数据使用当前登录用户的 Token 获取
- Issue 创建使用当前用户的 Token（Issue 作者即为该用户）
- 服务运行使用项目 owner 的 Token（通过环境变量注入子进程，见 5.4）

### 7.3 Issue 作者标识

- GitLab/GitHub API 返回的 Issue 自带 `author` 字段
- 看板中显示作者头像和用户名作为 Tag
- 支持按作者筛选

### 7.4 PR 归属识别

- **GitLab MR**：`author` 字段直接标识创建者
- **GitHub PR**：`user` 字段标识创建者
- 由于 Codex 创建的 PR 使用的是项目级 Token，PR 作者会是 Token 对应的账号
- 解决方案：通过 PR 关联的 Issue 作者来反推 PR 归属（Issue 作者 = 任务发起人）

## 8. 用户管理

### 8.1 设计原则

- 不对外开放注册入口
- 仅管理员可添加用户
- 系统初始化时创建默认管理员账号

### 8.2 认证方式

- JWT Token 认证（单 Token 方案）
- 登录接口：`POST /api/auth/login`
- Token 有效期 7 天
- 签名算法限定 HS256，拒绝 `alg: none`
- 用户删除/密码修改时即时失效（内存黑名单 + SQLite 持久化）
- 登录 rate limit：同一用户名 5 次/分钟，同一 IP 20 次/分钟

### 8.3 用户操作

| 操作 | 权限 | 说明 |
|------|------|------|
| 添加用户 | admin | 设置用户名、初始密码、角色 |
| 删除用户 | admin | 软删除（设置 deleted_at） |
| 编辑个人信息 | 本人 | display_name |
| 配置 GitLab Token | 本人 | 加密存储 |
| 配置 GitHub Token | 本人 | 加密存储 |
| 重置密码 | admin | 重置为临时密码 |
| 修改密码 | 本人 | 需验证旧密码 |

## 9. AI 辅助 Issue 创建

### 9.1 设计目标

用户在 Web 上创建 Issue 时，提供 AI 辅助输入能力。用户只需输入简短的需求描述，AI 自动生成符合项目 WORKFLOW.md 规范的结构化 Issue 内容，确保 Codex Agent 能高效理解和执行。

### 9.2 AI 模型配置

| 配置项 | 值 | 来源 |
|--------|-----|------|
| 模型 | gpt-5.5 | Azure OpenAI |
| Base URL | `AZURE_OPENAI_BASEURL` | 服务端环境变量 |
| API Key | `AZURE_OPENAI_API_KEY` | 服务端环境变量 |

后端通过环境变量读取 Azure OpenAI 配置，前端无需感知密钥。

### 9.3 Issue 模板来源

AI 生成 Issue 时的模板直接来源于项目的 WORKFLOW.md 和 SPEC.md。

**从 WORKFLOW.md 提取的关键约束**：

WORKFLOW.md 中明确要求 Codex Agent 将 Issue 中的 `Validation`、`Test Plan`、`Testing` 等字段视为不可协商的验收输入：

> Treat any ticket-authored `Validation`, `Test Plan`, or `Testing` section as non-negotiable acceptance input: mirror it in the workpad and execute it before considering the work complete.

Agent 的 Workpad 模板结构为：

```markdown
## Codex Workpad

### Plan
- [ ] 1. Parent task
  - [ ] 1.1 Child task

### Acceptance Criteria
- [ ] Criterion 1
- [ ] Criterion 2

### Validation
- [ ] targeted tests: `<command>`

### Notes
- <short progress note>
```

这意味着 Issue 的 description 中如果包含结构化的 Acceptance Criteria 和 Validation，Agent 会直接映射到执行计划中。

**从 SPEC.md 提取的 Issue 数据模型**：

```
Issue Fields:
- identifier: 人类可读的 ticket key（如 ABC-123）
- title: 标题
- description: 描述（AI 生成的主要目标字段）
- priority: 优先级（数字越小越高）
- state: 当前状态
- labels: 标签列表
- blocked_by: 阻塞依赖
```

**最终 Issue 模板（AI 生成目标格式）**：

```markdown
## 描述

<需求背景、当前问题、期望行为的清晰描述>

## Acceptance Criteria

- [ ] <可验证的验收条件 1>
- [ ] <可验证的验收条件 2>
- [ ] <可验证的验收条件 3>

## Validation

- [ ] <具体验证步骤 1>: `<可执行的命令或操作路径>`
- [ ] <具体验证步骤 2>: `<可执行的命令或操作路径>`

## Notes

- <实现提示、技术约束或相关上下文（可选）>
```

此模板确保：
1. Agent 能直接将 Acceptance Criteria 复制到 Workpad 中作为执行清单
2. Validation 部分会被 Agent 视为必须执行的验证步骤（non-negotiable）
3. Notes 提供额外上下文帮助 Agent 理解实现方向

### 9.4 生成规则

根据当前 WORKFLOW.md 的要求，AI 生成的 Issue 必须包含以下结构：

```markdown
## 描述

<需求的详细描述，包含背景和目标>

## Acceptance Criteria

- [ ] 验收条件 1
- [ ] 验收条件 2

## Validation

- [ ] 验证步骤 1：`<具体命令或操作>`
- [ ] 验证步骤 2：`<具体命令或操作>`

## Notes

- 实现提示或约束（可选）
```

这些字段对应 WORKFLOW.md 中 Codex Workpad 的结构，确保 Agent 能直接映射到执行计划。

### 9.5 交互流程

```
用户输入简短需求描述（如："修复登录页面在移动端的样式错乱"）
    │
    ▼
前端调用 AI 生成接口
    │
    ▼
后端组装 Prompt:
  - System: 项目 WORKFLOW.md 中的 Issue 结构要求 + 项目描述
  - User: 用户输入的需求描述
    │
    ▼
调用 Azure OpenAI (gpt-5.5) → 流式返回
    │
    ▼
前端实时展示生成结果（Streaming）
    │
    ▼
用户可编辑/调整生成内容
    │
    ▼
确认后提交创建 Issue
```

### 9.6 System Prompt 设计

```text
你是一个技术 Issue 编写助手。根据用户的简短需求描述，生成结构化的 Issue 内容。

项目背景：
{project_description}

Issue 必须严格包含以下结构（Agent 会将这些字段直接映射到执行计划）：

1. **描述**：清晰说明需求背景、当前问题和期望行为
2. **Acceptance Criteria**：可验证的验收条件清单（checkbox 格式）。Agent 会逐条检查，全部通过才算完成。
3. **Validation**：具体的验证步骤，必须包含可执行的命令或 UI 操作路径。Agent 会将此视为不可跳过的必执行验证。
4. **Notes**（可选）：实现提示、技术约束或相关上下文

输出格式：
## 描述

<内容>

## Acceptance Criteria

- [ ] <条件>

## Validation

- [ ] <步骤>: `<命令>`

## Notes

- <提示>

要求：
- 描述要具体，避免模糊表述
- 验收条件要可测量、可验证，每条独立可判定
- 验证步骤必须包含具体命令（如 `cargo test`、`curl` 请求）或明确的 UI 操作路径
- 使用中文撰写（除代码和命令外）
- 不要包含 Issue 标题（标题由用户单独填写）
```

### 9.7 API 接口

```
POST /api/projects/:id/issues/ai-generate
```

**Request**：
```json
{
  "prompt": "用户输入的简短需求描述",
  "title": "可选，用户已填写的标题"
}
```

**Response**（SSE 流式）：
```
data: {"type": "chunk", "content": "## 描述\n\n"}
data: {"type": "chunk", "content": "登录页面在移动端..."}
...
data: {"type": "done", "content": "<完整生成内容>"}
```

### 9.8 前端交互设计

创建 Issue 页面布局：

```
┌─────────────────────────────────────────────────┐
│  创建 Issue                                      │
├─────────────────────────────────────────────────┤
│                                                  │
│  标题: [________________________]                │
│                                                  │
│  快速描述: [用一句话描述你的需求___________]      │
│  [AI 生成] 按钮                                  │
│                                                  │
│  ┌─────────────────────────────────────────┐    │
│  │  Issue 内容（Markdown 编辑器）           │    │
│  │                                          │    │
│  │  ## 描述                                 │    │
│  │  ...（AI 生成或手动编写）                 │    │
│  │                                          │    │
│  │  ## Acceptance Criteria                  │    │
│  │  - [ ] ...                               │    │
│  │                                          │    │
│  │  ## Validation                           │    │
│  │  - [ ] ...                               │    │
│  └─────────────────────────────────────────┘    │
│                                                  │
│  Labels: [选择标签]                              │
│                                                  │
│  [取消]                          [创建 Issue]    │
└─────────────────────────────────────────────────┘
```

- AI 生成按钮点击后，内容区域实时流式填充
- 生成过程中显示 loading 状态，支持中断
- 生成完成后用户可自由编辑
- 支持重新生成（覆盖当前内容，需确认）

### 9.9 后端实现要点

```rust
// Azure OpenAI 客户端配置
struct AzureOpenAIConfig {
    base_url: String,    // from AZURE_OPENAI_BASEURL
    api_key: String,     // from AZURE_OPENAI_API_KEY
    model: String,       // "gpt-5.5"
}

// Issue 生成请求处理
async fn generate_issue(
    project_id: i64,
    user_prompt: String,
) -> impl IntoResponse {
    // 1. 从 DB 获取项目信息
    // 2. 读取项目的 WORKFLOW.md 模板上下文
    // 3. 组装 system prompt + user prompt
    // 4. 调用 Azure OpenAI streaming API
    // 5. 以 SSE 流式返回给前端
}
```

## 10. 告警与通知系统

### 10.1 架构设计

告警系统与通知系统解耦，分为两个独立模块：

```
┌─────────────────────────────────────────────────────────────────┐
│                        告警与通知架构                              │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────────────┐         ┌──────────────────────────┐  │
│  │   Alert Engine        │         │   Notification Dispatcher │  │
│  │   (预警引擎)          │────────►│   (通知分发器)             │  │
│  │                       │  Alert  │                           │  │
│  │  ┌────────────────┐  │  Event  │  ┌─────────────────────┐ │  │
│  │  │ Rule Evaluator │  │────────►│  │ Channel Router      │ │  │
│  │  └────────────────┘  │         │  └──────┬──────────────┘ │  │
│  │  ┌────────────────┐  │         │         │                 │  │
│  │  │ Metric Collector│  │         │  ┌──────▼──────────────┐ │  │
│  │  └────────────────┘  │         │  │ DingTalk Channel    │ │  │
│  │  ┌────────────────┐  │         │  ├─────────────────────┤ │  │
│  │  │ Alert History  │  │         │  │ (未来) Slack        │ │  │
│  │  └────────────────┘  │         │  ├─────────────────────┤ │  │
│  └──────────────────────┘         │  │ (未来) Email        │ │  │
│                                    │  └─────────────────────┘ │  │
│                                    └──────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 10.2 预警引擎（Alert Engine）

负责指标采集、规则评估和告警事件生成，不关心通知如何发送。

**告警规则类型**：

| 规则 | 触发条件 | 严重级别 | 说明 |
|------|----------|----------|------|
| 任务超时 | Codex 单任务运行时间 > 阈值 | warning | 默认阈值 30 分钟，可配置 |
| 任务失败 | Codex 任务异常退出且重试耗尽 | critical | 重试次数用完仍失败 |
| 服务异常 | Symphony 实例进程意外退出 | critical | 子进程非正常终止 |
| 并行饱和 | 全局 Codex 并行数达到上限持续 > N 分钟 | warning | 默认 10 分钟 |
| 连续失败 | 同一项目连续 N 个任务失败 | critical | 默认 3 个 |
| API 不可达 | GitLab/GitHub API 连续请求失败 | critical | 连续 5 次失败 |

**告警事件结构**：

```rust
struct AlertEvent {
    id: String,              // 唯一标识
    rule_id: String,         // 触发的规则
    severity: Severity,      // critical | warning | info
    project_id: Option<i64>, // 关联项目
    title: String,           // 告警标题
    message: String,         // 详细描述
    context: HashMap<String, String>, // 上下文（issue_id, duration 等）
    fired_at: DateTime<Utc>,
    resolved_at: Option<DateTime<Utc>>,
}

enum Severity {
    Critical,
    Warning,
    Info,
}
```

**指标采集来源**：

- Symphony 实例的 HTTP API（`/api/v1/state`）— 任务运行时间、活跃数
- 进程管理器 — 子进程存活状态
- 告警历史 — 连续失败计数

### 10.3 通知分发器（Notification Dispatcher）

接收 Alert Event，根据配置的通知渠道和路由规则分发通知。与预警引擎完全解耦，通过内部事件通道通信。

**核心抽象**：

```rust
#[async_trait]
trait NotificationChannel: Send + Sync {
    fn channel_type(&self) -> &str;
    async fn send(&self, alert: &AlertEvent) -> Result<()>;
    async fn health_check(&self) -> bool;
}
```

**路由规则**：

- 按严重级别路由：critical → 立即发送，warning → 聚合后发送
- 按项目路由：不同项目可配置不同通知渠道（未来扩展）
- 防抖：同一规则在冷却期内（默认 5 分钟）不重复发送

### 10.4 钉钉群机器人通知（第一期）

**配置**：

```sql
-- system_configs 表新增
INSERT INTO system_configs (key, value, description) VALUES
('dingtalk_webhook_url', '', '钉钉群机器人 Webhook URL'),
('dingtalk_secret', '', '钉钉群机器人加签密钥（可选）'),
('alert_task_timeout_minutes', '30', '任务超时告警阈值（分钟）'),
('alert_consecutive_failures', '3', '连续失败告警阈值'),
('alert_cooldown_seconds', '300', '同一告警冷却时间（秒）');
```

**钉钉消息格式**：

```json
{
  "msgtype": "markdown",
  "markdown": {
    "title": "⚠️ Symphony 告警",
    "text": "### ⚠️ 任务超时告警\n\n**项目**: my-project\n\n**Issue**: #42 修复登录Bug\n\n**运行时间**: 35 分钟（阈值 30 分钟）\n\n**时间**: 2026-05-20 14:30:00"
  }
}
```

**钉钉签名实现**：

```rust
fn dingtalk_sign(secret: &str, timestamp: i64) -> String {
    let string_to_sign = format!("{}\n{}", timestamp, secret);
    let hmac = hmac_sha256(secret.as_bytes(), string_to_sign.as_bytes());
    base64_encode(&hmac)
}
```

### 10.5 告警配置管理

管理员可在 Web 界面配置：

- 启用/禁用各告警规则
- 调整阈值参数
- 配置通知渠道（钉钉 Webhook URL + Secret）
- 查看告警历史记录

### 10.6 告警历史

```sql
CREATE TABLE alert_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rule_id TEXT NOT NULL,
    severity TEXT NOT NULL,
    project_id INTEGER REFERENCES projects(id),
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    context_json TEXT,
    fired_at TEXT NOT NULL,
    resolved_at TEXT,
    notified_at TEXT,
    notification_channel TEXT,
    notification_status TEXT  -- 'sent' | 'failed' | 'suppressed'
);

CREATE INDEX idx_alert_history_project ON alert_history(project_id);
CREATE INDEX idx_alert_history_fired_at ON alert_history(fired_at);
CREATE INDEX idx_alert_history_severity ON alert_history(severity);
```

### 10.7 扩展性

通知系统通过 `NotificationChannel` trait 解耦，未来扩展新渠道只需：

1. 实现 `NotificationChannel` trait
2. 在路由配置中注册新渠道
3. 前端添加对应的配置表单

预留渠道：
- Slack（Webhook）
- 企业微信（群机器人）
- Email（SMTP）
- 自定义 Webhook（通用 HTTP POST）

## 11. API 设计

### 11.0 通用协议

**接口规范文档**：使用 OpenAPI 3.0（Swagger）规范，后端通过 `utoipa` crate 自动生成 Swagger YAML，部署后可通过 `/swagger-ui` 访问在线文档。

**请求规范**：遵循标准 RESTful 协议，Content-Type 为 `application/json`。

**统一响应格式**：

```typescript
interface ResponseData<T = any> {
  data: T;
  success: boolean;
  retCode: string;
  retMsg: string;
  showType?: ErrorShowType;  // 0: silent, 1: warn, 2: error, 4: notification, 9: redirect
}
```

**分页响应格式**（用于列表接口）：

```typescript
interface PaginationData<T = any> {
  limit: number;
  offset: number;
  pageNo: number;
  pageSize: number;
  pages: number;
  records: T[];
  totalCount: number;
}
```

分页请求参数：`?pageNo=1&pageSize=20`（默认 pageNo=1, pageSize=20）。

**错误码规范**：

| retCode | 含义 | showType |
|---------|------|----------|
| `0` | 成功 | 0 (silent) |
| `AUTH_001` | 未登录或 Token 过期 | 9 (redirect to login) |
| `AUTH_002` | 权限不足 | 2 (error) |
| `BIZ_001` | 业务参数错误 | 1 (warn) |
| `BIZ_002` | 资源不存在 | 2 (error) |
| `BIZ_003` | 操作冲突（如服务运行中不可删除） | 1 (warn) |
| `SYS_001` | 系统内部错误 | 2 (error) |
| `EXT_001` | 外部服务不可用（GitLab/GitHub/AI） | 4 (notification) |

### 11.1 认证

```
POST   /api/auth/login              登录
PUT    /api/auth/password           修改密码
```

### 11.2 用户管理（admin）

```
GET    /api/admin/users             用户列表（分页）
POST   /api/admin/users             创建用户
DELETE /api/admin/users/:id         删除用户（软删除）
PUT    /api/admin/users/:id/reset-password  重置密码
```

### 11.3 个人配置

```
GET    /api/user/profile            获取个人信息
PUT    /api/user/profile            更新个人信息
GET    /api/user/config             获取个人配置
PUT    /api/user/config             更新 Token 配置
```

### 11.4 项目管理

```
GET    /api/projects                项目列表（含服务状态，按成员关系过滤）
POST   /api/projects                创建项目（输入 git URL）
GET    /api/projects/:id            项目详情
PUT    /api/projects/:id            更新项目配置（含 WORKFLOW.md）
DELETE /api/projects/:id            删除项目（owner 权限）
POST   /api/projects/:id/start      启动服务
POST   /api/projects/:id/stop       停止服务
POST   /api/projects/:id/restart    重启服务
GET    /api/projects/:id/status     服务运行状态
```

### 11.5 项目成员管理

```
GET    /api/projects/:id/members              成员列表
POST   /api/projects/:id/members              添加成员
PUT    /api/projects/:id/members/:user_id     更新成员角色
DELETE /api/projects/:id/members/:user_id     移除成员
POST   /api/projects/:id/members/sync         从 GitLab/GitHub 同步成员
```

### 11.6 看板与 AI 生成

```
GET    /api/projects/:id/kanban     获取看板数据
POST   /api/projects/:id/issues     创建 Issue
POST   /api/projects/:id/issues/ai-generate   AI 辅助生成 Issue 内容（SSE 流式）
GET    /api/projects/:id/issues/:iid          Issue 详情
GET    /api/projects/:id/issues/:iid/mrs      Issue 关联的 MR/PR
GET    /api/projects/:id/mrs/:iid             MR/PR 详情（含关联 issues）
```

### 11.7 系统配置（admin）

```
GET    /api/admin/config            获取系统配置
PUT    /api/admin/config            更新系统配置
GET    /api/admin/stats             全局统计（总并行数等）
```

### 11.8 告警与通知（admin）

```
GET    /api/admin/alerts            告警历史列表（分页、筛选）
GET    /api/admin/alerts/rules      获取告警规则配置
PUT    /api/admin/alerts/rules      更新告警规则（启用/禁用、阈值）
GET    /api/admin/alerts/channels   获取通知渠道配置
PUT    /api/admin/alerts/channels   更新通知渠道（钉钉 Webhook 等）
POST   /api/admin/alerts/test       发送测试通知（验证渠道连通性）
```

## 12. 技术选型

| 组件 | 技术 | 理由 |
|------|------|------|
| 后端框架 | Axum | 与现有 Symphony 一致，复用生态 |
| API 文档 | utoipa + Swagger UI | OpenAPI 3.0 规范，自动生成接口文档 |
| 数据库 | SQLite (rusqlite) | 轻量、无外部依赖、适合小项目 |
| 数据库迁移 | refinery | 基于版本号的 SQL 迁移，嵌入二进制 |
| 认证 | JWT (jsonwebtoken) | 无状态、适合 API 服务 |
| 密码哈希 | argon2 | 安全标准 |
| 前端框架 | React + Vite | 生态丰富、组件化开发 |
| 前端状态管理 | Zustand + React State | Zustand 管理全局状态，组件局部用 React State |
| 前端 HTTP 客户端 | Axios | 拦截器机制成熟，便于统一处理 Token 刷新和错误 |
| 样式 | Tailwind CSS | 原子化 CSS、快速迭代、无预设组件约束 |
| 后端 HTTP 客户端 | Reqwest | 复用现有依赖 |
| 进程管理 | tokio::process | 异步子进程管理 |
| 加密存储 | aes-gcm | Token 加密存储到 SQLite |
| AI 集成 | reqwest + SSE | 调用 Azure OpenAI API（gpt-5.5），流式返回 |

## 13. 前端页面规划

| 页面 | 路由 | 说明 |
|------|------|------|
| 登录 | `/login` | 用户名密码登录 |
| 看板 | `/projects/:id/kanban` | 三列看板视图 |
| 项目列表 | `/projects` | 所有项目 + 服务状态 |
| 项目设置 | `/projects/:id/settings` | 项目配置（含 WORKFLOW.md 编辑） |
| 项目成员 | `/projects/:id/members` | 成员管理 + 平台同步 |
| 个人设置 | `/settings` | 个人信息 + Token 配置 |
| 用户管理 | `/admin/users` | 管理员专属 |
| 系统配置 | `/admin/config` | 管理员专属 |
| 告警管理 | `/admin/alerts` | 告警历史 + 规则配置 + 通知渠道 |

## 14. 安全考虑

- Token 使用 AES-GCM 加密后存入 SQLite，密钥通过环境变量注入
- 密码使用 Argon2 哈希存储
- JWT 签名密钥通过环境变量配置
- API 全部需要认证（登录接口除外）
- 管理员接口额外校验角色
- 用户只能访问自己的配置
- 删除项目前强制停止服务，防止孤儿进程

## 15. 实施路径建议

### Phase 1：基础框架
- SQLite 数据库初始化 + refinery 迁移框架
- Repository trait 抽象 + SqliteRepository 实现
- 用户认证（登录、JWT、Refresh Token、黑名单）
- 管理员用户管理
- Graceful Shutdown 框架
- OpenAPI 3.0 文档生成（utoipa + Swagger UI）

### Phase 2：项目管理
- Git URL 解析 + 平台识别
- 项目 CRUD + WORKFLOW.md 配置管理
- 项目成员管理（手动添加 + 平台同步）
- Symphony 子进程生命周期管理

### Phase 3：看板
- GitLab/GitHub API 集成（Issue 列表、MR 关联）
- 三列看板视图
- Issue 创建
- AI 辅助 Issue 生成（Azure OpenAI gpt-5.5 集成）

### Phase 4：协作与控制
- 多用户 Token 隔离
- 全局并行控制
- 作者标识与筛选

### Phase 5：告警与通知
- 预警引擎（指标采集 + 规则评估）
- 通知分发器框架（Channel trait）
- 钉钉群机器人通知接入
- 告警历史与管理界面

## 16. 已知限制与迁移触发条件

本方案有意选择"最简技术解决真实问题"的路径，以下是已知限制及对应的架构升级触发条件。

### 16.1 架构限制

| 限制 | 原因 | 当前缓解措施 |
|------|------|-------------|
| 单机部署，无法水平扩展 | SQLite 文件数据库 + 子进程管理模型 | 内部工具定位，systemd 自动重启保障可用性 |
| SQLite 写入串行化 | WAL 模式下仍为单写者 | 写操作仅为配置变更和状态更新，QPS 极低 |
| 看板依赖外部 API 可用性 | 不做本地持久化缓存，数据一致性优先 | 服务端短时内存缓存（5-10s TTL）+ 优雅降级 |
| 子进程模型无资源限额 | 未引入容器编排 | PID 文件回收 + 进程存活检查 + 告警 |
| JWT 无法即时撤销 | 无状态 Token 设计 | 7 天有效期 + 内存黑名单（用户删除/密码修改时即时失效） |
| AI 功能依赖外部服务 | Azure OpenAI 单一供应商 | AI 不可用时回退手动创建，非核心路径 |

### 16.2 迁移触发条件

当以下任一条件满足时，应启动对应的架构升级：

| 触发条件 | 升级方向 |
|----------|---------|
| 用户数 > 50 或需要多节点部署 | SQLite → PostgreSQL（通过 Repository trait 切换） |
| 并发写入 > 100 QPS | 引入连接池 + 数据库迁移 |
| 需要跨机器管理 Symphony 实例 | 子进程 → gRPC 远程调度 或 容器编排 |
| GitLab/GitHub API Rate Limit 频繁触及 | 引入服务端持久化缓存层（Redis 或 SQLite 缓存表） |
| 告警渠道 > 3 个或需要复杂路由 | 引入消息队列解耦告警生产与消费 |
| 安全审计要求提升 | 环境变量 → HashiCorp Vault / 云 KMS |

### 16.3 开发前必须落实的设计保障

以下措施需在 Phase 1 中实现，为未来迁移预留接口：

1. **数据访问层抽象**：引入 Repository trait，当前实现为 `SqliteRepository`，确保未来可替换为 `PostgresRepository` 而不改动业务逻辑。

2. **服务端缓存层**：看板 API 加入内存缓存（5-10s TTL），相同请求在窗口期内复用结果（singleflight 模式，同一请求只发一次，其他等待）。API 不可用时展示最后成功数据并标注"数据可能过期"。

3. **Graceful Shutdown**：收到 SIGTERM 后停止接受新任务，等待当前 Symphony 子进程优雅退出（超时 30s 后 SIGKILL），持久化未完成的调度状态到 SQLite。

4. **子进程生命周期加固**：

   **互斥锁机制**：
   - 每个项目维护一个 per-project Mutex（`tokio::sync::Mutex`），所有服务操作（启动/停止/重启）必须先获取锁
   - 防止快速双击、网络重发等导致的并发操作冲突
   - 锁获取超时 5s，超时返回 `BIZ_003`（操作冲突）错误

   **PID 验证机制**：
   - 启动子进程后记录 PID + 启动时间戳 + 进程 cmdline 到 SQLite
   - 健康检查时验证三要素：(1) PID 存在；(2) `/proc/<pid>/cmdline` 包含 "symphony-rust"（macOS 下用 `ps -p <pid> -o comm=`）；(3) 启动时间戳匹配
   - 发送信号前必须通过 PID 验证，防止信号发送给被 OS 复用 PID 的无关进程
   - 验证失败时标记服务为 `stopped`，清除 PID 记录

   **启动前清理机制**：
   - 平台启动时扫描 `projects` 表中 `service_status = 'running'` 的记录
   - 对每条记录执行 PID 验证：验证通过则保持状态，验证失败则标记为 `stopped`
   - 对验证通过但属于旧平台实例的孤儿进程（通过 process group 判断），发送 SIGTERM 清理
   - 使用 process group（`setsid`）管理子进程，确保父进程退出时可通过 `killpg` 批量清理
   - Linux 下通过 `prctl(PR_SET_PDEATHSIG, SIGTERM)` 让子进程感知父进程退出并自行终止
   - macOS 下通过 kqueue `EVFILT_PROC` + `NOTE_EXIT` 监听父进程退出（或定期检查 `getppid() == 1`）

   **崩溃自动恢复**：
   - 子进程非正常退出后自动重试，最多 3 次，间隔递增 5s/15s/60s
   - 超过重试次数标记为 `failed`，触发 critical 告警
   - 重试期间记录每次崩溃的 exit code 和 stderr 最后 50 行，用于诊断

5. **AI Prompt 注入防护**：

   AI 生成的 Issue 内容会被 Codex Agent 执行（Validation 中的命令被视为 non-negotiable），构成从用户输入到代码执行的完整链路，必须在 Phase 1 中防护。

   **输入层防护**：
   - 用户输入长度限制：prompt ≤ 2000 字符，title ≤ 200 字符
   - 过滤明显的 prompt 注入模式：`ignore previous`、`system:`、`you are now` 等
   - System prompt 与 user prompt 严格分离，使用 OpenAI API 的 role 机制隔离

   **输出层审查**：
   - AI 生成的 Validation 字段中的命令，进行白名单校验：
     - 允许：`cargo test`、`cargo build`、`cargo clippy`、`npm test`、`pytest`、`curl`（仅 localhost）、`grep`、`cat`
     - 禁止：`rm`、`sudo`、`chmod`、`chown`、`kill`、`dd`、`mkfs`、`wget`/`curl`（外部 URL）、管道到 `sh`/`bash`、反引号执行
   - 包含禁止命令的生成结果，在对应行标注 `⚠️ 此命令需人工确认` 警告
   - 前端展示时高亮标注危险命令，用户必须手动确认或修改后才能提交

   **架构层隔离**：
   - AI 生成接口的 rate limit：每用户 10 次/分钟
   - 生成结果必须经过用户预览和确认，不可直接创建 Issue（方案已有此设计）
   - 后端日志记录每次 AI 生成的完整 prompt 和 response，用于事后审计

6. **JWT 认证方案**：
   - 单 Token 方案：JWT 有效期 7 天，签名算法限定 HS256，拒绝 `alg: none`
   - 用户删除/密码修改时，将该用户的所有 Token 加入内存黑名单（重启后从 SQLite `token_blacklist` 表恢复）
   - API middleware 在验签通过后，额外检查：(1) 用户是否在黑名单中；(2) 用户 `deleted_at` 是否非空
   - 登录接口 rate limit：同一用户名 5 次/分钟，同一 IP 20 次/分钟

### 16.4 Phase 1 后可补充的安全加固

| 加固项 | 优先级 | 说明 |
|--------|--------|------|
| Token 加密密钥轮换 | 中 | 支持多版本密钥，新加密用新密钥，解密时按版本选择 |
| SQLite 文件保护 | 中 | 文件权限 600 + 定期 `.backup` 命令 + 可选 SQLCipher 全库加密 |
| Token 访问审计日志 | 中 | 记录每次 Token 解密操作的时间、操作者、用途 |
| 告警系统自监控 | 低 | 外部 healthcheck 心跳（如 UptimeRobot），平台自身不可用时外部感知 |
| 告警升级机制 | 低 | 告警未确认超过 N 分钟后升级通知渠道或负责人 |
