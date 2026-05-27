# 全局看板方案

## 背景

当前看板是单项目维度（`/projects/:id/kanban`），运行多个项目时切换不便。需要一个全局看板页面，聚合所有"运行中"项目的看板信息。

## 设计决策

- **布局**：按项目分组，每个运行中项目一个独立区块，区块内仍为 Todo / In Progress / PR 三列
- **导航**：侧边栏"项目"分组上方新增"总览"独立入口
- **接口拆分**：将现有 kanban 接口拆为 issues 和 prs 两个独立接口，前端并行请求、先到先渲染

## 一、接口拆分（后端）

### 现状

`GET /api/projects/:id/kanban` 一次返回 todo + in_progress + pr 三列数据：
- Issues（todo + in_progress）：多次 `list_issues` 调用 + 并行获取每个 in_progress issue 的 MR count
- PRs：独立的一次 `list_merge_requests` 调用

### 拆分为两个接口

| 接口 | 返回内容 | 说明 |
|------|----------|------|
| `GET /api/projects/:id/kanban/issues` | `{ todo, in_progress, platform }` | Issues 两列，含 mr_count |
| `GET /api/projects/:id/kanban/prs` | `{ pr, platform }` | PR/MR 列 |

**查询参数**（与现有保持一致）：

- `/kanban/issues`：`todo_limit`, `assignee`, `labels`, `search`, `author`, `no_cache`
- `/kanban/prs`：`no_cache`（PR 列目前不受 assignee/labels 等过滤影响）

**缓存**：issues 和 prs 各自独立缓存 key 和 TTL。

**兼容性**：原 `/kanban` 接口保留，内部调用两个子逻辑组装返回（后续可废弃）。

## 二、全局看板接口（后端）

### 新增接口

| 接口 | 说明 |
|------|------|
| `GET /api/overview/kanban/issues` | 返回所有 running 项目的 issues 数据，按项目分组 |
| `GET /api/overview/kanban/prs` | 返回所有 running 项目的 PR 数据，按项目分组 |

### 查询参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `max_projects` | u32 | 10 | 最多返回项目数，上限 20 |
| `todo_limit` | u32 | 5 | 每个项目 todo 列最大 issue 数，上限 10 |

注意：Overview 接口**不支持** `no_cache` 参数，强制使用缓存以防止 API 放大攻击。

### 响应结构

**`/api/overview/kanban/issues`**：
```json
{
  "projects": [
    {
      "project_id": 1,
      "project_name": "my-app",
      "platform": "gitlab",
      "namespace": "group/subgroup",
      "repo_name": "my-app",
      "todo": {
        "issues": [...],
        "total_count": 5,
        "has_more": true
      },
      "in_progress": {
        "issues": [...],
        "total_count": 3
      },
      "error": null
    }
  ],
  "total_running_projects": 12,
  "has_more": true
}
```

**`/api/overview/kanban/prs`**：
```json
{
  "projects": [
    {
      "project_id": 1,
      "project_name": "my-app",
      "platform": "gitlab",
      "namespace": "group/subgroup",
      "repo_name": "my-app",
      "pr": {
        "merge_requests": [...],
        "total_count": 2
      },
      "error": null
    }
  ],
  "total_running_projects": 12,
  "has_more": true
}
```

两个接口使用统一的 `ProjectMeta`（project_id, project_name, platform, namespace, repo_name）确保前端无论哪个先返回都能渲染项目 header。

### 关键实现细节

#### 并发控制与超时

**GitHub/GitLab API 限制**：两者的 issues/MRs 接口都是单仓库维度，不支持跨项目聚合查询。后端实现：

1. DB 查询 `service_status = "running"` 的项目（SQL 级 JOIN membership 过滤，见下方权限章节）
2. 硬上限：最多处理 `max_projects` 个项目（默认 10，上限 20）
3. **并行获取**各项目数据，并发控制采用 **per-platform-host semaphore**（非 per-request）：
   - 同一 GitLab 实例：全局 max 5 并发项目
   - GitHub (github.com)：全局 max 5 并发项目
   - 不同平台实例之间互不影响
4. **Per-project timeout**：每个项目的获取操作包裹 `tokio::time::timeout(Duration::from_secs(10), ...)`，超时则标记该项目 `error: "timeout"`
5. **整体 endpoint timeout**：15s，超时返回已完成的项目数据 + 未完成项目标记 timeout

#### Overview 中跳过 mr_count

单项目看板中，in_progress 列的每个 issue 会并行调 `get_issue_merge_requests` 获取 mr_count。这在 overview 场景下会产生 N+1 放大（5 项目 × 10 issues = 50 次额外调用）。

**决策**：Overview 接口的 in_progress issues **不获取 mr_count**（返回 `mr_count: null`）。用户点击进入单项目看板时才加载完整数据。

#### 限流

Overview 接口使用**独立的 rate limiter**，与单项目看板分开：
- Overview：5 req/min/user（因为每次请求扇出多个外部调用）
- 单项目看板：保持现有 30 req/min/user

#### Token 管理

- Token 解析 **per-project**：根据 `project.platform` 字段选择对应的 `user_config.github_token` 或 `user_config.gitlab_token`
- 如果用户缺少某平台的 token，跳过该平台的项目并在响应中标记 `error: "no_token"`
- Token **lazy decrypt**：按平台分组，只解密实际需要的 token
- 解密后的 token 使用 `secrecy::SecretString`，用完即 zeroize

#### 权限（SQL 级过滤）

```sql
SELECT p.* FROM projects p
INNER JOIN project_members pm ON pm.project_id = p.id
WHERE p.service_status = 'running'
  AND pm.user_id = :current_user_id
ORDER BY p.updated_at DESC
LIMIT :max_projects
```

权限过滤必须在 SQL 层完成（JOIN），禁止先查所有 running 项目再在应用层过滤。

#### 缓存策略

**去掉 overview 级缓存**，只使用 per-project 缓存：

- Overview handler 遍历各项目时，先检查该项目的 per-project cache（key: `"{user_id}:{project_id}:kanban:issues:{hash}"`）
- 命中 → 直接复用，不发外部请求
- 未命中 → 发起外部 API 调用，结果写入 per-project cache（TTL 10s）
- 这样避免了两级缓存的一致性问题，且用户刚看过的项目数据可以直接复用

## 三、前端

### 新增页面

- 路由：`/overview`
- 组件：`OverviewKanbanPage`
- Store：`overviewKanbanStore`（独立于 kanbanStore）

### 导航

侧边栏"项目"分组上方新增"总览"入口，图标建议用 Dashboard/Grid 类。

### 组件架构

**不复用 `KanbanBoard`**（它与单项目 KanbanData 结构耦合，含 load-more 等单项目逻辑）。

新建组件：
- `ProjectKanbanSection` — 项目区块容器（header + 三列布局）
- 复用叶子组件：`KanbanColumn`、`IssueCard`、`PrCard`

注意：`IssueCard` 和 `PrCard` 使用 `web_url`（绝对链接），无项目上下文耦合，可直接复用。

### 布局

```
┌─────────────────────────────────────────────────────┐
│  总览 - 运行中项目                    [自动刷新 30s] │
├─────────────────────────────────────────────────────┤
│                                                     │
│  ┌─ my-app (GitLab) ──────────── [查看详情 →] ───┐  │
│  │  Todo(5)    │  In Progress(3)  │  PR/MR(2)    │  │
│  │  ┌──────┐   │  ┌──────┐        │  ┌──────┐    │  │
│  │  │ card │   │  │ card │        │  │ card │    │  │
│  │  └──────┘   │  └──────┘        │  └──────┘    │  │
│  │  ┌──────┐   │  ┌──────┐        │  ┌──────┐    │  │
│  │  │ card │   │  │ card │        │  │ card │    │  │
│  │  └──────┘   │  └──────┘        │  └──────┘    │  │
│  └────────────────────────────────────────────────┘  │
│                                                     │
│  ┌─ another-project (GitHub) ──── [查看详情 →] ──┐  │
│  │  Todo(2)    │  In Progress(1)  │  PR/MR(1)    │  │
│  │  ...        │  ...             │  ...         │  │
│  └────────────────────────────────────────────────┘  │
│                                                     │
│  ┌─ 还有 2 个运行中项目 ──────── [展开更多] ─────┐  │
│  └────────────────────────────────────────────────┘  │
│                                                     │
└─────────────────────────────────────────────────────┘
```

### State Management

**`overviewKanbanStore`** 结构：

```typescript
interface OverviewKanbanStore {
  // 数据
  projectIssues: Map<number, ProjectIssuesData>;  // projectId → issues data
  projectPrs: Map<number, ProjectPrsData>;        // projectId → prs data
  projectMetas: ProjectMeta[];                    // 项目元信息列表

  // 加载状态
  issuesLoading: boolean;
  prsLoading: boolean;
  issuesError: string | null;
  prsError: string | null;

  // 分页
  totalRunningProjects: number;
  hasMore: boolean;

  // Actions
  fetchIssues: () => Promise<void>;
  fetchPrs: () => Promise<void>;
  reset: () => void;  // 页面卸载时清理
}
```

**Store 生命周期**：
- `OverviewKanbanPage` mount → 调用 `fetchIssues()` + `fetchPrs()`
- `OverviewKanbanPage` unmount → 调用 `reset()` 清理数据
- 同时为现有 `kanbanStore` 添加 `reset()` action，在 `KanbanPage` 的 projectId 变化或 unmount 时调用，避免切换项目时闪现旧数据

### 错误处理策略

| 场景 | UI 表现 |
|------|---------|
| `/overview/issues` 整体失败 | 全页错误状态 + 重试按钮 |
| `/overview/prs` 整体失败 | Issues 正常渲染，PR 列显示 inline 错误提示 + 重试按钮（非永久 skeleton） |
| 单个项目 issues 返回 error | 该项目区块显示错误状态 + 跳转到单项目看板的链接 |
| 单个项目 prs 返回 error | 该项目 PR 列显示 "加载失败" 提示 |
| 0 个运行中项目 | Empty state：插图 + "暂无运行中的项目，去项目列表启动一个" + CTA 按钮 |
| 项目有 0 个 issue | 显示项目 header，列内显示 "暂无" 单行提示 |

### 性能控制

- **V1 硬上限**：最多展示 8 个项目，超出显示 "还有 N 个项目" + 展开按钮
- **每项目 todo_limit = 5**：overview 场景只需快速扫一眼，详细内容去单项目看板
- **React key**：`ProjectKanbanSection` 内部各列独立渲染，key 使用 `iid`（同一项目内唯一），不存在跨项目 key 冲突
- **后续优化**：如果项目数持续增长（>15），考虑 accordion 折叠模式或虚拟滚动

### 自动刷新

- 间隔：30s（+ 随机 jitter ±3s，避免多用户同时刷新的 thundering herd）
- **Page Visibility API**：tab 不可见时暂停刷新，重新可见时立即触发一次
- **请求去重**：刷新前检查 `issuesLoading` / `prsLoading`，如果上一次请求仍在 flight 则跳过
- **AbortController**：页面卸载或手动刷新时取消上一次未完成的请求

### 移动端适配

移动端（xs/sm）下 overview 页面简化为项目摘要列表：
- 每个项目一行：项目名 + 各列数量 badge（Todo: 5 | WIP: 3 | PR: 2）
- 点击展开该项目的完整三列看板
- 或直接跳转到单项目看板页

## 四、数据流图

### 4.1 单项目看板（接口拆分后）

```
┌──────────────────────────────────────────────────────────────────────┐
│ Browser (KanbanPage)                                                 │
│                                                                      │
│  mount / refresh                                                     │
│       │                                                              │
│       ├──── GET /api/projects/:id/kanban/issues ────┐                │
│       │                                             │                │
│       └──── GET /api/projects/:id/kanban/prs ───┐   │                │
│                                                 │   │                │
│  ┌─ issues 先返回 ─────────────────────────┐    │   │                │
│  │  渲染 Todo 列 + In Progress 列          │    │   │                │
│  └─────────────────────────────────────────┘    │   │                │
│                                                 │   │                │
│  ┌─ prs 后返回 ───────────────────────────┐    │   │                │
│  │  填充 PR/MR 列（替换 skeleton）         │    │   │                │
│  └─────────────────────────────────────────┘    │   │                │
└──────────────────────────────────────────────────────────────────────┘
                                                  │   │
                                                  ▼   ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Web Platform (Rust)                                                   │
│                                                                      │
│  ┌─ kanban/issues handler ─────────────────────────────────────┐     │
│  │                                                             │     │
│  │  1. Auth + membership check                                 │     │
│  │  2. Check cache → hit? return                               │     │
│  │  3. list_issues(exclude in-progress labels) → Todo          │     │
│  │  4. list_issues(per workflow label × 4) → In Progress       │     │
│  │  5. get_issue_merge_requests(per issue, max 10) → mr_count  │     │
│  │  6. Write cache                                             │     │
│  │                                                             │     │
│  └─────────────────────────────────────────────────────────────┘     │
│                                                                      │
│  ┌─ kanban/prs handler ────────────────────────────────────────┐     │
│  │                                                             │     │
│  │  1. Auth + membership check                                 │     │
│  │  2. Check cache → hit? return                               │     │
│  │  3. list_merge_requests(state=opened) → PR/MR              │     │
│  │  4. Filter pending + sort by updated_at                     │     │
│  │  5. Write cache                                             │     │
│  │                                                             │     │
│  └─────────────────────────────────────────────────────────────┘     │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌──────────────────────────────────────────────────────────────────────┐
│ GitLab / GitHub API                                                   │
│                                                                      │
│  - GET /projects/:id/issues?state=opened&...                         │
│  - GET /projects/:id/issues/:iid/merge_requests                      │
│  - GET /projects/:id/merge_requests?state=opened                     │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### 4.2 全局看板（Overview）

```
┌──────────────────────────────────────────────────────────────────────┐
│ Browser (OverviewKanbanPage)                                         │
│                                                                      │
│  mount / refresh (30s + jitter)                                      │
│       │                                                              │
│       ├──── GET /api/overview/kanban/issues ────┐                    │
│       │                                         │                    │
│       └──── GET /api/overview/kanban/prs ───┐   │                    │
│                                             │   │                    │
│  ┌─ issues 先返回 ─────────────────────┐    │   │                    │
│  │  渲染所有项目的 Todo + InProgress   │    │   │                    │
│  │  PR 列显示 skeleton 占位            │    │   │                    │
│  └─────────────────────────────────────┘    │   │                    │
│                                             │   │                    │
│  ┌─ prs 后返回 ───────────────────────┐    │   │                    │
│  │  填充所有项目的 PR 列              │    │   │                    │
│  │  失败则显示 inline error           │    │   │                    │
│  └─────────────────────────────────────┘    │   │                    │
└──────────────────────────────────────────────────────────────────────┘
                                              │   │
                                              ▼   ▼
┌──────────────────────────────────────────────────────────────────────┐
│ Web Platform (Rust) — Overview Handler                                │
│                                                                      │
│  Rate limit: 5 req/min/user (独立于单项目看板)                        │
│  Endpoint timeout: 15s                                               │
│                                                                      │
│  ┌─ overview/kanban/issues handler ────────────────────────────┐     │
│  │                                                             │     │
│  │  1. Auth check                                              │     │
│  │  2. DB query (SQL JOIN membership):                         │     │
│  │     projects WHERE running AND user is member               │     │
│  │     ORDER BY updated_at DESC LIMIT max_projects             │     │
│  │                                                             │     │
│  │  3. Per-platform token resolution (lazy decrypt):           │     │
│  │     - GitLab projects → decrypt gitlab_token                │     │
│  │     - GitHub projects → decrypt github_token                │     │
│  │     - Missing token → skip, mark error                      │     │
│  │                                                             │     │
│  │  4. 并行获取 (per-platform-host semaphore, max 5):          │     │
│  │     Each project wrapped in tokio::time::timeout(10s)       │     │
│  │                                                             │     │
│  │     ┌─────────┐  ┌─────────┐  ┌─────────┐                  │     │
│  │     │Project A│  │Project B│  │Project C│  ...              │     │
│  │     └────┬────┘  └────┬────┘  └────┬────┘                  │     │
│  │          │            │            │                        │     │
│  │     check per-   check per-   check per-                   │     │
│  │     project      project      project                      │     │
│  │     cache        cache        cache                        │     │
│  │          │            │            │                        │     │
│  │       hit? ──→ use   miss ──→ API call                     │     │
│  │                                                             │     │
│  │  5. 聚合结果（跳过 mr_count，overview 不获取）              │     │
│  │     失败/超时的项目标记 error，不阻塞其他                   │     │
│  │                                                             │     │
│  └─────────────────────────────────────────────────────────────┘     │
│                                                                      │
│  ┌─ overview/kanban/prs handler ───────────────────────────────┐     │
│  │  同上结构：per-platform semaphore + per-project timeout     │     │
│  │  并行获取各项目的 merge_requests                            │     │
│  └─────────────────────────────────────────────────────────────┘     │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
                          │
                          │  per-platform-host semaphore (max 5)
                          │  per-project timeout (10s)
                          ▼
┌──────────────────────────────────────────────────────────────────────┐
│ GitLab / GitHub API (per project)                                     │
│                                                                      │
│  Project A (GitLab @ gitlab.company.com):                            │
│    - GET /projects/:a/issues?state=opened&without_labels=...         │
│    - GET /projects/:a/issues?state=opened&labels=symphony-claimed    │
│    - GET /projects/:a/issues?state=opened&labels=In+Progress         │
│    - GET /projects/:a/issues?state=opened&labels=Merging             │
│    - GET /projects/:a/issues?state=opened&labels=Rework              │
│    (NO get_issue_merge_requests — overview skips mr_count)           │
│                                                                      │
│  Project B (GitHub @ github.com):                                    │
│    - GET /repos/:owner/:repo/issues?state=open&...                   │
│    (NO pulls for issues endpoint — only in prs handler)              │
│                                                                      │
│  Project C (GitLab @ gitlab.company.com):                            │
│    - shares semaphore with Project A (same host)                     │
│    - ...                                                             │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### 4.3 缓存策略（单层 per-project cache）

```
Overview 请求到达
    │
    ▼
┌─ 遍历各项目 ──────────────────────────────────┐
│                                               │
│  For each project:                            │
│       │                                       │
│       ▼                                       │
│  ┌─ Per-Project Cache ─────────────────────┐  │
│  │  Key: "{user_id}:{project_id}:kanban:   │  │
│  │        issues:{hash}"                   │  │
│  │  TTL: 10s                               │  │
│  │                                         │  │
│  │  命中 → 直接复用，不调外部 API          │  │
│  │  未命中 → 继续 ↓                        │  │
│  └─────────────────────────────────────────┘  │
│       │ miss                                  │
│       ▼                                       │
│  ┌─ External API Call ─────────────────────┐  │
│  │  GitLab / GitHub REST API               │  │
│  │  结果写入 per-project cache             │  │
│  └─────────────────────────────────────────┘  │
│                                               │
└───────────────────────────────────────────────┘
    │
    ▼
聚合所有项目结果 → 返回响应
```

优势：
- 无两级缓存一致性问题
- 用户刚看过的单项目看板数据可被 overview 直接复用
- 单项目看板的缓存失效（如创建 MR 后）自动反映到 overview

## 五、实施顺序

| 阶段 | 内容 | 依赖 |
|------|------|------|
| 1 | 后端：拆分 kanban handler 为 issues/prs 两个子 handler | 无 |
| 2 | 前端：现有 KanbanPage 改为调两个接口，验证渲染体验 | 阶段 1 |
| 3 | 后端：新增 overview 聚合接口（含 per-platform semaphore、timeout、限流） | 阶段 1 |
| 4 | 前端：新增 OverviewKanbanPage + 路由 + 侧边栏入口 | 阶段 2, 3 |

## 六、注意事项

- **Rate limit**：
  - GitHub：认证用户 5000 req/hour，overview 跳过 mr_count 后单次请求约 5 calls/project
  - GitLab：默认 300 req/min（自建实例可能不同）
  - Overview 独立限流 5 req/min/user，防止放大攻击
- **项目数量**：硬上限 20，默认 10，V1 前端展示上限 8
- **Token 安全**：per-project 解析，lazy decrypt，SecretString + zeroize
- **超时**：per-project 10s，endpoint 整体 15s
- **前端性能**：todo_limit=5，不获取 mr_count，控制 DOM 节点数
- **移动端**：简化为摘要列表模式
